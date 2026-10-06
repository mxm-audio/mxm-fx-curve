# mxm-fx-curve — UI design brief

Answers mxm-kit's [`docs/MXM_DESIGN_SYSTEM.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/MXM_DESIGN_SYSTEM.md) §14's ten questions before the editor is built, as
`plugins/AGENTS.md` requires. The plan is `plans/plan-mxm-fx-curve.md`; the reference is
`research:effects/dynamics-processing.md`.

**Implementation status:** C0.5's interaction, C1's measured DSP, C2's persisted plugin shell,
C3's production editor and C4's preset system are delivered. The accepted probe moved into the
plugin and was deleted; C3 also delivered bounded snapshot undo/redo, host dirty-state notification,
per-stage live telemetry, reflow and keyboard editing. C4 adds sixty complete curve-stack sounds
across dynamics, gates, expansion, shaping, folding, quantisation and serial combinations, plus the
shared browser/save surface and complete-model Init. The finishing pass adds wet-path Input gain,
bounded curve-derived Auto makeup, matched post-gain **In** / post-Mix **Out** stereo meter rails,
draggable stage chips and a canvas
that consumes the space released by maximizing the editor. C5's final listening, owner by-eye and
real-host gate remains.

**Two of §14's questions are written for instruments and are answered in the adapted form the
collection's other effect briefs already use.** Question 5 asks which *software-added* controls are
advanced, and confirms "every reproduced source-panel sound and performance control remains directly
visible"; question 9 asks what is "removed from the source hardware layout". **This effect copies no
hardware** — there is no source panel and nothing to reproduce or remove. Both are answered against
the *technique* the research page describes instead, which is the honest equivalent: what a
conventional compressor's panel would have shown, and what this one does not.

---

## 1. Primary sound-design task

**Draw the gain law, instead of describing it with a threshold and a ratio.**

A conventional dynamics processor gives you four numbers and a fixed shape. Here the shape *is* the
interface: input level on X, output level on Y, a curve through points you place. The research page
settles that this is not a novelty — a compressor's **static gain computer** is defined in the
literature as "a memoryless nonlinear function, that maps input level to output level" (Wright &
Välimäki, DAFx-2022), and the textbook hard knee is that function restricted to two straight lines.
Drawing it removes a restriction rather than adding a feature.

The task has two halves, and the optional detector is the switch between them:

- **With a detector** — compression, limiting, expansion, gating, and shapes between them that have
  no name. The research page is explicit that these are *"one curve family, not four effects."*
- **Without one** — the curve maps the sample directly: a waveshaper. Switching a point to Linear
  admits the sharp corner needed for distortion.

Stages in series exist because **stacking curves is how you refine a shape you cannot draw in one
pass** — the owner's reason, and the colourist's.

## 2. Controls reached for most

Three groups:

1. **The curve itself** — placing, dragging and shaping points. Everything else is in service of it.
2. **Level** — the vertical Input-gain fader drives the wet stack, optional Auto makeup restores its
   nominal static level, and Mix performs the final dry/wet blend. These are the plugin's three host
   parameters; the post-gain In and post-Mix Out rails make their signal positions explicit.
3. **The stage strip** — press a chip to put that stage on screen, continue the press into a drag to
   reorder, add with the button, and right-click or focus-and-Delete a chip to remove it.

Attack and release matter *when a detector is on*, but they are set while listening rather than
performed; they are durable model state, not parameters (decision 2), and they sit on the stage's own
card. Point and handle drags are auditioned continuously: each intermediate shape is prepared
off-audio and published as a transient preview, while release remains one durable commit and one Undo
step.

## 3. Signal flow that must be visible

```
in ──┬────────────────────────────── dry ───────────────────────────┐
     │                                                             │
     └─▶ Input gain ─▶ [stage 1] ─▶ … ─▶ [stage n] ─▶ Auto makeup ─┴─▶ Mix ─▶ out
                            │
                            └─ curve  ( + optional detector: attack / release )
```

Three things must read off the panel without explanation:

- **That stages are a chain, and in what order** — because order changes the sound (composition does
  not commute), so the strip is a signal path, not a tab bar.
- **Which stages have a detector**, since that is the difference between dynamics and distortion and
  cannot be inferred from the curve.
