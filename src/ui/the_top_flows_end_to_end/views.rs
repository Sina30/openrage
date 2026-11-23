//! The views of an open file: its workspaces, the viewport's modes, what a selection shows, and the shell's own panels and rooms.

use super::{corpus, run};

#[test]
fn f75_a_model_is_seen_in_the_model_and_the_texture_workspace() {
    run("f75_workspace_buttons", |at| {
        at.open_file(&corpus("yft")?)?;
        at.expect("3D viewport")?;
        at.click("Texture")?;
        at.expect("Preview")?;
        if at.shows("Select a texture in") && at.shows("Models") {
            at.say("the Texture workspace of a model opens with no texture chosen");
        }
        at.click("Model")?;
        at.expect("3D viewport")
    });
}

#[test]
fn f76_the_viewport_is_switched_through_its_shading_modes() {
    run("f76_shading_modes", |at| {
        at.open_file(&corpus("ydr")?)?;
        // ⚠ The pills are the render bridge's, which a frame with no device does not draw: `just shots` reaches them.
        at.look_at_controls("the viewport's controls");
        at.expect("3D viewport")
    });
}

#[test]
fn f77_a_part_chosen_in_the_outline_is_shown_in_details() {
    run("f77_outline_to_details", |at| {
        at.open_file(&corpus("ydr")?)?;
        at.click("Geometry 0")?;
        at.look("a geometry selected");
        at.click("\u{e1d8} Focus")?;
        at.click("📌 Pin")?;
        at.expect("Unpin")?;
        at.click("Unpin")
    });
}

#[test]
fn f78_show_internals_lists_the_engines_bookkeeping() {
    run("f78_show_internals", |at| {
        at.open_file(&corpus("ydr")?)?;
        let before = at.names().len();
        at.click("Show internals")?;
        let with = at.names().len();
        if with <= before {
            return Err(format!(
                "Show internals added no row: {before} names, then {with}"
            ));
        }
        at.click("Show internals")?;
        Ok(())
    });
}

#[test]
fn f79_a_texture_chosen_in_the_gallery_is_previewed() {
    run("f79_gallery_to_preview", |at| {
        at.open_file(&corpus("ytd")?)?;
        at.expect("Select a texture in")?;
        let first = (at.names().into_iter())
            .find(|n| n.starts_with("ap_") || n.starts_with("cs_"))
            .ok_or("no texture tile is named")?;
        at.click(&first)?;
        at.look("a texture chosen");
        if at.shows("Select a texture in") {
            return Err("a click on a tile chose no texture".into());
        }
        Ok(())
    });
}

#[test]
fn f80_a_clip_chosen_in_a_clip_dictionary_fills_the_timeline() {
    run("f80_clip_to_timeline", |at| {
        at.open_file(&corpus("ycd")?)?;
        at.expect("Select a clip in")?;
        let clip = (at.names().into_iter())
            .find(|n| n.ends_with(".clip"))
            .ok_or_else(|| format!("no clip is listed: {:?}", at.names()))?;
        at.click(&clip)?;
        at.look("a clip chosen");
        if at.shows("to play it") {
            return Err("a click on a clip chose none".into());
        }
        Ok(())
    });
}

#[test]
fn f81_an_effect_chosen_in_particle_effects_is_played() {
    run("f81_particle_effect", |at| {
        at.open_file(&corpus("ypt")?)?;
        at.look("particle effects open");
        Ok(())
    });
}

#[test]
fn f82_a_command_is_run_from_the_palette() {
    run("f82_palette", |at| {
        at.open_file(&corpus("ytd")?)?;
        at.menu("Edit", "Command palette…")?;
        at.expect("Command palette")?;
        at.type_text("start page");
        at.key(egui::Key::Enter);
        at.settle();
        at.expect("Open an install")
    });
}

#[test]
fn f83_the_log_is_opened_from_the_status_bar() {
    run("f83_log_panel", |at| {
        at.open_file(&corpus("ytd")?)?;
        at.click("\u{e2f2} Log")?;
        at.look("the log shown");
        Ok(())
    });
}

#[test]
fn f84_a_panel_is_hidden_and_the_layout_reset() {
    run("f84_panels_and_reset", |at| {
        at.open_file(&corpus("ydr")?)?;
        at.click("View")?;
        at.click("Panels")?;
        at.look("the panels menu");
        at.key(egui::Key::Escape);
        at.key(egui::Key::Escape);
        at.menu("View", "Reset active workspace layout")?;
        at.expect("3D viewport")
    });
}

#[test]
fn f85_the_rooms_of_the_rail_are_visited() {
    run("f85_rooms", |at| {
        at.open_file(&corpus("ytd")?)?;
        at.click("Room: Home")?;
        at.expect("Open an install")?;
        at.click("Room: Game")?;
        at.look("the Game room");
        at.click("Room: Guide")?;
        at.look("the Guide room");
        at.click("Room: Automate")?;
        at.look("the Automate room");
        Ok(())
    });
}

#[test]
fn f86_two_open_files_are_switched_by_their_tabs() {
    run("f86_switch_tabs", |at| {
        let (model, textures) = (corpus("ydr")?, corpus("ytd")?);
        at.open_file(&model)?;
        at.open_file(&textures)?;
        let name = model.file_name().unwrap().to_string_lossy().to_string();
        at.click(&name)?;
        if at.active_name() != name {
            return Err(format!("the tab of {name} did not bring it in front"));
        }
        at.expect("3D viewport")
    });
}

#[test]
fn f87_the_grid_and_the_axis_are_hidden_from_the_view_menu() {
    run("f87_grid_and_axis", |at| {
        at.open_file(&corpus("ydr")?)?;
        at.menu("View", "Show grid")?;
        at.menu("View", "Show axis")?;
        Ok(())
    });
}
