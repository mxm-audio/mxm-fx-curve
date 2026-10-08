//! Production editor for the serial curve processor.

use crate::canvas::{CanvasOutcome, CurveCanvas};
use crate::model::{CurveStackState, StageModeState, StageState};
use crate::params::MxmFxCurveParams;
use crate::telemetry::Telemetry;
use egui::{Response, Ui, Vec2};
use mxm_ui::control::{self, ParamView, Size, Wheel};
use mxm_ui::space::{HAIRLINE, SPACE_2, SPACE_3, SPACE_5};
use mxm_ui::theme::Tokens;
use mxm_ui::tree::{self, Flow, Font, Height, Kind, Node};
use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;
use nice_plug_egui::{EguiEditorState, NiceEguiApp, create_egui_editor};
use std::sync::Arc;

const REFERENCE: (u32, u32) = (1024, 760);
const MINIMUM: (u32, u32) = (416, 520);
const SIDE_BY_SIDE_FLOOR: f32 = 840.0;
const CONTROL_COLUMN_WIDTH: f32 = 300.0;
const INPUT_RAIL_WIDTH: f32 = 132.0;
const OUTPUT_RAIL_WIDTH: f32 = 64.0;
const CANVAS_FLOOR: f32 = 300.0;
const STAGE_CHIP_SIZE: Vec2 = Vec2::new(76.0, 24.0);

/// The keyboard cursor's cards, in its order. This editor has no paging renderer, so each is a
/// bar card (`mxm_ui::navigation::bar_card`): the Input gain rail and the Output card, the two that
/// hold host parameters. The Stage card's controls edit the curve model, not parameters.
const INPUT_CARD: u64 = 1;
const OUTPUT_CARD: u64 = 2;
const CURSOR_CARDS: [u64; 2] = [INPUT_CARD, OUTPUT_CARD];

pub type MxmFxCurveEditor = nice_plug_egui::EguiEditor<MxmFxCurveApp>;

pub fn create(
    params: Arc<MxmFxCurveParams>,
    telemetry: Arc<Telemetry>,
) -> Option<MxmFxCurveEditor> {
    let state = EguiEditorState::from_size(
        nice_plug::editor::dpi::LogicalSize::new(REFERENCE.0, REFERENCE.1),
        1.0,
    );
    create_egui_editor(
        state,
        nice_plug_egui::RepaintNotifier::new(),
        nice_plug_egui::EguiNiceSettings {
            title: crate::NAME.to_owned(),
            resize_hint: ResizeHint {
                size_constraints: nice_plug::editor::SizeConstraints::min_logical_size(
                    nice_plug::editor::dpi::LogicalSize::new(MINIMUM.0 as f32, MINIMUM.1 as f32),
                ),
                ..ResizeHint::RESIZABLE
            },
            ..Default::default()
        },
        MxmFxCurveApp::new(params, telemetry),
    )
}

const METER_PEAK_HOLD_SECONDS: f64 = 0.450;
const METER_RELEASE_SECONDS: f64 = 0.300;

#[derive(Default)]
struct MeterDisplay {
    input: [f32; 2],
    output: [f32; 2],
    input_hold_until: [f64; 2],
    output_hold_until: [f64; 2],
    last_time: Option<f64>,
}

impl MeterDisplay {
    fn update(&mut self, now: f64, input: [f32; 2], output: [f32; 2]) {
        let elapsed = self
            .last_time
            .map_or(0.0, |last| (now - last).clamp(0.0, 0.25));
        self.last_time = Some(now);
        // Immediate attack, a visible 450 ms peak hold, then a 300 ms release. In particular, a
        // one-block clip must remain in the yellow/red zones long enough to be seen. These are
        // editor-only display values and never feed back into audio.
        let release = (-elapsed / METER_RELEASE_SECONDS).exp() as f32;
        for channel in 0..2 {
            update_meter_channel(
                &mut self.input[channel],
                &mut self.input_hold_until[channel],
                input[channel],
                now,
                release,
            );
            update_meter_channel(
                &mut self.output[channel],
                &mut self.output_hold_until[channel],
                output[channel],
                now,
                release,
            );
        }
    }
}

