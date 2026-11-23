//! A conversion in a modder's words: its question names each value it drops
//! without the declarations' paths, its job and its dialogs say Convert, a
//! selection converted into a folder that holds one of its names offers
//! Replace, Skip and Keep both, and a document converts from its tab as it
//! stands.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::desc::ScalarValue;
use or_io::gta5::retarget::{Lost, Spot};
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_io::Generation;
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8, TARGET_GEN9};
use serde_json::Value;

use super::convert_words;
use super::converting_from_the_app::{
    browsing, buttons, clis_entry, convert_from_the_menu, database, shown_report, staged,
    written_by_the_job,
};
use super::jobs::JobKind;
use super::the_files_view_in_the_centre::{accessible, click, find, labels, node, settled};
use super::{AppState, DocumentId, Prefs};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
const DEADLINE: Duration = Duration::from_secs(120);

fn file_name(path: &Path) -> String {
    super::jobs::file_name(path)
}

/// The words of `text` that join names with `_`, `.` or `/`, or run a lower-case name into its number, as the declarations name a place or a field.
fn engine_paths(text: &str) -> Vec<&str> {
    let numbered = |word: &str| {
        let name = word.trim_end_matches(|c: char| c.is_ascii_digit());
        name.len() < word.len() && !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase())
    };
    (text.split(|c: char| c.is_whitespace() || ",:;()".contains(c)))
        .filter(|word| {
            word.contains(['_', '/'])
                || word.split('.').filter(|part| !part.is_empty()).count() > 1
                || numbered(word)
        })
        .collect()
}

/// The names the file itself gives that the command's report holds: a texture's in a place, a referred texture's in a field.
fn own_names(entry: &Value) -> Vec<String> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    (entry["dropped"].as_array().into_iter().flatten())
        .filter_map(|d| {
            let (at, field) = (text(&d["at"]), text(&d["field"]));
            let texture = (at.rsplit('/').next())
                .and_then(|spot| spot.strip_prefix("texture "))
                .map(str::to_string);
            texture.or_else(|| {
                Some(
                    field
                        .strip_prefix("reference ")?
                        .split_once(": ")?
                        .0
                        .to_string(),
                )
            })
        })
        .collect()
}

#[test]
fn a_drawable_converts_with_each_value_it_drops_in_words_and_no_engine_path() {
    const WHAT: &str = "a_drawable_converts_with_each_value_it_drops_in_words_and_no_engine_path";
    let defs = database();
    let remap = ParamRemap::of(&defs);
    for (target, to) in [
        (TARGET_GEN8, Generation::Gen9),
        (TARGET_GEN9, Generation::Gen8),
    ] {
        let Some(path) = staged(target, &remap) else {
            skip_corpus(
                WHAT,
                format_args!("no {target} .ydr whose conversion drops a value"),
            );
            continue;
        };
        let name = file_name(&path);
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let input = scratch.path().join("in");
        std::fs::create_dir_all(&input).expect("a folder");
        std::fs::copy(&path, input.join(&name)).expect("staged");
        let defs_json = scratch.path().join("shader_defs.json");
        std::fs::write(&defs_json, defs.to_json().expect("JSON")).expect("written");
        let cli = clis_entry(&input, &scratch.path().join("cli"), &defs_json);
        let dropped = cli["dropped"].as_array().map_or(0, Vec::len);
        assert!(dropped > 0, "{name}: the command drops nothing: {cli}");

        let (mut state, ctx) = browsing(&input, &remap);
        let item = format!("Convert to {}…", to.edition_label());
        convert_from_the_menu(&ctx, &mut state, &name, &item);
        let report = shown_report(&ctx, &mut state, &name);
        assert_eq!(
            report.lines().count(),
            dropped + 1,
            "{name}: a line for each value dropped, under the file's own:\n{report}"
        );
        let mut text = report.clone();
        for own in std::iter::once(name.clone()).chain(own_names(&cli)) {
            text = text.replace(&own, "");
        }
        let paths = engine_paths(&text);
        assert!(
            paths.is_empty(),
            "{name}: the question names {paths:?} as the declarations do:\n{report}"
        );
    }
}

