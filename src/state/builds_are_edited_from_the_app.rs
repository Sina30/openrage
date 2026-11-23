//! The Builds window's edits: a build's content, ceilings, split and file types change the plan the
//! scan makes, a project of links alone builds, linking a project links what it holds and not its links,
//! and a second build is not named so that it reads as a part of a split.
//!
//! ⚠ Built with `AppState::from_prefs` and projects in a scratch folder: no install, no keys, and a
//! worker for each scan and build a case waits on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_project::{Container, Module, Output, OutputKind, Project};

use super::workbench::{LastBuild, Question};
use super::{AppState, Prefs};

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
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

/// A project `name` in its own folder under `dir` holding `files`, with the builds `outputs`, and the file it is in.
fn project(dir: &Path, name: &str, files: &[&str], container: Container) -> PathBuf {
    let folder = dir.join(name);
    let members: Vec<PathBuf> = files
        .iter()
        .map(|rel| {
            write(&folder.join(rel), rel.as_bytes());
            folder.join(rel)
        })
        .collect();
    let file = folder.join(format!("{name}.openrage-proj"));
    Project::create(&file, &container, &members).expect("a new project");
    file
}

fn opened(file: &Path) -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(file.to_path_buf());
    state
}

fn one_build(resource: &str) -> Container {
    Container {
        outputs: vec![Output::fivem_resource("fivem", resource, &["files"])],
        ..Container::default()
    }
}

/// The shipped paths of the build `id` as the last scan planned them, resource by resource.
fn planned(state: &AppState, id: &str) -> Vec<Vec<String>> {
    (state.workbench.resources_of(id).iter())
        .map(|r| r.plan.files.iter().map(|f| f.path.clone()).collect())
        .collect()
}

#[test]
fn a_second_default_name_never_reads_as_a_part_of_a_split() {
    let dir = tempfile::tempdir().unwrap();
    let file = project(dir.path(), "police_pack", &["a.lua"], Container::default());
    let mut state = opened(&file);

    let mut names = Vec::new();
    for _ in 0..4 {
        let name = state.default_build_name(&OutputKind::FivemResource, None);
        state
            .add_build(&name, OutputKind::FivemResource, None)
            .expect("the offered name is free");
        names.push(name);
    }

    assert_eq!(names[0], "police_pack");
    for later in &names[1..] {
        assert!(
            !names
                .iter()
                .any(|first| or_project::container::reads_as_a_part(first, later)),
            "`{later}` reads as a part of a split of another build: {names:?}"
        );
    }
}

#[test]
fn a_ceiling_and_a_split_set_in_the_window_change_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let brand = project(
        dir.path(),
        "brand",
        &["c.lua", "d.lua"],
        Container::default(),
    );
    let police = project(
        dir.path(),
        "police",
        &["a.lua", "b.lua"],
        one_build("police_pack"),
    );
    let mut state = opened(&police);
    state
        .link_folder(brand.parent().unwrap())
        .expect("a project is linked by its folder");
    until(&mut state, "the plan", |s| {
        !planned(s, "fivem").is_empty() && planned(s, "fivem")[0].len() == 5
    });
    assert!(!state.workbench.is_blocked("fivem"));

    state
        .edit_build("fivem", "ceiling".into(), |o| o.max_files = Some(4))
        .expect("a ceiling");
    until(&mut state, "the plan to be blocked", |s| {
        s.workbench.is_blocked("fivem")
    });

    state
        .edit_build("fivem", "split".into(), |o| o.split = true)
        .expect("a split");
    until(&mut state, "the plan to divide", |s| {
        planned(s, "fivem").len() == 2
    });
    assert!(!state.workbench.is_blocked("fivem"), "each part fits");
    let parts = planned(&state, "fivem");
    assert!(parts[0].contains(&"a.lua".to_string()) && !parts[0].contains(&"c.lua".to_string()));
    assert!(parts[1].contains(&"c.lua".to_string()) && !parts[1].contains(&"a.lua".to_string()));
    let names: Vec<String> = (state.workbench.resources_of("fivem").iter())
        .map(|r| r.plan.resource.clone())
        .collect();
    assert_eq!(names, ["police_pack_1", "police_pack_2"]);
    assert!(state.project_carried.changed, "Save project keeps the edit");
}