fn update_meter_channel(
    displayed: &mut f32,
    hold_until: &mut f64,
    observed: f32,
    now: f64,
    release: f32,
) {
    let observed = if observed.is_finite() {
        observed.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if observed >= *displayed && observed > 0.0 {
        *displayed = observed;
        *hold_until = now + METER_PEAK_HOLD_SECONDS;
    } else if now >= *hold_until {
        *displayed = observed.max(*displayed * release);
    }
}

pub struct MxmFxCurveApp {
    params: Arc<MxmFxCurveParams>,
    telemetry: Arc<Telemetry>,
    context: Option<GuiContext>,
    working: CurveStackState,
    revision: u64,
    selected_stage: usize,
    canvas: CurveCanvas,
    gesture_origin: Option<CurveStackState>,
    input_gain_text: Option<String>,
    mix_text: Option<String>,
    meters: MeterDisplay,
    presets: mxm_preset::PresetUi,
    nav: mxm_ui::navigation::State,
}

impl MxmFxCurveApp {
    fn new(params: Arc<MxmFxCurveParams>, telemetry: Arc<Telemetry>) -> Self {
        let presets = mxm_preset::PresetUi::new(params.as_ref());
        Self {
            working: params.curves.snapshot(),
            revision: params.curves.revision(),
            params,
            telemetry,
            context: None,
            selected_stage: 0,
            canvas: CurveCanvas::default(),
            gesture_origin: None,
            input_gain_text: None,
            mix_text: None,
            meters: MeterDisplay::default(),
            presets,
            nav: mxm_ui::navigation::State::default(),
        }
    }

    /// Whether another surface owns the keyboard this frame, so the cursor stands aside: a value
    /// being typed, the preset browser, or a widget the cursor does not know holding egui's focus —
    /// the canvas nudging its selected point, a ballistics slider, a stage chip. Without the last,
    /// one arrow both nudged the point and moved the cursor's parameter.
    fn keyboard_held_elsewhere(&self, ctx: &egui::Context) -> bool {
        if self.input_gain_text.is_some()
            || self.mix_text.is_some()
            || self.presets.holds_the_keyboard()
        {
            return true;
        }
        ctx.memory(|memory| memory.focused())
            .is_some_and(|focused| {
                !mxm_ui::navigation::spots(ctx)
                    .iter()
                    .any(|spot| spot.focus_ids.contains(&focused))
            })
    }

    fn synchronize(&mut self) {
        if self.gesture_origin.is_none() && self.revision != self.params.curves.revision() {
            self.working = self.params.curves.snapshot();
            self.revision = self.params.curves.revision();
            self.selected_stage = self
                .selected_stage
                .min(self.working.stages.len().saturating_sub(1));
            self.canvas.clear_selection();
        }
    }

    fn apply_model_outcome(&mut self, before: CurveStackState, outcome: CanvasOutcome) {
        if outcome.gesture_started && self.gesture_origin.is_none() {
            self.gesture_origin = Some(before);
        }
        if outcome.changed
            && !outcome.gesture_ended
            && self.gesture_origin.is_some()
            && self.context.is_some()
        {
            // Audition a complete immutable model on every drag frame. This does not touch durable
            // state, history or host dirty state; release performs the one real transaction.
            let _ = self.params.curves.preview_editor(&self.working);
        }
        if outcome.gesture_ended {
            self.commit_gesture();
        }
    }

    fn commit_gesture(&mut self) {
        let Some(context) = self.context.as_ref() else {
            self.gesture_origin = None;
            return;
        };
        let Some(origin) = self.gesture_origin.take() else {
            return;
        };
        if origin != self.working
            && self
                .params
                .curves
                .commit_editor(context, self.working.clone())
        {
            self.revision = self.params.curves.revision();
        } else {
            // A gesture returned to its origin or failed its durable commit. Replace any audible
            // intermediate preview with the last committed model.
            let _ = self.params.curves.restore_committed_audio();
            self.working = self.params.curves.snapshot();
        }
    }

    fn immediate_edit(&mut self, edit: impl FnOnce(&mut CurveStackState, &mut usize)) {
        let before = self.working.clone();
        edit(&mut self.working, &mut self.selected_stage);
        self.apply_model_outcome(
            before,
            CanvasOutcome {
                changed: true,
                gesture_started: true,
                gesture_ended: true,
            },
        );
        self.canvas.clear_selection();
    }

    fn undo(&mut self) {
        if let Some(context) = self.context.as_ref()
            && self.params.curves.undo(context)
        {
            self.working = self.params.curves.snapshot();
            self.revision = self.params.curves.revision();
            self.selected_stage = self
                .selected_stage
                .min(self.working.stages.len().saturating_sub(1));
            self.canvas.clear_selection();
        }
    }

    fn redo(&mut self) {
        if let Some(context) = self.context.as_ref()
            && self.params.curves.redo(context)
        {
            self.working = self.params.curves.snapshot();
            self.revision = self.params.curves.revision();
            self.selected_stage = self
                .selected_stage
                .min(self.working.stages.len().saturating_sub(1));
            self.canvas.clear_selection();
        }
    }
}

impl Drop for MxmFxCurveApp {
    fn drop(&mut self) {
        if self.gesture_origin.is_some() {
            // Closing the transient editor mid-drag must not leave an uncommitted preview audible.
            let _ = self.params.curves.restore_committed_audio();
        }
    }
}

/// Draw the complete production surface into a caller-owned panel.
pub fn panel(ui: &mut Ui, app: &mut MxmFxCurveApp) {
    app.draw_panel(ui);
}

impl MxmFxCurveApp {
    fn draw_panel(&mut self, ui: &mut Ui) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
        self.synchronize();
        self.handle_shortcuts(ui.ctx());
        // The keyboard cursor (`plugins/AGENTS.md`), before anything is drawn: it moves over the
        // registry the last frame painted.
        let inert = self.keyboard_held_elsewhere(ui.ctx());
        mxm_ui::navigation::paged_with_bar(ui.ctx(), &mut self.nav, inert, &CURSOR_CARDS);
        let tokens = tokens_for(ui);
        let can_undo = self.params.curves.can_undo();
        let can_redo = self.params.curves.can_redo();
        let mut undo = false;
        let mut redo = false;
        let gui_context = self.context.clone();
        let setter = gui_context.as_ref().map(GuiContext::param_setter);
        mxm_ui::AppBar::new(crate::NAME).show_with(
            ui,
            &tokens,
            |ui| {
                if let Some(setter) = setter.as_ref() {
                    mxm_preset::ui::preset_row(
                        ui,
                        &tokens,
                        self.params.as_ref(),
                        setter,
                        &mut self.presets,
                    );
                }
                undo = ui
                    .add_enabled(can_undo, egui::Button::new("Undo"))
                    .clicked();
                redo = ui
                    .add_enabled(can_redo, egui::Button::new("Redo"))
                    .clicked();
            },
            |ui| {
                mxm_ui::shell::zoom_control(ui);
                mxm_ui::shell::editor_theme_control(ui);
            },
        );
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if let Some(setter) = setter.as_ref() {
            mxm_preset::ui::overlays(ui, &tokens, self.params.as_ref(), setter, &mut self.presets);
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(tokens.canvas)
                    .inner_margin(egui::Margin::same(SPACE_5 as i8)),
            )
            .show(ui, |ui| {
                self.stage_strip(ui, &tokens);
                ui.add_space(SPACE_3);
                if ui.available_width() >= SIDE_BY_SIDE_FLOOR {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        // Input gain and the two meter rails are part of the processing surface:
                        // their outer height is exactly the canvas height. The ordinary controls
                        // remain a natural-height column beyond the post-Mix output meter.
                        let canvas_size = wide_canvas_size(ui.available_size());
                        self.metered_curve_surface(ui, &tokens, canvas_size);
                        ui.add_space(SPACE_3);
                        ui.vertical(|ui| {
                            ui.set_width(CONTROL_COLUMN_WIDTH);
                            self.controls(ui, &tokens);
                        });
                    });
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let canvas_width = (ui.available_width()
                            - INPUT_RAIL_WIDTH
                            - OUTPUT_RAIL_WIDTH
                            - SPACE_3 * 2.0)
                            .clamp(CANVAS_FLOOR, 600.0);
                        egui::ScrollArea::horizontal().show(ui, |ui| {
                            self.metered_curve_surface(
                                ui,
                                &tokens,
                                Vec2::new(canvas_width, CANVAS_FLOOR),
                            );
                        });
                        ui.add_space(SPACE_5);
                        self.controls(ui, &tokens);
                    });
                }
            });
    }
}

impl NiceEguiApp for MxmFxCurveApp {
    fn build(
        &mut self,
        context: egui::Context,
        gui_context: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        mxm_ui::theme::apply(&context);
        mxm_ui::typography::apply(&context);
        context.set_theme(mxm_ui::theme::preference());
        self.telemetry.connect(true);
        self.context = Some(gui_context);
        self.synchronize();
        Ok(())
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut nice_plug_egui::Frame) {
        panel(ui, self);
    }

    fn editor_closed(&mut self) {
        // A host may close the window mid-drag. An incomplete gesture is discarded rather than
        // becoming a surprising partial history step; completed history belongs to CurveField and
        // therefore survives this editor object.
        if self.gesture_origin.take().is_some() {
            self.working = self.params.curves.snapshot();
        }
        self.telemetry.connect(false);
        self.context = None;
    }
}

impl MxmFxCurveApp {
    fn handle_shortcuts(&mut self, context: &egui::Context) {
        if self.input_gain_text.is_some()
            || self.mix_text.is_some()
            || self.presets.holds_the_keyboard()
        {
            return;
        }
        let command = context.input(|input| input.modifiers.command);
        if command && context.input(|input| input.key_pressed(egui::Key::Z)) {
            if context.input(|input| input.modifiers.shift) {
                self.redo();
            } else {
                self.undo();
            }
        } else if command && context.input(|input| input.key_pressed(egui::Key::Y)) {
            self.redo();
        }
    }

