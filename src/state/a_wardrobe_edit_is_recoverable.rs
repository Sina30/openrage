//! What makes a Wardrobe edit recoverable: Redo after Undo, the one Undo of the workspace that joins the clothes' changes
//! and the project's steps, a second window that reads a pack and cannot write it, a `wardrobe.json` that does not read and
//! comes back from its last good version, one garment put back while the others stay, and
//! the History asking before it removes the sources of an import.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, as the Wardrobe's other cases are: no install, no
//! keys, and a worker for each edit that a case waits on.

use std::path::Path;
use std::time::Duration;

use or_wardrobe::edit::Edit;
use or_wardrobe::Wardrobe;

use super::the_wardrobe_workspace::{bare, drop_files, garments, opened, until, C};
use super::workbench::{Finding, Fix, Question};
use super::AppState;
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

fn idle(state: &mut AppState, what: &str) {
    until(state, what, |s| s.workbench.wardrobe.is_idle());
}

/// The pack as the disk holds it.
fn on_disk(project: &Path) -> Wardrobe {
    Wardrobe::read(&project.parent().unwrap().join("clothing")).expect("a pack on disk")
}

/// Two jackets of one model, each with a texture: a duplicate with a remedy.
fn two_of_one_jacket(dir: &Path) -> Vec<std::path::PathBuf> {
    drop_files(
        dir,
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_001_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"t0"),
            (&format!("{C}^jbib_diff_001_a_uni.ytd"), b"t0"),
        ],
    )
}

/// A jacket and some trousers, which differ.
fn a_jacket_and_trousers(dir: &Path) -> Vec<std::path::PathBuf> {
    drop_files(
        dir,
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^lowr_000_u.ydd"), b"some trousers"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"t0"),
            (&format!("{C}^jbib_diff_000_b_uni.ytd"), b"t1"),
        ],
    )
}

fn fix_of_the_duplicate(state: &AppState) -> Finding {
    state
        .workbench
        .findings()
        .iter()
        .find(|f| {
            f.fixes.iter().any(|fix| match fix {
                Fix::Remedy { finding, .. } => finding.code == "duplicate-content",
                _ => false,
            })
        })
        .cloned()
        .expect("a duplicate has a finding with a fix")
}

fn the_remedy(finding: &Finding) -> (String, or_wardrobe::Finding) {
    finding
        .fixes
        .iter()
        .find_map(|fix| match fix {
            Fix::Remedy { module, finding } => Some((module.clone(), finding.clone())),
            _ => None,
        })
        .expect("a remedy")
}

/// The garment of the pack whose model is of the component `slot`.
fn garment_of<'a>(pack: &'a Wardrobe, slot: &str) -> &'a or_wardrobe::Garment {
    (pack.garments.iter())
        .find(|g| g.model.as_deref().is_some_and(|m| m.contains(slot)))
        .expect("a garment of that slot")
}

fn error_toast(state: &AppState) -> Option<String> {
    (state.toasts.iter())
        .map(|t| t.message.clone())
        .find(|m| m.contains("Another OpenRage window"))
}

#[test]
fn redo_after_undo_restores_a_fix_and_the_fix_can_be_taken_back_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(two_of_one_jacket(dir.path()));
    until(&mut state, "the duplicate", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| !f.fixes.is_empty() && f.text.contains("same model"))
    });
    let (module, remedy) = the_remedy(&fix_of_the_duplicate(&state));
    state.fix_finding(module, remedy);
    until(&mut state, "the fix", |s| garments(s) == 1);
    let fixed = on_disk(&file);

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 2);
    idle(&mut state, "the undo");
    assert_eq!(
        state.workbench.wardrobe.redo.len(),
        1,
        "the Undo left a Redo"
    );
    assert!(
        state.redo_label().is_some_and(|l| l.starts_with("Fixed: ")),
        "{:?}",
        state.redo_label()
    );
    assert_eq!(
        commands::of(&state, Action::RedoClothes).unwrap().enabled,
        Ok(())
    );

    state.redo_last_change();
    until(&mut state, "the redo", |s| garments(s) == 1);
    idle(&mut state, "the redo");
    assert_eq!(
        on_disk(&file),
        fixed,
        "the fix is made again, byte for byte"
    );
    assert!(state.workbench.wardrobe.redo.is_empty());
    assert_eq!(
        state.workbench.wardrobe.undo_words(),
        "Undo fix",
        "and it is a step of Undo again"
    );

    state.undo_last_change();
    until(&mut state, "the second undo", |s| garments(s) == 2);
    idle(&mut state, "the second undo");
}

