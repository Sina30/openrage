//! What the Project workspace knows of an open project: its assets and their states, its problems, a saved
//! image recooked into the shadow folder, and a build written and read back.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a
//! worker for each scan, cook and build that a case waits on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_project::{Container, Module, Output, OutputKind, Project};

use super::workbench::{
    AssetRow, AssetState, BuildDot, Kept, LastBuild, Place, Question, Severity,
};
use super::{AppState, Prefs};
use crate::ui::dock::Workspace;

pub(super) fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

pub(super) fn png(path: &Path, shade: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([shade, 0, 0, 255]))
        .save(path)
        .unwrap();
}

/// A textures source `logo.textures.src` in `folder` holding one image, which compiles as a new texture.
fn source(folder: &Path) -> PathBuf {
    let dir = folder.join("logo.textures.src");
    write(
        &dir.join("asset.json"),
        br#"{
  "created_with": "openrage test",
  "format": "openrage-source",
  "id": "7c1e2b0a-5d1f-4e7a-9b3c-2a6f0d8e4c11",
  "kind": "textures",
  "name": "logo",
  "notes": "",
  "origin": { "from_game": false, "how": "authored" },
  "platform": "win64",
  "schema": 1,
  "target": "gen8"
}
"#,
    );
    write(&dir.join("textures.json"), b"{ \"textures\": [] }\n");
    png(&dir.join("textures/logo.png"), 40);
    dir
}

/// A project of two scripts, a missing file and a source folder, with one build, opened in a state.
pub(super) fn opened(dir: &Path) -> (AppState, PathBuf) {
    let folder = dir.join("police_pack");
    write(&folder.join("client.lua"), b"print('hello')");
    write(
        &folder.join("data/vehicles.meta"),
        b"<CVehicleModelInfo__InitDataList/>",
    );
    source(&folder);
    let mut container = Container::default();
    container
        .outputs
        .push(Output::fivem_resource("fivem", "police_pack", &["files"]));
    let file = folder.join("police_pack.openrage-proj");
    Project::create(
        &file,
        &container,
        &[
            folder.join("client.lua"),
            folder.join("data/vehicles.meta"),
            folder.join("missing_hat.ydd"),
        ],
    )
    .expect("a new project");
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(file.clone());
    (state, file)
}

/// Polls the workbench until `done`, which a worker's answer makes true.
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

pub(super) fn row<'a>(state: &'a AppState, name: &str) -> &'a AssetRow {
    state
        .workbench
        .rows()
        .iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no asset named {name}: {:?}", state.workbench.rows()))
}

#[test]
fn the_assets_of_a_project_are_its_files_and_its_sources_each_with_its_state() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());

    let names: Vec<&str> = state
        .workbench
        .rows()
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["client.lua", "logo", "missing_hat.ydd", "vehicles.meta"],
        "a row for each file the project holds and each source folder in it"
    );
    let state_of = |name| state.workbench.state_of(row(&state, name));
    assert_eq!(state_of("client.lua"), AssetState::Ready);
    assert_eq!(state_of("missing_hat.ydd"), AssetState::Missing);
    assert_eq!(state_of("logo"), AssetState::NotCooked);

    let logo = row(&state, "logo");
    assert_eq!(logo.kept, Kept::Source);
    assert_eq!(
        logo.id.as_deref(),
        Some("7c1e2b0a-5d1f-4e7a-9b3c-2a6f0d8e4c11")
    );
    assert_eq!(logo.content, "files");
    assert_eq!(logo.textures, ["logo.png"]);
    // ⚠ Nothing chooses the layout a source compiles to, so Properties does not offer it as a fact.
    assert!(
        logo.facts
            .iter()
            .all(|(name, _)| *name != "Target" && *name != "Platform"),
        "{:?}",
        logo.facts
    );
    assert_eq!(row(&state, "client.lua").kept, Kept::File);
    assert_eq!(row(&state, "client.lua").bytes, Some(14));
}

