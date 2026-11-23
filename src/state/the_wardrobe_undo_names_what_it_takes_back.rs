//! Take back the last change names the change it will take back wherever it is offered: the Wardrobe's header, the Project
//! menu and the command palette all read the one command, and its label is the step's own sentence.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, as the Wardrobe's other cases are.

use or_wardrobe::edit::Edit;
use or_wardrobe::shop_edit::Field;

use super::the_wardrobe_workspace::{drop_files, garments, opened, until, C};
use crate::ui::commands::{self, Action};

#[test]
fn the_label_of_undo_is_the_change_it_takes_back_and_says_what_a_redo_makes_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    let label = |state: &super::AppState, action| commands::of(state, action).unwrap().label;
    assert_eq!(label(&state, Action::UndoClothes), "Undo clothes change");

    state.add_clothes(drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    ));
    until(&mut state, "the drop", |s| garments(s) == 1);
    until(&mut state, "the drop to settle", |s| {
        s.workbench.wardrobe.is_idle()
    });
    let id = state
        .wardrobe_pack()
        .unwrap()
        .read
        .as_ref()
        .unwrap()
        .wardrobe
        .garments[0]
        .id
        .clone();

    state.edit_garment(id.clone(), Edit::Shop(0, Field::Cost(Some("250".into()))));
    until(&mut state, "the price", |s| s.workbench.wardrobe.is_idle());
    let step = state
        .workbench
        .wardrobe
        .undo_label()
        .expect("a step")
        .to_string();
    assert!(step.contains("price"), "{step}");

    // The Project menu draws this command and the palette lists it.
    let undo = label(&state, Action::UndoClothes);
    assert_eq!(undo, format!("Undo \"{step}\""));
    let palette = commands::all(&state);
    assert!(
        palette.iter().any(|c| c.label == undo),
        "the palette lists it by the change it takes back"
    );

    state.undo_last_change();
    until(&mut state, "the undo", |s| s.workbench.wardrobe.is_idle());
    let next = state
        .workbench
        .wardrobe
        .undo_label()
        .expect("the drop is a step still")
        .to_string();
    assert_ne!(next, step);
    assert_eq!(
        label(&state, Action::UndoClothes),
        format!("Undo \"{next}\""),
        "the step before it is the next one it names"
    );
    let redo = label(&state, Action::RedoClothes);
    assert!(
        redo.starts_with("Redo \"") && redo.contains("price"),
        "{redo}"
    );
}
