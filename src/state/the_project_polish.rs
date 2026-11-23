//! What the project polish changes in the state: a missing file is a problem and no toast, a build that asks to be verified is built twice,
//! and a project follows a source folder that moved.

use super::the_project_workspace::{opened, row, until};
use super::workbench::{LastBuild, Severity};
use or_project::Reproducible;

#[test]
fn opening_a_project_that_names_a_missing_file_lists_it_in_problems_and_raises_no_toast() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    assert!(
        (state.toasts.iter()).all(|t| !t.message.contains("missing_hat")),
        "a toast names the file: {:?}",
        (state.toasts.iter())
            .map(|t| t.message.clone())
            .collect::<Vec<_>>()
    );
    until(&mut state, "the findings", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| f.text.contains("missing_hat.ydd"))
    });
    let found = (state.workbench.findings().iter())
        .find(|f| f.text.contains("missing_hat.ydd"))
        .expect("a finding");
    assert_eq!(found.severity, Severity::Error);
    assert_eq!(
        found.fixes.iter().map(|f| f.label()).collect::<Vec<_>>(),
        ["Locate…", "Remove from project"]
    );
}

/// A project of one script that builds, opened in a state, and its project file.
fn a_buildable_project(dir: &std::path::Path) -> (super::AppState, std::path::PathBuf) {
    let (mut state, file) = opened(dir);
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.remove_asset(&row(&state, "missing_hat.ydd").key.clone());
    until(&mut state, "the plan to clear", |s| {
        s.workbench.is_scanned() && !s.workbench.is_blocked("fivem")
    });
    (state, file)
}

fn build_and_wait(state: &mut super::AppState) -> crate::state::workbench::BuiltSummary {
    state.build_selected();
    until(state, "the build", |s| !s.workbench.builds().is_running());
    match state.workbench.builds().last("fivem") {
        Some(LastBuild::Built(done)) => done.clone(),
        other => panic!("the build stopped: {other:?}"),
    }
}

#[test]
fn a_build_is_built_a_second_time_only_when_it_asks_to_be_verified() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_buildable_project(dir.path());

    let plain = build_and_wait(&mut state);
    assert_eq!(plain.parts[0].reproducible, Reproducible::NotRun);

    state
        .edit_build("fivem", "Verify the build".into(), |o| {
            o.extra
                .insert("verify".to_string(), serde_json::Value::Bool(true));
        })
        .expect("the build is edited");
    let verified = build_and_wait(&mut state);
    assert!(
        matches!(verified.parts[0].reproducible, Reproducible::Yes { files } if files > 0),
        "{:?}",
        verified.parts[0].reproducible
    );

    // Off again, the next build does not run it.
    state
        .edit_build("fivem", "Stop verifying the build".into(), |o| {
            o.extra.remove("verify");
        })
        .expect("the build is edited");
    let again = build_and_wait(&mut state);
    assert_eq!(again.parts[0].reproducible, Reproducible::NotRun);
}

/// A template file of one clothing content `uniforms` and one build `police` that takes it, in `dir`.
fn a_template_file(dir: &std::path::Path) -> std::path::PathBuf {
    use or_project::template::{Template, EXTENSION};
    use or_project::{Container, Module, Output};
    let mut container = Container::default();
    container
        .modules
        .push(Module::clothing("uniforms", "uniforms"));
    container
        .outputs
        .push(Output::fivem_resource("police", "x", &["uniforms"]));
    let (template, _) = Template::from_project("Police pack", "", &container, None, None, None);
    let file = dir.join(format!("police.{EXTENSION}"));
    std::fs::write(&file, template.to_json()).unwrap();
    file
}

