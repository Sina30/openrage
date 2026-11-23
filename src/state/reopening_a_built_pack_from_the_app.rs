//! Reopening a built pack from the app: a pack file dropped or picked asks where the project is made, runs on a worker
//! that says how far it has got and can be stopped, and leaves a report; the pack is never written, so nothing is undone.
//!
//! ⚠ Built with `AppState::from_prefs` and packs a test builds itself: no install, no keys. Nobody can run GTA V here, so
//! this holds that what a build wrote reopens as sources, not that the game loads it.

use std::path::{Path, PathBuf};

use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{build_all_with, Container, Cook, Module, Output, OutputKind, Project};
use or_wardrobe::edit::Watch;

use super::importing_game_files_from_the_app::until;
use super::jobs::{JobKind, Outcome, Then};
use super::os_drop::{plan, DropAction, DropTarget};
use super::workbench::{destination, Question};
use super::{AppState, Prefs};

const HANDLING: &[u8] = b"<?xml version=\"1.0\"?>\n<CHandlingDataMgr/>\n";

/// The pack file a build of two jackets and a handling file wrote as `kind`: a `dlc.rpf`, or the `.oiv` package.
pub(crate) fn a_built_pack(root: &Path, kind: OutputKind) -> PathBuf {
    // ⚠ The project is a folder of its own: a build is refused into the project's folder or inside it.
    let dir = &root.join("source");
    let pack = dir.join("source_pack");
    or_wardrobe::edit::create(&pack, "demo").unwrap();
    let drop = dir.join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    let c = "mp_m_freemode_01_mp_m_demo";
    for n in 0..2 {
        let tag = format!("jacket{n}");
        let ydd = format!("{c}^jbib_00{n}_u.ydd");
        let ytd = format!("{c}^jbib_diff_00{n}_a_uni.ytd");
        std::fs::write(drop.join(ydd), or_source::sample::drawables_of(&tag)).unwrap();
        std::fs::write(drop.join(ytd), or_source::sample::textures_of(&tag)).unwrap();
    }
    or_wardrobe::edit::add_clothes(&pack, &[drop]).unwrap();
    // A file a clothing pack does not read, registered under a type, which a reopened project keeps under `files/`.
    let extension = if kind == OutputKind::OivPackage {
        "oiv"
    } else {
        "rpf"
    };
    let handling = dir.join("data/handling.meta");
    std::fs::create_dir_all(handling.parent().unwrap()).unwrap();
    std::fs::write(&handling, HANDLING).unwrap();
    let mut files = Module::files("files");
    files
        .set_file_type("data/handling.meta", Some("HANDLING_FILE"))
        .unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "source_pack"), files],
        outputs: vec![Output::of(kind, "out", "demo", &["wardrobe", "files"])],
    };
    let file = dir.join("demo.openrage-proj");
    Project::create(&file, &container, &[handling]).unwrap();
    let plans = or_wardrobe::plans(&Project::load(&file).unwrap(), "out").unwrap();
    let out = root.join("built");
    let cook = |source: &Path, scratch: &Path| or_source::cook::cook_source(source, scratch);
    let (built, stopped) =
        build_all_with(&plans, &out, &ParamRemap::default(), Some(&cook as &Cook));
    assert!(stopped.is_none(), "{stopped:?}");
    std::fs::read_dir(&built[0].folder)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == extension))
        .unwrap_or_else(|| panic!("no .{extension} in {}", built[0].folder.display()))
}

fn asked_about(state: &AppState) -> Option<&Question> {
    state
        .workbench
        .asking
        .as_ref()
        .filter(|q| matches!(q, Question::Reopen { .. }))
}

/// The question with its folder changed to `place`, as typing in the field does.
fn placed_in(state: &AppState, place: &Path) -> Question {
    let Some(Question::Reopen {
        built,
        name,
        pack,
        error,
        focus,
        ..
    }) = asked_about(state).cloned()
    else {
        panic!(
            "no question about reopening is open: {:?}",
            state.workbench.asking
        );
    };
    Question::Reopen {
        built,
        place: place.display().to_string(),
        name,
        pack,
        error,
        focus,
    }
}

