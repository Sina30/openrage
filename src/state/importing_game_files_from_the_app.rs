//! Importing from the app: Import game files, Import into project and Add images as a textures source
//! turn files into sources of the open project, report what became of each, and take it back.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a
//! worker for each import, scan and cook that a case waits on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PgDictionary;
use or_project::{Container, Output, Project};
use or_source::asset::{folder_name, AssetManifest, How};
use or_source::game_import::Verdict;
use or_source::textures;
use or_title::Game;

use super::every_add_lands_where_the_tree_shows_it::opened as open_folder;
use super::jobs::JobKind;
use super::nested_archives_survive_a_save::archive;
use super::opening_a_ydd::{finish_load, node_at};
use super::toasts::ToastAction;
use super::workbench::{AssetRow, AssetState, Kept, NEEDS_A_PROJECT};
use super::{AppState, DocumentKind, Prefs};
use crate::platform::file_dialogs;
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

pub(super) fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A Legacy texture dictionary of one block-compressed texture, whose pixels `shade` decides.
pub(super) fn ytd(name: &str, shade: u8) -> Vec<u8> {
    let format = GrcTextureFormat::DXT1;
    let texture = GrcTexture {
        name: name.into(),
        width: 16,
        height: 16,
        depth: 1,
        mip_count: 1,
        mip_stride: u16::try_from(format.legacy_mip_stride(16)).unwrap(),
        format: format.to_raw_format(),
        backing_store: vec![shade; format.total_size(16, 16, 1) as usize],
        ..Default::default()
    };
    PgDictionary {
        codes: vec![or_io::string_hash(name.as_bytes())],
        entries: vec![Some(Box::new(texture))],
        ..Default::default()
    }
    .to_bytes()
    .unwrap()
}

/// An empty project in `dir/police_pack`, opened in a state, and the folder it is in.
pub(super) fn project(dir: &Path) -> (AppState, PathBuf) {
    let folder = dir.join("police_pack");
    std::fs::create_dir_all(&folder).unwrap();
    let mut container = Container::default();
    container
        .outputs
        .push(Output::fivem_resource("fivem", "police_pack", &["files"]));
    let file = folder.join("police_pack.openrage-proj");
    Project::create(&file, &container, &[]).expect("a new project");
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(file);
    until(&mut state, "the first scan", |s| s.workbench.is_scanned());
    (state, folder)
}

pub(super) fn until(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_workbench();
        if done(state) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Polls until the import that was started has ended and left its report.
pub(super) fn imported(state: &mut AppState) {
    until(state, "the import", |s| {
        !s.workbench.imports.is_running() && s.workbench.imports.report().is_some()
    });
}

pub(super) fn row<'a>(state: &'a AppState, name: &str) -> &'a AssetRow {
    state
        .workbench
        .rows()
        .iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no asset named {name}: {:?}", state.workbench.rows()))
}

