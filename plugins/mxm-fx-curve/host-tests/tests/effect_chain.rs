//! `mxm-fx-curve` through MXM Player's ordinary discovery, its parameter and state paths and its
//! serial effect chain, against the release bundle.
//!
//! Product DSP stays in its own crate. Init is an exact no-op (the owner's exception to an effect
//! opening engaged), so what is heard is set the way a host restores a saved one, through the CLAP
//! state. These hold the shipping boundary: the bundle is classified as an effect, its control map
//! loads and names a parameter the bundle has; the player reads every parameter at its default; a
//! changed Input gain saved through the player opens again in a fresh instance; inserted at Init
//! it changes nothing, to the bit; a restored Input gain after `mxm-mono-01` is finite and heard,
//! and adds nothing before its input; and the player's bypass restores the dry render to the bit.

use mxm_player_harness::app_harness;
use mxm_player_harness::harness;

use clack_extensions::state::PluginState;
use clack_host::prelude::*;
use harness::{CHANNELS, Harness};
use mxm_player::engine::Engine;
use mxm_player::events::input::Payload;
use mxm_player::host::MxmHost;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

const PLUGIN: &str = "mxm-fx-curve";
const SOURCE_ID: &str = "dk.mxm.mxm-mono-01";
const EFFECT_ID: &str = "dk.mxm.mxm-fx-curve";
const BLOCK: usize = 256;
// The note and its release; the curve is memoryless, with no tail of its own.
const BLOCKS: usize = 700;
/// Input gain, Auto makeup and Mix (`src/params.rs`); the curve stack is state, not parameters.
const PARAMETERS: usize = 3;
/// The control map's roles, each with the parameter it names, by the name the host shows.
const ROLES: &[(&str, &str)] = &[("fx.dynamics", "Mix")];
/// Input gain as nice-plug stores it, a plain gain: +6 dB into the 1:1 line.
const GAIN: f32 = 2.0;

/// This plugin's release bundle, or `None` when it has not been built: **the one skip**.
fn bundle() -> Option<PathBuf> {
    let found = mxm_player_harness::host_test::plugin_bundle(PLUGIN).map(|(_, file)| file);
    if found.is_none() {
        eprintln!("skipping: run `cargo xtask bundle {PLUGIN} --release`");
    }
    found
}

/// The source the chain plays, fetched beside the bundle (`test-bundles.txt`). Required once the
/// bundle exists, so a fetch that failed fails the tests rather than passing them unrun.
fn source() -> PathBuf {
    let path = mxm_player_harness::workspace_root().join("target/bundled/mxm-mono-01.clap");
    assert!(
        path.exists(),
        "{} is missing: run `cargo xtask fetch`",
        path.display()
    );
    path
}

/// The effect alone in a player engine, as the chain's Add puts it there.
fn engine(bundle: &Path) -> Engine {
    let mut engine = Engine::new();
    engine.add_fx(bundle, EFFECT_ID).expect("the effect loads");
    engine
}

#[test]
fn ordinary_discovery_classifies_the_bundle_and_its_map_names_real_parameters() {
    let Some(bundle) = bundle() else {
        return;
    };
    let directory = bundle.parent().unwrap().to_owned();
    let mut app = app_harness::AppHarness::new("fx-curve-discovery", vec![directory]);
    app.harness.state_mut().rescan();
    app.run();
    let state = app.state();
    let found = state
        .found
        .iter()
        .find(|found| found.id == EFFECT_ID)
        .unwrap_or_else(|| panic!("ordinary discovery did not find {EFFECT_ID}"));
    assert!(
        found.effect,
        "the one-input bundle was not classified as an effect"
    );
    assert!(
        app.app().control_map().knows_instrument(EFFECT_ID),
        "the control-map sidecar was not loaded"
    );

    let params = engine(&bundle).read_fx_params(0).expect("an effect");
    for (role, name) in ROLES {
        let parameter = params
            .params
            .iter()
            .find(|parameter| parameter.name == *name)
            .unwrap_or_else(|| panic!("no parameter called {name}"));
        assert_eq!(
            app.app().control_map().param_for(EFFECT_ID, role),
            Some(parameter.id),
            "{role}"
        );
    }
}

#[test]
fn the_player_reads_every_parameter_at_its_default() {
    let Some(bundle) = bundle() else {
        return;
    };
    let params = engine(&bundle).read_fx_params(0).expect("an effect");
    assert_eq!(params.params.len(), PARAMETERS);
    for parameter in &params.params {
        assert_eq!(parameter.value, parameter.default, "{}", parameter.name);
        assert!(!parameter.text.is_empty(), "{}", parameter.name);
        assert!(
            !parameter.is_hidden && !parameter.is_read_only,
            "{}",
            parameter.name
        );
    }
}

/// The nice-plug state a host saves: its length, then uncompressed JSON.
fn decode(bytes: &[u8]) -> Value {
    let size = u64::from_le_bytes(bytes[..8].try_into().expect("CLAP state length prefix"));
    assert_eq!(size as usize, bytes.len() - 8);
    serde_json::from_slice(&bytes[8..]).expect("uncompressed nice-plug state")
}

