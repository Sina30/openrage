//! An install opened from the start page and browsed: its archives, its folders, its files, and what the Files panel offers over them.

use super::{run, skip, Flow, Step};

const LEGACY: &str = "GTA V (Legacy)";
const ENHANCED: &str = "GTA V (Enhanced)";
const RDR2: &str = "Red Dead Redemption 2";

/// The install of `game` open and its keys read.
fn install(at: &mut Flow, game: &str) -> Step {
    at.open_install(game)?;
    at.wait_for_the_keys()
}

/// The file at `path` of the install opened from the centre's list, until its tab is in front.
fn open_from_install(at: &mut Flow, path: &str) -> Step {
    at.go_to(path)?;
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    at.double_click_row(&name)?;
    let wanted = name.clone();
    at.wait(&format!("{name} open"), move |flow| {
        flow.shell.state.show_error_dialog || flow.active_name() == wanted
    })?;
    if at.shell.state.show_error_dialog {
        let said = at.shell.state.error_dialog_message.clone();
        return Err(format!("{name} did not open: {said}"));
    }
    Ok(())
}

#[test]
fn f01_gta_v_legacy_is_opened_from_the_start_page() {
    run("f01_open_legacy", |at| {
        at.open_install(LEGACY)?;
        at.expect("x64a.rpf")?;
        at.expect("common.rpf")?;
        at.expect(LEGACY)
    });
}

#[test]
fn f02_gta_v_enhanced_is_opened_from_the_start_page() {
    run("f02_open_enhanced", |at| {
        at.open_install(ENHANCED)?;
        at.expect("common.rpf")?;
        at.expect(ENHANCED)
    });
}

#[test]
fn f03_red_dead_redemption_2_is_opened_from_the_start_page() {
    run("f03_open_rdr2", |at| {
        at.open_install(RDR2)?;
        at.expect("anim_0.rpf")?;
        at.expect(RDR2)
    });
}

#[test]
fn f04_the_game_is_switched_from_the_status_bar() {
    run("f04_switch_game", |at| {
        at.open_install(LEGACY)?;
        at.click(LEGACY)?;
        at.click("GTA V (Enhanced)   ")?;
        at.settle();
        at.look("after choosing Enhanced in the chip");
        at.wait("the Enhanced install in front", |flow| {
            (flow.shell.state.active_document())
                .and_then(|d| d.path.clone())
                .is_some_and(|p| p.to_string_lossy().contains("Enhanced"))
        })?;
        at.expect(ENHANCED)
    });
}

#[test]
fn f05_an_archive_opened_before_the_keys_are_read_says_so() {
    run("f05_archive_before_keys", |at| {
        at.open_install(LEGACY)?;
        if at.shell.state.keys.has_keys() {
            return Err(skip("the keys were read before the first click could land"));
        }
        at.click("Type a path")?;
        at.type_text("common.rpf/data/handling.meta");
        at.key(egui::Key::Enter);
        at.settle();
        at.wait_for_the_keys()?;
        at.wait_for("CHandlingDataMgr")?;
        if let Some(toast) = at.toasts.first() {
            return Err(format!("the walk did not wait for the keys: {toast}"));
        }
        Ok(())
    });
}

#[test]
fn f06_an_archive_is_expanded_in_the_files_tree() {
    run("f06_expand_archive", |at| {
        install(at, LEGACY)?;
        at.click_beside("Expand", "common.rpf")?;
        at.wait_for("data")
    });
}

#[test]
fn f07_folders_are_entered_and_left_in_the_centre() {
    run("f07_browse_folders", |at| {
        install(at, LEGACY)?;
        at.go_to("common.rpf/data")?;
        at.wait_for("ai")?;
        at.look("inside common.rpf/data");
        at.click("Up one folder")?;
        at.wait_for("data")?;
        at.click("Back")?;
        at.wait_for("ai")?;
        at.click("Forward")?;
        at.settle();
        Ok(())
    });
}

#[test]
fn f08_a_metadata_file_is_opened_from_a_legacy_archive() {
    run("f08_legacy_handling_meta", |at| {
        install(at, LEGACY)?;
        open_from_install(at, "common.rpf/data/handling.meta")?;
        at.expect("CHandlingDataMgr")
    });
}

#[test]
fn f09_a_metadata_file_is_opened_from_an_enhanced_archive() {
    run("f09_enhanced_handling_meta", |at| {
        install(at, ENHANCED)?;
        open_from_install(at, "common.rpf/data/handling.meta")?;
        at.expect("CHandlingDataMgr")
    });
}

#[test]
fn f10_a_vehicle_model_is_opened_from_a_legacy_archive() {
    run("f10_legacy_adder_yft", |at| {
        install(at, LEGACY)?;
        open_from_install(at, "x64e.rpf/levels/gta5/vehicles.rpf/adder.yft")?;
        at.expect("Fragment")?;
        at.expect("Legacy, version")
    });
}

#[test]
fn f11_a_texture_dictionary_is_opened_from_a_legacy_archive() {
    run("f11_legacy_adder_ytd", |at| {
        install(at, LEGACY)?;
        open_from_install(at, "x64e.rpf/levels/gta5/vehicles.rpf/adder.ytd")?;
        at.expect("Texture Dictionary")
    });
}

#[test]
fn f12_a_vehicle_model_is_opened_from_an_enhanced_archive() {
    run("f12_enhanced_adder_yft", |at| {
        install(at, ENHANCED)?;
        open_from_install(at, "x64e.rpf/levels/gta5/vehicles.rpf/adder.yft")?;
        at.expect("Fragment")?;
        at.expect("Enhanced, version")
    });
}

