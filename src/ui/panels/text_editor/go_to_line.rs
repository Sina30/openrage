//! Go to Line: a field over the editor that moves the caret to the line typed.

use egui::{Align2, Frame, Margin, Vec2};

use crate::ui::theme;

/// The field while it is open.
#[derive(Default)]
pub struct GoToLine {
    pub typed: String,
    /// Set when it opens, so the field takes the keys at once.
    pub focus: bool,
}

/// How wide the field is when the editor has room for it.
const FIELD_WIDTH: f32 = 80.0;

pub enum Outcome {
    Wait,
    /// Counted from 1, and within the document.
    Go(usize),
    Close,
}

/// The field, over the top right of the editor's `body`; what it was asked this frame.
pub fn draw(ui: &mut egui::Ui, body: egui::Rect, bar: &mut GoToLine, lines: usize) -> Outcome {
    let area = egui::Area::new(ui.id().with("text_editor_go_to_line"))
        .order(egui::Order::Foreground)
        .movable(false)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(body.right_top() + Vec2::new(-12.0, 12.0));
    let frame = Frame::group(ui.style())
        .fill(theme::BG_L1.get())
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .inner_margin(Margin::symmetric(8, 6));
    // The row wraps within what is left of `body` once 12 points clear each side.
    let room = (body.width() - 24.0 - frame.total_margin().sum().x).max(0.0);
    let field = area.show(ui.ctx(), |ui| {
        frame
            .show(ui, |ui| {
                ui.set_max_width(room);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("Go to line")
                            .color(theme::TEXT_SECONDARY)
                            .small(),
                    );
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut bar.typed)
                            .desired_width(FIELD_WIDTH.min(room))
                            .hint_text(format!("1 to {lines}")),
                    );
                    if std::mem::take(&mut bar.focus) {
                        field.request_focus();
                    }
                    field
                })
                .inner
            })
            .inner
    });
    let field = field.inner;
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if escape {
        return Outcome::Close;
    }
    if !(enter && field.lost_focus()) {
        return Outcome::Wait;
    }
    match bar.typed.trim().parse::<usize>() {
        Ok(line) => Outcome::Go(line.clamp(1, lines.max(1))),
        Err(_) => Outcome::Close,
    }
}
