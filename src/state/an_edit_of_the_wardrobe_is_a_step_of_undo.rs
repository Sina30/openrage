//! What a person can take back in the Wardrobe: a fix, a waiver and every edit is one step of the one Undo stack with a
//! label that names the change, a second drop of the same folder adds nothing and says how many it left, a texture that
//! is not there offers Locate and Remove, and what a removal or a drop did is said in a sentence that names the files.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, as the Wardrobe's other cases are: no install, no
//! keys, and a worker for each edit that a case waits on.

use std::path::Path;

use or_wardrobe::edit::Edit;
use or_wardrobe::{Component, Ped, Slot, Wardrobe};

use super::the_wardrobe_workspace::{bare, drop_files, garments, opened, until, C};
use super::toasts::ToastAction;
use super::workbench::{Choice, Finding, Fix, Missing, Place, Question};
use super::AppState;

/// The pack of the clothing shown, as the last scan read it.
fn pack(state: &AppState) -> Wardrobe {
    (state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| read.wardrobe.clone())
        .expect("a pack was read")
}

/// The pack as the disk holds it, which is what Undo compares with.
fn on_disk(project: &Path) -> Wardrobe {
    Wardrobe::read(&project.parent().unwrap().join("clothing")).expect("a pack on disk")
}

/// The finding whose fix or waiver is for the rule `code`.
fn finding_of(state: &AppState, code: &str) -> Option<Finding> {
    state
        .workbench
        .findings()
        .iter()
        .find(|f| {
            f.fixes.iter().any(|fix| match fix {
                Fix::Remedy { finding, .. } | Fix::Waive { finding, .. } => finding.code == code,
                _ => false,
            })
        })
        .cloned()
}

fn idle(state: &mut AppState, what: &str) {
    until(state, what, |s| s.workbench.wardrobe.is_idle());
}

/// Two jackets of one model, each with a texture.
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

#[test]
fn undo_after_the_fix_of_a_duplicate_restores_the_garment_the_fix_removed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(two_of_one_jacket(dir.path()));
    until(&mut state, "the duplicate", |s| {
        finding_of(s, "duplicate-content").is_some()
    });
    let before = on_disk(&file);
    let module = state.wardrobe_module_id().unwrap();
    let Some(Fix::Remedy { finding, .. }) = finding_of(&state, "duplicate-content").and_then(|f| {
        (f.fixes.iter())
            .find(|fix| matches!(fix, Fix::Remedy { .. }))
            .cloned()
    }) else {
        panic!("a duplicate has a remedy");
    };

    state.toasts.clear();
    state.fix_finding(module, finding);
    until(&mut state, "the fix", |s| garments(s) == 1);

    // The fix is a step, and its toast has the button that takes it back.
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo fix");
    assert!(
        state
            .toasts
            .iter()
            .any(|t| matches!(t.action, Some(ToastAction::Undo(_)))),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    assert!(state
        .workbench
        .wardrobe
        .undo_label()
        .is_some_and(
            |label| label.starts_with("Fixed: ") && label.contains("ships the same model")
        ));

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 2);
    assert_eq!(
        on_disk(&file),
        before,
        "the garment is back with all it stated"
    );
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        1,
        "only the drop is left to take back"
    );
}

#[test]
fn dropping_one_folder_twice_adds_the_garments_once_and_the_second_toast_says_how_many_were_left() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    let paths = two_of_one_jacket(dir.path());
    state.add_clothes(paths.clone());
    until(&mut state, "the garments", |s| garments(s) == 2);
    idle(&mut state, "the first drop");

    state.toasts.clear();
    state.add_clothes(paths);
    idle(&mut state, "the second drop");

    assert_eq!(garments(&state), 2, "the garments are added once");
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        1,
        "a drop that added nothing has nothing to take back"
    );
    let said: Vec<String> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(
        said.iter()
            .any(|m| m == "Nothing was added: the 2 garments of the drop are already in the pack"),
        "{said:?}"
    );
}

#[test]
fn the_toast_of_a_drop_names_the_first_file_it_left_out_and_why() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    let paths = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            ("mp_m_freemode_01_mp_m_t.ymt", b"metadata"),
        ],
    );
    state.toasts.clear();
    state.add_clothes(paths);
    until(&mut state, "the garment", |s| garments(s) == 1);

    let said: Vec<String> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(
        said.iter().any(|m| m.contains(
            "1 file left out, the first is mp_m_freemode_01_mp_m_t.ymt: a .ymt file: a build writes the metadata"
        )),
        "{said:?}"
    );
}