pub(super) fn sources_of(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder.join("sources"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

pub(super) fn manifest_of(folder: &Path) -> AssetManifest {
    let text = std::fs::read_to_string(folder.join("asset.json")).expect("an asset.json");
    AssetManifest::parse(&text).expect("a manifest")
}

#[test]
fn importing_a_texture_dictionary_from_the_picker_makes_a_source_whose_row_opens_in_the_texture_workspace(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let picked = dir.path().join("downloads/road.ytd");
    write(&picked, &ytd("road", 3));
    let ctx = egui::Context::default();

    file_dialogs::answer_next(picked.clone());
    commands::run_action(&mut state, &ctx, Action::ImportGameFiles);

    assert!(
        (state.running_jobs().iter()).any(|job| job.kind == JobKind::ProjectImport),
        "the import is a job of the status bar while it runs"
    );
    imported(&mut state);
    let made = folder
        .join("sources")
        .join(folder_name("road", textures::KIND));
    let report = state.workbench.imports.report().expect("a report");
    assert!(
        matches!(&report.report.files[..], [one] if one.verdict == Verdict::Made { folder: made.clone() }),
        "{report:?}"
    );
    assert!(
        !state.workbench.imports.shown,
        "an import that worked is a toast with Undo import, and the report waits for Show report"
    );
    let manifest = manifest_of(&made);
    assert_eq!(
        manifest.origin.how,
        How::Imported,
        "a file picked from a download is not the game's"
    );
    assert!(!manifest.origin.from_game);
    assert!(
        folder.join(".gitignore").is_file(),
        "the first import writes the managed ignore block"
    );

    // The row appears with no refresh, and the import itself cooked it.
    until(&mut state, "the row", |s| {
        s.workbench
            .rows()
            .iter()
            .any(|r| r.name == "road" && r.kept == Kept::Source)
    });
    until(&mut state, "the cook", |s| {
        s.workbench.state_of(row(s, "road")) == AssetState::Cooked
    });
    let key = row(&state, "road").key.clone();
    state.open_asset(&key, None);
    assert_eq!(state.active_workspace, Workspace::Texture);
    finish_load(&mut state).expect("the cooked dictionary opens");
    let doc = state.active_document().expect("a document");
    assert_eq!(doc.kind, DocumentKind::Ytd);

    let imports: Vec<_> = (state.workbench.history().iter())
        .filter(|event| event.text.starts_with("Imported"))
        .collect();
    assert_eq!(imports.len(), 1, "one History line: {imports:?}");
}

#[test]
fn the_rows_menu_offers_import_into_project_and_open_still_opens_the_file_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    write(&dir.path().join("mods/road.ytd"), &ytd("road", 3));
    let doc = open_folder(&mut state, &dir.path().join("mods"));
    let road = node_at(&mut state, doc, "road.ytd");
    let ctx = egui::Context::default();
    let target = commands::Target::rows(&state, doc, vec![road], false).expect("a row");

    let import =
        commands::on(&state, Action::ImportIntoProject, target.clone()).expect("a command");
    assert_eq!(
        import.enabled,
        Ok(()),
        "a project is open and the row is a file"
    );
    assert_eq!(import.label, "Import into project");
    let open = commands::on(&state, Action::OpenSelection, target).expect("a command");
    commands::run(&mut state, &ctx, &open);
    finish_load(&mut state).expect("the file opens where it is");

    let opened = state.active_document().expect("a document");
    assert_eq!(opened.kind, DocumentKind::Ytd);
    assert!(
        !state.workbench.imports.is_running(),
        "Open imports nothing"
    );
    assert!(
        sources_of(&folder).is_empty(),
        "Open made no source: {:?}",
        sources_of(&folder)
    );
    assert!(
        !state.project_carried.changed,
        "Open does not change the project"
    );
}

#[test]
fn opening_a_folder_in_a_project_adds_no_member_and_no_problem_and_the_project_does_not_keep_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    write(&dir.path().join("mods/road.ytd"), &ytd("road", 3));
    let mods = dir.path().join("mods");

    let doc = open_folder(&mut state, &mods);
    state.workbench.want_scan();
    until(&mut state, "a scan after the folder opened", |s| {
        !s.workbench.rows().iter().any(|r| r.path == mods) && s.workbench.is_scanned()
    });

    assert!(
        !state.member_paths().contains(&mods),
        "an opened folder is no file of the project"
    );
    assert!(
        state.workbench.findings().is_empty(),
        "{:?}",
        state.workbench.findings()
    );
    assert!(!state.project_carried.changed);
    let saved = state.snapshot_project();
    assert!(
        !saved.documents.iter().any(|m| m.path == mods),
        "a save wrote the opened folder as a member"
    );
    state.close_document(doc);
    assert!(!state.project_carried.members.iter().any(|m| m.path == mods));
    assert!(sources_of(&folder).is_empty());
}