- **That Input gain and Auto makeup belong only to the wet branch, and Mix is after the whole
  stack.** Exact Mix zero therefore remains the untouched input regardless of either gain control.

Detector stages are **stereo-linked with no link control**: the larger channel peak asks for gain
change and the same gain is applied to both channels, so dynamics cannot tilt the stereo image.

## 4. Play view

**No view bar**, as with every other effect here — one set of Effects cards, plus the app bar.

The **curve canvas is the play surface** and takes the space (owner: large enough for fine
movements). At the opening size it dominates the panel. A same-height left rail holds the vertical
Input-gain fader and stereo post-gain **In** meter; a same-height right rail holds the stereo post-Mix
**Out** meter. The stage strip remains above, while the compact Stage and Output controls cards form
the separate right column. There is no smaller "performance" arrangement to switch to, because
there is nothing to switch away from.

## 5. Advanced controls and disclosure

Everything this effect has is directly visible; nothing is disclosed behind an expander.

The adapted form of the question: a conventional compressor's panel would show **threshold, ratio,
knee, attack, release, makeup**. Here **threshold, ratio and knee do not exist as controls** — they
are the curve's shape, and putting them back would be two ways to say one thing. **Attack and
release exist per stage**, on that stage's card, visible whenever its detector is on and absent when
it is off. The curve still supplies its own vertical placement, but **Auto makeup** is available as
an explicit convenience when comparing shapes: it estimates the complete serial stack's static RMS
response to a full-scale sine and bounds correction to ±24 dB. It does not claim programme-loudness
matching, so the panel names the result a *Curve estimate*. **Input gain** drives only the wet branch
before stage 1; it is not hidden inside the curve.

The curve has one direct point workflow. It begins as the lower-left to upper-right identity line;
clicking that line adds a point without changing it, and dragging the point makes one automatically
smooth curve. The black endpoint moves along the left or bottom boundary; the white endpoint moves
along the top or right. Points snap independently to one another's exact X and Y with visible guides:
Y alignment makes an exact flat plateau, while X alignment makes an intentional right-continuous
discontinuity. **Alt-click cycles the selected point Curve → Linear → Handles → Curve.** Linear is the
deliberate distortion-corner mode. Only a selected point in Handles mode exposes its two manual
handles, because three permanent targets per point would make the canvas unusable at the
pointer-target floor.

## 6. Categories, cards, and grouping

Primary category **Effects** throughout; the paging renderer orders category-first.

| Card | Holds | Floor |
|---|---|---|
| **Curve processing surface** | Vertical Input-gain fader with post-gain In L/R meter, canvas, and post-Mix Out L/R meter | The canvas keeps the largest floor; both rails match its full height, and a canvas too small to place a point accurately is a failed design rather than a tight fit |
| **Stage** | The visible stage's detector: present, attack, release | Natural content height; collapses to the detector toggle alone when off |
| **Stages** | The strip: select, add, drag to reorder, context-menu/Delete-key removal | One row |
| **Output controls** | Mix, Auto makeup and its curve estimate | Natural content height; never stretches to match the canvas |

**Curve and Stage stay together while space permits** — the detector's settings are read against the
curve they shape.

## 7. Identity accent

The editor deliberately keeps the collection cyan rather than inventing a box-like identity hue. The
curve line measures **8.84:1** against the dark card and **4.65:1** against the light card; against
the surrounding canvases it measures **9.51:1** and **4.18:1**. These WCAG ratios come from
`mxm_ui::theme::contrast`. The line therefore clears the 3:1 graphical-object floor in both themes
on the plotted surface, which is the harder case than a filled knob.

## 8. Live visualization

Three readings, each with one job:

- The canvas dot shows **the selected stage's current input target on the authored curve**. It always
  lies on that curve; a detector's attack/release may intentionally make the actual gain lag behind
  it.
- The In L/R meter reports the wet branch immediately after Input gain, including at exact Mix zero.
- The Out L/R meter reports the actual post-ballistics, post-makeup, post-Mix output.

Both meter rails use the shared −60 to 0 dBFS vertical scale with a neutral unreached track, vivid
green/yellow/red signal colour only through the reached level, explicit threshold dividers and a
non-colour peak cap, plus immediate attack, a 450 ms peak hold and an approximately 300 ms visual
release. The palette and
geometry remain readable without red/green discrimination. A fresh clip lights its per-channel cell at full red; an older unacknowledged latch remains
subdued red until the corresponding meter is clicked. Brief upper-zone and repeat clip events are
therefore visible instead of disappearing or looking identical to an old latch.

