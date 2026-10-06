//! Factory curve stacks and the shared preset-system seam.

use crate::model::CurveStackState;
use crate::params::MxmFxCurveParams;
use mxm_preset::{Instrument, PresetIdentity};
use std::sync::RwLock;

pub use mxm_preset::{
    Category, Entry, INIT_NAME, Library, Loaded, Origin, Preset, Refused, Value, factory, loaded,
    mark_loaded, mark_none, read_favourites, snapshot, write_favourites,
};

impl Instrument for MxmFxCurveParams {
    fn clap_id(&self) -> &'static str {
        crate::CLAP_ID
    }

    fn parameters(&self) -> Vec<(&'static str, &dyn mxm_preset::ErasedParam)> {
        vec![
            ("inputgain", &self.input_gain),
            ("automakeup", &self.auto_makeup),
            ("mix", &self.mix),
        ]
    }

    fn identity(&self) -> &RwLock<PresetIdentity> {
        &self.preset
    }

    fn factory_files(&self) -> &'static [(&'static str, &'static str)] {
        FACTORY_FILES
    }

    fn init_preset_state(&self) -> Option<serde_json::Value> {
        Some(
            serde_json::to_value(CurveStackState::identity())
                .expect("the built-in Init curve serializes"),
        )
    }

    fn capture_preset_state(&self) -> Option<serde_json::Value> {
        Some(
            serde_json::to_value(self.curves.snapshot())
                .expect("a validated curve stack serializes"),
        )
    }

    fn preset_state_fingerprint(&self) -> Option<u64> {
        Some(self.curves.fingerprint())
    }

    fn validate_preset_state(&self, state: Option<&serde_json::Value>) -> Result<(), String> {
        decode_state(state).and_then(|state| {
            state
                .validate()
                .map_err(|error| format!("invalid curve stack: {error:?}"))
        })
    }

    fn apply_preset_state(&self, state: Option<&serde_json::Value>) -> Result<(), String> {
        let state = decode_state(state)?;
        self.curves
            .stage_preset(state)
            .map_err(|error| format!("could not prepare curve stack: {error:?}"))
    }

    fn commit_preset_state(&self) {
        self.curves.commit_staged_preset();
    }
}

fn decode_state(state: Option<&serde_json::Value>) -> Result<CurveStackState, String> {
    let state = state.ok_or_else(|| "curve preset has no curve stack".to_owned())?;
    serde_json::from_value(state.clone()).map_err(|error| format!("invalid curve stack: {error}"))
}