#[test]
fn a_lua_script_of_a_resource_opens_in_the_data_workspaces_text_editor_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let manifest = folder.join("files/fxmanifest.lua");
    write(
        &manifest,
        b"fx_version 'cerulean'
",
    );
    state.active_workspace = Workspace::Project;

    state.load_file(manifest.clone());
    finish_load(&mut state).expect("a script opens");

    assert_eq!(
        DocumentKind::from_path(&manifest),
        Some(DocumentKind::Text),
        "a .lua is a text kind, which the Assets table and a member's kind say"
    );
    let doc = state.active_document().expect("a document");
    assert_eq!(doc.kind, DocumentKind::Text, "a .lua is text, not bytes");
    assert_eq!(state.active_workspace, Workspace::Data);
    assert!(
        !state.project_carried.changed,
        "Open does not change the project"
    );
    assert!(sources_of(&folder).is_empty());
}

#[test]
fn every_import_command_is_in_the_palette_and_only_they_say_import() {
    let state = AppState::from_prefs(Prefs::default());
    let listed = commands::all(&state);
    for action in [
        Action::ImportGameFiles,
        Action::ImportFolder,
        Action::ImportIntoProject,
        Action::AddImagesAsTexturesSource,
        Action::UndoImport,
    ] {
        let spec = commands::spec(action).expect("a row of the table");
        assert!(
            listed.iter().any(|command| command.id == spec.id),
            "{action:?} is not in the command palette"
        );
    }
    for spec in commands::COMMANDS {
        let ours = matches!(
            spec.action,
            Action::ImportGameFiles
                | Action::ImportFolder
                | Action::ImportIntoProject
                | Action::OpenBuiltPack
        );
        assert_eq!(
            spec.label.contains("Import"),
            ours,
            "{:?} is labelled `{}`: Import means a game file, or a built pack, becoming sources on disk",
            spec.action,
            spec.label
        );
    }
}

#[test]
fn with_no_project_the_import_commands_are_disabled_with_their_reason_and_the_refusal_offers_a_new_project(
) {
    let mut state = AppState::from_prefs(Prefs::default());
    let ctx = egui::Context::default();
    for action in [Action::ImportGameFiles, Action::AddImagesAsTexturesSource] {
        let command = commands::of(&state, action).expect("a command");
        assert_eq!(command.enabled, Err(NEEDS_A_PROJECT), "{action:?}");
    }

    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("mods/lamp.ybn"), b"bn");
    state.open_folder(dir.path().join("mods"));
    let doc = state.active_document.expect("the folder opens");
    let lamp = node_at(&mut state, doc, "lamp.ybn");
    let target = commands::Target::rows(&state, doc, vec![lamp], false).expect("a row");
    let command = commands::on(&state, Action::ImportIntoProject, target).expect("a command");
    assert_eq!(command.enabled, Err(NEEDS_A_PROJECT));

    commands::run_action(&mut state, &ctx, Action::ImportGameFiles);
    let offered = state
        .toasts
        .iter()
        .find(|toast| toast.message == NEEDS_A_PROJECT)
        .expect("the refusal is said");
    assert_eq!(offered.action, Some(ToastAction::NewProject));
    state.run_toast_action(ToastAction::NewProject);
    assert!(
        state.front_door.new_project.is_some(),
        "the offer opens the New project dialog"
    );
}

#[test]
fn an_archive_entry_is_imported_with_its_archive_path_and_the_archive_is_not_touched() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let pack = archive(&[("a.ytd", &ytd("road", 3))]);
    let at = dir.path().join("mods/pack.rpf");
    write(&at, &pack);
    let doc = open_folder(&mut state, &dir.path().join("mods"));
    let entry = node_at(&mut state, doc, "pack.rpf/a.ytd");

    state.import_rows(doc, &[entry]);
    imported(&mut state);

    let made = folder
        .join("sources")
        .join(folder_name("a", textures::KIND));
    assert!(
        made.join("asset.json").is_file(),
        "{:?}",
        state.workbench.imports.report()
    );
    assert_eq!(
        std::fs::read(&at).unwrap(),
        pack,
        "the archive was written to"
    );
    let manifest = manifest_of(&made);
    assert_eq!(
        manifest.origin.virtual_path.as_deref(),
        Some("pack.rpf/a.ytd")
    );
    assert_eq!(manifest.origin.how, How::Imported);
    assert!(
        !manifest.origin.from_game && manifest.origin.game_build.is_none(),
        "an archive outside the game is not the game's: {:?}",
        manifest.origin
    );
}

