//! Production curve canvas, moved from the accepted C0.5 interaction probe without changing its
//! editing vocabulary.

use crate::model::{HandleState, HandlesState, PointModeState, PointState, StageState};
use egui::{Key, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use mxm_ui::space::{SPACE_2, SPACE_3};
use mxm_ui::theme::Tokens;
use mxm_ui::visual::{AXIS_STROKE, CANVAS_RADIUS, EMPHASIS_STROKE, TRACE_STROKE};

const HIT_RADIUS: f32 = 12.0;
const SNAP_PIXELS: f32 = 9.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct CanvasOutcome {
    pub changed: bool,
    pub gesture_started: bool,
    pub gesture_ended: bool,
    /// BACK cancelled the drag that is ending (`mxm_ui::drag`): the gesture puts back the curve it
    /// began with instead of committing.
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandleSide {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grabbed {
    Point(usize),
    Handle(usize, HandleSide),
}

#[derive(Default)]
pub struct CurveCanvas {
    selected: Option<usize>,
    held: Option<Grabbed>,
    snap_x: Option<f32>,
    snap_y: Option<f32>,
    /// Where the last frame drew the canvas, so a test can press on its curve.
    rect: Option<Rect>,
}

impl CurveCanvas {
    #[cfg(test)]
    pub fn rect(&self) -> Rect {
        self.rect.expect("the canvas has been drawn")
    }

    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.held = None;
        self.snap_x = None;
        self.snap_y = None;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        stage: &mut StageState,
        signal_input: Option<f32>,
    ) -> CanvasOutcome {
        let size = ui.available_size();
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        response = response.on_hover_text(
            "Click the curve to add a point. Drag points to shape it. Alt-click cycles Curve, Linear, and Handles. Right-click deletes an interior point.",
        );
        // **The keyboard follows the pointer**, as the collection's keyboard cursor does: a point
        // clicked or dragged is the next arrow's to nudge, and a press anywhere else — a knob being
        // dragged, which egui does not count as a click elsewhere — gives the keyboard back.
        if response.clicked() || response.drag_started() {
            response.request_focus();
        } else if response.has_focus()
            && ui.input(|input| input.pointer.any_pressed())
            && !response.is_pointer_button_down_on()
        {
            response.surrender_focus();
        }
        if response.has_focus() {
            // Claims the arrows, or egui spends the first one moving its focus to a neighbour and
            // the point is nudged once. `Escape` still leaves.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: false,
                    },
                );
            });
        }
        self.rect = Some(rect);

        let to_screen = |point: Pos2| {
            Pos2::new(
                rect.left() + point.x * rect.width(),
                rect.bottom() - point.y * rect.height(),
            )
        };
        let to_curve = |point: Pos2| {
            Pos2::new(
                ((point.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0),
                ((rect.bottom() - point.y) / rect.height().max(1.0)).clamp(0.0, 1.0),
            )
        };
        let outcome = self.interact(ui, &response, rect, stage, &to_curve);
        let accessible = self.selected.and_then(|index| {
            stage.points.get(index).map(|point| {
                (
                    point.x,
                    format!(
                        "Point {} of {}, input {:.3}, output {:.3}, {}",
                        index + 1,
                        stage.points.len(),
                        point.x,
                        point.y,
                        mode_label(point.mode)
                    ),
                )
            })
        });
        response.widget_info(|| {
            let (value, text) = accessible
                .clone()
                .unwrap_or((0.0, "No point selected".to_owned()));
            let mut info =
                egui::WidgetInfo::slider(true, f64::from(value), "Transfer curve editor");
            info.current_text_value = Some(text);
            info
        });

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CANVAS_RADIUS, tokens.surface_1);
        let faint = tokens.text_primary.gamma_multiply(0.22);
        for index in 1..4 {
            let value = index as f32 / 4.0;
            painter.line_segment(
                [
                    to_screen(Pos2::new(value, 0.0)),
                    to_screen(Pos2::new(value, 1.0)),
                ],
                Stroke::new(AXIS_STROKE, faint),
            );
            painter.line_segment(
                [
                    to_screen(Pos2::new(0.0, value)),
                    to_screen(Pos2::new(1.0, value)),
                ],
                Stroke::new(AXIS_STROKE, faint),
            );
        }
        painter.line_segment(
            [
                to_screen(Pos2::new(0.0, 0.0)),
                to_screen(Pos2::new(1.0, 1.0)),
            ],
            Stroke::new(AXIS_STROKE, tokens.text_primary.gamma_multiply(0.42)),
        );

        let samples = sampled_curve(stage, rect.width(), &to_screen);
        painter.add(egui::Shape::line(
            samples,
            Stroke::new(TRACE_STROKE, tokens.accent),
        ));

        if let Some(x) = self.snap_x {
            painter.line_segment(
                [to_screen(Pos2::new(x, 0.0)), to_screen(Pos2::new(x, 1.0))],
                Stroke::new(EMPHASIS_STROKE, tokens.accent.gamma_multiply(0.55)),
            );
        }
        if let Some(y) = self.snap_y {
            painter.line_segment(
                [to_screen(Pos2::new(0.0, y)), to_screen(Pos2::new(1.0, y))],
                Stroke::new(EMPHASIS_STROKE, tokens.accent.gamma_multiply(0.55)),
            );
        }

        if let Some(index) = self.selected
            && stage
                .points
                .get(index)
                .is_some_and(|point| point.mode == PointModeState::Handles)
        {
            for side in [HandleSide::Incoming, HandleSide::Outgoing] {
                if let Some(handle) = handle_position(stage, index, side) {
                    let point = Pos2::new(stage.points[index].x, stage.points[index].y);
                    painter.line_segment(
                        [to_screen(point), to_screen(handle)],
                        Stroke::new(AXIS_STROKE, tokens.text_primary.gamma_multiply(0.55)),
                    );
                    painter.circle_stroke(
                        to_screen(handle),
                        SPACE_2,
                        Stroke::new(AXIS_STROKE, tokens.text_primary.gamma_multiply(0.85)),
                    );
                }
            }
        }

        for (index, point) in stage.points.iter().enumerate() {
            let at = to_screen(Pos2::new(point.x, point.y));
            if Some(index) == self.selected {
                painter.circle_stroke(at, SPACE_3, Stroke::new(EMPHASIS_STROKE, tokens.accent));
            }
            painter.circle_filled(at, SPACE_2 + 1.0, tokens.accent);
        }

        if let Some(input) = signal_input {
            // The dot explains the authored transfer law, so it stays on that law. A detector's
            // attack/release deliberately makes its actual output lag this target; the stereo
            // output meters show that post-ballistics result instead.
            let at = to_screen(signal_curve_position(stage, input));
            painter.circle_filled(at, SPACE_3, tokens.text_primary);
            painter.circle_stroke(at, SPACE_3 + 2.0, Stroke::new(AXIS_STROKE, tokens.accent));
        }

        if let Some(index) = self.selected
            && let Some(point) = stage.points.get(index)
        {
            painter.text(
                rect.left_top() + Vec2::splat(SPACE_3),
                egui::Align2::LEFT_TOP,
                format!(
                    "in {:.3}   out {:.3}   {}",
                    point.x,
                    point.y,
                    mode_label(point.mode)
                ),
                mxm_ui::typography::value_style(ui.style()).resolve(ui.style()),
                tokens.text_primary,
            );
        }
        outcome
    }

    fn interact(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        stage: &mut StageState,
        to_curve: &impl Fn(Pos2) -> Pos2,
    ) -> CanvasOutcome {
        let mut outcome = CanvasOutcome::default();
        let near = |a: Pos2, b: Pos2| {
            let dx = (a.x - b.x) * rect.width();
            let dy = (a.y - b.y) * rect.height();
            (dx * dx + dy * dy).sqrt() < HIT_RADIUS
        };
        let point_at = |points: &[PointState], at: Pos2| {
            points
                .iter()
                .position(|point| near(Pos2::new(point.x, point.y), at))
        };

        if response.secondary_clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            let at = to_curve(at);
            if let Some(index) = point_at(&stage.points, at)
                && index != 0
                && index + 1 != stage.points.len()
            {
                stage.points.remove(index);
                self.selected = None;
                outcome.changed = true;
                outcome.gesture_started = true;
                outcome.gesture_ended = true;
            }
            return outcome;
        }

        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            let at = to_curve(at);
            if let Some(index) = point_at(&stage.points, at) {
                if ui.input(|input| input.modifiers.alt) {
                    cycle_mode(stage, index);
                    self.selected = Some(index);
                    outcome.changed = true;
                    outcome.gesture_started = true;
                    outcome.gesture_ended = true;
                } else {
                    self.selected = Some(index);
                }
            } else {
                let on_curve = Pos2::new(at.x, evaluate(stage, at.x));
                if near(on_curve, at)
                    && at.x > 0.005
                    && at.x < 0.995
                    && stage.points.len() < mxm_fx_curve_dsp::MAX_POINTS
                {
                    self.selected = Some(insert_on_curve(stage, at.x));
                    outcome.changed = true;
                    outcome.gesture_started = true;
                    outcome.gesture_ended = true;
                } else {
                    self.selected = None;
                }
            }
            return outcome;
        }

        if response.drag_started()
            && let Some(at) = ui
                .input(|input| input.pointer.press_origin())
                .filter(|at| rect.contains(*at))
                .or_else(|| response.interact_pointer_pos())
        {
            let at = to_curve(at);
            let mut grabbed = None;
            if let Some(index) = self.selected
                && stage
                    .points
                    .get(index)
                    .is_some_and(|point| point.mode == PointModeState::Handles)
            {
                for side in [HandleSide::Incoming, HandleSide::Outgoing] {
                    if handle_position(stage, index, side).is_some_and(|handle| near(handle, at)) {
                        grabbed = Some(Grabbed::Handle(index, side));
                        break;
                    }
                }
            }
            if grabbed.is_none()
                && let Some(index) = point_at(&stage.points, at)
            {
                grabbed = Some(Grabbed::Point(index));
                self.selected = Some(index);
            }
            if grabbed.is_none() {
                let on_curve = Pos2::new(at.x, evaluate(stage, at.x));
                if near(on_curve, at)
                    && at.x > 0.005
                    && at.x < 0.995
                    && stage.points.len() < mxm_fx_curve_dsp::MAX_POINTS
                {
                    let index = insert_on_curve(stage, at.x);
                    grabbed = Some(Grabbed::Point(index));
                    self.selected = Some(index);
                    outcome.changed = true;
                }
            }
            self.held = grabbed;
            outcome.gesture_started = grabbed.is_some();
        }

        if response.dragged()
            && let (Some(held), Some(at)) = (self.held, response.interact_pointer_pos())
        {
            let at = to_curve(at);
            match held {
                Grabbed::Point(index) => {
                    clamp_point(stage, index, at.x, at.y);
                    self.snap_point(stage, index, at, rect);
                }
                Grabbed::Handle(index, side) => {
                    self.snap_x = None;
                    self.snap_y = None;
                    drag_handle(stage, index, side, at);
                }
            }
            outcome.changed = true;
        }

        if response.drag_stopped() {
            outcome.gesture_ended = self.held.is_some();
            outcome.cancelled =
                self.held.is_some() && mxm_ui::drag::cancelled(&response.ctx, response.id);
            self.held = None;
            self.snap_x = None;
            self.snap_y = None;
        }

        if response.has_focus() {
            let modifiers = ui.input(|input| input.modifiers);
            if ui.input(|input| input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace))
                && let Some(index) = self.selected
                && index > 0
                && index + 1 < stage.points.len()
            {
                stage.points.remove(index);
                self.selected = None;
                outcome.changed = true;
                outcome.gesture_started = true;
                outcome.gesture_ended = true;
            } else if modifiers.alt && ui.input(|input| input.key_pressed(Key::Enter)) {
                if let Some(index) = self.selected {
                    cycle_mode(stage, index);
                    outcome.changed = true;
                    outcome.gesture_started = true;
                    outcome.gesture_ended = true;
                }
            } else if let Some(index) = self.selected {
                let step = if modifiers.shift { 0.01 } else { 0.002 };
                let delta = ui.input(|input| {
                    let x = (input.key_pressed(Key::ArrowRight) as i8
                        - input.key_pressed(Key::ArrowLeft) as i8)
                        as f32
                        * step;
                    let y = (input.key_pressed(Key::ArrowUp) as i8
                        - input.key_pressed(Key::ArrowDown) as i8)
                        as f32
                        * step;
                    Vec2::new(x, y)
                });
                if delta != Vec2::ZERO {
                    let point = stage.points[index];
                    clamp_point(stage, index, point.x + delta.x, point.y + delta.y);
                    outcome.changed = true;
                    outcome.gesture_started = true;
                    outcome.gesture_ended = true;
                }
            }
        }
        outcome
    }

    fn snap_point(&mut self, stage: &mut StageState, index: usize, raw: Pos2, rect: Rect) {
        self.snap_x = None;
        self.snap_y = None;
        let last = stage.points.len() - 1;
        let can_snap_x = (index > 0 && index < last)
            || (index == 0 && raw.x > raw.y)
            || (index == last && 1.0 - raw.y < 1.0 - raw.x);
        let can_snap_y = (index > 0 && index < last)
            || (index == 0 && raw.x <= raw.y)
            || (index == last && 1.0 - raw.y >= 1.0 - raw.x);
        if can_snap_x {
            let low = if index == 0 {
                0.0
            } else {
                stage.points[index - 1].x
            };
            let high = if index == last {
                1.0
            } else {
                stage.points[index + 1].x
            };
            if let Some(x) = stage
                .points
                .iter()
                .enumerate()
                .filter(|(other, point)| *other != index && point.x >= low && point.x <= high)
                .map(|(_, point)| point.x)
                .min_by(|a, b| {
                    (stage.points[index].x - *a)
                        .abs()
                        .total_cmp(&(stage.points[index].x - *b).abs())
                })
                .filter(|x| (stage.points[index].x - *x).abs() * rect.width() <= SNAP_PIXELS)
            {
                stage.points[index].x = x;
                self.snap_x = Some(x);
            }
        }
        if can_snap_y
            && let Some(y) = stage
                .points
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, point)| point.y)
                .min_by(|a, b| {
                    (stage.points[index].y - *a)
                        .abs()
                        .total_cmp(&(stage.points[index].y - *b).abs())
                })
                .filter(|y| (stage.points[index].y - *y).abs() * rect.height() <= SNAP_PIXELS)
        {
            stage.points[index].y = y;
            self.snap_y = Some(y);
        }
    }
}

