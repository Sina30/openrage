//! Content another project holds is linked and read here, a folder that exists is adopted with what
//! was recognised in it grouped and what was not placed listed, a project's shape is shared as a
//! template, and a file that has no place in the project is refused with a sentence.
//!
//! ⚠ Built with `AppState::from_prefs` and projects in a scratch folder: no install, no keys.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_project::{ModuleKind, Target};
use or_wardrobe::{layout, Component, Ped, Skin, Slot, Wardrobe};

use super::front_door::{GameChoice, NewProject, Previews, Template};
use super::project::load_project;
use super::{AppState, Prefs};
use crate::ui::commands::{self, Action};

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Polls the workbench until `done`, which a worker's answer makes true.
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

/// A project `brand` beside a project `police`, which is open in the state, and the pack `brand` holds.
fn two_projects(dir: &Path) -> (AppState, PathBuf, PathBuf) {
    let pack = dir
        .join("shared")
        .join("brand")
        .join("modules")
        .join("uniforms");
    write(&pack.join("models/m.ydd"), b"m");
    Wardrobe::new("brand").write(&pack).unwrap();
    write(
        &dir.join("shared").join("brand").join("brand.openrage-proj"),
        b"{}",
    );

    let mut draft = NewProject::new(Template::Files, dir.join("work"));
    std::fs::create_dir_all(dir.join("work")).unwrap();
    draft.set_name("police".into());
    let file = draft.create().expect("police is made").file;
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(file.clone());
    (state, file, pack)
}

#[test]
fn a_pack_another_project_holds_is_linked_shown_with_its_writer_and_kept_across_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file, pack) = two_projects(dir.path());
    let ctx = egui::Context::default();

    let nothing_chosen = commands::of(&state, Action::UnlinkContent)
        .expect("a row")
        .enabled;
    assert_eq!(
        nothing_chosen,
        Err("Choose linked content in the Content panel first")
    );

    let id = state.link_content(&pack).expect("a pack is linked");
    assert_eq!(id, "uniforms");
    assert!(state.project_carried.changed, "Save project keeps the link");
    let container = &state.project_carried.container;
    assert!(container.module(&id).is_some_and(|m| m.is_link()));
    assert!(container.outputs[0].take.contains(&id), "a build takes it");

    until(&mut state, "the scan", |s| {
        s.workbench.link("uniforms").is_some()
    });
    let link = state.workbench.link("uniforms").unwrap();
    assert_eq!(link.problem, None);
    assert!(
        link.banner().contains("brand.openrage-proj"),
        "{}",
        link.banner()
    );
    assert!(
        state.selected_link().is_some(),
        "the new content is the one chosen"
    );
    assert_eq!(
        commands::of(&state, Action::UnlinkContent).unwrap().enabled,
        Ok(())
    );

    assert!(state.save_active_project(|_| None));
    state.load_project_from(file.clone());
    let kept = state
        .project_carried
        .container
        .module("uniforms")
        .expect("kept");
    assert!(kept.is_link());
    assert_eq!(kept.kind, ModuleKind::Clothing);

    // A folder inside the project, around it, or already linked is no link.
    let inside = file.parent().unwrap().join("modules").join("own");
    std::fs::create_dir_all(&inside).unwrap();
    Wardrobe::new("own").write(&inside).unwrap();
    assert!(state.link_content(&inside).unwrap_err().contains("inside"));
    assert!(state
        .link_content(&pack)
        .unwrap_err()
        .contains("already linked"));
    let bare = dir.path().join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    assert!(state
        .link_content(&bare)
        .unwrap_err()
        .contains("holds no content"));

    // The folder going away is a problem with a sentence, and unlinking touches no file.
    std::fs::remove_dir_all(&pack).unwrap();
    state.workbench.want_scan();
    until(&mut state, "the scan to see the folder gone", |s| {
        s.workbench
            .link("uniforms")
            .is_some_and(|l| l.problem.is_some())
    });
    assert!(state
        .workbench
        .findings()
        .iter()
        .any(|f| f.text.contains("not there")));
    // Choosing the content in the Content panel is what Unlink acts on.
    state.workbench.selected_content = Some("uniforms".into());
    commands::run_action(&mut state, &ctx, Action::UnlinkContent);
    assert!(state.project_carried.container.module("uniforms").is_none());
    assert!(state.project_carried.container.outputs[0]
        .take
        .iter()
        .all(|t| t != "uniforms"));
}

