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

    /// The plugin's version, its `Plugin::VERSION`: its factory presets are installed for it.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn parameters(&self) -> Vec<(&'static str, &dyn mxm_params::ErasedParam)> {
        vec![
            ("inputgain", &self.input_gain),
            ("automakeup", &self.auto_makeup),
            ("mix", &self.mix),
        ]
    }

    fn identity(&self) -> &RwLock<PresetIdentity> {
        &self.preset
    }

    /// The installed set, but in a unit test the repository's, never the machine's.
    #[cfg(test)]
    fn factory_files(&self) -> Vec<(String, String)> {
        FACTORY_FILES
            .iter()
            .map(|(name, json)| ((*name).to_owned(), (*json).to_owned()))
            .collect()
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

/// The factory set as it ships, `(name, json)` in its order: the repository's `presets/`, which
/// is installed for the plugin's version and read there (`mxm_preset::factory_dir`). For the
/// tests, the integration tests and the audits; the plugin itself never reads it.
pub static FACTORY_FILES: std::sync::LazyLock<Vec<(&'static str, &'static str)>> =
    std::sync::LazyLock::new(|| {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("presets");
        mxm_preset::read_factory_dir(&folder)
            .into_iter()
            .map(|(file, json)| {
                let name = mxm_preset::Preset::parse(&json, crate::CLAP_ID)
                    .map_or(file, |preset| preset.name);
                (&*String::leak(name), &*String::leak(json))
            })
            .collect()
    });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PointModeState, StageModeState};
    use mxm_params::ErasedParam;
    use mxm_preset::Instrument;

    use nice_plug::params::persist::PersistentField;
    use nice_plug::prelude::ParamSetter;
    use std::collections::{BTreeSet, HashSet};

    use mxm_plugin_test::NoHost;

    /// **Every stored `text` is this plugin's own formatting of the `v` beside it**
    /// (`mxm_preset::stale_factory_texts`).
    #[test]
    fn every_factory_text_is_this_plugins_formatting_of_its_value() {
        let stale = mxm_preset::stale_factory_texts(&MxmFxCurveParams::default());
        assert!(
            stale.is_empty(),
            "{} stale texts — regenerate the files:\n{}",
            stale.len(),
            stale.join("\n")
        );
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
        for (name, json) in FACTORY_FILES.iter() {
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