/// A project, and an install mounted as Legacy at `dir/GTAV` that holds `x64/pack.rpf` of `a.ytd`.
pub(super) fn project_and_install(dir: &Path) -> (AppState, PathBuf, PathBuf) {
    let (mut state, folder) = project(dir);
    let install = dir.join("GTAV");
    write(
        &install.join("x64/pack.rpf"),
        &archive(&[("a.ytd", &ytd("road", 3))]),
    );
    state
        .prefs
        .game_paths
        .set(Game::GtaVLegacy, Some(install.clone()));
    state.prefs.active_game = Some(Game::GtaVLegacy);
    (state, folder, install)
}

/// What the import of `source_name` left in `folder`, which must say the game's content is the game's.
fn assert_extracted_from_the_game(folder: &Path, source_name: &str, why: &str) {
    let kind = folder_name(source_name, textures::KIND);
    let manifest = manifest_of(&folder.join("sources").join(&kind));
    assert!(manifest.origin.from_game, "{why}: {:?}", manifest.origin);
    assert_eq!(manifest.origin.how, How::Extracted, "{why}");
    assert_eq!(
        manifest.origin.game_build.as_deref(),
        Some(Game::GtaVLegacy.label()),
        "{why}: the build of the mounted install"
    );
    let ignore = std::fs::read_to_string(folder.join(".gitignore")).expect("a managed ignore");
    assert!(
        ignore.contains(&format!("/sources/{kind}/")),
        "{why}: game content stays out of the repository:\n{ignore}"
    );
}

#[test]
fn an_entry_of_an_archive_in_the_installed_game_is_the_games_however_far_above_it_the_folder_was_opened(
) {
    for (root, entry) in [
        ("", "GTAV/x64/pack.rpf/a.ytd"),
        ("GTAV", "x64/pack.rpf/a.ytd"),
        ("GTAV/x64", "pack.rpf/a.ytd"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, folder, _) = project_and_install(dir.path());
        let opened = match root {
            "" => dir.path().to_path_buf(),
            below => dir.path().join(below),
        };
        let doc = open_folder(&mut state, &opened);
        let entry = node_at(&mut state, doc, entry);

        state.import_rows(doc, &[entry]);
        imported(&mut state);

        assert_extracted_from_the_game(&folder, "a", &format!("a folder opened at `{root}`"));
    }
}

#[test]
fn a_file_picked_in_the_installed_game_is_the_games_and_one_picked_elsewhere_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder, install) = project_and_install(dir.path());
    write(&install.join("x64/road.ytd"), &ytd("road", 3));
    write(&dir.path().join("downloads/lamp.ytd"), &ytd("lamp", 9));

    state.import_game_files(vec![
        install.join("x64/road.ytd"),
        dir.path().join("downloads/lamp.ytd"),
    ]);
    imported(&mut state);

    assert_extracted_from_the_game(&folder, "road", "picked in the install");
    let lamp = manifest_of(
        &folder
            .join("sources")
            .join(folder_name("lamp", textures::KIND)),
    );
    assert!(!lamp.origin.from_game && lamp.origin.game_build.is_none());
    assert_eq!(lamp.origin.how, How::Imported);
}

#[test]
fn a_picked_archive_is_refused_with_the_way_to_import_its_files_and_nothing_is_copied() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let pack = dir.path().join("downloads/Pack.RPF");
    write(&pack, &archive(&[("a.ytd", &ytd("road", 3))]));
    write(&dir.path().join("downloads/lamp.ytd"), &ytd("lamp", 9));

    state.import_game_files(vec![dir.path().join("downloads/lamp.ytd"), pack]);

    assert!(
        !state.workbench.imports.is_running() && state.workbench.imports.report().is_none(),
        "nothing was started, not even for the file beside the archive"
    );
    assert!(
        (state.toasts.iter()).any(|t| t.message.contains("Import into project")),
        "the refusal says how to import what is in it: {:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    assert!(!folder.join("files").exists() && sources_of(&folder).is_empty());
    assert!(!state.project_carried.changed);
}

