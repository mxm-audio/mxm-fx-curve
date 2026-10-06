use super::*;
use crate::model::{CurveStackState, PointState, StageModeState, StageState};
use nice_plug::context::PluginApi;
use nice_plug::context::gui::{GuiContext, GuiContextInner};
use nice_plug::params::internals::ParamPtr;
use nice_plug::params::persist::PersistentField;
use nice_plug::params::{InternalParamMut, Param};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct ModelHost {
    params: Arc<MxmFxCurveParams>,
    dirty: Arc<AtomicBool>,
}

impl GuiContextInner for ModelHost {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
    unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {}
    unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}

    fn get_state(&self) -> PluginState {
        PluginState {
            version: "test".to_owned(),
            params: Default::default(),
            fields: std::collections::BTreeMap::from([(
                "curves".to_owned(),
                nice_plug::params::persist::serialize_field(&self.params.curves.snapshot())
                    .unwrap(),
            )]),
        }
    }

    fn set_state(&self, state: PluginState) {
        assert!(
            state.params.is_empty(),
            "a model edit must not restore or automate ordinary parameters"
        );
        let encoded = state.fields.get("curves").unwrap();
        let model: CurveStackState =
            nice_plug::params::persist::deserialize_field(encoded).unwrap();
        assert_eq!(
            model,
            self.params.curves.snapshot(),
            "the prepared model is committed before the dirty-only host transaction"
        );
        self.dirty.store(true, Ordering::Release);
    }
}

fn model_context(params: Arc<MxmFxCurveParams>) -> (GuiContext, Arc<AtomicBool>) {
    let dirty = Arc::new(AtomicBool::new(false));
    (
        GuiContext::new(Arc::new(ModelHost {
            params,
            dirty: dirty.clone(),
        })),
        dirty,
    )
}

fn prepared(channels: usize) -> MxmFxCurve {
    let mut plugin = MxmFxCurve::default();
    unsafe {
        plugin.params.mix._internal_update_smoother(48_000.0, true);
        plugin
            .params
            .input_gain
            ._internal_update_smoother(48_000.0, true);
    }
    assert!(plugin.prepare_for_test(48_000.0, channels));
    plugin
}

fn set_mix(plugin: &MxmFxCurve, value: f32) {
    unsafe {
        let normalized = plugin.params.mix.preview_normalized(value);
        let _ = plugin.params.mix._internal_set_normalized_value(normalized);
        plugin.params.mix._internal_update_smoother(48_000.0, true);
    }
}

fn set_input_gain(plugin: &MxmFxCurve, value: f32) {
    unsafe {
        let normalized = plugin.params.input_gain.preview_normalized(value);
        let _ = plugin
            .params
            .input_gain
            ._internal_set_normalized_value(normalized);
        plugin
            .params
            .input_gain
            ._internal_update_smoother(48_000.0, true);
    }
}

fn set_auto_makeup(plugin: &MxmFxCurve, enabled: bool) {
    unsafe {
        let _ = plugin.params.auto_makeup._internal_set_plain_value(enabled);
    }
}

fn identity_state() -> CurveStackState {
    CurveStackState::identity()
}

fn zero_state() -> CurveStackState {
    CurveStackState {
        schema_version: crate::model::SCHEMA_VERSION,
        stages: vec![StageState {
            mode: StageModeState::Memoryless,
            points: vec![PointState::curve(0.0, 0.0), PointState::curve(1.0, 0.0)],
        }],
    }
}

#[test]
fn fresh_construction_is_the_unlinked_identity_init() {
    let params = MxmFxCurveParams::default();
    params.curves.map(|state| {
        assert_eq!(state, &CurveStackState::identity());
        assert_eq!(state.stages.len(), 1);
        assert!(matches!(state.stages[0].mode, StageModeState::Memoryless));
        assert_eq!(
            state.stages[0].points,
            [PointState::curve(0.0, 0.0), PointState::curve(1.0, 1.0)]
        );
    });
}

