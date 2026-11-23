//! Recents say when a file was last opened, skip what is gone and hold archive
//! entries; the viewport's look survives a restart; a question leaves the
//! shell behind it inert; and a game switch never drops or opens a file the
//! user did not answer for.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use or_renderer::ShadingMode;

use super::navigation::{resolve_path, Resolved};
use super::nested_archives_survive_a_save::{archive, open, Opened};
use super::prefs::{load_prefs_from, save_prefs_to, Game, Prefs};
use super::{
    AppState, CloseAnswer, DocPayload, DocumentId, DocumentKind, EnhancedOpenDecision,
    RpfEntryBinding, TextDocument,
};
use crate::ui::commands::{self, Action, Command};

/// Poll as the editor's frame does until nothing is loading or walking.
fn settle(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_load_result();
        state.poll_expansions();
        state.tick_navigation();
        if state.load_receiver.is_none() && state.navigation.pending_place().is_none() {
            return;
        }
        assert!(Instant::now() < deadline, "the open never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The recent-file command whose hover names `path`.
fn recent(state: &AppState, path: &str) -> Option<Command> {
    commands::recent_files(state)
        .into_iter()
        .find(|c| c.hover.as_deref() == Some(path))
}

#[test]
fn recents_say_when_a_file_was_last_opened_skip_missing_files_and_hold_archive_entries() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());

    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, b"notes").expect("notes.txt");
    let three_days = Duration::from_secs(3 * 24 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(&notes)
        .and_then(|f| f.set_modified(SystemTime::now() - three_days))
        .expect("an old modified time");
    state.load_file(notes.clone());
    settle(&mut state);
    let label = recent(&state, &notes.display().to_string())
        .map(|c| c.label)
        .expect("an opened file is a recent one");
    assert!(
        label.contains("just now"),
        "a file opened a moment ago reads as {label:?}, its modified time"
    );

    let gone = dir.path().join("gone.txt");
    std::fs::write(&gone, b"gone").expect("gone.txt");
    state.load_file(gone.clone());
    settle(&mut state);
    std::fs::remove_file(&gone).expect("the file is deleted");
    assert!(
        recent(&state, &gone.display().to_string()).is_none(),
        "a deleted file is still offered: {:?}",
        commands::recent_files(&state)
            .iter()
            .map(|c| &c.label)
            .collect::<Vec<_>>()
    );

    let outer = archive(&[("inside.txt", b"inside")]);
    let (doc, path, _) = open(&mut state, dir.path(), &outer, Opened::AsADocument);
    let row = match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => resolve_path(&rpf.session, "inside.txt"),
        _ => Resolved::Missing,
    };
    let Resolved::Found(row) = row else {
        panic!("inside.txt is not in the archive: {row:?}");
    };
    state.open_rpf_entry(doc, row);
    settle(&mut state);
    let entry = commands::recent_files(&state)
        .into_iter()
        .find(|c| c.label.starts_with("inside.txt"))
        .expect("an archive entry opened is a recent file");
    assert!(
        entry
            .hover
            .as_deref()
            .is_some_and(|hover| hover.contains(&path.display().to_string())),
        "the entry does not say which archive holds it: {:?}",
        entry.hover
    );

    state.documents.clear();
    state.active_document = None;
    commands::run(&mut state, &egui::Context::default(), &entry);
    settle(&mut state);
    assert!(
        state
            .documents
            .iter()
            .any(|d| d.rpf_entry_binding.is_some() && d.file_name() == "inside.txt"),
        "the recent entry did not open from its archive"
    );
}

#[test]
fn shaded_is_a_mode_a_restart_reopens_in_and_the_key_the_game_shaders_mode_had_reads_as_it() {
    let mut state = AppState::from_prefs(Prefs::default());
    state.remember_shading_mode(ShadingMode::Shaded);
    state.persist_viewport_prefs();

    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = dir.path().join("prefs.json");
    save_prefs_to(&file, &state.prefs);
    assert!(
        std::fs::read_to_string(&file)
            .expect("the file is written")
            .contains("\"shading\": \"shaded\""),
        "the mode is kept by its own key"
    );
    let restarted = AppState::from_prefs(load_prefs_from(&file));
    assert_eq!(restarted.prefs.viewport.shading, ShadingMode::Shaded);

    // What the mode that draws with the game's shaders was saved as, before it took the name
    let before = std::fs::read_to_string(&file)
        .expect("the file is written")
        .replace("\"shading\": \"shaded\"", "\"shading\": \"game\"");
    std::fs::write(&file, before).expect("the file is written");
    let restarted = AppState::from_prefs(load_prefs_from(&file));
    assert_eq!(restarted.prefs.viewport.shading, ShadingMode::Shaded);
}