#[test]
fn import_into_project_says_before_it_runs_that_a_folder_walk_leaves_an_archive_out() {
    let hover = commands::spec(Action::ImportIntoProject)
        .expect("a row of the table")
        .hover;
    assert!(
        hover.contains("folder")
            && hover.contains("archive")
            && hover.contains("left out")
            && !hover.contains("whole"),
        "{hover}"
    );
}

#[test]
fn an_archive_and_a_folder_inside_one_are_imported_by_choosing_the_files_in_them() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = project(dir.path());
    write(
        &dir.path().join("mods/pack.rpf"),
        &archive(&[("props/a.ytd", &ytd("road", 3))]),
    );
    let doc = open_folder(&mut state, &dir.path().join("mods"));
    let pack = node_at(&mut state, doc, "pack.rpf");
    let props = node_at(&mut state, doc, "pack.rpf/props");

    for row in [pack, props] {
        let why = state.rows_cannot_be_imported(doc, &[row]);
        assert!(
            why.is_some_and(|why| why.contains("selecting the files")),
            "{why:?}"
        );
    }
    state.import_rows(doc, &[pack]);
    assert!(!state.workbench.imports.is_running(), "nothing was started");
}

#[test]
fn a_folder_of_the_installed_game_waits_for_its_files_to_be_chosen_and_any_other_folder_is_walked()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let install = dir.path().join("GTAV");
    write(&install.join("x64/models.rpf"), b"an archive of gigabytes");
    write(&install.join("mods/lamp.ybn"), b"collision");
    write(&dir.path().join("pack/road.ytd"), &ytd("road", 3));
    state
        .prefs
        .game_paths
        .set(or_title::Game::GtaVLegacy, Some(install.clone()));

    let doc = open_folder(&mut state, dir.path());
    let (x64, mods, pack) = (
        node_at(&mut state, doc, "GTAV/x64"),
        node_at(&mut state, doc, "GTAV/mods"),
        node_at(&mut state, doc, "pack"),
    );

    let why = state.rows_cannot_be_imported(doc, &[x64]);
    assert!(
        why.is_some_and(|why| why.contains("selecting the files")),
        "{why:?}"
    );
    assert_eq!(
        state.rows_cannot_be_imported(doc, &[mods]),
        None,
        "the overlay is not the game as shipped"
    );
    assert_eq!(state.rows_cannot_be_imported(doc, &[pack]), None);

    state.import_rows(doc, &[pack]);
    imported(&mut state);
    let made = folder
        .join("sources")
        .join(folder_name("road", textures::KIND));
    assert!(made.is_dir(), "{:?}", state.workbench.imports.report());
    assert!(!folder.join("files/models.rpf").exists());
}

#[test]
fn a_compiled_file_with_no_source_form_is_kept_and_joins_the_files_content() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let picked = dir.path().join("downloads/lamp.ybn");
    write(&picked, b"collision");

    state.import_game_files(vec![picked.clone()]);
    imported(&mut state);

    let kept = folder.join("files/lamp.ybn");
    assert_eq!(std::fs::read(&kept).unwrap(), b"collision");
    assert!(
        state.member_paths().contains(&kept),
        "the kept file is a member: {:?}",
        state.member_paths()
    );
    assert!(
        (state.project_carried.container.modules.iter())
            .any(|m| m.kind == or_project::ModuleKind::Files),
        "the Files content holds it"
    );
    assert!(
        state.project_carried.changed,
        "the project file has a member to write"
    );
    let report = state.workbench.imports.report().expect("a report");
    assert!(matches!(
        &report.report.files[0].verdict,
        Verdict::Kept { copied_to: Some(to) } if *to == kept
    ));
    assert_eq!(
        std::fs::read(&picked).unwrap(),
        b"collision",
        "what was picked is as it was"
    );
    until(&mut state, "the row", |s| {
        s.workbench.rows().iter().any(|r| r.name == "lamp.ybn")
    });
}