#[test]
fn a_texture_that_is_not_there_offers_locate_and_remove_and_each_is_a_step_of_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    let paths = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            (&format!("{C}^jbib_diff_000_b_uni.ytd"), b"blue"),
        ],
    );
    state.add_clothes(paths);
    until(&mut state, "the garment", |s| garments(s) == 1);
    idle(&mut state, "the drop");
    let before = on_disk(&file);
    let gone = file
        .parent()
        .unwrap()
        .join("clothing")
        .join(before.garments[0].variations[1].texture.as_deref().unwrap());
    std::fs::remove_dir_all(&gone).unwrap();
    state.workbench.want_scan();
    let missing = |s: &AppState| {
        (s.workbench.findings().iter())
            .find(|f| {
                f.fixes
                    .iter()
                    .any(|fix| matches!(fix, Fix::FindFile { .. }))
            })
            .cloned()
    };
    until(&mut state, "the finding", |s| missing(s).is_some());

    let finding = missing(&state).unwrap();
    let labels: Vec<&str> = finding.fixes.iter().map(Fix::label).collect();
    assert_eq!(
        labels,
        ["Locate…", "Remove this texture"],
        "no waiver: no reason mends a file that is not there"
    );
    assert!(
        matches!(
            &finding.fixes[0],
            Fix::FindFile {
                what: Missing::Texture(1, _),
                ..
            }
        ),
        "{:?}",
        finding.fixes
    );
    assert!(matches!(finding.place, Place::Garment { id: Some(_), .. }));

    // Locate… asks, in the picker, for the file, and points the texture at the source it becomes.
    let found = dir.path().join("elsewhere").join("blue.ytd");
    std::fs::create_dir_all(found.parent().unwrap()).unwrap();
    let blue = or_source::sample::textures_of("blue, found");
    std::fs::write(&found, &blue).unwrap();
    state.ask_about_fix(&finding.fixes[0]);
    let Some(Question::Pick(picker)) = state.workbench.asking.clone() else {
        panic!("Locate opens the picker: {:?}", state.workbench.asking);
    };
    state.choose(picker.wanting, Choice::Outside(found.clone()));
    state.answer_import_or_reference(true);
    idle(&mut state, "the locate");
    until(&mut state, "the finding to go", |s| missing(s).is_none());
    let located = on_disk(&file);
    let now = file.parent().unwrap().join("clothing").join(
        located.garments[0].variations[1]
            .texture
            .as_deref()
            .unwrap(),
    );
    assert_eq!(
        or_source::asset::Source::open(&now)
            .unwrap()
            .manifest
            .origin
            .sha256,
        Some(or_source::asset::sha256_hex(&blue)),
        "the file found is decompiled, not copied"
    );
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo locate");

    state.undo_last_change();
    idle(&mut state, "the undo");
    assert_eq!(
        on_disk(&file),
        before,
        "the texture is where it was, not there"
    );
    assert!(!now.exists(), "the copy the locate made goes with it");
    until(&mut state, "the finding again", |s| missing(s).is_some());

    // Remove takes the texture off the garment, and Undo puts it back.
    let finding = missing(&state).unwrap();
    state.ask_about_fix(&finding.fixes[1]);
    until(&mut state, "the removal", |s| {
        (s.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
            .is_some_and(|read| read.wardrobe.garments[0].variations.len() == 1)
    });
    idle(&mut state, "the removal");
    assert_eq!(
        state.workbench.wardrobe.undo_words(),
        "Undo texture removal"
    );
    assert!(missing(&state).is_none() || state.workbench.is_scanned());
    state.undo_last_change();
    idle(&mut state, "the undo");
    assert_eq!(on_disk(&file), before);
}