    fn stage_strip(&mut self, ui: &mut Ui, tokens: &Tokens) {
        egui::Frame::new()
            .fill(tokens.surface_1)
            .stroke(egui::Stroke::new(HAIRLINE, tokens.border))
            .corner_radius(mxm_ui::space::RADIUS)
            .inner_margin(egui::Margin::same(SPACE_3 as i8))
            .show(ui, |ui| {
                let mut select = None;
                let mut reorder = None;
                let mut delete = None;
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Serial chain").strong());
                    for index in 0..self.working.stages.len() {
                        if index > 0 {
                            ui.label("→");
                        }
                        let detector = matches!(
                            self.working.stages[index].mode,
                            StageModeState::Detector { .. }
                        );
                        let label =
                            format!("{}  Stage {}", if detector { "D" } else { "C" }, index + 1);
                        // Use one click-and-drag response for both jobs. `dnd_drag_source` overlays
                        // a drag-only interaction on the selectable label and swallows its click;
                        // that was why Stage 2 could be selected only by dragging and Stage 1 could
                        // not be selected again. The low-level payload API preserves a real click,
                        // while a movement past egui's drag threshold still starts reordering.
                        let response = ui
                            .add(
                                egui::Button::selectable(self.selected_stage == index, label)
                                    // Every chip reserves and paints the same frame. The selected
                                    // button style otherwise changes its frame margin/stroke by a
                                    // pixel or two, shifting the rest of this signal-path row.
                                    .frame_when_inactive(true)
                                    .stroke(egui::Stroke::new(HAIRLINE, tokens.border))
                                    .min_size(STAGE_CHIP_SIZE)
                                    .sense(egui::Sense::click_and_drag()),
                            )
                            .on_hover_text(format!(
                                "{}. Drag to reorder; right-click to delete.",
                                if detector {
                                    "Linked detector stage"
                                } else {
                                    "Memoryless curve stage"
                                }
                            ));
                        response.dnd_set_drag_payload(index);
                        if response.clicked() || response.drag_started() {
                            select = Some(index);
                        }
                        response.context_menu(|ui| {
                            if ui
                                .add_enabled(
                                    self.working.stages.len() > 1,
                                    egui::Button::new("Delete stage"),
                                )
                                .on_disabled_hover_text("A curve stack needs at least one stage.")
                                .clicked()
                            {
                                delete = Some(index);
                                ui.close();
                            }
                        });
                        if response.has_focus()
                            && self.working.stages.len() > 1
                            && ui.input(|input| input.key_pressed(egui::Key::Delete))
                        {
                            delete = Some(index);
                        }

                        if let Some(source) = response.dnd_hover_payload::<usize>()
                            && *source != index
                        {
                            let after = ui
                                .ctx()
                                .pointer_latest_pos()
                                .is_some_and(|pointer| pointer.x >= response.rect.center().x);
                            let x = if after {
                                response.rect.right() + SPACE_2
                            } else {
                                response.rect.left() - SPACE_2
                            };
                            ui.painter().line_segment(
                                [
                                    egui::pos2(x, response.rect.top()),
                                    egui::pos2(x, response.rect.bottom()),
                                ],
                                egui::Stroke::new(2.0, tokens.accent),
                            );
                        }
                        if let Some(source) = response.dnd_release_payload::<usize>() {
                            let after = ui
                                .ctx()
                                .pointer_latest_pos()
                                .is_some_and(|pointer| pointer.x >= response.rect.center().x);
                            reorder = Some((*source, index, after));
                        }
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.working.stages.len() < mxm_fx_curve_dsp::MAX_STAGES,
                            egui::Button::new("+ Stage"),
                        )
                        .on_hover_text("Append an identity curve stage")
                        .clicked()
                    {
                        self.immediate_edit(|model, selected| {
                            model.stages.push(identity_stage());
                            *selected = model.stages.len() - 1;
                        });
                    }
                });
                if let Some(index) = select {
                    self.selected_stage = index;
                    self.canvas.clear_selection();
                }
                if let Some((source, target, after)) = reorder {
                    self.immediate_edit(move |model, selected| {
                        reorder_stage(model, selected, source, target, after);
                    });
                } else if let Some(index) = delete {
                    self.immediate_edit(move |model, selected| {
                        delete_stage(model, selected, index);
                    });
                }
            });
    }

    fn curve_field(&mut self, ui: &mut Ui, tokens: &Tokens) {
        let before = self.working.clone();
        let signal = self
            .telemetry
            .take(self.selected_stage)
            .map(|(input, _actual_output)| input);
        let outcome = self.canvas.show(
            ui,
            tokens,
            &mut self.working.stages[self.selected_stage],
            signal,
        );
        if outcome.changed || outcome.gesture_started || outcome.gesture_ended {
            self.apply_model_outcome(before, outcome);
        }
    }

    fn metered_curve_surface(&mut self, ui: &mut Ui, tokens: &Tokens, canvas_size: Vec2) {
        let input = self.telemetry.take_input();
        let output = self.telemetry.take_output();
        let now = ui.input(|input| input.time);
        self.meters.update(now, input, output);
        let input_peaks = self.meters.input;
        let output_peaks = self.meters.output;
        let input_clipped = self.telemetry.input_clipped();
        let output_clipped = self.telemetry.output_clipped();

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                Vec2::new(INPUT_RAIL_WIDTH, canvas_size.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.input_rail(ui, tokens, canvas_size.y, input_peaks, input_clipped),
            );
            ui.add_space(SPACE_3);
            ui.allocate_ui(canvas_size, |ui| self.curve_field(ui, tokens));
            ui.add_space(SPACE_3);
            ui.allocate_ui_with_layout(
                Vec2::new(OUTPUT_RAIL_WIDTH, canvas_size.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.output_rail(ui, tokens, canvas_size.y, output_peaks, output_clipped),
            );
        });
    }

    fn input_rail(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        height: f32,
        peaks: [f32; 2],
        clipped: [bool; 2],
    ) {
        mxm_ui::navigation::bar_card(ui, INPUT_CARD, |ui| {
            let rect = self
                .input_rail_body(ui, tokens, height, peaks, clipped)
                .response
                .rect;
            mxm_ui::navigation::paint_card(ui, tokens, INPUT_CARD, rect);
            rect
        });
    }

    fn input_rail_body(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        height: f32,
        peaks: [f32; 2],
        clipped: [bool; 2],
    ) -> egui::InnerResponse<()> {
        rail_frame(tokens).show(ui, |ui| {
            let content_height = (height - SPACE_3 * 2.0).max(CANVAS_FLOOR - SPACE_3 * 2.0);
            ui.set_min_size(Vec2::new(INPUT_RAIL_WIDTH - SPACE_3 * 2.0, content_height));
            let setter = self.context.as_ref().map(GuiContext::param_setter);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                float_vertical_slider_control(
                    ui,
                    tokens,
                    "inputgain",
                    &self.params.input_gain,
                    "Gain before the curves; the meter beside it shows the result.",
                    setter.as_ref(),
                    &mut self.input_gain_text,
                    72.0,
                    content_height,
                );
                let acknowledged = stereo_meter(
                    ui,
                    tokens,
                    "In",
                    ["Input left", "Input right"],
                    peaks,
                    clipped,
                    content_height,
                );
                for (channel, acknowledged) in acknowledged.into_iter().enumerate() {
                    if acknowledged {
                        self.telemetry.clear_input_clip(channel);
                    }
                }
            });
        })
    }

    fn output_rail(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        height: f32,
        peaks: [f32; 2],
        clipped: [bool; 2],
    ) {
        rail_frame(tokens).show(ui, |ui| {
            let content_height = (height - SPACE_3 * 2.0).max(CANVAS_FLOOR - SPACE_3 * 2.0);
            ui.set_min_size(Vec2::new(OUTPUT_RAIL_WIDTH - SPACE_3 * 2.0, content_height));
            let acknowledged = ui
                .vertical_centered(|ui| {
                    stereo_meter(
                        ui,
                        tokens,
                        "Out",
                        ["Output left", "Output right"],
                        peaks,
                        clipped,
                        content_height,
                    )
                })
                .inner;
            for (channel, acknowledged) in acknowledged.into_iter().enumerate() {
                if acknowledged {
                    self.telemetry.clear_output_clip(channel);
                }
            }
        });
    }

    fn controls(&mut self, ui: &mut Ui, tokens: &Tokens) {
        self.stage_controls(ui, tokens);
        ui.add_space(SPACE_5);
        self.output_controls(ui, tokens);
    }

    /// Whether the selected stage is a linked detector.
    fn selected_is_detector(&self) -> bool {
        matches!(
            self.working.stages[self.selected_stage].mode,
            StageModeState::Detector { .. }
        )
    }

    /// The Stage card: its body is a tree ([`stage_tree`]) drawn inside the card's own frame.
    fn stage_controls(&mut self, ui: &mut Ui, tokens: &Tokens) {
        card(tokens).show(ui, |ui| {
            let tree = stage_tree(ui, self.selected_stage, self.selected_is_detector());
            // egui's `Frame` hugs what it holds: the wrapped note's widest line, not the column.
            ui.set_max_width(stage_hug(ui, &tree));
            tree::show(ui, tokens, &tree, |ui, leaf, _| self.paint_stage(ui, *leaf));
        });
    }

    /// The Output card: its body is a tree ([`output_tree`]) drawn inside the card's own frame.
    fn output_controls(&mut self, ui: &mut Ui, tokens: &Tokens) {
        mxm_ui::navigation::bar_card(ui, OUTPUT_CARD, |ui| {
            let rect = card(tokens)
                .show(ui, |ui| {
                    let estimate = curve_estimate(self.params.curves.nominal_makeup_gain());
                    let tree = output_tree(ui, &self.params, &estimate);
                    tree::show(ui, tokens, &tree, |ui, leaf, _| {
                        self.paint_output(ui, tokens, *leaf, &estimate);
                    });
                })
                .response
                .rect;
            mxm_ui::navigation::paint_card(ui, tokens, OUTPUT_CARD, rect);
            rect
        });
    }

    /// Draws one leaf of the Stage card, as its hand-laid body drew it.
    fn paint_stage(&mut self, ui: &mut Ui, leaf: Leaf) {
        match leaf {
            Leaf::Heading => {
                // What a curve does to the wave, on hover rather than printed (design system §7.6).
                ui.heading(stage_heading(self.selected_stage))
                    .on_hover_text(STAGE_HOVER);
            }
            Leaf::Linked => {
                let before = self.working.clone();
                let mut requested = self.selected_is_detector();
                let toggle = ui
                    .checkbox(&mut requested, LINKED)
                    .on_hover_text("Treats both channels together, so the stereo image stays put.");
                if toggle.changed() {
                    self.working.stages[self.selected_stage].mode = if requested {
                        StageModeState::Detector {
                            attack_ms: 10.0,
                            release_ms: 120.0,
                        }
                    } else {
                        StageModeState::Memoryless
                    };
                    self.apply_model_outcome(before, instant());
                }
            }
            Leaf::Ballistics => self.ballistics(ui),
            // The Output card's leaves; not in this tree.
            Leaf::Mix | Leaf::AutoMakeup | Leaf::Estimate => {}
        }
    }

    /// The detector's attack and release: an `egui::Grid` of two named sliders, which states its
    /// size in [`ballistics_size`].
    fn ballistics(&mut self, ui: &mut Ui) {
        let StageModeState::Detector {
            attack_ms,
            release_ms,
        } = self.working.stages[self.selected_stage].mode
        else {
            // Linked was switched off this frame; the tree drops the grid on the next.
            return;
        };
        egui::Grid::new("curve-detector-ballistics")
            .num_columns(2)
            .spacing([SPACE_3, SPACE_2])
            .show(ui, |ui| {
                ui.label(BALLISTICS[0]);
                let mut attack_ms = attack_ms;
                let before = self
                    .gesture_origin
                    .clone()
                    .unwrap_or_else(|| self.working.clone());
                let attack = ui
                    .add(
                        egui::Slider::new(&mut attack_ms, BALLISTICS_RANGE)
                            .logarithmic(true)
                            .suffix(" ms"),
                    )
                    .on_hover_text("How quickly gain moves toward more reduction.");
                if attack.changed()
                    && let StageModeState::Detector {
                        attack_ms: value, ..
                    } = &mut self.working.stages[self.selected_stage].mode
                {
                    *value = attack_ms;
                }
                apply_response(self, before, attack);
                ui.end_row();

                ui.label(BALLISTICS[1]);
                let mut release_ms = release_ms;
                let before = self
                    .gesture_origin
                    .clone()
                    .unwrap_or_else(|| self.working.clone());
                let release = ui
                    .add(
                        egui::Slider::new(&mut release_ms, BALLISTICS_RANGE)
                            .logarithmic(true)
                            .suffix(" ms"),
                    )
                    .on_hover_text("How quickly gain returns after the peak falls.");
                if release.changed()
                    && let StageModeState::Detector {
                        release_ms: value, ..
                    } = &mut self.working.stages[self.selected_stage].mode
                {
                    *value = release_ms;
                }
                apply_response(self, before, release);
                ui.end_row();
            });
    }

    /// Draws one leaf of the Output card, as its hand-laid body drew it.
    fn paint_output(&mut self, ui: &mut Ui, tokens: &Tokens, leaf: Leaf, estimate: &str) {
        let setter = self.context.as_ref().map(GuiContext::param_setter);
        match leaf {
            Leaf::Heading => {
                ui.heading(OUTPUT_HEADING);
            }
            Leaf::Mix => float_knob_control(
                ui,
                tokens,
                "mix",
                &self.params.mix,
                "The balance of the dry sound and the processed sound.",
                setter.as_ref(),
                &mut self.mix_text,
            ),
            Leaf::AutoMakeup => {
                auto_makeup_control(ui, tokens, &self.params.auto_makeup, setter.as_ref());
            }
            Leaf::Estimate => {
                ui.label(estimate).on_hover_text(
                    "An estimate of the level the curves take away, given back afterwards.",
                );
            }
            // The Stage card's leaves; not in this tree.
            Leaf::Linked | Leaf::Ballistics => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Stage and Output bodies, as trees (plans/plan-layout-tree.md). They stay local frames in the
// owner's fixed 300-point column beside the stretching canvas — this editor's exception — and only
// their bodies are data: one description each, measured and drawn, so their heights and narrowest
// widths are computed and a test holds both inside the column. The canvas, its rails and the stage
// strip stay hand-laid.
// ---------------------------------------------------------------------------------------------

/// What a leaf of the Stage and Output bodies draws.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Leaf {
    Heading,
    Linked,
    Ballistics,
    Mix,
    AutoMakeup,
    Estimate,
}

const STAGE_HOVER: &str = "Each curve shapes the wave's upswings and downswings the same way.";
const LINKED: &str = "Linked detector";
const OUTPUT_HEADING: &str = "Output controls";
const AUTO_MAKEUP: &str = "Auto makeup";
/// The ballistics grid's names, in its rows' order.
const BALLISTICS: [&str; 2] = ["Attack", "Release"];
const BALLISTICS_RANGE: std::ops::RangeInclusive<f32> = 0.1..=5_000.0;
/// Mix's knob column in the Output card.
const MIX_COLUMN: f32 = 96.0;

fn stage_heading(stage: usize) -> String {
    format!("Stage {}", stage + 1)
}

/// The Output card's closing reading, from the stack's nominal makeup gain.
fn curve_estimate(nominal_gain: f32) -> String {
    format!("Curve estimate {:+.1} dB", 20.0 * nominal_gain.log10())
}

fn heading(text: &str) -> Node<Leaf> {
    tree::leaf(
        Leaf::Heading,
        Kind::Text {
            text: text.to_owned(),
            font: Font::Heading,
            flow: Flow::Line,
        },
    )
}

/// The Stage body, in the frame's own item spacing: its heading, `SPACE_3` and Linked detector,
/// and — for a detector stage — `SPACE_2` and the ballistics.
fn stage_tree(ui: &Ui, stage: usize, detector: bool) -> Node<Leaf> {
    let mut body = vec![
        heading(&stage_heading(stage)),
        tree::pad(
            SPACE_3,
            tree::leaf(
                Leaf::Linked,
                Kind::Checkbox {
                    label: LINKED.to_owned(),
                },
            ),
        ),
    ];
    if detector {
        let size = ballistics_size(ui);
        body.push(tree::pad(
            SPACE_2,
            tree::leaf(
                Leaf::Ballistics,
                Kind::Custom {
                    min_width: size.x,
                    height: Height::Fixed(size.y),
                    fills: false,
                },
            ),
        ));
    }
    tree::stack_gap(ui.spacing().item_spacing.y, body)
}

/// How wide the Stage frame's body is drawn: its narrowest, which is what egui's `Frame` hugs.
fn stage_hug(ui: &Ui, tree: &Node<Leaf>) -> f32 {
    tree.min_width(ui)
}

/// The Output body, in the frame's own item spacing: its heading, Mix at its 96-point column,
/// `SPACE_3` and Auto makeup, and the curve estimate on one line.
fn output_tree(ui: &Ui, params: &MxmFxCurveParams, estimate: &str) -> Node<Leaf> {
    let mix = &params.mix;
    tree::stack_gap(
        ui.spacing().item_spacing.y,
        vec![
            heading(OUTPUT_HEADING),
            tree::leaf(
                Leaf::Mix,
                Kind::Knob {
                    name: mix.name().to_owned(),
                    widest: control::widest_value(|n| {
                        mix.normalized_value_to_string(n as f32, true)
                    }),
                    size: Size::Primary,
                    column: MIX_COLUMN,
                },
            ),
            tree::pad(
                SPACE_3,
                tree::leaf(
                    Leaf::AutoMakeup,
                    Kind::Toggle {
                        label: AUTO_MAKEUP.to_owned(),
                    },
                ),
            ),
            tree::leaf(
                Leaf::Estimate,
                Kind::Text {
                    text: estimate.to_owned(),
                    font: Font::Body,
                    flow: Flow::Line,
                },
            ),
        ],
    )
}

/// What the ballistics grid occupies, in the `Ui` it is drawn in: egui's `Grid` arithmetic over
/// two rows. The name column is the wider name, the slider column an `egui::Slider` whose value box
/// holds its widest reading ([`ballistics_widest`]) — each at least egui's interact width — and
/// each row is the taller of its two cells, at least the interact height; `SPACE_3` between the
/// columns and `SPACE_2` between the rows.
fn ballistics_size(ui: &Ui) -> Vec2 {
    let spacing = ui.spacing();
    let body = tree::font_id(ui, Font::Body);
    let names = BALLISTICS.map(|name| {
        ui.painter()
            .layout_no_wrap(name.to_owned(), body.clone(), egui::Color32::PLACEHOLDER)
            .size()
    });
    let name = names
        .iter()
        .map(|size| size.x)
        .fold(spacing.interact_size.x, f32::max);
    let slider = control::egui_slider_size(ui, &ballistics_widest(ui), None);
    let row = names
        .iter()
        .map(|size| size.y)
        .fold(slider.y.max(spacing.interact_size.y), f32::max);
    Vec2::new(
        name + SPACE_3 + slider.x.max(spacing.interact_size.x),
        2.0 * row + SPACE_2,
    )
}

/// The widest reading egui prints in a ballistics slider's value box. egui formats the value
/// itself, with as many decimals as one pixel of the logarithmic rail is worth there and two more:
/// at the smallest zoom that is at most six figures — four whole and two decimal places toward
/// 5000 ms, one and five below 1 ms — so six of the widest digit, a point and the unit.
fn ballistics_widest(ui: &Ui) -> String {
    let font = ui.style().drag_value_text_style.resolve(ui.style());
    let digit = ('0'..='9')
        .max_by(|a, b| {
            let width = |c: &char| {
                ui.painter()
                    .layout_no_wrap(c.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
                    .size()
                    .x
            };
            width(a).total_cmp(&width(b))
        })
        .unwrap_or('0');
    let four: String = std::iter::repeat_n(digit, 4).collect();
    let two: String = std::iter::repeat_n(digit, 2).collect();
    format!("{four}.{two} ms")
}

fn apply_response(app: &mut MxmFxCurveApp, before: CurveStackState, response: Response) {
    if !response.changed() && !response.drag_started() && !response.drag_stopped() {
        return;
    }
    let instantaneous = response.changed()
        && !response.dragged()
        && !response.drag_started()
        && !response.drag_stopped();
    app.apply_model_outcome(
        before,
        CanvasOutcome {
            changed: response.changed(),
            gesture_started: response.drag_started() || instantaneous,
            gesture_ended: response.drag_stopped() || instantaneous,
        },
    );
}

fn instant() -> CanvasOutcome {
    CanvasOutcome {
        changed: true,
        gesture_started: true,
        gesture_ended: true,
    }
}

fn reorder_stage(
    model: &mut CurveStackState,
    selected: &mut usize,
    source: usize,
    target: usize,
    after: bool,
) {
    if source >= model.stages.len() || target >= model.stages.len() {
        return;
    }
    let old_selected = *selected;
    let mut insertion = target + usize::from(after);
    let moved = model.stages.remove(source);
    if source < insertion {
        insertion -= 1;
    }
    insertion = insertion.min(model.stages.len());
    model.stages.insert(insertion, moved);

    *selected = if old_selected == source {
        insertion
    } else {
        let after_removal = if old_selected > source {
            old_selected - 1
        } else {
            old_selected
        };
        if after_removal >= insertion {
            after_removal + 1
        } else {
            after_removal
        }
    };
}

fn delete_stage(model: &mut CurveStackState, selected: &mut usize, index: usize) {
    if model.stages.len() <= 1 || index >= model.stages.len() {
        return;
    }
    model.stages.remove(index);
    if *selected > index {
        *selected -= 1;
    } else if *selected == index {
        *selected = index.min(model.stages.len() - 1);
    }
}

fn identity_stage() -> StageState {
    StageState {
        mode: StageModeState::Memoryless,
        points: vec![
            crate::model::PointState::curve(0.0, 0.0),
            crate::model::PointState::curve(1.0, 1.0),
        ],
    }
}

fn wide_canvas_size(available: Vec2) -> Vec2 {
    Vec2::new(
        (available.x - INPUT_RAIL_WIDTH - OUTPUT_RAIL_WIDTH - CONTROL_COLUMN_WIDTH - SPACE_3 * 3.0)
            .max(CANVAS_FLOOR),
        available.y.max(CANVAS_FLOOR),
    )
}

fn card(tokens: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(tokens.surface_1)
        .stroke(egui::Stroke::new(HAIRLINE, tokens.border))
        .corner_radius(mxm_ui::space::RADIUS)
        .inner_margin(egui::Margin::same(SPACE_5 as i8))
}

fn rail_frame(tokens: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(tokens.surface_1)
        .stroke(egui::Stroke::new(HAIRLINE, tokens.border))
        .corner_radius(mxm_ui::space::RADIUS)
        .inner_margin(egui::Margin::same(SPACE_3 as i8))
}

fn stereo_meter(
    ui: &mut Ui,
    tokens: &Tokens,
    title: &str,
    channel_names: [&str; 2],
    peaks: [f32; 2],
    clipped: [bool; 2],
    height: f32,
) -> [bool; 2] {
    let label_height = ui.text_style_height(&egui::TextStyle::Body);
    let meter_height = (height - label_height * 3.0 - SPACE_2 * 3.0).max(48.0);
    let mut acknowledged = [false; 2];
    ui.allocate_ui_with_layout(
        Vec2::new(mxm_ui::shell::VERTICAL_METER_WIDTH * 2.0 + SPACE_2, height),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.y = SPACE_2;
            ui.label(egui::RichText::new(title).strong());
            ui.label(
                egui::RichText::new("CLIP")
                    .text_style(mxm_ui::typography::caption_style(ui.style()))
                    .color(tokens.text_secondary),
            );
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                for channel in 0..2 {
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = SPACE_2;
                        ui.label(if channel == 0 { "L" } else { "R" });
                        acknowledged[channel] = mxm_ui::shell::vertical_level_meter(
                            ui,
                            tokens,
                            channel_names[channel],
                            peaks[channel],
                            clipped[channel],
                            meter_height,
                        );
                    });
                }
            });
        },
    );
    acknowledged
}