#[test]
fn a_template_applied_to_an_open_project_adds_what_it_lacks_as_one_step_that_undo_takes_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_buildable_project(dir.path());
    let template = a_template_file(dir.path());
    let before = state.project_carried.container.clone();

    let said = state
        .apply_template(&template)
        .expect("the template applies");

    assert!(
        said.contains("uniforms") && said.contains("police"),
        "{said}"
    );
    let container = &state.project_carried.container;
    assert!(container.module("uniforms").is_some());
    let build = container.output("police").expect("the build is added");
    assert!(build.take.contains(&"uniforms".to_string()));
    let names: Vec<_> = container.outputs.iter().map(|o| o.name.clone()).collect();
    assert_eq!(
        names.len(),
        names
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "no two builds write one folder: {names:?}"
    );
    assert!(
        file.parent()
            .unwrap()
            .join("uniforms/wardrobe.json")
            .is_file(),
        "the clothing content starts its folder"
    );
    assert_eq!(
        state.project_undo_label().as_deref(),
        Some("Applied the template Police pack")
    );

    state.undo_project().expect("the step is taken back");
    assert_eq!(state.project_carried.container, before);

    // What is already there is not added twice.
    state.redo_project().expect("the step is made again");
    let again = state.apply_template(&template).unwrap_err();
    assert!(again.contains("already"), "{again}");
}

#[test]
fn the_choice_of_what_to_commit_is_written_in_the_git_block_and_the_project_reads_it_back() {
    use or_source::git::{self, Commit};
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_buildable_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    assert_eq!(state.commit_choice(), Commit::default());

    let chosen = Commit {
        sources: true,
        build: true,
        game_sources: false,
    };
    state
        .choose_what_to_commit(chosen)
        .expect("the block is written");

    let ignore = std::fs::read_to_string(folder.join(git::IGNORE)).unwrap();
    assert!(
        !ignore.contains("/build/"),
        "the build is committed: {ignore}"
    );
    assert!(ignore.contains("# commit: sources build"), "{ignore}");
    // A project opened again reads what the block states.
    state.load_project_from(file.clone());
    assert_eq!(state.commit_choice(), chosen);
    // Nothing is rewritten for a choice that is already the one.
    state
        .choose_what_to_commit(chosen)
        .expect("the same choice");
    assert_eq!(
        std::fs::read_to_string(folder.join(git::IGNORE)).unwrap(),
        ignore
    );
}

#[test]
fn a_project_reopens_on_the_workspace_it_was_left_on_and_writes_nothing_before_a_change() {
    use crate::ui::dock::Workspace;
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    let private = or_project::snapshot::private_dir(&file).join("workspace.json");
    state.poll_workbench();
    assert!(!private.exists(), "opening a project is no reason to write");

    let other = if state.active_workspace == Workspace::Texture {
        Workspace::Model
    } else {
        Workspace::Texture
    };
    state.active_workspace = other;
    state.poll_workbench();
    assert!(
        private.is_file(),
        "the workspace left on is kept beside the project"
    );

    drop(state);
    let mut again = super::AppState::from_prefs(super::Prefs::default());
    again.documents.clear();
    again.load_project_from(file);
    assert_eq!(again.active_workspace, other);
}

#[test]
fn a_member_source_folder_that_moved_is_followed_by_its_id_and_a_path_alone_is_still_missing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let source = folder.join("logo.textures.src");
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.remove_asset(&row(&state, "missing_hat.ydd").key.clone());
    state.add_files_to_project(vec![source.clone()]);
    assert!(state.save_active_project(|_| None));

    // The project file states the id beside the path.
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let entry = saved["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| {
            d["path"]
                .as_str()
                .is_some_and(|p| p.ends_with("logo.textures.src"))
        })
        .expect("the source is a member")
        .clone();
    assert_eq!(entry["id"], "7c1e2b0a-5d1f-4e7a-9b3c-2a6f0d8e4c11");

    // The folder moves while the project is closed, which is while no window holds it.
    drop(state);
    let moved = folder.join("art/logo.textures.src");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&source, &moved).unwrap();

    let mut again = super::AppState::from_prefs(super::Prefs::default());
    again.documents.clear();
    again.load_project_from(file.clone());
    until(&mut again, "the member to be followed", |s| {
        s.project_carried.members.iter().any(|m| m.path == moved)
    });
    assert!(
        again
            .project_carried
            .members
            .iter()
            .all(|m| m.path != source),
        "the old place is not kept"
    );
    until(&mut again, "the missing member to clear", |s| {
        !(s.workbench.findings().iter())
            .any(|f| f.text.contains("logo") && f.text.contains("missing"))
    });
    assert!(
        again
            .workbench
            .history()
            .iter()
            .any(|e| e.text.contains("followed")),
        "{:?}",
        again.workbench.history()
    );

    // A member that never stated an id is still a missing file with Locate.
    let other = tempfile::tempdir().unwrap();
    let (mut plain, _) = opened(other.path());
    until(&mut plain, "the scan", |s| s.workbench.is_scanned());
    assert!(plain
        .workbench
        .rows()
        .iter()
        .any(|r| r.name == "missing_hat.ydd"));
}

