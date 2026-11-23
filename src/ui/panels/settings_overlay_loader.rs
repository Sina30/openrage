//! Settings ▸ Editing ▸ Mods overlay: whether the game itself reads the overlay, and what to add when it does not.

use std::path::Path;

use or_title::game::{GameProfile, OverlayLoaderReport};

use crate::state::overlay_loader::why_unread;
use crate::state::AppState;
use crate::ui::theme;

/// What makes the active install's game read the overlay, as found in its folder, with what to add when it would not.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(root) = state.prefs.active_game_root().cloned() else {
        return;
    };
    let Some(reading) = state.overlay_loader(&root) else {
        return;
    };
    let (profile, report) = (reading.profile, reading.report.clone());
    let (label, exe) = (profile.label(), profile.executable_name());

    match &report {
        Err(why) => {
            ui.label(
                egui::RichText::new(tr!(
                    "Could not look in {root} for what makes {label} read the overlay: {why}",
                    label = label,
                    why = why,
                    root = root.display()
                ))
                .weak(),
            );
        }
        Ok(report) if report.reads() => {
            let mut there: Vec<&str> = report.redirects.iter().map(|r| r.file).collect();
            there.extend(report.loader);
            ui.colored_label(
                theme::SUCCESS,
                tr!(
                    "{label} reads this overlay: {there} stands beside {exe}.",
                    label = label,
                    exe = exe,
                    there = there.join(" and ")
                ),
            );
        }
        Ok(report) => draw_what_is_missing(ui, profile, &root, report),
    }

    ui.add_space(theme::SPACING_HAIR);
    if ui
        .button(tr!("Check again"))
        .on_hover_text(tr!(
            "Look in the install folder again, after adding or removing a file there"
        ))
        .clicked()
    {
        state.recheck_overlay_loader(&root);
    }
    ui.add_space(theme::SPACING_SNUG);
}