#[test]
fn every_edit_of_a_garment_a_waiver_a_shop_id_and_the_pack_is_a_step_that_names_what_it_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(two_of_one_jacket(dir.path()));
    until(&mut state, "the duplicate", |s| {
        finding_of(s, "duplicate-content").is_some()
    });
    idle(&mut state, "the drop");
    let started = on_disk(&file);
    let first = started.garments[0].id.clone();
    let second = started.garments[1].id.clone();
    let module = state.wardrobe_module_id().unwrap();

    let mut steps: Vec<(&str, String)> = Vec::new();
    let mut make = |state: &mut AppState, noun: &'static str, change: &dyn Fn(&mut AppState)| {
        change(state);
        idle(state, noun);
        steps.push((
            noun,
            state
                .workbench
                .wardrobe
                .undo_label()
                .unwrap_or_default()
                .to_string(),
        ));
        assert_eq!(
            state.workbench.wardrobe.undo_words(),
            format!("Undo {noun}")
        );
    };
    make(&mut state, "slot", &|s| {
        s.edit_garment(
            first.clone(),
            Edit::Slot(Some(Slot::Component(Component::Lowr))),
        )
    });
    make(&mut state, "ped", &|s| {
        s.edit_garment(first.clone(), Edit::Ped(Some(Ped::Female)))
    });
    make(&mut state, "place", &|s| {
        s.edit_garment(second.clone(), Edit::Position(Some(9)))
    });
    make(&mut state, "name", &|s| {
        s.edit_garment(second.clone(), Edit::Label(Some("the red one".into())))
    });
    make(&mut state, "skin", &|s| {
        s.edit_garment(second.clone(), Edit::Skin(or_wardrobe::Skin::Race))
    });
    make(&mut state, "shop id", &|s| {
        s.set_shop_id(second.clone(), 0, Some("HARBOR_RED".into()))
    });
    make(&mut state, "waiver", &|s| {
        s.waive_finding(
            module.clone(),
            "duplicate-content".into(),
            Some(second.clone()),
            "the same mesh on purpose".into(),
        )
    });
    make(&mut state, "waiver removal", &|s| s.remove_waiver(0));
    make(&mut state, "pack name", &|s| s.rename_pack("harbor".into()));
    make(&mut state, "shop names", &|s| s.set_durty_hashes(true));

    // Each label names the change and, where it is about one, the garment.
    let labels: Vec<&str> = steps.iter().map(|(_, l)| l.as_str()).collect();
    assert!(labels[0].starts_with("Changed the slot of "), "{labels:?}");
    assert!(
        labels[2].starts_with("Pinned ") && labels[2].ends_with(" at 009"),
        "{labels:?}"
    );
    assert!(labels[3].starts_with("Changed the name of "), "{labels:?}");
    assert!(
        labels[6].starts_with(
            "Waived \"two garments of one type ship the same model and the same textures\" for "
        ),
        "{labels:?}"
    );
    assert_eq!(labels[8], "Renamed the pack to harbor");
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        11,
        "the drop and the ten edits"
    );
    assert_ne!(on_disk(&file), started);

    // Taken back from the newest, they leave the pack as it was before the first.
    for noun in steps.iter().rev().map(|(noun, _)| *noun) {
        assert_eq!(
            state.workbench.wardrobe.undo_words(),
            format!("Undo {noun}")
        );
        state.undo_last_change();
        idle(&mut state, "an undo");
    }
    assert_eq!(on_disk(&file), started);
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        1,
        "only the drop is left to take back"
    );
}

#[test]
fn undo_of_an_edit_the_pack_has_moved_on_from_is_refused_in_a_sentence_and_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(two_of_one_jacket(dir.path()));
    until(&mut state, "the garments", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    let ids: Vec<String> = pack(&state).garments.iter().map(|g| g.id.clone()).collect();
    state.edit_garment(ids[0].clone(), Edit::Position(Some(9)));
    idle(&mut state, "the first edit");
    let older = state
        .workbench
        .wardrobe
        .undo
        .last()
        .map(|s| s.number)
        .unwrap();
    state.edit_garment(ids[1].clone(), Edit::Position(Some(8)));
    idle(&mut state, "the second edit");
    let now = on_disk(&file);
    let held = numbers(&state);
    assert_eq!(held.len(), 3, "the drop and the two edits");

    state.toasts.clear();
    state.undo_change(older);
    idle(&mut state, "the refused undo");

    assert_eq!(on_disk(&file), now, "nothing was lost");
    let said: Vec<String> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(
        said.iter().any(|m| m.contains("take those back first")),
        "{said:?}"
    );
    assert_eq!(
        numbers(&state),
        held,
        "the sentence says to take the later steps back first, so the refused step must still be there to take back"
    );

    // The same toast's button, pressed again, is refused the same way and not told the step is gone.
    state.toasts.clear();
    state.undo_change(older);
    idle(&mut state, "the second refused undo");
    let said: Vec<String> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(
        said.iter().all(|m| !m.contains("taken back already")),
        "{said:?}"
    );
    assert_eq!(numbers(&state), held);

    state.undo_last_change();
    idle(&mut state, "the newest undo");
    assert_eq!(pack_pinned(&on_disk(&file)), [Some(9), None]);

    // Later steps taken back, the step that was refused takes back.
    state.undo_change(older);
    idle(&mut state, "the older undo");
    assert_eq!(pack_pinned(&on_disk(&file)), [None, None]);
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        1,
        "only the drop is left"
    );
}

