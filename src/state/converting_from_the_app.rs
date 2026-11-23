//! A file converts to the other edition from its row's menu, and the report
//! the question shows before anything is written names the values
//! `openrage-cli convert retarget` reports for the same file, each in words,
//! and writes the bytes it writes.
//!
//! ⚠ The database is given to the app's state, not installed in the process,
//! so no other test's database reaches these reports.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::gta5::shaders::fxc::GrcEffect;
use or_io::gta5::shaders::shader_defs::{ParamRemap, ShaderDefinitions};
use or_io::Generation;
use or_test_support::{Corpus, TARGET_GEN8, TARGET_GEN9};
use serde_json::Value;

use super::convert::{Converted, ConvertedFile};
use super::the_files_view_in_the_centre::{
    accessible, click, click_with, find, frame, leaf_of, listed, settled,
};
use super::{AppState, EditMode, Game, Prefs};
use crate::ui::panels::registry::ids;

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
const DEADLINE: Duration = Duration::from_secs(120);

/// A shader database assembled from the staged `.fxc` effects, which may be none.
pub(super) fn database() -> ShaderDefinitions {
    let effects: Vec<(String, GrcEffect)> = (Corpus::new(MANIFEST, "fxc").find().iter())
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            let effect = stem.rsplit("__").next()?.to_string();
            Some((effect, GrcEffect::parse(&std::fs::read(path).ok()?).ok()?))
        })
        .collect();
    ShaderDefinitions::assemble("gta5", "gen8", &effects, &[]).0
}

/// The smallest staged `.ydr` of `target` whose conversion drops a value, and drops others without `remap`.
///
/// ⚠ A report the database does not change would match the command's even
/// if the app converted with no database at all.
pub(super) fn staged(target: &str, remap: &ParamRemap) -> Option<PathBuf> {
    let mut files = Corpus::new(MANIFEST, "ydr").target(target).find();
    files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
    let report = |path: &Path, remap: &ParamRemap| {
        let bytes = std::fs::read(path).ok()?;
        let out = or_cli::retarget_file(&path.to_string_lossy(), &bytes, remap).ok()?;
        Some(out.report)
    };
    files.into_iter().find(|path| {
        let with = report(path, remap);
        with.as_ref().is_some_and(|r| !r.0.is_empty())
            && with != report(path, &ParamRemap::default())
    })
}

/// What `openrage-cli convert retarget` reports for the one file in `input`, writing into `out`.
pub(super) fn clis_entry(input: &Path, out: &Path, defs: &Path) -> Value {
    let mut words = vec!["convert".to_string(), "retarget".to_string()];
    for word in [
        input,
        Path::new("--out"),
        out,
        Path::new("--shader-defs"),
        defs,
    ] {
        words.push(word.to_string_lossy().into_owned());
    }
    let _ = or_cli::run(&words);
    let report: Value = serde_json::from_slice(
        &std::fs::read(out.join("retarget_report.json")).expect("the command writes its report"),
    )
    .expect("the report is JSON");
    match report["files"].as_array().map(Vec::as_slice) {
        Some([one]) => one.clone(),
        other => panic!("the command reports one file: {other:?}"),
    }
}

/// `input` open as a folder in Browse, converting with `remap`.
pub(super) fn browsing(input: &Path, remap: &ParamRemap) -> (AppState, egui::Context) {
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.shader_defs.remap = Arc::new(remap.clone());
    state.open_folder(input.to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let ctx = accessible();
    let until = Instant::now() + DEADLINE;
    while listed(&state, doc).is_empty() && Instant::now() < until {
        state.poll_expansions();
        frame(&ctx, &mut state, Vec::new());
    }
    (state, ctx)
}

/// Opens row `name`'s menu in the Files view, clicks `item` in it, and waits for the question.
pub(super) fn convert_from_the_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    name: &str,
    item: &str,
) {
    let shown = settled(ctx, state);
    let row = find(&shown, leaf_of(state, ids::DOCUMENT), name);
    click_with(ctx, state, row, egui::PointerButton::Secondary);
    let menu = frame(ctx, state, Vec::new());
    click(ctx, state, find(&menu, egui::Rect::EVERYTHING, item));
    let until = Instant::now() + DEADLINE;
    while state.convert.done.is_none() && Instant::now() < until {
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        state.convert.done.is_some(),
        "{name}: the conversion never asked"
    );
}