fn finished(state: &AppState) -> bool {
    !state.workbench.reopen.is_running()
}

fn a_state() -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state
}

#[test]
fn a_built_pack_asks_to_be_reopened_where_a_drop_puts_files_in_the_project_and_an_rpf_elsewhere_still_opens(
) {
    let oiv = PathBuf::from("pack.OIV");
    let grzy = PathBuf::from("pack.gctproject");
    let dlc = PathBuf::from("dlc.rpf");
    let openable = |p: &Path| p.extension().is_some_and(|e| e == "rpf");

    for target in [
        DropTarget::Nowhere,
        DropTarget::Project,
        DropTarget::Wardrobe,
    ] {
        let planned = plan(&[oiv.clone(), grzy.clone()], target, None, openable);
        assert_eq!(
            planned,
            [
                DropAction::ReopenBuilt(oiv.clone()),
                DropAction::ReopenBuilt(grzy.clone())
            ],
            "{target:?}"
        );
    }
    // ⚠ Aimed at an archive's folder everything imports, and an image dropped on a tile is that tile's.
    let (just_oiv, just_dlc) = ([oiv.clone()], [dlc.clone()]);
    let into = plan(
        &just_oiv,
        DropTarget::Folder { doc: 1, folder: 2 },
        None,
        openable,
    );
    assert!(matches!(into[..], [DropAction::Import { .. }]), "{into:?}");

    // An archive asks only where a drop already means "put this in the project"; anywhere else it opens, as it does today.
    assert_eq!(
        plan(&just_dlc, DropTarget::Project, None, openable),
        [DropAction::ReopenBuilt(dlc.clone())]
    );
    assert_eq!(
        plan(&just_dlc, DropTarget::Nowhere, None, openable),
        [DropAction::Open(dlc.clone())]
    );
    assert_eq!(
        plan(&just_dlc, DropTarget::Wardrobe, None, openable),
        [DropAction::AddClothes(vec![dlc])]
    );

    // ⚠ Every singleplayer DLC is called `dlc.rpf` whatever the case: any other archive is not one, and is never asked about.
    let shouting = PathBuf::from("D:/packs/DLC.RPF");
    assert_eq!(
        plan(
            std::slice::from_ref(&shouting),
            DropTarget::Project,
            None,
            openable
        ),
        [DropAction::ReopenBuilt(shouting)]
    );
    let other = PathBuf::from("x64a.rpf");
    assert_eq!(
        plan(
            std::slice::from_ref(&other),
            DropTarget::Project,
            None,
            openable
        ),
        [DropAction::ImportIntoProject(vec![other.clone()])]
    );
    assert_eq!(
        plan(
            std::slice::from_ref(&other),
            DropTarget::Nowhere,
            None,
            openable
        ),
        [DropAction::Open(other)]
    );
}

#[test]
fn several_packs_dropped_together_are_asked_about_one_after_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let packs: Vec<PathBuf> = ["a.oiv", "b.oiv", "c.gctproject"]
        .iter()
        .map(|name| dir.path().join(name))
        .collect();
    let mut state = a_state();
    state.handle_os_drop(packs.clone(), DropTarget::Nowhere);

    let asked = |state: &AppState| match asked_about(state) {
        Some(Question::Reopen { built, .. }) => Some(built.clone()),
        _ => None,
    };
    assert_eq!(asked(&state), Some(packs[0].clone()), "the first is asked");
    state.poll_workbench();
    assert_eq!(
        asked(&state),
        Some(packs[0].clone()),
        "and stays while it is on screen"
    );

    // Cancel, and the next one comes.
    state.workbench.asking = None;
    state.poll_workbench();
    assert_eq!(asked(&state), Some(packs[1].clone()));

    // Answer it: the last waits for the worker, then is asked.
    let watch = Watch::default();
    watch.cancel.set();
    let answer = placed_in(&state, dir.path());
    state.begin_reopen(answer, dir.path().join("stopped"), watch);
    until(&mut state, "the stop", finished);
    state.poll_workbench();
    assert_eq!(asked(&state), Some(packs[2].clone()));

    state.workbench.asking = None;
    state.poll_workbench();
    assert!(
        state.workbench.asking.is_none(),
        "nothing more waits: {:?}",
        state.workbench.asking
    );
}

