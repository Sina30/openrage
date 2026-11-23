//! Opening and closing a project at the front door: what Close project asks, which tab a reopened project returns to, what makes the
//! title say the project changed, the job that reads a project, a project that is made on a worker, and the screen a project opens on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_project::template::Template as Shared;
use or_project::{Container, Module, Output};

use super::front_door::new_project::GAME_KEY;
use super::front_door::{Loaded, NewProject, Template};
use super::jobs::JobKind;
use super::project::{load_project, save_project, ProjectDocument, ProjectFile};
use super::{AppState, CloseAnswer, DocumentKind, Prefs};
use crate::ui::dock::Workspace;

fn a_project(dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(name).join(format!("{name}.openrage-proj"));
    save_project(&file, &ProjectFile::default()).expect("a project on disk");
    file
}

/// Poll as the editor's frame does until every file the project listed is open and no load waits.
fn until_loaded(state: &mut AppState, documents: usize) {
    let until = Instant::now() + Duration::from_secs(30);
    while state.documents.len() < documents
        || state.load_receiver.is_some()
        || !state.pending_loads.is_empty()
    {
        assert!(Instant::now() < until, "the files never opened");
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn close_project_asks_when_the_project_changed_and_closes_at_once_when_it_did_not() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = a_project(dir.path(), "police");
    let mut state = AppState::from_prefs(Prefs::default());
    state.load_project_from(file.clone());

    state.close_project();
    assert_eq!(
        state.project_path, None,
        "an unchanged project closes at once"
    );

    state.load_project_from(file.clone());
    state.project_carried.changed = true;
    state.close_project();
    assert!(
        state.pending_close.is_some(),
        "a project with a change nothing wrote down was closed without a question"
    );
    assert_eq!(
        state.project_path.as_deref(),
        Some(file.as_path()),
        "it stays open until the question is answered"
    );

    state.answer_close(CloseAnswer::Cancel);
    assert_eq!(state.project_path.as_deref(), Some(file.as_path()));
    assert!(state.pending_close.is_none());

    state.close_project();
    state.answer_close(CloseAnswer::DontSave);
    assert_eq!(state.project_path, None);
    assert!(
        state.front_door.raised(state.active_workspace),
        "closing raises the start page"
    );
}

#[test]
fn saving_at_the_close_question_writes_the_project_before_it_closes() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = a_project(dir.path(), "police");
    let mut state = AppState::from_prefs(Prefs::default());
    state.load_project_from(file.clone());
    state.project_carried.container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::fivem_resource("fivem", "police", &["files"])],
    };
    state.project_carried.changed = true;

    state.close_project();
    state.answer_close(CloseAnswer::Save);
    state.drive_close(|_, _| None);

    assert_eq!(
        state.project_path, None,
        "the project closed after it was saved"
    );
    let saved = load_project(&file).expect("it reads").project;
    assert_eq!(
        saved.container.outputs.len(),
        1,
        "the build was written down"
    );
}

#[test]
fn a_reopened_project_returns_to_the_tab_it_was_saved_on() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let folder = dir.path().join("police");
    let names = ["a.txt", "b.txt", "c.txt"];
    for name in names {
        std::fs::create_dir_all(&folder).expect("a folder");
        std::fs::write(folder.join(name), name.as_bytes()).expect("a file");
    }
    let file = folder.join("police.openrage-proj");
    let project = ProjectFile {
        workspace: Workspace::Data,
        // ⚠ Not the last tab, which is where the last load leaves the keyboard.
        active: Some(0),
        open: vec![0, 1, 2],
        documents: names
            .iter()
            .map(|name| ProjectDocument::new(folder.join(name), Some(DocumentKind::Text)))
            .collect(),
        ..ProjectFile::default()
    };
    save_project(&file, &project).expect("a project on disk");
    let mut state = AppState::from_prefs(Prefs::default());

    state.load_project_from(file);
    until_loaded(&mut state, 3);

    let active = state.active_document().and_then(|doc| doc.path.clone());
    assert_eq!(
        active.as_deref(),
        Some(folder.join("a.txt").as_path()),
        "the project was saved on a.txt and came back on the last file it opened"
    );
    let order: Vec<_> = state
        .documents
        .iter()
        .filter_map(|doc| doc.path.as_ref()?.file_name()?.to_str().map(str::to_string))
        .collect();
    assert_eq!(order, names, "the tabs keep the order they were saved in");
}

