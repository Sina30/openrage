//! What the Wardrobe does with a garment beyond adding and removing it: a model or a texture replaced, a garment copied to
//! the other ped, moved in its slot, its textures added, moved, tinted and taken out, and a list of garments removed,
//! re-assigned or exported together. Each is one step of the one Undo, which takes it back whole.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, as the Wardrobe's other cases are: no install, no
//! keys, and a worker for each edit that a case waits on.

use std::path::{Path, PathBuf};

use or_wardrobe::edit::Toward;
use or_wardrobe::{Ped, Wardrobe};

use super::the_wardrobe_workspace::{drop_files, garments, opened, until, write, C};
use super::toasts::ToastAction;
use super::AppState;

fn idle(state: &mut AppState, what: &str) {
    until(state, what, |s| s.workbench.wardrobe.is_idle());
}

fn pack_folder(project: &Path) -> PathBuf {
    project.parent().unwrap().join("clothing")
}

/// The pack as the disk holds it.
fn on_disk(project: &Path) -> Wardrobe {
    Wardrobe::read(&pack_folder(project)).expect("a pack on disk")
}

/// A state with `count` jackets dropped on it, each with the textures `a` and `b`, in the order of their numbers.
fn with_jackets(dir: &Path, count: usize) -> (AppState, PathBuf) {
    let (mut state, file) = opened(dir);
    let mut names: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..count {
        names.push((
            format!("{C}^jbib_{i:03}_u.ydd"),
            format!("jacket {i}").into_bytes(),
        ));
        for letter in ["a", "b"] {
            names.push((
                format!("{C}^jbib_diff_{i:03}_{letter}_uni.ytd"),
                format!("jacket {i} texture {letter}").into_bytes(),
            ));
        }
    }
    let files: Vec<(&str, &[u8])> = names
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    state.add_clothes(drop_files(dir, &files));
    until(&mut state, "the drop", |s| garments(s) == count);
    idle(&mut state, "the drop");
    (state, file)
}

fn labels(project: &Path) -> Vec<String> {
    on_disk(project)
        .garments
        .iter()
        .map(|g| g.label.clone().unwrap_or_default())
        .collect()
}

fn the_undo_toast(state: &AppState) -> bool {
    state
        .toasts
        .iter()
        .any(|t| matches!(t.action, Some(ToastAction::Undo(_))))
}

#[test]
fn a_texture_dictionary_replaces_a_texture_by_a_source_and_undo_names_the_old_one_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 1);
    let id = on_disk(&file).garments[0].id.clone();
    let rel = on_disk(&file).garments[0].variations[1]
        .texture
        .clone()
        .unwrap();
    let pack = on_disk(&file);
    let picked = write(
        &dir.path().join("picked/better.ytd"),
        &or_source::sample::textures_of("a better texture b"),
    );
    let steps = state.workbench.wardrobe.undo.len();

    state.toasts.clear();
    state.replace_texture(id, 1, picked);
    until(&mut state, "the replacement", |s| {
        s.workbench.wardrobe.undo.len() == steps + 1
    });
    idle(&mut state, "the replacement");

    let after = on_disk(&file);
    assert_eq!(
        after.garments[0].variations[1].texture.as_deref(),
        Some("textures/better.textures.src"),
        "the texture is the source of the file chosen"
    );
    let mut same = after.clone();
    same.garments[0].variations[1].texture = Some(rel.clone());
    assert_eq!(same, pack, "the tint and the flags are as they were");
    assert!(
        pack_folder(&file).join(&rel).join("asset.json").is_file(),
        "the source the garment named is not touched"
    );
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo texture");
    assert!(
        the_undo_toast(&state),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );

    state.undo_last_change();
    until(&mut state, "the undo", |s| {
        s.workbench.wardrobe.undo.len() == steps
    });
    idle(&mut state, "the undo");
    assert_eq!(on_disk(&file), pack, "the garment names the old one again");

    state.redo_last_change();
    until(&mut state, "the redo", |s| {
        s.workbench.wardrobe.undo.len() == steps + 1
    });
    idle(&mut state, "the redo");
    assert_eq!(on_disk(&file), after);
}