#[test]
fn a_missing_file_is_a_problem_with_what_to_do_and_the_plan_is_not_said_twice() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    until(&mut state, "the findings", |s| {
        !s.workbench.findings().is_empty()
    });

    let missing: Vec<_> = state
        .workbench
        .findings()
        .iter()
        .filter(|f| f.text.contains("missing_hat.ydd"))
        .collect();
    assert_eq!(
        missing.len(),
        1,
        "said once: {:?}",
        state.workbench.findings()
    );
    assert_eq!(missing[0].severity, Severity::Error);
    assert_eq!(
        missing[0]
            .fixes
            .iter()
            .map(|f| f.label())
            .collect::<Vec<_>>(),
        ["Locate…", "Remove from project"]
    );
    let counts = state
        .workbench
        .counts(super::workbench::Scope::Content("files"));
    assert!(counts.errors >= 1);
    assert_eq!(counts.worst(), Some(Severity::Error));
    assert!(
        state.workbench.is_blocked("fivem"),
        "a plan with a missing file would be refused"
    );
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Blocked);

    // Locate points the project at the file's new place.
    let found = file.parent().unwrap().join("stream/missing_hat.ydd");
    write(&found, b"hat");
    let old = file.parent().unwrap().join("missing_hat.ydd");
    state.locate_member(&old, found.clone());
    assert!(
        state.project_carried.changed,
        "the project has an edit to save"
    );
    until(&mut state, "the rescan", |s| {
        s.workbench.rows().iter().any(|r| r.path == found)
    });
    assert_eq!(
        state.workbench.state_of(row(&state, "missing_hat.ydd")),
        AssetState::Ready
    );
    until(&mut state, "the plan to clear", |s| {
        !s.workbench.is_blocked("fivem")
    });
}

#[test]
fn a_saved_image_recooks_that_one_asset_and_a_broken_rule_leaves_the_last_output() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    // Saving a file in the folder is what the watcher reports: `file_saved` says it here, and the watcher itself would say each write again at a time of its own.
    state.workbench.cook_only_what_is_saved();
    let source = row(&state, "logo").path.clone();
    let output = row(&state, "logo")
        .output
        .clone()
        .expect("a source has an output");
    assert!(output.starts_with(or_project::shadow_root(&file)));
    assert!(!output.exists());

    let then = Instant::now() - Duration::from_secs(1);
    state
        .workbench
        .file_saved(source.join("textures/logo.png"), then);
    until(&mut state, "the recook", |s| {
        s.workbench.state_of(row(s, "logo")) == AssetState::Cooked && output.is_file()
    });
    let first = std::fs::read(&output).unwrap();
    assert!(!first.is_empty());
    assert!(
        state
            .workbench
            .history()
            .iter()
            .any(|e| e.text == "Compiled logo.textures.src"),
        "{:?}",
        state.workbench.history()
    );
    // Nothing else was cooked: the shadow folder holds this one asset's folder.
    let cooked: Vec<_> = std::fs::read_dir(or_project::shadow_root(&file))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(cooked.len(), 1);

    // A new image changes the output.
    png(&source.join("textures/logo.png"), 200);
    state
        .workbench
        .file_saved(source.join("textures/logo.png"), then);
    until(&mut state, "the second recook", |s| {
        let recooked = s
            .workbench
            .history()
            .iter()
            .filter(|e| e.text.starts_with("Compiled"))
            .count();
        recooked == 2 && std::fs::read(&output).is_ok_and(|bytes| bytes != first)
    });

    // A rule the cook refuses is a problem and the previous output stays.
    let good = std::fs::read(&output).unwrap();
    write(
        &source.join("textures.json"),
        b"{ \"textures\": [ { \"name\": \"logo\", \"pack\": {} } ] }",
    );
    state
        .workbench
        .file_saved(source.join("textures.json"), then);
    until(&mut state, "the failure", |s| {
        s.workbench.state_of(row(s, "logo")) == AssetState::Failed
    });
    assert_eq!(
        std::fs::read(&output).unwrap(),
        good,
        "the last output was kept"
    );
    until(&mut state, "the finding", |s| {
        s.workbench.findings().iter().any(
            |f| matches!(&f.place, Place::Asset { key, .. } if key.contains("logo.textures.src")),
        )
    });
    let said = state
        .workbench
        .findings()
        .iter()
        .find(|f| f.text.starts_with("logo:"))
        .expect("the cook's problem is listed");
    assert_eq!(said.severity, Severity::Error);

    // Its own writes and files that are no input start nothing, once the cook has answered them.
    let before = state.workbench.history().len();
    state.workbench.file_saved(source.join("notes.txt"), then);
    state
        .workbench
        .file_saved(or_project::shadow_root(&file).join("x.ytd"), then);
    until(&mut state, "the answers", |s| s.workbench.cooks_are_idle());
    assert_eq!(
        state.workbench.history().len(),
        before,
        "an ignored path cooked"
    );
}