#[test]
fn a_file_with_no_place_in_the_project_is_refused_with_a_sentence_and_never_added_silently() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file, pack) = two_projects(dir.path());
    state.link_content(&pack).unwrap();
    let inside = file.parent().unwrap().join("client").join("main.lua");
    write(&inside, b"x");
    let outside = dir.path().join("elsewhere").join("a.ydr");
    write(&outside, b"x");
    let linked = pack.join("models").join("m.ydd");

    let done = state.add_files_to_project(vec![inside.clone(), outside.clone(), linked]);
    assert_eq!(done.added, 1);
    assert_eq!(done.refused.len(), 2, "{:#?}", done.refused);
    assert!(
        done.refused[0].contains("outside the project's folder"),
        "{}",
        done.refused[0]
    );
    assert!(
        done.refused[1].contains("linked content `uniforms`"),
        "{}",
        done.refused[1]
    );
    assert!(state.member_paths().contains(&inside));
    assert!(!state.member_paths().contains(&outside));

    // Pointing a member at a file outside the folder is refused too.
    let before = state.project_carried.members.clone();
    state.locate_member(&inside, outside);
    assert_eq!(state.project_carried.members, before);
}

/// A resource of the pack `harbor` with files that only have names, and the scripts of a resource.
fn resource(folder: &Path) {
    let slot = Slot::Component(Component::Jbib);
    let directory = layout::directory("harbor", Ped::Male, "mp_m_harbor", slot);
    let model = layout::model_name(Ped::Male, "mp_m_harbor", slot, 0, Skin::Universal);
    let texture = layout::texture_name(Ped::Male, "mp_m_harbor", slot, 0, 0, "uni");
    write(&folder.join(&directory).join(model), b"model");
    write(&folder.join(&directory).join(texture), b"texture");
    write(&folder.join("fxmanifest.lua"), b"fx_version 'cerulean'\n");
    write(&folder.join("client/main.lua"), b"print('hi')");
    write(&folder.join("stream/harbor/odd.ydd"), b"odd");
}

#[test]
fn a_folder_that_holds_a_pack_is_adopted_as_clothing_and_files_and_what_was_not_placed_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("harbor_resource");
    resource(&folder);

    // The banner and the dialog read the folder off the UI thread.
    let mut previews = Previews::default();
    let until_read = Instant::now() + Duration::from_secs(30);
    let preview = loop {
        if let Some(preview) = previews.of(&folder) {
            break preview.clone();
        }
        assert!(Instant::now() < until_read, "the folder was never read");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        preview.found[0].starts_with("Clothing:"),
        "{:?}",
        preview.found
    );
    assert!(
        preview.headline().starts_with("Found: Clothing, Files (2)"),
        "{}",
        preview.headline()
    );
    assert!(
        !folder.join("wardrobe.json").exists(),
        "reading writes nothing"
    );

    let created = NewProject::adopting(&folder).create().expect("adopted");
    assert_eq!(created.found.len(), 1);
    assert!(
        created.unplaced >= 2,
        "the manifest and the odd model: {}",
        created.unplaced
    );
    assert!(folder.join("wardrobe.json").is_file());

    let loaded = load_project(&created.file).expect("it reads");
    let kinds: Vec<&ModuleKind> = loaded
        .project
        .container
        .modules
        .iter()
        .map(|m| &m.kind)
        .collect();
    assert_eq!(kinds, [&ModuleKind::Clothing, &ModuleKind::Files]);
    let held: Vec<String> = loaded
        .project
        .documents
        .iter()
        .map(|d| d.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(held, ["main.lua", "odd.ydd"]);
    let adopted = loaded
        .project
        .extra
        .get(or_project::adopt::ADOPTED_KEY)
        .expect("recorded");
    assert!(
        adopted["unplaced"].as_array().is_some_and(|l| l.len() >= 2),
        "{adopted}"
    );

    // The same project opened in the app carries the record, which Properties lists.
    let mut state = AppState::from_prefs(Prefs::default());
    state.load_project_from(created.file.clone());
    assert!(state
        .project_carried
        .top
        .contains_key(or_project::adopt::ADOPTED_KEY));
}

