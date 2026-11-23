//! Drops and bulk import: a folder dropped on the window opens or imports by where it lands, an import
//! of a folder reports its bounds and what it left out, and a drop of clothes asks before it imports them.
//!
//! ⚠ Built with `AppState::from_prefs` over scratch folders: no install, no keys, and a worker for each
//! import a case waits on. The job's own mechanics (progress, Cancel, a stopped report) are tested
//! beside the job, in `workbench/imports.rs`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_project::ModuleKind;
use or_source::asset::folder_name;
use or_source::game_import::Verdict;
use or_source::textures;
use or_title::Game;

use super::front_door::{NewProject, Offer, Template};
use super::importing_game_files_from_the_app::{
    imported, manifest_of, project, project_and_install, sources_of, until, write, ytd,
};
use super::jobs::JobKind;
use super::os_drop::DropTarget;
use super::toasts::ToastType;
use super::workbench::Question;
use super::{AppState, DocumentKind, Prefs};
use crate::platform::file_dialogs;
use crate::ui::commands::{self, Action};

const C: &str = "mp_m_freemode_01_mp_m_t";

/// A state with no project, and the folder `pack` of two scripts under `dir`.
fn a_window_and_a_folder(dir: &Path) -> (AppState, PathBuf) {
    let folder = dir.join("pack");
    write(&folder.join("client.lua"), b"print('hello')");
    write(&folder.join("fxmanifest.lua"), b"fx_version 'cerulean'");
    (AppState::from_prefs(Prefs::default()), folder)
}

/// Polls the loads a drop started until none is left.
fn loaded(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_load_result();
        if state.load_receiver.is_none() && state.pending_loads.is_empty() {
            return;
        }
        assert!(Instant::now() < deadline, "the load never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn folder_documents(state: &AppState, folder: &Path) -> usize {
    (state.documents.iter())
        .filter(|d| d.kind == DocumentKind::Folder && d.path.as_deref() == Some(folder))
        .count()
}

fn errors(state: &AppState) -> Vec<String> {
    (state.toasts.iter())
        .filter(|t| t.toast_type == ToastType::Error)
        .map(|t| t.message.clone())
        .collect()
}

#[test]
fn a_dropped_folder_opens_as_a_folder_with_the_offer_to_make_it_a_project_and_not_as_a_load_error()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_window_and_a_folder(dir.path());

    state.handle_os_drop(vec![folder.clone()], DropTarget::Nowhere);
    loaded(&mut state);

    assert_eq!(folder_documents(&state, &folder), 1);
    assert_eq!(
        errors(&state),
        Vec::<String>::new(),
        "a drop is no load error"
    );
    assert_eq!(state.adoption_offer(), Some(Offer::Make(folder)));
}

#[test]
fn a_dropped_folder_that_holds_a_project_opens_with_the_offer_to_open_that_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut draft = NewProject::new(Template::Files, dir.path().to_path_buf());
    draft.set_name("harbor".into());
    let file = draft.create().expect("a project").file;
    let folder = file.parent().unwrap().to_path_buf();
    let mut state = AppState::from_prefs(Prefs::default());

    state.handle_os_drop(vec![folder.clone()], DropTarget::Nowhere);
    loaded(&mut state);

    assert_eq!(folder_documents(&state, &folder), 1);
    assert_eq!(state.adoption_offer(), Some(Offer::Open(file)));
    assert_eq!(errors(&state), Vec::<String>::new());
}

#[test]
fn a_folder_dropped_on_the_projects_panels_is_walked_into_it_and_the_report_names_the_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let dropped = dir.path().join("downloads/props");
    write(&dropped.join("road.ytd"), &ytd("road", 3));
    write(&dropped.join("deep/lamp.ytd"), &ytd("lamp", 9));
    write(&dropped.join("deep/hull.ybn"), b"collision");

    state.handle_os_drop(vec![dropped], DropTarget::Project);
    imported(&mut state);

    assert_eq!(
        sources_of(&folder),
        [
            folder_name("lamp", textures::KIND),
            folder_name("road", textures::KIND)
        ]
    );
    assert!(folder.join("files/deep/hull.ybn").is_file());
    let shown = state.workbench.imports.report().expect("a report");
    assert_eq!(
        shown.report.bounds,
        Some(or_source::game_import::Options::default().bounds),
        "the report names how far the walk went"
    );
    assert!(state.workbench.imports.shown, "the report window opens");
}

#[test]
fn a_dropped_project_folder_asks_where_it_opens_and_is_never_imported_into_the_open_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    std::fs::create_dir_all(dir.path().join("elsewhere")).unwrap();
    let mut draft = NewProject::new(Template::Files, dir.path().join("elsewhere"));
    draft.set_name("harbor".into());
    let other = draft.create().expect("a project").file;
    let other_folder = other.parent().unwrap().to_path_buf();

    state.handle_os_drop(vec![other_folder.clone()], DropTarget::Project);
    loaded(&mut state);

    // ⚠ A project is open, so the project in the folder asks whether it opens here or in a new window, and is not a folder to look in.
    assert_eq!(state.front_door.switching.as_deref(), Some(other.as_path()));
    assert_eq!(folder_documents(&state, &other_folder), 0);
    assert!(
        !state.workbench.imports.is_running() && state.workbench.imports.report().is_none(),
        "nothing was imported"
    );
    assert!(sources_of(&folder).is_empty() && !folder.join("files").exists());
}