#[test]
fn the_viewports_mode_background_and_exposure_survive_a_restart() {
    let mut state = AppState::from_prefs(Prefs::default());
    state.remember_shading_mode(ShadingMode::Shaded);
    state.remember_shading_mode(ShadingMode::Channel);
    state.drivers.world_background.colour = [0.5, 0.25, 0.125, 1.0];
    state.drivers.world_background.sky = true;
    state.drivers.world_background.ground = true;
    state.drivers.exposure.set_value(0.75);
    state.persist_viewport_prefs();

    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = dir.path().join("prefs.json");
    save_prefs_to(&file, &state.prefs);
    let restarted = AppState::from_prefs(load_prefs_from(&file));

    assert_eq!(
        restarted.prefs.viewport.shading,
        ShadingMode::Shaded,
        "one file's channel is not a mode to reopen in"
    );
    assert_eq!(
        restarted.drivers.world_background.colour,
        [0.5, 0.25, 0.125, 1.0]
    );
    assert!(
        restarted.drivers.world_background.sky,
        "the sky is kept as the background"
    );
    assert!(
        restarted.drivers.world_background.ground,
        "the ground under an effect is kept"
    );
    assert_eq!(restarted.drivers.exposure.value, 0.75);
}

/// Draws the editor's shell and its overlays as its frame does, and says where each labelled widget is.
struct Shell {
    ctx: egui::Context,
    time: f64,
}

impl Shell {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self { ctx, time: 0.0 }
    }

    fn frame(
        &mut self,
        state: &mut AppState,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Pos2)> {
        self.time += 0.05;
        self.ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        });
        crate::ui::draw(&self.ctx, state, None);
        crate::app::modals::draw_editor_overlays(&self.ctx, state);
        let output = self.ctx.end_pass();
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        let centre =
                            egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                        Some((node.label()?.to_string(), centre))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clicks the widget labelled `label` and says what the next frame draws.
    fn click(&mut self, state: &mut AppState, label: &str) -> Vec<String> {
        let at = self
            .frame(state, Vec::new())
            .into_iter()
            .find(|(l, _)| l == label)
            .unwrap_or_else(|| panic!("nothing labelled {label} is drawn"))
            .1;
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(state, vec![egui::Event::PointerMoved(at), press(true)]);
        self.frame(state, vec![press(false)]);
        self.frame(state, Vec::new())
            .into_iter()
            .map(|(l, _)| l)
            .collect()
    }
}

/// Leaves a question waiting on a state.
type Ask = Box<dyn Fn(&mut AppState)>;

