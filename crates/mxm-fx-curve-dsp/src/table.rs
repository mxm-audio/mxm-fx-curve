//! Off-audio curve preparation and the bounded realtime lookup.

use crate::model::{ControlPoint, Curve, PointMode};

/// Intervals prepared for every non-vertical point pair.
///
/// Selected by C1's table-error harness and retained by its measured error bounds.
pub const TABLE_INTERVALS_PER_SEGMENT: usize = 1024;

#[derive(Debug, Clone)]
struct SegmentTable {
    start_x: f32,
    end_x: f32,
    values: Box<[f32]>,
    /// Integral from this segment's start to each table sample. `f64` because these areas accumulate
    /// across every segment off audio; the sample path converts only the final quotient to `f32`.
    areas: Box<[f64]>,
}

impl SegmentTable {
    fn vertical(x: f32, y: f32) -> Self {
        Self {
            start_x: x,
            end_x: x,
            values: vec![y, y].into_boxed_slice(),
            areas: vec![0.0, 0.0].into_boxed_slice(),
        }
    }

    fn evaluate(&self, x: f32) -> f32 {
        let span = self.end_x - self.start_x;
        if span == 0.0 {
            return self.values[self.values.len() - 1];
        }
        if x <= self.start_x {
            return self.values[0];
        }
        if x >= self.end_x {
            return self.values[self.values.len() - 1];
        }
        let position = (x - self.start_x) / span * (self.values.len() - 1) as f32;
        let index = position.floor() as usize;
        let fraction = position - index as f32;
        self.values[index] + (self.values[index + 1] - self.values[index]) * fraction
    }

    fn integral(&self, x: f32) -> f64 {
        let span = self.end_x - self.start_x;
        if span == 0.0 || x <= self.start_x {
            return 0.0;
        }
        if x >= self.end_x {
            return self.areas[self.areas.len() - 1];
        }
        let intervals = self.values.len() - 1;
        let position = (x - self.start_x) / span * intervals as f32;
        let index = position.floor() as usize;
        let fraction = (position - index as f32) as f64;
        let step = span as f64 / intervals as f64;
        let y0 = self.values[index] as f64;
        let y1 = self.values[index + 1] as f64;
        self.areas[index] + step * fraction * (y0 + 0.5 * (y1 - y0) * fraction)
    }

    fn area(&self) -> f64 {
        self.areas[self.areas.len() - 1]
    }
}

/// A prepared curve. Evaluation performs a binary search over point X positions and linear lookup
/// inside one table; it never walks or solves the authored spline.
#[derive(Debug, Clone)]
pub struct CurveTable {
    points: Box<[ControlPoint]>,
    segments: Box<[SegmentTable]>,
    /// Integral from zero to each point. Equal-X stacks repeat the same area.
    areas_at_point: Box<[f64]>,
    identity: bool,
}

impl CurveTable {
    pub fn prepare(curve: &Curve) -> Self {
        let points = curve.points();
        let mut segments = Vec::with_capacity(points.len() - 1);
        let mut areas_at_point = Vec::with_capacity(points.len());
        areas_at_point.push(points[0].x as f64 * points[0].y as f64);

        for index in 0..points.len() - 1 {
            let segment = prepare_segment(points, index);
            let next_area = areas_at_point[index] + segment.area();
            segments.push(segment);
            areas_at_point.push(next_area);
        }

        let identity = points.len() == 2
            && points[0] == ControlPoint::curve(0.0, 0.0)
            && points[1] == ControlPoint::curve(1.0, 1.0);

        Self {
            points: points.to_vec().into_boxed_slice(),
            segments: segments.into_boxed_slice(),
            areas_at_point: areas_at_point.into_boxed_slice(),
            identity,
        }
    }

    pub fn is_identity(&self) -> bool {
        self.identity
    }

    /// Right-continuous evaluation: the last equal-X point owns the exact X and the segment to its
    /// right. Before the first and after the last point, the endpoint value is held.
    pub fn evaluate(&self, x: f32) -> f32 {
        let x = finite_unit(x);
        if self.identity {
            return x;
        }
        let right = self.points.partition_point(|point| point.x <= x);
        if right == 0 {
            return self.points[0].y;
        }
        if right == self.points.len() {
            return self.points[self.points.len() - 1].y;
        }
        self.segments[right - 1].evaluate(x).clamp(0.0, 1.0)
    }