#[test]
fn an_archive_in_a_dropped_folder_is_left_out_and_listed_and_a_dropped_archive_asks_to_be_reopened()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let dropped = dir.path().join("downloads/mods");
    write(&dropped.join("road.ytd"), &ytd("road", 3));
    write(&dropped.join("x64/Models.rpf"), b"an archive of gigabytes");

    state.handle_os_drop(vec![dropped], DropTarget::Project);
    imported(&mut state);

    let shown = state.workbench.imports.report().expect("a report");
    assert!(!folder.join("files/x64/Models.rpf").exists());
    let left: Vec<&str> = (shown.report.left_out.iter())
        .map(|l| l.path.as_str())
        .collect();
    assert_eq!(left, ["x64/Models.rpf"], "{:?}", shown.report);
    assert!(
        shown.report.left_out[0].why.contains("archive"),
        "{:?}",
        shown.report
    );

    // An archive named in the drop itself cannot be walked, and an archive is not imported whole: on the Project workspace it asks to be reopened as a project instead, and the import commands refuse it with the way in.
    let pack = dir.path().join("downloads/dlc.rpf");
    write(&pack, b"an archive");
    let before = sources_of(&folder);
    state.handle_os_drop(vec![pack.clone()], DropTarget::Project);
    assert!(!state.workbench.imports.is_running());
    assert_eq!(sources_of(&folder), before);
    assert!(
        matches!(&state.workbench.asking, Some(Question::Reopen { built, .. }) if *built == pack),
        "the drop asks whether to reopen it: {:?}",
        state.workbench.asking
    );

    state.workbench.asking = None;
    state.import_game_files(vec![pack]);
    assert!(!state.workbench.imports.is_running());
    assert_eq!(sources_of(&folder), before);
    assert!(
        (state.toasts.iter()).any(|t| t.message.contains("Import into project")),
        "the refusal says how: {:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );

    // ⚠ Only a `dlc.rpf` is a singleplayer DLC: any other archive dropped on the project is refused as it was, and nothing is asked.
    let models = dir.path().join("downloads/x64a.rpf");
    write(&models, b"an archive");
    state.handle_os_drop(vec![models], DropTarget::Project);
    assert!(
        state.workbench.asking.is_none(),
        "no question: {:?}",
        state.workbench.asking
    );
    assert!(!state.workbench.imports.is_running());
    assert_eq!(sources_of(&folder), before);
}

#[test]
fn a_source_is_labelled_with_the_build_of_the_install_that_holds_the_file_and_not_the_mounted_one()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder, _legacy) = project_and_install(dir.path());
    let enhanced = dir.path().join("GTAV_Enhanced");
    write(&enhanced.join("x64/road.ytd"), &ytd("road", 3));
    state
        .prefs
        .game_paths
        .set(Game::GtaVEnhanced, Some(enhanced.clone()));
    assert_eq!(state.prefs.active_game, Some(Game::GtaVLegacy));

    state.import_game_files(vec![enhanced.join("x64/road.ytd")]);
    imported(&mut state);

    let manifest = manifest_of(
        &folder
            .join("sources")
            .join(folder_name("road", textures::KIND)),
    );
    assert!(manifest.origin.from_game);
    assert_eq!(
        manifest.origin.game_build.as_deref(),
        Some(Game::GtaVEnhanced.label()),
        "the install the file is in, with Legacy mounted"
    );
}

#[test]
fn import_folder_is_a_command_that_walks_the_folder_the_dialog_picks_and_waits_for_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let mut idle = AppState::from_prefs(Prefs::default());
    assert_eq!(
        commands::of(&idle, Action::ImportFolder).map(|c| c.enabled),
        Some(Err(super::workbench::NEEDS_A_PROJECT))
    );
    commands::run_action(&mut idle, &ctx, Action::ImportFolder);

    let (mut state, folder) = project(dir.path());
    let picked = dir.path().join("downloads/props");
    write(&picked.join("road.ytd"), &ytd("road", 3));
    write(&picked.join("sub/lamp.ytd"), &ytd("lamp", 9));
    assert_eq!(
        commands::of(&state, Action::ImportFolder).map(|c| c.label),
        Some("Import folder…".to_string())
    );

    file_dialogs::answer_next(picked);
    commands::run_action(&mut state, &ctx, Action::ImportFolder);
    assert!(
        (state.running_jobs().iter()).any(|job| job.kind == JobKind::ProjectImport),
        "the import is a job of the status bar"
    );
    imported(&mut state);

    assert_eq!(sources_of(&folder).len(), 2);
    let shown = state.workbench.imports.report().expect("a report");
    assert!(shown.report.bounds.is_some());
}

