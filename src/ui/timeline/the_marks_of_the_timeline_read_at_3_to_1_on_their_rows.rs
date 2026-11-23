//! Every colour the animation timeline's source names, drawn as a key, a span's
//! edge, a curve, a curve's handle or a density graph, reads at 3:1 on the row
//! it is drawn on, in both themes: the colours are literals, and a pastel that
//! reads on a dark panel does not on a light one.
//!
//! ⚠ Judged at a mark's full opacity: a density graph's fainter columns and a
//! muted row's dimmed marks are dim on purpose, and say so another way.

use egui::epaint::ColorMode;
use egui::{Color32, Pos2, Rect, Rgba, Shape};
use or_io::codec::anim::ChannelFormat;
use or_transport::TimeRange;

use super::channels::{event_colour, RowStates};
use super::curve::{self, CurveDraw, CurveState};
use super::keys;
use super::provider::{
    ChannelRow, CurveChannel, CurveDomain, CurveHandle, CurveSeries, KeyEvent, KeyShape, RowId,
    SpanEvent,
};
use super::time_map::TimeMap;
use super::DensityGraph;
use crate::state::{AppearancePrefs, ThemeChoice};
use crate::ui::panels::timeline_ycd::{format_colour, ROTATION, SCALE, TAG_SPAN, TRANSLATION};
use crate::ui::theme::{self, contrast, GRAPHIC};

/// The colours the YCD timeline gives its sources' marks, by name.
fn literals() -> Vec<(String, Color32)> {
    let mut named = vec![
        ("translation".to_string(), TRANSLATION),
        ("rotation".to_string(), ROTATION),
        ("scale".to_string(), SCALE),
        ("tag span".to_string(), TAG_SPAN),
    ];
    named.extend(
        ChannelFormat::ALL
            .into_iter()
            .map(|format| (format!("{format:?} channels"), format_colour(format))),
    );
    named
}

/// `top` over an opaque `under`, as the framebuffer blends them.
fn over(top: Color32, under: Color32) -> Color32 {
    let kept = 1.0 - f32::from(top.a()) / 255.0;
    Color32::from(Rgba::from(top) + Rgba::from(under) * kept)
}

/// The shapes `draw` paints in one frame of a 400 by 200 canvas, flat, in paint order.
fn painted(ctx: &egui::Context, mut draw: impl FnMut(&mut egui::Ui)) -> Vec<Shape> {
    fn push(shape: Shape, out: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(inner) => inner.into_iter().for_each(|shape| push(shape, out)),
            shape => out.push(shape),
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 200.0))),
        ..Default::default()
    };
    let mut out = Vec::new();
    for clipped in ctx.run_ui(input, |ui| draw(ui)).shapes {
        push(clipped.shape, &mut out);
    }
    out
}

/// The colour a mark is drawn in: a fill, else a stroke, else a mesh's strongest column.
fn drawn(shape: &Shape) -> Option<Color32> {
    match shape {
        Shape::Circle(circle) => Some(if circle.fill.a() > 0 {
            circle.fill
        } else {
            circle.stroke.color
        }),
        Shape::Rect(rect) => Some(if rect.fill.a() > 0 {
            rect.fill
        } else {
            rect.stroke.color
        }),
        Shape::LineSegment { stroke, .. } => Some(stroke.color),
        Shape::Path(path) if path.fill.a() > 0 => Some(path.fill),
        Shape::Path(path) => match &path.stroke.color {
            ColorMode::Solid(colour) => Some(*colour),
            ColorMode::UV(_) => None,
        },
        Shape::Mesh(mesh) => mesh.vertices.iter().map(|v| v.color).max_by_key(|c| c.a()),
        _ => None,
    }
}

fn key(time: f32, shape: KeyShape, colour: Color32) -> KeyEvent {
    KeyEvent {
        time,
        shape,
        colour,
        selected: false,
    }
}

/// What the timeline paints under its marks: the body, then a row's fill over it.
struct Rows {
    /// A group row, an even row and an odd row, each with the fill painted before it.
    backdrops: Vec<Color32>,
    rect: Rect,
}

fn rows(ctx: &egui::Context) -> Rows {
    let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(380.0, 20.0));
    let body = theme::BG_L0.get();
    let backdrops = [(0, true), (0, false), (1, false)]
        .into_iter()
        .map(|(index, group)| {
            let shapes = painted(ctx, |ui| {
                keys::draw_row_background(ui.painter(), rect, index, group);
            });
            let Some(Shape::Rect(fill)) = shapes.first() else {
                panic!("a row's background is its first shape: {shapes:?}");
            };
            over(fill.fill, body)
        })
        .collect();
    Rows { backdrops, rect }
}

fn judged(
    found: &mut Vec<String>,
    theme_name: &str,
    what: &str,
    colour: Color32,
    backdrops: &[Color32],
) {
    for backdrop in backdrops {
        let ratio = contrast(over(colour, *backdrop), *backdrop);
        if ratio < GRAPHIC {
            found.push(format!(
                "{theme_name}: {what} {colour:?} on {backdrop:?} is {ratio:.2}:1"
            ));
        }
    }
}

