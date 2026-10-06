# NOTES.md — plugins/mxm-fx-curve

The detail behind this folder's AGENTS.md: history, measurements, rationale and worked examples.
AGENTS.md is the contract; this file is the reference it links to.

## Editor layout

The editor opens at `REFERENCE` and stops at `MINIMUM` (`src/editor.rs`). At `SIDE_BY_SIDE_FLOOR`
of available width it changes from the side-by-side processing surface/control column to a
vertically stacked, horizontally scrollable flow; the canvas keeps `CANVAS_FLOOR`. In the wide
layout a vertical Input-gain control and post-gain stereo **In** meter sit to the canvas's left, and
a matching stereo post-Mix **Out** meter sits to its right. All three share the canvas height. The
Stage and Output controls cards stay at natural height in a fixed `CONTROL_COLUMN_WIDTH` column while
the canvas receives every remaining horizontal and vertical pixel. This deliberate exception lets
maximizing on a 4K screen become a precision edit mode rather than stretching empty controls. The
opening size is inside the quarter-4K budget (`editor_fits_quarter_4k`), and the minimum remains
inside it at 200% zoom.

## The controls cards as trees

**The two controls cards' bodies are `mxm_ui::tree`s, inside that exception** (`src/editor.rs`,
`stage_tree` and `output_tree`): each is drawn with `tree::show` in its own local frame, which is not
a paged card and has no paging floor. Stage: the heading — whose hover text says each curve shapes
the wave's upswings and downswings alike, a note printed under it until the owner ruled out help
text on the panel (2026-09-27) — Linked detector, and for a detector stage the ballistics
`egui::Grid` as a plugin-owned leaf whose size `ballistics_size` states from egui's grid arithmetic,
its value boxes at the widest reading egui prints at the smallest zoom. Output: the heading, Mix at
its `MIX_COLUMN`, Auto makeup and the curve estimate on one line. Both trees are built in the frame's
own item spacing, so the wide layout's zero horizontal spacing is measured as drawn.
`both_bodies_fit_the_300_point_column_in_every_stage_state` holds both inside the column (memoryless
and detector stages, Linked off and on, the ±24 dB estimate, wide and stacked spacing);
`both_bodies_pass_the_tree_checks_in_every_stage_state` runs `mxm_plugin_test::tree_checks`'s
checks on every leaf; `the_ballistics_grid_is_the_size_it_states` compares the grid with its
statement. The canvas, the rails and the stage strip stay hand-laid, and the column stays
`CONTROL_COLUMN_WIDTH`.

## The keyboard cursor

**The keyboard cursor runs over two bar cards**, because this editor has no paging renderer:
`INPUT_CARD` (the Input gain rail) and `OUTPUT_CARD` (Mix, Auto makeup), drawn through
`mxm_ui::navigation::bar_card`, outlined with `paint_card` and driven by `paged_with_bar`. The Stage
card's controls edit the curve model, not parameters, and are not cursor targets. **The cursor
stands aside** (`keyboard_held_elsewhere`) while a value is typed, the preset browser is open, or a
widget it does not know holds egui's focus — the canvas, a ballistics slider, a stage chip — or one
arrow would edit two things. The canvas takes focus on a click or a drag, claims the arrows while it
has it (egui otherwise moved its focus to a neighbour after the first nudge), and gives it back on a
press elsewhere or `Escape`. `the_keyboard_cursor_reaches_and_operates_every_parameter` and
`a_focused_canvas_keeps_the_arrows_and_escape_returns_them` hold it.

## Canvas colour

The canvas uses the collection cyan rather than inventing a hardware identity hue. Its line contrast
is 8.84:1 on the dark card and 4.65:1 on the light card (9.51:1 and 4.18:1 against the surrounding
canvases), measured with `mxm_ui::theme::contrast`'s WCAG law.

## The factory sounds

Sixty factory sounds cover linked compression, limiting, upward and downward dynamics, expanders,
noise gates, memoryless saturation/clipping/fuzz/folding/quantisation, equal-X discontinuities,
manual handles and order-sensitive serial combinations up to the five-stage limit. The source table
in `src/preset_designs.rs` is authoritative; the generator writes complete JSON and a test compares
every compiled file with a fresh generation.

## Canvas interaction and gestures

The production canvas preserves the accepted probe interaction: line-click insertion, automatic
smooth curvature, endpoint boundary movement, independent exact X/Y snapping, right-continuous
equal-X stacks, and Alt-click Curve → Linear → Handles → Curve. Only the selected Handles point
exposes handles; right-click/Delete removes only interior points.