#[test]
fn fresh_instance_is_a_bit_exact_audio_no_op() {
    let mut plugin = prepared(2);
    let original_left: Vec<f32> = (0..257).map(|n| (n as f32 * 0.117).sin() * 0.9).collect();
    let original_right: Vec<f32> = (0..257).map(|n| (n as f32 * 0.071).cos() * 0.8).collect();
    let mut left = original_left.clone();
    let mut right = original_right.clone();
    let mut channels = [&mut left[..], &mut right[..]];
    plugin.process_block_for_test(&mut channels);
    assert_eq!(left, original_left);
    assert_eq!(right, original_right);
}

#[test]
fn editor_preview_changes_audio_without_changing_durable_state_or_history() {
    let mut plugin = prepared(2);
    let params = plugin.params.clone();
    let durable_revision = params.curves.revision();
    let preview = zero_state();

    assert!(params.curves.preview_editor(&preview));
    assert_eq!(params.curves.snapshot(), identity_state());
    assert_eq!(params.curves.revision(), durable_revision);
    assert!(!params.curves.can_undo());

    let mut left = vec![0.5; 2_000];
    let mut right = vec![-0.25; 2_000];
    let mut channels = [&mut left[..], &mut right[..]];
    plugin.process_block_for_test(&mut channels);
    assert!(left[1_500].abs() < 1.0e-6);
    assert!(right[1_500].abs() < 1.0e-6);

    let (context, dirty) = model_context(params.clone());
    assert!(params.curves.commit_editor(&context, preview.clone()));
    assert_eq!(params.curves.snapshot(), preview);
    assert!(params.curves.can_undo());
    assert!(dirty.load(Ordering::Acquire));
}

#[test]
fn mix_zero_is_bit_exact_dry_in_mono_and_stereo() {
    for channel_count in [1, 2] {
        let mut plugin = prepared(channel_count);
        set_mix(&plugin, 0.0);
        let original_l: Vec<f32> = (0..256).map(|n| (n as f32 * 0.13).sin()).collect();
        let original_r: Vec<f32> = (0..256).map(|n| (n as f32 * 0.07).cos()).collect();
        let mut left = original_l.clone();
        let mut right = original_r.clone();
        if channel_count == 1 {
            let mut channels = [&mut left[..]];
            plugin.process_block_for_test(&mut channels);
        } else {
            let mut channels = [&mut left[..], &mut right[..]];
            plugin.process_block_for_test(&mut channels);
        }
        assert_eq!(left, original_l);
        if channel_count == 2 {
            assert_eq!(right, original_r);
        }
    }
}

#[test]
fn input_gain_amplifies_only_the_wet_path_and_mix_zero_remains_exact_dry() {
    let mut plugin = MxmFxCurve::default();
    plugin.params.curves.set(identity_state());
    unsafe {
        plugin.params.mix._internal_update_smoother(48_000.0, true);
        plugin
            .params
            .input_gain
            ._internal_update_smoother(48_000.0, true);
    }
    assert!(plugin.prepare_for_test(48_000.0, 2));
    set_input_gain(&plugin, 4.0);

    let mut left = [0.2; 64];
    let mut right = [-0.1; 64];
    let mut channels = [&mut left[..], &mut right[..]];
    plugin.process_block_for_test(&mut channels);
    assert!((left[0] - 0.8).abs() < 1.0e-6);
    assert!((right[0] + 0.4).abs() < 1.0e-6);

    set_mix(&plugin, 0.0);
    left.fill(0.2);
    right.fill(-0.1);
    let mut channels = [&mut left[..], &mut right[..]];
    plugin.process_block_for_test(&mut channels);
    assert_eq!(left, [0.2; 64]);
    assert_eq!(right, [-0.1; 64]);
}

