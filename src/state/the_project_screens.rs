//! What the project screens read from the workbench: a clothing content's garments among the assets, a failed
//! build among the problems, the Target of a loose file, and what Open does with a file no workspace takes.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a worker
//! for each scan and build that a case waits on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::resource::Rsc7;
use or_io::Generation;
use or_project::{Container, Output, Project};

use super::front_door::{NewProject, Template};
use super::workbench::{AssetRow, LastBuild, Place, Severity};
use super::{AppState, Prefs};
use crate::ui::dock::Workspace;

const C: &str = "mp_m_freemode_01_mp_m_t";

fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

fn bare() -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state
}

fn until(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
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

/// A clothing project of `jackets` garments, the first two of them the same bytes so the engine has something to say.
fn clothing_of(dir: &Path, jackets: usize) -> (AppState, String) {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let created = draft.create().expect("the template makes a project");
    let pack = created.file.parent().unwrap().join("clothing");
    let mut dropped = Vec::new();
    for n in 0..jackets {
        let model = if n < 2 {
            or_source::sample::drawables_of("the same jacket")
        } else {
            or_source::sample::drawables_of(&format!("jacket {n}"))
        };
        dropped.push(write(
            &dir.join("drop").join(format!("{C}^jbib_{n:03}_u.ydd")),
            &model,
        ));
        dropped.push(write(
            &dir.join("drop")
                .join(format!("{C}^jbib_diff_{n:03}_a_uni.ytd")),
            // The same jacket in the same colour: only a copy of a garment is a duplicate of it.
            if n < 2 {
                or_source::sample::textures_of("the same texture")
            } else {
                or_source::sample::textures_of(&format!("texture {n}"))
            }
            .as_slice(),
        ));
    }
    or_wardrobe::edit::add_clothes(&pack, &dropped).expect("the garments are added");
    let mut state = bare();
    state.load_project_from(created.file);
    let content = state.project_carried.container.modules[0].id.clone();
    (state, content)
}

/// A project of the files in `members`, with one FiveM resource that ships them.
fn files_project(dir: &Path, members: &[PathBuf]) -> (AppState, PathBuf) {
    let mut container = Container::default();
    container
        .outputs
        .push(Output::fivem_resource("fivem", "pack", &["files"]));
    let file = dir.join("pack").join("pack.openrage-proj");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    Project::create(&file, &container, members).expect("a new project");
    let mut state = bare();
    state.load_project_from(file.clone());
    (state, file)
}

fn row<'a>(state: &'a AppState, name: &str) -> &'a AssetRow {
    state
        .workbench
        .rows()
        .iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no asset named {name}: {:?}", state.workbench.rows()))
}

#[test]
fn a_clothing_pack_of_ten_garments_lists_ten_rows_each_in_the_state_of_its_worst_finding() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, content) = clothing_of(dir.path(), 10);
    until(&mut state, "the garments", |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.wardrobe.garments.len() == 10)
    });
    until(&mut state, "the rows", |s| {
        s.workbench.rows().iter().any(|r| r.content == content)
    });

    let rows: Vec<&AssetRow> = (state.workbench.rows().iter())
        .filter(|r| r.content == content)
        .collect();
    assert_eq!(
        rows.len(),
        10,
        "a pack of 10 garments is not a project that holds no files: {rows:?}"
    );
    let mut said = 0;
    for asset in &rows {
        let id = asset.id.clone().expect("a garment has its stable id");
        let worst = (state.workbench.findings().iter())
            .filter(|f| matches!(&f.place, Place::Garment { id: Some(g), .. } if *g == id))
            .map(|f| f.severity)
            .max();
        said += usize::from(worst.is_some());
        let expected = match worst {
            None => "Ready",
            Some(Severity::Warning) => "Warning",
            Some(Severity::Error) => "Error",
        };
        assert_eq!(
            state.workbench.state_of(asset).label(),
            expected,
            "{} stands where its worst finding puts it",
            asset.name
        );
    }
    assert!(
        said > 0,
        "the case needs a garment the engine has found something in"
    );
}

#[test]
fn opening_a_garment_row_opens_the_wardrobe_on_that_garment() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, content) = clothing_of(dir.path(), 3);
    until(&mut state, "the rows", |s| {
        s.workbench
            .rows()
            .iter()
            .filter(|r| r.content == content)
            .count()
            == 3
    });
    state.active_workspace = Workspace::Project;
    let asset = state
        .workbench
        .rows()
        .iter()
        .find(|r| r.content == content)
        .expect("a garment row")
        .clone();

    state.open_asset(&asset.key, None);

    assert_eq!(state.active_workspace, Workspace::Wardrobe);
    assert_eq!(
        state.workbench.wardrobe.selected,
        Some((content, asset.id.expect("its stable id")))
    );
}