#[test]
fn a_folder_for_the_project_must_be_a_name_in_a_folder_that_is_there_and_holds_nothing_of_that_name(
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("taken")).unwrap();
    std::fs::write(dir.path().join("taken/readme.txt"), b"x").unwrap();
    std::fs::create_dir_all(dir.path().join("empty")).unwrap();
    let place = dir.path().display().to_string();

    assert_eq!(destination(&place, " fresh "), Ok(dir.path().join("fresh")));
    assert_eq!(destination(&place, "empty"), Ok(dir.path().join("empty")));
    for (place, name, says) in [
        (place.as_str(), "taken", "already holds something"),
        (place.as_str(), "", "cannot be a folder name"),
        (place.as_str(), "a/b", "cannot be a folder name"),
        (place.as_str(), ".hidden", "cannot be a folder name"),
        ("", "fresh", "Say which folder"),
        ("D:/no/such/folder/anywhere", "fresh", "is not a folder"),
    ] {
        let why = destination(place, name).expect_err(name);
        assert!(why.contains(says), "{name:?} in {place:?}: {why}");
    }
}

#[test]
fn a_dropped_oiv_asks_then_reopens_as_a_project_on_a_worker_and_the_pack_is_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let oiv = a_built_pack(dir.path(), OutputKind::OivPackage);
    let before = std::fs::read(&oiv).unwrap();
    let mut state = a_state();

    state.handle_os_drop(vec![oiv.clone()], DropTarget::Nowhere);
    let Some(Question::Reopen { built, name, .. }) = asked_about(&state).cloned() else {
        panic!("the drop asks: {:?}", state.workbench.asking);
    };
    assert_eq!(built, oiv);
    assert_eq!(
        name,
        oiv.file_stem().unwrap().to_string_lossy(),
        "the project folder is offered under the pack file's name"
    );
    assert!(
        !state.workbench.reopen.is_running(),
        "nothing is read before the answer"
    );

    let into = dir.path().join("projects");
    std::fs::create_dir_all(&into).unwrap();
    state.answer_reopen(placed_in(&state, &into));
    assert!(
        state.workbench.reopen.is_running(),
        "the answer starts a worker"
    );
    assert!(
        state.workbench.asking.is_none(),
        "and the question is closed"
    );
    let job = state.running_jobs();
    assert!(
        job.iter().any(|j| j.kind == JobKind::ProjectReopen),
        "the job list shows it, with its Stop: {job:?}"
    );

    until(&mut state, "the reopening", finished);
    let report = state.workbench.reopen.report().expect("a report").clone();
    assert!(state.workbench.reopen.shown, "the report opens");
    assert_eq!(report.made.garments, 2);
    assert!(report.made.project.is_file(), "{:?}", report.made.project);
    assert!(report.into.starts_with(&into), "{:?}", report.into);

    // What could not be placed is named, and the file the clothing cannot read is kept under files/.
    let kept = (report.made.unplaced.iter())
        .find(|left| left.path.ends_with("handling.meta"))
        .unwrap_or_else(|| panic!("handling.meta is not listed: {:#?}", report.made.unplaced));
    assert!(kept.why.contains("files/"), "{}", kept.why);
    assert!(report.into.join("files").is_dir());

    assert_eq!(
        std::fs::read(&oiv).unwrap(),
        before,
        "the pack was not written"
    );
    let ended = state.jobs.ended();
    assert_eq!(ended[0].kind, JobKind::ProjectReopen);
    assert!(matches!(
        ended[0].outcome,
        Outcome::Done(Then::OpenFolder(_))
    ));

    // Open project opens what it made.
    state.open_reopened_project();
    assert_eq!(
        state.project_path.as_deref(),
        Some(report.made.project.as_path())
    );
    assert!(!state.workbench.reopen.shown, "and the report is put away");
}

