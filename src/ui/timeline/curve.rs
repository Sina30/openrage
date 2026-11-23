use egui::accesskit::Role;
use egui::{Align2, Color32, FontId, Painter, Rect, Response, Sense, Stroke, Ui};

use super::provider::{CurveDomain, CurveSeries, TimelineEdit};
use super::ruler;
use super::time_map::TimeMap;
use crate::ui::{accessible_name, theme};

const HANDLE_RADIUS: f32 = 4.0;

const V_PADDING: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueRange {
    pub min: f32,
    pub max: f32,
}

impl ValueRange {
    pub fn of(series: &CurveSeries) -> Self {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for channel in &series.channels {
            for (_, value) in &channel.samples {
                if value.is_finite() {
                    min = min.min(*value);
                    max = max.max(*value);
                }
            }
        }
        if min > max {
            return Self { min: 0.0, max: 1.0 };
        }
        let span = max - min;
        if span < 1e-6 {
            let centre = min;
            Self {
                min: centre - 0.5,
                max: centre + 0.5,
            }
        } else {
            let pad = span * 0.08;
            Self {
                min: min - pad,
                max: max + pad,
            }
        }
    }

    pub fn span(&self) -> f32 {
        (self.max - self.min).max(1e-6)
    }

    pub fn value_to_y(&self, value: f32, rect: Rect) -> f32 {
        let inner_top = rect.min.y + V_PADDING;
        let inner_bottom = rect.max.y - V_PADDING;
        let height = (inner_bottom - inner_top).max(1.0);
        let normalized = ((value - self.min) / self.span()).clamp(0.0, 1.0);
        inner_bottom - normalized * height
    }

    pub fn y_to_value(&self, y: f32, rect: Rect) -> f32 {
        let inner_top = rect.min.y + V_PADDING;
        let inner_bottom = rect.max.y - V_PADDING;
        let height = (inner_bottom - inner_top).max(1.0);
        let normalized = ((inner_bottom - y) / height).clamp(0.0, 1.0);
        self.min + normalized * self.span()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CurveState {
    pub selected: usize,
    dragging: Option<(usize, usize)>,
}

pub fn draw_tabs(ui: &mut Ui, state: &mut CurveState, series: &[CurveSeries]) -> bool {
    if series.is_empty() {
        return false;
    }
    state.selected = state.selected.min(series.len() - 1);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (index, s) in series.iter().enumerate() {
            let label = egui::RichText::new(&s.label).small();
            if ui
                .selectable_label(state.selected == index, label)
                .on_hover_text(format!("{} channels", s.channels.len()))
                .clicked()
                && state.selected != index
            {
                state.selected = index;
                changed = true;
            }
        }
    });
    changed
}

pub struct CurveDraw<'a> {
    pub rect: Rect,
    pub timeline_map: &'a TimeMap,
    pub series: &'a CurveSeries,
    pub series_index: usize,
    pub playhead: f32,
    pub editable: bool,
}

pub fn draw_series(
    ui: &Ui,
    draw: &CurveDraw<'_>,
    state: &mut CurveState,
    response: &Response,
    edits: &mut Vec<TimelineEdit>,
) -> Option<f32> {
    let CurveDraw {
        rect,
        timeline_map,
        series,
        playhead,
        editable,
        ..
    } = *draw;
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, theme::BG_L1.gamma_multiply(0.6));

    let (map, shares_the_clock) = match series.domain {
        CurveDomain::Time => (*timeline_map, true),
        CurveDomain::Normalized => (
            TimeMap::new(
                or_transport::TimeRange::new(0.0, 1.0),
                rect.min.x,
                rect.max.x,
            ),
            false,
        ),
    };
    let map = &map;

    let range = ValueRange::of(series);

    if range.min < 0.0 && range.max > 0.0 {
        let y = range.value_to_y(0.0, rect);
        painter.line_segment(
            [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
            Stroke::new(1.0_f32, theme::BORDER.gamma_multiply(0.6)),
        );
    }

    painter.text(
        egui::pos2(rect.min.x + 3.0, rect.min.y + 1.0),
        Align2::LEFT_TOP,
        format!("{:.3}", range.max),
        FontId::monospace(theme::SMALL_TEXT),
        theme::TEXT_SECONDARY.get(),
    );
    painter.text(
        egui::pos2(rect.min.x + 3.0, rect.max.y - 1.0),
        Align2::LEFT_BOTTOM,
        format!("{:.3}", range.min),
        FontId::monospace(theme::SMALL_TEXT),
        theme::TEXT_SECONDARY.get(),
    );

    for channel in &series.channels {
        draw_channel_line(painter, rect, map, &range, &channel.samples, channel.colour);
    }

    if editable {
        draw_handles(ui, map, &range, draw, state, edits);
    }

    if shares_the_clock {
        ruler::draw_playhead(painter, rect, map, playhead);
    } else {
        if !series.domain_label.is_empty() {
            painter.text(
                egui::pos2(rect.center().x, rect.max.y - 1.0),
                Align2::CENTER_BOTTOM,
                &series.domain_label,
                FontId::proportional(theme::SMALL_TEXT),
                theme::TEXT_SECONDARY.get(),
            );
        }
    }

    if shares_the_clock && state.dragging.is_none() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if (response.dragged() || response.clicked()) && rect.contains(pointer) {
                return Some(map.x_to_time(pointer.x));
            }
        }
    }
    None
}