fn mode_label(mode: PointModeState) -> &'static str {
    match mode {
        PointModeState::Curve => "Curve",
        PointModeState::Linear => "Linear",
        PointModeState::Handles => "Handles",
    }
}

fn auto_tangent(stage: &StageState, index: usize) -> f32 {
    let points = &stage.points;
    let previous = (0..index)
        .rev()
        .find(|&other| points[other].x < points[index].x);
    let next = (index + 1..points.len()).find(|&other| points[other].x > points[index].x);
    let stacked_before = index > 0 && points[index - 1].x == points[index].x;
    let stacked_after = index + 1 < points.len() && points[index + 1].x == points[index].x;
    let secant = |a: usize, b: usize| {
        (points[b].y - points[a].y) / (points[b].x - points[a].x).max(f32::EPSILON)
    };
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
            let before_span = points[index].x - points[before].x;
            let after_span = points[after].x - points[index].x;
            let w1 = 2.0 * after_span + before_span;
            let w2 = after_span + 2.0 * before_span;
            (w1 + w2) / (w1 / before_slope + w2 / after_slope)
        }
        (None, None) => 0.0,
    }
}

fn cubic(u: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let v = 1.0 - u;
    v * v * v * p0 + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * p3
}

fn segment_controls(stage: &StageState, index: usize) -> (Pos2, Pos2) {
    let a = stage.points[index];
    let b = stage.points[index + 1];
    let span = (b.x - a.x).max(f32::EPSILON);
    let c1 = if a.mode == PointModeState::Handles {
        let handle = a.handles.unwrap_or(default_handles()).outgoing;
        Pos2::new(
            (a.x + handle.dx).clamp(a.x, b.x),
            (a.y + handle.dy).clamp(0.0, 1.0),
        )
    } else {
        let dx = span / 3.0;
        Pos2::new(a.x + dx, a.y + auto_tangent(stage, index) * dx)
    };
    let c2 = if b.mode == PointModeState::Handles {
        let handle = b.handles.unwrap_or(default_handles()).incoming;
        Pos2::new(
            (b.x + handle.dx).clamp(a.x, b.x),
            (b.y + handle.dy).clamp(0.0, 1.0),
        )
    } else {
        let dx = span / 3.0;
        Pos2::new(b.x - dx, b.y - auto_tangent(stage, index + 1) * dx)
    };
    (c1, c2)
}