fn dropped_values_read_in_words(shard: usize) {
    const WHAT: &str = "every_value_the_staged_files_drop_reads_in_words_with_no_engine_path";
    let with = ParamRemap::of(&database());
    let mut all: Vec<PathBuf> = ["ydr", "ytd", "ydd", "yft"]
        .into_iter()
        .flat_map(|ext| Corpus::new(MANIFEST, ext).all_targets().find())
        .collect();
    if all.is_empty() {
        skip_corpus(WHAT, "no staged file drops a value");
        return;
    }
    // Largest first, so that the shards that take every eighth file share the heavy ones.
    all.sort_by_key(|path| std::cmp::Reverse(std::fs::metadata(path).map_or(0, |m| m.len())));
    for path in super::shards::take(all, shard) {
        let bytes = std::fs::read(&path).expect("staged");
        for remap in [&with, &ParamRemap::default()] {
            let Ok(out) = or_cli::retarget_file(&file_name(&path), &bytes, remap) else {
                continue;
            };
            for dropped in &out.report.0 {
                let mut text = convert_words::dropped(dropped);
                // The names the file itself gives: a texture's in its place, a referred texture's in its loss.
                let own = match (&dropped.at.spot, &dropped.lost) {
                    (_, Lost::Reference(name, _)) | (Spot::Texture(name), _) => name.as_str(),
                    _ => "",
                };
                if !own.is_empty() {
                    text = text.replace(own, "");
                }
                let paths = engine_paths(&text);
                assert!(
                    paths.is_empty(),
                    "{}: `{dropped}` reads {text:?}, naming {paths:?}",
                    file_name(&path)
                );
            }
        }
    }
}

mod every_value_the_staged_files_drop_reads_in_words_with_no_engine_path {
    super::super::shards::shard_tests!(super::dropped_values_read_in_words);
}

/// Bytes `path` converts to with no shader database, as the command writes them.
fn converted(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("staged");
    or_cli::retarget_file(&file_name(path), &bytes, &ParamRemap::default())
        .expect("it converts")
        .bytes
}

/// Two Legacy `.ydr` converted as a selection into a folder that already holds the first's name, up to the question about it.
struct Clash {
    state: AppState,
    ctx: egui::Context,
    dest: PathBuf,
    names: [String; 2],
    staged: [PathBuf; 2],
    /// The question's widgets, once it is drawn.
    asked: Vec<(String, egui::Pos2)>,
    _scratch: tempfile::TempDir,
}

fn a_folder_that_holds_one_of_the_names(what: &str) -> Option<Clash> {
    let mut files = Corpus::new(MANIFEST, "ydr").target(TARGET_GEN8).find();
    files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
    let [a, b, ..] = &files[..] else {
        skip_corpus(what, "fewer than two Legacy .ydr staged");
        return None;
    };
    let names = [file_name(a), file_name(b)];
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (input, dest) = (scratch.path().join("in"), scratch.path().join("out"));
    std::fs::create_dir_all(&input).expect("a folder");
    std::fs::create_dir_all(&dest).expect("a folder");
    for (path, name) in [a, b].into_iter().zip(&names) {
        std::fs::copy(path, input.join(name)).expect("staged");
    }
    std::fs::write(dest.join(&names[0]), b"mine").expect("a file already there");

    let (mut state, ctx) = browsing(&input, &ParamRemap::default());
    let doc = state.active_document.expect("the folder is open");
    state.selection.select(doc, node(&state, doc, &names[0]));
    state.selection.toggle(doc, node(&state, doc, &names[1]));
    convert_from_the_menu(&ctx, &mut state, &names[0], "Convert to Enhanced…");
    assert_eq!(
        state.convert.done.as_ref().map(|done| done.files.len()),
        Some(2),
        "the menu converts the selection"
    );

    let asked = settled(&ctx, &mut state);
    crate::platform::file_dialogs::answer_next(dest.clone());
    let convert = find(&asked, egui::Rect::EVERYTHING, "Convert…");
    let said = crate::platform::tests::said(|| click(&ctx, &mut state, convert));
    assert!(
        said.iter()
            .any(|line| line.contains("the dialog \"Convert Into Folder\"")),
        "the folder is asked for as a conversion's: {said:?}"
    );
    let asked = settled(&ctx, &mut state);
    Some(Clash {
        state,
        ctx,
        dest,
        names,
        staged: [a.clone(), b.clone()],
        asked,
        _scratch: scratch,
    })
}

