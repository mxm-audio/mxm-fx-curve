# AGENTS.md — plugins/mxm-fx-curve

Parent: [`../AGENTS.md`](../AGENTS.md)

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
- `control-map.json` maps Mix to the existing dynamics role; Input gain and Auto makeup have no collection-wide role. `LICENSE` applies MIT to this plugin.

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
- The editor opens at `REFERENCE` and stops at `MINIMUM` (`src/editor.rs`). At
  `SIDE_BY_SIDE_FLOOR` of available width it changes from the side-by-side processing
  surface/control column to a vertically stacked, horizontally scrollable flow; the canvas keeps
  `CANVAS_FLOOR`. In the wide layout a vertical
  Input-gain control and post-gain stereo **In** meter sit to the canvas's left, and a matching stereo
  post-Mix **Out** meter sits to its right. All three share the canvas height. The Stage and Output
  controls cards stay at natural height in a fixed `CONTROL_COLUMN_WIDTH` column while the canvas
  receives every remaining horizontal and vertical pixel. This deliberate exception lets maximizing
  on a 4K screen become a precision edit mode rather than stretching empty controls. The opening
  size is inside the quarter-4K budget (`editor_fits_quarter_4k`), and the minimum remains inside it
  at 200% zoom.
- **The two controls cards' bodies are `mxm_ui::tree`s, inside that exception** (`src/editor.rs`,
  `stage_tree` and `output_tree`): each is drawn with `tree::show` in its own local frame, which is
  not a paged card and has no paging floor. Stage: the heading — whose hover text says each curve
  shapes the wave's upswings and downswings alike, a note printed under it until the owner ruled out
  help text on the panel (2026-09-27) — Linked detector, and for a detector
  stage the ballistics `egui::Grid` as a plugin-owned leaf whose size `ballistics_size` states from
  egui's grid arithmetic, its value boxes at the widest reading egui prints at the smallest zoom.
  Output: the heading, Mix at its `MIX_COLUMN`, Auto makeup and the curve estimate on one line.
  Both trees are built in the frame's own item spacing, so the wide layout's zero horizontal spacing
  is measured as drawn. `both_bodies_fit_the_300_point_column_in_every_stage_state` holds both
  inside the column (memoryless and detector stages, Linked off and on, the ±24 dB estimate, wide and
  stacked spacing); `both_bodies_pass_the_tree_checks_in_every_stage_state` runs
  `mxm_plugin_test::tree_checks`'s checks on every leaf; `the_ballistics_grid_is_the_size_it_states`
  compares the grid with its statement. The canvas, the rails and the stage strip stay hand-laid,
  and the column stays `CONTROL_COLUMN_WIDTH`.
- **The keyboard cursor runs over two bar cards**, because this editor has no paging renderer:
  `INPUT_CARD` (the Input gain rail) and `OUTPUT_CARD` (Mix, Auto makeup), drawn through
  `mxm_ui::navigation::bar_card`, outlined with `paint_card` and driven by `paged_with_bar`. The
  Stage card's controls edit the curve model, not parameters, and are not cursor targets. **The
  cursor stands aside** (`keyboard_held_elsewhere`) while a value is typed, the preset browser is
  open, or a widget it does not know holds egui's focus — the canvas, a ballistics slider, a stage
  chip — or one arrow would edit two things. The canvas takes focus on a click or a drag, claims
  the arrows while it has it (egui otherwise moved its focus to a neighbour after the first nudge),
  and gives it back on a press elsewhere or `Escape`.
  `the_keyboard_cursor_reaches_and_operates_every_parameter` and
  `a_focused_canvas_keeps_the_arrows_and_escape_returns_them` hold it.
- The canvas uses the collection cyan rather than inventing a hardware identity hue. Its line
  contrast is 8.84:1 on the dark card and 4.65:1 on the light card (9.51:1 and 4.18:1 against the
  surrounding canvases), measured with `mxm_ui::theme::contrast`'s WCAG law.

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
  allocation-free fingerprint, so moving
  only a point, mode, handle, stage, detector setting or stage order marks a loaded preset modified.
- Sixty factory sounds cover linked compression, limiting, upward and downward dynamics, expanders,
  noise gates, memoryless saturation/clipping/fuzz/folding/quantisation, equal-X discontinuities,
  manual handles and order-sensitive serial combinations up to the five-stage limit. The source
  table in `src/preset_designs.rs` is authoritative; the generator writes complete JSON and a test
  compares every compiled file with a fresh generation.
- Preset models prepare completely before global-parameter gestures and publish only after them. An unchanged
  model publishes nothing and preserves undo history; a changed preset or Init begins a new history
  epoch. Init from either browser route restores the one-stage unlinked 1:1 identity rather than
  preserving an edited model.

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

- The production canvas preserves the accepted probe interaction: line-click insertion, automatic
  smooth curvature, endpoint boundary movement, independent exact X/Y snapping, right-continuous
  equal-X stacks, and Alt-click Curve → Linear → Handles → Curve. Only the selected Handles point
  exposes handles; right-click/Delete removes only interior points.