/// One frame of the questions alone on a 1600 by 900 screen, with `events`.
pub(super) fn question_frame(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    question_frame_on(ctx, state, egui::vec2(1600.0, 900.0), events)
}

/// One frame of the questions alone on a screen of `size`, with `events`.
pub(super) fn question_frame_on(
    ctx: &egui::Context,
    state: &mut AppState,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    ctx.run_ui(input, |ui| {
        crate::ui::panels::file_prompts::draw(ui.ctx(), state);
    })
}

/// Each labelled widget `output` laid out and where; a label's text runs are part of it and are left out.
pub(super) fn question_labels(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    laid_out(output, |role| role != egui::accesskit::Role::TextRun)
}

/// Each row of text `output` laid out and where: a label that wraps is one row for each line it takes.
pub(super) fn question_rows(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    laid_out(output, |role| role == egui::accesskit::Role::TextRun)
}

fn laid_out(
    output: &egui::FullOutput,
    of_role: impl Fn(egui::accesskit::Role) -> bool,
) -> Vec<(String, egui::Rect)> {
    let Some(update) = &output.platform_output.accesskit_update else {
        return Vec::new();
    };
    (update.nodes.iter())
        .filter(|(_, node)| of_role(node.role()))
        .filter_map(|(_, node)| {
            let text = node.label().or_else(|| node.value())?;
            let b = node.bounds()?;
            let rect = egui::Rect::from_min_max(
                egui::pos2(b.x0 as f32, b.y0 as f32),
                egui::pos2(b.x1 as f32, b.y1 as f32),
            );
            Some((text.to_string(), rect))
        })
        .collect()
}

/// The lines of the report `output` shows: those between the question's summary of `name` and its note, top to bottom.
///
/// ⚠ The report scrolls, and the accessibility tree holds a line below the area's edge no more and one above it still.
fn report_lines_shown(output: &egui::FullOutput, name: &str) -> Vec<String> {
    let labels = question_labels(output);
    let summary = (labels.iter())
        .find(|(text, _)| text.starts_with(&format!("{name} ")) && text.ends_with(['.', ':']))
        .map_or(f32::INFINITY, |(_, rect)| rect.bottom());
    let note = (labels.iter())
        .find(|(text, _)| text.starts_with("Nothing is invented"))
        .map_or(f32::NEG_INFINITY, |(_, rect)| rect.top());
    let mut lines: Vec<(String, egui::Rect)> = (labels.into_iter())
        .filter(|(_, rect)| summary <= rect.top() && rect.bottom() <= note)
        .collect();
    lines.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
    lines.into_iter().map(|(text, _)| text).collect()
}

/// The report the question shows, which begins with the first file's name: its line, then a line for each value it drops, indented.
///
/// ⚠ Scrolled a page at a time to the end, since the report's area clips what does not fit.
pub(super) fn shown_report(ctx: &egui::Context, state: &mut AppState, name: &str) -> String {
    for _ in 0..2 {
        question_frame(ctx, state, Vec::new());
    }
    let first = question_frame(ctx, state, Vec::new());
    let mut lines = report_lines_shown(&first, name);
    let Some(over) = question_labels(&first)
        .into_iter()
        .find(|(text, _)| lines.first() == Some(text))
        .map(|(_, rect)| rect.center())
    else {
        panic!("{name}: the question shows no report: {lines:?}");
    };
    for _ in 0..40 {
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -200.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        };
        question_frame(ctx, state, vec![egui::Event::PointerMoved(over), wheel]);
        // The scroll eases in over a few frames.
        for _ in 0..40 {
            question_frame(ctx, state, Vec::new());
        }
        let page = report_lines_shown(&question_frame(ctx, state, Vec::new()), name);
        let seen = (0..=page.len().min(lines.len()))
            .rev()
            .find(|&n| lines.ends_with(&page[..n]))
            .unwrap_or(0);
        if seen == page.len() {
            break;
        }
        lines.extend_from_slice(&page[seen..]);
    }
    let start = (lines.iter())
        .position(|text| text.starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("{name}: the question shows no report: {lines:?}"));
    let values = lines[start + 1..].iter().map(|text| format!("  {text}"));
    let report: Vec<String> = std::iter::once(lines[start].clone())
        .chain(values)
        .collect();
    report.join("\n")
}

