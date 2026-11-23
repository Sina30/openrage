//! A file of each kind the corpus holds, opened with File ▸ Open file…, saved as a copy and the copy opened again.

use super::{corpus, meta_corpus, run, Flow, Step};

/// What the tab in front is, in the app's own words for its kind.
fn kind(at: &Flow) -> String {
    (at.shell.state.active_document()).map_or(String::new(), |d| format!("{:?}", d.kind))
}

fn opens_saves_and_opens_again(at: &mut Flow, path: Result<std::path::PathBuf, String>) -> Step {
    let path = path?;
    let extension = path.extension().unwrap().to_string_lossy().to_string();
    if let Err(why) = at.open_file(&path) {
        // A version the reader refuses on purpose is said in the error window, and is the corpus's, not a fault of the flow.
        if !why.contains("is refused") {
            return Err(why);
        }
        at.note(format!(
            "the corpus's .{extension} does not open: {}",
            why.lines().next().unwrap_or_default()
        ));
        return Ok(());
    }
    let (opened, ws) = (kind(at), at.shell.state.active_workspace);
    at.say(format!(
        "opened as {opened} in the {} workspace",
        ws.label()
    ));
    let handled = (at.shell.state.active_document()).is_some_and(|d| ws.handles_kind(d.kind));
    if !handled {
        at.note(format!(
            "a .{extension} opens on the {} workspace, which does not draw it",
            ws.label()
        ));
    }
    for nothing in ["Nothing selected", "Nothing to show", "No data file open"] {
        if at.shows(nothing) {
            at.note(format!("a .{extension} just opened is met by `{nothing}`"));
        }
    }
    if at.find("Save").is_ok_and(|save| !save.disabled) {
        return Err(format!(
            "the bar's Save is lit on a .{extension} nothing has changed"
        ));
    }

    let copy = at.scratch.path().join(format!("copy.{extension}"));
    match at.save_as(&copy) {
        Ok(true) => {}
        Ok(false) => {
            at.say(format!(
                "Save as… is greyed for a .{extension}: it cannot be written"
            ));
            return Ok(());
        }
        Err(why) => return Err(why),
    }
    let (before, after) = (
        std::fs::metadata(&path).map_or(0, |m| m.len()),
        std::fs::metadata(&copy).map_or(0, |m| m.len()),
    );
    at.say(format!("{before} bytes read, {after} bytes written"));
    at.close_document()?;
    at.open_file(&copy)?;
    if kind(at) != opened {
        return Err(format!(
            "the copy opens as {}, the file as {opened}",
            kind(at)
        ));
    }
    Ok(())
}

macro_rules! formats {
    ($($test:ident, $name:literal, $path:expr;)*) => {$(
        #[test]
        fn $test() {
            run($name, |at| opens_saves_and_opens_again(at, $path));
        }
    )*};
}

formats! {
    f30_a_drawable_is_opened_saved_and_opened_again, "f30_ydr", corpus("ydr");
    f31_a_fragment_is_opened_saved_and_opened_again, "f31_yft", corpus("yft");
    f32_a_texture_dictionary_is_opened_saved_and_opened_again, "f32_ytd", corpus("ytd");
    f33_a_drawable_dictionary_is_opened_saved_and_opened_again, "f33_ydd", corpus("ydd");
    f34_a_clip_dictionary_is_opened_saved_and_opened_again, "f34_ycd", corpus("ycd");
    f35_particle_effects_are_opened_saved_and_opened_again, "f35_ypt", corpus("ypt");
    f36_collision_bounds_are_opened_saved_and_opened_again, "f36_ybn", corpus("ybn");
    f37_a_navmesh_is_opened_saved_and_opened_again, "f37_ynv", corpus("ynv");
    f38_path_nodes_are_opened_saved_and_opened_again, "f38_ynd", corpus("ynd");
    f39_an_audio_container_is_opened_saved_and_opened_again, "f39_awc", corpus("awc");
    f40_a_shader_effect_is_opened_saved_and_opened_again, "f40_fxc", corpus("fxc");
    f41_a_cutscene_is_opened_saved_and_opened_again, "f41_cut", corpus("cut");
    f42_an_expression_dictionary_is_opened_saved_and_opened_again, "f42_yed", corpus("yed");
    f43_a_cloth_dictionary_is_opened_saved_and_opened_again, "f43_yld", corpus("yld");
    f44_a_pose_database_is_opened_saved_and_opened_again, "f44_ypdb", corpus("ypdb");
    f45_a_waypoint_recording_is_opened_saved_and_opened_again, "f45_ywr", corpus("ywr");
    f46_a_vehicle_recording_is_opened_saved_and_opened_again, "f46_yvr", corpus("yvr");
    f47_a_frame_filter_dictionary_is_opened_saved_and_opened_again, "f47_yfd", corpus("yfd");
    f48_a_blendshape_dictionary_is_opened_saved_and_opened_again, "f48_ybd", corpus("ybd");
    f49_a_metadata_resource_is_opened_saved_and_opened_again, "f49_ymt", meta_corpus("ymt");
    f50_a_manifest_is_opened_saved_and_opened_again, "f50_ymf", meta_corpus("ymf");
}
