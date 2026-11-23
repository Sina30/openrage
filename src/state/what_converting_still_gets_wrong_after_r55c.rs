//! What converting still got wrong after R55c: a glTF export's write and
//! Export as…'s are listed as exports, the palette and its key convert the open
//! document with no row selected, and the conversion's question wraps a long
//! name inside its panel and selects line by line.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use or_interchange::Scene;
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_io::gta5::{gen8::GrcTexture, File as _, PgDictionary};

use super::convert_words;
use super::converting_from_the_app::{
    browsing, convert_from_the_menu, question_frame, question_frame_on, question_labels,
    question_rows, written_by_the_job,
};
use super::every_write_asks_the_jobs_reading_it::exported;
use super::jobs::{JobKind, Outcome};
use super::one_place_for_background_jobs::{click as click_in_the_list, frame as the_status_bar};
use super::the_files_view_in_the_centre::{accessible, node};
use super::{AppState, DocPayload, Document, Prefs};
use crate::ui::commands::{self, Action};
use crate::ui::panels::file_prompts::QUESTION_WIDTH;

const DEADLINE: Duration = Duration::from_secs(60);

/// The labels of the status bar's list of background jobs, opened.
fn the_jobs_list(ctx: &egui::Context, state: &mut AppState) -> Vec<String> {
    the_status_bar(ctx, state, Vec::new());
    let count = match state.running_jobs().len() {
        1 => "1 job running".to_string(),
        n => format!("{n} jobs running"),
    };
    click_in_the_list(ctx, state, &count);
    (the_status_bar(ctx, state, Vec::new()).into_iter())
        .map(|(label, _)| label)
        .collect()
}

/// The write `doing` names is in the list as an export, never an extraction; its Cancel reaches it, and it ends under Export.
fn is_listed_as_an_export_and_cancelled(state: &mut AppState, doing: &str) {
    let job = (state.running_jobs().into_iter())
        .find(|job| job.doing == doing)
        .unwrap_or_else(|| panic!("`{doing}` is not listed: {:?}", state.running_jobs()));
    assert_eq!(
        job.kind,
        JobKind::Export,
        "`{doing}` is listed as another job"
    );

    let ctx = accessible();
    let listed = the_jobs_list(&ctx, state);
    for label in [doing, "Cancel Export"] {
        assert!(
            listed.iter().any(|shown| shown == label),
            "the list shows no `{label}`: {listed:?}"
        );
    }
    assert!(
        !listed.iter().any(|shown| shown.contains("Extract")),
        "the list says Extract of a write that is not one: {listed:?}"
    );

    click_in_the_list(&ctx, state, "Cancel Export");
    assert!(
        (state.running_jobs().iter())
            .find(|job| job.doing == doing)
            .is_some_and(|job| job.stopping),
        "Cancel Export did not reach the write, or the job left the list"
    );
    written_by_the_job(state);
    let kinds: Vec<JobKind> = state.jobs.ended().iter().map(|job| job.kind).collect();
    assert!(
        kinds.contains(&JobKind::Export) && !kinds.contains(&JobKind::Extraction),
        "the write ends under another name than Export: {:?}",
        state.jobs.ended()
    );
}

#[test]
fn a_gltf_exports_write_is_listed_as_an_export_and_cancel_export_reaches_it() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());
    state.gltf.asked = Some(exported(Scene::default()));
    state.write_gltf(scratch.path().join("prop.glb"));
    is_listed_as_an_export_and_cancelled(&mut state, "Exporting prop.glb");
}

#[test]
fn an_export_as_write_is_listed_as_an_export_and_cancel_export_reaches_it() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (input, out) = (scratch.path().join("in"), scratch.path().join("out"));
    for folder in [&input, &out] {
        std::fs::create_dir_all(folder).expect("a folder");
    }
    std::fs::write(input.join("notes.txt"), b"a note").expect("a file to export");

    let (mut state, _) = browsing(&input, &ParamRemap::default());
    let doc = state.active_document.expect("the folder is open");
    let notes = node(&state, doc, "notes.txt");
    state.export_rpf_node_as(doc, notes, out.join("notes.txt"), Some(false));
    is_listed_as_an_export_and_cancelled(&mut state, "Exporting notes.txt");
}

