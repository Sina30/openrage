use egui::{Response, Ui};
use or_transport::{TimeRange, Transport};

use super::ruler;
use super::time_map::TimeMap;
use crate::ui::theme;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrubResult {
    pub seek_to: Option<f32>,
    pub evaluate: bool,
    pub set_range: Option<TimeRange>,
    pub reset_range: bool,
}

impl ScrubResult {
    fn seek(time: f32, evaluate: bool) -> Self {
        Self {
            seek_to: Some(time),
            evaluate,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScrubState {
    range_anchor: Option<f32>,
}

impl ScrubState {
    pub fn handle(
        &mut self,
        ui: &Ui,
        response: &Response,
        map: &TimeMap,
        bounds: TimeRange,
    ) -> ScrubResult {
        if response.double_clicked() {
            self.range_anchor = None;
            return ScrubResult {
                reset_range: true,
                ..ScrubResult::default()
            };
        }

        let Some(pointer) = response.interact_pointer_pos() else {
            if let Some(anchor) = self.range_anchor.take() {
                if let Some(latest) = ui.input(|i| i.pointer.latest_pos()) {
                    return commit_range(anchor, map.x_to_time(latest.x), bounds);
                }
            }
            return ScrubResult::default();
        };
        let time = bounds.clamp(map.x_to_time(pointer.x));

        let shift = ui.input(|i| i.modifiers.shift);
        if shift && (response.dragged() || response.drag_started()) {
            let anchor = *self.range_anchor.get_or_insert(time);
            return commit_range(anchor, time, bounds);
        }
        self.range_anchor = None;

        if response.dragged() || response.clicked() {
            let middle = ui.input(|i| i.pointer.middle_down());
            return ScrubResult::seek(time, !middle);
        }
        ScrubResult::default()
    }
}

fn commit_range(anchor: f32, other: f32, bounds: TimeRange) -> ScrubResult {
    let (start, end) = if anchor <= other {
        (anchor, other)
    } else {
        (other, anchor)
    };
    if (end - start) < bounds.span() * 1e-3 {
        return ScrubResult::default();
    }
    ScrubResult {
        set_range: Some(TimeRange::new(start, end)),
        ..ScrubResult::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TransportAction {
    pub zoom_to_fit: bool,
}

/// The transport, with Play, Rate and Loop greyed under `no_playback`, the reason playing shows nothing.
pub fn draw(
    ui: &mut Ui,
    transport: &mut Transport,
    extra_summary: Option<&str>,
    no_playback: Option<&str>,
) -> TransportAction {
    let mut action = TransportAction::default();
    let plays = no_playback.is_none();
    let reason = no_playback.unwrap_or_default();

    // ⚠ Wrapped: a row that cannot shrink widens the panel's layout, and the tracks below it then
    // run past the panel's edge, so a narrow panel shows only the first frames.
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("⏮")
            .on_hover_text(tr!("Go to start (Home)"))
            .clicked()
        {
            transport.seek(transport.range.start);
        }
        if ui
            .button("⏪")
            .on_hover_text(tr!("Previous frame (Left)"))
            .clicked()
        {
            transport.step_frames(-1, FALLBACK_STEP);
        }

        let playing = transport.playing;
        let label = if playing { "⏸" } else { "▶" };
        if ui
            .add_enabled(plays, egui::Button::new(label))
            .on_hover_text(if playing {
                tr!("Pause (Space)")
            } else {
                tr!("Play (Space)")
            })
            .on_disabled_hover_text(reason)
            .clicked()
        {
            transport.toggle();
        }
        if ui
            .button("⏹")
            .on_hover_text(tr!("Stop and rewind"))
            .clicked()
        {
            transport.stop();
        }
        if ui
            .button("⏩")
            .on_hover_text(tr!("Next frame (Right)"))
            .clicked()
        {
            transport.step_frames(1, FALLBACK_STEP);
        }
        if ui
            .button("⏭")
            .on_hover_text(tr!("Go to end (End)"))
            .clicked()
        {
            transport.seek(transport.range.end);
        }

        ui.separator();

        ui.label(
            egui::RichText::new(ruler::format_time(transport))
                .monospace()
                .color(theme::TEXT_PRIMARY),
        )
        .on_hover_text(tr!("Elapsed · progress · frame"));

        ui.separator();

        ui.label(tr!("Rate"));
        let mut rate = transport.rate;
        let drag = ui.add_enabled(
            plays,
            egui::DragValue::new(&mut rate)
                .speed(0.05)
                .range(0.05..=8.0)
                .suffix("×"),
        );
        if crate::ui::accessible_name::named(drag, tr!("Playback rate"))
            .on_hover_text(tr!("Playback speed multiplier"))
            .on_disabled_hover_text(reason)
            .changed()
        {
            transport.rate = rate;
        }

        let mut looping = transport.is_looping();
        if ui
            .add_enabled(plays, egui::Checkbox::new(&mut looping, tr!("Loop")))
            .on_hover_text(tr!("Wrap at the end of the working range"))
            .on_disabled_hover_text(reason)
            .changed()
        {
            transport.set_looping(looping);
        }

        let narrowed = transport.range != transport.bounds;
        if narrowed {
            ui.separator();
            ui.label(
                egui::RichText::new(tr!(
                    "range {start}–{end}s",
                    start = format!("{:.2}", transport.range.start),
                    end = format!("{:.2}", transport.range.end)
                ))
                .small()
                .color(theme::ACCENT_TEXT),
            );
            if ui
                .small_button(egui_phosphor::regular::X)
                .on_hover_text(tr!("Clear the working range (or double-click the ruler)"))
                .clicked()
            {
                transport.reset_range();
            }
        }

        if let Some(summary) = extra_summary {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(summary)
                            .small()
                            .color(theme::TEXT_SECONDARY),
                    )
                    .truncate(),
                );
            });
        }
    });

    action.zoom_to_fit = false;
    action
}

