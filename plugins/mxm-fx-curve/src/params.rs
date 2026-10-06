//! Three automatable global controls and one durable, non-parameter curve-stack field.

use crate::model::CurveStackState;
use crate::publication::EngineBank;
use mxm_preset::PresetIdentity;
use nice_plug::context::gui::GuiContext;
use nice_plug::params::persist::PersistentField;
use nice_plug::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

const HISTORY_LIMIT: usize = 64;

#[derive(Clone, Default)]
struct ModelHistory {
    undo: Vec<CurveStackState>,
    redo: Vec<CurveStackState>,
}

#[derive(Clone, Copy)]
enum HistoryAction {
    Record,
    Undo,
    Redo,
}

#[derive(Params)]
pub struct MxmFxCurveParams {
    #[id = "inputgain"]
    pub input_gain: FloatParam,

    #[id = "automakeup"]
    pub auto_makeup: BoolParam,

    #[id = "mix"]
    pub mix: FloatParam,

    #[persist = "curves"]
    pub curves: CurveField,

    #[persist = "preset"]
    pub preset: RwLock<PresetIdentity>,
}

impl Default for MxmFxCurveParams {
    fn default() -> Self {
        let curves = CurveField::default();
        // Fresh construction and Init are the same committed, prepared no-op identity state.
        curves
            .prepare_for_rate(48_000.0)
            .expect("the built-in Init curve is valid");
        Self {
            input_gain: FloatParam::new(
                "Input gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-24.0),
                    max: util::db_to_gain(24.0),
                    factor: FloatRange::gain_skew_factor(-24.0, 24.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            auto_makeup: BoolParam::new("Auto makeup", false),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(Arc::new(|value| format!("{:.0} %", value * 100.0)))
                .with_string_to_value(Arc::new(|text| {
                    text.trim()
                        .trim_end_matches('%')
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .map(|value| value / 100.0)
                })),
            curves,
            preset: RwLock::new(PresetIdentity::none()),
        }
    }
}

struct StagedPreset {
    request: u64,
    state: CurveStackState,
    engine: mxm_fx_curve_dsp::CurveEngine,
    sample_rate: f32,
}

pub struct CurveField {
    committed: RwLock<CurveStackState>,
    bank: Arc<EngineBank>,
    sample_rate_bits: AtomicU32,
    latest_request: AtomicU64,
    commit: Mutex<()>,
    rejected: AtomicBool,
    revision: AtomicU64,
    nominal_makeup_bits: AtomicU32,
    history: Mutex<ModelHistory>,
    pending_history: Mutex<Option<HistoryAction>>,
    staged_preset: Mutex<Option<StagedPreset>>,
}

impl Default for CurveField {
    fn default() -> Self {
        Self {
            committed: RwLock::new(CurveStackState::default()),
            bank: Arc::new(EngineBank::new()),
            sample_rate_bits: AtomicU32::new(48_000.0f32.to_bits()),
            latest_request: AtomicU64::new(0),
            commit: Mutex::new(()),
            rejected: AtomicBool::new(false),
            revision: AtomicU64::new(0),
            nominal_makeup_bits: AtomicU32::new(1.0_f32.to_bits()),
            history: Mutex::new(ModelHistory::default()),
            pending_history: Mutex::new(None),
            staged_preset: Mutex::new(None),
        }
    }
}

impl CurveField {
    pub fn bank(&self) -> Arc<EngineBank> {
        self.bank.clone()
    }

    pub fn prepare_for_rate(&self, sample_rate: f32) -> Result<u64, PrepareError> {
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Release);
        let request = self.begin_request();
        let state = self
            .committed
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let engine = state
            .prepare(sample_rate)
            .map_err(|_| PrepareError::InvalidModel)?;
        if self.publish_if_latest(request, &state, engine, sample_rate)? {
            Ok(request)
        } else {
            Err(PrepareError::Obsolete)
        }
    }

    pub(crate) fn begin_request(&self) -> u64 {
        self.staged_preset
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        self.latest_request.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Prepare a complete preset model before any parameter gesture is emitted. Publication waits
    /// for `commit_staged_preset`, after the preset system has written the global parameters.
    pub(crate) fn stage_preset(&self, state: CurveStackState) -> Result<(), PrepareError> {
        let request = self.begin_request();
        if state == self.snapshot() {
            // An unchanged preset restore is not a model boundary and must not clear undo history.
            return Ok(());
        }
        let sample_rate = f32::from_bits(self.sample_rate_bits.load(Ordering::Acquire));
        let engine = state
            .prepare(sample_rate)
            .map_err(|_| PrepareError::InvalidModel)?;
        if self.latest_request.load(Ordering::Acquire) != request {
            return Err(PrepareError::Obsolete);
        }
        *self
            .staged_preset
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(StagedPreset {
            request,
            state,
            engine,
            sample_rate,
        });
        Ok(())
    }

    /// Publish the model prepared by `stage_preset`. The fixed-slot bank has six slots for a
    /// two-engine transition and superseded candidates, so this commit is expected to be infallible
    /// after staging; a defensive failure leaves the prior model committed and marks the field
    /// rejected for host-state rollback.
    pub(crate) fn commit_staged_preset(&self) {
        let staged = self
            .staged_preset
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let Some(staged) = staged else {
            return;
        };
        let old = self.snapshot();
        match self.publish_if_latest(
            staged.request,
            &staged.state,
            staged.engine,
            staged.sample_rate,
        ) {
            Ok(true) => {
                self.update_history(old, &staged.state);
                self.rejected.store(false, Ordering::Release);
            }
            Ok(false) | Err(_) => self.rejected.store(true, Ordering::Release),
        }
    }

    pub(crate) fn publish_if_latest(
        &self,
        request: u64,
        state: &CurveStackState,
        engine: mxm_fx_curve_dsp::CurveEngine,
        sample_rate: f32,
    ) -> Result<bool, PrepareError> {
        // Preparation is intentionally outside this control-only mutex. The short commit section
        // orders the final obsolescence check, slot publication and durable-state update together.
        let _commit = self
            .commit
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.latest_request.load(Ordering::Acquire) != request {
            return Ok(false);
        }
        let nominal_makeup_gain = engine.nominal_makeup_gain();
        self.bank
            .publish(engine, sample_rate)
            .map_err(|_| PrepareError::PublicationBusy)?;
        self.nominal_makeup_bits
            .store(nominal_makeup_gain.to_bits(), Ordering::Release);
        let mut committed = self
            .committed
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if *committed != *state {
            *committed = state.clone();
            self.revision.fetch_add(1, Ordering::Release);
        }
        Ok(true)
    }

    pub fn rejected(&self) -> bool {
        self.rejected.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> CurveStackState {
        self.committed
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn fingerprint(&self) -> u64 {
        self.committed
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .fingerprint()
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn nominal_makeup_gain(&self) -> f32 {
        f32::from_bits(self.nominal_makeup_bits.load(Ordering::Acquire))
    }

    /// Publish one complete drag frame for audition without changing durable state, history,
    /// fingerprint or host dirty state. Preparation and reclamation remain on the GUI thread; audio
    /// receives the result through the same fixed-slot handoff and 20 ms transition as a commit.
    pub fn preview_editor(&self, value: &CurveStackState) -> bool {
        let request = self.begin_request();
        let sample_rate = f32::from_bits(self.sample_rate_bits.load(Ordering::Acquire));
        let Ok(engine) = value.prepare(sample_rate) else {
            return false;
        };
        let _commit = self
            .commit
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.latest_request.load(Ordering::Acquire) != request {
            return false;
        }
        let nominal_makeup_gain = engine.nominal_makeup_gain();
        if self.bank.publish(engine, sample_rate).is_err() {
            return false;
        }
        self.nominal_makeup_bits
            .store(nominal_makeup_gain.to_bits(), Ordering::Release);
        true
    }

    /// Replace an abandoned preview with the durable model without creating history or dirty state.
    pub fn restore_committed_audio(&self) -> bool {
        self.preview_editor(&self.snapshot())
    }

    pub fn can_undo(&self) -> bool {
        !self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .undo
            .is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .redo
            .is_empty()
    }

    /// Prepare and publish one complete editor gesture through the field's bounded control-side
    /// path, then use `GuiContext::set_state()` only to mark the already-committed non-parameter
    /// state dirty. Host restore remains a separate rollback-capable wrapper transaction.
    pub fn commit_editor(&self, context: &GuiContext, new_value: CurveStackState) -> bool {
        self.commit_editor_action(context, new_value, HistoryAction::Record)
    }

    pub fn undo(&self, context: &GuiContext) -> bool {
        let target = self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .undo
            .last()
            .cloned();
        target.is_some_and(|target| self.commit_editor_action(context, target, HistoryAction::Undo))
    }

    pub fn redo(&self, context: &GuiContext) -> bool {
        let target = self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .redo
            .last()
            .cloned();
        target.is_some_and(|target| self.commit_editor_action(context, target, HistoryAction::Redo))
    }

    fn commit_editor_action(
        &self,
        context: &GuiContext,
        new_value: CurveStackState,
        action: HistoryAction,
    ) -> bool {
        if new_value.validate().is_err() || new_value == self.snapshot() {
            return false;
        }
        let history_before = self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        *self
            .pending_history
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(action);

        // Prepare and publish before notifying the host. Sending the complete model through
        // `GuiContext::set_state()` made the wrapper hold its plugin mutex while constructing every
        // lookup table. A playing audio callback contending for that mutex can allocate inside
        // parking_lot and, worse, wait for the whole preparation. The persistent field already owns
        // the bounded control-side publication transaction, so use it directly.
        self.set(new_value.clone());
        let committed = self.snapshot() == new_value;
        if !committed {
            self.pending_history
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            *self
                .history
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = history_before;
            return false;
        }

        // The model is now both durable and published. This deliberately carries no parameter
        // values: nice-plug recognizes current fields plus an empty parameter map as a dirty-only
        // GUI transaction, calls CLAP host.state.mark_dirty(), and never takes the processor lock.
        let mut state = context.get_state();
        state.params.clear();
        context.set_state(state);
        true
    }

    fn update_history(&self, old: CurveStackState, new: &CurveStackState) {
        let action = self
            .pending_history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let mut history = self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match action {
            Some(HistoryAction::Record) => {
                push_bounded(&mut history.undo, old);
                history.redo.clear();
            }
            Some(HistoryAction::Undo) => {
                let _ = history.undo.pop();
                push_bounded(&mut history.redo, old);
            }
            Some(HistoryAction::Redo) => {
                let _ = history.redo.pop();
                push_bounded(&mut history.undo, old);
            }
            None => {
                // Preset and host restores start a new history epoch. The history itself belongs to
                // the plugin, so editor close/reopen does not clear it.
                history.undo.clear();
                history.redo.clear();
            }
        }
        debug_assert_eq!(self.snapshot(), *new);
    }
}

fn push_bounded(stack: &mut Vec<CurveStackState>, value: CurveStackState) {
    if stack.len() == HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(value);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareError {
    InvalidModel,
    PublicationBusy,
    Obsolete,
}

impl<'a> PersistentField<'a, CurveStackState> for CurveField {
    fn set(&self, new_value: CurveStackState) {
        let request = self.begin_request();
        let old_value = {
            // Registering the request happened before taking this lock. A publisher that had
            // already passed its final check may finish first; compare against the state it
            // actually committed rather than a snapshot taken while that commit was in flight.
            let _commit = self
                .commit
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.latest_request.load(Ordering::Acquire) != request {
                return;
            }
            let old_value = self
                .committed
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            if old_value == new_value {
                self.pending_history
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                // An unchanged host restore has no model boundary to cross. In particular, this is
                // also the shape of the wrapper restoring its snapshot after a rejected transaction,
                // which must not erase a history the failed request never changed.
                self.rejected.store(false, Ordering::Release);
                return;
            }
            old_value
        };
        // Format bounds and model invariants are checked before prepared lookup tables are
        // constructed. This function never runs in process().
        let sample_rate = f32::from_bits(self.sample_rate_bits.load(Ordering::Acquire));
        let Ok(engine) = new_value.prepare(sample_rate) else {
            let _commit = self
                .commit
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.latest_request.load(Ordering::Acquire) == request {
                self.pending_history
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                self.rejected.store(true, Ordering::Release);
            }
            return;
        };
        match self.publish_if_latest(request, &new_value, engine, sample_rate) {
            Ok(true) => {
                self.update_history(old_value, &new_value);
                self.rejected.store(false, Ordering::Release);
            }
            Ok(false) => {
                self.pending_history
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
            }
            Err(_) => {
                self.pending_history
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                self.rejected.store(true, Ordering::Release);
            }
        }
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&CurveStackState) -> R,
    {
        f(&self
            .committed
            .read()
            .unwrap_or_else(|error| error.into_inner()))
    }
}
