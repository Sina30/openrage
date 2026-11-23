//! The mesh doctor as the Wardrobe workspace runs it: on the worker, with the report in the Log, one step of Undo each, and the model a step replaced kept so that the viewport can draw it.
//!
//! ⚠ Reads a ped component staged in the corpus; a machine without it skips, and says so. Built with `AppState::from_prefs` and a
//! project in a scratch folder: no install, so the ped's skeleton is stood up where a case needs it.

use std::path::{Path, PathBuf};

use or_interchange::Bone;
use or_io::decl::{File, Gen8};
use or_io::gta5::GtaFragType;
use or_source::models::{self, Extraction};
use or_test_support::{Corpus, TARGET_GEN8};
use or_wardrobe::{Component, Garment, Ped, Slot, Wardrobe};

use super::the_wardrobe_workspace::{opened, until};
use super::AppState;
use crate::ui::commands::{self, Action};

fn staged(name: &str) -> Option<PathBuf> {
    let found = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == name));
    if found.is_none() {
        or_test_support::skip_corpus(
            &format!("the ped component {name}"),
            "not staged".to_string(),
        );
    }
    found
}

/// The staged skeleton with the most bones: a ped's.
fn ped_skeleton() -> Option<Vec<Bone>> {
    Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft")
        .target(TARGET_GEN8)
        .find()
        .iter()
        .filter_map(|p| {
            let fragment = GtaFragType::<Gen8>::from_bytes(&std::fs::read(p).ok()?).ok()?;
            fragment.drawables().into_iter().find_map(|d| {
                d.base
                    .skeleton
                    .as_deref()
                    .map(or_interchange::extract::bones_of)
            })
        })
        .max_by_key(Vec::len)
        .filter(|b| b.len() >= 100)
}

/// The opened project's pack with one jacket whose model is a source made of the staged `file`.
fn a_pack_with(dir: &Path, file: &Path) -> (AppState, PathBuf, String) {
    let (mut state, project) = opened(dir);
    let pack = project.parent().unwrap().join("clothing");
    std::fs::create_dir_all(pack.join("models")).unwrap();
    models::extract_drawables(
        &std::fs::read(file).unwrap(),
        "model",
        &pack.join("models"),
        &Extraction {
            from_game: false,
            virtual_path: None,
            textures: false,
            skeleton: None,
        },
    )
    .expect("a staged ped component extracts");
    let mut w = Wardrobe::new("tshirts");
    let mut g = Garment::new();
    g.ped = Some(Ped::Male);
    g.slot = Some(Slot::Component(Component::Jbib));
    g.model = Some("models/model.drawables.src".into());
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    state.workbench.want_scan();
    until(&mut state, "the pack", |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|r| r.wardrobe.garments.len() == 1)
    });
    let module = state.wardrobe_module_id().unwrap();
    state.workbench.wardrobe.selected = Some((module, id.clone()));
    (state, pack, id)
}

fn idle(state: &mut AppState) {
    until(state, "the worker to finish", |s| s.wardrobe_is_idle());
}

fn logged(state: &AppState, text: &str) -> bool {
    state
        .workbench
        .history()
        .iter()
        .any(|e| e.text.contains(text))
}

fn run(state: &mut AppState, action: Action) {
    let command = commands::of(state, action).expect("a command");
    assert!(command.enabled.is_ok(), "{action:?}: {:?}", command.enabled);
    commands::run(state, &egui::Context::default(), &command);
}

fn model_of(pack: &Path) -> Option<String> {
    Wardrobe::read(pack).unwrap().garments[0].model.clone()
}

#[test]
fn levels_are_added_on_the_worker_as_one_step_of_undo_and_the_viewport_can_draw_the_model_as_it_was(
) {
    let Some(file) = staged("jbib_000_u_1.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack, id) = a_pack_with(dir.path(), &file);
    let old = "models/model.drawables.src";

    run(&mut state, Action::AddLevels);
    idle(&mut state);
    state.poll_workbench();

    let now = model_of(&pack).unwrap();
    assert_ne!(now, old, "the garment names the new source");
    assert!(
        pack.join(&now).is_dir() && pack.join(old).is_dir(),
        "the old source stays"
    );
    assert_eq!(state.workbench.wardrobe.undo.len(), 1, "one step of Undo");
    assert!(
        logged(&state, "Add levels of detail"),
        "the Log says what was done"
    );
    assert!(
        logged(&state, "Medium level"),
        "the Log says what the Medium level came to"
    );

    // The viewport draws the model as it was while it is asked to, and the new one otherwise.
    let desk = &mut state.workbench.wardrobe.mesh;
    let before = desk.before.clone().expect("the step is kept");
    assert_eq!(
        (
            before.garment.as_str(),
            before.was.as_str(),
            before.now.as_str()
        ),
        (id.as_str(), old, now.as_str())
    );
    assert_eq!(desk.drawn_instead(&id, &now), None);
    desk.before.as_mut().unwrap().shown = true;
    assert_eq!(desk.drawn_instead(&id, &now), Some(old));
    assert_eq!(desk.drawn_instead("another garment", &now), None);
    assert_eq!(
        desk.drawn_instead(&id, old),
        None,
        "once the garment names another model the step no longer applies"
    );

    // Pressed again, the doctor finds the levels there and has nothing to do.
    run(&mut state, Action::AddLevels);
    idle(&mut state);
    assert_eq!(state.workbench.wardrobe.undo.len(), 1);
    assert_eq!(model_of(&pack).as_deref(), Some(now.as_str()));

    state.undo_last_change();
    idle(&mut state);
    assert_eq!(model_of(&pack).as_deref(), Some(old));
    assert!(!pack.join(&now).exists(), "Undo takes the new source out");
}

#[test]
fn a_look_at_the_models_writes_nothing_and_says_what_they_hold() {
    let Some(file) = staged("jbib_000_u_1.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack, id) = a_pack_with(dir.path(), &file);
    let before = model_of(&pack);

    run(&mut state, Action::CheckMesh);
    until(&mut state, "the look", |s| {
        s.workbench.wardrobe.mesh.looked.is_some()
    });

    let looked = state.workbench.wardrobe.mesh.looked.as_ref().unwrap();
    assert_eq!(looked.garment, id);
    let lines = looked.result.as_ref().expect("the models are read").lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("High ") && l.contains(" none")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("no model for")),
        "{lines:?}"
    );
    assert_eq!(model_of(&pack), before);
    assert!(state.workbench.wardrobe.undo.is_empty());
}

#[test]
fn a_mirror_without_the_peds_skeleton_is_refused_and_with_it_turns_the_model_over() {
    let (Some(file), Some(skeleton)) = (staged("jbib_000_u_1.ydd"), ped_skeleton()) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack, _) = a_pack_with(dir.path(), &file);
    let old = model_of(&pack);

    run(&mut state, Action::MirrorModel);
    idle(&mut state);
    assert_eq!(model_of(&pack), old, "nothing changed");
    assert!(state.workbench.wardrobe.undo.is_empty());
    assert!(
        logged(&state, "freemode skeleton"),
        "the Log says where the skeleton comes from"
    );

    state
        .workbench
        .wardrobe
        .mesh
        .give_skeleton(Ped::Male, skeleton);
    run(&mut state, Action::MirrorModel);
    idle(&mut state);
    assert_ne!(
        model_of(&pack),
        old,
        "the garment names the mirrored source"
    );
    assert_eq!(state.workbench.wardrobe.undo.len(), 1);
}