fn draw_channel_line(
    painter: &Painter,
    rect: Rect,
    map: &TimeMap,
    range: &ValueRange,
    samples: &[(f32, f32)],
    colour: Color32,
) {
    let colour = theme::graphic(colour);
    if samples.len() < 2 {
        if let Some((_, value)) = samples.first() {
            let y = range.value_to_y(*value, rect);
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                Stroke::new(1.5_f32, colour),
            );
        }
        return;
    }

    let view = map.view();
    let mut points = Vec::new();
    for window in samples.windows(2) {
        let (t0, v0) = window[0];
        let (t1, v1) = window[1];
        if t1 < view.start || t0 > view.end {
            continue;
        }
        if points.is_empty() {
            points.push(egui::pos2(map.time_to_x(t0), range.value_to_y(v0, rect)));
        }
        points.push(egui::pos2(map.time_to_x(t1), range.value_to_y(v1, rect)));
    }
    if points.len() >= 2 {
        painter.add(egui::Shape::line(points, Stroke::new(1.5_f32, colour)));
    }
}

fn draw_handles(
    ui: &Ui,
    map: &TimeMap,
    range: &ValueRange,
    draw: &CurveDraw<'_>,
    state: &mut CurveState,
    edits: &mut Vec<TimelineEdit>,
) {
    let (rect, series, series_index) = (draw.rect, draw.series, draw.series_index);
    let painter = ui.painter();
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let primary_down = ui.input(|i| i.pointer.primary_down());

    if let Some((dragging_series, dragging_index)) = state.dragging {
        if !primary_down {
            state.dragging = None;
        } else if dragging_series == series_index {
            if let Some(pointer) = pointer {
                if let Some(handle) = series.handles.get(dragging_index) {
                    edits.push(TimelineEdit {
                        series: series_index,
                        channel: handle.channel,
                        index: handle.index,
                        time: map.x_to_time(pointer.x),
                        value: range.y_to_value(pointer.y, rect),
                    });
                }
            }
        }
    }

    for (slot, handle) in series.handles.iter().enumerate() {
        let x = map.time_to_x(handle.time);
        if x < rect.min.x - HANDLE_RADIUS || x > rect.max.x + HANDLE_RADIUS {
            continue;
        }
        let centre = egui::pos2(x, range.value_to_y(handle.value, rect));
        let colour = theme::graphic(
            series
                .channels
                .get(handle.channel)
                .map(|c| c.colour)
                .unwrap_or(theme::ACCENT.get()),
        );

        let hit =
            Rect::from_center_size(centre, egui::vec2(HANDLE_RADIUS * 3.0, HANDLE_RADIUS * 3.0));
        // ⚠ The pointer alone grabs a handle, so this describes it and takes no input.
        accessible_name::described(
            ui.ctx(),
            ui.id().with(("curve_handle", series_index, slot)),
            hit,
            Role::Button,
            &format!(
                "{} channel {} key {}: time {:.3}, value {:.3}",
                series.label,
                handle.channel + 1,
                handle.index + 1,
                handle.time,
                handle.value
            ),
        );
        let hovered = pointer.is_some_and(|p| hit.contains(p));
        let active = state.dragging == Some((series_index, slot));

        painter.circle_filled(centre, HANDLE_RADIUS, colour);
        if hovered || active {
            painter.circle_stroke(
                centre,
                HANDLE_RADIUS + 2.0,
                Stroke::new(1.5_f32, theme::ACCENT),
            );
        }

        if hovered && primary_down && state.dragging.is_none() {
            state.dragging = Some((series_index, slot));
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }
}

pub fn allocate(ui: &mut Ui, height: f32) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height.max(24.0)),
        Sense::click_and_drag(),
    );
    (rect, accessible_name::named(response, "Curve view"))
}

