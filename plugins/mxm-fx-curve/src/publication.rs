//! Fixed-slot ownership handoff for prepared engines.
//!
//! Control code is the only writer/reclaimer. Audio owns the current and transition slots and only
//! changes atomic slot states; it never allocates, locks, or destroys a prepared engine.

use mxm_fx_curve_dsp::CurveEngine;
use std::cell::UnsafeCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

const SLOT_COUNT: usize = 6;
const NONE: usize = usize::MAX;
const FREE: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const AUDIO: u8 = 3;
const RETIRED: u8 = 4;

pub struct PreparedEngine {
    pub engine: CurveEngine,
    pub sample_rate: f32,
    pub revision: u64,
}

struct Slot {
    state: AtomicU8,
    value: UnsafeCell<Option<PreparedEngine>>,
}

// The state machine grants exclusive value access: only control touches WRITING/FREE/RETIRED, and
// only audio touches AUDIO. READY is immutable until audio claims it. Publication uses Release and
// every claimant uses Acquire.
unsafe impl Sync for Slot {}

impl Slot {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(FREE),
            value: UnsafeCell::new(None),
        }
    }
}

pub struct EngineBank {
    slots: [Slot; SLOT_COUNT],
    published: AtomicUsize,
    revision: AtomicU64,
    control: Mutex<()>,
}