#[test]
fn opening_a_file_never_marks_the_project_changed_whether_it_is_a_member_or_not() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let folder = dir.path().join("police");
    std::fs::create_dir_all(&folder).expect("a folder");
    std::fs::write(folder.join("member.txt"), b"member").expect("a file");
    std::fs::write(folder.join("stranger.txt"), b"stranger").expect("a file");
    let file = folder.join("police.openrage-proj");
    let project = ProjectFile {
        documents: vec![ProjectDocument::new(
            folder.join("member.txt"),
            Some(DocumentKind::Text),
        )],
        ..ProjectFile::default()
    };
    save_project(&file, &project).expect("a project on disk");
    let mut state = AppState::from_prefs(Prefs::default());
    state.load_project_from(file);

    state.load_file(folder.join("member.txt"));
    until_loaded(&mut state, 1);
    assert!(
        !state.project_carried.changed,
        "a member opened is no change"
    );
    assert!(
        !state.window_title().contains('●'),
        "{}",
        state.window_title()
    );

    state.load_file(folder.join("stranger.txt"));
    until_loaded(&mut state, 2);
    assert!(
        !state.project_carried.changed,
        "Open looks at a file; only a command that puts something in the project changes it"
    );
    assert!(
        !state.window_title().contains('●'),
        "the title says the project changed: {}",
        state.window_title()
    );
}

#[test]
fn reading_a_project_is_a_job_in_the_status_bar_until_its_files_are_read() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = a_project(dir.path(), "police");
    let mut state = AppState::from_prefs(Prefs::default());
    assert!(!state
        .running_jobs()
        .iter()
        .any(|job| job.kind == JobKind::ProjectRead));

    state.load_project_from(file);
    let job = (state.running_jobs().into_iter())
        .find(|job| job.kind == JobKind::ProjectRead)
        .expect("the project's files are being read and the status bar says so");
    assert!(job.doing.contains("police"), "{}", job.doing);

    let until = Instant::now() + Duration::from_secs(30);
    while !state.workbench.is_scanned() {
        assert!(Instant::now() < until, "the scan never ended");
        state.poll_workbench();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !state
            .running_jobs()
            .iter()
            .any(|job| job.kind == JobKind::ProjectRead),
        "the job stayed on the status bar after the project was read"
    );
}

/// The dialog's draft on `template`, named and placed.
fn draft(template: Template, place: &Path, name: &str) -> NewProject {
    let mut draft = NewProject::new(template, place.to_path_buf());
    draft.set_name(name.to_string());
    draft
}

#[test]
fn a_built_in_template_records_its_name_in_the_project_it_makes() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for (ix, template) in [Template::Empty, Template::Files, Template::Clothing]
        .into_iter()
        .enumerate()
    {
        let created = draft(template, dir.path(), &format!("made_{ix}"))
            .create()
            .expect("made");
        let made = load_project(&created.file).expect("it reads").project;
        assert_eq!(
            made.extra.get(or_project::template::TEMPLATE_KEY),
            Some(&serde_json::json!(template.label())),
            "{template:?}"
        );
    }
    let folder = dir.path().join("adoptable");
    std::fs::create_dir_all(&folder).expect("a folder");
    std::fs::write(folder.join("client.lua"), b"print(1)").expect("a file");
    let created = NewProject::adopting(&folder).create().expect("adopted");
    let made = load_project(&created.file).expect("it reads").project;
    assert_eq!(
        made.extra.get(or_project::template::TEMPLATE_KEY),
        Some(&serde_json::json!(Template::Adopt.label()))
    );
}

/// A template file whose content is `modules` and which opens on `opens_on`.
fn shared(name: &str, modules: Vec<Module>, opens_on: Option<&str>) -> Loaded {
    let take: Vec<&str> = modules.iter().map(|m| m.id.as_str()).collect();
    let container = Container {
        outputs: vec![Output::fivem_resource("fivem", "pack", &take)],
        modules,
    };
    let (template, findings) = Shared::from_project(name, "", &container, opens_on, None, None);
    Loaded {
        template,
        from: None,
        findings,
    }
}