#[test]
fn redo_of_a_drop_brings_back_its_garments_and_the_files_and_a_new_change_ends_the_chance() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(a_jacket_and_trousers(dir.path()));
    until(&mut state, "the drop", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    let dropped = on_disk(&file);
    let model = dropped.garments[0].model.clone().expect("a model");
    let folder = file.parent().unwrap().join("clothing");
    assert!(folder.join(&model).join("asset.json").is_file());

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 0);
    idle(&mut state, "the undo");
    assert!(!folder.join(&model).exists(), "the drop's file is gone");

    state.redo_last_change();
    until(&mut state, "the redo", |s| garments(s) == 2);
    idle(&mut state, "the redo");
    assert_eq!(on_disk(&file), dropped);
    assert!(
        folder.join(&model).join("asset.json").is_file(),
        "and so is the source"
    );
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        1,
        "the drop is a step of Undo again"
    );

    // Taken back once more, then a different change: the Redo is gone, and so is what it kept.
    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 0);
    idle(&mut state, "the undo");
    let held = folder.join(".openrage").join("held");
    assert!(
        held.is_dir() && walk(&held) > 0,
        "the files wait for a Redo"
    );
    state.add_clothes(drop_files(
        dir.path(),
        &[(&format!("{C}^hair_000_u.ydd"), b"hair")],
    ));
    until(&mut state, "the new drop", |s| garments(s) == 1);
    idle(&mut state, "the new drop");
    assert!(
        state.workbench.wardrobe.redo.is_empty(),
        "a new change ends the Redo"
    );
    assert_eq!(walk(&held), 0, "and what was kept for it is deleted");
    assert!(commands::of(&state, Action::RedoClothes)
        .unwrap()
        .enabled
        .is_err());
}

fn walk(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.path().is_dir() {
            true => walk(&e.path()),
            false => 1,
        })
        .sum()
}

#[test]
fn a_redo_over_a_pack_that_changed_is_refused_with_the_reason_and_waits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(a_jacket_and_trousers(dir.path()));
    until(&mut state, "the drop", |s| garments(s) == 2);
    let id = on_disk(&file).garments[0].id.clone();
    state.edit_garment(id.clone(), Edit::Label(Some("first".into())));
    idle(&mut state, "the edit");
    state.undo_last_change();
    idle(&mut state, "the undo");
    assert_eq!(state.workbench.wardrobe.redo.len(), 1);

    // Somebody else writes the pack, outside the Wardrobe's own changes.
    let mut by_hand = on_disk(&file);
    by_hand.garments[1].label = Some("by hand".into());
    by_hand
        .write(&file.parent().unwrap().join("clothing"))
        .unwrap();
    state.toasts.clear();
    state.redo_last_change();
    idle(&mut state, "the refused redo");

    assert_eq!(on_disk(&file), by_hand, "nothing was written");
    assert_eq!(
        state.workbench.wardrobe.redo.len(),
        1,
        "the Redo waits where it was"
    );
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("changed after")),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
}

#[test]
fn edit_undo_and_redo_in_the_wardrobe_join_the_clothes_changes_and_the_project_steps() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _file) = opened(dir.path());
    let ctx = egui::Context::default();
    state.active_workspace = Workspace::Wardrobe;
    state.add_clothes(a_jacket_and_trousers(dir.path()));
    until(&mut state, "the drop", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    std::thread::sleep(Duration::from_millis(20));
    state.record_step("A project change");

    // The project's step is the newer, so the one Undo takes it back first.
    assert_eq!(
        commands::of(&state, Action::Undo).unwrap().label,
        "Undo \"A project change\""
    );
    commands::run_action(&mut state, &ctx, Action::Undo);
    assert!(state.project_can_redo(), "the project step was taken back");
    assert_eq!(garments(&state), 2, "and the clothes were not touched");
    assert!(state.workbench.wardrobe.undo_label().is_some());

    // Then the drop.
    commands::run_action(&mut state, &ctx, Action::Undo);
    until(&mut state, "the drop taken back", |s| garments(s) == 0);
    idle(&mut state, "the drop taken back");

    // Redo makes again in the order Undo took back: the drop, then the project's step.
    assert_eq!(
        state.joined_redo_side(),
        Some(super::workbench::Side::Wardrobe)
    );
    commands::run_action(&mut state, &ctx, Action::Redo);
    until(&mut state, "the drop made again", |s| garments(s) == 2);
    idle(&mut state, "the redo");
    assert!(state.project_can_redo(), "the project's step still waits");
    commands::run_action(&mut state, &ctx, Action::Redo);
    assert!(!state.project_can_redo());
    assert_eq!(state.joined_redo_side(), None);
}