#[test]
fn a_content_left_out_of_a_build_leaves_its_files_out_of_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let brand = project(dir.path(), "brand", &["c.lua"], Container::default());
    let police = project(dir.path(), "police", &["a.lua"], one_build("police_pack"));
    let mut state = opened(&police);
    state.link_folder(brand.parent().unwrap()).unwrap();
    until(&mut state, "both contents in the plan", |s| {
        planned(s, "fivem")
            .first()
            .is_some_and(|files| files.contains(&"c.lua".to_string()))
    });

    state
        .take_content("fivem", "brand", false)
        .expect("a content is left out");

    until(&mut state, "the plan without it", |s| {
        planned(s, "fivem")
            .first()
            .is_some_and(|files| !files.contains(&"c.lua".to_string()))
    });
    assert!(planned(&state, "fivem")[0].contains(&"a.lua".to_string()));
    assert!(state.take_content("fivem", "ghost", true).is_err());
}

#[test]
fn a_project_of_links_only_is_built_by_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let brand = project(
        dir.path(),
        "brand",
        &["client/main.lua", "html/ui.html"],
        Container::default(),
    );
    let hub = project(
        dir.path(),
        "hub",
        &[],
        Container {
            modules: Vec::new(),
            outputs: vec![Output::fivem_resource("fivem", "hub_pack", &[])],
        },
    );
    let mut state = opened(&hub);
    let said = state
        .link_folder(brand.parent().unwrap())
        .expect("a project is linked");
    assert!(said.contains("brand"), "{said}");
    assert!(state
        .project_carried
        .container
        .modules
        .iter()
        .all(Module::is_link));

    until(&mut state, "the plan", |s| {
        !planned(s, "fivem").is_empty() && !s.workbench.is_blocked("fivem")
    });
    state.build_selected();
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running()
    });

    let last = state.workbench.builds().last("fivem").expect("it ended");
    assert!(matches!(last, LastBuild::Built(_)), "{last:?}");
    let built = hub.parent().unwrap().join("build/hub_pack");
    assert_eq!(
        std::fs::read(built.join("client/main.lua")).unwrap(),
        b"client/main.lua"
    );
    assert!(built.join("html/ui.html").is_file());
}

#[test]
fn linking_a_project_links_what_it_holds_and_not_its_links() {
    let dir = tempfile::tempdir().unwrap();
    let logos = project(dir.path(), "logos", &["logo.lua"], Container::default());
    let brand = project(dir.path(), "brand", &["c.lua"], Container::default());
    let police = project(dir.path(), "police", &["a.lua"], one_build("police_pack"));
    let mut state = opened(&brand);
    state.link_folder(logos.parent().unwrap()).unwrap();
    assert!(state.save_active_project(|_| None));

    let mut state = opened(&police);
    let said = state
        .link_folder(brand.parent().unwrap())
        .expect("brand is linked");

    assert!(
        said.contains("logos") && said.contains("not followed"),
        "the sentence says what it did not follow: {said}"
    );
    assert!(state.project_carried.container.module("logos").is_none());
    until(&mut state, "the link", |s| {
        s.workbench.link("brand").is_some()
    });
    let banner = state.workbench.link("brand").unwrap().banner();
    assert!(
        banner.contains("logos") && banner.contains("not followed"),
        "{banner}"
    );
}

#[test]
fn the_window_opens_on_the_chosen_build_and_on_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let file = project(
        dir.path(),
        "police_pack",
        &["a.lua"],
        one_build("police_pack"),
    );
    let mut state = opened(&file);

    state.ask_about_builds(false);
    assert!(matches!(
        &state.workbench.asking,
        Some(Question::Builds { chosen: Some(id), adding: None }) if id == "fivem"
    ));

    state.ask_about_builds(true);
    let Some(Question::Builds {
        adding: Some(new), ..
    }) = state.workbench.asking.clone()
    else {
        panic!("a new build was not offered");
    };
    assert_eq!(new.take, ["files"], "a new build takes all the content");
    assert_ne!(new.name, "police_pack", "the name offered is free");
    assert!(new.focus && !new.edited);

    let id = state
        .add_build_taking(&new.name, new.kind, new.target, &[])
        .expect("a build that takes nothing yet");
    assert!(state
        .project_carried
        .container
        .output(&id)
        .unwrap()
        .take
        .is_empty());
    assert!(state
        .add_build_taking("other", OutputKind::FivemResource, None, &["ghost".into()])
        .is_err());
    assert_eq!(state.project_carried.container.outputs.len(), 2);
}

