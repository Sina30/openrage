//! The Mods panel: what the active install's mods are, in the load order that decides which of
//! two mods writing one path wins, with each mod's kind, status and files.

use egui_phosphor::regular as ph;
use or_title::mods::{ClaimKind, InstalledMod, ModPlace, ModStatus};

use super::question;
use crate::state::AppState;
use crate::ui::theme;

/// ⚠ A mod of thousands of files lists its first ones, so no frame lays out every one.
const LISTED: usize = 200;

/// What the user did this frame, applied once the lists are no longer borrowed.
#[derive(Default)]
struct Asked {
    refresh: bool,
    nudge: Option<(String, bool)>,
    toggle: Option<String>,
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if state.mod_manager.root.as_ref() != state.prefs.active_game_root() {
        state.refresh_mods();
    }
    let Some(root) = state.mod_manager.root.clone() else {
        theme::empty_message(ui, tr!("Choose a game install to see its mods"));
        return;
    };

    let mut asked = Asked::default();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(tr!("{icon} Refresh", icon = ph::ARROW_CLOCKWISE))
                    .on_hover_text(tr!("Read the install's mods again"))
                    .clicked()
                {
                    asked.refresh = true;
                }
                ui.label(
                    egui::RichText::new(root.display().to_string())
                        .monospace()
                        .color(theme::TEXT_SECONDARY),
                );
            });
            let manager = &state.mod_manager;
            let in_order: Vec<&InstalledMod> = (manager.mods.iter())
                .filter(|m| m.place == ModPlace::Overlay)
                .collect();

            heading(
                ui,
                tr!("Load order"),
                tr!("The last mod to write a path wins it."),
            );
            if in_order.is_empty() {
                weak(ui, tr!("No mod is in `mods`. A folder put there is a mod."));
            }
            for (at, found) in in_order.iter().enumerate() {
                mod_row(ui, found, Some((at, in_order.len())), &mut asked);
            }

            heading(
                ui,
                tr!("Conflicts"),
                tr!("Paths two enabled mods both write, and the mod each resolves to."),
            );
            let resolution = &manager.resolution;
            for conflict in &resolution.conflicts {
                let title = tr!(
                    "{kind}: {count} path(s) written by {mods}; {winner} wins",
                    kind = kind_label(conflict.kind),
                    count = conflict.targets.len(),
                    mods = conflict.mods.join(", "),
                    winner = conflict.winner
                );
                egui::CollapsingHeader::new(title)
                    .id_salt(("mod_conflict", conflict.kind as u8, &conflict.mods))
                    .show(ui, |ui| list(ui, &conflict.targets));
            }
            if !resolution.is_complete() {
                ui.label(
                    egui::RichText::new(tr!(
                        "These archives could not be read, so what they hold is not compared:"
                    ))
                    .color(theme::WARNING),
                );
                for unread in &resolution.unread {
                    weak(
                        ui,
                        &format!("{}: {}: {}", unread.mod_name, unread.archive, unread.why),
                    );
                }
            } else if resolution.conflicts.is_empty() && in_order.len() > 1 {
                weak(ui, tr!("No two enabled mods write the same path."));
            }

            let parked: Vec<&InstalledMod> = (manager.mods.iter())
                .filter(|m| m.place == ModPlace::Parked)
                .collect();
            if !parked.is_empty() {
                heading(
                    ui,
                    tr!("Disabled"),
                    tr!("Kept in `mods_`, and out of the load order."),
                );
                for found in parked {
                    mod_row(ui, found, None, &mut asked);
                }
            }

            let in_game_folder: Vec<&InstalledMod> = (manager.mods.iter())
                .filter(|m| m.place == ModPlace::GameFolder)
                .collect();
            if !in_game_folder.is_empty() {
                heading(
                    ui,
                    tr!("In the game folder"),
                    tr!("ASI plugins, loaders and scripts."),
                );
                for found in in_game_folder {
                    mod_row(ui, found, None, &mut asked);
                }
            }

            ui.add_space(theme::SPACING_DEFAULT);
            weak(
                ui,
                tr!(
                    "The winner is how the VFS resolves the enabled mods in this order. \
                 Writing that into the overlay is not built."
                ),
            );
        });

    if asked.refresh {
        state.refresh_mods();
    }
    if let Some((name, up)) = asked.nudge {
        state.move_mod(&name, up);
    }
    if let Some(name) = asked.toggle {
        state.ask_toggle_mod(&name);
    }
    draw_question(ui.ctx(), state);
}

fn heading(ui: &mut egui::Ui, title: &str, hover: &str) {
    ui.add_space(theme::SPACING_RELAXED);
    ui.label(egui::RichText::new(title).strong())
        .on_hover_text(hover);
}

fn weak(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .small()
                .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
}

fn kind_label(kind: ClaimKind) -> &'static str {
    match kind {
        ClaimKind::File => tr!("File"),
        ClaimKind::ArchiveEntry => tr!("Archive entry"),
        ClaimKind::DlcContent => tr!("DLC content"),
    }
}

