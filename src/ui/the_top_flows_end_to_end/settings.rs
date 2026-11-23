//! The Settings window: reached from the rail and the menus, each page used for what it is for.

use super::{run, skip, Flow, Step};
use crate::state::ThemeChoice;

fn settings(at: &mut Flow, page: &str) -> Step {
    at.click("Room: Settings…")?;
    at.expect("Type to filter settings…")?;
    at.click(page)
}

#[test]
fn f90_the_theme_is_switched_to_light_and_back() {
    run("f90_theme", |at| {
        settings(at, "Appearance")?;
        at.click("Light")?;
        if at.shell.state.prefs.appearance.theme != ThemeChoice::Light {
            return Err("Light did not change the theme".into());
        }
        at.click("Dark")?;
        at.close_settings();
        at.expect("Open an install")
    });
}

#[test]
fn f91_the_interface_is_read_in_french_and_put_back() {
    run("f91_language", |at| {
        settings(at, "Appearance")?;
        at.click("Language")?;
        at.look("the languages");
        at.look_at_controls("the languages");
        at.click("Français")?;
        at.look("in French");
        at.expect("Apparence")?;
        at.click("Langue")?;
        at.click("English")?;
        at.expect("Appearance")
    });
}

#[test]
fn f92_the_interface_scale_is_changed() {
    run("f92_scale", |at| {
        settings(at, "Appearance")?;
        at.click("Interface scale")?;
        at.look("the scales");
        at.look_at_controls("the scales");
        at.click("125%")?;
        at.click("Restore defaults")?;
        Ok(())
    });
}

#[test]
fn f93_a_shortcut_is_bound_to_a_key_and_reset() {
    run("f93_rebind_shortcut", |at| {
        settings(at, "Keyboard shortcuts")?;
        at.click_beside("Edit", "File: Close all")?;
        at.expect("Press a combo")?;
        at.key(egui::Key::F9);
        at.look("after pressing F9");
        at.expect("F9")?;
        at.click_beside("Reset", "File: Close all")?;
        at.expect("(unbound)")
    });
}

#[test]
fn f94_a_game_is_made_the_active_one() {
    run("f94_set_active_game", |at| {
        at.wait_for_the_installs();
        settings(at, "Games")?;
        if !at.shows("Set active") {
            return Err(skip("no second install to make active on this machine"));
        }
        at.click_beside("Set active", "GTA V (Legacy)")?;
        at.settle();
        at.look("after Set active");
        at.close_settings();
        at.look("the window behind");
        Ok(())
    });
}

#[test]
fn f95_the_edit_mode_is_chosen_in_settings() {
    run("f95_edit_mode_in_settings", |at| {
        settings(at, "Editing")?;
        at.click("Edit the mods overlay")?;
        at.click("Read only\n")?;
        Ok(())
    });
}

#[test]
fn f96_a_setting_is_found_by_typing_its_name() {
    run("f96_search_settings", |at| {
        at.click("Room: Settings…")?;
        at.click("Type to filter settings…")?;
        at.type_text("theme");
        at.look("settings filtered by theme");
        at.expect("Appearance")
    });
}

#[test]
fn f97_a_folder_that_is_no_gta_vi_install_is_refused_in_plain_words() {
    run("f97_gta_vi_folder", |at| {
        settings(at, "Games")?;
        let empty = at.scratch.path().join("not_a_game");
        std::fs::create_dir_all(&empty).unwrap();
        at.pick(&empty);
        // Every game with no folder has this button: the one meant is GTA VI's.
        at.click_beside("Select folder…", "GTA VI")?;
        at.settle();
        let said = at.toasts.last().cloned().unwrap_or_default();
        if !said.contains("does not look like a GTA VI folder") {
            return Err(format!(
                "an empty folder picked for GTA VI is answered `{said}`"
            ));
        }
        Ok(())
    });
}

#[test]
fn f98_the_file_associations_are_ticked_and_cleared() {
    run("f98_file_associations", |at| {
        settings(at, "File associations")?;
        at.click("None")?;
        at.click("Game formats")?;
        at.look("the associations page");
        Ok(())
    });
}

#[test]
fn f99_the_shortcuts_page_is_reached_from_the_help_menu() {
    run("f99_shortcuts_from_help", |at| {
        at.menu("Help", "Keyboard shortcuts…")?;
        at.expect("Restore all defaults")
    });
}