#[test]
fn a_build_writes_the_resource_reads_it_back_and_the_dot_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    // The plan is blocked by the missing file until it is removed.
    state.remove_asset(&row(&state, "missing_hat.ydd").key.clone());
    until(&mut state, "the plan to clear", |s| {
        s.workbench.is_scanned()
            && !s.workbench.is_blocked("fivem")
            && s.workbench.rows().len() == 3
    });
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Never);

    state.build_selected();
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Running);
    assert!(
        state
            .running_jobs()
            .iter()
            .any(|j| j.doing == "Building police_pack"),
        "the build is a job in the list"
    );
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running()
    });

    let built = state.workbench.builds().last("fivem").expect("it ended");
    let LastBuild::Built(done) = built else {
        panic!("the build stopped: {built:?}");
    };
    let resource = file.parent().unwrap().join("build/police_pack");
    assert_eq!(
        done.parts.len(),
        1,
        "an output that does not split is one resource"
    );
    let part = &done.parts[0];
    assert_eq!(part.resource, "police_pack");
    assert_eq!(part.folder, resource);
    assert_eq!(
        part.files, 4,
        "two files, the cooked source and the manifest"
    );
    assert_eq!(part.checked, 4);
    assert_eq!(done.files(), 4);
    assert_eq!(
        part.cooked
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        ["stream/logo.ytd"],
        "the source is cooked, not copied"
    );
    assert_eq!(
        part.cooked[0].bytes,
        std::fs::metadata(resource.join("stream/logo.ytd"))
            .unwrap()
            .len(),
        "and the bytes it came out at are the file's"
    );
    assert!(
        resource.join("stream/logo.ytd").is_file() && !resource.join("logo.textures.src").exists(),
        "the source folder shipped, or its cook did not"
    );
    assert_eq!(
        std::fs::read(resource.join("client.lua")).unwrap(),
        b"print('hello')"
    );
    assert!(resource.join("fxmanifest.lua").is_file());
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Built);

    // The next scan compares the plan with the build that is there now, which recorded its items.
    state.workbench.want_scan();
    until(&mut state, "the plan to know the last build", |s| {
        s.workbench
            .resources_of("fivem")
            .first()
            .is_some_and(|r| r.since.is_some())
    });
    assert_eq!(
        state.workbench.resources_of("fivem")[0].since,
        Some(or_project::Diff::default()),
        "nothing changed since the build that was just written"
    );
    assert!(
        state
            .workbench
            .history()
            .iter()
            .any(|e| e.text.starts_with("Built police_pack")),
        "{:?}",
        state.workbench.history()
    );

    // Clean removes it, and only it.
    state.clean_builds();
    assert!(!resource.exists());
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Never);
    assert_eq!(
        std::fs::read_dir(resource.parent().unwrap())
            .unwrap()
            .count(),
        0,
        "a clean left something of the build in build/"
    );
    until(&mut state, "the plan to know no build is there", |s| {
        s.workbench
            .resources_of("fivem")
            .first()
            .is_some_and(|r| r.since.is_none())
    });
    assert!(file.is_file() && file.parent().unwrap().join("client.lua").is_file());
}