#[test]
fn a_second_window_reads_a_pack_another_window_edits_and_cannot_write_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut first, file) = opened(dir.path());
    first.add_clothes(a_jacket_and_trousers(dir.path()));
    until(&mut first, "the first window's drop", |s| garments(s) == 2);
    idle(&mut first, "the first window's drop");

    let mut second = bare();
    second.load_project_from(file.clone());
    until(&mut second, "the second window reads the pack", |s| {
        garments(s) == 2
    });
    let before = on_disk(&file);
    // It reads the pack, and it knows before any click that another window holds it.
    until(
        &mut second,
        "the second window learns who holds the pack",
        |s| s.wardrobe_locked_out(),
    );
    // ⚠ The window that holds the pack asks too, and must never take its own lock for another window's.
    first.try_pack_lock_again();
    assert!(
        !first.wardrobe_locked_out(),
        "and the window that holds it is not locked out of it"
    );

    second.add_clothes(drop_files(
        dir.path(),
        &[(&format!("{C}^hair_000_u.ydd"), b"hair")],
    ));
    idle(&mut second, "the refused drop");
    assert_eq!(on_disk(&file), before, "the second window wrote nothing");
    assert!(second.wardrobe_locked_out());
    assert!(error_toast(&second).is_some(), "it says why");
    let add = commands::of(&second, Action::AddClothes).unwrap();
    assert!(
        add.enabled
            .is_err_and(|why| why.contains("Another OpenRage window")),
        "and the button says it before the click"
    );

    // The first window lets go; the second asks again and writes.
    drop(first);
    second.try_pack_lock_again();
    assert!(!second.wardrobe_locked_out());
    second.add_clothes(drop_files(
        dir.path(),
        &[(&format!("{C}^hair_000_u.ydd"), b"hair")],
    ));
    until(&mut second, "the second window's drop", |s| {
        garments(s) == 3
    });
}

#[test]
fn a_wardrobe_json_that_does_not_read_offers_its_last_good_version_and_restore_brings_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(a_jacket_and_trousers(dir.path()));
    until(&mut state, "the drop", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    let id = on_disk(&file).garments[0].id.clone();
    state.edit_garment(id, Edit::Label(Some("my jacket".into())));
    idle(&mut state, "the edit");
    let good = on_disk(&file);
    let manifest = file
        .parent()
        .unwrap()
        .join("clothing")
        .join("wardrobe.json");

    std::fs::write(&manifest, b"{ \"garments\": [ oops").unwrap();
    state.workbench.want_scan();
    until(&mut state, "the pack that does not read", |s| {
        s.wardrobe_pack().is_some_and(|p| p.read.is_err())
    });
    let pack = state.wardrobe_pack().unwrap();
    let last = pack.last_good.as_ref().expect("a version of it reads");
    assert_eq!(last.garments, 2);
    assert_eq!(
        commands::of(&state, Action::RestoreLastGood)
            .unwrap()
            .enabled,
        Ok(())
    );

    commands::run_action(
        &mut state,
        &egui::Context::default(),
        Action::RestoreLastGood,
    );
    idle(&mut state, "the restore");
    assert_eq!(on_disk(&file), good, "the last good pack is back");
    until(&mut state, "the pack reads again", |s| garments(s) == 2);
    let kept = std::fs::read_dir(manifest.parent().unwrap().join(".openrage/unreadable"))
        .unwrap()
        .count();
    assert_eq!(kept, 1, "the text that did not read is kept");
    assert_eq!(
        commands::of(&state, Action::RestoreLastGood)
            .unwrap()
            .enabled,
        Err("The pack reads: there is nothing to put back")
    );
}

#[test]
fn a_rollback_of_one_garment_or_one_texture_leaves_the_others_and_undo_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(a_jacket_and_trousers(dir.path()));
    until(&mut state, "the drop", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    let before = on_disk(&file);
    let (jacket, trousers) = (
        garment_of(&before, "jbib").id.clone(),
        garment_of(&before, "lowr").id.clone(),
    );
    assert_eq!(
        garment_of(&before, "jbib").variations.len(),
        2,
        "the jacket has two textures"
    );

    state.edit_garment(jacket.clone(), Edit::Label(Some("renamed jacket".into())));
    idle(&mut state, "the jacket edit");
    state.edit_garment(
        trousers.clone(),
        Edit::Label(Some("renamed trousers".into())),
    );
    idle(&mut state, "the trousers edit");
    let mut tinted = on_disk(&file);
    tinted
        .garments
        .iter_mut()
        .find(|g| g.id == jacket)
        .unwrap()
        .variations[1]
        .tint = Some("red".into());
    tinted
        .write(&file.parent().unwrap().join("clothing"))
        .unwrap();
    until(&mut state, "the scan", |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|r| r.wardrobe.garment(&jacket))
            .is_some_and(|g| g.variations[1].tint.is_some())
    });

    // The versions the pack kept: the drop is the one before the jacket was renamed.
    state.load_versions();
    let versions = state.workbench.wardrobe.versions.clone().expect("versions");
    let dropped = versions
        .list
        .iter()
        .find(|(_, pack)| *pack == before)
        .map(|(version, _)| version.n)
        .expect("the pack as dropped is a version");

    state.roll_back_variation(jacket.clone(), 1, dropped);
    idle(&mut state, "the texture rollback");
    let now = on_disk(&file);
    assert_eq!(
        garment_of(&now, "jbib").variations[1].tint,
        None,
        "texture b is back"
    );
    assert_eq!(
        garment_of(&now, "jbib").label.as_deref(),
        Some("renamed jacket")
    );
    assert_eq!(
        garment_of(&now, "lowr").label.as_deref(),
        Some("renamed trousers")
    );

    state.roll_back_garment(jacket.clone(), dropped);
    idle(&mut state, "the garment rollback");
    let now = on_disk(&file);
    assert_eq!(
        garment_of(&now, "jbib"),
        garment_of(&before, "jbib"),
        "the jacket is as it was dropped"
    );
    assert_eq!(
        garment_of(&now, "lowr").label.as_deref(),
        Some("renamed trousers"),
        "the trousers keep their change"
    );

    state.undo_last_change();
    idle(&mut state, "the undo");
    assert_eq!(
        garment_of(&on_disk(&file), "jbib").label.as_deref(),
        Some("renamed jacket")
    );
}