/// A drop that cannot be taken back is refused with the steps that went with it still on the stack.
#[test]
fn a_refused_undo_of_a_drop_keeps_the_drop_and_the_removal_of_its_garment_on_the_stack() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(two_of_one_jacket(dir.path()));
    until(&mut state, "the garments", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    let first = pack(&state).garments[0].id.clone();
    state.edit_garment(first, Edit::Remove);
    until(&mut state, "the removal", |s| garments(s) == 1);
    idle(&mut state, "the removal");
    let held = numbers(&state);
    assert_eq!(held.len(), 2, "the drop and the removal");

    // The pack's record cannot be read, so the edit that takes the drop back fails.
    let record = file
        .parent()
        .unwrap()
        .join("clothing")
        .join(or_wardrobe::model::FILE);
    let kept = std::fs::read(&record).unwrap();
    std::fs::write(&record, b"not a pack").unwrap();
    state.toasts.clear();
    state.undo_change(held[0]);
    idle(&mut state, "the refused undo");
    std::fs::write(&record, kept).unwrap();

    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.toast_type == super::toasts::ToastType::Error),
        "the refusal is said"
    );
    assert_eq!(
        numbers(&state),
        held,
        "both steps are still there, in order"
    );
}

/// The numbers of the steps Undo holds, oldest first.
fn numbers(state: &AppState) -> Vec<u64> {
    (state.workbench.wardrobe.undo.iter())
        .map(|s| s.number)
        .collect()
}

fn pack_pinned(w: &Wardrobe) -> Vec<Option<u32>> {
    w.garments.iter().map(|g| g.position).collect()
}

#[test]
fn removing_a_garment_says_which_garments_took_another_place() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    let names: Vec<(String, &[u8])> = (0..3)
        .flat_map(|i| {
            [
                (format!("{C}^jbib_{i:03}_u.ydd"), b"jacket" as &[u8]),
                (format!("{C}^jbib_diff_{i:03}_a_uni.ytd"), b"red" as &[u8]),
            ]
        })
        .collect();
    let as_refs: Vec<(&str, &[u8])> = names.iter().map(|(n, b)| (n.as_str(), *b)).collect();
    // Three different jackets, so the pack holds no duplicate to say anything else.
    let bodies: Vec<Vec<u8>> = (0..3)
        .map(|i| or_source::sample::drawables_of(&format!("jacket {i}")))
        .collect();
    let mut dropped = drop_files(dir.path(), &as_refs);
    for (i, path) in dropped.iter_mut().enumerate().filter(|(i, _)| i % 2 == 0) {
        std::fs::write(&path, &bodies[i / 2]).unwrap();
    }
    state.add_clothes(dropped);
    until(&mut state, "the garments", |s| garments(s) == 3);
    idle(&mut state, "the drop");
    let first = pack(&state).garments[0].id.clone();

    state.toasts.clear();
    state.edit_garment(first, Edit::Remove);
    // The scan that reads the pack without the garment can come before the worker's answer, which holds the sentence.
    until(&mut state, "the removal", |s| {
        garments(s) == 2 && s.workbench.wardrobe.is_idle()
    });

    let said: Vec<String> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(
        said.iter().any(|m| m.contains("its files stay")
            && m.contains("2 garments moved: ")
            && m.contains("from 001 to 000")
            && m.contains("from 002 to 001")),
        "{said:?}"
    );
}

