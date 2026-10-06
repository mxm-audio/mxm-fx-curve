//! Versioned durable curve-stack state. Prepared tables are deliberately not serialized.

use mxm_fx_curve_dsp::{
    ControlPoint, Curve, CurveEngine, Handle, MAX_POINTS, MAX_STAGES, PointMode, StageSpec,
};
use serde::de::{Error as _, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveStackState {
    pub schema_version: u32,
    #[serde(deserialize_with = "deserialize_stages")]
    pub stages: Vec<StageState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageState {
    pub mode: StageModeState,
    #[serde(deserialize_with = "deserialize_points")]
    pub points: Vec<PointState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StageModeState {
    Memoryless,
    Detector { attack_ms: f32, release_ms: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointState {
    pub x: f32,
    pub y: f32,
    pub mode: PointModeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handles: Option<HandlesState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointModeState {
    Curve,
    Linear,
    Handles,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlesState {
    pub incoming: HandleState,
    pub outgoing: HandleState,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandleState {
    pub dx: f32,
    pub dy: f32,
}

impl Default for CurveStackState {
    fn default() -> Self {
        Self::identity()
    }
}

impl CurveStackState {
    /// Fresh construction and Init are one exact no-op: a single unlinked 1:1 curve.
    pub fn identity() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            stages: vec![StageState {
                mode: StageModeState::Memoryless,
                points: vec![PointState::curve(0.0, 0.0), PointState::curve(1.0, 1.0)],
            }],
        }
    }

    /// Decode and prepare only after all format-level collection bounds have been checked.
    pub fn prepare(&self, sample_rate: f32) -> Result<CurveEngine, ModelError> {
        let specs = self.stage_specs()?;
        CurveEngine::prepare(&specs, sample_rate).map_err(|_| ModelError::InvalidDsp)
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        self.stage_specs().map(|_| ())
    }

    /// Cheap deterministic identity for preset dirty comparison. The hash covers every persisted
    /// field in stage order and performs no allocation.
    pub fn fingerprint(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        hash_u32(&mut hash, self.schema_version);
        hash_u32(&mut hash, self.stages.len() as u32);
        for stage in &self.stages {
            match stage.mode {
                StageModeState::Memoryless => hash_byte(&mut hash, 0),
                StageModeState::Detector {
                    attack_ms,
                    release_ms,
                } => {
                    hash_byte(&mut hash, 1);
                    hash_u32(&mut hash, attack_ms.to_bits());
                    hash_u32(&mut hash, release_ms.to_bits());
                }
            }
            hash_u32(&mut hash, stage.points.len() as u32);
            for point in &stage.points {
                hash_u32(&mut hash, point.x.to_bits());
                hash_u32(&mut hash, point.y.to_bits());
                hash_byte(
                    &mut hash,
                    match point.mode {
                        PointModeState::Curve => 0,
                        PointModeState::Linear => 1,
                        PointModeState::Handles => 2,
                    },
                );
                if let Some(handles) = point.handles {
                    for value in [
                        handles.incoming.dx,
                        handles.incoming.dy,
                        handles.outgoing.dx,
                        handles.outgoing.dy,
                    ] {
                        hash_u32(&mut hash, value.to_bits());
                    }
                }
            }
        }
        hash
    }

    fn stage_specs(&self) -> Result<Vec<StageSpec>, ModelError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ModelError::UnknownSchema);
        }
        if self.stages.is_empty() || self.stages.len() > MAX_STAGES {
            return Err(ModelError::StageCount);
        }

        let mut specs = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            if stage.points.len() < 2 || stage.points.len() > MAX_POINTS {
                return Err(ModelError::PointCount);
            }
            let mut points = Vec::with_capacity(stage.points.len());
            for point in &stage.points {
                points.push(point.to_dsp()?);
            }
            let curve = Curve::new(points).map_err(|_| ModelError::InvalidCurve)?;
            specs.push(match stage.mode {
                StageModeState::Memoryless => StageSpec::memoryless(curve),
                StageModeState::Detector {
                    attack_ms,
                    release_ms,
                } => StageSpec::detector(curve, attack_ms, release_ms),
            });
        }
        Ok(specs)
    }
}

impl PointState {
    pub const fn curve(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            mode: PointModeState::Curve,
            handles: None,
        }
    }

    fn to_dsp(self) -> Result<ControlPoint, ModelError> {
        let zero = HandleState { dx: 0.0, dy: 0.0 };
        let (mode, incoming, outgoing) = match (self.mode, self.handles) {
            (PointModeState::Curve, None) => (PointMode::Curve, zero, zero),
            (PointModeState::Linear, None) => (PointMode::Linear, zero, zero),
            (PointModeState::Handles, Some(handles)) => {
                (PointMode::Handles, handles.incoming, handles.outgoing)
            }
            _ => return Err(ModelError::HandleMode),
        };
        Ok(ControlPoint {
            x: self.x,
            y: self.y,
            mode,
            incoming: Handle {
                dx: incoming.dx,
                dy: incoming.dy,
            },
            outgoing: Handle {
                dx: outgoing.dx,
                dy: outgoing.dy,
            },
        })
    }
}

fn hash_byte(hash: &mut u64, byte: u8) {
    *hash ^= u64::from(byte);
    *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
}

fn hash_u32(hash: &mut u64, value: u32) {
    for byte in value.to_le_bytes() {
        hash_byte(hash, byte);
    }
}

fn deserialize_stages<'de, D>(deserializer: D) -> Result<Vec<StageState>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StagesVisitor;
    impl<'de> Visitor<'de> for StagesVisitor {
        type Value = Vec<StageState>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "at most {MAX_STAGES} curve stages")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if sequence
                .size_hint()
                .is_some_and(|length| length > MAX_STAGES)
            {
                return Err(A::Error::custom(
                    "curve stage count exceeds the format bound",
                ));
            }
            let mut stages = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_STAGES));
            while let Some(stage) = sequence.next_element()? {
                if stages.len() == MAX_STAGES {
                    return Err(A::Error::custom(
                        "curve stage count exceeds the format bound",
                    ));
                }
                stages.push(stage);
            }
            Ok(stages)
        }
    }
    deserializer.deserialize_seq(StagesVisitor)
}

fn deserialize_points<'de, D>(deserializer: D) -> Result<Vec<PointState>, D::Error>
where
    D: Deserializer<'de>,
{
    struct PointsVisitor;
    impl<'de> Visitor<'de> for PointsVisitor {
        type Value = Vec<PointState>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "at most {MAX_POINTS} control points")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if sequence
                .size_hint()
                .is_some_and(|length| length > MAX_POINTS)
            {
                return Err(A::Error::custom("point count exceeds the format bound"));
            }
            let mut points = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_POINTS));
            while let Some(point) = sequence.next_element()? {
                if points.len() == MAX_POINTS {
                    return Err(A::Error::custom("point count exceeds the format bound"));
                }
                points.push(point);
            }
            Ok(points)
        }
    }
    deserializer.deserialize_seq(PointsVisitor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    UnknownSchema,
    StageCount,
    PointCount,
    HandleMode,
    InvalidCurve,
    InvalidDsp,
}