#[test]
fn a_selection_converted_into_a_folder_holding_one_of_its_names_offers_replace_skip_and_keep_both()
{
    const WHAT: &str = "a_selection_converted_into_a_folder_holding_one_of_its_names_offers_replace_skip_and_keep_both";
    let Some(mut clash) = a_folder_that_holds_one_of_the_names(WHAT) else {
        return;
    };
    let (ctx, state, [a_name, b_name]) = (&clash.ctx, &mut clash.state, &clash.names);
    let offered = buttons(ctx, state);
    for choice in ["Replace", "Skip", "Keep both"] {
        assert!(
            offered.contains(&(choice.to_string(), true)),
            "{a_name} is already there, and the question offers no {choice}: {offered:?}"
        );
    }
    click(
        ctx,
        state,
        find(&clash.asked, egui::Rect::EVERYTHING, "Keep both"),
    );
    let doing: Vec<String> = (state.running_jobs().into_iter())
        .map(|job| job.doing)
        .collect();
    assert!(
        doing.iter().any(|d| d.starts_with("Converting"))
            && !doing.iter().any(|d| d.starts_with("Extracting")),
        "the files are written as the conversion's job: {doing:?}"
    );
    written_by_the_job(state);
    let stem = Path::new(a_name).file_stem().expect("a name");
    let kept = clash
        .dest
        .join(format!("{} (2).ydr", stem.to_string_lossy()));
    assert_eq!(
        std::fs::read(clash.dest.join(a_name)).ok().as_deref(),
        Some(&b"mine"[..]),
        "Keep both leaves the file already there as it was"
    );
    assert!(
        std::fs::read(&kept).ok() == Some(converted(&clash.staged[0])),
        "Keep both writes the converted file beside it as {}",
        kept.display()
    );
    assert!(
        std::fs::read(clash.dest.join(b_name)).ok() == Some(converted(&clash.staged[1])),
        "the other file is written under its own name"
    );
}

#[test]
fn replace_writes_over_the_file_already_there_and_skip_leaves_it_as_it_is() {
    const WHAT: &str = "replace_writes_over_the_file_already_there_and_skip_leaves_it_as_it_is";
    for (choice, replaced) in [("Replace", true), ("Skip", false)] {
        let Some(mut clash) = a_folder_that_holds_one_of_the_names(WHAT) else {
            return;
        };
        let (ctx, state) = (&clash.ctx, &mut clash.state);
        click(
            ctx,
            state,
            find(&clash.asked, egui::Rect::EVERYTHING, choice),
        );
        written_by_the_job(state);
        let there = std::fs::read(clash.dest.join(&clash.names[0])).ok();
        let expected = if replaced {
            converted(&clash.staged[0])
        } else {
            b"mine".to_vec()
        };
        assert!(there == Some(expected), "{choice} on {}", clash.names[0]);
        assert!(
            std::fs::read(clash.dest.join(&clash.names[1])).ok()
                == Some(converted(&clash.staged[1])),
            "{choice} still writes the file nothing was holding"
        );
        assert!(
            state.convert.clash.is_none() && state.convert.done.is_none(),
            "{choice} answers the question"
        );
    }
}
#[test]
fn cancelling_a_conversions_write_stops_it_and_never_stops_an_extraction() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());
    state.extract_jobs.start(
        JobKind::Conversion,
        "3 files".into(),
        scratch.path().to_path_buf(),
        Vec::new(),
        |cancel, progress| {
            or_job::run(0..1_000_000u32, &cancel, &progress, |_| {
                std::thread::sleep(Duration::from_millis(1));
                or_job::Step::Done(0)
            })
        },
    );
    let job = state.running_jobs().pop().expect("the write is listed");
    assert_eq!(
        (job.kind, job.doing.as_str()),
        (JobKind::Conversion, "Converting 3 files")
    );

    state.cancel_job(JobKind::Extraction);
    assert!(
        state.running_jobs().iter().all(|job| !job.stopping),
        "Cancel Extraction leaves a conversion's write running"
    );
    state.cancel_job(JobKind::Conversion);
    assert!(
        state.running_jobs().iter().all(|job| job.stopping),
        "Cancel Conversion stops its write"
    );
}

/// One frame of the tab strip, the dock and the questions, and every labelled widget it laid out.
fn shell(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    labels(&ctx.run_ui(input, |ui| {
        crate::ui::panels::document_tabs::draw(ui, state);
        crate::ui::dock::draw(ui, state, None);
        crate::ui::panels::file_prompts::draw(ui.ctx(), state);
    }))
}

fn press(ctx: &egui::Context, state: &mut AppState, at: egui::Pos2, which: egui::PointerButton) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: which,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell(
        ctx,
        state,
        vec![egui::Event::PointerMoved(at), button(true)],
    );
    shell(ctx, state, vec![button(false)]);
}