#[cfg(test)]
mod tests {
    use super::super::provider::CurveChannel;
    use super::*;

    fn series(samples: Vec<(f32, f32)>) -> CurveSeries {
        CurveSeries {
            label: "test".into(),
            channels: vec![CurveChannel {
                colour: Color32::WHITE,
                samples,
            }],
            handles: Vec::new(),
            ..CurveSeries::default()
        }
    }

    fn rect() -> Rect {
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0))
    }

    #[test]
    fn value_range_spans_every_channel_with_padding() {
        let mut s = series(vec![(0.0, 0.0), (1.0, 10.0)]);
        s.channels.push(CurveChannel {
            colour: Color32::WHITE,
            samples: vec![(0.0, -5.0), (1.0, 3.0)],
        });
        let range = ValueRange::of(&s);
        assert!(
            range.min < -5.0,
            "min {} should be padded below -5",
            range.min
        );
        assert!(
            range.max > 10.0,
            "max {} should be padded above 10",
            range.max
        );
    }

    #[test]
    fn a_constant_series_gets_a_usable_range() {
        let range = ValueRange::of(&series(vec![(0.0, 7.0), (1.0, 7.0)]));
        assert!(range.span() > 0.0);
        assert!(range.min < 7.0 && range.max > 7.0);
        let y = range.value_to_y(7.0, rect());
        assert!((y - rect().center().y).abs() < 1.0, "y = {y}");
    }

    #[test]
    fn an_empty_series_gets_a_default_range() {
        let range = ValueRange::of(&series(vec![]));
        assert_eq!(range.min, 0.0);
        assert_eq!(range.max, 1.0);
    }

    #[test]
    fn non_finite_samples_are_ignored() {
        let range = ValueRange::of(&series(vec![
            (0.0, f32::NAN),
            (1.0, 2.0),
            (2.0, f32::INFINITY),
            (3.0, 4.0),
        ]));
        assert!(range.min.is_finite() && range.max.is_finite());
        assert!(range.min < 2.0 && range.max > 4.0);
    }

    #[test]
    fn a_series_of_only_non_finite_samples_falls_back() {
        let range = ValueRange::of(&series(vec![(0.0, f32::NAN)]));
        assert_eq!(range.min, 0.0);
        assert_eq!(range.max, 1.0);
    }

    #[test]
    fn value_and_y_are_inverses() {
        let range = ValueRange {
            min: -1.0,
            max: 3.0,
        };
        let r = rect();
        for value in [-1.0, 0.0, 1.5, 3.0] {
            let y = range.value_to_y(value, r);
            let back = range.y_to_value(y, r);
            assert!((back - value).abs() < 1e-3, "{value} -> {y} -> {back}");
        }
    }

    #[test]
    fn larger_values_map_higher() {
        let range = ValueRange {
            min: 0.0,
            max: 10.0,
        };
        let r = rect();
        assert!(range.value_to_y(10.0, r) < range.value_to_y(0.0, r));
    }

    #[test]
    fn values_outside_the_range_are_clamped_into_the_pane() {
        let range = ValueRange { min: 0.0, max: 1.0 };
        let r = rect();
        let above = range.value_to_y(99.0, r);
        let below = range.value_to_y(-99.0, r);
        assert!(above >= r.min.y && above <= r.max.y);
        assert!(below >= r.min.y && below <= r.max.y);
    }

    #[test]
    fn a_zero_height_pane_does_not_divide_by_zero() {
        let range = ValueRange { min: 0.0, max: 1.0 };
        let flat = Rect::from_min_max(egui::pos2(0.0, 50.0), egui::pos2(100.0, 50.0));
        assert!(range.value_to_y(0.5, flat).is_finite());
        assert!(range.y_to_value(50.0, flat).is_finite());
    }
}