#[test]
fn a_pack_the_reader_refuses_brings_the_question_back_with_the_reason_and_makes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_project = dir.path().join("notes.gctproject");
    std::fs::write(&not_a_project, b"these bytes are no zip").unwrap();
    let mut state = a_state();

    state.handle_os_drop(vec![not_a_project], DropTarget::Nowhere);
    let into = dir.path().join("projects");
    std::fs::create_dir_all(&into).unwrap();
    state.answer_reopen(placed_in(&state, &into));
    until(&mut state, "the refusal", finished);

    let Some(Question::Reopen { error, name, .. }) = asked_about(&state).cloned() else {
        panic!("the question comes back: {:?}", state.workbench.asking);
    };
    let why = error.expect("the reason is on it");
    assert!(why.contains("is not a grzyClothTool project"), "{why}");
    assert!(
        state.workbench.reopen.report().is_none() && !state.workbench.reopen.shown,
        "a refusal reports nothing"
    );
    assert!(!into.join(name).exists(), "and leaves no folder");
    assert_eq!(state.jobs.ended()[0].outcome, Outcome::Failed);
}

#[test]
fn a_folder_that_holds_something_is_refused_in_the_question_before_a_worker_starts() {
    let dir = tempfile::tempdir().unwrap();
    let oiv = a_built_pack(dir.path(), OutputKind::OivPackage);
    let mut state = a_state();
    state.ask_to_reopen(oiv);
    let taken = dir.path().join("taken");
    std::fs::create_dir_all(taken.join("demo")).unwrap();
    std::fs::write(taken.join("demo/readme.txt"), b"x").unwrap();
    let Some(Question::Reopen {
        built, pack, focus, ..
    }) = asked_about(&state).cloned()
    else {
        panic!("{:?}", state.workbench.asking);
    };
    let asked = Question::Reopen {
        built,
        place: taken.display().to_string(),
        name: "demo".into(),
        pack,
        error: None,
        focus,
    };

    state.answer_reopen(asked);

    assert!(!state.workbench.reopen.is_running());
    let Some(Question::Reopen { error, .. }) = asked_about(&state).cloned() else {
        panic!("the question stays open");
    };
    assert!(error.is_some_and(|why| why.contains("already holds something")));
}

#[test]
fn a_stop_asked_for_makes_nothing_and_says_so_and_a_pack_waits_while_another_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let oiv = a_built_pack(dir.path(), OutputKind::OivPackage);
    let before = std::fs::read(&oiv).unwrap();
    let mut state = a_state();
    state.ask_to_reopen(oiv.clone());
    let asked = placed_in(&state, dir.path());
    let into = dir.path().join("stopped");

    let watch = Watch::default();
    watch.cancel.set();
    state.begin_reopen(asked, into.clone(), watch);

    // While it is read, another pack is not asked about and the command waits.
    assert!(state.reopen_refused().is_some());
    state.ask_to_reopen(oiv.clone());
    assert!(
        asked_about(&state).is_none(),
        "the second pack is not asked about"
    );

    until(&mut state, "the stop", finished);
    assert!(!into.exists(), "a stop leaves no project folder");
    assert!(state.workbench.reopen.report().is_none());
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("nothing was made")),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    assert_eq!(
        state.jobs.ended()[0].outcome,
        Outcome::Stopped(Then::Nothing)
    );
    assert_eq!(std::fs::read(&oiv).unwrap(), before);
    assert!(
        state.reopen_refused().is_none(),
        "the next pack can be asked about"
    );
}

#[test]
fn stop_in_the_job_list_asks_the_worker_to_stop() {
    let dir = tempfile::tempdir().unwrap();
    let oiv = a_built_pack(dir.path(), OutputKind::OivPackage);
    let mut state = a_state();
    state.ask_to_reopen(oiv);
    let asked = placed_in(&state, dir.path());
    state.begin_reopen(asked, dir.path().join("made"), Watch::default());

    let before = state.project_reopen_job().expect("a job");
    assert!(!before.stopping);
    state.cancel_job(JobKind::ProjectReopen);
    assert!(state.project_reopen_job().is_some_and(|job| job.stopping));
    until(&mut state, "the worker to end", finished);
}
