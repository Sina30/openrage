//! A ring around the widget the keyboard is on.
//!
//! ⚠ egui paints a focused widget only as far as the widget lets it: a button
//! takes its pressed look, a row drawn by hand takes nothing. So this draws one
//! around every widget, once a key that moves focus has been pressed more
//! recently than the pointer.

use egui::{Context, Id, Rect, Stroke, StrokeKind};

use crate::ui::theme;

/// The ring's stroke, and the gap between it and its widget, in points.
const WIDTH: f32 = 2.0;
const GAP: f32 = 2.0;

const KEYBOARD: &str = "the_keyboard_was_used_after_the_pointer";
const TYPING: &str = "the_widget_that_reads_typed_text_itself";

/// Treat `id`, which has the keyboard and reads typed text itself, as egui treats a `TextEdit`: Space, Enter and the arrows in it are text, and bring no ring.
///
/// ⚠ Without the arrows in the focus lock egui moves the keyboard to the widget beside it.
pub fn reads_typed_text(ctx: &Context, id: Id) {
    ctx.data_mut(|data| data.insert_temp(Id::new(TYPING), id));
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
}

/// Whether a key that moves focus was pressed this frame: Tab anywhere, and the arrows, Space and Enter outside a text field, where they edit.
fn moves_focus(ctx: &Context) -> bool {
    use egui::Key::{ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Enter, Space, Tab};
    let typing = ctx.data(|data| data.get_temp::<Id>(Id::new(TYPING)));
    let in_a_field =
        ctx.text_edit_focused() || (typing.is_some() && typing == ctx.memory(|m| m.focused()));
    ctx.input(|i| {
        i.events.iter().any(|event| match event {
            egui::Event::Key {
                key: Tab,
                pressed: true,
                ..
            } => true,
            egui::Event::Key {
                key: ArrowUp | ArrowDown | ArrowLeft | ArrowRight | Space | Enter,
                pressed: true,
                ..
            } => !in_a_field,
            _ => false,
        })
    })
}

/// Draw the ring around the focused widget, at the end of the frame.
pub fn draw(ctx: &Context) {
    let pointer = ctx.input(|i| i.pointer.any_pressed());
    let keys = moves_focus(ctx);
    let keyboard = ctx.data_mut(|data| {
        let was = data.get_temp::<bool>(Id::new(KEYBOARD)).unwrap_or(false);
        let now = !pointer && (keys || was);
        data.insert_temp(Id::new(KEYBOARD), now);
        now
    });
    if !keyboard {
        return;
    }
    let Some(focused) = ctx
        .memory(|m| m.focused())
        .and_then(|id| ctx.read_response(id))
    else {
        return;
    };
    // ⚠ A widget scrolled out of its area keeps the focus while it is still built,
    // and its `rect` is where it would be: only `interact_rect` is clipped to what shows.
    if !focused.interact_rect.is_positive() {
        return;
    }
    // ⚠ egui does not say where its scroll area clips a widget, but `interact_rect` is the part that
    // shows: on a side where it falls short of `rect` the clip's edge is its edge, and the ring stops there.
    let (shown, whole) = (focused.interact_rect, focused.rect);
    let reach = |clipped: bool| if clipped { 0.0 } else { GAP + WIDTH };
    let clip = Rect::from_min_max(
        shown.min
            - egui::vec2(
                reach(whole.min.x < shown.min.x),
                reach(whole.min.y < shown.min.y),
            ),
        shown.max
            + egui::vec2(
                reach(whole.max.x > shown.max.x),
                reach(whole.max.y > shown.max.y),
            ),
    );
    // In the widget's own layer, so a menu or a window opened over it covers the ring too.
    ctx.layer_painter(focused.layer_id)
        .with_clip_rect(clip)
        .rect_stroke(
            shown.expand(GAP),
            theme::RADIUS_TINY,
            Stroke::new(WIDTH, theme::ACCENT_TEXT),
            StrokeKind::Outside,
        );
}

#[cfg(test)]
mod tests {
    use egui::epaint::ClippedShape;
    use egui::{Color32, Event, Key, Order, Rect, Shape};

    use super::*;

    fn flat(shapes: &[ClippedShape]) -> Vec<Shape> {
        fn push(shape: &Shape, out: &mut Vec<Shape>) {
            match shape {
                Shape::Vec(inner) => inner.iter().for_each(|shape| push(shape, out)),
                shape => out.push(shape.clone()),
            }
        }
        let mut out = Vec::new();
        shapes
            .iter()
            .for_each(|clipped| push(&clipped.shape, &mut out));
        out
    }

    /// Where the ring is among the shapes of a frame, in paint order.
    fn ring_at(shapes: &[ClippedShape]) -> Option<usize> {
        flat(shapes).iter().position(|shape| {
            matches!(shape, Shape::Rect(ring) if ring.stroke.color == theme::ACCENT_TEXT.get())
        })
    }

