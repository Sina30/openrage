//! The question Create DLC pack… asks before anything is written, and the
//! window a built pack leaves: the line `dlclist.xml` needs, and its folder.

use or_title::dlc::SetupContentType;

use super::{drag_value, question};
use crate::platform::file_dialogs;
use crate::state::{AppState, RpfOutputMode};
use crate::ui::theme;

/// The types `openrage-cli dlc create --type` takes, in words.
const TYPES: [(SetupContentType, &str); 4] = [
    (SetupContentType::CompatPack, tr_key!("Compatibility pack")),
    (SetupContentType::LevelPack, tr_key!("Level pack")),
    (SetupContentType::TitleUpdate, tr_key!("Title update")),
    (SetupContentType::PaidPack, tr_key!("Paid pack")),
];

/// ⚠ A folder of thousands of files lists its first ones, so no frame lays out every one.
const LISTED: usize = 200;

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    draw_question(ctx, state);
    draw_built(ctx, state);
}

fn draw_question(ctx: &egui::Context, state: &mut AppState) {
    let Some(asked) = state.dlc_pack.asked.as_mut() else {
        return;
    };
    let mut answer: Option<bool> = None;
    question(ctx, tr!("Create DLC pack"), |ui| {
        ui.set_min_width(640.0);
        ui.label(egui::RichText::new(asked.folder.display().to_string()).monospace());
        ui.add_space(theme::SPACING_SNUG);
        let mut changed = false;
        egui::Grid::new("dlc_pack_answers")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(tr!("Name"));
                let name = ui.text_edit_singleline(&mut asked.spec.name);
                if std::mem::take(&mut asked.focus_name) {
                    name.request_focus();
                }
                changed |= name.changed();
                ui.end_row();
                ui.label(tr!("Type"));
                ui.horizontal(|ui| {
                    for (kind, words) in TYPES {
                        let picked = ui.selectable_value(
                            &mut asked.spec.content_type,
                            kind,
                            crate::i18n::tr(words),
                        );
                        changed |= picked.changed();
                    }
                });
                ui.end_row();
                ui.label(tr!("Order"));
                ui.horizontal(|ui| {
                    changed |= drag_value(ui, &mut asked.spec.order, |d| d).inner;
                    ui.label(tr!("minor order"));
                    changed |= drag_value(ui, &mut asked.spec.minor_order, |d| d).inner;
                });
                ui.end_row();
                ui.label(tr!("Encryption"));
                ui.horizontal(|ui| {
                    for mode in RpfOutputMode::ALL {
                        let picked = ui.selectable_value(&mut asked.encryption, mode, mode.label());
                        changed |= picked.changed();
                    }
                });
                ui.end_row();
            });
        if changed {
            asked.recheck();
        }

        ui.separator();
        let files = &asked.spec.files;
        ui.label(
            egui::RichText::new(tr!(
                "Into dlc.rpf go setup2.xml, content.xml and the folder's {count} file(s):",
                count = files.len()
            ))
            .strong(),
        );
        egui::ScrollArea::vertical()
            .id_salt("dlc_pack_files")
            .max_height(160.0)
            .show(ui, |ui| {
                for file in files.iter().take(LISTED) {
                    let kind = match file.file_type.as_str() {
                        "" => tr!("no fileType"),
                        kind => kind,
                    };
                    let line = format!("{}    {kind}", file.archive_path);
                    ui.label(egui::RichText::new(line).monospace());
                }
                if files.len() > LISTED {
                    ui.label(tr!("… and {count} more", count = files.len() - LISTED));
                }
            });
        ui.add_space(theme::SPACING_SNUG);
        if asked.refused.is_empty() {
            ui.label(egui::RichText::new(tr!("Nothing in it is refused.")).color(theme::SUCCESS));
        } else {
            ui.label(
                egui::RichText::new(tr!("It is refused:"))
                    .strong()
                    .color(theme::ERROR),
            );
            egui::ScrollArea::vertical()
                .id_salt("dlc_pack_refused")
                .max_height(120.0)
                .show(ui, |ui| {
                    for why in asked.refused.iter().take(LISTED) {
                        ui.label(egui::RichText::new(why).color(theme::ERROR));
                    }
                    if asked.refused.len() > LISTED {
                        ui.label(tr!(
                            "… and {count} more",
                            count = asked.refused.len() - LISTED
                        ));
                    }
                });
        }
        ui.add_space(theme::SPACING_SNUG);
        ui.label(tr!("It registers its files against {content_device}:/, and dlclist.xml needs this line, inside <Paths>:", content_device = asked.spec.content_device()));
        ui.label(egui::RichText::new(asked.line()).monospace());
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            let create = ui
                .add_enabled(asked.refused.is_empty(), egui::Button::new(tr!("Create…")))
                .on_hover_text(tr!(
                    "Choose the folder the pack's folder is written into, outside any game install"
                ))
                .on_disabled_hover_text(tr!("Nothing is written while the pack is refused"));
            if create.clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    match answer {
        Some(true) => {
            if let Some(dest) = file_dialogs::pick_dlc_pack_destination() {
                state.create_dlc_pack(dest);
            }
        }
        Some(false) => state.dlc_pack.asked = None,
        None => {}
    }
}

fn draw_built(ctx: &egui::Context, state: &mut AppState) {
    let Some(built) = &state.dlc_pack.built else {
        return;
    };
    let mut close = false;
    question(ctx, tr!("DLC pack built"), |ui| {
        ui.set_min_width(520.0);
        ui.label(tr!(
            "{folder} is written.",
            folder = built.folder.join("dlc.rpf").display()
        ));
        ui.label(tr!(
            "Install it by copying its folder into update/x64/dlcpacks and adding this line \
             to dlclist.xml, inside <Paths>:"
        ));
        ui.label(egui::RichText::new(&built.line).monospace());
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if ui.button(tr!("Copy line")).clicked() {
                ui.ctx().copy_text(built.line.clone());
            }
            if ui.button(tr!("Open folder")).clicked() {
                crate::platform::shell::open_folder(&built.folder);
            }
            if ui.button(tr!("Close")).clicked() {
                close = true;
            }
        });
    });
    if close {
        state.dlc_pack.built = None;
    }
}