fn evaluate_segment(stage: &StageState, index: usize, x: f32) -> f32 {
    let a = stage.points[index];
    let b = stage.points[index + 1];
    let span = b.x - a.x;
    if span <= f32::EPSILON {
        return b.y;
    }
    if x <= a.x {
        return a.y;
    }
    if x >= b.x {
        return b.y;
    }
    if a.mode == PointModeState::Linear || b.mode == PointModeState::Linear {
        return (a.y + (b.y - a.y) * ((x - a.x) / span)).clamp(0.0, 1.0);
    }
    let (c1, c2) = segment_controls(stage, index);
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..24 {
        let middle = (low + high) * 0.5;
        if cubic(middle, a.x, c1.x, c2.x, b.x) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    cubic((low + high) * 0.5, a.y, c1.y, c2.y, b.y).clamp(0.0, 1.0)
}

pub fn evaluate(stage: &StageState, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let Some(right) = stage.points.iter().position(|point| point.x > x) else {
        return stage.points.last().map_or(x, |point| point.y);
    };
    if right == 0 {
        stage.points[0].y
    } else {
        evaluate_segment(stage, right - 1, x)
    }
}

fn signal_curve_position(stage: &StageState, input: f32) -> Pos2 {
    let input = input.clamp(0.0, 1.0);
    Pos2::new(input, evaluate(stage, input))
}

fn sampled_curve(stage: &StageState, width: f32, to_screen: &impl Fn(Pos2) -> Pos2) -> Vec<Pos2> {
    let mut samples = Vec::new();
    if let Some(first) = stage.points.first() {
        samples.push(to_screen(Pos2::new(0.0, first.y)));
    }
    for index in 0..stage.points.len().saturating_sub(1) {
        let a = stage.points[index];
        let b = stage.points[index + 1];
        if b.x <= a.x + f32::EPSILON {
            samples.push(to_screen(Pos2::new(b.x, b.y)));
            continue;
        }
        let steps = ((b.x - a.x) * width).ceil().max(1.0) as usize;
        for step in 1..=steps {
            let x = a.x + (b.x - a.x) * step as f32 / steps as f32;
            samples.push(to_screen(Pos2::new(x, evaluate_segment(stage, index, x))));
        }
    }
    if let Some(last) = stage.points.last()
        && last.x < 1.0
    {
        samples.push(to_screen(Pos2::new(1.0, last.y)));
    }
    samples
}

fn clamp_point(stage: &mut StageState, index: usize, x: f32, y: f32) {
    let last = stage.points.len() - 1;
    if index == 0 {
        let high = stage.points[1].x;
        if x <= y {
            stage.points[0].x = 0.0;
            stage.points[0].y = y.clamp(0.0, 1.0);
        } else {
            stage.points[0].x = x.clamp(0.0, high);
            stage.points[0].y = 0.0;
        }
    } else if index == last {
        let low = stage.points[last - 1].x;
        if 1.0 - x <= 1.0 - y {
            stage.points[last].x = 1.0;
            stage.points[last].y = y.clamp(0.0, 1.0);
        } else {
            stage.points[last].x = x.clamp(low, 1.0);
            stage.points[last].y = 1.0;
        }
    } else {
        let low = stage.points[index - 1].x;
        let high = stage.points[index + 1].x;
        stage.points[index].x = x.clamp(low.min(high), high.max(low));
        stage.points[index].y = y.clamp(0.0, 1.0);
    }
}

fn cycle_mode(stage: &mut StageState, index: usize) {
    match stage.points[index].mode {
        PointModeState::Curve => {
            stage.points[index].mode = PointModeState::Linear;
            stage.points[index].handles = None;
        }
        PointModeState::Linear => {
            let slope = auto_tangent(stage, index);
            let incoming = if index > 0 {
                let dx = (stage.points[index].x - stage.points[index - 1].x) / 3.0;
                HandleState {
                    dx: -dx,
                    dy: -slope * dx,
                }
            } else {
                HandleState { dx: 0.0, dy: 0.0 }
            };
            let outgoing = if index + 1 < stage.points.len() {
                let dx = (stage.points[index + 1].x - stage.points[index].x) / 3.0;
                HandleState { dx, dy: slope * dx }
            } else {
                HandleState { dx: 0.0, dy: 0.0 }
            };
            stage.points[index].mode = PointModeState::Handles;
            stage.points[index].handles = Some(HandlesState { incoming, outgoing });
        }
        PointModeState::Handles => {
            stage.points[index].mode = PointModeState::Curve;
            stage.points[index].handles = None;
        }
    }
}

fn insert_on_curve(stage: &mut StageState, x: f32) -> usize {
    let low = stage.points.first().map_or(0.01, |point| point.x);
    let high = stage.points.last().map_or(0.99, |point| point.x);
    let x = x.clamp(low.min(high), high.max(low));
    let y = evaluate(stage, x);
    let index = stage
        .points
        .iter()
        .position(|point| point.x > x)
        .unwrap_or(stage.points.len());
    stage.points.insert(index, PointState::curve(x, y));
    index
}

fn default_handles() -> HandlesState {
    HandlesState {
        incoming: HandleState { dx: 0.0, dy: 0.0 },
        outgoing: HandleState { dx: 0.0, dy: 0.0 },
    }
}

fn handle_position(stage: &StageState, index: usize, side: HandleSide) -> Option<Pos2> {
    let point = *stage.points.get(index)?;
    let handles = point.handles?;
    match side {
        HandleSide::Incoming if index > 0 && stage.points[index - 1].x < point.x => Some(
            Pos2::new(point.x + handles.incoming.dx, point.y + handles.incoming.dy),
        ),
        HandleSide::Outgoing
            if index + 1 < stage.points.len() && point.x < stage.points[index + 1].x =>
        {
            Some(Pos2::new(
                point.x + handles.outgoing.dx,
                point.y + handles.outgoing.dy,
            ))
        }
        _ => None,
    }
}

fn drag_handle(stage: &mut StageState, index: usize, side: HandleSide, at: Pos2) {
    let point = stage.points[index];
    let raw = Vec2::new(at.x - point.x, at.y - point.y);
    let bounded = match side {
        HandleSide::Incoming if index > 0 => {
            let span = point.x - stage.points[index - 1].x;
            if span <= 0.002 {
                return;
            }
            Vec2::new(
                raw.x.clamp(-span, -0.002),
                raw.y.clamp(-point.y, 1.0 - point.y),
            )
        }
        HandleSide::Outgoing if index + 1 < stage.points.len() => {
            let span = stage.points[index + 1].x - point.x;
            if span <= 0.002 {
                return;
            }
            Vec2::new(
                raw.x.clamp(0.002, span),
                raw.y.clamp(-point.y, 1.0 - point.y),
            )
        }
        _ => return,
    };
    let handles = stage.points[index].handles.get_or_insert(default_handles());
    match side {
        HandleSide::Incoming => {
            handles.incoming = HandleState {
                dx: bounded.x,
                dy: bounded.y,
            }
        }
        HandleSide::Outgoing => {
            handles.outgoing = HandleState {
                dx: bounded.x,
                dy: bounded.y,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(points: &[(f32, f32)]) -> StageState {
        StageState {
            mode: crate::model::StageModeState::Memoryless,
            points: points
                .iter()
                .map(|&(x, y)| PointState::curve(x, y))
                .collect(),
        }
    }

    #[test]
    fn identity_is_exactly_straight() {
        let curve = stage(&[(0.0, 0.0), (1.0, 1.0)]);
        for step in 0..=100 {
            let x = step as f32 / 100.0;
            assert!((evaluate(&curve, x) - x).abs() < 1.0e-6);
        }
    }

    #[test]
    fn live_signal_position_is_always_on_the_authored_curve() {
        let stage = stage(&[(0.0, 0.0), (0.5, 0.35), (1.0, 0.6)]);
        let position = signal_curve_position(&stage, 0.75);
        assert_eq!(position.x, 0.75);
        assert_eq!(position.y, evaluate(&stage, position.x));
    }

    #[test]
    fn equal_y_points_make_an_exact_flat() {
        let curve = stage(&[(0.0, 0.0), (0.3, 0.6), (0.7, 0.6), (1.0, 1.0)]);
        for step in 0..=100 {
            let x = 0.3 + 0.4 * step as f32 / 100.0;
            assert!((evaluate(&curve, x) - 0.6).abs() < 1.0e-6);
        }
    }

    #[test]
    fn equal_x_points_are_right_continuous() {
        let curve = stage(&[(0.0, 0.0), (0.5, 0.2), (0.5, 0.8), (1.0, 1.0)]);
        assert_eq!(evaluate(&curve, 0.5), 0.8);
        assert!(evaluate(&curve, 0.5 - 1.0e-4).is_finite());
        assert!(evaluate(&curve, 0.5 + 1.0e-4).is_finite());
    }

    #[test]
    fn painted_curve_tracks_the_prepared_product_evaluator() {
        let curve = stage(&[(0.0, 0.1), (0.24, 0.72), (0.63, 0.38), (1.0, 1.0)]);
        let model = crate::model::CurveStackState {
            schema_version: crate::model::SCHEMA_VERSION,
            stages: vec![curve.clone()],
        };
        let engine = model.prepare(48_000.0).unwrap();
        let prepared = engine.stages()[0].curve();
        for step in 0..=1_000 {
            let x = step as f32 / 1_000.0;
            assert!((evaluate(&curve, x) - prepared.evaluate(x)).abs() < 5.0e-4);
        }
    }
}