fn marks_read(choice: ThemeChoice) -> Vec<String> {
    let appearance = AppearancePrefs {
        theme: choice,
        ..AppearancePrefs::default()
    };
    let ctx = crate::app::gpu::configured_egui_context(None, &appearance);
    let rows = rows(&ctx);
    let map = TimeMap::new(TimeRange::new(0.0, 1.0), rows.rect.min.x, rows.rect.max.x);
    let name = format!("{choice:?}");
    let mut found = Vec::new();
    for (source, colour) in literals() {
        // Keys of every shape.
        let keys: Vec<KeyEvent> = [
            KeyShape::Diamond,
            KeyShape::Square,
            KeyShape::Circle,
            KeyShape::Cross,
        ]
        .into_iter()
        .enumerate()
        .map(|(n, shape)| key(0.2 * (n as f32 + 1.0), shape, colour))
        .collect();
        for shape in painted(&ctx, |ui| {
            keys::draw_keys(ui.painter(), rows.rect, &map, &keys)
        }) {
            if let Some(mark) = drawn(&shape) {
                judged(
                    &mut found,
                    &name,
                    &format!("a key of {source}"),
                    mark,
                    &rows.backdrops,
                );
            }
        }

        // A span's edge, which is the colour in full.
        let spans = [SpanEvent {
            range: TimeRange::new(0.2, 0.8),
            colour,
            label: None,
        }];
        for shape in painted(&ctx, |ui| {
            keys::draw_spans(ui.painter(), rows.rect, &map, &spans)
        }) {
            if let Shape::Rect(edge) = &shape {
                if edge.fill.a() == 0 {
                    judged(
                        &mut found,
                        &name,
                        &format!("a span of {source}"),
                        edge.stroke.color,
                        &rows.backdrops,
                    );
                }
            }
        }

        // A density graph, at its tallest column, in the colour the timeline gives it for its row.
        let positions: Vec<f32> = (0..2000)
            .map(|n| rows.rect.min.x + n as f32 * 0.19)
            .collect();
        let row = ChannelRow::group(RowId::new(0, 0, 0), "row").with_colour(colour);
        let density = event_colour(&row, &RowStates::default());
        for shape in painted(&ctx, |ui| {
            let mut graph = DensityGraph::default();
            graph.paint(ui.painter(), rows.rect, positions.clone(), density);
        }) {
            if let Some(mark) = drawn(&shape) {
                judged(
                    &mut found,
                    &name,
                    &format!("a density graph of {source}"),
                    mark,
                    &rows.backdrops,
                );
            }
        }

        // A curve and its handles, on the curve view's fill over the body or over a panel.
        let series = CurveSeries {
            label: "Translation".into(),
            channels: vec![CurveChannel {
                colour,
                samples: vec![(0.0, -1.0), (0.5, 0.25), (1.0, 1.0)],
            }],
            handles: vec![CurveHandle {
                channel: 0,
                index: 1,
                time: 0.5,
                value: 0.25,
            }],
            domain: CurveDomain::Normalized,
            domain_label: String::new(),
        };
        let shapes = painted(&ctx, |ui| {
            let (rect, response) = curve::allocate(ui, 120.0);
            let draw = CurveDraw {
                rect,
                timeline_map: &map,
                series: &series,
                series_index: 0,
                playhead: 0.0,
                editable: true,
            };
            let mut edits = Vec::new();
            curve::draw_series(ui, &draw, &mut CurveState::default(), &response, &mut edits);
        });
        let curve_fill = theme::BG_L1.gamma_multiply(0.6);
        let behind = [theme::BG_L0.get(), theme::BG_L1.get()].map(|panel| over(curve_fill, panel));
        for shape in &shapes {
            // The curve's line and its handle: a path or a circle, where the rest is text, the zero line and the playhead.
            if matches!(shape, Shape::Path(_) | Shape::Circle(_)) {
                if let Some(mark) = drawn(shape) {
                    judged(
                        &mut found,
                        &name,
                        &format!("a curve of {source}"),
                        mark,
                        &behind,
                    );
                }
            }
        }
    }
    // The playhead crosses every row, selected or not, and the ruler above them.
    let crossed = [
        theme::BG_L0,
        theme::BG_L1,
        theme::BG_L2,
        theme::BG_L3,
        theme::SELECTION_BG,
    ]
    .map(theme::Themed::get);
    for shape in painted(&ctx, |ui| {
        super::ruler::draw_playhead(ui.painter(), rows.rect, &map, 0.5)
    }) {
        if let Some(mark) = drawn(&shape) {
            judged(&mut found, &name, "the playhead", mark, &crossed);
        }
    }
    found
}

#[test]
fn every_colour_the_timeline_draws_a_mark_in_reads_at_3_to_1_on_its_row_in_both_themes() {
    let _serial = theme::serial();
    let mut found = Vec::new();
    for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
        found.extend(marks_read(choice));
    }
    crate::ui::theme::apply_theme(&egui::Context::default(), &AppearancePrefs::default());
    found.sort();
    found.dedup();
    assert!(
        found.is_empty(),
        "{} marks read under {GRAPHIC}:1:\n{}",
        found.len(),
        found.join("\n")
    );
}
