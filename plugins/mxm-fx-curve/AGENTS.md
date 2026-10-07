# AGENTS.md — plugins/mxm-fx-curve

Parent: [`../AGENTS.md`](../AGENTS.md) · The detail, measurements and reasoning behind each rule:
[`NOTES.md`](NOTES.md)

# Purpose

The CLAP shell, durable authored model and production editor for `mxm-fx-curve`, an original serial
transfer-curve effect. Framework-free processing stays in
[`../../crates/mxm-fx-curve-dsp/`](../../crates/mxm-fx-curve-dsp/).

# Ownership

- `src/model.rs` owns persisted schema version 1 and conversion into DSP stage specifications.
- `src/publication.rs` owns prepared-engine publication, transition and off-audio retirement.
- `src/params.rs` owns Input gain, Auto makeup, Mix, the durable curve-stack field, preset staging and its bounded snapshot history.
- `src/preset.rs`, `src/preset_designs.rs` and `presets/` own the shared preset seam, reviewable source table and sixty compiled factory sounds.
- `src/canvas.rs` owns the accepted point/handle interaction and keyboard surface.
- `src/editor.rs` owns the production shell, stage chain controls, reflow and complete model gestures.
- `src/telemetry.rs` owns lock-free per-stage signal-on-curve observation plus stereo post-input-gain and post-Mix peak/clip publication.
- `src/lib.rs` owns the CLAP shell, layouts, activity, tail behavior and editor construction.
- `control-map.json` maps Mix to the existing dynamics role; Input gain and Auto makeup have no
  collection-wide role. The licence is the repository's root `LICENSE` (`../../LICENSE`).

# Local Contracts

## Identity and host surface

- `plugin_name!` is the only in-crate product-name literal: `mxm-fx-curve`; `CLAP_ID` is
  `dk.mxm.mxm-fx-curve` and `bundler.toml` is its one external duplicate.
- Audio layouts are mono-in/mono-out and stereo-in/stereo-out. There is no MIDI, latency or tail.
- Three permanent global parameters are automatable: Input gain (`inputgain`), Auto makeup
  (`automakeup`) and Mix (`mix`). Authored points, point modes, handles, stage order, detector enable
  and ballistics are persisted model state, never hidden host parameters.
- Input gain spans −24 to +24 dB, defaults to unity and feeds only the wet branch before stage 1.
  Auto makeup defaults off, so both additions preserve old projects and every existing factory sound.
- Fresh construction and Init are the same exact no-op: one memoryless stage, Linked detector off,
  and the 1:1 line `(0,0)` → `(1,1)`. This plugin is an owner-directed exception to the collection
  default that an effect opens engaged; inserting it must not alter audio before the curve is edited
  or a factory sound is chosen.

## Editor layout and keyboard