#[test]
fn f13_the_files_tree_is_filtered_by_a_few_letters() {
    run("f13_filter_tree", |at| {
        install(at, LEGACY)?;
        at.click("Filter (substring)…")?;
        at.type_text("x64a");
        at.expect("x64a.rpf")?;
        at.look("filtered by x64a");
        at.click("Clear filter")?;
        at.expect("x64b.rpf")
    });
}

#[test]
fn f14_a_file_is_found_inside_the_archives() {
    run("f14_search_inside", |at| {
        install(at, LEGACY)?;
        at.click("Search inside archives")?;
        at.click("Glob (**/*.ydr)…")?;
        at.type_text("**/handling.meta");
        at.key(egui::Key::Enter);
        (at.wait("a found handling.meta", |flow| flow.shows("handling.meta"))).ok();
        at.look("search inside archives");
        at.expect("handling.meta")
    });
}

#[test]
fn f15_go_to_file_finds_a_file_by_name() {
    run("f15_go_to_file", |at| {
        install(at, LEGACY)?;
        at.click("Go to file…")?;
        at.wait_for("Type a file's name")?;
        // Typed straight away: the field has the keyboard as soon as it is asked for.
        at.type_text("adder");
        at.wait_for("adder.yft")?;
        at.look("go to file results");
        at.key(egui::Key::Enter);
        at.wait("adder.yft open", |flow| flow.active_name() == "adder.yft")
    });
}

#[test]
fn f16_the_list_is_sorted_and_drawn_as_icons() {
    run("f16_sort_and_icons", |at| {
        install(at, LEGACY)?;
        at.click("\u{e444} Name")?;
        at.click("Stored size")?;
        at.key(egui::Key::Escape);
        at.click("Icons")?;
        at.settle();
        at.expect("common.rpf")
    });
}

#[test]
fn f17_a_selected_file_is_previewed_beside_the_list() {
    run("f17_preview_pane", |at| {
        install(at, LEGACY)?;
        at.go_to("common.rpf/data/handling.meta")?;
        at.expect("CHandlingDataMgr")?;
        at.expect("Inside an archive")
    });
}

#[test]
fn f18_a_file_is_extracted_from_an_archive() {
    run("f18_extract", |at| {
        install(at, LEGACY)?;
        at.go_to("common.rpf/data/handling.meta")?;
        at.right_click_row("handling.meta")?;
        at.look("row menu");
        let out = at.scratch.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        at.pick(&out);
        at.click("Extract")?;
        at.settle();
        at.look("after Extract");
        at.wait("handling.meta extracted", move |_| {
            out.join("handling.meta").exists()
        })
    });
}

#[test]
fn f19_a_file_inside_an_install_is_saved_only_as_a_copy() {
    run("f19_read_only_save", |at| {
        install(at, LEGACY)?;
        open_from_install(at, "common.rpf/data/handling.meta")?;
        at.click("File")?;
        let save = at.find(super::SAVE)?;
        at.key(egui::Key::Escape);
        if !save.disabled || !at.find("Save")?.disabled {
            return Err("Save is offered on an unchanged file inside a read-only install".into());
        }
        let copy = at.scratch.path().join("handling.meta");
        if !at.save_as(&copy)? {
            return Err("Save as… is greyed on a file of the install".into());
        }
        Ok(())
    });
}

#[test]
fn f20_the_edit_mode_is_changed_from_the_menu_bar() {
    run("f20_edit_mode", |at| {
        at.open_install(LEGACY)?;
        at.click("Read only")?;
        at.click("Edit the mods overlay")?;
        at.look("after choosing the overlay");
        at.click("mods overlay")?;
        at.click("Read only\n")?;
        at.expect("Read only")
    });
}

#[test]
fn f21_an_rdr2_archive_is_opened_and_a_file_of_it_read() {
    run("f21_rdr2_archive", |at| {
        at.open_install(RDR2)?;
        at.go_to("appdata0_update.rpf")?;
        at.settle();
        at.look("inside appdata0_update.rpf");
        Ok(())
    });
}

#[test]
fn f22_closing_the_install_brings_the_start_page_back() {
    run("f22_close_install", |at| {
        at.open_install(LEGACY)?;
        at.click("Close")?;
        at.wait("no document", |flow| flow.shell.state.documents.is_empty())?;
        at.expect("Open an install")
    });
}

#[test]
fn f23_the_installed_mods_are_listed() {
    run("f23_installed_mods", |at| {
        at.open_install(LEGACY)?;
        at.click("Installed mods")?;
        at.settle();
        at.look("installed mods");
        Ok(())
    });
}

#[test]
fn f24_a_folder_is_opened_and_the_offer_to_make_it_a_project_declined() {
    run("f24_open_folder", |at| {
        let dir = at.scratch.path().join("mod");
        std::fs::create_dir_all(dir.join("stream")).unwrap();
        std::fs::write(dir.join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.join("stream").join("vehicles.meta"), "<Item/>").unwrap();
        at.pick(&dir);
        at.menu("File", "Open folder…")?;
        at.wait_for("notes.txt")?;
        at.wait_for("Make this a project")?;
        at.click("Not now")?;
        at.settle();
        if at.shows("Make this a project") {
            return Err("Not now left the offer on screen".into());
        }
        at.double_click("stream")?;
        at.wait_for("vehicles.meta")
    });
}

#[test]
fn f25_a_file_of_a_folder_shows_its_menu_and_so_does_the_empty_space() {
    run("f25_folder_file_menu", |at| {
        let dir = at.scratch.path().join("mod");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "hello").unwrap();
        at.pick(&dir);
        at.menu("File", "Open folder…")?;
        at.wait_for("notes.txt")?;
        at.right_click("notes.txt")?;
        at.look("file row menu");
        at.key(egui::Key::Escape);
        at.right_click("Empty space in Files")?;
        at.look("empty space menu");
        Ok(())
    });
}