fn encode(value: &Value) -> Vec<u8> {
    let json = serde_json::to_vec(value).expect("state JSON");
    let mut bytes = Vec::with_capacity(json.len() + 8);
    bytes.extend_from_slice(&(json.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&json);
    bytes
}

/// A saved state with Input gain raised to [`GAIN`]. Init is checked first, so a changed state
/// format or default fails here by name.
fn with_gain(mut state: Value) -> Value {
    let params = state["params"].as_object_mut().expect("parameter map");
    assert_eq!(params.len(), PARAMETERS);
    assert_eq!(
        params["inputgain"],
        json!({"f32": 1.0}),
        "Input gain is not unity at Init"
    );
    params.insert("inputgain".into(), json!({"f32": GAIN}));
    state
}

/// The parameter's text, by the name the host shows.
fn text(engine: &mut Engine, name: &str) -> String {
    engine
        .read_fx_params(0)
        .expect("an effect")
        .params
        .iter()
        .find(|parameter| parameter.name == name)
        .unwrap_or_else(|| panic!("no parameter called {name}"))
        .text
        .clone()
}

/// **What the player saves, it opens.** The state it captures for a project, with Input gain
/// changed in it, loads into a fresh instance, which shows the change and saves it again unchanged.
#[test]
fn a_changed_parameter_saves_and_opens_again_through_the_player() {
    let Some(bundle) = bundle() else {
        return;
    };
    let mut saved = engine(&bundle);
    let state = with_gain(decode(&saved.capture_fx_state(0).expect("the state saves")));

    let mut opened = engine(&bundle);
    opened
        .load_fx_state(0, &encode(&state))
        .expect("the state opens");
    assert_eq!(text(&mut opened, "Input gain"), "6.0 dB");
    let again = decode(&opened.capture_fx_state(0).expect("the state saves again"));
    assert_eq!(again["params"], state["params"]);
}

/// Restores the raised Input gain into the chain's own instance, before it processes, as a host
/// opening a project does.
fn restore_gain(instance: &mut PluginInstance<MxmHost>) {
    let state: PluginState = instance
        .plugin_shared_handle()
        .get_extension()
        .expect("state extension");
    let mut bytes = Vec::new();
    state
        .save(&mut instance.plugin_handle(), &mut bytes)
        .expect("state saves");
    let restored = encode(&with_gain(decode(&bytes)));
    state
        .load(&mut instance.plugin_handle(), &mut &restored[..])
        .expect("state loads");
}

/// What the chain holds for the render.
#[derive(Clone, Copy, PartialEq)]
enum Chain {
    /// The source alone.
    Dry,
    /// The effect as inserted, at Init.
    Init,
    /// The effect with Input gain restored to [`GAIN`].
    Gain,
}

fn render(chain: Chain, bypassed: bool) -> Option<Vec<f32>> {
    let effect = bundle()?;
    let source = source();
    let effects: Vec<(&Path, &str)> = if chain == Chain::Dry {
        vec![]
    } else {
        vec![(effect.as_path(), EFFECT_ID)]
    };
    let mut harness = Harness::with_fx(&source, SOURCE_ID, 1, &effects).expect("the chain builds");
    if let Some(effect) = harness.fx.first_mut() {
        if chain == Chain::Gain {
            restore_gain(&mut effect.instance);
        }
        effect.bypassed.store(bypassed, Ordering::Relaxed);
    }

    let mut output = Vec::with_capacity(BLOCKS * BLOCK * CHANNELS);
    for block in 0..BLOCKS {
        if block == 2 {
            assert!(harness.push_at(
                0,
                0,
                Payload::NoteOn {
                    channel: 0,
                    key: 60,
                    velocity: 100.0 / 127.0,
                },
            ));
        }
        if block == 28 {
            assert!(harness.push_at(
                0,
                0,
                Payload::NoteOff {
                    channel: 0,
                    key: 60,
                    velocity: 0.0,
                },
            ));
        }
        output.extend_from_slice(harness.render(BLOCK));
    }
    harness.shutdown();
    Some(output)
}

fn peak(interleaved: &[f32]) -> f32 {
    interleaved
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
}

fn first_sounding_frame(interleaved: &[f32]) -> Option<usize> {
    interleaved
        .chunks(CHANNELS)
        .position(|frame| frame.iter().any(|sample| *sample != 0.0))
}

/// **Inserting the plugin changes nothing**: fresh construction is the 1:1 line, bit-exact dry.
#[test]
fn inserted_at_init_it_changes_nothing_to_the_bit() {
    let Some(dry) = render(Chain::Dry, false) else {
        return;
    };
    let wet = render(Chain::Init, false).expect("the bundle existed above");
    assert!(peak(&dry) > 1e-3, "the source must be audible");
    assert_eq!(wet, dry);
}

/// **A saved setting is heard from its restore**: the raised Input gain into the 1:1 line makes
/// the note louder, finite, and nothing comes before the source.
#[test]
fn a_restored_input_gain_is_heard_finite_and_adds_nothing_before_its_input() {
    let Some(dry) = render(Chain::Dry, false) else {
        return;
    };
    let wet = render(Chain::Gain, false).expect("the bundle existed above");
    assert!(wet.iter().all(|sample| sample.is_finite()));
    assert_ne!(wet, dry, "the restored gain must change the signal");
    assert!(
        peak(&wet) > peak(&dry),
        "the restored gain is not heard: {} against the dry {}",
        peak(&wet),
        peak(&dry)
    );
    let first = first_sounding_frame(&dry).expect("the source sounded");
    assert!(
        wet[..first * CHANNELS].iter().all(|sample| *sample == 0.0),
        "the effect produced sound before its input"
    );
}

#[test]
fn player_bypass_restores_dry_audio_to_the_bit() {
    let Some(dry) = render(Chain::Dry, false) else {
        return;
    };
    let bypassed = render(Chain::Gain, true).expect("the bundle existed above");
    assert!(peak(&dry) > 1e-3, "the source must be audible");
    assert_eq!(bypassed, dry);
}
