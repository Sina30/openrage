//! The controls this wave adds to the Wardrobe as a person meets them: the sets of alternate variations in force, the view from the head,
//! the daylight of the Shaded view, and the places held among a garment's textures. Each has a name a screen reader can speak and does
//! what it says to the state.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are; the file dialogs are the stand-ins of a
//! test build, which answer with the path a case names.

use or_wardrobe::dress::Sets;

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::the_wardrobe_operations_on_screen::{
    a_shell_with_two_jackets, click_on, names, put, shows, until,
};
use crate::platform::file_dialogs;
use crate::state::workbench::{AlternatesEdit, ViewMode};
use crate::ui::theme;

#[test]
fn the_alternates_the_first_person_view_and_the_daylight_are_named_and_do_what_they_say() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    for text in ["First person", "Light"] {
        assert!(
            shows(&shell, text),
            "the tools lack `{text}`: {:?}",
            names(&shell)
        );
    }
    assert!(
        !shows(&shell, "Alternates"),
        "a pack with no set has nothing to choose among"
    );

    // A set of the pack is offered, and what it is set to is the state's.
    shell.state.edit_alternates(AlternatesEdit::New {
        ped: or_wardrobe::Ped::Male,
        name: "Hat and hair".into(),
        join: None,
    });
    until(&mut shell, "the set", |s| shows(s, "Alternates"));
    assert_eq!(shell.state.workbench.wardrobe.dressed.sets, Sets::Worn);
    let combo = (shell.described.iter())
        .find(|w| {
            w.role == egui::accesskit::Role::ComboBox && w.name.as_deref() == Some("Alternates")
        })
        .expect("the drop-down that chooses the sets")
        .bounds
        .center();
    shell.click(combo);
    shell.settled();
    until(&mut shell, "the choices", |s| shows(s, "Hat and hair"));
    assert!(
        shows(&shell, "As worn") && shows(&shell, "None"),
        "{:?}",
        names(&shell)
    );
    click_on(&mut shell, "None");
    assert_eq!(shell.state.workbench.wardrobe.dressed.sets, Sets::Off);

    // The view from the head needs the ped's skeleton, which the install gives: with none it cannot be turned on.
    let first_person = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("First person"))
        .expect("the check box");
    assert!(first_person.disabled, "no skeleton, no head to stand at");
    assert!(!shell.state.workbench.wardrobe.dressed.first_person);

    // The daylight lights the Shaded view and no other: a plain view has no shadows to set, and the Shaded view has them to set.
    click_on(&mut shell, "Light");
    assert!(!shows(&shell, "Shadows"), "{:?}", names(&shell));
    assert!(shell.state.workbench.wardrobe.dressed.shadows);
    shell.state.workbench.wardrobe.dressed.view = ViewMode::Shaded;
    shell.settled();
    assert!(shows(&shell, "Shadows") && shows(&shell, "Time of day"));
    let shadows = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Shadows"))
        .expect("the check box");
    assert!(!shadows.disabled, "the view is Shaded");
    click_on(&mut shell, "Shadows");
    assert!(
        !shell.state.workbench.wardrobe.dressed.shadows,
        "the click took the shadows off"
    );

    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the daylight of the dressed ped", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_texture_place_is_held_from_the_garment_panel_filled_from_its_row_and_each_is_one_click_from_undo(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    // The second jacket has one texture, so that its places are in sight in the panel.
    let module = shell.state.wardrobe_module_id().unwrap();
    let second = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| read.wardrobe.garments[1].id.clone())
        .unwrap();
    shell.state.show_garment(&module, second);
    shell.settled();
    let textures = |shell: &super::frames_of_the_shell::Shell| {
        (shell.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .map(|read| {
                read.wardrobe.garments[1]
                    .variations
                    .iter()
                    .map(|v| v.texture.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    assert!(shows(&shell, "Add an empty texture"), "{:?}", names(&shell));
    let last = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Move the empty textures to the end"))
        .expect("the button");
    assert!(last.disabled, "no place is held, so none is out of order");

    click_on(&mut shell, "Add an empty texture");
    until(&mut shell, "the place", |s| textures(s).len() == 2);
    assert_eq!(textures(&shell)[1], None);
    until(&mut shell, "its row", |s| shows(s, "Fill texture b"));
    assert!(
        !shows(&shell, "Replace texture b"),
        "a place is filled, not replaced"
    );

    // Filling it takes a file and keeps the letter.
    // The toast of the last step stands over the panel; the click is on the button, not on it.
    shell.state.toasts.clear();
    shell.settled();
    let picked = put(dir.path(), &[("fill.ytd", b"a fill")]).remove(0);
    file_dialogs::answer_next(picked);
    click_on(&mut shell, "Fill texture b");
    until(&mut shell, "the picker", |s| shows(s, "Choose the texture"));
    click_on(&mut shell, "Browse…");
    until(&mut shell, "the question", |s| {
        shows(s, "Only reference it")
    });
    click_on(&mut shell, "Import into the project");
    until(&mut shell, "the filled place", |s| {
        let held = textures(s);
        held.len() == 2
            && held[1]
                .as_ref()
                .is_some_and(|t| t.ends_with("fill.textures.src"))
    });
    until(&mut shell, "its step", |s| shows(s, "Undo texture"));
    click_on(&mut shell, "Undo texture");
    until(&mut shell, "the place held again", |s| {
        let held = textures(s);
        held.len() == 2 && held[1].is_none()
    });
    until(&mut shell, "its row", |s| shows(s, "Fill texture b"));

    let mut found = std::collections::BTreeMap::new();
    mute(
        &shell,
        "the garment panel with a held texture place",
        &mut found,
    );
    assert!(found.is_empty(), "{found:?}");
}
