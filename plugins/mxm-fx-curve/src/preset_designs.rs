//! Reviewable source table for the sixty factory curve stacks.
//!
//! Detector stages interpret X/Y as input/output level; memoryless stages interpret them as sample
//! magnitude. Factory generation stores the complete versioned stack plus Mix, so no sound depends
//! on whatever model happened to be loaded before it.

use crate::model::{
    CurveStackState, HandleState, HandlesState, PointModeState, PointState, SCHEMA_VERSION,
    StageModeState, StageState,
};
use mxm_preset::{Category, Preset, Value};
use nice_plug::prelude::Param;

#[derive(Clone, Copy)]
pub enum PointDesign {
    Curve(f32, f32),
    Linear(f32, f32),
    Handles {
        x: f32,
        y: f32,
        incoming: (f32, f32),
        outgoing: (f32, f32),
    },
}

#[derive(Clone, Copy)]
pub struct StageDesign {
    pub detector: Option<(f32, f32)>,
    pub points: &'static [PointDesign],
}

#[derive(Clone, Copy)]
pub struct Design {
    pub name: &'static str,
    pub category: Category,
    pub mix: f32,
    pub stages: &'static [StageDesign],
}

const fn memoryless(points: &'static [PointDesign]) -> StageDesign {
    StageDesign {
        detector: None,
        points,
    }
}

const fn detector(attack_ms: f32, release_ms: f32, points: &'static [PointDesign]) -> StageDesign {
    StageDesign {
        detector: Some((attack_ms, release_ms)),
        points,
    }
}

use PointDesign::{Curve as C, Handles as H, Linear as L};

const IDENTITY: &[PointDesign] = &[C(0.0, 0.0), C(1.0, 1.0)];
const COMP_GENTLE: &[PointDesign] = &[C(0.0, 0.0), C(0.35, 0.35), C(0.70, 0.58), C(1.0, 0.72)];
const COMP_GLUE: &[PointDesign] = &[C(0.0, 0.0), C(0.25, 0.25), C(0.55, 0.50), C(1.0, 0.76)];
const COMP_PUNCH: &[PointDesign] = &[L(0.0, 0.0), L(0.45, 0.45), L(0.72, 0.60), L(1.0, 0.67)];
const COMP_HARD: &[PointDesign] = &[L(0.0, 0.0), L(0.30, 0.30), L(0.55, 0.45), L(1.0, 0.56)];
const PEAK_TAME: &[PointDesign] = &[C(0.0, 0.0), C(0.58, 0.58), C(0.78, 0.68), C(1.0, 0.72)];
const LIMIT_SOFT: &[PointDesign] = &[C(0.0, 0.0), C(0.55, 0.55), C(0.75, 0.68), C(1.0, 0.70)];
const LIMIT_HARD: &[PointDesign] = &[L(0.0, 0.0), L(0.62, 0.62), L(0.70, 0.68), L(1.0, 0.68)];
const LIMIT_LOW: &[PointDesign] = &[L(0.0, 0.0), L(0.42, 0.42), L(0.50, 0.48), L(1.0, 0.48)];
const UPWARD: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.04, 0.12),
    C(0.18, 0.32),
    C(0.50, 0.62),
    C(1.0, 1.0),
];
const UPWARD_DETAIL: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.02, 0.08),
    C(0.10, 0.24),
    C(0.35, 0.52),
    C(1.0, 1.0),
];
const EXPAND_GENTLE: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.18, 0.09),
    C(0.42, 0.31),
    C(0.70, 0.66),
    C(1.0, 1.0),
];
const EXPAND_HARD: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.20, 0.03),
    L(0.42, 0.22),
    L(0.62, 0.58),
    L(1.0, 1.0),
];
const GATE_SOFT: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.07, 0.0),
    C(0.14, 0.08),
    C(0.28, 0.25),
    C(1.0, 1.0),
];
const GATE_HARD: &[PointDesign] = &[L(0.0, 0.0), L(0.10, 0.0), L(0.10, 0.10), L(1.0, 1.0)];
const GATE_DRUM: &[PointDesign] = &[L(0.0, 0.0), L(0.18, 0.0), L(0.18, 0.18), L(1.0, 1.0)];
const GATE_BREATH: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.035, 0.0),
    C(0.075, 0.035),
    C(0.16, 0.14),
    C(1.0, 1.0),
];
const FLOOR_SUPPRESS: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.10, 0.025),
    C(0.25, 0.15),
    C(0.50, 0.45),
    C(1.0, 1.0),
];
const CREST_RIDER: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.08, 0.14),
    C(0.35, 0.42),
    C(0.70, 0.65),
    C(1.0, 0.78),
];