#[test]
fn cancelling_an_exports_write_stops_it_and_never_stops_an_extraction() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());
    state.extract_jobs.start(
        JobKind::Export,
        "prop.glb".into(),
        scratch.path().to_path_buf(),
        Vec::new(),
        |cancel, progress| {
            or_job::run(0..1_000_000u32, &cancel, &progress, |_| {
                std::thread::sleep(Duration::from_millis(1));
                or_job::Step::Done(0)
            })
        },
    );

    state.cancel_job(JobKind::Extraction);
    assert!(
        state.running_jobs().iter().all(|job| !job.stopping),
        "Cancel Extraction stops an export's write"
    );
    state.cancel_job(JobKind::Export);
    assert!(
        state.running_jobs().iter().all(|job| job.stopping),
        "Cancel Export leaves its write running"
    );
    written_by_the_job(&mut state);
    let ended = (state.jobs.ended().iter()).find(|job| job.kind == JobKind::Export);
    assert!(
        matches!(ended.map(|job| &job.outcome), Some(Outcome::Stopped(_))),
        "the write did not end as stopped: {:?}",
        state.jobs.ended()
    );
}

/// A Legacy `.ytd` of one texture called `name`, whose conversion drops a few words.
pub(super) fn a_ytd_holding(name: &str) -> PgDictionary<GrcTexture> {
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    dictionary.insert(GrcTexture {
        name: name.to_string(),
        width: 4,
        height: 4,
        depth: 1,
        format: u32::from_le_bytes(*b"DXT1"),
        mip_count: 1,
        mip_stride: 8,
        handle_index: 3,
        extra_flags: 1,
        stereo_rt_mode: 2,
        backing_store: vec![0; 128],
        ..Default::default()
    });
    dictionary
}

/// A Legacy `.ytd`, `paint`, open as the only document, with nothing selected.
fn holding_paint() -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.documents.push(Document::new(
        1,
        Some(PathBuf::from("paint.ytd")),
        DocPayload::Resource(or_io::Asset::TextureDict(a_ytd_holding("paint"))),
    ));
    state.active_document = Some(1);
    state
}

/// The names the conversion under way converted, once it asks, and whether it was of the open document.
fn asked_about(state: &mut AppState) -> (Vec<String>, bool) {
    let until = Instant::now() + DEADLINE;
    while state.convert.done.is_none() && Instant::now() < until {
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
    let done = state.convert.done.as_ref().expect("the conversion asked");
    (
        done.files.iter().map(|file| file.name.clone()).collect(),
        done.of_document,
    )
}

#[test]
fn the_palette_and_its_key_convert_the_open_document_with_no_row_selected() {
    let mut state = holding_paint();
    let ctx = egui::Context::default();
    assert!(
        state.selection.current().is_none(),
        "a row is selected, so the test shows nothing"
    );

    let palette = commands::all(&state);
    let convert = (palette.iter())
        .find(|command| command.id == "convert")
        .expect("Convert is in the palette");
    assert_eq!(
        (convert.enabled, convert.label.as_str()),
        (Ok(()), "Convert to Enhanced…"),
        "the palette does not offer to convert the open document"
    );
    commands::run(&mut state, &ctx, convert);
    assert_eq!(
        asked_about(&mut state),
        (vec!["paint.ytd".to_string()], true),
        "the palette converts something else than the open document as it stands"
    );

    state.convert.done = None;
    commands::run_action(&mut state, &ctx, Action::Convert);
    assert_eq!(
        asked_about(&mut state),
        (vec!["paint.ytd".to_string()], true),
        "the key does not convert the open document"
    );
}

#[test]
fn a_folder_open_with_nothing_selected_still_says_to_select_the_files() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(scratch.path().join("notes.txt"), b"a note").expect("a file to list");
    let (state, _) = browsing(scratch.path(), &ParamRemap::default());
    let convert = commands::of(&state, Action::Convert).expect("Convert is a command");
    assert_eq!(convert.enabled, Err("Select the files to convert first"));
}

/// A name no dialog is wide enough for, without a space to break at.
fn a_long_name() -> String {
    "road_asphalt_wet_diffuse_".repeat(7)
}

/// What a `.ytd` of one texture called `name` drops on its way to Enhanced, each as the question words it.
fn dropped_in_words(name: &str) -> Vec<String> {
    let bytes = a_ytd_holding(name).to_bytes().expect("a Legacy .ytd saves");
    let made = or_cli::retarget_file("long.ytd", &bytes, &ParamRemap::default());
    (made.expect("it converts").report.0.iter())
        .map(convert_words::dropped)
        .collect()
}