#[test]
fn a_source_that_cannot_be_cooked_fails_the_build_as_a_problem_and_keeps_the_last_build() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.remove_asset(&row(&state, "missing_hat.ydd").key.clone());
    until(&mut state, "the plan to clear", |s| {
        s.workbench.is_scanned() && !s.workbench.is_blocked("fivem")
    });
    state.build_selected();
    until(&mut state, "the first build", |s| {
        !s.workbench.builds().is_running()
    });
    assert!(
        matches!(
            state.workbench.builds().last("fivem"),
            Some(LastBuild::Built(_))
        ),
        "{:?}",
        state.workbench.builds().last("fivem")
    );
    let resource = file.parent().unwrap().join("build/police_pack");
    let before = std::fs::read(resource.join("stream/logo.ytd")).unwrap();

    write(
        &file
            .parent()
            .unwrap()
            .join("logo.textures.src/textures/logo.png"),
        b"not an image",
    );
    state.build_selected();
    until(&mut state, "the second build", |s| {
        !s.workbench.builds().is_running()
            && matches!(
                s.workbench.builds().last("fivem"),
                Some(LastBuild::Failed(_))
            )
    });
    let Some(LastBuild::Failed(reasons)) = state.workbench.builds().last("fivem") else {
        panic!("a build over a broken image was built");
    };
    assert!(
        reasons.iter().any(|r| r.contains("logo.textures.src")),
        "{reasons:?}"
    );
    assert_eq!(
        std::fs::read(resource.join("stream/logo.ytd")).unwrap(),
        before,
        "the last build changed"
    );
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Failed);
    until(&mut state, "the failure to be a problem", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| f.severity == Severity::Error && f.text.contains("logo"))
    });
}

#[test]
fn a_build_the_project_no_longer_declares_is_still_cleaned() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.remove_asset(&row(&state, "missing_hat.ydd").key.clone());
    until(&mut state, "the plan to clear", |s| {
        s.workbench.is_scanned() && !s.workbench.is_blocked("fivem")
    });
    state.build_selected();
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running()
    });
    let builds = file.parent().unwrap().join("build");
    assert!(builds.join("police_pack").is_dir(), "the build is there");

    state.remove_build("fivem");
    assert!(state.project_carried.container.outputs.is_empty());
    assert!(
        builds.join("police_pack").is_dir(),
        "removing a build leaves what it wrote"
    );
    state.clean_builds();

    let left: Vec<_> = std::fs::read_dir(&builds)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(
        left.is_empty(),
        "a clean cannot reach the build of a removed build: {left:?}"
    );
    assert!(file.is_file() && file.parent().unwrap().join("client.lua").is_file());
}
#[test]
fn a_build_that_a_plan_refuses_reports_why_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.build_selected();
    until(&mut state, "the refusal", |s| {
        !s.workbench.builds().is_running() && s.workbench.builds().last("fivem").is_some()
    });
    let Some(LastBuild::Failed(reasons)) = state.workbench.builds().last("fivem") else {
        panic!("a plan with a missing file was built");
    };
    assert!(
        reasons.iter().any(|r| r.contains("missing_hat.ydd")),
        "{reasons:?}"
    );
    assert!(!file
        .parent()
        .unwrap()
        .join("build")
        .join("police_pack")
        .exists());
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Failed);
}

#[test]
fn content_and_builds_are_added_checked_and_remembered_by_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());

    assert!(
        state
            .add_build("police_pack", OutputKind::FivemResource, None)
            .is_err(),
        "that resource is already written"
    );
    for bad in ["", ".hidden", "a/b", "a:b", "tail."] {
        assert!(
            state
                .add_build(bad, OutputKind::FivemResource, None)
                .is_err(),
            "{bad:?} is no resource name"
        );
    }
    let id = state
        .add_build("Police Mods", OutputKind::FivemResource, None)
        .expect("a plain name is a build");
    assert_eq!(id, "police mods");
    assert_eq!(state.active_output_id().as_deref(), Some("police mods"));
    assert!(state.project_carried.changed);

    let extra = file.parent().unwrap().join("html/index.html");
    write(&extra, b"<p/>");
    assert_eq!(
        state
            .add_files_to_project(vec![extra.clone(), extra.clone()])
            .added,
        1
    );
    assert_eq!(
        state.add_files_to_project(vec![extra]).added,
        0,
        "already held"
    );

    state.save_project_to(file.clone()).expect("saved");
    assert!(!state.project_carried.changed);
    let saved = Project::load(&file).expect("it reads");
    assert_eq!(saved.container.outputs.len(), 2);
    assert!(saved.members.iter().any(|m| m.path.ends_with("index.html")));
    assert!(saved.container.modules.iter().any(|m| m.id == "files"));

    state.remove_build("police mods");
    assert_eq!(state.project_carried.container.outputs.len(), 1);
    assert_eq!(state.active_output_id().as_deref(), Some("fivem"));
}