#[test]
fn auto_makeup_restores_the_curve_stacks_nominal_static_level() {
    let mut plugin = MxmFxCurve::default();
    plugin.params.curves.set(CurveStackState {
        schema_version: crate::model::SCHEMA_VERSION,
        stages: vec![StageState {
            mode: StageModeState::Memoryless,
            points: vec![PointState::curve(0.0, 0.0), PointState::curve(1.0, 0.5)],
        }],
    });
    set_auto_makeup(&plugin, true);
    unsafe {
        plugin.params.mix._internal_update_smoother(48_000.0, true);
        plugin
            .params
            .input_gain
            ._internal_update_smoother(48_000.0, true);
    }
    assert!(plugin.prepare_for_test(48_000.0, 2));
    assert!((plugin.params.curves.nominal_makeup_gain() - 2.0).abs() < 1.0e-3);

    let mut left = [0.25; 64];
    let mut right = [-0.25; 64];
    let mut channels = [&mut left[..], &mut right[..]];
    plugin.process_block_for_test(&mut channels);
    assert!((left[0] - 0.25).abs() < 1.0e-3);
    assert!((right[0] + 0.25).abs() < 1.0e-3);
}

#[test]
fn state_round_trip_preserves_authored_model_and_never_serializes_tables() {
    let state = CurveStackState::identity();
    let json = nice_plug::params::persist::serialize_field(&state).unwrap();
    assert!(!json.contains("table"));
    let restored: CurveStackState = nice_plug::params::persist::deserialize_field(&json).unwrap();
    assert_eq!(restored, state);
}

#[test]
fn format_bounds_reject_point_arrays_before_model_construction() {
    let point = r#"{"x":0.0,"y":0.0,"mode":"curve"}"#;
    let points = std::iter::repeat_n(point, mxm_fx_curve_dsp::MAX_POINTS + 1)
        .collect::<Vec<_>>()
        .join(",");
    let json = format!(
        r#"{{"schema_version":1,"stages":[{{"mode":{{"kind":"memoryless"}},"points":[{points}]}}]}}"#
    );
    assert!(serde_json::from_str::<CurveStackState>(&json).is_err());
}

#[test]
fn the_three_global_controls_are_the_only_host_parameters() {
    let params = MxmFxCurveParams::default();
    let map = params.param_map();
    assert_eq!(map.len(), 3);
    assert_eq!(
        map.iter().map(|(id, _, _)| id.as_str()).collect::<Vec<_>>(),
        ["inputgain", "automakeup", "mix"]
    );
}

#[test]
fn malformed_host_state_is_a_whole_transaction_no_op() {
    let mut state = PluginState {
        version: "0.1.0".to_owned(),
        params: std::collections::BTreeMap::from([(
            "mix".to_owned(),
            nice_plug::plugin::ParamValue::F32(0.25),
        )]),
        fields: std::collections::BTreeMap::from([(
            "curves".to_owned(),
            r#"{"schema_version":99,"stages":[]}"#.to_owned(),
        )]),
    };
    <MxmFxCurve as Plugin>::filter_state(&mut state);
    assert!(state.params.is_empty());
    assert!(state.fields.is_empty());
}

#[test]
fn malformed_or_newer_models_are_rejected_without_replacing_committed_state() {
    let params = MxmFxCurveParams::default();
    let mut invalid = identity_state();
    invalid.schema_version = 2;
    params.curves.set(invalid);
    assert!(params.curves.rejected());
    params
        .curves
        .map(|state| assert_eq!(state, &CurveStackState::identity()));
}

#[test]
fn a_model_edit_uses_one_continuous_transition_path() {
    let mut plugin = prepared(2);
    let mut left = vec![0.9; 2_000];
    let mut right = vec![0.45; 2_000];
    {
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
    }
    plugin.params.curves.set(zero_state());
    let mut left = vec![0.9; 2_000];
    let mut right = vec![0.45; 2_000];
    {
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
    }
    let maximum_step = left
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);
    assert!(maximum_step < 0.01, "transition step was {maximum_step}");
    assert!(left[1_500].abs() < 1.0e-6);
}

