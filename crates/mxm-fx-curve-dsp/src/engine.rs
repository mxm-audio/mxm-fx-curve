//! A bounded serial stage chain and its dry/wet output law.

use crate::stage::{Stage, StageError, StageSpec};

/// Provisional authored-stage ceiling.
///
/// Chosen high enough for the editing model and low enough to bound preparation before C1's release
/// cost harness. The recorded double-chain transition measurement owns the final number.
pub const MAX_STAGES: usize = 5;

/// Bound for curve-derived nominal output compensation: ±24.08 dB.
pub const MAX_MAKEUP_GAIN: f32 = 16.0;
const MAKEUP_REFERENCE_SAMPLES: usize = 2_048;

#[derive(Debug, Clone)]
pub struct CurveEngine {
    stages: Vec<Stage>,
    nominal_makeup_gain: f32,
    parked: bool,
}

impl CurveEngine {
    /// Prepare every table and detector off audio. No processing call changes capacity.
    pub fn prepare(specs: &[StageSpec], sample_rate: f32) -> Result<Self, EngineError> {
        if specs.len() > MAX_STAGES {
            return Err(EngineError::TooManyStages);
        }
        let mut stages = Vec::with_capacity(specs.len());
        for spec in specs {
            stages.push(Stage::prepare(spec, sample_rate).map_err(EngineError::Stage)?);
        }
        let nominal_makeup_gain = nominal_makeup_gain(&stages);
        Ok(Self {
            stages,
            nominal_makeup_gain,
            parked: false,
        })
    }

    /// Process one stereo frame. `mix == 0` is a bit-exact finite dry path and parks the whole chain.
    /// Leaving zero is expected to occur on the plugin's Mix ramp, which runs every detector on its
    /// real upstream signal while the wet share becomes audible.
    pub fn process(&mut self, input: [f32; 2], mix: f32) -> [f32; 2] {
        self.process_inner(input, mix, None).0
    }

    /// Process one frame while reporting each serial stage's input and output magnitudes into a
    /// caller-owned fixed array. This is the production editor's signal-on-curve observation path;
    /// it adds no state, allocation, or editor-selected-stage input to the DSP.
    pub fn process_with_stage_levels(
        &mut self,
        input: [f32; 2],
        mix: f32,
        levels: &mut [[f32; 2]; MAX_STAGES],
    ) -> ([f32; 2], usize) {
        self.process_inner(input, mix, Some(levels))
    }

    fn process_inner(
        &mut self,
        input: [f32; 2],
        mix: f32,
        mut levels: Option<&mut [[f32; 2]; MAX_STAGES]>,
    ) -> ([f32; 2], usize) {
        let dry = [finite(input[0]), finite(input[1])];
        let mix = if mix.is_finite() {
            mix.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if mix == 0.0 {
            if !self.parked {
                for stage in &mut self.stages {
                    stage.settle_to_silence();
                }
            }
            self.parked = true;
            return (dry, 0);
        }

        self.parked = false;
        let mut wet = dry;
        for (index, stage) in self.stages.iter_mut().enumerate() {
            if let Some(levels) = levels.as_deref_mut() {
                levels[index][0] = wet[0].abs().max(wet[1].abs()).min(1.0);
            }
            wet = stage.process(wet);
            if let Some(levels) = levels.as_deref_mut() {
                levels[index][1] = wet[0].abs().max(wet[1].abs()).min(1.0);
            }
        }
        (
            [
                dry[0] + (wet[0] - dry[0]) * mix,
                dry[1] + (wet[1] - dry[1]) * mix,
            ],
            self.stages.len(),
        )
    }

    /// Set every stateful stage to its exact silent destination without processing a sample.
    ///
    /// The plugin uses this when Mix is exactly zero or an entire block is exactly silent. This is
    /// also the destination reached by `process(_, 0.0)`, exposed so the shell can park two engines
    /// during a model transition without manufacturing audio.
    pub fn settle_to_silence(&mut self) {
        if !self.parked {
            for stage in &mut self.stages {
                stage.settle_to_silence();
            }
        }
        self.parked = true;
    }

    pub fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
        self.parked = false;
    }

    /// Carry the unchanged prefix's realtime history into a replacement chain. Carry stops at the
    /// first changed stage because every later stage then has a changed upstream signal and must earn
    /// new history while the two chains run together. With gain-after-curve smoothing, a curve edit
    /// changes its own state law too; it therefore starts the primed suffix rather than carrying.
    pub fn carry_compatible_prefix_history_from(&mut self, source: &Self) -> usize {
        let mut carried = 0;
        for (new, old) in self.stages.iter_mut().zip(&source.stages) {
            if !new.carry_history_from(old) {
                break;
            }
            carried += 1;
        }
        carried
    }

    /// Curve-derived static compensation for a full-scale sine reference.
    ///
    /// This is intentionally a nominal law rather than a claim about programme loudness: detector
    /// ballistics and the input distribution are unknowable from the curve alone. Sampling the
    /// complete static serial response is robust for non-monotonic shapers whose endpoint may be
    /// zero, and the bound prevents a near-silent curve from requesting unbounded gain.
    pub fn nominal_makeup_gain(&self) -> f32 {
        self.nominal_makeup_gain
    }

    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }

    pub fn is_parked(&self) -> bool {
        self.parked
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineError {
    TooManyStages,
    Stage(StageError),
}

impl core::fmt::Display for EngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooManyStages => f.write_str("the stage count exceeds MAX_STAGES"),
            Self::Stage(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for EngineError {}

fn nominal_makeup_gain(stages: &[Stage]) -> f32 {
    let mut input_energy = 0.0_f64;
    let mut output_energy = 0.0_f64;
    for index in 0..MAKEUP_REFERENCE_SAMPLES {
        // Midpoints avoid assigning special weight to a discontinuity exactly at a sample edge.
        let phase = core::f64::consts::TAU * (index as f64 + 0.5) / MAKEUP_REFERENCE_SAMPLES as f64;
        let input = phase.sin().abs() as f32;
        let output = stages
            .iter()
            .fold(input, |level, stage| stage.static_output(level));
        input_energy += f64::from(input) * f64::from(input);
        output_energy += f64::from(output) * f64::from(output);
    }
    if output_energy <= f64::EPSILON {
        return MAX_MAKEUP_GAIN;
    }
    let gain = (input_energy / output_energy).sqrt() as f32;
    let gain = gain.clamp(1.0 / MAX_MAKEUP_GAIN, MAX_MAKEUP_GAIN);
    if (gain - 1.0).abs() < 1.0e-6 {
        1.0
    } else {
        gain
    }
}

fn finite(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}