/// Each question the shell can ask, by name, with what leaves it waiting.
fn questions() -> Vec<(&'static str, Ask)> {
    use super::delete_prompt::{DeleteFrom, PendingDelete};
    use super::extract_job::{ExtractClash, PendingExportFixes, TypeFilter};
    use super::{PendingEnhancedOpen, PendingRetailEdit, PendingRetailEditPrompt};

    vec![
        (
            "unsaved changes",
            Box::new(|state: &mut AppState| {
                let id = state.new_empty_document(DocumentKind::Text);
                state.close_document(id);
            }),
        ),
        (
            "changed on disk",
            Box::new(|state: &mut AppState| state.reload_conflicts.push("a.ypt".into())),
        ),
        (
            "a delete",
            Box::new(|state: &mut AppState| {
                state.pending_delete = Some(PendingDelete {
                    doc_id: 0,
                    node_id: 0,
                    name: "old.ydr".into(),
                    files: 1,
                    folders: 0,
                    bytes: 4,
                    from: DeleteFrom::Disk,
                })
            }),
        ),
        (
            "an extraction's clash",
            Box::new(|state: &mut AppState| {
                state.extract_jobs.clash = Some(ExtractClash {
                    doc_id: 0,
                    nodes: Vec::new(),
                    dest: "out".into(),
                    form: or_io::container::rpf::BodyForm::Decoded,
                    filter: TypeFilter::All,
                    fixes: false,
                    clashes: 1,
                    planned: 1,
                    first: "a.ydr".into(),
                })
            }),
        ),
        (
            "a base-game write",
            Box::new(|state: &mut AppState| {
                state.pending_retail_edit = Some(PendingRetailEditPrompt {
                    action: PendingRetailEdit::SaveLooseFile { doc_id: 0 },
                    target_path: "x64a.rpf".into(),
                    verb: "save",
                })
            }),
        ),
        (
            "the export fixes",
            Box::new(|state: &mut AppState| {
                state.pending_export_fixes = Some(PendingExportFixes::DragOut)
            }),
        ),
        (
            "an Enhanced file",
            Box::new(|state: &mut AppState| {
                state.pending_enhanced_open = Some(PendingEnhancedOpen {
                    path: "enhanced.ydr".into(),
                    game: Game::GtaVEnhanced,
                    version: 0,
                    enhanced_root_known: false,
                })
            }),
        ),
        (
            "the keys",
            Box::new(|state: &mut AppState| state.keys.show_prompt = true),
        ),
        (
            "a refused write",
            Box::new(|state: &mut AppState| state.refused_write = Some("x64a.rpf".into())),
        ),
        (
            "an error",
            Box::new(|state: &mut AppState| {
                state.set_error("Could not open the file", "not a drawable".into())
            }),
        ),
    ]
}