#[test]
fn an_older_preparation_that_finishes_late_is_discarded() {
    let params = MxmFxCurveParams::default();
    let older = params.curves.begin_request();
    let newer = params.curves.begin_request();
    let old_state = identity_state();
    let new_state = zero_state();
    let old_engine = old_state.prepare(48_000.0).unwrap();
    let new_engine = new_state.prepare(48_000.0).unwrap();
    assert!(
        !params
            .curves
            .publish_if_latest(older, &old_state, old_engine, 48_000.0)
            .unwrap()
    );
    assert!(
        params
            .curves
            .publish_if_latest(newer, &new_state, new_engine, 48_000.0)
            .unwrap()
    );
    params.curves.map(|state| assert_eq!(state, &new_state));
}

#[test]
fn the_latest_model_wins_when_two_preparations_arrive_before_audio() {
    let mut plugin = prepared(2);
    plugin.params.curves.set(identity_state());
    plugin.params.curves.set(zero_state());
    let mut left = [0.7; 2_048];
    let mut right = [0.3; 2_048];
    let mut channels = [&mut left[..], &mut right[..]];
    plugin.process_block_for_test(&mut channels);
    assert_eq!(left[1_500], 0.0);
    assert_eq!(right[1_500], 0.0);
}

#[test]
fn same_rate_host_state_reactivation_keeps_the_model_transition() {
    let mut plugin = prepared(2);
    plugin.params.curves.set(zero_state());
    let mut left = [0.7; 2_048];
    let mut right = [0.3; 2_048];
    {
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
    }
    plugin.params.curves.set(identity_state());
    assert!(plugin.prepare_for_test(48_000.0, 2));
    left.fill(0.7);
    right.fill(0.3);
    {
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
    }
    assert_eq!(
        left[0], 0.0,
        "reactivation bypassed the old side of the fade"
    );
    assert_eq!(left[1_500], 0.7);
    assert_eq!(right[1_500], 0.3);
}

#[test]
fn callback_update_transition_and_retirement_do_not_touch_the_allocator() {
    let mut plugin = prepared(2);
    plugin.telemetry.connect(true);
    plugin.params.curves.set(identity_state());
    let mut left = [0.7; 2_048];
    let mut right = [-0.3; 2_048];
    let mut channels = [&mut left[..], &mut right[..]];

    let status =
        nice_assert_no_alloc::assert_no_alloc(|| plugin.process_block_for_test(&mut channels));
    assert_eq!(status, ProcessStatus::Normal);
}

#[test]
fn stage_telemetry_runs_only_while_an_editor_is_connected() {
    let mut plugin = prepared(2);
    let mut left = [0.8; 64];
    let mut right = [0.4; 64];
    {
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
    }
    assert_eq!(plugin.telemetry.take(0), None);

    plugin.telemetry.connect(true);
    left.fill(0.8);
    right.fill(0.4);
    {
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
    }
    let (input, output) = plugin.telemetry.take(0).expect("connected stage telemetry");
    assert!(input > 0.0 && output > 0.0);
    assert_eq!(plugin.telemetry.take_input(), [0.8, 0.4]);
    let output = plugin.telemetry.take_output();
    assert!(output[0] > 0.0 && output[1] > 0.0);

    plugin.telemetry.connect(false);
    left.fill(0.8);
    right.fill(0.4);
    {
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
    }
    assert_eq!(plugin.telemetry.take(0), None);
}

#[test]
fn input_meter_is_post_gain_per_channel_and_remains_live_at_mix_zero() {
    let mut plugin = prepared(2);
    set_mix(&plugin, 0.0);
    set_input_gain(&plugin, 2.0);
    plugin.telemetry.connect(true);
    let mut left = [0.6; 64];
    let mut right = [0.3; 64];
    let mut channels = [&mut left[..], &mut right[..]];
    plugin.process_block_for_test(&mut channels);

    let input = plugin.telemetry.take_input();
    assert_eq!(input[0], 1.0);
    assert!((input[1] - 0.6).abs() < 1.0e-6);
    assert_eq!(plugin.telemetry.input_clipped(), [true, false]);
    assert_eq!(plugin.telemetry.take_output(), [0.6, 0.3]);
    assert_eq!(plugin.telemetry.output_clipped(), [false, false]);
    assert_eq!(left, [0.6; 64]);
    assert_eq!(right, [0.3; 64]);
}