#[test]
fn the_sentence_of_a_garments_finding_has_no_rule_code_no_backtick_and_no_repeat_of_the_name_its_row_shows(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    let mut paths = two_of_one_jacket(dir.path());
    paths.extend(drop_files(dir.path(), &[("my cap.ydd", b"a cap")]));
    state.add_clothes(paths);
    until(&mut state, "the findings", |s| {
        finding_of(s, "duplicate-content").is_some()
            && s.workbench
                .findings()
                .iter()
                .any(|f| f.text.contains("no ped"))
    });

    let found: Vec<Finding> = state.workbench.findings().to_vec();
    assert!(found.len() >= 3, "{found:?}");
    for finding in &found {
        assert!(!finding.text.contains('`'), "{}", finding.text);
        assert!(
            !or_wardrobe::RULES
                .iter()
                .any(|r| finding.text.starts_with(r.code)),
            "{}",
            finding.text
        );
        if let Place::Garment { label, .. } = &finding.place {
            assert!(
                !finding.text.starts_with(label.as_str()),
                "the row says {label}, so its sentence does not: {}",
                finding.text
            );
        }
    }
    assert!(
        found.iter().any(|f| f.text == "Has no ped and no slot"),
        "{:?}",
        found.iter().map(|f| &f.text).collect::<Vec<_>>()
    );
}

#[test]
fn a_bare_state_edits_nothing_and_takes_nothing_back() {
    let mut state = bare();
    state.undo_last_change();
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo");
    assert_eq!(state.workbench.wardrobe.undo_label(), None);
}

#[test]
fn a_cloth_the_metadata_expects_or_the_pack_names_and_lacks_offers_locate_beside_the_fix_its_rule_gives(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    let paths = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    );
    state.add_clothes(paths);
    until(&mut state, "the garment", |s| garments(s) == 1);
    idle(&mut state, "the drop");
    let folder = file.parent().unwrap().join("clothing");

    // Its metadata states ownsCloth and no cloth file is linked: Locate, the remedy of its rule, and a waiver.
    let mut w = Wardrobe::read(&folder).unwrap();
    w.garments[0].missing_cloth = true;
    w.write(&folder).unwrap();
    state.workbench.want_scan();
    let cloth = |s: &AppState| {
        (s.workbench.findings().iter())
            .find(|f| {
                f.fixes.iter().any(|fix| {
                    matches!(
                        fix,
                        Fix::FindFile {
                            what: Missing::Cloth,
                            ..
                        }
                    )
                })
            })
            .cloned()
    };
    until(&mut state, "the finding", |s| cloth(s).is_some());
    let labels: Vec<&str> = cloth(&state)
        .unwrap()
        .fixes
        .iter()
        .map(Fix::label)
        .collect();
    assert_eq!(labels, ["Locate…", "Fix…", "Waive…"]);

    // Locate… links the file chosen, and Undo gives the pack back as it was.
    let before = on_disk(&file);
    let found = dir.path().join("elsewhere").join("jacket.yld");
    std::fs::create_dir_all(found.parent().unwrap()).unwrap();
    std::fs::write(&found, b"cloth").unwrap();
    state.ask_about_fix(&cloth(&state).unwrap().fixes[0]);
    let Some(Question::Pick(picker)) = state.workbench.asking.clone() else {
        panic!("Locate opens the picker: {:?}", state.workbench.asking);
    };
    state.choose(picker.wanting, Choice::Outside(found));
    state.answer_import_or_reference(true);
    idle(&mut state, "the link");
    assert_eq!(
        on_disk(&file).garments[0].cloth.as_deref(),
        Some("cloth/jacket.yld")
    );
    assert!(!on_disk(&file).garments[0].missing_cloth);
    state.undo_last_change();
    idle(&mut state, "the undo");
    assert_eq!(on_disk(&file), before);

    // A cloth the pack names and has not got: Locate and Remove, and no waiver, since no reason mends a file that is not there.
    let mut w = Wardrobe::read(&folder).unwrap();
    (w.garments[0].missing_cloth, w.garments[0].cloth) = (false, Some("cloth/gone.yld".into()));
    w.write(&folder).unwrap();
    state.workbench.want_scan();
    until(&mut state, "the named cloth", |s| {
        cloth(s).is_some_and(|f| {
            f.fixes
                .iter()
                .any(|fix| matches!(fix, Fix::DropFile { .. }))
        })
    });
    let named = cloth(&state).unwrap();
    assert_eq!(
        named.fixes.iter().map(Fix::label).collect::<Vec<_>>(),
        ["Locate…", "Remove the cloth"]
    );
    state.ask_about_fix(&named.fixes[1]);
    idle(&mut state, "the unlink");
    assert_eq!(on_disk(&file).garments[0].cloth, None);
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo cloth");
}