#[test]
fn importing_the_same_file_again_says_it_is_already_a_source_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let picked = dir.path().join("downloads/road.ytd");
    write(&picked, &ytd("road", 3));

    state.import_game_files(vec![picked.clone()]);
    imported(&mut state);
    let first = sources_of(&folder);
    state.import_game_files(vec![picked]);
    imported(&mut state);

    let report = state.workbench.imports.report().expect("a report");
    assert!(
        matches!(&report.report.files[..], [one] if matches!(one.verdict, Verdict::AlreadyASource { .. })),
        "{report:?}"
    );
    assert_eq!(sources_of(&folder), first);
    assert!(report.report.created.is_empty());
}

#[test]
fn undo_import_removes_exactly_the_folders_it_made_and_refuses_while_one_of_them_was_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    // A source that was there before, which no undo may touch.
    let mine = folder.join("sources/mine.textures.src");
    write(&mine.join("asset.json"), b"{}");
    let (a, b) = (
        dir.path().join("downloads/a.ytd"),
        dir.path().join("downloads/b.ytd"),
    );
    write(&a, &ytd("a", 3));
    write(&b, &ytd("b", 9));
    state.import_game_files(vec![a, b]);
    imported(&mut state);
    let (made_a, made_b) = (
        folder
            .join("sources")
            .join(folder_name("a", textures::KIND)),
        folder
            .join("sources")
            .join(folder_name("b", textures::KIND)),
    );
    assert!(made_a.is_dir() && made_b.is_dir());

    // Somebody edits one of them.
    let notes = made_b.join("notes.txt");
    write(&notes, b"mine");
    state.undo_import();
    let shown = state.workbench.imports.report().expect("a report");
    let Some(Err(why)) = &shown.undone else {
        panic!("an edited source was taken back: {:?}", shown.undone);
    };
    assert!(
        why.contains(folder_name("b", textures::KIND).as_str()),
        "{why}"
    );
    assert!(made_a.is_dir() && made_b.is_dir(), "nothing was removed");

    // As the import left them, they go, and only they.
    std::fs::remove_file(&notes).unwrap();
    state.undo_import();
    let shown = state.workbench.imports.report().expect("a report");
    assert!(matches!(shown.undone, Some(Ok(_))), "{:?}", shown.undone);
    assert!(!made_a.exists() && !made_b.exists());
    assert!(mine.is_dir(), "a source the import did not make stays");
    assert!(
        (state.workbench.history().iter()).any(|e| e.text.starts_with("Took back the import")),
        "{:?}",
        state.workbench.history()
    );
}

#[test]
fn undo_import_is_a_command_that_waits_for_an_import_and_works_with_the_report_shut() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let ctx = egui::Context::default();
    let command = |state: &AppState| commands::of(state, Action::UndoImport).expect("a command");
    assert_eq!(
        command(&state).enabled,
        Err("No import has been made to take back")
    );
    let picked = dir.path().join("downloads/road.ytd");
    write(&picked, &ytd("road", 3));
    state.import_game_files(vec![picked]);
    imported(&mut state);
    state.workbench.imports.shown = false;
    let made = folder
        .join("sources")
        .join(folder_name("road", textures::KIND));
    assert!(made.is_dir());
    assert_eq!(command(&state).enabled, Ok(()));

    commands::run_action(&mut state, &ctx, Action::UndoImport);

    assert!(!made.exists(), "the command took the import back");
    assert!(
        (state.toasts.iter()).any(|toast| toast.message.starts_with("Took back the import")),
        "a toast says so, since the report window was shut"
    );
    assert_eq!(
        command(&state).enabled,
        Err("No import has been made to take back"),
        "an import is taken back once"
    );
}

#[test]
fn undoing_an_import_that_kept_a_file_takes_it_out_of_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let picked = dir.path().join("downloads/lamp.ybn");
    write(&picked, b"collision");
    state.import_game_files(vec![picked]);
    imported(&mut state);
    assert!(state
        .member_paths()
        .contains(&folder.join("files/lamp.ybn")));

    state.undo_import();

    assert!(!folder.join("files/lamp.ybn").exists());
    assert!(!state
        .member_paths()
        .contains(&folder.join("files/lamp.ybn")));
}