#[test]
fn silence_has_no_tail_and_settles_a_pending_transition() {
    let mut plugin = prepared(2);
    plugin.params.curves.set(identity_state());
    let mut left = [0.0; 64];
    let mut right = [0.0; 64];
    let mut channels = [&mut left[..], &mut right[..]];
    assert_eq!(
        plugin.process_block_for_test(&mut channels),
        ProcessStatus::Normal
    );
    assert_eq!(left, [0.0; 64]);
    assert_eq!(right, [0.0; 64]);
}

#[test]
fn browsing_every_factory_model_while_audio_runs_is_safe() {
    let mut plugin = prepared(2);
    for (index, design) in crate::preset_designs::DESIGNS.iter().enumerate() {
        plugin
            .params
            .curves
            .set(crate::preset_designs::state(design));
        assert!(
            plugin.prepare_for_test(48_000.0, 2),
            "reactivation failed for {}",
            design.name
        );
        let mut left = [0.73; 64];
        let mut right = [-0.41; 64];
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
        assert!(
            left.iter().chain(&right).all(|sample| sample.is_finite()),
            "factory model {} ({index}) produced non-finite output",
            design.name
        );
    }
}

#[test]
fn adding_reordering_and_editing_stages_while_audio_runs_is_safe() {
    let mut plugin = prepared(2);
    let mut state = identity_state();
    for iteration in 0..512 {
        if state.stages.len() < mxm_fx_curve_dsp::MAX_STAGES {
            state.stages.push(StageState {
                mode: StageModeState::Memoryless,
                points: vec![PointState::curve(0.0, 0.0), PointState::curve(1.0, 1.0)],
            });
        }
        let last = state.stages.len() - 1;
        state.stages.swap(0, last);
        let stage = &mut state.stages[0];
        stage.points[1].y = 0.2 + 0.8 * ((iteration % 17) as f32 / 16.0);
        plugin.params.curves.set(state.clone());
        assert!(plugin.prepare_for_test(48_000.0, 2));
        plugin.reset();

        let mut left = [0.83; 32];
        let mut right = [-0.37; 32];
        let mut channels = [&mut left[..], &mut right[..]];
        plugin.process_block_for_test(&mut channels);
        assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
    }
}

#[test]
fn repeated_two_stage_editor_commits_are_safe_against_the_audio_thread() {
    let mut plugin = prepared(2);
    let params = plugin.params.clone();
    let (context, dirty) = model_context(params.clone());
    let audio = std::thread::spawn(move || {
        for block in 0..5_000 {
            let level = 0.1 + 0.8 * ((block % 29) as f32 / 28.0);
            let mut left = [level; 32];
            let mut right = [-0.7 * level; 32];
            let mut channels = [&mut left[..], &mut right[..]];
            plugin.process_block_for_test(&mut channels);
            assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        }
    });

    let mut state = CurveStackState {
        schema_version: 1,
        stages: vec![
            StageState {
                mode: StageModeState::Memoryless,
                points: vec![PointState::curve(0.0, 0.0), PointState::curve(1.0, 1.0)],
            },
            StageState {
                mode: StageModeState::Memoryless,
                points: vec![PointState::curve(0.0, 0.0), PointState::curve(1.0, 1.0)],
            },
        ],
    };
    for iteration in 0..2_000 {
        let stage = iteration % 2;
        state.stages[stage].points[1].y = 0.2 + 0.8 * ((iteration % 31) as f32 / 30.0);
        assert!(params.curves.commit_editor(&context, state.clone()));
    }
    audio.join().unwrap();
    assert!(dirty.load(Ordering::Acquire));
}

