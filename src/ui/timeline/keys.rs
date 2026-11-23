use egui::{Color32, FontId, Painter, Rect, Shape, Stroke};

use super::provider::{KeyEvent, KeyShape, SpanEvent};
use super::time_map::TimeMap;
use crate::ui::theme;

const KEY_RADIUS: f32 = 3.5;

fn selection_ring() -> Color32 {
    theme::ACCENT.get()
}

pub fn draw_keys(painter: &Painter, rect: Rect, map: &TimeMap, keys: &[KeyEvent]) {
    let centre_y = rect.center().y;
    for key in keys {
        let x = map.time_to_x(key.time);
        if x < rect.min.x - KEY_RADIUS || x > rect.max.x + KEY_RADIUS {
            continue;
        }
        draw_key_glyph(painter, egui::pos2(x, centre_y), key);
    }
}

pub fn draw_key_glyph(painter: &Painter, centre: egui::Pos2, key: &KeyEvent) {
    let r = KEY_RADIUS;
    // ⚠ A source's colours are literals that read on one theme's rows and not the other's.
    let colour = theme::graphic(key.colour);
    match key.shape {
        KeyShape::Diamond => {
            painter.add(Shape::convex_polygon(
                vec![
                    egui::pos2(centre.x, centre.y - r),
                    egui::pos2(centre.x + r, centre.y),
                    egui::pos2(centre.x, centre.y + r),
                    egui::pos2(centre.x - r, centre.y),
                ],
                colour,
                Stroke::NONE,
            ));
        }
        KeyShape::Square => {
            painter.rect_filled(
                Rect::from_center_size(centre, egui::vec2(r * 1.7, r * 1.7)),
                1.0,
                colour,
            );
        }
        KeyShape::Circle => {
            painter.circle_filled(centre, r * 0.9, colour);
        }
        KeyShape::Cross => {
            let stroke = Stroke::new(1.2_f32, colour);
            painter.line_segment(
                [
                    egui::pos2(centre.x - r, centre.y - r),
                    egui::pos2(centre.x + r, centre.y + r),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(centre.x - r, centre.y + r),
                    egui::pos2(centre.x + r, centre.y - r),
                ],
                stroke,
            );
        }
    }
    if key.selected {
        painter.circle_stroke(centre, r + 1.5, Stroke::new(1.2_f32, selection_ring()));
    }
}

pub fn draw_spans(painter: &Painter, rect: Rect, map: &TimeMap, spans: &[SpanEvent]) {
    for span in spans {
        let x0 = map.time_to_x(span.range.start);
        let x1 = map.time_to_x(span.range.end);
        if x1 < rect.min.x || x0 > rect.max.x {
            continue;
        }
        let colour = theme::graphic(span.colour);
        let clamped = Rect::from_min_max(
            egui::pos2(x0.max(rect.min.x), rect.min.y + 3.0),
            egui::pos2(
                x1.min(rect.max.x).max(x0.max(rect.min.x) + 1.5),
                rect.max.y - 3.0,
            ),
        );
        painter.rect_filled(clamped, 3.0, colour.gamma_multiply(0.55));
        painter.rect_stroke(
            clamped,
            3.0,
            Stroke::new(1.0_f32, colour),
            egui::StrokeKind::Inside,
        );

        if let Some(label) = &span.label {
            let galley = painter.layout_no_wrap(
                label.clone(),
                FontId::proportional(theme::SMALL_TEXT),
                theme::TEXT_PRIMARY.get(),
            );
            if clamped.width() > galley.size().x + 8.0 {
                let at = egui::pos2(
                    clamped.min.x + 4.0,
                    clamped.center().y - galley.size().y / 2.0,
                );
                // ⚠ No text colour reads on every span colour, so the label sits on a plate the colour of a panel.
                painter.rect_filled(
                    Rect::from_min_size(at, galley.size()).expand2(egui::vec2(2.0, 0.0)),
                    2.0,
                    theme::overlay_bg(),
                );
                painter.galley(at, galley, theme::TEXT_PRIMARY.get());
            }
        }
    }
}

pub fn draw_row_background(painter: &Painter, rect: Rect, index: usize, is_group: bool) {
    let fill = if is_group {
        theme::BG_L2.get()
    } else if index.is_multiple_of(2) {
        theme::BG_L1.gamma_multiply(0.6)
    } else {
        theme::BG_L1.gamma_multiply(0.85)
    };
    painter.rect_filled(rect, 0.0, fill);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_shape_is_distinct() {
        let shapes = [
            KeyShape::Diamond,
            KeyShape::Square,
            KeyShape::Circle,
            KeyShape::Cross,
        ];
        for (i, a) in shapes.iter().enumerate() {
            for b in shapes.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }
}