impl EngineBank {
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot::new()),
            published: AtomicUsize::new(NONE),
            revision: AtomicU64::new(0),
            control: Mutex::new(()),
        }
    }

    /// Install a fully prepared candidate. This is a control-thread operation.
    pub fn publish(&self, engine: CurveEngine, sample_rate: f32) -> Result<u64, PublishError> {
        let _guard = self
            .control
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.reclaim_retired();

        let index = self
            .slots
            .iter()
            .position(|slot| {
                slot.state
                    .compare_exchange(FREE, WRITING, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            })
            .ok_or(PublishError::Busy)?;
        let revision = self.revision.fetch_add(1, Ordering::Relaxed) + 1;
        // SAFETY: this thread changed this slot from FREE to WRITING while holding the sole writer
        // mutex. Audio cannot claim a slot until its state becomes READY below.
        unsafe {
            *self.slots[index].value.get() = Some(PreparedEngine {
                engine,
                sample_rate,
                revision,
            });
        }
        self.slots[index].state.store(READY, Ordering::Release);

        let superseded = self.published.swap(index, Ordering::AcqRel);
        if superseded != NONE {
            self.slots[superseded]
                .state
                .store(RETIRED, Ordering::Release);
            self.reclaim_retired();
        }
        Ok(revision)
    }

    fn reclaim_retired(&self) {
        for slot in &self.slots {
            if slot
                .state
                .compare_exchange(RETIRED, WRITING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // SAFETY: audio published RETIRED only after its final access, and this control
                // thread now owns WRITING exclusively. Destruction therefore happens off audio.
                unsafe { *slot.value.get() = None };
                slot.state.store(FREE, Ordering::Release);
            }
        }
    }

    fn take_published(&self) -> Option<usize> {
        let index = self.published.swap(NONE, Ordering::AcqRel);
        if index == NONE {
            return None;
        }
        if self.slots[index]
            .state
            .compare_exchange(READY, AUDIO, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(index)
        } else {
            None
        }
    }

    fn retire(&self, index: usize) {
        if index != NONE {
            self.slots[index].state.store(RETIRED, Ordering::Release);
        }
    }

    fn engine(&self, index: usize) -> &PreparedEngine {
        // SAFETY: callers only use a slot they own in AUDIO state. Control never accesses AUDIO.
        unsafe { (*self.slots[index].value.get()).as_ref().unwrap_unchecked() }
    }

    fn with_engine_mut<R>(
        &self,
        index: usize,
        operation: impl FnOnce(&mut PreparedEngine) -> R,
    ) -> R {
        // SAFETY: one AudioConsumer owns each AUDIO slot, and its operations take `&mut self`, so no
        // two mutable accesses to this slot overlap. Control never accesses AUDIO. Keeping the
        // reference inside this closure also prevents it from escaping the ownership operation.
        unsafe { operation((*self.slots[index].value.get()).as_mut().unwrap_unchecked()) }
    }

    fn carry_prefix(&self, target: usize, source: usize) {
        debug_assert_ne!(target, source);
        // SAFETY: both distinct slots are AUDIO-owned by one AudioConsumer. The target is accessed
        // mutably and the source immutably for this call only; control never accesses either.
        unsafe {
            let target = (*self.slots[target].value.get())
                .as_mut()
                .unwrap_unchecked();
            let source = (*self.slots[source].value.get())
                .as_ref()
                .unwrap_unchecked();
            target
                .engine
                .carry_compatible_prefix_history_from(&source.engine);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishError {
    Busy,
}

pub struct AudioConsumer {
    current: usize,
    next: usize,
    transition_position: u32,
    transition_length: u32,
    sample_rate: f32,
    last_revision: u64,
}

impl Default for AudioConsumer {
    fn default() -> Self {
        Self {
            current: NONE,
            next: NONE,
            transition_position: 0,
            transition_length: 1,
            sample_rate: 48_000.0,
            last_revision: 0,
        }
    }
}

impl AudioConsumer {
    pub fn activate(&mut self, bank: &EngineBank, sample_rate: f32) -> bool {
        self.sample_rate = sample_rate;
        let Some(candidate) = bank.take_published() else {
            return self.current != NONE;
        };
        bank.retire(self.current);
        bank.retire(self.next);
        self.current = candidate;
        self.next = NONE;
        self.last_revision = bank.engine(candidate).revision;
        bank.with_engine_mut(candidate, |prepared| prepared.engine.reset());
        true
    }

    pub fn accept_update(&mut self, bank: &EngineBank) {
        // Never rewind an audible blend to its old endpoint. Leave the newest READY candidate in
        // the bank until this transition completes; control may still supersede it meanwhile.
        if self.next != NONE {
            return;
        }
        let Some(candidate) = bank.take_published() else {
            return;
        };
        if bank.engine(candidate).sample_rate.to_bits() != self.sample_rate.to_bits()
            || bank.engine(candidate).revision <= self.last_revision
        {
            bank.retire(candidate);
            return;
        }
        self.last_revision = bank.engine(candidate).revision;
        if self.current == NONE {
            self.current = candidate;
            bank.with_engine_mut(candidate, |prepared| prepared.engine.reset());
            return;
        }
        bank.retire(self.next);
        self.next = candidate;
        bank.carry_prefix(candidate, self.current);
        self.transition_position = 0;
        // Twenty milliseconds is the one C2/C3 model-transition window. The test suite bounds its
        // output discontinuity; C5 program-material audition may lengthen it, never bypass it.
        self.transition_length = (self.sample_rate * 0.020).round().max(1.0) as u32;
    }

    pub fn process(&mut self, bank: &EngineBank, frame: [f32; 2], makeup_amount: f32) -> [f32; 2] {
        if self.current == NONE {
            return frame;
        }
        let old = bank.with_engine_mut(self.current, |prepared| {
            let output = prepared.engine.process(frame, 1.0);
            apply_makeup(output, prepared.engine.nominal_makeup_gain(), makeup_amount)
        });
        if self.next == NONE {
            return old;
        }
        let new = bank.with_engine_mut(self.next, |prepared| {
            let output = prepared.engine.process(frame, 1.0);
            apply_makeup(output, prepared.engine.nominal_makeup_gain(), makeup_amount)
        });
        self.finish_transition(bank, old, new)
    }

    pub fn process_with_stage_levels(
        &mut self,
        bank: &EngineBank,
        frame: [f32; 2],
        makeup_amount: f32,
        levels: &mut [[f32; 2]; mxm_fx_curve_dsp::MAX_STAGES],
    ) -> ([f32; 2], usize) {
        if self.current == NONE {
            return (frame, 0);
        }
        let (old, old_count) = bank.with_engine_mut(self.current, |prepared| {
            let (output, count) = prepared
                .engine
                .process_with_stage_levels(frame, 1.0, levels);
            (
                apply_makeup(output, prepared.engine.nominal_makeup_gain(), makeup_amount),
                count,
            )
        });
        if self.next == NONE {
            return (old, old_count);
        }
        let (new, new_count) = bank.with_engine_mut(self.next, |prepared| {
            let (output, count) = prepared
                .engine
                .process_with_stage_levels(frame, 1.0, levels);
            (
                apply_makeup(output, prepared.engine.nominal_makeup_gain(), makeup_amount),
                count,
            )
        });
        (self.finish_transition(bank, old, new), new_count)
    }

    fn finish_transition(&mut self, bank: &EngineBank, old: [f32; 2], new: [f32; 2]) -> [f32; 2] {
        let amount = self.transition_position as f32 / self.transition_length as f32;
        let output = [
            old[0] + (new[0] - old[0]) * amount,
            old[1] + (new[1] - old[1]) * amount,
        ];
        self.transition_position += 1;
        if self.transition_position >= self.transition_length {
            bank.retire(self.current);
            self.current = self.next;
            self.next = NONE;
        }
        output
    }

    pub fn settle_and_finish(&mut self, bank: &EngineBank) {
        if self.current != NONE {
            bank.with_engine_mut(self.current, |prepared| {
                prepared.engine.settle_to_silence();
            });
        }
        if self.next != NONE {
            bank.with_engine_mut(self.next, |prepared| {
                prepared.engine.settle_to_silence();
            });
            bank.retire(self.current);
            self.current = self.next;
            self.next = NONE;
        }
    }

    pub fn reset(&mut self, bank: &EngineBank) {
        if self.current != NONE {
            bank.with_engine_mut(self.current, |prepared| prepared.engine.reset());
        }
        if self.next != NONE {
            bank.with_engine_mut(self.next, |prepared| prepared.engine.reset());
        }
    }
}

fn apply_makeup(frame: [f32; 2], nominal_gain: f32, amount: f32) -> [f32; 2] {
    let amount = if amount.is_finite() {
        amount.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let gain = 1.0 + (nominal_gain - 1.0) * amount;
    if gain == 1.0 {
        return frame;
    }
    frame.map(|sample| {
        (f64::from(sample) * f64::from(gain)).clamp(
            -(mxm_fx_curve_dsp::MAX_OUTPUT as f64),
            mxm_fx_curve_dsp::MAX_OUTPUT as f64,
        ) as f32
    })
}