One complete gesture is one **durable commit**, one host-dirty notification and one undo step.
Intermediate drag frames prepare complete engines on the GUI thread and publish them only as audible
previews through the fixed-slot handoff; they never alter persisted state, fingerprint, history or
host dirty state. Release performs the durable transaction. Returning to the origin, a failed
commit, or closing the editor mid-drag republishes the committed engine so no preview can remain
audible. Stage add/delete/reorder, detector edits and Init use the same durable transaction.
Pressing a stage chip selects it immediately, including when that press becomes a drag; continued
movement reorders it. Every chip keeps one fixed frame and size across selected and unselected
states, so changing stages never shifts the signal-path row. The context menu and focused Delete key
remove a stage, with snapshot Undo as the recovery path. The old permanent Move left / Move right /
Delete row does not return.

## The dirty-only state transaction

Every editor transaction prepares and publishes through `CurveField` on the control thread before it
notifies the host. It then sends a **dirty-only** `GuiContext::set_state()` transaction: no
parameters and the canonical fields already committed. The vendored nice-plug wrapper recognizes
that no-op state restore, calls CLAP `host.state.mark_dirty()`, and does not take the processor
mutex, reactivate or reset. Model state is never represented by fake parameters.

The processor-mutex-free dirty-only path is load-bearing. The old full GUI restore held nice-plug's
plugin mutex while preparing every 1,024-interval segment table. With audio playing, repeated edits
across two wavefolder curves made the callback contend; parking_lot's first contention allocated its
1,024-byte parking table inside the allocation guard and aborted the host. Host and preset restores
remain real state transactions and retain the ordinary reactivation/reset contract.

## Telemetry and meters

The selected stage is editor-only state and is never read by audio or persisted. While an editor is
connected, audio max-combines bounded per-stage magnitudes, stereo peaks immediately after Input
gain, and stereo peaks after Mix, then publishes atomics once per block. Input and output clipping
latch independently per channel until that channel's meter is clicked. The editor gives meters
immediate attack, a 450 ms peak hold and an approximately 300 ms visual release, so a one-block
excursion remains visible through the yellow/red zones. The shared vertical scale is −60 to 0 dBFS
with vivid green below −12 dBFS, true signal yellow from −12 to −3 dBFS and saturated red above −3
dBFS, painted only through the level the signal has reached; unreached track stays neutral gray.
Fixed positions, threshold dividers and the peak cap keep it readable without red/green
discrimination. A fresh clip lights its CLIP cell at full danger colour; the fresh-versus-held
distinction is visible because an older unacknowledged latch recedes to a lower-intensity red. Input
observation remains active at exact Mix zero even though the wet chain is parked. Disconnected
telemetry does no observation work. The canvas uses its selected stage's observed input magnitude to
place the dot on the authored static curve; it deliberately does not plot the detector's lagging
actual output as though that output changed the curve. No telemetry feeds an editor choice back into
DSP.

## Auto makeup

Auto makeup is prepared from the complete stack's static response to a full-scale sine: the output
RMS is normalized to the reference sine RMS and bounded to ±24 dB. Both stage modes share that
static law; detector timing and programme loudness cannot be inferred from a curve, so the editor
labels the number a **Curve estimate**, not a loudness promise. During a model transition each
engine receives its own prepared gain before the two outputs crossfade.

## What the tests cover, and the last verification

The unit suite covers Init equivalence, all sixty generated factory files, complete valid and
distinct preset models, detector/memoryless/serial/discontinuous/manual-handle coverage, preset
prepare/commit order and curve-only dirty comparison, the three global parameters, wet-only Input
gain, bounded curve-derived Auto makeup, the parameter-free already-committed dirty transaction,
bounded malformed state, model round-trip, canvas identity, flats and right continuity, bounded
history across close/restore boundaries, the host-visible GUI state path, dry Mix, one transition
route, every factory model under running audio, repeated editor commits across two stages
concurrent with audio, add/reorder/edit churn across same-rate reactivation, no callback allocator
operations, stereo post-gain input and post-Mix output peak/clip telemetry (including input
observation at Mix zero), wide-window canvas growth, exact silent activity and permanent identity.
The ignored player native-window test (in mxm-player since the split) inventories this editor. Current Windows debug and release
bundles each pass clap-validator with 33 passed, 0 failed and 11 skipped. The release bundle is
restored as the current artifact. Fresh release-bundle captures cover the opening size and 1800 ×
800 at 100%, including the matched In/Out rails. A native three-chip drag moved a new memoryless
stage ahead of the detector, and `fx dumpstate` confirmed the persisted memoryless/detector/memoryless
order. After replacing the drag-source wrapper, a fresh release-window check selected Stage 2 with an
ordinary click and then returned to Stage 1 with an ordinary click. The native 75–200% sweep,
real-DAW state-dirty behavior, listening approval, Linux and macOS remain unverified from the
Windows development machine.