#[allow(clippy::too_many_arguments)]
fn float_vertical_slider_control(
    ui: &mut Ui,
    tokens: &Tokens,
    id: &'static str,
    param: &FloatParam,
    description: &'static str,
    setter: Option<&ParamSetter<'_>>,
    text_entry: &mut Option<String>,
    width: f32,
    height: f32,
) {
    let mut normalized = f64::from(param.unmodulated_normalized_value());
    let text = param.normalized_value_to_string(normalized as f32, true);
    let view = ParamView::new(param.name(), &text, description)
        .default_at(f64::from(param.default_normalized_value()))
        .marked(param.modulated_normalized_value() != param.unmodulated_normalized_value());
    let outcome = mxm_ui::navigation::at(ui, id, |ui| {
        control::slider_vertical(
            ui,
            tokens,
            &view,
            &mut normalized,
            width,
            height,
            text_entry,
            Wheel::Off,
        )
    });
    let Some(setter) = setter else {
        return;
    };
    if outcome.gesture_started {
        setter.begin_set_parameter(param);
    }
    if outcome.changed {
        if let Some(text) = text_entry.take() {
            if let Some(value) = param.string_to_normalized_value(&text) {
                setter.set_parameter_normalized(param, value);
            }
        } else {
            setter.set_parameter_normalized(param, normalized as f32);
        }
    }
    if outcome.gesture_ended {
        setter.end_set_parameter(param);
    }
}

