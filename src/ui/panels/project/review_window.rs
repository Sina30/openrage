//! Review: two versions of the pack side by side, read-only, with the garments, textures and settings added, changed and removed between them.
//!
//! The window writes nothing to the pack. Nothing here knows how two versions differ: `or-wardrobe` does, and the lines the window lists are its
//! review. The pictures of the two sides are the Wardrobe's Pictures tool, drawn once for each side of each garment that differs.

use or_wardrobe::review::{Kind, Review};

use super::readable::arrows;
use crate::platform::file_dialogs;
use crate::state::front_door::spoken;
use crate::state::workbench::Question;
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Review versions");

/// How a version is named in a choice: when it was kept and what the write that made it changed.
fn named(state: &AppState, n: Option<u64>) -> String {
    let Some(n) = n else {
        return tr!("Now, the pack as it is").to_string();
    };
    (state.workbench.wardrobe.versions.as_ref())
        .and_then(|v| v.list.iter().find(|(version, _)| version.n == n))
        .map_or_else(
            || tr!("a version that is not kept any more").to_string(),
            |(version, _)| {
                format!(
                    "{} · {}",
                    crate::state::prefs::ago(version.at),
                    spoken(&version.label)
                )
            },
        )
}

/// One row of the grid of the two sides: its name, then the choice of version, so that the two choices start at one edge.
fn side_choice(
    ui: &mut egui::Ui,
    state: &AppState,
    label: &str,
    id: &str,
    current: &mut Option<u64>,
    now: bool,
) {
    ui.label(label);
    let shown = named(state, *current);
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(360.0)
        .show_ui(ui, |ui| {
            if now {
                ui.selectable_value(current, None, named(state, None));
            }
            if let Some(versions) = &state.workbench.wardrobe.versions {
                for (version, _) in &versions.list {
                    ui.selectable_value(current, Some(version.n), named(state, Some(version.n)));
                }
            }
        });
    combo.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("{label} version"))
    });
    ui.end_row();
}

fn listing(ui: &mut egui::Ui, review: &Review) {
    egui::ScrollArea::vertical()
        .id_salt("review_lines")
        .max_height(320.0)
        .show(ui, |ui| {
            for g in &review.garments {
                let color = match g.kind {
                    Kind::Added => theme::SUCCESS,
                    Kind::Changed => theme::WARNING,
                    Kind::Removed => theme::ERROR,
                };
                ui.label(
                    egui::RichText::new(format!("{} {}", g.name, g.kind.word()))
                        .strong()
                        .color(color),
                );
                for line in &g.lines {
                    ui.label(arrows(line));
                }
                for t in &g.textures {
                    ui.label(tr!(
                        "texture {number} {word}: {file}",
                        number = t.number,
                        word = t.kind.word(),
                        file = t.file.as_deref().unwrap_or(tr!("empty texture"))
                    ));
                    for line in &t.lines {
                        ui.label(format!("    {}", arrows(line)));
                    }
                }
            }
            // What the pack states beyond its garments, each line saying what it is, so it needs no heading of its own.
            if !review.settings.is_empty() && !review.garments.is_empty() {
                ui.separator();
            }
            for line in &review.settings {
                ui.label(arrows(line));
            }
        });
}

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    mut from: Option<u64>,
    mut to: Option<u64>,
    mut shown: Option<std::sync::Arc<Review>>,
) {
    let (was_from, was_to) = (from, to);
    let mut close = false;
    let mut pictures = false;
    question_within(ctx, crate::i18n::tr(TITLE), 640.0, |ui| {
        ui.label(
            egui::RichText::new(tr!("Read-only: nothing here changes the pack."))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        egui::Grid::new("review_sides")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                side_choice(ui, state, tr!("From"), "review_from", &mut from, false);
                side_choice(ui, state, tr!("To"), "review_to", &mut to, true);
            });
        ui.separator();
        match &shown {
            None => {
                ui.label(tr!("One of the versions cannot be read."));
            }
            Some(review) => {
                ui.label(egui::RichText::new(review.summary()).strong());
                listing(ui, review);
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let any = shown.as_ref().is_some_and(|r| !r.garments.is_empty());
            if ui
                .add_enabled(any, egui::Button::new(tr!("Pictures of both sides…")))
                .on_hover_text(tr!("Draw each garment that differs as the older version held it and as the newer does, into a folder you pick: WebP pictures named for the garment, _before and _after"))
                .on_disabled_hover_text(tr!("No garment differs between these two versions"))
                .clicked()
            {
                pictures = true;
            }
            if ui.button(tr!("Close")).clicked() {
                close = true;
            }
        });
    });
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        close = true;
    }
    if (from, to) != (was_from, was_to) {
        shown = state.review_of(&module, from, to).map(std::sync::Arc::new);
    }
    if pictures {
        if let Some(out) = file_dialogs::pick_folder(tr!("Write the pictures of both sides into")) {
            state.review_pictures(&module, from, to, out);
        }
    }
    match close {
        true => state.workbench.asking = None,
        false => {
            state.workbench.asking = Some(Question::Review {
                module,
                from,
                to,
                shown,
            })
        }
    }
}