#[test]
fn a_model_is_replaced_by_a_source_and_undo_names_the_old_one_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 1);
    let id = on_disk(&file).garments[0].id.clone();
    let rel = on_disk(&file).garments[0].model.clone().unwrap();
    let picked = write(
        &dir.path().join("picked/better.ydd"),
        &or_source::sample::drawables_of("a better jacket"),
    );

    state.replace_model(id, picked);
    until(&mut state, "the replacement", |s| {
        s.workbench.wardrobe.undo_words() == "Undo model"
    });
    idle(&mut state, "the replacement");
    assert_eq!(
        on_disk(&file).garments[0].model.as_deref(),
        Some("models/better.drawables.src")
    );
    assert!(pack_folder(&file).join(&rel).join("asset.json").is_file());

    state.undo_last_change();
    until(&mut state, "the undo", |s| {
        s.workbench.wardrobe.undo.is_empty() || s.workbench.wardrobe.undo_words() != "Undo model"
    });
    idle(&mut state, "the undo");
    assert_eq!(
        on_disk(&file).garments[0].model.as_deref(),
        Some(rel.as_str())
    );
    assert!(!pack_folder(&file)
        .join("models/better.drawables.src")
        .exists());
}

#[test]
fn a_garment_is_copied_to_the_other_ped_with_its_textures_and_undo_takes_the_copy_out() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 1);
    let id = on_disk(&file).garments[0].id.clone();

    state.duplicate_garments(vec![id], Ped::Female);
    until(&mut state, "the copy", |s| garments(s) == 2);
    idle(&mut state, "the copy");

    let w = on_disk(&file);
    let copy = &w.garments[1];
    assert_eq!(copy.ped, Some(Ped::Female));
    assert_eq!(copy.variations.len(), 2, "its textures came with it");
    let copied = or_source::asset::Source::open(
        &pack_folder(&file).join(copy.variations[1].texture.as_deref().unwrap()),
    )
    .expect("the copy has a source of its own");
    assert_eq!(
        copied.manifest.origin.sha256,
        Some(or_source::asset::sha256_hex(
            &or_source::sample::textures_of("jacket 0 texture b")
        ))
    );
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo copy");

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 1);
    idle(&mut state, "the undo");
    assert!(!pack_folder(&file)
        .join(copy.model.as_deref().unwrap())
        .exists());
}

#[test]
fn a_bulk_removal_of_three_garments_is_one_step_of_undo_that_brings_them_all_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 5);
    let before = on_disk(&file);
    let ids: Vec<String> = [0, 2, 4]
        .iter()
        .map(|i| before.garments[*i].id.clone())
        .collect();
    let steps = state.workbench.wardrobe.undo.len();

    state.remove_garments(ids);
    until(&mut state, "the removal", |s| garments(s) == 2);
    idle(&mut state, "the removal");

    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        steps + 1,
        "three garments, one step"
    );
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo removal");
    assert!(state
        .workbench
        .wardrobe
        .undo_label()
        .is_some_and(|l| l.contains("3 garments")));

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 5);
    idle(&mut state, "the undo");
    assert_eq!(on_disk(&file), before, "all three, in their places");
    assert_eq!(state.workbench.wardrobe.undo.len(), steps);
}

#[test]
fn the_ped_and_the_slot_of_a_list_change_together_and_one_undo_takes_each_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 3);
    let before = on_disk(&file);
    let ids: Vec<String> = before.garments.iter().map(|g| g.id.clone()).collect();
    let steps = state.workbench.wardrobe.undo.len();

    state.set_ped_of(ids.clone(), Some(Ped::Female));
    state.set_slot_of(
        ids,
        Some(or_wardrobe::Slot::Component(or_wardrobe::Component::Lowr)),
    );
    until(&mut state, "both edits", |s| {
        s.workbench.wardrobe.undo.len() == steps + 2
    });
    idle(&mut state, "both edits");
    let after = on_disk(&file);
    assert!(after.garments.iter().all(|g| g.ped == Some(Ped::Female)
        && g.slot == Some(or_wardrobe::Slot::Component(or_wardrobe::Component::Lowr))));

    state.undo_last_change();
    state.undo_last_change();
    until(&mut state, "the undos", |s| {
        s.workbench.wardrobe.undo.len() == steps
    });
    idle(&mut state, "the undos");
    assert_eq!(on_disk(&file), before);
}