fn float_knob_control(
    ui: &mut Ui,
    tokens: &Tokens,
    id: &'static str,
    param: &FloatParam,
    description: &'static str,
    setter: Option<&ParamSetter<'_>>,
    text_entry: &mut Option<String>,
) {
    let mut normalized = f64::from(param.unmodulated_normalized_value());
    let text = param.normalized_value_to_string(normalized as f32, true);
    let view = ParamView::new(param.name(), &text, description)
        .default_at(f64::from(param.default_normalized_value()))
        .marked(param.modulated_normalized_value() != param.unmodulated_normalized_value());
    let outcome = mxm_ui::navigation::at(ui, id, |ui| {
        control::knob(
            ui,
            tokens,
            &view,
            &mut normalized,
            Size::Primary,
            96.0,
            text_entry,
            Wheel::Off,
        )
    });
    let Some(setter) = setter else {
        return;
    };
    if outcome.gesture_started {
        setter.begin_set_parameter(param);
    }
    if outcome.changed {
        if let Some(text) = text_entry.take() {
            if let Some(value) = param.string_to_normalized_value(&text) {
                setter.set_parameter_normalized(param, value);
            }
        } else {
            setter.set_parameter_normalized(param, normalized as f32);
        }
    }
    if outcome.gesture_ended {
        setter.end_set_parameter(param);
    }
}