#[test]
fn repeated_editor_previews_are_safe_against_audio_and_commit_once() {
    let mut plugin = prepared(2);
    let params = plugin.params.clone();
    let (context, dirty) = model_context(params.clone());
    let audio = std::thread::spawn(move || {
        for block in 0..3_000 {
            let level = 0.1 + 0.8 * ((block % 29) as f32 / 28.0);
            let mut left = [level; 32];
            let mut right = [-0.7 * level; 32];
            let mut channels = [&mut left[..], &mut right[..]];
            plugin.process_block_for_test(&mut channels);
            assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        }
    });

    let mut preview = identity_state();
    let mut accepted = 0;
    for iteration in 0..512 {
        preview.stages[0].points[1].y = 0.2 + 0.8 * ((iteration % 31) as f32 / 30.0);
        accepted += usize::from(params.curves.preview_editor(&preview));
    }
    assert!(accepted > 0);
    assert_eq!(params.curves.snapshot(), identity_state());
    assert!(!params.curves.can_undo());
    assert!(!dirty.load(Ordering::Acquire));

    assert!(params.curves.commit_editor(&context, preview.clone()));
    assert_eq!(params.curves.snapshot(), preview);
    assert!(params.curves.can_undo());
    assert!(dirty.load(Ordering::Acquire));
    audio.join().unwrap();
}

#[test]
fn editor_history_survives_editor_objects_and_host_restore_starts_a_new_epoch() {
    let params = Arc::new(MxmFxCurveParams::default());
    let (first_editor_context, dirty) = model_context(params.clone());
    assert!(
        params
            .curves
            .commit_editor(&first_editor_context, zero_state())
    );
    assert!(
        dirty.load(Ordering::Acquire),
        "model edit did not travel through the host-visible state transaction"
    );
    assert!(params.curves.can_undo());

    // A reconstructed editor receives a new context but the history belongs to CurveField.
    drop(first_editor_context);
    let (reopened_context, reopened_dirty) = model_context(params.clone());
    assert!(params.curves.undo(&reopened_context));
    assert!(reopened_dirty.load(Ordering::Acquire));
    assert_eq!(params.curves.snapshot(), CurveStackState::identity());
    assert!(params.curves.redo(&reopened_context));
    assert_eq!(params.curves.snapshot(), zero_state());

    // Changed host and future preset loads do not permit undoing back across a different project epoch.
    params.curves.set(identity_state());
    assert!(!params.curves.can_undo());
    assert!(!params.curves.can_redo());
}

#[test]
fn the_name_id_and_bundle_name_share_one_literal() {
    assert_eq!(NAME, "mxm-fx-curve");
    assert_eq!(CLAP_ID, format!("dk.mxm.{NAME}"));
    mxm_plugin_test::bundle::is_named(env!("CARGO_MANIFEST_DIR"), env!("CARGO_PKG_NAME"), NAME);
}

/// The curve effect fills only the Dynamics page's first slot, with its mix. MXM Player held this
/// test, loading the map as a host does, until the collection was split into one repository per
/// product (2026-10-06); `cargo xtask bundle` holds the map to the standard, and this pins what it
/// maps.
#[test]
fn the_curve_effect_maps_only_mix_to_the_dynamics_page() {
    let map = mxm_control_map::InstrumentMap::parse(include_str!("../control-map.json"))
        .expect("mxm-fx-curve's map parses");
    let instrument = map
        .instruments
        .iter()
        .find(|instrument| instrument.clap_id == CLAP_ID)
        .expect("the map names mxm-fx-curve");
    let standard = mxm_control_map::shipped();
    standard
        .check_instrument(instrument)
        .expect("the map holds to the standard");
    assert_eq!(
        instrument
            .params
            .get("fx.dynamics")
            .map(mxm_control_map::ParamRef::clap_id),
        Some(mxm_control_map::hash_param_id("mix"))
    );
    let page = standard
        .pages
        .iter()
        .find(|page| page.name == "Dynamics")
        .expect("the standard has the Dynamics page");
    assert_eq!(page.section, "Effects");
    assert_eq!(page.slots[0].as_deref(), Some("fx.dynamics"));
    assert!(page.slots[1..].iter().all(Option::is_none));
}