    /// Integral of the held-endpoint curve from zero to `x` over the normalized domain.
    pub fn integral(&self, x: f32) -> f64 {
        let x = finite_unit(x);
        let right = self.points.partition_point(|point| point.x <= x);
        if right == 0 {
            return x as f64 * self.points[0].y as f64;
        }
        if right == self.points.len() {
            let last = self.points.len() - 1;
            return self.areas_at_point[last]
                + (x - self.points[last].x) as f64 * self.points[last].y as f64;
        }
        self.areas_at_point[right - 1] + self.segments[right - 1].integral(x)
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    pub(crate) fn same_authored_shape(&self, other: &Self) -> bool {
        self.points == other.points
    }
}

fn finite_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn prepare_segment(points: &[ControlPoint], index: usize) -> SegmentTable {
    let a = points[index];
    let b = points[index + 1];
    if b.x == a.x {
        return SegmentTable::vertical(a.x, b.y);
    }

    let mut values = Vec::with_capacity(TABLE_INTERVALS_PER_SEGMENT + 1);
    for sample in 0..=TABLE_INTERVALS_PER_SEGMENT {
        let x = a.x + (b.x - a.x) * sample as f32 / TABLE_INTERVALS_PER_SEGMENT as f32;
        let value = evaluate_authored(points, index, x).clamp(0.0, 1.0);
        values.push(if value.abs() < f32::MIN_POSITIVE {
            0.0
        } else {
            value
        });
    }

    let step = (b.x - a.x) as f64 / TABLE_INTERVALS_PER_SEGMENT as f64;
    let mut areas = Vec::with_capacity(values.len());
    areas.push(0.0);
    for pair in values.windows(2) {
        let next = areas[areas.len() - 1] + 0.5 * (pair[0] as f64 + pair[1] as f64) * step;
        areas.push(next);
    }

    SegmentTable {
        start_x: a.x,
        end_x: b.x,
        values: values.into_boxed_slice(),
        areas: areas.into_boxed_slice(),
    }
}

fn evaluate_authored(points: &[ControlPoint], index: usize, x: f32) -> f32 {
    let a = points[index];
    let b = points[index + 1];
    let span = b.x - a.x;
    if x <= a.x {
        return a.y;
    }
    if x >= b.x {
        return b.y;
    }
    if span < f32::MIN_POSITIVE || a.mode == PointMode::Linear || b.mode == PointMode::Linear {
        let t = (x - a.x) / span;
        return a.y + (b.y - a.y) * t;
    }

    let (c1x, c1y) = if a.mode == PointMode::Handles {
        (
            (a.x + a.outgoing.dx).clamp(a.x, b.x),
            (a.y + a.outgoing.dy).clamp(0.0, 1.0),
        )
    } else {
        let dx = span / 3.0;
        (
            a.x + dx,
            (a.y + automatic_tangent(points, index) * dx).clamp(0.0, 1.0),
        )
    };
    let (c2x, c2y) = if b.mode == PointMode::Handles {
        (
            (b.x + b.incoming.dx).clamp(a.x, b.x),
            (b.y + b.incoming.dy).clamp(0.0, 1.0),
        )
    } else {
        let dx = span / 3.0;
        (
            b.x - dx,
            (b.y - automatic_tangent(points, index + 1) * dx).clamp(0.0, 1.0),
        )
    };

    let mut low = 0.0f32;
    let mut high = 1.0f32;
    for _ in 0..24 {
        let middle = 0.5 * (low + high);
        if cubic(middle, a.x, c1x, c2x, b.x) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    cubic(0.5 * (low + high), a.y, c1y, c2y, b.y)
}

fn cubic(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let u = 1.0 - t;
    u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
}

fn automatic_tangent(points: &[ControlPoint], index: usize) -> f32 {
    let previous = (0..index).rev().find(|&i| points[i].x < points[index].x);
    let next = (index + 1..points.len()).find(|&i| points[i].x > points[index].x);
    let stacked_before = index > 0 && points[index - 1].x == points[index].x;
    let stacked_after = index + 1 < points.len() && points[index + 1].x == points[index].x;
    let secant = |a: usize, b: usize| (points[b].y - points[a].y) / (points[b].x - points[a].x);

    if stacked_before && stacked_after {
        return 0.0;
    }
    if stacked_after {
        return previous.map_or(0.0, |before| secant(before, index));
    }
    if stacked_before {
        return next.map_or(0.0, |after| secant(index, after));
    }
    match (previous, next) {
        (None, Some(after)) => secant(index, after),
        (Some(before), None) => secant(before, index),
        (Some(before), Some(after)) => {
            let before_slope = secant(before, index);
            let after_slope = secant(index, after);
            if before_slope == 0.0
                || after_slope == 0.0
                || before_slope.signum() != after_slope.signum()
            {
                return 0.0;
            }
            let before_width = points[index].x - points[before].x;
            let after_width = points[after].x - points[index].x;
            let weight_1 = 2.0 * after_width + before_width;
            let weight_2 = after_width + 2.0 * before_width;
            (weight_1 + weight_2) / (weight_1 / before_slope + weight_2 / after_slope)
        }
        (None, None) => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Handle;

    #[test]
    fn prepared_value_and_integral_errors_are_bounded() {
        let curve = Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::handles(
                0.18,
                0.62,
                Handle {
                    dx: -0.10,
                    dy: -0.04,
                },
                Handle { dx: 0.01, dy: 0.32 },
            ),
            ControlPoint::curve(0.51, 0.34),
            ControlPoint::linear(0.73, 0.81),
            ControlPoint::curve(1.0, 1.0),
        ])
        .unwrap();
        let table = CurveTable::prepare(&curve);
        let mut max_value_error = 0.0f32;
        let mut reference_area = 0.0f64;
        let samples = 1_000_000usize;
        let mut previous = reference_evaluate(&curve, 0.0);
        let mut max_area_error = 0.0f64;
        for index in 0..=samples {
            let x = index as f32 / samples as f32;
            let reference = reference_evaluate(&curve, x);
            max_value_error = max_value_error.max((table.evaluate(x) - reference).abs());
            if index > 0 {
                reference_area += 0.5 * (previous as f64 + reference as f64) / samples as f64;
                max_area_error = max_area_error.max((table.integral(x) - reference_area).abs());
            }
            previous = reference;
        }
        assert!(max_value_error < 5.0e-4, "value error {max_value_error}");
        assert!(max_area_error < 2.0e-6, "area error {max_area_error}");
    }

    fn reference_evaluate(curve: &Curve, x: f32) -> f32 {
        let points = curve.points();
        let right = points.partition_point(|point| point.x <= x);
        if right == 0 {
            points[0].y
        } else if right == points.len() {
            points[points.len() - 1].y
        } else {
            evaluate_authored(points, right - 1, x).clamp(0.0, 1.0)
        }
    }
}