fn auto_makeup_control(
    ui: &mut Ui,
    tokens: &Tokens,
    param: &BoolParam,
    setter: Option<&ParamSetter<'_>>,
) {
    let mut enabled = param.unmodulated_plain_value();
    let marked = param.modulated_plain_value() != enabled;
    let changed = mxm_ui::navigation::at(ui, "automakeup", |ui| {
        control::toggle(
            ui,
            tokens,
            "Auto makeup",
            &mut enabled,
            marked,
            "Gives back the level the curves take away.",
        )
    });
    if changed && let Some(setter) = setter {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, enabled);
        setter.end_set_parameter(param);
    }
}

fn tokens_for(ui: &Ui) -> Tokens {
    if ui.visuals().dark_mode {
        mxm_ui::DARK
    } else {
        mxm_ui::LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use mxm_plugin_test::keyboard_checks;
    use mxm_plugin_test::tree_checks;

    /// An editor wired to a host that counts gestures.
    fn recorded_app() -> (MxmFxCurveApp, Arc<keyboard_checks::Recorder>) {
        let host = Arc::new(keyboard_checks::Recorder::default());
        let mut app = MxmFxCurveApp::new(
            Arc::new(MxmFxCurveParams::default()),
            Arc::new(Telemetry::default()),
        );
        app.context = Some(GuiContext::new(host.clone()));
        (app, host)
    }

    /// **The keyboard cursor reaches and operates all three host parameters** — the check every
    /// editor owes it (`plugins/AGENTS.md`). This editor has no paging renderer, so it passes no
    /// items: both its cursor cards are always on screen.
    #[test]
    fn the_keyboard_cursor_reaches_and_operates_every_parameter() {
        let (mut app, host) = recorded_app();
        keyboard_checks::the_cursor_reaches_and_operates(
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &[],
            keyboard_checks::Coverage::Exactly(&["automakeup", "inputgain", "mix"]),
            &|_| {},
            &host,
            &mut |ui| panel(ui, &mut app),
        );
    }

    /// **A focused canvas keeps the arrows, and `Escape` hands them back.** A point pressed onto
    /// the curve is nudged by every press, so egui did not carry the focus off after the first, and
    /// no host parameter moves, so the cursor stood aside rather than editing its parameter with the
    /// same press. Once `Escape` leaves the canvas, the keys are the cursor's again.
    #[test]
    fn a_focused_canvas_keeps_the_arrows_and_escape_returns_them() {
        use egui::{Event, Key, Modifiers, PointerButton};
        let (mut app, host) = recorded_app();
        let session =
            keyboard_checks::Session::new(egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32));
        session.settle(&mut |ui| panel(ui, &mut app));

        // The middle of the canvas is on Init's 1:1 line: a click there adds a point and takes it.
        let at = app.canvas.rect().center();
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        session.frame(
            &mut |ui| panel(ui, &mut app),
            vec![Event::PointerMoved(at), button(true)],
        );
        session.frame(&mut |ui| panel(ui, &mut app), vec![button(false)]);
        session.frame(&mut |ui| panel(ui, &mut app), Vec::new());
        assert_eq!(
            app.working.stages[0].points.len(),
            3,
            "the click added a point"
        );
        let before = app.working.stages[0].points[1].y;

        let sets = host.sets();
        for _ in 0..3 {
            session.frame(
                &mut |ui| panel(ui, &mut app),
                keyboard_checks::press(Key::ArrowUp, Modifiers::NONE),
            );
            session.frame(&mut |ui| panel(ui, &mut app), Vec::new());
        }
        let nudged = app.working.stages[0].points[1].y;
        assert!(
            (nudged - before - 3.0 * 0.002).abs() < 1e-4,
            "three presses nudged the point from {before} to {nudged}, not three steps"
        );
        assert_eq!(
            host.sets(),
            sets,
            "an arrow meant for the canvas moved a parameter"
        );

        session.frame(
            &mut |ui| panel(ui, &mut app),
            keyboard_checks::press(Key::Escape, Modifiers::NONE),
        );
        // VALUE + ↑, kept with OUT: W, ↑ and Tab in the default keymap.
        let edit = [Key::W, Key::ArrowUp, Key::Tab]
            .into_iter()
            .flat_map(|key| keyboard_checks::press(key, Modifiers::NONE))
            .collect();
        session.frame(&mut |ui| panel(ui, &mut app), edit);
        session.frame(&mut |ui| panel(ui, &mut app), Vec::new());
        assert!(
            host.sets() > sets,
            "after Escape the keys were not the cursor's"
        );
        assert_eq!(app.working.stages[0].points[1].y, nudged);
    }

    /// The whole panel at the opening size, light and dark, for the owner's review of the
    /// layout-tree conversion (plans/plan-layout-tree.md §4.3):
    /// `target/layout-tree/mxm-fx-curve/<tag>/`, where `MXM_PICTURES` names the tag — `before` on
    /// the hand-laid control column, `after` on its trees. The editor is not paged, so this is one
    /// picture per theme: Init, and in `detector/` the same stage as a linked detector, which shows
    /// the ballistics grid.
    ///
    /// `MXM_PICTURES=after cargo test -p mxm-fx-curve --lib tree_pictures -- --ignored`
    #[test]
    #[ignore = "renders through wgpu; run by hand"]
    fn tree_pictures() {
        let tag = std::env::var("MXM_PICTURES").unwrap_or_else(|_| "after".to_owned());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/layout-tree/mxm-fx-curve")
            .join(tag);
        for (sub, detector) in [("", false), ("detector", true)] {
            let params = Arc::new(MxmFxCurveParams::default());
            let telemetry = Arc::new(Telemetry::default());
            let mut app = MxmFxCurveApp::new(params, telemetry);
            if detector {
                app.working.stages[0].mode = StageModeState::Detector {
                    attack_ms: 10.0,
                    release_ms: 120.0,
                };
            }
            tree_checks::pictures(
                &|_| {},
                egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
                &dir.join(sub),
                &mut |ui| panel(ui, &mut app),
            );
        }
    }

    /// A stage state the Stage body can be in: its number of stages, the selected one, and its
    /// mode.
    struct StageCase {
        name: &'static str,
        stages: usize,
        selected: usize,
        mode: StageModeState,
    }

    /// The Stage body's structural states: a memoryless stage (Linked off), a detector stage
    /// (Linked on) at its default ballistics, and the fifth of five stages as a detector at the
    /// ballistics' extremes, where the heading and the readings are at their longest.
    const STAGE_CASES: [StageCase; 3] = [
        StageCase {
            name: "memoryless stage, Linked off",
            stages: 1,
            selected: 0,
            mode: StageModeState::Memoryless,
        },
        StageCase {
            name: "detector stage, Linked on",
            stages: 1,
            selected: 0,
            mode: StageModeState::Detector {
                attack_ms: 10.0,
                release_ms: 120.0,
            },
        },
        StageCase {
            name: "fifth detector stage at the ballistics' extremes",
            stages: 5,
            selected: 4,
            mode: StageModeState::Detector {
                attack_ms: 0.1,
                release_ms: 4_999.99,
            },
        },
    ];

    /// The Output body's readings at their extremes: Auto makeup's estimate is bounded to ±24 dB.
    const ESTIMATES: [f32; 3] = [1.0, 0.063_095_73, 15.848_932];

    fn app_in(case: &StageCase) -> MxmFxCurveApp {
        let params = Arc::new(MxmFxCurveParams::default());
        let telemetry = Arc::new(Telemetry::default());
        let mut app = MxmFxCurveApp::new(params, telemetry);
        app.working.stages = std::iter::repeat_with(identity_stage)
            .take(case.stages)
            .collect();
        app.working.stages[case.selected].mode = case.mode;
        app.selected_stage = case.selected;
        app
    }

    /// The narrowest a body's module card can be, in an editor's context.
    fn content_floor(title: &str, build: &dyn Fn(&Ui) -> Node<Leaf>) -> f32 {
        let ctx = tree_checks::context(&|_| {});
        let mut floor = 0.0;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                floor = mxm_ui::tree::card_floor(ui, title, &build(ui));
            });
            output.textures_delta.clear();
        }
        floor
    }

    /// Every leaf of both bodies, in every stage state, passes the layout tree's shared checks
    /// (plans/plan-layout-tree.md §4.3, `tree_checks::card`): drawn in a module card at its content
    /// floor and wider, the floor is exact, the stated height is the drawn height, nothing paints
    /// outside the card and every leaf stays in the room it was given — the ballistics grid's
    /// stated size included. A module card's rhythm is not the local frame's; what this holds is
    /// each leaf's statement, and the column test below holds the frame.
    #[test]
    fn both_bodies_pass_the_tree_checks_in_every_stage_state() {
        for case in &STAGE_CASES {
            let mut app = app_in(case);
            let (stage, detector) = (app.selected_stage, app.selected_is_detector());
            let build = move |ui: &Ui| stage_tree(ui, stage, detector);
            tree_checks::card(
                &|_| {},
                case.name,
                "Stage",
                content_floor("Stage", &build),
                &build,
                &mut |ui, leaf, _| app.paint_stage(ui, *leaf),
            );
        }
        for gain in ESTIMATES {
            let mut app = app_in(&STAGE_CASES[0]);
            let params = Arc::clone(&app.params);
            let estimate = curve_estimate(gain);
            let build = |ui: &Ui| output_tree(ui, &params, &estimate);
            tree_checks::card(
                &|_| {},
                &estimate,
                "Output",
                content_floor("Output", &build),
                &build,
                &mut |ui, leaf, _| app.paint_output(ui, &mxm_ui::LIGHT, *leaf, &estimate),
            );
        }
    }

    /// **Both bodies fit the owner's 300-point column in every stage state** — this editor's
    /// exception keeps them local frames in a fixed column beside the canvas, so their trees are
    /// measured against that column rather than a paging floor. Drawn in their own frames, in the
    /// column's own spacing for the wide layout (no horizontal item spacing) and for the stacked
    /// one: the tree's narrowest and the frame's padding fit 300 points, the frame ends inside the
    /// column, and every leaf stays in its room.
    #[test]
    fn both_bodies_fit_the_300_point_column_in_every_stage_state() {
        let default_spacing = egui::Style::default().spacing.item_spacing.x;
        for (layout, spacing_x) in [("wide", 0.0), ("stacked", default_spacing)] {
            for case in &STAGE_CASES {
                for gain in ESTIMATES {
                    let mut app = app_in(case);
                    let estimate = curve_estimate(gain);
                    let ctx = tree_checks::context(&|_| {});
                    for _ in 0..3 {
                        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                            let tokens = mxm_ui::LIGHT;
                            let column = egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(CONTROL_COLUMN_WIDTH, 2_000.0),
                            );
                            let mut ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(column)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            ui.spacing_mut().item_spacing.x = spacing_x;
                            ui.set_width(CONTROL_COLUMN_WIDTH);
                            let mut strays = Vec::new();
                            let mut observe = |leaf: &Leaf, given: egui::Rect, drew: egui::Rect| {
                                if !given.expand(0.75).contains_rect(drew) {
                                    strays.push((*leaf, given, drew));
                                }
                            };
                            let stage = card(&tokens).show(&mut ui, |ui| {
                                let tree =
                                    stage_tree(ui, app.selected_stage, app.selected_is_detector());
                                let narrowest = tree.min_width(ui);
                                ui.set_max_width(stage_hug(ui, &tree));
                                tree::show_observed(
                                    ui,
                                    &tokens,
                                    &tree,
                                    &mut |ui, leaf, _| app.paint_stage(ui, *leaf),
                                    &mut observe,
                                );
                                narrowest
                            });
                            let out = card(&tokens).show(&mut ui, |ui| {
                                let tree = output_tree(ui, &app.params.clone(), &estimate);
                                let narrowest = tree.min_width(ui);
                                tree::show_observed(
                                    ui,
                                    &tokens,
                                    &tree,
                                    &mut |ui, leaf, _| {
                                        app.paint_output(ui, &tokens, *leaf, &estimate);
                                    },
                                    &mut observe,
                                );
                                narrowest
                            });
                            let who = format!("{} / {estimate} ({layout})", case.name);
                            for (body, frame) in [("Stage", &stage), ("Output", &out)] {
                                assert!(
                                    frame.inner + 2.0 * SPACE_5 <= CONTROL_COLUMN_WIDTH,
                                    "{who}: the {body} body needs {:.1} of the column's {CONTROL_COLUMN_WIDTH}",
                                    frame.inner + 2.0 * SPACE_5
                                );
                                assert!(
                                    column.expand(0.5).contains_rect(frame.response.rect),
                                    "{who}: the {body} frame {:?} leaves the column",
                                    frame.response.rect
                                );
                            }
                            assert!(strays.is_empty(), "{who}: leaves drew outside: {strays:?}");
                        });
                        output.textures_delta.clear();
                    }
                }
            }
        }
    }

    /// The ballistics grid is the size [`ballistics_size`] states: as tall, and never wider — its
    /// value boxes follow the readings, which the statement takes at their widest.
    #[test]
    fn the_ballistics_grid_is_the_size_it_states() {
        let default_spacing = egui::Style::default().spacing.item_spacing.x;
        for spacing_x in [0.0, default_spacing] {
            for case in &STAGE_CASES[1..] {
                let mut app = app_in(case);
                let ctx = tree_checks::context(&|_| {});
                let mut sizes = (Vec2::ZERO, Vec2::ZERO);
                for _ in 0..3 {
                    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                        ui.spacing_mut().item_spacing.x = spacing_x;
                        let top = ui.cursor().min;
                        let drew = ui.scope(|ui| app.ballistics(ui)).response.rect;
                        sizes = (drew.max - top, ballistics_size(ui));
                    });
                    output.textures_delta.clear();
                }
                let (drew, stated) = sizes;
                assert!(
                    (drew.y - stated.y).abs() < 0.5 && drew.x <= stated.x + 0.5,
                    "{} at {spacing_x}: drew {drew:?}, stated {stated:?}",
                    case.name
                );
            }
        }
    }

    #[test]
    fn editor_fits_quarter_4k() {
        assert!(REFERENCE.0 <= 1920 && REFERENCE.1 <= 1080);
    }

    #[test]
    fn a_wide_window_gives_its_extra_space_to_the_canvas() {
        let ordinary = wide_canvas_size(egui::vec2(992.0, 660.0));
        let maximized = wide_canvas_size(egui::vec2(3_808.0, 1_980.0));
        assert_eq!(ordinary.x, 472.0);
        assert!(ordinary.y >= CANVAS_FLOOR);
        assert!(maximized.x > 3_200.0);
        assert!(maximized.y > ordinary.y);
        assert_eq!(maximized.x - ordinary.x, 3_808.0 - 992.0);
    }

    #[test]
    fn meter_peak_hold_keeps_a_clip_visible_before_release() {
        let mut meter = MeterDisplay::default();
        meter.update(0.0, [1.0, 0.0], [0.0; 2]);
        meter.update(0.2, [0.1, 0.0], [0.0; 2]);
        assert_eq!(meter.input[0], 1.0);

        meter.update(0.46, [0.1, 0.0], [0.0; 2]);
        assert!(meter.input[0] < 1.0);
        assert!(meter.input[0] > 0.1);
    }

    #[test]
    fn identity_stage_is_valid() {
        CurveStackState {
            schema_version: crate::model::SCHEMA_VERSION,
            stages: vec![identity_stage()],
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn dragging_stage_chips_reorders_the_model_and_keeps_selection_on_its_stage() {
        let stage = |end| StageState {
            mode: StageModeState::Memoryless,
            points: vec![
                crate::model::PointState::curve(0.0, 0.0),
                crate::model::PointState::curve(1.0, end),
            ],
        };
        let mut model = CurveStackState {
            schema_version: crate::model::SCHEMA_VERSION,
            stages: vec![stage(0.25), stage(0.50), stage(0.75)],
        };
        let mut selected = 0;
        reorder_stage(&mut model, &mut selected, 0, 2, true);
        assert_eq!(
            model
                .stages
                .iter()
                .map(|stage| stage.points[1].y)
                .collect::<Vec<_>>(),
            [0.50, 0.75, 0.25]
        );
        assert_eq!(selected, 2);

        reorder_stage(&mut model, &mut selected, 2, 0, false);
        assert_eq!(
            model
                .stages
                .iter()
                .map(|stage| stage.points[1].y)
                .collect::<Vec<_>>(),
            [0.25, 0.50, 0.75]
        );
        assert_eq!(selected, 0);
    }

    #[test]
    fn deleting_a_stage_chip_keeps_a_valid_selected_stage() {
        let mut model = CurveStackState {
            schema_version: crate::model::SCHEMA_VERSION,
            stages: std::iter::repeat_with(identity_stage).take(3).collect(),
        };
        let mut selected = 2;
        delete_stage(&mut model, &mut selected, 1);
        assert_eq!(model.stages.len(), 2);
        assert_eq!(selected, 1);
        delete_stage(&mut model, &mut selected, 1);
        assert_eq!(model.stages.len(), 1);
        assert_eq!(selected, 0);
        delete_stage(&mut model, &mut selected, 0);
        assert_eq!(model.stages.len(), 1);
    }

    #[test]
    fn curve_line_contrast_clears_the_graphical_object_floor() {
        for tokens in [mxm_ui::DARK, mxm_ui::LIGHT] {
            assert!(mxm_ui::theme::contrast(tokens.accent, tokens.surface_1) >= 3.0);
            assert!(mxm_ui::theme::contrast(tokens.accent, tokens.canvas) >= 3.0);
        }
    }

    #[test]
    fn complete_surface_paints_inside_quarter_4k_at_zoom_extremes() {
        // Logical spaces presented by a 1920×1080 physical budget at 75% and 200% editor zoom.
        for size in [egui::vec2(2560.0, 1440.0), egui::vec2(960.0, 540.0)] {
            for theme in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
                let context = egui::Context::default();
                mxm_ui::theme::apply(&context);
                mxm_ui::typography::apply(&context);
                context.set_theme(theme);
                let params = Arc::new(MxmFxCurveParams::default());
                let telemetry = Arc::new(Telemetry::default());
                let mut app = MxmFxCurveApp::new(params, telemetry);
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                };
                let mut output = context.run_ui(input, |ui| panel(ui, &mut app));
                output.textures_delta.clear();
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for clipped in &output.shapes {
                    let bounds = clipped
                        .shape
                        .visual_bounding_rect()
                        .intersect(clipped.clip_rect);
                    if bounds.min.is_finite() && bounds.max.is_finite() && bounds.is_positive() {
                        assert!(
                            screen.expand(1.0).contains_rect(bounds),
                            "{theme:?} at {size:?} painted {bounds:?} outside the quarter-4K surface"
                        );
                    }
                }
            }
        }
    }
}