fn draw_what_is_missing(
    ui: &mut egui::Ui,
    profile: &dyn GameProfile,
    root: &Path,
    report: &OverlayLoaderReport,
) {
    let (label, exe) = (profile.label(), profile.executable_name());
    ui.colored_label(
        theme::WARNING,
        tr!("{label} will not read this overlay.", label = label),
    );
    if let Some(why) = why_unread(profile, report) {
        ui.label(why);
    }
    if !report.for_other_titles.is_empty() {
        ui.label(
            egui::RichText::new(tr!(
                "{for_other_titles} serves another edition and does nothing for {label}.",
                label = label,
                for_other_titles = report.for_other_titles.join(" and ")
            ))
            .weak(),
        );
    }

    if report.lacks_redirect() {
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(tr!(
            "Put one of these in {root}, beside {exe}:",
            exe = exe,
            root = root.display()
        ));
        for plugin in report.rules.redirects {
            ui.horizontal(|ui| {
                ui.monospace(plugin.file);
                ui.hyperlink_to(plugin.project.name, plugin.project.page);
                if plugin.loads_itself {
                    ui.label(egui::RichText::new(tr!("starts itself")).weak());
                }
            });
        }
    }
    if report.needs_loader_file() {
        ui.add_space(theme::SPACING_TIGHT);
        let lead = if report.lacks_loader() {
            tr!("Put an ASI loader")
        } else {
            tr!("Unless it starts itself, a plugin also needs an ASI loader")
        };
        ui.label(tr!(
            "{lead} named {loader_files} beside {exe}, from one of:",
            lead = lead,
            exe = exe,
            loader_files = report.rules.loader_files.join(" or ")
        ));
        ui.horizontal(|ui| {
            for project in report.rules.loader_projects {
                ui.hyperlink_to(project.name, project.page);
            }
        });
    }
    ui.add_space(theme::SPACING_TIGHT);
    ui.label(
        egui::RichText::new(tr!(
            "These are separate projects. OpenRage names and links them, and downloads, \
             bundles and installs none of them."
        ))
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use or_title::Game;

    use super::*;

    fn install_of(game: Game, files: &[&str]) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("scratch");
        for name in files {
            std::fs::write(dir.path().join(name), b"").expect("a stand-in file");
        }
        let mut state = AppState::default();
        state
            .prefs
            .game_paths
            .set(game, Some(dir.path().to_path_buf()));
        state.prefs.active_game = Some(game);
        (dir, state)
    }

    /// Every label the section draws.
    fn drawn(state: &mut AppState) -> Vec<String> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| draw(ui, state));
        let nodes = output
            .platform_output
            .accesskit_update
            .map(|update| update.nodes)
            .unwrap_or_default();
        (nodes.iter())
            .filter_map(|(_, node)| node.label().or_else(|| node.value()).map(str::to_owned))
            .collect()
    }

    fn said(labels: &[String], text: &str) -> bool {
        labels.iter().any(|label| label.contains(text))
    }

    #[test]
    fn an_install_with_no_loader_says_what_to_add_and_where_to_find_it() {
        let (dir, mut state) = install_of(Game::GtaVLegacy, &["GTA5.exe"]);
        let labels = drawn(&mut state);
        assert!(
            said(&labels, "GTA V (Legacy) will not read this overlay."),
            "{labels:?}"
        );
        assert!(said(
            &labels,
            "Nothing beside GTA5.exe makes the game read the mods folder."
        ));
        assert!(
            said(&labels, &dir.path().display().to_string()),
            "where it goes"
        );
        for named in [
            "OpenIV.asi",
            "RageOpenV",
            "dinput8.dll",
            "Ultimate ASI Loader",
            "ScriptHookV",
        ] {
            assert!(said(&labels, named), "{named}: {labels:?}");
        }
        assert!(said(
            &labels,
            "downloads, bundles and installs none of them"
        ));
        assert!(said(&labels, "Check again"));
    }

    #[test]
    fn an_install_the_game_reads_says_so_and_a_new_file_shows_after_checking_again() {
        let (dir, mut state) = install_of(Game::GtaVLegacy, &["GTA5.exe", "OpenIV.asi"]);
        let labels = drawn(&mut state);
        assert!(said(
            &labels,
            "OpenIV.asi stands beside GTA5.exe, but nothing starts it"
        ));
        assert!(said(&labels, "dinput8.dll"));

        std::fs::write(dir.path().join("dinput8.dll"), b"").expect("the loader");
        assert!(
            said(&drawn(&mut state), "will not read this overlay"),
            "the folder is read once and kept"
        );

        let root = dir.path().to_path_buf();
        state.recheck_overlay_loader(&root);
        let labels = drawn(&mut state);
        assert!(
            said(
                &labels,
                "GTA V (Legacy) reads this overlay: OpenIV.asi and dinput8.dll stand"
            ),
            "{labels:?}"
        );
        assert!(!said(&labels, "will not read"));
    }

    #[test]
    fn enhanced_names_its_own_plugin_and_its_own_loader() {
        let (_dir, mut state) =
            install_of(Game::GtaVEnhanced, &["GTA5_Enhanced.exe", "OpenIV.asi"]);
        let labels = drawn(&mut state);
        assert!(
            said(&labels, "GTA V (Enhanced) will not read this overlay."),
            "{labels:?}"
        );
        assert!(said(&labels, "Nothing beside GTA5_Enhanced.exe"));
        assert!(said(&labels, "OpenIV.asi serves another edition"));
        assert!(said(&labels, "OpenRPF.asi") && said(&labels, "xinput1_4.dll or dsound.dll"));
    }

    #[test]
    fn a_folder_that_is_no_install_says_nothing() {
        let (_dir, mut state) = install_of(Game::GtaVLegacy, &[]);
        assert_eq!(drawn(&mut state), Vec::<String>::new());
    }
}