fn opened(state: &mut AppState) -> DocumentId {
    let until = Instant::now() + DEADLINE;
    while state.load_receiver.is_some() && Instant::now() < until {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    state.active_document.expect("the model opened")
}

/// Sets the first level of detail's distance of the open drawable `doc` to `distance`, as the grid does, and leaves it unsaved.
fn edit_the_first_distance(state: &mut AppState, doc: DocumentId, distance: f32) {
    let image = (state.document(doc).and_then(|d| d.resource.clone()))
        .expect("an open drawable keeps its image");
    let format = or_io::gta5::formats::format("ydr").expect("declared");
    let target = (format.target_of_version(image.seal.version())).expect("a declared version");
    let pack = format.description(target).expect("described");
    let root = format.root_name(target).expect("a root");
    let edit = (or_editor::Summary::of(&pack, &root, image.view(), 0)
        .expect("a summary")
        .rows)
        .iter()
        .find(|row| row.label.ends_with("Distances [0]"))
        .and_then(|row| row.edit)
        .expect("the first distance is editable");
    assert!(state.patch_scalar(doc, "distance", edit, ScalarValue::F32(distance)));
}

#[test]
fn a_document_converts_from_its_tab_as_it_stands_its_unsaved_edits_included() {
    const WHAT: &str = "a_document_converts_from_its_tab_as_it_stands_its_unsaved_edits_included";
    const DISTANCE: f32 = 1234.5;
    let remap = ParamRemap::of(&database());
    let Some(path) = staged(TARGET_GEN8, &remap) else {
        skip_corpus(WHAT, "no Legacy .ydr whose conversion drops a value");
        return;
    };
    let name = file_name(&path);
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let model = scratch.path().join(&name);
    std::fs::copy(&path, &model).expect("staged");
    let on_disk = std::fs::read(&model).expect("staged");
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.shader_defs.remap = Arc::new(remap);
    state.load_file(model.clone());
    let doc = opened(&mut state);
    edit_the_first_distance(&mut state, doc, DISTANCE);

    let ctx = accessible();
    shell(&ctx, &mut state, Vec::new());
    let title = state.tab_title(state.document(doc).expect("open"));
    let strip = egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::pos2(1600.0, crate::ui::theme::TAB_STRIP_HEIGHT),
    );
    let tab = find(
        &shell(&ctx, &mut state, Vec::new()),
        strip,
        &format!("{title} ●"),
    );
    press(&ctx, &mut state, tab, egui::PointerButton::Secondary);
    let menu = shell(&ctx, &mut state, Vec::new());
    let item = find(&menu, egui::Rect::EVERYTHING, "Convert to Enhanced…");
    press(&ctx, &mut state, item, egui::PointerButton::Primary);
    let until = Instant::now() + DEADLINE;
    while state.convert.done.is_none() && Instant::now() < until {
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        state.convert.done.is_some(),
        "the tab's conversion never asked"
    );

    shell(&ctx, &mut state, Vec::new());
    let asked = shell(&ctx, &mut state, Vec::new());
    assert!(
        asked
            .iter()
            .any(|(text, _)| text.contains("unsaved edits included")),
        "the question says it converts the document as it stands: {asked:?}"
    );
    let out: PathBuf = scratch.path().join("out");
    std::fs::create_dir_all(&out).expect("a folder");
    crate::platform::file_dialogs::answer_next(out.join(&name));
    let convert = find(&asked, egui::Rect::EVERYTHING, "Convert…");
    let said = crate::platform::tests::said(|| {
        press(&ctx, &mut state, convert, egui::PointerButton::Primary)
    });
    assert!(
        said.iter()
            .any(|line| line.contains(&format!("the dialog \"Convert {name}\""))),
        "the file is asked for as a conversion's: {said:?}"
    );
    written_by_the_job(&mut state);

    let written = std::fs::read(out.join(&name)).expect("the conversion wrote the file");
    let enhanced =
        <or_io::gta5::GtaDrawable<or_io::decl::Gen9> as or_io::decl::File>::from_bytes(&written)
            .expect("an Enhanced drawable");
    assert_eq!(
        enhanced.base.lod_group.lod_distances[0], DISTANCE,
        "the edit not yet saved is in what the conversion wrote"
    );
    assert_eq!(
        std::fs::read(&model).ok(),
        Some(on_disk),
        "the file on disk is as it was"
    );
}