const C: &str = "mp_m_freemode_01_mp_m_t";

#[test]
fn a_dropped_folder_that_holds_content_proposes_that_content_and_each_answer_does_what_it_says() {
    use super::workbench::Question;
    use crate::state::os_drop::DropTarget;
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_buildable_project(dir.path());
    let modules = state.project_carried.container.modules.len();

    // A folder of addon peds: linked as content, or imported as sources.
    let peds = dir.path().join("a_friends_peds");
    std::fs::create_dir_all(&peds).unwrap();
    or_addonped::AddonPeds::new().write(&peds).unwrap();
    state.handle_os_drop(vec![peds.clone()], DropTarget::Project);
    let Some(Question::DropContent { folder, kind, .. }) = state.workbench.asking.clone() else {
        panic!("nothing proposed: {:?}", state.workbench.asking.is_some());
    };
    assert_eq!((folder, kind.as_str()), (peds.clone(), "Addon ped"));
    state.answer_dropped_content(true);
    assert!(state.workbench.asking.is_none());
    let linked = (state.project_carried.container.modules.iter())
        .find(|m| m.kind == or_project::ModuleKind::AddonPed)
        .expect("the folder is linked as addon ped content");
    assert!(linked.is_link());
    assert_eq!(state.project_carried.container.modules.len(), modules + 1);
    assert!(
        state
            .workbench
            .history()
            .iter()
            .any(|e| e.text.contains("Linked")),
        "{:?}",
        state.workbench.history()
    );

    // A folder that is a clothing pack: the same two answers, and the second links nothing.
    let pack = dir.path().join("a_pack");
    or_wardrobe::edit::create(&pack, "pack").unwrap();
    state.handle_os_drop(vec![pack.clone()], DropTarget::Project);
    assert!(matches!(
        state.workbench.asking,
        Some(Question::DropContent { ref kind, .. }) if kind == "Clothing"
    ));
    let before = state.project_carried.container.modules.len();
    state.answer_dropped_content(false);
    assert!(state.workbench.asking.is_none());
    assert_eq!(state.project_carried.container.modules.len(), before);

    // A folder that holds nothing OpenRage knows is imported as it was.
    let plain = dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::write(plain.join("note.txt"), b"hello").unwrap();
    state.handle_os_drop(vec![plain], DropTarget::Project);
    assert!(state.workbench.asking.is_none());
}

#[test]
fn a_dropped_folder_of_clothes_asks_whether_to_make_clothing_as_the_files_of_the_drop_do() {
    use super::workbench::Question;
    use crate::state::os_drop::DropTarget;
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_buildable_project(dir.path());
    assert!(!state.has_clothing());
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(
        downloads.join(format!("{C}^jbib_000_u.ydd")),
        or_source::sample::drawables_of("a jacket"),
    )
    .unwrap();
    std::fs::write(
        downloads.join(format!("{C}^jbib_diff_000_a_uni.ytd")),
        or_source::sample::textures_of("a texture"),
    )
    .unwrap();

    state.handle_os_drop(vec![downloads.clone()], DropTarget::Project);

    assert!(
        matches!(&state.workbench.asking, Some(Question::DropClothes { paths }) if *paths == vec![downloads]),
        "{:?}",
        state.workbench.asking.is_some()
    );
}