#[test]
fn an_import_cancelled_the_moment_it_starts_loses_no_file_whatever_it_had_reached() {
    const FILES: usize = 40;
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let dropped = dir.path().join("downloads/props");
    for n in 0..FILES {
        write(
            &dropped.join(format!("prop_{n:02}.ytd")),
            &ytd(&format!("prop_{n:02}"), n as u8 + 1),
        );
    }

    state.import_game_files(vec![dropped]);
    state.cancel_job(JobKind::ProjectImport);
    assert!(
        (state.running_jobs().iter()).any(|job| job.kind == JobKind::ProjectImport && job.stopping),
        "the list says it is stopping"
    );
    imported(&mut state);

    let shown = state.workbench.imports.report().expect("a report");
    let made = shown
        .report
        .files
        .iter()
        .filter(|f| matches!(f.verdict, Verdict::Made { .. }))
        .count();
    assert_eq!(
        shown.report.files.len() + shown.report.left_out.len(),
        FILES,
        "every file is made or listed: {:?}",
        shown.report.notes
    );
    assert_eq!(
        shown.report.cancelled,
        !shown.report.left_out.is_empty(),
        "it says it stopped exactly when it left files"
    );
    assert_eq!(sources_of(&folder).len(), made, "what was made stays");
    assert_eq!(shown.report.created.len(), made);
}

fn a_jacket_and_its_texture(dir: &Path) -> Vec<PathBuf> {
    let jacket = dir.join(format!("{C}^jbib_000_u.ydd"));
    let texture = dir.join(format!("{C}^jbib_diff_000_a_uni.ytd"));
    write(&jacket, &or_source::sample::drawables_of("a jacket"));
    write(&texture, &ytd("jbib_diff_000_a_uni", 5));
    vec![jacket, texture]
}

#[test]
fn clothes_dropped_on_a_project_without_clothing_ask_before_anything_is_imported() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let dropped = a_jacket_and_its_texture(&dir.path().join("downloads"));

    state.handle_os_drop(dropped.clone(), DropTarget::Project);

    assert!(
        matches!(&state.workbench.asking, Some(Question::DropClothes { paths }) if *paths == dropped),
        "{:?}",
        state.workbench.asking.is_some()
    );
    assert!(
        !state.workbench.imports.is_running() && state.workbench.imports.report().is_none(),
        "nothing happens until the question is answered"
    );
    assert!(!state.has_clothing() && sources_of(&folder).is_empty());
}

#[test]
fn making_clothing_for_dropped_clothes_gives_the_project_a_pack_with_the_garment_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = project(dir.path());
    let dropped = a_jacket_and_its_texture(&dir.path().join("downloads"));
    state.handle_os_drop(dropped, DropTarget::Project);

    state.answer_dropped_clothes(true);
    until(&mut state, "the garment", |s| {
        s.workbench.packs().first().is_some_and(|p| {
            p.read
                .as_ref()
                .is_ok_and(|read| read.wardrobe.garments.len() == 1)
        })
    });

    assert!(state.workbench.asking.is_none());
    assert!(
        (state.project_carried.container.modules.iter()).any(|m| m.kind == ModuleKind::Clothing)
    );
    assert!(!state.workbench.imports.is_running() && state.workbench.imports.report().is_none());
}

#[test]
fn importing_dropped_clothes_as_sources_makes_no_clothing_and_a_cancelled_question_does_neither() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let dropped = a_jacket_and_its_texture(&dir.path().join("downloads"));

    state.handle_os_drop(dropped.clone(), DropTarget::Project);
    state.dismiss_dropped_clothes();
    assert!(state.workbench.asking.is_none() && !state.workbench.imports.is_running());
    assert!(sources_of(&folder).is_empty() && !state.has_clothing());

    state.handle_os_drop(dropped, DropTarget::Project);
    state.answer_dropped_clothes(false);
    imported(&mut state);
    assert!(!state.has_clothing());
    assert!(
        (sources_of(&folder).iter()).any(|s| s.starts_with(&format!("{C}^jbib_diff"))),
        "the texture dictionary became a source: {:?}",
        sources_of(&folder)
    );
}

#[test]
fn a_drop_with_no_clothes_in_it_and_a_drop_on_a_project_that_has_clothing_ask_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = project(dir.path());
    let lamp = dir.path().join("downloads/lamp.ydr");
    write(&lamp, b"a drawable");
    state.handle_os_drop(vec![lamp], DropTarget::Project);
    assert!(state.workbench.asking.is_none(), "a .ydr is no clothes");
    imported(&mut state);

    state.add_clothing_content();
    let dropped = a_jacket_and_its_texture(&dir.path().join("other"));
    state.handle_os_drop(dropped, DropTarget::Project);
    assert!(state.workbench.asking.is_none(), "the project has clothing");
}