- The editor opens at `REFERENCE` and stops at `MINIMUM` (`src/editor.rs`); below
  `SIDE_BY_SIDE_FLOOR` it stacks vertically and scrolls, and the canvas keeps `CANVAS_FLOOR`. The
  controls stay in a fixed `CONTROL_COLUMN_WIDTH` column while the canvas takes every remaining pixel
  — a deliberate exception. `editor_fits_quarter_4k` holds the budget
  ([NOTES.md § Editor layout](NOTES.md#editor-layout)).
- The Stage and Output card bodies are `mxm_ui::tree`s drawn with `tree::show`, not paged cards.
  `both_bodies_fit_the_300_point_column_in_every_stage_state`,
  `both_bodies_pass_the_tree_checks_in_every_stage_state` and
  `the_ballistics_grid_is_the_size_it_states` hold them. The canvas, the rails and the stage strip
  stay hand-laid ([NOTES.md § The controls cards as trees](NOTES.md#the-controls-cards-as-trees)).
- **The keyboard cursor runs over two bar cards**, `INPUT_CARD` and `OUTPUT_CARD`; the Stage card
  edits the model and is not a cursor target. **The cursor stands aside**
  (`keyboard_held_elsewhere`) whenever another widget holds egui's focus, so one arrow never edits
  two things. The canvas claims the arrows while focused and gives them back on a press elsewhere or
  `Escape` ([NOTES.md § The keyboard cursor](NOTES.md#the-keyboard-cursor)).
- The canvas uses the collection cyan, its contrast measured with `mxm_ui::theme::contrast`
  ([NOTES.md § Canvas colour](NOTES.md#canvas-colour)).

## Persisted model

- Schema version 1 stores stages, point coordinates, Curve/Linear/Handles mode, manual handles, and
  detector attack/release. Prepared lookup and antiderivative tables, antialias choice, detector
  history, transitions and revisions are runtime data and never serialized.
- Decoding bounds stage arrays at five and point arrays at 64 while deserializing, before allocating
  the model vectors. Unknown schema versions, unknown fields, mismatched handle/mode forms and every
  DSP-invalid curve reject the whole transaction; parameters must not apply around rejected state.
- Curve stacks contain one through five stages. A Handles point has exactly one handle pair;
  Curve/Linear points have none. The DSP crate remains the final authority for geometry and numeric
  validation.

## Presets

- The generated Init and every saved preset carry the complete schema-1 curve stack in `state` plus
  Input gain, Auto makeup and Mix as ordinary parameters. Curve state has a deterministic
  allocation-free fingerprint, so moving only a point, mode, handle, stage, detector setting or stage
  order marks a loaded preset modified.
- Sixty factory sounds; the source table in `src/preset_designs.rs` is authoritative, and a test
  compares every compiled file with a fresh generation
  ([NOTES.md § The factory sounds](NOTES.md#the-factory-sounds)).
- Preset models prepare completely before global-parameter gestures and publish only after them. An
  unchanged model publishes nothing and preserves undo history; a changed preset or Init begins a new
  history epoch. Init from either browser route restores the one-stage unlinked 1:1 identity rather
  than preserving an edited model.

## Publication and transitions

- Model edits prepare a complete immutable engine off audio, then publish one fixed-slot index with a
  monotonically increasing revision. Newer accepted work supersedes an older unpublished candidate;
  audio discards obsolete revisions.
- A slot is exclusively FREE, WRITING, READY, AUDIO or RETIRED. Control alone writes and reclaims;
  audio alone mutates AUDIO engines. Release/Acquire state changes are the ownership boundary.
- Audio may move ownership between slot indices but must never allocate, lock or destroy an engine.
  RETIRED engines are destroyed only by the next control-side publication/reclamation or final
  plugin teardown after processing has stopped.
- Every audible model replacement uses the same 20 ms two-engine linear transition. The new engine
  carries only the unchanged serial prefix's compatible history; the changed stage and downstream
  suffix prime on their actual upstream signal. Reorder, add/delete, state restore, undo/redo and
  Init paths may not bypass this mechanism.
- Reset resets both engines without cancelling a pending transition. Exact Mix zero or an exactly
  silent block settles both chains to silence and may finish the inaudible ownership transition.

## Editor transactions and history

- The canvas keeps the accepted probe interaction; only interior points can be removed
  ([NOTES.md § Canvas interaction](NOTES.md#canvas-interaction-and-gestures)).
- One complete gesture is one **durable commit**, one host-dirty notification and one undo step.
  Drag previews are complete engines published through the fixed-slot handoff and never alter
  persisted state, fingerprint, history or host dirty state; returning to the origin, a failed commit
  or closing mid-drag republishes the committed engine. Stage add/delete/reorder, detector edits and
  Init use the same durable transaction. Stage chips keep one fixed frame and size; the old
  permanent Move left / Move right / Delete row does not return.
- Snapshot history is bounded to 64 whole models and belongs to `CurveField`, not the transient
  editor. It survives editor close/reopen. A changed preset or host restore starts a new history
  epoch; undo and redo may not cross that boundary. An unchanged or rejected restore leaves history
  intact.
- Every editor transaction publishes through `CurveField` first, then sends a **dirty-only**
  `GuiContext::set_state()` that the vendored wrapper turns into `host.state.mark_dirty()` without
  the processor mutex, reactivation or reset. **That mutex-free path is load-bearing**: the old full
  restore aborted the host. Model state is never represented by fake parameters
  ([NOTES.md § The dirty-only state transaction](NOTES.md#the-dirty-only-state-transaction)).
- The selected stage is editor-only state, never read by audio or persisted. Telemetry is published
  once per block only while an editor is connected; disconnected telemetry does no work. Clipping
  latches per channel until its meter is clicked. Meters stay readable without red/green
  discrimination. No telemetry feeds an editor choice back into DSP
  ([NOTES.md § Telemetry and meters](NOTES.md#telemetry-and-meters)).

## Audio behavior

- The detector is stereo-linked in stereo and receives the duplicated mono sample in mono.
- The wet path is Input gain → serial curve stack → optional Auto makeup. Mix then crossfades that
  path against the untouched input, so exact Mix zero remains bit-exact dry at every gain setting.
  Input gain and Mix are smoothed over 20 ms. Auto makeup's on/off amount receives the same bounded
  20 ms ramp rather than stepping its calculated gain.
- Auto makeup is the complete stack's static response to a full-scale sine, bounded to ±24 dB, and
  the editor calls it a **Curve estimate**, not a loudness promise
  ([NOTES.md § Auto makeup](NOTES.md#auto-makeup)).
- Mix is applied after the model transition. Exact zero is bit-exact finite dry and parks detector
  history; leaving zero primes the chain under the Mix ramp.
- Non-finite audio input becomes zero. Exact silence produces exact silence and `ProcessStatus::Normal`:
  this processor has detector memory but no signal-generating tail, so a host may sleep it.
- `SAMPLE_ACCURATE_AUTOMATION` is enabled for Input gain, Auto makeup and Mix. The wrapper may split
  callbacks at parameter events; curve-model publication remains block-boundary coherent.

# Work Guidance

- Any GUI-authored transaction or drag preview prepares through `CurveField` and publishes through
  `EngineBank`; do not add an editor-only DSP path. Intermediate previews must remain transient and
  complete—never mutate an audio-owned engine, persisted state, history or host dirty state.
- Keep the canvas local to this plugin until a second consumer demonstrates a shared API.
- Do not persist or expose Direct/ADAA selection. Production antialiasing remains shape-derived in
  the DSP crate.

# Verification

```bash
cargo test -p mxm-fx-curve
cargo clippy -p mxm-fx-curve --all-targets -- -D warnings
MXM_PICTURES=after cargo test -p mxm-fx-curve --lib tree_pictures -- --ignored   # target/layout-tree/mxm-fx-curve/after/
cargo xtask bundle mxm-fx-curve
clap-validator validate target/bundled/mxm-fx-curve.clap
```

What the unit suite covers and the last bundle/validator/native results:
[NOTES.md § What the tests cover](NOTES.md#what-the-tests-cover-and-the-last-verification). The
native 75–200% sweep, real-DAW state-dirty behavior, listening approval, Linux and macOS remain
unverified from the Windows development machine. *Since the split (2026-10-06):* Linux and macOS are
checked later, together (Windows only during the work), and by CI on `v*` tags or by hand;
native-window, DAW and listening checks there are still not done.

# Child DOX Index

No child AGENTS.md files.