/// One mod: its place in the load order when it has one, its kind, status and files.
fn mod_row(
    ui: &mut egui::Ui,
    found: &InstalledMod,
    order: Option<(usize, usize)>,
    asked: &mut Asked,
) {
    ui.horizontal(|ui| {
        if let Some((at, of)) = order {
            let up = ui.add_enabled(at > 0, egui::Button::new(ph::ARROW_UP));
            if up.on_hover_text(tr!("Earlier: it wins less")).clicked() {
                asked.nudge = Some((found.name.clone(), true));
            }
            let down = ui.add_enabled(at + 1 < of, egui::Button::new(ph::ARROW_DOWN));
            if down.on_hover_text(tr!("Later: it wins more")).clicked() {
                asked.nudge = Some((found.name.clone(), false));
            }
            ui.label(format!("{}.", at + 1));
        }
        ui.label(egui::RichText::new(&found.name).strong());
        ui.label(egui::RichText::new(found.kind.label()).color(theme::ACCENT_TEXT));
        let status = found.place.status();
        let color = match status {
            ModStatus::Enabled => theme::SUCCESS,
            ModStatus::Disabled => theme::TEXT_DISABLED,
        };
        ui.label(egui::RichText::new(status.label()).color(color));
        if found.place.is_ordered() {
            let (label, hover) = match found.place {
                ModPlace::Parked => (tr!("Enable"), tr!("Move it from `mods_` into `mods`")),
                _ => (tr!("Disable"), tr!("Move it from `mods` into `mods_`")),
            };
            if ui.small_button(label).on_hover_text(hover).clicked() {
                asked.toggle = Some(found.name.clone());
            }
        }
    });
    let id = ("mod_files", found.place as u8, &found.name);
    egui::CollapsingHeader::new(format!("{} file(s)", found.files.len()))
        .id_salt(id)
        .show(ui, |ui| {
            for pack in &found.packs {
                let listed = match pack.listed {
                    Some(true) => tr!("named in dlclist.xml").to_string(),
                    Some(false) => tr!("not in dlclist.xml: the game will not load it").to_string(),
                    None => tr!("dlclist.xml was not read").to_string(),
                };
                weak(
                    ui,
                    &tr!(
                        "DLC pack {name}: {listed}",
                        listed = listed,
                        name = pack.name
                    ),
                );
                if let Some(why) = &pack.unread {
                    ui.label(
                        egui::RichText::new(tr!(
                            "{archive} could not be read: {why}",
                            why = why,
                            archive = pack.archive
                        ))
                        .small()
                        .color(theme::WARNING),
                    );
                }
            }
            list(ui, &found.files);
        });
}

/// The first `LISTED` of `lines`, and how many more there are.
fn list(ui: &mut egui::Ui, lines: &[String]) {
    for line in lines.iter().take(LISTED) {
        ui.label(egui::RichText::new(line).monospace().small());
    }
    if lines.len() > LISTED {
        weak(ui, &format!("and {} more", lines.len() - LISTED));
    }
}

fn draw_question(ctx: &egui::Context, state: &mut AppState) {
    let Some(pending) = state.mod_manager.pending_move.clone() else {
        return;
    };
    let (title, words) = if pending.enable {
        (
            tr!("Enable this mod?"),
            tr!(
                "It joins the load order in the place it had, or last when it is new, and \
             takes every path it shares with a mod before it."
            ),
        )
    } else {
        (
            tr!("Disable this mod?"),
            tr!(
                "It leaves the load order, so what it wins goes back to the mods it was \
             winning over. Enable moves it back to its place."
            ),
        )
    };
    let mut answer: Option<bool> = None;
    question(ctx, title, |ui| {
        ui.set_min_width(420.0);
        ui.label(
            egui::RichText::new(tr!(
                "{name}: {files} file(s) move",
                name = pending.name,
                files = pending.files
            ))
            .strong(),
        );
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(egui::RichText::new(pending.from.display().to_string()).monospace());
        ui.label(tr!("to"));
        ui.label(egui::RichText::new(pending.to.display().to_string()).monospace());
        ui.add_space(theme::SPACING_SNUG);
        ui.label(egui::RichText::new(words).weak());
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if ui.button(tr!("Move")).clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if let Some(accept) = answer {
        state.answer_mod_move(accept);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{EditMode, Game};

    /// The text each frame drew, one galley to a line.
    fn drawn(state: &mut AppState) -> String {
        let ctx = crate::app::gpu::configured_egui_context(Some(2048), &state.prefs.appearance);
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        });
        crate::ui::root::with_root_ui(&ctx, |ui| draw(ui, state));
        let output = ctx.end_pass();
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn write(root: &std::path::Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        std::fs::write(path, bytes).expect("write");
    }

    #[test]
    fn the_panel_shows_each_mods_kind_and_status_and_the_winner_of_a_conflict() {
        let install = tempfile::tempdir().expect("tempdir");
        let root = install.path();
        let shared = "x64a.rpf/levels/gta5/props.ydr";
        write(root, &format!("mods/BigProps/{shared}"), b"big");
        write(root, &format!("mods/SmallProps/{shared}"), b"small");
        write(root, "mods_/Parked/a.ytd", b"p");
        write(root, "NativeTrainer.asi", b"MZ");
        let mut state = AppState::default();
        state.documents.clear();
        state
            .prefs
            .game_paths
            .set(Game::GtaVLegacy, Some(root.to_path_buf()));
        state.prefs.active_game = Some(Game::GtaVLegacy);
        state.prefs.edit_mode = EditMode::ReadOnly;

        let text = drawn(&mut state);
        for expected in [
            "BigProps",
            "SmallProps",
            "Loose files",
            "Enabled",
            "Parked",
            "Disabled",
            "NativeTrainer.asi",
            "ASI plugin",
            "Archive entry: 1 path(s) written by BigProps, SmallProps; SmallProps wins",
        ] {
            assert!(
                text.contains(expected),
                "`{expected}` is not drawn:\n{text}"
            );
        }

        state.move_mod("SmallProps", true);
        let text = drawn(&mut state);
        assert!(
            text.contains("written by SmallProps, BigProps; BigProps wins"),
            "the reorder is drawn:\n{text}"
        );
    }

    #[test]
    fn the_panel_without_an_install_says_so() {
        let mut state = AppState::default();
        state.documents.clear();
        state.prefs.active_game = None;
        assert!(drawn(&mut state).contains("Choose a game install"));
    }
}