#[test]
fn images_added_as_a_textures_source_are_authored_and_cook() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let art = dir.path().join("art");
    std::fs::create_dir_all(&art).unwrap();
    for (name, shade) in [("logo.png", 40), ("badge.png", 90)] {
        image::RgbaImage::from_pixel(8, 8, image::Rgba([shade, 0, 0, 255]))
            .save(art.join(name))
            .unwrap();
    }
    write(&art.join("notes.txt"), b"not an image");

    state.add_images_as_textures_source(vec![
        art.join("logo.png"),
        art.join("badge.png"),
        art.join("notes.txt"),
    ]);
    imported(&mut state);

    let made = folder.join("sources/art.textures.src");
    let manifest = manifest_of(&made);
    assert_eq!(manifest.origin.how, How::Authored);
    assert!(!manifest.origin.from_game);
    assert!(made.join("textures/logo.png").is_file() && made.join("textures/badge.png").is_file());
    let report = state.workbench.imports.report().expect("a report");
    assert!(
        (report.report.files.iter())
            .any(|f| f.verdict == Verdict::Refused && f.input.ends_with("notes.txt")),
        "a file that is no image is refused and said: {report:?}"
    );
    until(&mut state, "the cook", |s| {
        s.workbench.rows().iter().any(|r| r.name == "art")
            && s.workbench.state_of(row(s, "art")) == AssetState::Cooked
    });
    assert!(row(&state, "art")
        .output
        .as_ref()
        .is_some_and(|o| o.is_file()));
}

#[test]
fn a_corpus_texture_dictionary_and_drawable_import_cook_and_open_in_their_workspaces() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    // ⚠ The smallest of each: the first by name is a 10 MB terrain dictionary whose import and cook take seconds of CPU, which a loaded machine stretches past the 30 s each wait allows.
    let smallest = |extension| {
        (or_test_support::Corpus::new(manifest, extension)
            .find()
            .into_iter())
        .min_by_key(|path| std::fs::metadata(path).map_or(u64::MAX, |m| m.len()))
    };
    let (ytd, ydr) = (smallest("ytd"), smallest("ydr"));
    let (Some(ytd), Some(ydr)) = (ytd, ydr) else {
        or_test_support::skip_corpus(
            "a_corpus_texture_dictionary_and_drawable_import_cook_and_open_in_their_workspaces",
            "no Legacy .ytd or .ydr fixture",
        );
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());

    state.import_game_files(vec![ytd.clone(), ydr.clone()]);
    imported(&mut state);

    let report = state.workbench.imports.report().expect("a report");
    assert!(
        (report.report.files.iter()).all(|f| matches!(f.verdict, Verdict::Made { .. })),
        "{report:?}"
    );
    for (path, kind, workspace, extension) in [
        (&ytd, "Textures source", Workspace::Texture, "ytd"),
        (&ydr, "Drawable source", Workspace::Model, "ydr"),
    ] {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        until(&mut state, "the row", |s| {
            s.workbench.rows().iter().any(|r| r.name == name)
        });
        until(&mut state, "the cook", |s| {
            matches!(
                s.workbench.state_of(row(s, &name)),
                AssetState::Cooked | AssetState::Failed
            )
        });
        let asset = row(&state, &name).clone();
        assert_eq!(asset.kind, kind, "{asset:?}");
        assert_eq!(
            state.workbench.state_of(&asset),
            AssetState::Cooked,
            "{:?}",
            state.workbench.findings()
        );
        let output = asset.output.clone().expect("an output");
        assert_eq!(output.extension().and_then(|e| e.to_str()), Some(extension));
        assert!(output.is_file());
        state.open_asset(&asset.key, None);
        assert_eq!(state.active_workspace, workspace, "{name}");
        finish_load(&mut state).expect("the cooked file opens");
    }
    assert!(folder.join("sources").is_dir());
}
