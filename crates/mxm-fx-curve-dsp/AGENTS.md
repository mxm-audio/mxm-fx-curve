# AGENTS.md — mxm-fx-curve-dsp

Parent: [`../../AGENTS.md`](../../AGENTS.md)

# Purpose

Framework-free, zero-runtime-dependency DSP for the original `mxm-fx-curve` effect: the accepted
point-curve model, bounded prepared evaluation, antialiased memoryless shaping, linked peak dynamics,
a serial stage chain, the curve-derived nominal makeup estimate and the final dry/wet law.

The product contract is `../../plans/plan-mxm-fx-curve.md` (`plans/plan-mxm-fx-curve.md` in the private archive); the
dynamics technique is `research:effects/dynamics-processing.md`; the collection's own waveshaping
evidence is mxm-kit's [`docs/oscillators/17-waveshaping-and-folding.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/oscillators/17-waveshaping-and-folding.md).
No existing processor implementation was opened.

# Ownership

- `src/model.rs`: authored points, modes, handles and model validation.
- `src/table.rs`: off-audio spline preparation, right-continuous lookup and antiderivative tables.
- `src/stage.rs`: memoryless shaping, first-order ADAA, linked detector gain and ballistics.
- `src/engine.rs`: bounded serial composition, static-stack makeup analysis, Mix, parking/reset, replacement-history carry and caller-owned per-stage level observations.
- `tests/`: public evaluator, dynamics, numeric, state-carry and alias proofs.
- `examples/curve_alias.rs`: direct-versus-ADAA measurement for smooth, corner and discontinuous
  curves.
- `examples/detector_order.rs`: detector smoothing-order measurement.
- `examples/curve_cost.rs`: maximum-state, two-chain transition cost across sample rates.

# Local Contracts

- **The C0.5 model is the input.** X is nondecreasing; exact equal-X stacks are accepted and
  right-continuous. Exact equal-Y runs remain flat. Curve is the automatic shape, Linear forces both
  neighboring segments straight, and Handles supplies bounded cubic controls. Manual X controls may
  not cross a segment or one another, because the evaluator inverts X(t).
- **Preparation is off audio.** Every segment receives 1,024 uniform-X intervals with value and
  antiderivative tables. Processing binary-searches point X positions, interpolates one table and
  never walks or solves a spline. The stress curve's measured maximum value error is below 5e-4 and
  integral error below 2e-6; the limits are tests, not claims about arbitrary real-valued curves.
- **A memoryless stage is odd and silence-preserving.** It maps sample magnitude through the drawn
  curve and restores sign. Production derives antialiasing from the shape: continuous smooth and
  Linear-corner curves use direct prepared lookup; an equal-X discontinuity uses first-order ADAA on
  the nonlinear residual. `Auto` is the product path, and the result is not persisted or
  user-selectable. Identity remains bit-exact and the linear path has zero reported latency.
- **A detector stage is linked peak dynamics.** The larger stereo magnitude addresses the static
  curve; one resulting gain is applied to both channels. Attack/release smooth **gain after the
  curve**, not level before it. At the fastest 0.1 ms setting, the ordering probe measured energy
  above 10 kHz at -49.63 dB of gain-signal AC versus -35.08 dB when level was smoothed first. This
  selection makes a curve edit a state-law edit: its stage and every downstream stage prime rather
  than carrying history.
- **Level observation is not control.** `process_with_stage_levels` writes each stage's bounded input/output magnitude into a caller-owned fixed array for editor telemetry; it allocates nothing and reads no selected-stage or editor state. The ordinary `process` path delegates to the same engine law.
- **Replacement history is a prefix.** `carry_compatible_prefix_history_from` carries only identical
  curve/mode/ballistics stages before the first change. Every later stage has a changed upstream and
  earns history while old and new chains run together. Publication, transition duration, retirement
  and request ordering belong to the plugin crate.
- **Nominal makeup is a curve calculation, not programme analysis.** Preparation samples the complete
  serial stack's static magnitude response with a full-scale sine, normalizes its RMS back to the
  reference sine and bounds the result to ±24 dB. Both stage modes share the authored static law;
  detector timing and the future input distribution are deliberately absent. Non-monotonic and
  zero-endpoint curves are sampled across the domain rather than guessed from `curve(1)`.