These visualizations materially improve understanding: the dot turns an abstract law into a reading
of what the music asks it to do, the In rail shows what is driven into the stack, and the Out rail
shows what leaves the plugin. They are telemetry,
so they follow the collection's rules: atomics only, dropped frames acceptable, disabled when the
editor is disconnected, and **the curve stays legible without them**.

Explicitly **not** included in v1: a histogram behind the curve and a separate gain-reduction meter.
The In/Out meters answer level and clipping without claiming that a generic free curve has one
universally meaningful gain-reduction quantity.

## 9. What is removed, and why

Adapted, since there is no source hardware. Against the technique the research page describes:

- **Threshold, ratio and knee** — absorbed into the curve (§5). This is the product.
- **Look-ahead** — excluded from v1 by the plan's §2.2: it cannot coexist with the zero-Mix
  identity, the idle cost, the tail and the latency contract as they stand.
- **An external sidechain key** — the research page notes the side chain may take another signal;
  v1 does not, keeping one broadband path.
- **Multiband** — out of scope; this is one path.
- **A gain-reduction meter as a separate widget** — §8.

## 10. Fit, reflow, zoom, and Init

**Fit.** One page at the quarter-4K budget with disclosures open, verified by the shared
`opening_size` check. The canvas takes all horizontal and vertical slack between fixed-width In/Out
rails, while the right column keeps its fixed `CONTROL_COLUMN_WIDTH` at natural content height;
maximizing the native window turns the canvas into a precision surface, including on a 4K display.
The rails match the canvas height rather than stretching the Stage or Output controls cards. The
binding risk is the pointer-target floor, so the canvas has the largest card floor in the panel and
manual handles are drawn only for the selected Handles-mode point (§5). A separate fullscreen button
is deliberately absent unless a host is later found not to expose native maximize.

**Reflow.** Below the editor's `SIDE_BY_SIDE_FLOOR` of available width the processing surface and
control column stack. The fixed In / canvas / Out surface becomes horizontally scrollable rather
than squeezing either meter or shrinking the canvas below the floor at which a point can be placed
accurately.

**Zoom.** 75–200% from the app bar. The canvas scales its grid and its targets together, so the
pointer floor holds at every step. C3's headless surface proof paints both themes inside the logical
spaces corresponding to a 1920 × 1080 physical budget at **75% and 200%**. Fresh Windows native
captures at 100% cover the opening surface (`REFERENCE`) and an 1800 × 800 wide surface after the
finishing layout. The full native zoom sweep and owner acceptance remain part of C5 rather than being
implied by either screenshots or headless geometry.

**Init — revised by owner after native audition.** This open-ended authoring effect is a specific
exception to *an effect starts engaged*. **Init and a fresh instance are an exact no-op: one stage,
Linked detector off, and the straight 1:1 line `(0,0)` → `(1,1)`.** Inserting the plugin must not
change sound until the player draws a law or chooses a factory sound. Init and a fresh instance render
bit-exactly alike through the durable-state transaction the plan's §5.1 requires.

## Sign-off

- [x] §14's ten questions answered, with 5 and 9 answered in the adapted form stated at the top
- [x] Init revised after native audition: one unlinked memoryless stage on the 1:1 identity line
- [x] Collection accent chosen and line contrast measured in both themes (C3)
- [x] Headless 75–200% quarter-4K surfaces and native 100% opening/wide surfaces recorded
- [ ] §15 QA gate run by eye on the real panel (C5)

**Deviations from the norm, recorded rather than left implicit:**

| §14.n | Question | What was decided | Why |
|---|---|---|---|
| 5 | Advanced software-added controls | Nothing is disclosed; points snap in X/Y, Alt-click cycles Curve → Linear → Handles, and manual handles appear only for the selected Handles-mode point | There is no source panel to reproduce; direct point editing stays primary, snapping makes exact flats and discontinuities practical, Linear preserves distortion corners, and permanent handles break the pointer-target floor |
| 9 | Removed from source layout | Answered against the *technique*, not hardware | This effect copies no box — the research page describes a practice, and what it "removes" is threshold-and-ratio |