#[test]
fn a_file_type_stated_in_the_window_registers_the_file_in_a_dlc() {
    let dir = tempfile::tempdir().unwrap();
    let file = project(
        dir.path(),
        "police_pack",
        &["data/handling.meta"],
        Container::default(),
    );
    let mut state = opened(&file);
    let id = state
        .add_build("police_dlc", OutputKind::SingleplayerDlc, None)
        .unwrap();
    until(&mut state, "a plan that needs a type", |s| {
        s.workbench.is_scanned() && s.workbench.is_blocked(&id)
    });
    assert!(
        state
            .workbench
            .cannot_ship("files", &OutputKind::SingleplayerDlc)
            .is_some_and(|why| why.contains("data/handling.meta")),
        "the cell of the matrix is locked, and says which file"
    );

    state
        .state_file_type("data/handling.meta", Some("HANDLING_FILE"))
        .expect("a type is stated");

    until(&mut state, "the plan to be buildable", |s| {
        !s.workbench.is_blocked(&id)
            && s.workbench
                .cannot_ship("files", &OutputKind::SingleplayerDlc)
                .is_none()
    });
    assert!(state
        .state_file_type("data/handling.meta", Some("handling"))
        .is_err());
    state.state_file_type("data/handling.meta", None).unwrap();
    assert!(state.own_files_content().unwrap().file_types().is_empty());
}

#[test]
fn the_files_content_a_first_file_makes_is_taken_by_every_build_and_is_the_projects_own_beside_a_link(
) {
    let dir = tempfile::tempdir().unwrap();
    let brand = project(dir.path(), "brand", &["c.lua"], Container::default());
    let hub = project(
        dir.path(),
        "hub",
        &[],
        Container {
            modules: vec![Module::clothing("hats", "hats")],
            outputs: vec![Output::fivem_resource("fivem", "hub_pack", &["hats"])],
        },
    );
    let mut state = opened(&hub);
    state.link_folder(brand.parent().unwrap()).unwrap();
    let own = hub.parent().unwrap().join("own.lua");
    write(&own, b"own");

    let added = state.add_files_to_project(vec![own]);

    assert_eq!(added.added, 1, "{:?}", added.refused);
    let container = &state.project_carried.container;
    let files: Vec<&Module> = (container.modules.iter())
        .filter(|m| m.kind == or_project::ModuleKind::Files && !m.is_link())
        .collect();
    assert_eq!(
        files.len(),
        1,
        "a linked Files content is not the project's own"
    );
    assert!(
        container.outputs[0].take.contains(&files[0].id),
        "a content no build takes ships nowhere: {:?}",
        container.outputs[0].take
    );
}

#[test]
fn a_content_a_build_takes_or_leaves_out_is_said_in_the_language_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let brand = project(dir.path(), "brand", &["c.lua"], Container::default());
    let police = project(dir.path(), "police", &["a.lua"], one_build("police_pack"));
    let mut state = opened(&police);
    state.link_folder(brand.parent().unwrap()).unwrap();
    let _french = crate::i18n::pin("fr");

    state.take_content("fivem", "brand", false).unwrap();
    let left_out = state.project_undo_label().expect("a step");
    state.take_content("fivem", "brand", true).unwrap();
    let taken = state.project_undo_label().expect("a step");

    for said in [&left_out, &taken] {
        assert!(
            !said.contains("takes") && !said.contains("leaves out"),
            "English in French: {said}"
        );
    }
    assert_ne!(left_out, taken, "the two steps say different things");
}