pub const FACTORY_FILES: &[(&str, &str)] = &[
    (
        "Vocal leveler",
        include_str!("../presets/vocal-leveler.json"),
    ),
    ("Bus glue", include_str!("../presets/bus-glue.json")),
    (
        "Punch compressor",
        include_str!("../presets/punch-compressor.json"),
    ),
    (
        "Slow leveller",
        include_str!("../presets/slow-leveller.json"),
    ),
    (
        "Fast peak tamer",
        include_str!("../presets/fast-peak-tamer.json"),
    ),
    ("Soft limiter", include_str!("../presets/soft-limiter.json")),
    (
        "Brickwall limiter",
        include_str!("../presets/brickwall-limiter.json"),
    ),
    (
        "Transient clamp",
        include_str!("../presets/transient-clamp.json"),
    ),
    (
        "Pumping compressor",
        include_str!("../presets/pumping-compressor.json"),
    ),
    (
        "Parallel squeeze",
        include_str!("../presets/parallel-squeeze.json"),
    ),
    ("Upward lift", include_str!("../presets/upward-lift.json")),
    (
        "Low level detail",
        include_str!("../presets/low-level-detail.json"),
    ),
    (
        "Gentle expander",
        include_str!("../presets/gentle-expander.json"),
    ),
    (
        "Downward expander",
        include_str!("../presets/downward-expander.json"),
    ),
    ("Noise gate", include_str!("../presets/noise-gate.json")),
    ("Hard gate", include_str!("../presets/hard-gate.json")),
    ("Drum gate", include_str!("../presets/drum-gate.json")),
    ("Breath gate", include_str!("../presets/breath-gate.json")),
    (
        "Floor suppressor",
        include_str!("../presets/floor-suppressor.json"),
    ),
    ("Crest rider", include_str!("../presets/crest-rider.json")),
    (
        "Soft saturation",
        include_str!("../presets/soft-saturation.json"),
    ),
    (
        "Rounded drive",
        include_str!("../presets/rounded-drive.json"),
    ),
    ("Hard clip", include_str!("../presets/hard-clip.json")),
    ("Dense clip", include_str!("../presets/dense-clip.json")),
    (
        "Gentle overdrive",
        include_str!("../presets/gentle-overdrive.json"),
    ),
    (
        "Hot overdrive",
        include_str!("../presets/hot-overdrive.json"),
    ),
    ("Fuzz", include_str!("../presets/fuzz.json")),
    ("Square maker", include_str!("../presets/square-maker.json")),
    ("Dead zone", include_str!("../presets/dead-zone.json")),
    (
        "Crossover crunch",
        include_str!("../presets/crossover-crunch.json"),
    ),
    ("Single fold", include_str!("../presets/single-fold.json")),
    ("Double fold", include_str!("../presets/double-fold.json")),
    ("Triple fold", include_str!("../presets/triple-fold.json")),
    (
        "Triangle shaper",
        include_str!("../presets/triangle-shaper.json"),
    ),
    ("S curve", include_str!("../presets/s-curve.json")),
    (
        "Concave boost",
        include_str!("../presets/concave-boost.json"),
    ),
    ("Convex cut", include_str!("../presets/convex-cut.json")),
    (
        "Four step quantizer",
        include_str!("../presets/four-step-quantizer.json"),
    ),
    (
        "Eight step quantizer",
        include_str!("../presets/eight-step-quantizer.json"),
    ),
    ("Comparator", include_str!("../presets/comparator.json")),
    (
        "Staircase drive",
        include_str!("../presets/staircase-drive.json"),
    ),
    ("Broken cone", include_str!("../presets/broken-cone.json")),
    ("Needle clip", include_str!("../presets/needle-clip.json")),
    ("Hollow clip", include_str!("../presets/hollow-clip.json")),
    ("Range crush", include_str!("../presets/range-crush.json")),
    (
        "Compressor into saturation",
        include_str!("../presets/compressor-into-saturation.json"),
    ),
    (
        "Saturation into compressor",
        include_str!("../presets/saturation-into-compressor.json"),
    ),
    (
        "Gate into drive",
        include_str!("../presets/gate-into-drive.json"),
    ),
    (
        "Drive into gate",
        include_str!("../presets/drive-into-gate.json"),
    ),
    (
        "Expander into limiter",
        include_str!("../presets/expander-into-limiter.json"),
    ),
    (
        "Two stage limiter",
        include_str!("../presets/two-stage-limiter.json"),
    ),
    (
        "Soft then hard",
        include_str!("../presets/soft-then-hard.json"),
    ),
    (
        "Fold then clip",
        include_str!("../presets/fold-then-clip.json"),
    ),
    (
        "Clip then fold",
        include_str!("../presets/clip-then-fold.json"),
    ),
    (
        "Detail then glue",
        include_str!("../presets/detail-then-glue.json"),
    ),
    (
        "Pump and crunch",
        include_str!("../presets/pump-and-crunch.json"),
    ),
    (
        "Gate comp clip",
        include_str!("../presets/gate-comp-clip.json"),
    ),
    (
        "Five stage crusher",
        include_str!("../presets/five-stage-crusher.json"),
    ),
    ("Unity curve", include_str!("../presets/unity-curve.json")),
    (
        "Manual bezier",
        include_str!("../presets/manual-bezier.json"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PointModeState, StageModeState};
    use mxm_preset::{ErasedParam, Instrument};
    use nice_plug::params::internals::ParamPtr;
    use nice_plug::params::persist::PersistentField;
    use nice_plug::prelude::{ParamSetter, PluginApi, PluginState};
    use std::collections::{BTreeSet, HashSet};

    struct NoHost;

    impl nice_plug::context::gui::GuiContextInner for NoHost {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {}
        unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            PluginState {
                version: String::new(),
                params: Default::default(),
                fields: Default::default(),
            }
        }
        fn set_state(&self, _state: PluginState) {}
    }

    #[test]
    fn generated_init_contains_parameter_defaults_and_the_default_curve() {
        let params = MxmFxCurveParams::default();
        let init = Preset::init(&params);
        assert_eq!(init.name, INIT_NAME);
        assert_eq!(init.params.len(), 3);
        assert_eq!(
            init.params["inputgain"].v,
            params.input_gain.default_normalised()
        );
        assert_eq!(
            init.params["automakeup"].v,
            params.auto_makeup.default_normalised()
        );
        assert_eq!(init.params["mix"].v, params.mix.default_normalised());
        let state: CurveStackState = serde_json::from_value(init.state.unwrap()).unwrap();
        assert_eq!(state, CurveStackState::identity());
    }

    #[test]
    fn factory_files_match_the_reviewable_design_table() {
        let generated = crate::preset_designs::generate();
        assert_eq!(generated.len(), FACTORY_FILES.len());
        let shipped: std::collections::HashMap<_, _> = FACTORY_FILES.iter().copied().collect();
        for (slug, json) in generated {
            let name = crate::preset_designs::DESIGNS
                .iter()
                .find(|design| crate::preset_designs::slug(design.name) == slug)
                .unwrap()
                .name;
            assert_eq!(
                shipped[name].replace("\r\n", "\n"),
                json,
                "{slug}.json is stale"
            );
        }
    }

    #[test]
    fn bank_has_sixty_valid_distinct_complete_models_covering_the_feature_set() {
        assert_eq!(crate::preset_designs::DESIGNS.len(), 60);
        assert_eq!(FACTORY_FILES.len(), 60);
        let params = MxmFxCurveParams::default();
        assert_eq!(
            factory(&params).len(),
            61,
            "sixty sounds plus generated Init"
        );

        let mut names = BTreeSet::new();
        let mut fingerprints = HashSet::new();
        let mut detector = false;
        let mut memoryless = false;
        let mut serial = false;
        let mut discontinuous = false;
        let mut handles = false;
        for (name, json) in FACTORY_FILES {
            let preset = Preset::parse(json, crate::CLAP_ID).expect(name);
            assert!(names.insert(preset.name.clone()), "duplicate name {name}");
            assert_ne!(preset.category, Category::Uncategorised, "{name}");
            assert_eq!(preset.params.len(), 3, "{name}");
            for id in ["inputgain", "automakeup", "mix"] {
                assert!(preset.params.contains_key(id), "{name}: missing {id}");
            }
            let state: CurveStackState = serde_json::from_value(preset.state.unwrap()).unwrap();
            state.validate().unwrap();
            state.prepare(48_000.0).unwrap();
            assert!(
                fingerprints.insert(state.fingerprint()),
                "duplicate model {name}"
            );
            serial |= state.stages.len() > 1;
            for stage in &state.stages {
                detector |= matches!(stage.mode, StageModeState::Detector { .. });
                memoryless |= matches!(stage.mode, StageModeState::Memoryless);
                discontinuous |= stage.points.windows(2).any(|pair| pair[0].x == pair[1].x);
                handles |= stage
                    .points
                    .iter()
                    .any(|point| point.mode == PointModeState::Handles);
            }
        }
        assert!(detector && memoryless && serial && discontinuous && handles);
    }

    #[test]
    fn init_from_the_shared_preset_surface_restores_the_complete_default_model() {
        let params = MxmFxCurveParams::default();
        params.curves.set(crate::preset_designs::state(
            &crate::preset_designs::DESIGNS[30],
        ));
        assert_ne!(params.curves.snapshot(), CurveStackState::identity());
        mxm_preset::ui::init_patch(&params, &ParamSetter::new(&NoHost));
        assert_eq!(params.curves.snapshot(), CurveStackState::identity());
    }

    #[test]
    fn preset_model_prepares_before_global_parameters_and_publishes_only_on_commit() {
        let params = MxmFxCurveParams::default();
        let before = params.curves.snapshot();
        let target = crate::preset_designs::state(&crate::preset_designs::DESIGNS[30]);
        let value = serde_json::to_value(&target).unwrap();
        params.validate_preset_state(Some(&value)).unwrap();
        params.apply_preset_state(Some(&value)).unwrap();
        assert_eq!(
            params.curves.snapshot(),
            before,
            "preset published before gestures"
        );
        params.commit_preset_state();
        assert_eq!(params.curves.snapshot(), target);
    }

    #[test]
    fn curve_only_edits_mark_a_loaded_preset_modified_and_can_return_clean() {
        let params = MxmFxCurveParams::default();
        mark_loaded(&params, "Init-like", Origin::Factory);
        assert!(matches!(loaded(&params), Loaded::Clean { .. }));
        assert!(
            nice_assert_no_alloc::assert_no_alloc(|| params.preset_state_fingerprint()).is_some()
        );
        let original = params.curves.snapshot();
        params.curves.set(crate::preset_designs::state(
            &crate::preset_designs::DESIGNS[20],
        ));
        assert!(matches!(loaded(&params), Loaded::Modified { .. }));
        params.curves.set(original);
        assert!(matches!(loaded(&params), Loaded::Clean { .. }));
    }
}