/// A folder holding `long.ytd`, a texture with `name`, converted from its menu up to the question about it.
fn asking_about_a_texture_named(name: &str) -> (AppState, egui::Context, tempfile::TempDir) {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let input = scratch.path().join("in");
    std::fs::create_dir_all(&input).expect("a folder");
    let bytes = a_ytd_holding(name).to_bytes().expect("a Legacy .ytd saves");
    std::fs::write(input.join("long.ytd"), bytes).expect("a file to convert");

    let (mut state, ctx) = browsing(&input, &ParamRemap::default());
    convert_from_the_menu(&ctx, &mut state, "long.ytd", "Convert to Enhanced…");
    (state, ctx, scratch)
}

/// The question on a screen of `size`, once it has settled.
fn the_question_on(
    ctx: &egui::Context,
    state: &mut AppState,
    size: egui::Vec2,
) -> egui::FullOutput {
    for _ in 0..2 {
        question_frame_on(ctx, state, size, Vec::new());
    }
    question_frame_on(ctx, state, size, Vec::new())
}

#[test]
fn a_texture_with_a_long_name_wraps_within_the_question_under_its_own_line() {
    let name = a_long_name();
    let values = dropped_in_words(&name).len();
    assert!(
        values > 1,
        "the texture drops {values} values, too few to show anything"
    );
    let (mut state, ctx, _scratch) = asking_about_a_texture_named(&name);

    let wide = the_question_on(&ctx, &mut state, egui::vec2(1600.0, 900.0));
    let (labels, rows) = (question_labels(&wide), question_rows(&wide));
    let at = |what: &str, shown: &[(String, egui::Rect)]| {
        (shown.iter())
            .find(|(text, _)| text.starts_with(what))
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("the question shows no `{what}`: {shown:?}"))
    };
    let file = at("long.ytd:", &rows);
    let closing = at("Nothing is invented", &labels);
    let under: Vec<&(String, egui::Rect)> = (rows.iter())
        .filter(|(_, row)| file.bottom() - 0.5 <= row.top() && row.bottom() <= closing.top() + 0.5)
        .collect();
    assert!(
        under.len() >= 2 * values,
        "the {values} values take one row each, so the long name did not wrap: {under:?}"
    );
    for (text, row) in &under {
        assert!(
            file.left() < row.left(),
            "`{text}` starts under its file's line, as a new line of the report does: {under:?}"
        );
        assert!(
            row.right() <= file.left() + QUESTION_WIDTH + 1.0,
            "`{text}` runs past the {QUESTION_WIDTH} of the question: {under:?}"
        );
    }

    let narrow = egui::vec2(560.0, 700.0);
    let shown = question_labels(&the_question_on(&ctx, &mut state, narrow));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, narrow);
    for (text, rect) in &shown {
        assert!(
            screen.contains_rect(*rect),
            "`{text}` is not within a window {} wide: {rect:?}",
            narrow.x
        );
    }
}

/// The text a drag across `rect`, then a copy, puts on the clipboard; `None` when nothing was selected.
fn dragged_over(ctx: &egui::Context, state: &mut AppState, rect: egui::Rect) -> Option<String> {
    let (from, to) = (
        rect.min + egui::vec2(1.0, 2.0),
        rect.max - egui::vec2(1.0, 2.0),
    );
    let button = |at, pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    question_frame(
        ctx,
        state,
        vec![egui::Event::PointerMoved(from), button(from, true)],
    );
    question_frame(ctx, state, vec![egui::Event::PointerMoved(to)]);
    question_frame(ctx, state, vec![button(to, false)]);
    let copied = question_frame(ctx, state, vec![egui::Event::Copy]);
    (copied.platform_output.commands.into_iter()).find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text),
        _ => None,
    })
}

#[test]
fn each_line_of_the_question_selects_on_its_own() {
    let _serial = crate::ui::theme::serial();
    let (mut state, ctx, _scratch) = asking_about_a_texture_named("paint");
    // ⚠ The app's own style, whose labels do not select unless a widget says they do.
    crate::ui::theme::apply_theme(&ctx, &Default::default());
    let shown = question_labels(&the_question_on(
        &ctx,
        &mut state,
        egui::vec2(1600.0, 900.0),
    ));

    let values = dropped_in_words("paint");
    assert!(
        values.len() > 1,
        "the texture drops too few values: {values:?}"
    );
    for line in &values {
        let rect = (shown.iter())
            .find(|(text, _)| text.contains(line.as_str()))
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("the question shows no `{line}`: {shown:?}"));
        assert_eq!(
            dragged_over(&ctx, &mut state, rect).as_deref(),
            Some(line.as_str()),
            "a drag across `{line}` does not select it and no other line"
        );
    }
}