const SAT_SOFT: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.25, 0.34),
    C(0.55, 0.62),
    C(0.80, 0.78),
    C(1.0, 0.84),
];
const SAT_ROUND: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.15, 0.24),
    C(0.40, 0.52),
    C(0.72, 0.75),
    C(1.0, 0.88),
];
const CLIP_HARD: &[PointDesign] = &[L(0.0, 0.0), L(0.62, 0.82), L(1.0, 0.82)];
const CLIP_DENSE: &[PointDesign] = &[L(0.0, 0.0), L(0.32, 0.72), L(1.0, 0.72)];
const DRIVE_GENTLE: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.18, 0.30),
    C(0.48, 0.62),
    C(0.78, 0.82),
    C(1.0, 0.90),
];
const DRIVE_HOT: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.08, 0.28),
    C(0.25, 0.58),
    C(0.55, 0.82),
    C(1.0, 0.94),
];
const FUZZ: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.035, 0.28),
    L(0.12, 0.68),
    L(0.30, 0.92),
    L(1.0, 0.98),
];
const SQUARE_MAKER: &[PointDesign] = &[L(0.0, 0.0), L(0.015, 0.0), L(0.015, 1.0), L(1.0, 1.0)];
const DEAD_ZONE: &[PointDesign] = &[L(0.0, 0.0), L(0.18, 0.0), L(0.32, 0.20), L(1.0, 1.0)];
const CROSSOVER: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.10, 0.0),
    L(0.22, 0.04),
    L(0.46, 0.52),
    L(1.0, 1.0),
];
const FOLD_ONE: &[PointDesign] = &[L(0.0, 0.0), L(0.42, 0.88), L(0.72, 0.22), L(1.0, 0.74)];
const FOLD_TWO: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.24, 0.82),
    L(0.47, 0.12),
    L(0.72, 0.88),
    L(1.0, 0.18),
];
const FOLD_THREE: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.17, 0.78),
    L(0.34, 0.08),
    L(0.52, 0.86),
    L(0.70, 0.12),
    L(0.86, 0.92),
    L(1.0, 0.22),
];
const TRIANGLE: &[PointDesign] = &[L(0.0, 0.0), L(0.50, 1.0), L(1.0, 0.0)];
const S_CURVE: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.18, 0.08),
    C(0.50, 0.50),
    C(0.82, 0.92),
    C(1.0, 1.0),
];
const CONCAVE: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.08, 0.25),
    C(0.28, 0.52),
    C(0.60, 0.78),
    C(1.0, 1.0),
];
const CONVEX: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.25, 0.06),
    C(0.55, 0.28),
    C(0.80, 0.62),
    C(1.0, 1.0),
];
const FOUR_STEP: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.125, 0.0),
    L(0.125, 0.25),
    L(0.375, 0.25),
    L(0.375, 0.50),
    L(0.625, 0.50),
    L(0.625, 0.75),
    L(0.875, 0.75),
    L(0.875, 1.0),
    L(1.0, 1.0),
];
const EIGHT_STEP: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.0625, 0.0),
    L(0.0625, 0.125),
    L(0.1875, 0.125),
    L(0.1875, 0.25),
    L(0.3125, 0.25),
    L(0.3125, 0.375),
    L(0.4375, 0.375),
    L(0.4375, 0.50),
    L(0.5625, 0.50),
    L(0.5625, 0.625),
    L(0.6875, 0.625),
    L(0.6875, 0.75),
    L(0.8125, 0.75),
    L(0.8125, 0.875),
    L(0.9375, 0.875),
    L(0.9375, 1.0),
    L(1.0, 1.0),
];
const COMPARATOR: &[PointDesign] = &[L(0.0, 0.0), L(0.05, 0.0), L(0.05, 1.0), L(1.0, 1.0)];
const STAIR_DRIVE: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.12, 0.22),
    L(0.28, 0.22),
    L(0.28, 0.48),
    L(0.52, 0.48),
    L(0.52, 0.72),
    L(0.78, 0.72),
    L(0.78, 0.92),
    L(1.0, 0.92),
];
const BROKEN_CONE: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.16, 0.25),
    L(0.34, 0.18),
    L(0.56, 0.72),
    L(0.76, 0.58),
    L(1.0, 0.92),
];
const NEEDLE_CLIP: &[PointDesign] = &[
    C(0.0, 0.0),
    C(0.10, 0.18),
    C(0.42, 0.50),
    C(0.72, 0.96),
    C(0.82, 0.78),
    C(1.0, 0.80),
];
const HOLLOW_CLIP: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.12, 0.22),
    L(0.32, 0.12),
    L(0.58, 0.76),
    L(1.0, 0.76),
];
const RANGE_CRUSH: &[PointDesign] = &[
    L(0.0, 0.0),
    L(0.08, 0.0),
    L(0.30, 0.52),
    L(0.60, 0.68),
    L(1.0, 0.72),
];
const BEZIER: &[PointDesign] = &[
    C(0.0, 0.0),
    H {
        x: 0.50,
        y: 0.42,
        incoming: (-0.16, -0.02),
        outgoing: (0.16, 0.24),
    },
    C(1.0, 1.0),
];