/// Falsified by drawing a question as a window the shell keeps working behind:
/// the File menu then opens and a key's command runs while it waits.
#[test]
fn every_question_leaves_the_shell_behind_it_inert_until_it_is_answered() {
    let fresh = || {
        let mut state = AppState::from_prefs(Prefs::default());
        state.documents.clear();
        state.active_document = None;
        state
    };
    let mut state = fresh();
    let mut shell = Shell::new();
    shell.frame(&mut state, Vec::new());
    assert!(
        shell.click(&mut state, "File").iter().any(|l| l == "Exit"),
        "with nothing asked the File menu does not open, so this cannot tell"
    );

    let mut failures = Vec::new();
    for (question, ask) in questions() {
        let mut state = fresh();
        let mut shell = Shell::new();
        ask(&mut state);
        shell.frame(&mut state, Vec::new());
        shell.frame(&mut state, Vec::new());
        if shell.click(&mut state, "File").iter().any(|l| l == "Exit") {
            failures.push(format!("{question}: the File menu opened"));
        }
        let grid = state.prefs.viewport.show_grid;
        commands::run_action(&mut state, &shell.ctx, Action::ShowGrid);
        if state.prefs.viewport.show_grid != grid {
            failures.push(format!("{question}: a key's command ran"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The Legacy install at `dir/legacy` open, with a file opened from it left unsaved, and an Enhanced install beside it.
fn an_install_with_unsaved_work(dir: &Path) -> (AppState, DocumentId, DocumentId) {
    let (legacy, enhanced) = (dir.join("legacy"), dir.join("enhanced"));
    for root in [&legacy, &enhanced] {
        std::fs::create_dir(root).expect("an install folder");
    }
    let mut prefs = Prefs::default();
    prefs.game_paths.set(Game::GtaVLegacy, Some(legacy.clone()));
    prefs.game_paths.set(Game::GtaVEnhanced, Some(enhanced));
    prefs.active_game = Some(Game::GtaVLegacy);
    let mut state = AppState::from_prefs(prefs);
    let install = state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::GameRoot)
        .expect("the Legacy install is open")
        .id;
    let file = state.insert_document(
        legacy.join("commandline.txt"),
        DocPayload::Text(TextDocument::decode(b"-windowed")),
        Some(RpfEntryBinding {
            parent_doc_id: install,
            entry_id: 1,
        }),
        None,
    );
    state.document_mut(file).expect("open").mark_edited();
    (state, install, file)
}

#[test]
fn clearing_the_active_games_folder_closes_its_install_once_its_unsaved_work_is_answered() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let (mut state, install, file) = an_install_with_unsaved_work(dir.path());

    state.set_game_install_folder(Game::GtaVLegacy, None);
    assert!(
        state.document(file).is_some_and(|d| d.dirty),
        "Clear dropped a file opened from the install before the user answered"
    );
    assert!(state.pending_close.is_some(), "Clear asked nothing");
    state.answer_close(CloseAnswer::Cancel);
    assert!(state.document(install).is_some());
    assert!(
        state.prefs.game_paths.get(Game::GtaVLegacy).is_some(),
        "Cancel still cleared the folder"
    );

    state.set_game_install_folder(Game::GtaVLegacy, None);
    state.answer_close(CloseAnswer::DontSave);
    assert!(
        !state
            .documents
            .iter()
            .any(|d| d.kind == DocumentKind::GameRoot),
        "the cleared folder's install is still open"
    );
    assert!(state.prefs.game_paths.get(Game::GtaVLegacy).is_none());
}

#[test]
fn a_game_switch_asked_while_another_close_waits_says_it_was_not_made() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let (mut state, _, _) = an_install_with_unsaved_work(dir.path());
    let other = state.new_empty_document(DocumentKind::Text);
    state.close_document(other);
    assert!(
        state.pending_close.is_some(),
        "closing the unsaved tab asks"
    );
    let said = state.toasts.iter().count();

    state.switch_active_game(Game::GtaVEnhanced);

    let told: Vec<&str> = state
        .toasts
        .iter()
        .skip(said)
        .map(|t| t.message.as_str())
        .collect();
    assert!(
        !told.is_empty(),
        "the switch was dropped and the user was told nothing"
    );
    assert_eq!(state.prefs.active_game, Some(Game::GtaVLegacy));
}

/// An `.ytd` whose RSC7 header names an Enhanced version, enough for the edition check.
fn an_enhanced_file(dir: &Path) -> PathBuf {
    let format = or_io::gta5::formats::format("ytd").expect("the texture dictionary format");
    let version = (0..=u8::MAX as u32)
        .find(|&v| format.target_of_version(v).ok() == Some(or_io::Generation::Gen9))
        .expect("an Enhanced version of .ytd");
    let mut header = b"RSC7".to_vec();
    header.extend(version.to_le_bytes());
    header.extend([0u8; 8]);
    let path = dir.join("enhanced.ytd");
    std::fs::write(&path, header).expect("enhanced.ytd");
    path
}

/// ⚠ Answered Cancel only: a switch that went ahead would remember an install
/// that exists in the test binary's shared preferences, and a parallel
/// `AppState::new` would mount it.
#[test]
fn an_enhanced_file_waits_for_the_switch_it_asked_for_and_is_not_opened_when_it_is_cancelled() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let (mut state, install, _) = an_install_with_unsaved_work(dir.path());
    let file = an_enhanced_file(dir.path());
    let is_open = |state: &AppState| {
        state
            .documents
            .iter()
            .any(|d| d.path.as_deref() == Some(&*file))
    };

    state.load_file(file.clone());
    assert!(
        state.pending_enhanced_open.is_some(),
        "the edition is not asked about"
    );
    state.apply_pending_enhanced_open(EnhancedOpenDecision::Switch);
    assert!(
        state.pending_close.is_some(),
        "the switch did not wait on the unsaved file"
    );
    for _ in 0..20 {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !is_open(&state) && state.load_receiver.is_none(),
        "the file opened before the switch it asked for was made"
    );
    assert!(
        state.pending_enhanced_open.is_none(),
        "the edition was asked about again while the switch waits"
    );

    state.answer_close(CloseAnswer::Cancel);
    for _ in 0..20 {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(state.document(install).is_some());
    assert!(
        !is_open(&state) && state.load_receiver.is_none() && state.pending_enhanced_open.is_none(),
        "a cancelled switch still opened the file or asked about it again"
    );
}

#[test]
fn the_installs_heading_names_an_install() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());
    let install = state.open_game_root(dir.path().to_path_buf());
    state.set_active_document(install);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut labels = Vec::new();
    for _ in 0..2 {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::panels::inspector::draw(ui, &mut state)
        });
        labels = output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        node.label().or_else(|| node.value()).map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    assert!(
        labels.iter().any(|l| l == "Game Install"),
        "no heading names the install: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.contains("Install Tree")),
        "{labels:?}"
    );
}