    /// How much of the ring shows in a frame: what it covers, cut to what its layer's clip lets through.
    fn ring_shows(shapes: &[ClippedShape]) -> Option<Rect> {
        fn find(shape: &Shape, clip: Rect) -> Option<Rect> {
            match shape {
                Shape::Vec(inner) => inner.iter().find_map(|shape| find(shape, clip)),
                Shape::Rect(ring) if ring.stroke.color == theme::ACCENT_TEXT.get() => {
                    let reach = match ring.stroke_kind {
                        StrokeKind::Inside => 0.0,
                        StrokeKind::Middle => ring.stroke.width / 2.0,
                        StrokeKind::Outside => ring.stroke.width,
                    };
                    Some(ring.rect.expand(reach).intersect(clip))
                }
                _ => None,
            }
        }
        shapes
            .iter()
            .find_map(|clipped| find(&clipped.shape, clipped.clip_rect))
    }

    /// A list of buttons in a scroll area `offset` points down, with an optional box over it.
    struct Screen {
        ctx: Context,
        time: f64,
        /// Where the scroll area shows its content, as of the last frame.
        area: Rect,
    }

    impl Screen {
        fn new() -> Self {
            Self {
                ctx: Context::default(),
                time: 0.0,
                area: Rect::NOTHING,
            }
        }

        fn frame(&mut self, events: Vec<Event>, offset: f32, over: bool) -> Vec<ClippedShape> {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 200.0),
                )),
                ..Default::default()
            };
            let mut area = Rect::NOTHING;
            let shapes = self
                .ctx
                .run_ui(input, |ui| {
                    // Room above the area, for a ring to bleed into.
                    ui.add_space(30.0);
                    area = egui::ScrollArea::vertical()
                        .max_height(100.0)
                        .vertical_scroll_offset(offset)
                        .show(ui, |ui| {
                            for n in 0..40 {
                                let _ = ui.button(format!("Button {n}"));
                            }
                        })
                        .inner_rect;
                    if over {
                        egui::Area::new(Id::new("a_menu"))
                            .order(Order::Foreground)
                            .fixed_pos(egui::pos2(0.0, 0.0))
                            .show(ui.ctx(), |ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(80.0, 40.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, 0.0, Color32::from_rgb(1, 2, 3));
                            });
                    }
                    draw(ui.ctx());
                })
                .shapes;
            self.area = area;
            shapes
        }

        /// Frames until Tab has reached the first button, and the last one's shapes.
        fn tabbed(&mut self, over: bool) -> Vec<ClippedShape> {
            self.frame(Vec::new(), 0.0, over);
            let tab = Event::Key {
                key: Key::Tab,
                physical_key: Some(Key::Tab),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![tab], 0.0, over)
        }
    }

    #[test]
    fn a_widget_scrolled_out_of_its_scroll_area_keeps_no_ring() {
        let _serial = theme::serial();
        let mut screen = Screen::new();
        assert!(
            ring_at(&screen.tabbed(false)).is_some(),
            "Tab to the first button drew no ring"
        );
        let scrolled = screen.frame(Vec::new(), 1000.0, false);
        assert!(
            ring_at(&scrolled).is_none(),
            "the ring stays where the button was before the list scrolled"
        );
    }

    /// ⚠ The ring is drawn outside its widget, so on a widget cut by its scroll area's edge it would lie over what is beyond the edge.
    #[test]
    fn a_widget_clipped_at_its_scroll_areas_edge_has_its_ring_inside_the_clip() {
        let _serial = theme::serial();
        let mut screen = Screen::new();
        screen.tabbed(false);
        for offset in [6.0, 12.0] {
            let shapes = screen.frame(Vec::new(), offset, false);
            let shown = ring_shows(&shapes).expect("the half-shown button keeps a ring");
            assert!(
                shown.is_positive(),
                "scrolled {offset}: nothing of the ring shows"
            );
            // egui lets what a scroll area holds draw a little past its edge, so that an outline is not cut.
            let clip = screen
                .area
                .expand(screen.ctx.global_style().visuals.clip_rect_margin);
            assert!(
                clip.contains_rect(shown),
                "scrolled {offset}: the ring shows over {shown:?}, which is outside the scroll area's clip {clip:?}"
            );
        }
    }

    #[test]
    fn a_menu_over_the_focused_widget_covers_its_ring() {
        let _serial = theme::serial();
        let mut screen = Screen::new();
        let shapes = screen.tabbed(true);
        let ring = ring_at(&shapes).expect("Tab to the first button drew no ring");
        let menu = flat(&shapes)
            .iter()
            .position(|shape| {
                matches!(shape, Shape::Rect(rect) if rect.fill == Color32::from_rgb(1, 2, 3))
            })
            .expect("the menu paints");
        assert!(ring < menu, "the ring is painted above the menu");
    }
}