#[test]
fn a_garment_moves_earlier_in_its_slot_and_the_sentence_names_the_garments_that_took_another_place()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 2);
    let id = on_disk(&file).garments[1].id.clone();
    let before = labels(&file);

    state.toasts.clear();
    state.move_garment(id, Toward::Earlier);
    until(&mut state, "the move", |s| {
        s.workbench.wardrobe.undo_words() == "Undo order"
    });
    idle(&mut state, "the move");

    let after = labels(&file);
    assert_eq!(
        after,
        [before[1].clone(), before[0].clone()],
        "the two swapped"
    );
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("2 garments moved")),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );

    state.undo_last_change();
    until(&mut state, "the undo", |s| {
        labels(&file) == before && s.workbench.wardrobe.is_idle()
    });
}

#[test]
fn a_texture_is_added_moved_tinted_and_taken_out_each_as_a_step_and_a_stale_row_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 1);
    let id = on_disk(&file).garments[0].id.clone();
    let textures = |file: &Path| -> Vec<Option<String>> {
        on_disk(file).garments[0]
            .variations
            .iter()
            .map(|v| v.texture.clone())
            .collect()
    };
    let first = textures(&file);
    let steps = state.workbench.wardrobe.undo.len();

    state.add_variation(
        id.clone(),
        write(
            &dir.path().join("picked/c.ytd"),
            &or_source::sample::textures_of("texture c"),
        ),
    );
    until(&mut state, "the added texture", |s| {
        s.workbench.wardrobe.undo.len() == steps + 1
    });
    idle(&mut state, "the added texture");
    let three = textures(&file);
    assert_eq!(three.len(), 3);
    assert!(
        pack_folder(&file)
            .join(three[2].as_deref().unwrap())
            .join("asset.json")
            .is_file(),
        "the texture added is a source of the pack"
    );

    state.move_variation(id.clone(), 2, three[2].clone(), 0);
    state.tint_variation(id.clone(), 0, three[2].clone(), Some("bla".into()));
    // A row drawn before the move names the file it was drawn with: it is not the texture at that place now.
    state.remove_variation(id.clone(), 2, three[2].clone());
    until(&mut state, "the three edits", |s| {
        s.workbench.wardrobe.is_idle()
    });
    let after = on_disk(&file).garments[0].variations.clone();
    assert_eq!(after.len(), 3, "the stale removal changed nothing");
    assert_eq!(after[0].texture, three[2]);
    assert_eq!(after[0].tint.as_deref(), Some("bla"));
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        steps + 3,
        "the add, the move and the tint are steps, and the refusal is not"
    );

    state.remove_variation(id.clone(), 0, three[2].clone());
    until(&mut state, "the removal", |s| {
        s.workbench.wardrobe.undo.len() == steps + 4
    });
    idle(&mut state, "the removal");
    assert_eq!(textures(&file), first);
}

#[test]
fn garments_are_exported_under_the_names_a_build_gives_them_and_the_pack_is_not_a_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = with_jackets(dir.path(), 2);
    let ids: Vec<String> = on_disk(&file)
        .garments
        .iter()
        .map(|g| g.id.clone())
        .collect();
    let steps = state.workbench.wardrobe.undo.len();
    let out = dir.path().join("exported");

    state.toasts.clear();
    state.export_garments(ids.clone(), out.clone());
    until(&mut state, "the export", |s| {
        s.toasts.iter().any(|t| t.message.starts_with("Wrote "))
    });
    idle(&mut state, "the export");

    let mut names: Vec<String> = std::fs::read_dir(&out)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 6, "two models and four textures: {names:?}");
    assert!(names[0].starts_with(C) && names.iter().any(|n| n.contains("jbib_001_u.ydd")));
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        steps,
        "an export is no edit of the pack"
    );

    state.export_variation(ids[0].clone(), 1, dir.path().join("one"));
    until(&mut state, "the single export", |s| {
        dir.path().join("one").is_dir() && s.workbench.wardrobe.is_idle()
    });
    assert_eq!(
        std::fs::read_dir(dir.path().join("one")).unwrap().count(),
        1
    );
}
