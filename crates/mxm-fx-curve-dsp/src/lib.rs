//! Framework-free DSP for `mxm-fx-curve`.
//!
//! This is an original transfer-curve processor against the public dynamics technique documented at
//! `research:effects/dynamics-processing.md` and the collection's own waveshaping measurements in
//! `docs/oscillators/17-waveshaping-and-folding.md`. No existing processor implementation was
//! opened. The accepted point interaction is recorded in `plans/plan-mxm-fx-curve.md` C0.5.
//!
//! An authored [`Curve`] is prepared off audio into [`CurveTable`]: one lookup table and one
//! antiderivative table per non-vertical point pair. Equal-X points are intentional discontinuities
//! and evaluate right-continuously. The sample path binary-searches point X positions, interpolates
//! one prepared table and never walks or solves a spline.
//!
//! A stage reads that same curve in one of two ways:
//!
//! - memoryless mode maps the **magnitude** of each sample and restores its sign, so arbitrary curves
//!   remain silence-preserving and symmetric. C1's alias-and-reference probe selected direct lookup
//!   for continuous curves and first-order residual ADAA for an equal-X discontinuity; this is
//!   derived from the shape rather than persisted state, and identity remains bit-exact;
//! - detector mode peak-links stereo, derives one gain from the curve, smooths that gain and applies
//!   it to both channels. Post-curve smoothing bounds the control signal produced by an arbitrary
//!   corner and preserves the stereo image.
//!
//! Stages run in authored order. [`CurveEngine`] owns no publication or editor state; C2 supplies the
//! off-audio prepare/publish/retire transaction and the host's parameter ramp.

#![forbid(unsafe_code)]

mod engine;
mod model;
mod stage;
mod table;

pub use engine::{CurveEngine, EngineError, MAX_MAKEUP_GAIN, MAX_STAGES};
pub use model::{ControlPoint, Curve, CurveError, Handle, MAX_POINTS, PointMode};
pub use stage::{
    AntialiasMode, MAX_GAIN, MAX_OUTPUT, MAX_SAMPLE_RATE, MAX_TIME_MS, MIN_SAMPLE_RATE,
    MIN_TIME_MS, Stage, StageError, StageMode, StageSpec,
};
pub use table::{CurveTable, TABLE_INTERVALS_PER_SEGMENT};