pub const FALLBACK_STEP: f32 = 0.1;

pub fn handle_shortcuts(ui: &Ui, transport: &mut Transport, plays: bool) -> bool {
    // ⚠ Only over the timeline, and never while a widget has keyboard focus: a
    // space typed into a filter is text, and Home over the viewport frames it.
    if !ui.ui_contains_pointer() || ui.ctx().egui_wants_keyboard_input() {
        return false;
    }
    let mut handled = false;
    ui.input(|i| {
        if plays && i.key_pressed(egui::Key::Space) {
            transport.toggle();
            handled = true;
        }
        if i.key_pressed(egui::Key::ArrowLeft) {
            transport.step_frames(-1, FALLBACK_STEP);
            handled = true;
        }
        if i.key_pressed(egui::Key::ArrowRight) {
            transport.step_frames(1, FALLBACK_STEP);
            handled = true;
        }
        if i.key_pressed(egui::Key::Home) {
            transport.seek(transport.range.start);
            handled = true;
        }
        if i.key_pressed(egui::Key::End) {
            transport.seek(transport.range.end);
            handled = true;
        }
    });
    handled
}

pub fn handle_pan_zoom(ui: &Ui, response: &Response, map: &TimeMap, bounds: TimeRange) -> TimeMap {
    let mut out = *map;
    if !response.hovered() {
        return out;
    }
    let (scroll, zoom_modifier, pointer) = ui.input(|i| {
        (
            i.smooth_scroll_delta,
            i.modifiers.command || i.modifiers.ctrl,
            i.pointer.latest_pos(),
        )
    });

    if scroll.y != 0.0 && zoom_modifier {
        if let Some(pointer) = pointer {
            let factor = (scroll.y * 0.005).exp();
            out = out.zoom_at(pointer.x, factor);
        }
    } else if scroll.x != 0.0 {
        out = out.pan(scroll.x);
    }

    if response.dragged_by(egui::PointerButton::Secondary) {
        out = out.pan(response.drag_delta().x);
    }

    out.clamped_to(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_drag_produces_an_ordered_range() {
        let bounds = TimeRange::new(0.0, 10.0);
        let result = commit_range(7.0, 2.0, bounds);
        let range = result.set_range.expect("a range");
        assert!((range.start - 2.0).abs() < 1e-6);
        assert!((range.end - 7.0).abs() < 1e-6);
    }

    #[test]
    fn a_negligible_range_drag_is_ignored() {
        let bounds = TimeRange::new(0.0, 10.0);
        assert!(commit_range(5.0, 5.0, bounds).set_range.is_none());
        assert!(commit_range(5.0, 5.0001, bounds).set_range.is_none());
        assert!(commit_range(5.0, 6.0, bounds).set_range.is_some());
    }

    #[test]
    fn range_threshold_scales_with_the_asset_length() {
        let long = TimeRange::new(0.0, 10_000.0);
        assert!(commit_range(0.0, 1.0, long).set_range.is_none());
        assert!(commit_range(0.0, 100.0, long).set_range.is_some());
    }

    #[test]
    fn stepping_uses_frames_when_the_rate_is_known() {
        let mut transport = Transport {
            frame_rate: Some(25.0),
            ..Transport::with_duration(4.0)
        };
        transport.step_frames(1, FALLBACK_STEP);
        assert!((transport.time - 0.04).abs() < 1e-5);
    }

    #[test]
    fn stepping_falls_back_to_the_time_nudge() {
        let mut transport = Transport::with_duration(4.0);
        transport.step_frames(1, FALLBACK_STEP);
        assert!((transport.time - FALLBACK_STEP).abs() < 1e-6);
    }

    /// Draws a 200×100 timeline at the top left with a text field below it, puts
    /// the pointer at `pointer`, presses Space and says whether playback started.
    fn space_starts_playback(field_focused: bool, pointer: egui::Pos2) -> bool {
        let ctx = egui::Context::default();
        let mut transport = Transport::with_duration(4.0);
        let mut text = String::new();
        let space = vec![
            egui::Event::Key {
                key: egui::Key::Space,
                physical_key: Some(egui::Key::Space),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Text(" ".into()),
        ];
        for events in [vec![egui::Event::PointerMoved(pointer)], Vec::new(), space] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                ui.allocate_ui(egui::vec2(200.0, 100.0), |ui| {
                    ui.allocate_space(ui.available_size());
                    handle_shortcuts(ui, &mut transport, true);
                });
                let field = ui.text_edit_singleline(&mut text);
                if field_focused {
                    field.request_focus();
                }
            });
        }
        transport.playing
    }

    #[test]
    fn the_transport_row_keeps_to_a_narrow_panel() {
        let ctx = egui::Context::default();
        let mut transport = Transport::with_duration(4.0);
        transport.set_range(TimeRange::new(1.0, 2.0));
        let mut used = 0.0;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.allocate_ui(egui::vec2(160.0, 300.0), |ui| {
                draw(
                    ui,
                    &mut transport,
                    Some("a long summary of what plays"),
                    None,
                );
                used = ui.min_rect().width();
            });
        });
        assert!(used <= 160.5, "the row takes {used} of 160 points");
    }

    #[test]
    fn space_over_the_timeline_starts_playback() {
        assert!(space_starts_playback(false, egui::pos2(50.0, 50.0)));
    }

    #[test]
    fn a_space_typed_into_a_text_field_does_not_start_playback() {
        assert!(!space_starts_playback(true, egui::pos2(50.0, 50.0)));
    }

    #[test]
    fn space_with_the_pointer_elsewhere_leaves_the_timeline_alone() {
        assert!(!space_starts_playback(false, egui::pos2(300.0, 250.0)));
    }
}
