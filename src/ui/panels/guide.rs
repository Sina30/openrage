//! The guide, in a window: the document `docs/guides/Workbench_Guide.md` itself, which the binary carries,
//! opened at the heading a link on the start page names.

use egui::text::{LayoutJob, TextFormat};

use crate::state::AppState;
use crate::ui::theme;

/// The guide as written; a heading is a line that begins `## `, a bullet one that begins `- `, and a word between backticks is code.
const GUIDE: &str = include_str!("../../../docs/guides/Workbench_Guide.md");

const TITLE: &str = tr_key!("Guide");

/// The headings of the guide, which the links into it name.
pub fn headings() -> Vec<&'static str> {
    GUIDE
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .collect()
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    // A control's Shift+F1 over it is an ask to open the guide where it is explained.
    if let Some(section) = crate::ui::guide_links::take(ctx) {
        state.front_door.show_guide(&section);
    }
    let Some(guide) = state.front_door.guide.as_mut() else {
        return;
    };
    let mut open = true;
    egui::Window::new(crate::i18n::tr(TITLE))
        .id(egui::Id::new("workbench_guide"))
        .open(&mut open)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_size([640.0, 520.0])
        .show(ctx, |ui| {
            // ⚠ The guide is a long page of sections, and a row that names each of them is the only way to see what is in it.
            let mut jump: Option<&str> = None;
            ui.horizontal_wrapped(|ui| {
                for heading in headings() {
                    let button = ui.small_button(heading);
                    button.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            tr!("Guide section: {heading}", heading = heading),
                        )
                    });
                    if button.clicked() {
                        jump = Some(heading);
                    }
                }
            });
            ui.separator();
            if let Some(heading) = jump {
                guide.section = heading.to_string();
                guide.scrolled = false;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                body(ui, &guide.section, &mut guide.scrolled);
            });
        });
    let escaped =
        ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input();
    if !open || escaped {
        state.front_door.guide = None;
    }
}

fn body(ui: &mut egui::Ui, section: &str, scrolled: &mut bool) {
    for line in GUIDE.lines() {
        if line.starts_with("# ") {
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            ui.add_space(theme::SPACING_RELAXED);
            let response = ui.label(egui::RichText::new(heading).strong().size(16.0));
            if !*scrolled && heading == section {
                response.scroll_to_me(Some(egui::Align::TOP));
                *scrolled = true;
            }
        } else if let Some(item) = line.strip_prefix("- ") {
            ui.add(egui::Label::new(text(ui, &format!("• {item}"))).wrap());
        } else if line.is_empty() {
            ui.add_space(theme::SPACING_TIGHT);
        } else {
            ui.add(egui::Label::new(text(ui, line)).wrap());
        }
    }
}

/// `line` as a paragraph of text, the words between backticks in the monospace font.
fn text(ui: &egui::Ui, line: &str) -> LayoutJob {
    let colour = ui.visuals().text_color();
    let mut job = LayoutJob::default();
    for (i, part) in line.split('`').enumerate() {
        let font = if i % 2 == 1 {
            egui::TextStyle::Monospace
        } else {
            egui::TextStyle::Body
        };
        job.append(
            part,
            0.0,
            TextFormat::simple(font.resolve(ui.style()), colour),
        );
    }
    job
}