#[test]
fn a_click_on_an_earlier_history_row_that_removes_an_imports_sources_asks_first() {
    use or_source::asset::folder_name;
    use or_source::textures;

    use super::importing_game_files_from_the_app::ytd;
    use super::the_project_history::{a_project, until, write};

    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let first = (state.workbench.history().iter())
        .find_map(|e| e.step)
        .unwrap_or(0);
    let picked = dir.path().join("downloads").join("road.ytd");
    write(&picked, &ytd("road", 3));
    state.import_game_files(vec![picked]);
    until(&mut state, "the import", |s| {
        !s.workbench.imports.is_running()
    });
    let source = (file.parent().unwrap().join("sources")).join(folder_name("road", textures::KIND));
    assert!(source.is_dir(), "the import made a source {source:?}");
    let first = (state.workbench.history().iter())
        .find_map(|e| e.step)
        .unwrap_or(first);

    state.roll_back_project_asking(first).unwrap();
    let Some(Question::RollBack { sources, .. }) = state.workbench.asking.clone() else {
        panic!("{:?}", state.workbench.asking.is_some());
    };
    assert_eq!(sources, [source.file_name().unwrap().to_string_lossy()]);
    assert!(source.is_dir(), "nothing is removed before the answer");

    // The answer is the same call the dialog's button makes.
    state.workbench.asking = None;
    state.roll_back_project_to(first).unwrap();
    assert!(!source.exists());
}

#[test]
fn a_click_on_a_step_that_removes_nothing_goes_straight_there_and_redo_leaves_a_missing_file_missing(
) {
    use super::the_project_history::{a_project, script};

    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let first = (state.workbench.history().iter())
        .find_map(|e| e.step)
        .unwrap_or(0);
    let client = script(&folder, "client.lua");
    state.add_files_to_project(vec![client.clone()]);
    assert!(state.sources_a_roll_back_removes(first).is_empty());

    state.roll_back_project_asking(first).unwrap();
    assert!(state.workbench.asking.is_none(), "nothing to ask about");
    assert!(state.project_carried.members.is_empty());

    // The file is gone before the step is made again: Redo goes on, and the member shows missing.
    std::fs::remove_file(&client).unwrap();
    state.redo_project().expect("a redo does not need the file");
    assert_eq!(state.project_carried.members.len(), 1);
    state.workbench.want_scan();
    super::the_project_history::until(&mut state, "the scan", |s| {
        s.workbench
            .rows()
            .iter()
            .any(|row| row.seen == super::workbench::Seen::Missing)
    });
}

#[test]
fn check_before_sharing_names_the_private_folder_a_clothing_pack_keeps_beside_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(a_jacket_and_trousers(dir.path()));
    until(&mut state, "the drop", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    state.edit_garment(
        on_disk(&file).garments[0].id.clone(),
        Edit::Label(Some("renamed".into())),
    );
    idle(&mut state, "the edit");

    state.check_before_sharing();
    until(&mut state, "the check reading the folders", |s| {
        matches!(s.workbench.asking, Some(Question::Sharing { .. }))
    });
    let Some(Question::Sharing { concerns }) = state.workbench.asking.clone() else {
        panic!("the check asks");
    };
    assert!(
        concerns
            .iter()
            .any(|c| c.text.starts_with("clothing/.openrage/")
                && c.text.contains("earlier versions")),
        "{:?}",
        concerns.iter().map(|c| &c.text).collect::<Vec<_>>()
    );
}