pub const DESIGNS: &[Design] = &[
    Design {
        name: "Vocal leveler",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(20.0, 250.0, COMP_GENTLE)],
    },
    Design {
        name: "Bus glue",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(30.0, 320.0, COMP_GLUE)],
    },
    Design {
        name: "Punch compressor",
        category: Category::Percussion,
        mix: 1.0,
        stages: &[detector(28.0, 120.0, COMP_PUNCH)],
    },
    Design {
        name: "Slow leveller",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(85.0, 900.0, COMP_GENTLE)],
    },
    Design {
        name: "Fast peak tamer",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(1.0, 85.0, PEAK_TAME)],
    },
    Design {
        name: "Soft limiter",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(2.0, 140.0, LIMIT_SOFT)],
    },
    Design {
        name: "Brickwall limiter",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(0.2, 90.0, LIMIT_HARD), memoryless(CLIP_HARD)],
    },
    Design {
        name: "Transient clamp",
        category: Category::Percussion,
        mix: 1.0,
        stages: &[detector(0.1, 38.0, LIMIT_LOW)],
    },
    Design {
        name: "Pumping compressor",
        category: Category::Sequence,
        mix: 1.0,
        stages: &[detector(4.0, 1_200.0, COMP_HARD)],
    },
    Design {
        name: "Parallel squeeze",
        category: Category::Fx,
        mix: 0.58,
        stages: &[detector(2.0, 260.0, COMP_HARD)],
    },
    Design {
        name: "Upward lift",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(35.0, 420.0, UPWARD)],
    },
    Design {
        name: "Low level detail",
        category: Category::Fx,
        mix: 0.82,
        stages: &[detector(100.0, 850.0, UPWARD_DETAIL)],
    },
    Design {
        name: "Gentle expander",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(20.0, 260.0, EXPAND_GENTLE)],
    },
    Design {
        name: "Downward expander",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(10.0, 180.0, EXPAND_HARD)],
    },
    Design {
        name: "Noise gate",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(2.0, 120.0, GATE_SOFT)],
    },
    Design {
        name: "Hard gate",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(0.5, 70.0, GATE_HARD)],
    },
    Design {
        name: "Drum gate",
        category: Category::Percussion,
        mix: 1.0,
        stages: &[detector(0.2, 45.0, GATE_DRUM)],
    },
    Design {
        name: "Breath gate",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(8.0, 300.0, GATE_BREATH)],
    },
    Design {
        name: "Floor suppressor",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(25.0, 520.0, FLOOR_SUPPRESS)],
    },
    Design {
        name: "Crest rider",
        category: Category::Fx,
        mix: 0.9,
        stages: &[detector(8.0, 180.0, CREST_RIDER)],
    },
    Design {
        name: "Soft saturation",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(SAT_SOFT)],
    },
    Design {
        name: "Rounded drive",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(SAT_ROUND)],
    },
    Design {
        name: "Hard clip",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(CLIP_HARD)],
    },
    Design {
        name: "Dense clip",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(CLIP_DENSE)],
    },
    Design {
        name: "Gentle overdrive",
        category: Category::Fx,
        mix: 0.78,
        stages: &[memoryless(DRIVE_GENTLE)],
    },
    Design {
        name: "Hot overdrive",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(DRIVE_HOT)],
    },
    Design {
        name: "Fuzz",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(FUZZ)],
    },
    Design {
        name: "Square maker",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(SQUARE_MAKER)],
    },
    Design {
        name: "Dead zone",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(DEAD_ZONE)],
    },
    Design {
        name: "Crossover crunch",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(CROSSOVER)],
    },
    Design {
        name: "Single fold",
        category: Category::Fx,
        mix: 0.86,
        stages: &[memoryless(FOLD_ONE)],
    },
    Design {
        name: "Double fold",
        category: Category::Fx,
        mix: 0.82,
        stages: &[memoryless(FOLD_TWO)],
    },
    Design {
        name: "Triple fold",
        category: Category::Fx,
        mix: 0.72,
        stages: &[memoryless(FOLD_THREE)],
    },
    Design {
        name: "Triangle shaper",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(TRIANGLE)],
    },
    Design {
        name: "S curve",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(S_CURVE)],
    },
    Design {
        name: "Concave boost",
        category: Category::Fx,
        mix: 0.7,
        stages: &[memoryless(CONCAVE)],
    },
    Design {
        name: "Convex cut",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(CONVEX)],
    },
    Design {
        name: "Four step quantizer",
        category: Category::Fx,
        mix: 0.82,
        stages: &[memoryless(FOUR_STEP)],
    },
    Design {
        name: "Eight step quantizer",
        category: Category::Fx,
        mix: 0.9,
        stages: &[memoryless(EIGHT_STEP)],
    },
    Design {
        name: "Comparator",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(COMPARATOR)],
    },
    Design {
        name: "Staircase drive",
        category: Category::Fx,
        mix: 0.88,
        stages: &[memoryless(STAIR_DRIVE)],
    },
    Design {
        name: "Broken cone",
        category: Category::Fx,
        mix: 0.76,
        stages: &[memoryless(BROKEN_CONE)],
    },
    Design {
        name: "Needle clip",
        category: Category::Fx,
        mix: 0.82,
        stages: &[memoryless(NEEDLE_CLIP)],
    },
    Design {
        name: "Hollow clip",
        category: Category::Fx,
        mix: 0.9,
        stages: &[memoryless(HOLLOW_CLIP)],
    },
    Design {
        name: "Range crush",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(RANGE_CRUSH)],
    },
    Design {
        name: "Compressor into saturation",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(12.0, 220.0, COMP_GLUE), memoryless(SAT_SOFT)],
    },
    Design {
        name: "Saturation into compressor",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(SAT_SOFT), detector(12.0, 220.0, COMP_GLUE)],
    },
    Design {
        name: "Gate into drive",
        category: Category::Fx,
        mix: 1.0,
        stages: &[detector(2.0, 110.0, GATE_SOFT), memoryless(DRIVE_HOT)],
    },
    Design {
        name: "Drive into gate",
        category: Category::Fx,
        mix: 0.9,
        stages: &[memoryless(DRIVE_HOT), detector(2.0, 110.0, GATE_SOFT)],
    },
    Design {
        name: "Expander into limiter",
        category: Category::Fx,
        mix: 1.0,
        stages: &[
            detector(8.0, 160.0, EXPAND_GENTLE),
            detector(0.8, 90.0, LIMIT_SOFT),
        ],
    },
    Design {
        name: "Two stage limiter",
        category: Category::Fx,
        mix: 1.0,
        stages: &[
            detector(8.0, 220.0, PEAK_TAME),
            detector(0.2, 70.0, LIMIT_HARD),
        ],
    },
    Design {
        name: "Soft then hard",
        category: Category::Fx,
        mix: 1.0,
        stages: &[memoryless(SAT_ROUND), memoryless(CLIP_HARD)],
    },
    Design {
        name: "Fold then clip",
        category: Category::Fx,
        mix: 0.82,
        stages: &[memoryless(FOLD_ONE), memoryless(CLIP_HARD)],
    },
    Design {
        name: "Clip then fold",
        category: Category::Fx,
        mix: 0.72,
        stages: &[memoryless(CLIP_HARD), memoryless(FOLD_ONE)],
    },
    Design {
        name: "Detail then glue",
        category: Category::Fx,
        mix: 0.84,
        stages: &[
            detector(70.0, 700.0, UPWARD),
            detector(25.0, 320.0, COMP_GLUE),
        ],
    },
    Design {
        name: "Pump and crunch",
        category: Category::Sequence,
        mix: 0.88,
        stages: &[detector(3.0, 1_100.0, COMP_HARD), memoryless(HOLLOW_CLIP)],
    },
    Design {
        name: "Gate comp clip",
        category: Category::Percussion,
        mix: 1.0,
        stages: &[
            detector(0.5, 55.0, GATE_DRUM),
            detector(8.0, 140.0, COMP_PUNCH),
            memoryless(CLIP_HARD),
        ],
    },
    Design {
        name: "Five stage crusher",
        category: Category::Fx,
        mix: 0.7,
        stages: &[
            memoryless(CONVEX),
            memoryless(STAIR_DRIVE),
            memoryless(FOLD_ONE),
            detector(1.0, 80.0, LIMIT_LOW),
            memoryless(CLIP_DENSE),
        ],
    },
    Design {
        name: "Unity curve",
        category: Category::Template,
        mix: 1.0,
        stages: &[memoryless(IDENTITY)],
    },
    Design {
        name: "Manual bezier",
        category: Category::Template,
        mix: 1.0,
        stages: &[memoryless(BEZIER)],
    },
];