- **Mix zero parks the whole chain.** Finite dry input is bit-exact; stage state settles to its silent
  destination once, so stale gain cannot return. Waking is a plugin-level transition that primes the
  chain while Mix ramps. Silence always yields exact silence; there is no tail or reported latency.
- **Numeric recovery is not character.** Non-finite input becomes zero at the wet seam, subnormal wet
  input/state flushes to zero, gain is bounded to 16 and hostile detector output to ±16. An identity
  memoryless stage deliberately carries every finite input unchanged. `reset()` is deterministic.

## Measured and chosen facts

- `MAX_POINTS = 64` is the stored-model safety ceiling; `MAX_STAGES = 5` is the realtime ceiling.
  Both are schema-visible limits once C2 ships.
- On the development Windows x86_64 machine, a release render of **two** maximum chains — five stages
  each, 64 points each, alternating discontinuous ADAA and detector stages — measured 18.4x realtime at 48 kHz,
  4.5x at 192 kHz and 1.1x at 768 kHz. Preparing both measured about 155 ms. These are local warning
  evidence, not portable proof; caching the prior antiderivative value made five viable, while the
  initial six-stage candidate missed 768 kHz.
- The alias harness measures both unwanted bins and RMS deviation from a 16x band-limited reference.
  At 996.826 Hz, direct / ADAA alias and reference-error pairs were: smooth -60.85/-68.70 and
  -60.69/-26.63 dB; corner -47.69/-55.49 and -46.39/-26.23 dB; discontinuity -22.56/-33.41 and
  -21.86/-19.95 dB. At 6,999.756 Hz they were: smooth -22.15/-34.95 and -21.23/-14.17 dB; corner
  -22.03/-35.16 and -21.07/-13.85 dB; discontinuity -13.67/-25.96 and -13.68/-21.92 dB. Thus ADAA's
  phase error costs more total fidelity on continuous curves, while the high-frequency discontinuity
  improves by 12.29 dB in alias and 8.24 dB against the reference. Production `Auto` selects direct
  for continuous curves and ADAA only for equal-X stacks. The alias regression keeps the comparison
  method from silently losing its measured benefit.
- `MIN_TIME_MS = 0.1`, `MAX_TIME_MS = 5000`, `MAX_GAIN = 16`, `MAX_OUTPUT = 16` and
  `MAX_MAKEUP_GAIN = 16` are chosen product and numeric bounds, not values read from a device.

# Work Guidance

- Do not add framework, GUI, serialization, filesystem, publication slots or plugin parameters here.
- Preparation may allocate; `Stage::process` and `CurveEngine::process` may not.
- Run the three release examples when changing table density, ADAA, detector order, count limits or
  per-sample work, and replace recorded numbers rather than appending a diary.
- Product code constructs memoryless stages with `StageSpec::memoryless`, which selects `Auto`.
  Explicit `Direct` and `Adaa` exist only to keep both measured candidates reproducible; neither is
  persisted or user-selectable.
- Do not extract the curve evaluator or detector until another shipped implementation demonstrates a
  genuinely shared API.

# Verification

```bash
cargo test -p mxm-fx-curve-dsp
cargo clippy -p mxm-fx-curve-dsp --all-targets -- -D warnings
cargo run -p mxm-fx-curve-dsp --release --example curve_alias
cargo run -p mxm-fx-curve-dsp --release --example detector_order
cargo run -p mxm-fx-curve-dsp --release --example curve_cost
cargo +1.87.0 test -p mxm-fx-curve-dsp
```

The tests prove right continuity, exact flats and identity, prepared-table error bounds, alias
improvement, one-pole ballistics, stereo linking, authored count and handle bounds, finite sample-rate
extremes, identity/reduction/lift/silent nominal-makeup behavior, Mix-zero identity/parking, reset
silence, serial order, compatible-prefix history carry and
zero allocator/destructor operations inside DSP processing. Plugin callback allocation instrumentation,
transition publication/retirement, host activity/tail, Linux/macOS and
real-DAW behavior belong to later phases. *Since the split (2026-10-06):* Linux and macOS are checked
later, together (Windows only during the work), and by CI on `v*` tags or by hand.

# Child DOX Index

No child AGENTS.md files.