#[test]
fn the_screen_a_template_says_it_opens_on_is_the_screen_the_project_opens_on() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut drafts = Vec::new();
    for (ix, template) in [Template::Empty, Template::Files, Template::Clothing]
        .into_iter()
        .enumerate()
    {
        drafts.push(draft(template, dir.path(), &format!("built_in_{ix}")));
    }
    for (name, modules, opens_on) in [
        (
            "only_clothes",
            vec![Module::clothing("c", "c")],
            Some("Wardrobe"),
        ),
        (
            "clothes_and_files",
            vec![Module::clothing("c", "c"), Module::files("f")],
            Some("Wardrobe"),
        ),
        ("only_files", vec![Module::files("f")], None),
    ] {
        let mut shared_draft = draft(Template::Files, dir.path(), name);
        shared_draft.offer(shared(name, modules, opens_on));
        drafts.push(shared_draft);
    }
    for draft in drafts {
        let said = draft.opens_on();
        let created = draft.create().expect("made");
        let made = load_project(&created.file).expect("it reads").project;
        assert_eq!(
            made.workspace, said,
            "{} says it opens on {said:?} and the project it made is saved on {:?}",
            draft.name, made.workspace
        );
    }
}

#[test]
fn a_project_holding_more_than_one_kind_of_content_opens_on_project_and_one_kind_on_its_screen() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut mixed = draft(Template::Clothing, dir.path(), "mixed");
    mixed.offer(shared(
        "Clothes and files",
        vec![Module::clothing("c", "c"), Module::files("f")],
        Some("Wardrobe"),
    ));
    let made = mixed.create().expect("made");
    let mut state = AppState::from_prefs(Prefs::default());
    // The file is saved on the Wardrobe, as an older build or another tool wrote it.
    let mut written = load_project(&made.file).expect("it reads").project;
    written.workspace = Workspace::Wardrobe;
    save_project(&made.file, &written).expect("saved");
    state.load_project_from(made.file);
    assert_eq!(
        state.active_workspace,
        Workspace::Project,
        "a project with clothing and files opened on a task screen"
    );

    let clothes = draft(Template::Clothing, dir.path(), "clothes")
        .create()
        .expect("made");
    state.load_project_from(clothes.file);
    assert_eq!(state.active_workspace, Workspace::Wardrobe);
}

#[test]
fn a_project_is_made_on_a_worker_and_opens_when_the_worker_is_done() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());
    state.start_new_project(Template::Files);
    let mut made = state
        .front_door
        .new_project
        .take()
        .expect("the dialog is open");
    made.place = dir.path().display().to_string();
    made.set_name("police".into());
    made.game = super::front_door::GameChoice::Legacy;
    state.front_door.new_project = Some(made.clone());

    state.start_creating(made);

    let file = dir.path().join("police").join("police.openrage-proj");
    assert!(
        state.is_creating(),
        "the write was done in the frame that asked for it"
    );
    assert_eq!(state.project_path, None);
    let until = Instant::now() + Duration::from_secs(30);
    while state.is_creating() {
        assert!(Instant::now() < until, "the project was never made");
        state.poll_creating();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(state.project_path.as_deref(), Some(file.as_path()));
    assert!(state.front_door.new_project.is_none(), "the dialog closed");
    let made = load_project(&file).expect("it reads").project;
    assert_eq!(
        made.extra.get(GAME_KEY),
        Some(&serde_json::json!("legacy")),
        "the worker made what the dialog described"
    );
}

#[test]
fn a_project_the_worker_could_not_make_leaves_the_dialog_open_with_the_reason() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());
    state.start_new_project(Template::Files);
    let mut made = state
        .front_door
        .new_project
        .take()
        .expect("the dialog is open");
    made.place = dir.path().display().to_string();
    made.set_name("police".into());
    state.front_door.new_project = Some(made.clone());
    // Taken between the dialog's check and the worker's write.
    std::fs::create_dir_all(dir.path().join("police")).expect("a folder");
    std::fs::write(dir.path().join("police").join("mine.txt"), b"mine").expect("a file");

    state.start_creating(made);
    let until = Instant::now() + Duration::from_secs(30);
    while state.is_creating() {
        assert!(Instant::now() < until, "the worker never answered");
        state.poll_creating();
        std::thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(state.project_path, None);
    let draft = state
        .front_door
        .new_project
        .as_ref()
        .expect("the dialog stays open");
    assert!(
        draft
            .error
            .as_deref()
            .is_some_and(|why| why.contains("not empty")),
        "{:?}",
        draft.error
    );
}
