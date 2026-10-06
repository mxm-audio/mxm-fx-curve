//! Authored point curves and their validation.

use core::fmt;

/// The largest authored point set accepted by one stage.
///
/// Chosen as a safety ceiling, not a recommendation. Sixty-four already permits detail far below
/// the editor's pointer floor; C1's transition-cost measurement prices the corresponding prepared
/// tables before this becomes a shipped state limit.
pub const MAX_POINTS: usize = 64;

/// How a control point shapes its neighboring segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointMode {
    /// Shape-preserving automatic curvature.
    Curve,
    /// Both adjacent segments are straight, admitting a deliberate corner.
    Linear,
    /// Incoming and outgoing cubic controls are authored explicitly.
    Handles,
}

/// A cubic control stored as an offset from its point in normalized curve coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handle {
    pub dx: f32,
    pub dy: f32,
}

impl Handle {
    pub const ZERO: Self = Self { dx: 0.0, dy: 0.0 };
}

/// One authored point. Both axes are normalized to `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlPoint {
    pub x: f32,
    pub y: f32,
    pub mode: PointMode,
    pub incoming: Handle,
    pub outgoing: Handle,
}

impl ControlPoint {
    pub const fn curve(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            mode: PointMode::Curve,
            incoming: Handle::ZERO,
            outgoing: Handle::ZERO,
        }
    }

    pub const fn linear(x: f32, y: f32) -> Self {
        Self {
            mode: PointMode::Linear,
            ..Self::curve(x, y)
        }
    }

    pub const fn handles(x: f32, y: f32, incoming: Handle, outgoing: Handle) -> Self {
        Self {
            x,
            y,
            mode: PointMode::Handles,
            incoming,
            outgoing,
        }
    }
}

/// The durable, framework-free shape authored by the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    points: Vec<ControlPoint>,
}

impl Default for Curve {
    fn default() -> Self {
        Self::identity()
    }
}

impl Curve {
    pub fn identity() -> Self {
        Self {
            points: vec![ControlPoint::curve(0.0, 0.0), ControlPoint::curve(1.0, 1.0)],
        }
    }

    pub fn new(points: Vec<ControlPoint>) -> Result<Self, CurveError> {
        validate(&points)?;
        Ok(Self { points })
    }

    pub fn points(&self) -> &[ControlPoint] {
        &self.points
    }

    pub fn has_discontinuity(&self) -> bool {
        self.points.windows(2).any(|pair| pair[0].x == pair[1].x)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveError {
    TooFewPoints,
    TooManyPoints,
    NonFinite,
    OutOfRange,
    ReversedX,
    InvalidBlackEndpoint,
    InvalidWhiteEndpoint,
    HandleCrossesSegment,
    HandleOutOfRange,
}

impl fmt::Display for CurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TooFewPoints => "a curve needs at least two points",
            Self::TooManyPoints => "the curve exceeds MAX_POINTS",
            Self::NonFinite => "curve coordinates and handles must be finite",
            Self::OutOfRange => "curve points must stay in the normalized unit square",
            Self::ReversedX => "point X positions must be nondecreasing",
            Self::InvalidBlackEndpoint => {
                "the black endpoint must remain on the left or bottom edge"
            }
            Self::InvalidWhiteEndpoint => "the white endpoint must remain on the top or right edge",
            Self::HandleCrossesSegment => "a manual handle crossed its neighboring segment",
            Self::HandleOutOfRange => "a manual handle left the normalized unit square",
        };
        f.write_str(message)
    }
}

impl std::error::Error for CurveError {}

fn validate(points: &[ControlPoint]) -> Result<(), CurveError> {
    if points.len() < 2 {
        return Err(CurveError::TooFewPoints);
    }
    if points.len() > MAX_POINTS {
        return Err(CurveError::TooManyPoints);
    }

    for point in points {
        if !point.x.is_finite()
            || !point.y.is_finite()
            || !point.incoming.dx.is_finite()
            || !point.incoming.dy.is_finite()
            || !point.outgoing.dx.is_finite()
            || !point.outgoing.dy.is_finite()
        {
            return Err(CurveError::NonFinite);
        }
        if !(0.0..=1.0).contains(&point.x) || !(0.0..=1.0).contains(&point.y) {
            return Err(CurveError::OutOfRange);
        }
    }
    if points.windows(2).any(|pair| pair[0].x > pair[1].x) {
        return Err(CurveError::ReversedX);
    }

    let first = points[0];
    if first.x != 0.0 && first.y != 0.0 {
        return Err(CurveError::InvalidBlackEndpoint);
    }
    let last = points[points.len() - 1];
    if last.x != 1.0 && last.y != 1.0 {
        return Err(CurveError::InvalidWhiteEndpoint);
    }

    for (index, point) in points.iter().enumerate() {
        if point.mode != PointMode::Handles {
            continue;
        }
        if index > 0 {
            let handle_x = point.x + point.incoming.dx;
            let handle_y = point.y + point.incoming.dy;
            if handle_x < points[index - 1].x || handle_x > point.x {
                return Err(CurveError::HandleCrossesSegment);
            }
            if !(0.0..=1.0).contains(&handle_y) {
                return Err(CurveError::HandleOutOfRange);
            }
        }
        if index + 1 < points.len() {
            let handle_x = point.x + point.outgoing.dx;
            let handle_y = point.y + point.outgoing.dy;
            if handle_x < point.x || handle_x > points[index + 1].x {
                return Err(CurveError::HandleCrossesSegment);
            }
            if !(0.0..=1.0).contains(&handle_y) {
                return Err(CurveError::HandleOutOfRange);
            }
        }
    }

    // Inverted cubic evaluation assumes X(t) is monotone. Each handle staying inside the segment is
    // not enough: opposing handles may still cross one another inside it.
    for pair in points.windows(2) {
        let span = pair[1].x - pair[0].x;
        let outgoing_x = if pair[0].mode == PointMode::Handles {
            pair[0].x + pair[0].outgoing.dx
        } else {
            pair[0].x + span / 3.0
        };
        let incoming_x = if pair[1].mode == PointMode::Handles {
            pair[1].x + pair[1].incoming.dx
        } else {
            pair[1].x - span / 3.0
        };
        if outgoing_x > incoming_x {
            return Err(CurveError::HandleCrossesSegment);
        }
    }
    Ok(())
}