pub fn state(design: &Design) -> CurveStackState {
    CurveStackState {
        schema_version: SCHEMA_VERSION,
        stages: design
            .stages
            .iter()
            .map(|stage| StageState {
                mode: stage.detector.map_or(
                    StageModeState::Memoryless,
                    |(attack_ms, release_ms)| StageModeState::Detector {
                        attack_ms,
                        release_ms,
                    },
                ),
                points: stage.points.iter().copied().map(point).collect(),
            })
            .collect(),
    }
}

fn point(point: PointDesign) -> PointState {
    match point {
        PointDesign::Curve(x, y) => PointState::curve(x, y),
        PointDesign::Linear(x, y) => PointState {
            x,
            y,
            mode: PointModeState::Linear,
            handles: None,
        },
        PointDesign::Handles {
            x,
            y,
            incoming,
            outgoing,
        } => PointState {
            x,
            y,
            mode: PointModeState::Handles,
            handles: Some(HandlesState {
                incoming: HandleState {
                    dx: incoming.0,
                    dy: incoming.1,
                },
                outgoing: HandleState {
                    dx: outgoing.0,
                    dy: outgoing.1,
                },
            }),
        },
    }
}

/// Compile the source table into complete factory files.
pub fn generate() -> Vec<(String, String)> {
    let params = crate::params::MxmFxCurveParams::default();
    let mut generated = Vec::with_capacity(DESIGNS.len());
    for design in DESIGNS {
        let mut preset = Preset::init(&params);
        preset.name = design.name.to_owned();
        preset.category = design.category;
        let normalized = params.mix.preview_normalized(design.mix);
        preset.params.insert(
            "mix".to_owned(),
            Value {
                v: normalized,
                text: params.mix.normalized_value_to_string(normalized, true),
            },
        );
        preset.state = Some(serde_json::to_value(state(design)).expect("curve state serializes"));
        generated.push((slug(design.name), preset.to_json() + "\n"));
    }
    assert_eq!(generated.len(), 60);
    generated
}

pub fn slug(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