#[test]
fn a_template_saved_from_a_project_makes_the_same_project_in_the_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _, pack) = two_projects(dir.path());
    state.link_content(&pack).unwrap();
    state.project_carried.container.outputs[0].split = true;
    let file = dir.path().join("police.openrage-template");

    let left_out = state.save_project_as_template(&file).expect("saved");
    assert_eq!(left_out.len(), 1, "a link is not carried: {left_out:?}");
    assert!(
        state.save_project_as_template(&file).is_err(),
        "a template is never written over a file"
    );

    let place = dir.path().join("made");
    std::fs::create_dir_all(&place).unwrap();
    let mut draft = NewProject::new(Template::Empty, place.clone());
    draft.load_template(&file).expect("it reads");
    assert_eq!(draft.template, Template::Shared);
    draft.set_name("Harbor Police".into());
    assert_eq!(draft.refusal(), None);
    assert!(draft.content().contains("Files"), "{}", draft.content());
    assert!(
        draft.builds().contains("harbor_police"),
        "{}",
        draft.builds()
    );

    let created = draft.create().expect("made from the template");
    let made = load_project(&created.file).unwrap();
    let ids: Vec<&str> = made
        .project
        .container
        .modules
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(ids, ["files"]);
    assert_eq!(made.project.container.outputs[0].name, "harbor_police");
    assert!(
        made.project.container.outputs[0].split,
        "what the build was set to travels"
    );
    assert_eq!(made.project.extra.get("template"), Some(&"police".into()));
    assert!(
        made.project.documents.is_empty(),
        "a template carries no file"
    );
}

#[test]
fn the_templates_that_were_saved_are_in_the_dialog_when_it_opens() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _, _) = two_projects(dir.path());
    let folder = super::front_door::new_project::templates_folder().expect("a config folder");
    let saved = folder.join("saved_by_this_test.openrage-template");
    let _ = std::fs::remove_file(&saved);
    std::fs::create_dir_all(&folder).unwrap();
    state.save_project_as_template(&saved).expect("saved");
    write(
        &folder.join("not_a_template.openrage-template"),
        b"not json",
    );

    state.start_new_project(Template::Empty);
    let draft = state
        .front_door
        .new_project
        .as_ref()
        .expect("the dialog is open");
    assert!(
        draft
            .loaded
            .iter()
            .any(|l| l.from.as_deref() == Some(saved.as_path())),
        "{:?}",
        draft
            .loaded
            .iter()
            .map(|l| &l.template.name)
            .collect::<Vec<_>>()
    );
    assert!(
        !draft.loaded.iter().any(|l| l
            .from
            .as_ref()
            .is_some_and(|p| p.ends_with("not_a_template.openrage-template"))),
        "a file that is no template is left out"
    );
    let _ = std::fs::remove_file(&saved);
    let _ = std::fs::remove_file(folder.join("not_a_template.openrage-template"));
}

#[test]
fn the_game_a_project_is_for_is_the_edition_each_new_build_ships_for_unless_it_states_its_own() {
    let dir = tempfile::tempdir().unwrap();
    for (game, expected) in [
        (GameChoice::Legacy, Some(Target::Legacy)),
        (GameChoice::Enhanced, Some(Target::Enhanced)),
        (GameChoice::Both, None),
    ] {
        let place = dir.path().join(game.word());
        std::fs::create_dir_all(&place).unwrap();
        let mut draft = NewProject::new(Template::Files, place);
        draft.set_name("pack".into());
        draft.game = game;
        assert_eq!(draft.container().outputs[0].target, expected, "{game:?}");

        let created = draft.create().unwrap();
        let mut state = AppState::from_prefs(Prefs::default());
        state.load_project_from(created.file);
        assert_eq!(state.default_edition(), expected, "{game:?}");
        // The build the project made states it, so the project's game does not move it.
        assert_eq!(state.project_carried.container.outputs[0].target, expected);
    }

    // An output that states its own edition keeps it.
    let mut container = or_project::Container::default();
    let mut own = or_project::Output::fivem_resource("fivem", "pack", &["files"]);
    own.target = Some(Target::Legacy);
    container.outputs.push(own);
    let kept =
        super::front_door::new_project::with_default_edition(container, GameChoice::Enhanced);
    assert_eq!(kept.outputs[0].target, Some(Target::Legacy));
}