#[test]
fn a_project_with_no_content_gets_a_files_module_with_its_first_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p.openrage-proj");
    let bare = Container {
        modules: Vec::new(),
        outputs: Vec::new(),
    };
    Project::create(&file, &bare, &[]).unwrap();
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.load_project_from(file);
    assert!(state.project_carried.container.modules.is_empty());

    let script = dir.path().join("a.lua");
    write(&script, b"x");
    assert_eq!(state.add_files_to_project(vec![script]).added, 1);
    assert_eq!(
        state.project_carried.container.modules,
        [Module::files("files")]
    );
}

#[test]
fn closing_the_project_forgets_it_and_leaves_the_workspace_it_has() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.active_workspace = Workspace::Project;

    state.close_project();
    state.poll_workbench();

    assert!(!state.workbench.is_scanned());
    assert!(state.workbench.rows().is_empty());
    assert_eq!(
        state.active_workspace,
        Workspace::Browse,
        "the Project workspace is offered only while a project is open"
    );
    assert_eq!(state.workspaces_offered(), Workspace::WITHOUT_A_PROJECT);
    assert_eq!(state.workbench.asking, None::<Question>);
}

#[test]
fn the_cleaning_question_names_what_it_would_remove_and_removes_nothing_until_it_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    let builds = file.parent().unwrap().join("build");
    // A resource OpenRage built that no output declares, and a folder it did not build.
    write(&builds.join("old_name/fxmanifest.lua"), b"x");
    write(&builds.join(".old_name.openrage-build"), b"marker");
    write(&builds.join("theirs/fxmanifest.lua"), b"y");

    state.ask_to_clean();

    assert_eq!(
        state.workbench.asking,
        Some(Question::Clean {
            resources: vec!["old_name".to_string()],
            files: 1,
            bytes: 1,
        })
    );
    assert!(builds.join("old_name/fxmanifest.lua").is_file());
    assert!(builds.join("theirs/fxmanifest.lua").is_file());
}

#[test]
fn closing_a_project_while_a_build_writes_does_not_forget_it_or_let_a_second_start_over_it() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("big_pack");
    // Big enough that the build is still writing when the project is closed.
    let big = folder.join("stream/big.bin");
    write(&big, &vec![7u8; 96 * 1024 * 1024]);
    let mut container = Container::default();
    container
        .outputs
        .push(Output::fivem_resource("fivem", "big_pack", &["files"]));
    let file = folder.join("big_pack.openrage-proj");
    Project::create(&file, &container, &[big]).expect("a new project");
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.load_project_from(file.clone());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());

    state.build_selected();
    assert!(state.workbench.builds().is_running());
    state.close_project();
    state.poll_workbench();
    assert!(
        state.workbench.builds().is_running(),
        "the worker goes on, so the run is still known"
    );

    state.load_project_from(file);
    state.poll_workbench();
    state.build_all();
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message == "A build is already running"),
        "a second build over the same folder was not refused"
    );

    until(&mut state, "the build to end", |s| {
        !s.workbench.builds().is_running()
    });
    assert!(
        matches!(
            state.workbench.builds().last("fivem"),
            Some(LastBuild::Built(_))
        ),
        "the build that was left running finished and its result was not lost"
    );
}