/// Each button the question draws, and whether it can be clicked.
pub(super) fn buttons(ctx: &egui::Context, state: &mut AppState) -> Vec<(String, bool)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        crate::ui::panels::file_prompts::draw(ui.ctx(), state);
    });
    let Some(update) = output.platform_output.accesskit_update else {
        return Vec::new();
    };
    (update.nodes.iter())
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
        .filter_map(|(_, node)| Some((node.label()?.to_string(), !node.is_disabled())))
        .collect()
}

pub(super) fn written_by_the_job(state: &mut AppState) {
    let until = Instant::now() + DEADLINE;
    while state.extract_jobs.is_running() && Instant::now() < until {
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_drawable_converts_from_its_menu_and_its_report_equals_the_clis() {
    const WHAT: &str = "a_drawable_converts_from_its_menu_and_its_report_equals_the_clis";
    let defs = database();
    let remap = ParamRemap::of(&defs);
    for (target, to) in [
        (TARGET_GEN8, Generation::Gen9),
        (TARGET_GEN9, Generation::Gen8),
    ] {
        let Some(path) = staged(target, &remap) else {
            or_test_support::skip_corpus(
                WHAT,
                format_args!("no {target} .ydr whose conversion the staged .fxc change"),
            );
            continue;
        };
        let name = path.file_name().expect("a file").to_string_lossy();
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let (input, by_app) = (scratch.path().join("in"), scratch.path().join("app"));
        std::fs::create_dir_all(&input).expect("a folder");
        std::fs::copy(&path, input.join(&*name)).expect("staged");
        let defs_json = scratch.path().join("shader_defs.json");
        std::fs::write(&defs_json, defs.to_json().expect("JSON")).expect("written");
        let cli = clis_entry(&input, &scratch.path().join("cli"), &defs_json);
        assert_eq!(cli["status"], "converted", "{name}: {cli}");
        let text = |v: &Value| v.as_str().expect("a string").to_string();
        let clis_drops: Vec<String> = (cli["dropped"].as_array().expect("a list").iter())
            .map(|d| format!("{}: {}", text(&d["at"]), text(&d["field"])))
            .collect();
        // The call both make: the command prints each value as the declarations name it, the app words the same ones.
        let made = std::fs::read(&path).expect("staged");
        let made = or_cli::retarget_file(&name, &made, &remap).expect("it converts");
        let printed: Vec<String> = made.report.0.iter().map(ToString::to_string).collect();
        assert_eq!(
            printed, clis_drops,
            "{name}: the command prints what it reports"
        );
        let in_words: Vec<String> = made
            .report
            .0
            .iter()
            .map(super::convert_words::dropped)
            .collect();

        let (mut state, ctx) = browsing(&input, &remap);
        let item = format!("Convert to {}…", to.edition_label());
        convert_from_the_menu(&ctx, &mut state, &name, &item);
        let asked = settled(&ctx, &mut state);
        find(
            &asked,
            egui::Rect::EVERYTHING,
            &format!("Convert to {}", to.edition_label()),
        );
        let report = shown_report(&ctx, &mut state, &name);
        let shown_drops: Vec<&str> = (report.lines())
            .filter_map(|line| line.strip_prefix("  "))
            .collect();
        assert_eq!(
            shown_drops, in_words,
            "{name}: the question's report names the values the command's does, each in words"
        );
        assert!(
            buttons(&ctx, &mut state).contains(&("Convert…".to_string(), true)),
            "{name}: the question offers to write it"
        );
        assert!(
            !by_app.exists(),
            "{name}: something was written before the report was answered"
        );

        let dest = by_app.join(&*name);
        std::fs::create_dir_all(&by_app).expect("a folder");
        state.write_conversion(dest.clone(), None);
        written_by_the_job(&mut state);
        assert!(
            state.convert.done.is_none(),
            "{name}: the question is answered"
        );
        let written = std::fs::read(&dest).unwrap_or_else(|e| panic!("{name}: {e}"));
        let clis = std::fs::read(scratch.path().join("cli").join(&*name)).expect("its output");
        assert!(
            written == clis,
            "{name}: the app wrote other bytes than the command for the same file"
        );
    }
}

/// A Legacy drawable's header over bytes no drawable holds.
fn not_a_drawable() -> Vec<u8> {
    use or_io::container::resource::Rsc7;
    let version = (or_io::gta5::formats::format("ydr")
        .expect("declared")
        .file_type)
        .version_for(Generation::Gen8)
        .expect("a Legacy version");
    let nibble = |v: u32| (v & 0xF) << 28;
    let header = Rsc7::header_from_entry_flags(nibble(version >> 4), nibble(version));
    [header.header_bytes(), vec![0; 64]].concat()
}

#[test]
fn a_file_that_cannot_convert_says_why_and_offers_nothing_to_write() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let input = scratch.path().join("in");
    std::fs::create_dir_all(&input).expect("a folder");
    std::fs::write(input.join("broken.ydr"), not_a_drawable()).expect("written");
    let none = ShaderDefinitions::new("gta5", "gen8");
    let defs_json = scratch.path().join("shader_defs.json");
    std::fs::write(&defs_json, none.to_json().expect("JSON")).expect("written");
    let cli = clis_entry(&input, &scratch.path().join("cli"), &defs_json);
    assert_eq!(cli["status"], "refused", "{cli}");
    let why = cli["why"].as_str().expect("why it is refused");

    let (mut state, ctx) = browsing(&input, &ParamRemap::of(&none));
    convert_from_the_menu(&ctx, &mut state, "broken.ydr", "Convert to Enhanced…");
    assert_eq!(
        shown_report(&ctx, &mut state, "broken.ydr"),
        format!("broken.ydr: cannot be converted: {why}"),
        "the refusal is the command's, in its words"
    );
    let offered = buttons(&ctx, &mut state);
    assert!(
        offered.contains(&("Convert…".to_string(), false))
            && offered.contains(&("Cancel".to_string(), true)),
        "a refused file offers nothing to write: {offered:?}"
    );
}

/// What a conversion made of `bytes`, as its worker hands it over.
fn converted(bytes: &[u8]) -> Converted {
    Converted {
        files: vec![ConvertedFile {
            name: "prop.ydr".into(),
            outcome: Ok(or_cli::Retargeted {
                from: Generation::Gen8,
                to: Generation::Gen9,
                bytes: bytes.to_vec(),
                report: Default::default(),
            }),
        }],
        without_database: true,
        of_document: false,
    }
}

#[test]
fn a_conversion_asks_the_edit_gate_where_it_writes() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let install = scratch.path().join("GTAV");
    std::fs::create_dir_all(install.join("mods")).expect("an install");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("a marker");
    let mut state = AppState::from_prefs(Prefs::default());
    (state.prefs.game_paths).set(Game::GtaVLegacy, Some(install.clone()));
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.convert.done = Some(converted(b"converted"));

    let inside = install.join("mods").join("prop.ydr");
    state.write_conversion(inside.clone(), None);
    written_by_the_job(&mut state);
    assert!(
        !inside.exists(),
        "Read only writes nothing inside the install"
    );
    assert!(
        state.convert.done.is_some(),
        "a refused write keeps the question, to choose another place"
    );

    let outside = scratch.path().join("prop.ydr");
    state.write_conversion(outside.clone(), None);
    written_by_the_job(&mut state);
    assert_eq!(
        std::fs::read(&outside).ok().as_deref(),
        Some(&b"converted"[..]),
        "outside the install it writes"
    );
}