#[test]
fn a_second_window_on_a_project_another_holds_is_told_so_and_changes_nothing_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let (first, file) = opened(dir.path());
    assert!(
        first.read_only_reason().is_none(),
        "the first window writes"
    );
    let folder = file.parent().unwrap().to_path_buf();

    let mut second = super::AppState::from_prefs(super::Prefs::default());
    second.documents.clear();
    second.load_project_from(file.clone());
    let why = second
        .read_only_reason()
        .expect("the second window only reads");
    assert!(why.contains("another OpenRage window"), "{why}");
    assert!(
        second
            .toasts
            .iter()
            .any(|t| t.message.contains("another OpenRage window")),
        "and is told when it opens: {:?}",
        second.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );

    let before = std::fs::read(&file).unwrap();
    let container = second.project_carried.container.clone();
    let members = second.project_carried.members.len();

    // Each door that would change the project says no and changes nothing.
    assert!(second.save_project_to(file.clone()).is_err());
    // ⚠ A file the project does not hold: one it holds is skipped by a window that may write too.
    let new = folder.join("server.lua");
    std::fs::write(&new, b"print('new')").unwrap();
    let added = second.add_files_to_project(vec![new]);
    assert_eq!(added.added, 0);
    assert!(!added.refused.is_empty(), "and says why");
    assert!(second
        .edit_build("fivem", "x".into(), |o| o.name = "other".into())
        .is_err());
    assert!(second.verify_build("fivem", true).is_err());
    assert!(second
        .choose_what_to_commit(or_source::git::Commit::default())
        .is_err());
    second.add_clothing_content();
    second.build_selected();
    assert!(!second.workbench.builds().is_running());

    assert_eq!(second.project_carried.container, container);
    assert_eq!(second.project_carried.members.len(), members);
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert!(!folder.join("build").exists(), "no build was written");

    // The first window lets go, and a window opened after it writes.
    drop(first);
    let mut third = super::AppState::from_prefs(super::Prefs::default());
    third.documents.clear();
    third.load_project_from(file);
    assert!(
        third.read_only_reason().is_none(),
        "nobody else holds it now"
    );
}

#[test]
fn the_sample_project_is_made_of_synthetic_content_opens_with_a_problem_to_find_and_is_made_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("sample");

    let file = super::AppState::make_sample_project(&root).expect("the sample is made");
    assert!(file.is_file() && file.starts_with(&root), "{file:?}");
    let written = std::fs::read(&file).unwrap();

    let mut state = super::AppState::from_prefs(super::Prefs::default());
    state.documents.clear();
    state.load_project_from(file.clone());
    until(&mut state, "the garments", |s| {
        s.workbench.packs().first().is_some_and(|p| {
            p.read
                .as_ref()
                .is_ok_and(|read| read.wardrobe.garments.len() == 3)
        }) && !s.workbench.findings().is_empty()
    });
    assert!(
        state
            .workbench
            .findings()
            .iter()
            .any(|f| f.text.contains("Ships the same model as")),
        "the sample has one problem to find: {:?}",
        state.workbench.findings()
    );

    // Made once: a second call finds it and writes nothing.
    assert_eq!(super::AppState::make_sample_project(&root).unwrap(), file);
    assert_eq!(std::fs::read(&file).unwrap(), written);
}

#[test]
fn the_tour_walks_the_screens_one_step_at_a_time_and_ends_on_the_guide() {
    let mut state = super::AppState::from_prefs(super::Prefs::default());
    assert!(state.front_door.tour.is_none());
    state.start_tour();
    assert_eq!(state.front_door.tour, Some(0));
    state.tour_back();
    assert_eq!(
        state.front_door.tour,
        Some(0),
        "there is nothing before the first step"
    );

    let steps = super::front_door::TOUR.len();
    assert!(steps >= 4, "a tour of the screens that matter");
    for at in 0..steps {
        assert_eq!(state.front_door.tour, Some(at));
        assert_eq!(
            state.active_workspace,
            super::front_door::TOUR[at].workspace,
            "each step shows its screen"
        );
        state.tour_next();
    }
    assert!(
        state.front_door.tour.is_none(),
        "the last step ends the tour"
    );
    assert!(state.front_door.guide.is_some(), "and opens the guide");

    state.front_door.guide = None;
    state.start_tour();
    state.end_tour();
    assert!(state.front_door.tour.is_none() && state.front_door.guide.is_none());
}