- One complete gesture is one **durable commit**, one host-dirty notification and one undo step.
  Intermediate drag frames prepare complete engines on the GUI thread and publish them only as
  audible previews through the fixed-slot handoff; they never alter persisted state, fingerprint,
  history or host dirty state. Release performs the durable transaction. Returning to the origin,
  a failed commit, or closing the editor mid-drag republishes the committed engine so no preview can
  remain audible. Stage add/delete/reorder, detector edits and Init use the same durable transaction.
  Pressing a stage chip selects it immediately, including when that press
  becomes a drag; continued movement reorders it. Every chip keeps one fixed frame and size across
  selected and unselected states, so changing stages never shifts the signal-path row. The context
  menu and focused Delete key remove a stage, with snapshot Undo as the recovery path. The old permanent Move left / Move right /
  Delete row does not return.
- Snapshot history is bounded to 64 whole models and belongs to `CurveField`, not the transient
  editor. It survives editor close/reopen. A changed preset or host restore starts a new history
  epoch; undo and redo may not cross that boundary. An unchanged or rejected restore leaves history
  intact.
- Every editor transaction prepares and publishes through `CurveField` on the control thread before
  it notifies the host. It then sends a **dirty-only** `GuiContext::set_state()` transaction: no
  parameters and the canonical fields already committed. The vendored nice-plug wrapper recognizes
  that no-op state restore, calls CLAP `host.state.mark_dirty()`, and does not take the processor
  mutex, reactivate or reset. Model state is never represented by fake parameters.
- The processor-mutex-free dirty-only path is load-bearing. The old full GUI restore held nice-plug's plugin
  mutex while preparing every 1,024-interval segment table. With audio playing, repeated edits across
  two wavefolder curves made the callback contend; parking_lot's first contention allocated its
  1,024-byte parking table inside the allocation guard and aborted the host. Host and preset restores
  remain real state transactions and retain the ordinary reactivation/reset contract.
- The selected stage is editor-only state and is never read by audio or persisted. While an editor
  is connected, audio max-combines bounded per-stage magnitudes, stereo peaks immediately after Input
  gain, and stereo peaks after Mix, then publishes atomics once per block. Input and output clipping
  latch independently per channel until that channel's meter is clicked. The editor gives meters
  immediate attack, a 450 ms peak hold and an approximately 300 ms visual release, so a one-block
  excursion remains visible through the yellow/red zones. The shared vertical scale is −60 to 0
  dBFS with vivid green below −12 dBFS, true signal yellow from −12 to −3 dBFS and saturated red
  above −3 dBFS, painted only through the level the signal has reached; unreached track stays neutral
  gray. Fixed positions, threshold dividers and the peak cap keep it readable without
  red/green discrimination. A fresh clip lights its CLIP cell at full danger colour; the fresh-versus-held distinction is
  visible because an older unacknowledged latch recedes to a lower-intensity red. Input observation
  remains active at exact Mix zero even though the wet chain is parked.
  Disconnected telemetry does no observation work. The canvas uses its selected stage's observed
  input magnitude to place the dot on the authored static curve; it deliberately does not plot the
  detector's lagging actual output as though that output changed the curve. No telemetry feeds an
  editor choice back into DSP.

## Audio behavior

- The detector is stereo-linked in stereo and receives the duplicated mono sample in mono.
- The wet path is Input gain → serial curve stack → optional Auto makeup. Mix then crossfades that
  path against the untouched input, so exact Mix zero remains bit-exact dry at every gain setting.
  Input gain and Mix are smoothed over 20 ms. Auto makeup's on/off amount receives the same bounded
  20 ms ramp rather than stepping its calculated gain.
- Auto makeup is prepared from the complete stack's static response to a full-scale sine: the output
  RMS is normalized to the reference sine RMS and bounded to ±24 dB. Both stage modes share that
  static law; detector timing and programme loudness cannot be inferred from a curve, so the editor
  labels the number a **Curve estimate**, not a loudness promise. During a model transition each
  engine receives its own prepared gain before the two outputs crossfade.
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

The unit suite covers Init equivalence, all sixty generated factory files, complete valid and distinct
preset models, detector/memoryless/serial/discontinuous/manual-handle coverage, preset prepare/commit
order and curve-only dirty comparison, the three global parameters, wet-only Input gain, bounded
curve-derived Auto makeup, the parameter-free already-committed dirty transaction,
bounded malformed state, model round-trip, canvas identity,
flats and right continuity, bounded history across close/restore boundaries, the host-visible GUI
state path, dry Mix, one transition route, every factory model under running audio, repeated
editor commits across two stages concurrent with audio, add/reorder/edit churn across same-rate
reactivation, no callback allocator operations, stereo post-gain input and post-Mix output
peak/clip telemetry (including input observation at Mix zero), wide-window canvas growth, exact
silent activity and permanent identity. The ignored player native-window test inventories this
editor. Current Windows debug and release bundles each pass clap-validator with 33 passed, 0 failed
and 11 skipped. The release bundle is restored as the current artifact. Fresh release-bundle
captures cover the opening size and 1800 × 800 at 100%, including the matched In/Out rails. A native
three-chip drag moved a new memoryless stage ahead of the detector, and `fx dumpstate` confirmed the
persisted memoryless/detector/memoryless order. After replacing the drag-source wrapper, a fresh
release-window check selected Stage 2 with an ordinary click and then returned to Stage 1 with an
ordinary click. The native 75–200% sweep, real-DAW state-dirty
behavior, listening approval, Linux and macOS remain unverified from the Windows development
machine.

# Child DOX Index

No child AGENTS.md files.