#[test]
fn a_build_that_failed_with_a_clear_plan_is_an_error_in_problems_naming_the_build_and_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let script = write(&dir.path().join("pack/client.lua"), b"print('hello')");
    let (mut state, file) = files_project(dir.path(), &[script]);
    until(&mut state, "the plan to clear", |s| {
        s.workbench.is_scanned() && !s.workbench.is_blocked("fivem")
    });
    assert!(
        state.workbench.findings().is_empty(),
        "nothing is wrong yet"
    );
    // The folder a build writes into is a file, so the plan is clear and the write is not.
    write(&file.parent().unwrap().join("build"), b"not a folder");

    state.build_selected();
    until(&mut state, "the failure", |s| {
        !s.workbench.builds().is_running() && s.workbench.builds().last("fivem").is_some()
    });
    let Some(LastBuild::Failed(reasons)) = state.workbench.builds().last("fivem").cloned() else {
        panic!("a build into a file was written");
    };
    until(&mut state, "the finding", |s| {
        !s.workbench.findings().is_empty()
    });

    let found: Vec<_> = (state.workbench.findings().iter())
        .filter(|f| f.place == Place::Build("fivem".into()))
        .collect();
    assert!(!found.is_empty(), "{:?}", state.workbench.findings());
    assert!(found.iter().all(|f| f.severity == Severity::Error));
    assert!(
        found.iter().any(|f| f.text.contains(&reasons[0])),
        "the reason {:?} is said: {found:?}",
        reasons[0]
    );

    // A build that goes through takes the row away.
    std::fs::remove_file(file.parent().unwrap().join("build")).unwrap();
    state.build_selected();
    until(&mut state, "the build", |s| {
        matches!(
            s.workbench.builds().last("fivem"),
            Some(LastBuild::Built(_))
        )
    });
    until(&mut state, "the row to go", |s| {
        s.workbench.findings().is_empty()
    });
}

#[test]
fn open_on_a_file_no_workspace_takes_says_why_and_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    // ⚠ A .lua is text since an opened fxmanifest.lua edits in the Data workspace; a library is a file nothing edits.
    let script = write(&dir.path().join("pack/client.dll"), b"MZ");
    let (mut state, _) = files_project(dir.path(), &[script]);
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.active_workspace = Workspace::Project;
    let key = row(&state, "client.dll").key.clone();

    state.open_asset(&key, None);

    let said = state
        .toasts
        .iter()
        .map(|t| t.message.clone())
        .collect::<Vec<_>>();
    assert!(
        said.iter()
            .any(|m| m.contains("client.dll") && m.contains(".dll")),
        "the person is told why nothing opened: {said:?}"
    );
    assert_eq!(state.active_workspace, Workspace::Project);
    assert!(
        state.documents.is_empty()
            && state.load_receiver.is_none()
            && state.pending_loads.is_empty(),
        "no hex view of a library was queued"
    );
}

/// A drawable's header over bytes that are no drawable, at the version `target` ships.
fn drawable_of(target: Generation) -> Vec<u8> {
    let version = (or_io::gta5::formats::format("ydr")
        .expect("declared")
        .file_type)
        .version_for(target)
        .expect("a version");
    let nibble = |v: u32| (v & 0xF) << 28;
    let header = Rsc7::header_from_entry_flags(nibble(version >> 4), nibble(version));
    [header.header_bytes(), vec![0; 64]].concat()
}

#[test]
fn a_loose_file_shows_the_target_its_header_names_and_a_script_shows_none() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let members = [
        write(&pack.join("legacy.ydr"), &drawable_of(Generation::Gen8)),
        write(&pack.join("enhanced.ydr"), &drawable_of(Generation::Gen9)),
        write(&pack.join("client.lua"), b"print('hello')"),
    ];
    let (mut state, _) = files_project(dir.path(), &members);
    until(&mut state, "the scan", |s| s.workbench.is_scanned());

    let target = |name: &str| {
        (row(&state, name).facts.iter())
            .find(|(fact, _)| *fact == "Target")
            .map(|(_, value)| value.clone())
    };
    assert_eq!(
        target("legacy.ydr").as_deref(),
        Some(Generation::Gen8.edition_label())
    );
    assert_eq!(
        target("enhanced.ydr").as_deref(),
        Some(Generation::Gen9.edition_label())
    );
    assert_eq!(target("client.lua"), None);
}

#[test]
fn a_clean_removes_exactly_what_its_question_counted() {
    use super::workbench::Question;

    let dir = tempfile::tempdir().unwrap();
    let script = write(&dir.path().join("pack/client.lua"), b"print('hello')");
    let (mut state, file) = files_project(dir.path(), &[script]);
    let builds = file.parent().unwrap().join("build");
    // The resource, the build it replaced and a build that died midway are what a clean takes.
    let ours: [(&str, &[u8]); 4] = [
        ("old_name/fxmanifest.lua", b"x"),
        ("old_name/data/a.meta", b"yy"),
        (".old_name.previous/a.bin", b"four"),
        (".old_name.new/b.bin", &[0; 8]),
    ];
    for (path, body) in ours {
        write(&builds.join(path), body);
    }
    write(&builds.join(".old_name.openrage-build"), b"marker");
    let theirs = write(
        &builds.join("theirs/fxmanifest.lua"),
        b"not ours, never counted",
    );

    state.ask_to_clean();

    let Some(Question::Clean { files, bytes, .. }) = state.workbench.asking.clone() else {
        panic!("Clean was not asked");
    };
    let expected_bytes: usize = ours.iter().map(|(_, body)| body.len()).sum();
    assert_eq!((files, bytes), (ours.len() as u64, expected_bytes as u64));

    state.clean_builds();

    for (path, _) in ours {
        assert!(!builds.join(path).exists(), "{path} is still there");
    }
    assert!(!builds.join(".old_name.openrage-build").exists());
    assert!(
        theirs.is_file(),
        "what OpenRage did not build is left alone"
    );
}
