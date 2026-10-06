//! Realtime stage processing: symmetric memoryless shaping or linked peak dynamics.

use crate::table::CurveTable;

/// The accepted host-rate envelope for the framework-free engine.
pub const MIN_SAMPLE_RATE: f32 = 1_000.0;
pub const MAX_SAMPLE_RATE: f32 = 768_000.0;
/// Chosen numeric bound for upward expansion: about +24 dB.
pub const MAX_GAIN: f32 = 16.0;
/// Chosen final-stage numeric ceiling. It is deliberately the same magnitude as the maximum gain:
/// normalized authored curves cannot request more, while hostile finite input still cannot overflow.
pub const MAX_OUTPUT: f32 = 16.0;
/// Chosen ballistics bounds. The floor follows the fastest published range in
/// `research:effects/dynamics-processing.md` §5; the ceiling keeps coefficient conversion finite.
pub const MIN_TIME_MS: f32 = 0.1;
pub const MAX_TIME_MS: f32 = 5_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntialiasMode {
    /// Production selection: direct lookup for continuous curves, residual ADAA for an equal-X
    /// discontinuity. The choice follows C1's alias-and-reference measurement.
    Auto,
    /// Measurement baseline. Product code uses [`StageSpec::memoryless`] instead.
    Direct,
    /// First-order antiderivative antialiasing of the nonlinear residual. Production `Auto` selects
    /// it only for an equal-X discontinuity; explicit use is for the comparison harness.
    Adaa,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StageMode {
    Memoryless {
        antialias: AntialiasMode,
    },
    /// Linked peak detection, with attack/release smoothing on gain after the curve.
    Detector {
        attack_ms: f32,
        release_ms: f32,
    },
}

#[derive(Debug, Clone)]
pub struct StageSpec {
    pub curve: crate::model::Curve,
    pub mode: StageMode,
}

impl StageSpec {
    /// A production memoryless stage. Antialiasing is derived from the authored shape and is not
    /// state: continuous curves use direct lookup; an equal-X discontinuity uses residual ADAA.
    pub fn memoryless(curve: crate::model::Curve) -> Self {
        Self {
            curve,
            mode: StageMode::Memoryless {
                antialias: AntialiasMode::Auto,
            },
        }
    }

    pub fn detector(curve: crate::model::Curve, attack_ms: f32, release_ms: f32) -> Self {
        Self {
            curve,
            mode: StageMode::Detector {
                attack_ms,
                release_ms,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Stage {
    curve: CurveTable,
    mode: RuntimeMode,
}

#[derive(Debug, Clone)]
enum RuntimeMode {
    Memoryless {
        antialias: AntialiasMode,
        previous: [f32; 2],
        previous_primitive: [f64; 2],
    },
    Detector {
        attack_coefficient: f64,
        release_coefficient: f64,
        gain: f64,
    },
}

impl Stage {
    pub fn prepare(spec: &StageSpec, sample_rate: f32) -> Result<Self, StageError> {
        if !sample_rate.is_finite() || !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(StageError::InvalidSampleRate);
        }
        let mode = match spec.mode {
            StageMode::Memoryless { antialias } => RuntimeMode::Memoryless {
                antialias: match antialias {
                    AntialiasMode::Auto if spec.curve.has_discontinuity() => AntialiasMode::Adaa,
                    AntialiasMode::Auto => AntialiasMode::Direct,
                    measured => measured,
                },
                previous: [0.0; 2],
                previous_primitive: [0.0; 2],
            },
            StageMode::Detector {
                attack_ms,
                release_ms,
            } => {
                if !attack_ms.is_finite()
                    || !release_ms.is_finite()
                    || !(MIN_TIME_MS..=MAX_TIME_MS).contains(&attack_ms)
                    || !(MIN_TIME_MS..=MAX_TIME_MS).contains(&release_ms)
                {
                    return Err(StageError::InvalidBallistics);
                }
                let curve = CurveTable::prepare(&spec.curve);
                let gain = silent_gain(&curve) as f64;
                return Ok(Self {
                    curve,
                    mode: RuntimeMode::Detector {
                        attack_coefficient: coefficient(attack_ms, sample_rate),
                        release_coefficient: coefficient(release_ms, sample_rate),
                        gain,
                    },
                });
            }
        };
        Ok(Self {
            curve: CurveTable::prepare(&spec.curve),
            mode,
        })
    }

    pub fn reset(&mut self) {
        self.settle_to_silence();
    }

    /// Put every targeted state at the destination reached by an unbroken passage of silence.
    /// Freezing a detector's old envelope would return stale gain after a parked Mix-zero span.
    pub fn settle_to_silence(&mut self) {
        match &mut self.mode {
            RuntimeMode::Memoryless {
                previous,
                previous_primitive,
                ..
            } => {
                *previous = [0.0; 2];
                *previous_primitive = [0.0; 2];
            }
            RuntimeMode::Detector { gain, .. } => *gain = silent_gain(&self.curve) as f64,
        }
    }

    pub fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let frame = [finite(frame[0]), finite(frame[1])];
        match &mut self.mode {
            RuntimeMode::Memoryless {
                antialias,
                previous,
                previous_primitive,
            } => {
                if self.curve.is_identity() {
                    *previous = frame;
                    *previous_primitive = [0.0; 2];
                    return frame;
                }
                let mut output = [0.0; 2];
                for channel in 0..2 {
                    let input = frame[channel].clamp(-1.0, 1.0);
                    output[channel] = match antialias {
                        AntialiasMode::Direct => shape_direct(&self.curve, input),
                        AntialiasMode::Adaa => {
                            let current_primitive = shape_primitive(&self.curve, input);
                            let shaped = shape_adaa(
                                &self.curve,
                                previous[channel],
                                previous_primitive[channel],
                                input,
                                current_primitive,
                            );
                            previous[channel] = input;
                            previous_primitive[channel] = current_primitive;
                            shaped.clamp(-MAX_OUTPUT, MAX_OUTPUT)
                        }
                        AntialiasMode::Auto => unreachable!("Auto resolves during preparation"),
                    };
                }
                output
            }
            RuntimeMode::Detector {
                attack_coefficient,
                release_coefficient,
                gain,
            } => {
                // Max linking preserves the stereo image: one channel can ask for more reduction,
                // but the channels never receive different gains.
                let detected = frame[0].abs().max(frame[1].abs());
                let target_gain = if detected > 1.0e-12 {
                    (self.curve.evaluate(detected.min(1.0)) / detected).clamp(0.0, MAX_GAIN)
                } else {
                    silent_gain(&self.curve)
                } as f64;
                // Smooth gain after the static curve. A drawn corner can make an unsmoothed gain
                // discontinuous even when its input level was smoothed first; C1's ordering probe
                // selected this path because it bounds that post-curve control signal directly.
                let coefficient = if target_gain < *gain {
                    *attack_coefficient
                } else {
                    *release_coefficient
                };
                *gain = target_gain + coefficient * (*gain - target_gain);
                if gain.abs() < f32::MIN_POSITIVE as f64 {
                    *gain = 0.0;
                }
                let gain = (*gain).clamp(0.0, MAX_GAIN as f64) as f32;
                [
                    bounded_product(frame[0], gain),
                    bounded_product(frame[1], gain),
                ]
            }
        }
    }

    /// Static magnitude response used by off-audio stack analysis. Both runtime modes share this
    /// same authored transfer law; detector ballistics only decide how quickly its gain is reached.
    pub(crate) fn static_output(&self, magnitude: f32) -> f32 {
        self.curve.evaluate(magnitude)
    }

    pub fn curve(&self) -> &CurveTable {
        &self.curve
    }

    pub(crate) fn carry_history_from(&mut self, source: &Self) -> bool {
        if !self.curve.same_authored_shape(&source.curve) {
            return false;
        }
        match (&mut self.mode, &source.mode) {
            (
                RuntimeMode::Memoryless {
                    antialias: new_antialias,
                    previous: new_previous,
                    previous_primitive: new_primitive,
                },
                RuntimeMode::Memoryless {
                    antialias: old_antialias,
                    previous: old_previous,
                    previous_primitive: old_primitive,
                },
            ) if new_antialias == old_antialias => {
                *new_previous = *old_previous;
                *new_primitive = *old_primitive;
                true
            }
            (
                RuntimeMode::Detector {
                    attack_coefficient: new_attack,
                    release_coefficient: new_release,
                    gain: new_gain,
                },
                RuntimeMode::Detector {
                    attack_coefficient: old_attack,
                    release_coefficient: old_release,
                    gain: old_gain,
                },
            ) if new_attack.to_bits() == old_attack.to_bits()
                && new_release.to_bits() == old_release.to_bits() =>
            {
                *new_gain = *old_gain;
                true
            }
            _ => false,
        }
    }

    pub fn antialias_mode(&self) -> Option<AntialiasMode> {
        match self.mode {
            RuntimeMode::Memoryless { antialias, .. } => Some(antialias),
            RuntimeMode::Detector { .. } => None,
        }
    }

    /// The linked detector's directly computed control signal, for telemetry and strong tests.
    pub fn detector_gain(&self) -> Option<f32> {
        match self.mode {
            RuntimeMode::Detector { gain, .. } => Some(gain as f32),
            RuntimeMode::Memoryless { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageError {
    InvalidSampleRate,
    InvalidBallistics,
}

impl core::fmt::Display for StageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSampleRate => f.write_str("sample rate is outside the supported envelope"),
            Self::InvalidBallistics => {
                f.write_str("attack and release must be finite and in range")
            }
        }
    }
}

impl std::error::Error for StageError {}

fn silent_gain(curve: &CurveTable) -> f32 {
    // The local slope is the finite gain represented at zero. A left-edge endpoint above zero asks
    // for infinite upward gain mathematically; MAX_GAIN is the declared numeric/product bound.
    (curve.evaluate(1.0e-5) / 1.0e-5).clamp(0.0, MAX_GAIN)
}

fn coefficient(milliseconds: f32, sample_rate: f32) -> f64 {
    (-1.0 / (milliseconds as f64 * 0.001 * sample_rate as f64)).exp()
}

fn finite(sample: f32) -> f32 {
    if sample.is_finite() && sample.abs() >= f32::MIN_POSITIVE {
        sample
    } else {
        0.0
    }
}

fn bounded_product(sample: f32, gain: f32) -> f32 {
    (sample as f64 * gain as f64).clamp(-(MAX_OUTPUT as f64), MAX_OUTPUT as f64) as f32
}

fn shape_direct(curve: &CurveTable, input: f32) -> f32 {
    if input == 0.0 {
        return 0.0;
    }
    input.signum() * curve.evaluate(input.abs().min(1.0))
}

fn shape_primitive(curve: &CurveTable, input: f32) -> f64 {
    let magnitude = input.abs();
    let curve_area = if magnitude <= 1.0 {
        curve.integral(magnitude)
    } else {
        curve.integral(1.0) + (magnitude - 1.0) as f64 * curve.evaluate(1.0) as f64
    };
    // Antiderivative of the nonlinear residual `shape(x) - x`. Both terms are odd,
    // so their primitives from zero are even.
    curve_area - 0.5 * input as f64 * input as f64
}

fn shape_adaa(
    curve: &CurveTable,
    previous: f32,
    previous_primitive: f64,
    input: f32,
    current_primitive: f64,
) -> f32 {
    let delta = input - previous;
    let residual = if delta.abs() > 1.0e-6 {
        ((current_primitive - previous_primitive) / delta as f64) as f32
    } else {
        let middle = 0.5 * (input + previous);
        shape_direct(curve, middle) - middle
    };
    let output = input + residual;
    if output.is_finite() { output } else { 0.0 }
}
