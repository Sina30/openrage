//! How a garment behaves, as a person meets it in the Garment panel: each control has a name a screen reader can speak,
//! a flag ticked is a bit of the garment that Undo takes back, and a set made from the garment is a set of the pack.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are.

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::Shell;
use super::the_wardrobe_operations_on_screen::{
    a_shell_with_two_jackets, click_on, names, shows, until,
};
use crate::ui::theme;
use or_wardrobe::behaviour::Behaviour;
use or_wardrobe::edit::Edit;
use or_wardrobe::flags::{self, Flag};
use or_wardrobe::mask::{self, Property};

fn first_garment(shell: &Shell) -> or_wardrobe::Garment {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| read.wardrobe.garments[0].clone())
        .expect("the pack is read")
}

fn sets(shell: &Shell) -> Vec<or_wardrobe::alternate::AlternateSet> {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| read.wardrobe.alternates.clone())
        .unwrap_or_default()
}

#[test]
fn the_garment_panel_offers_each_behaviour_with_a_name_a_screen_reader_can_speak() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let mut found = std::collections::BTreeMap::new();
    // The behaviour starts closed, and so does each of its sections: each is opened on its own, as a person would, and closed again so the next is in view.
    click_on(&mut shell, "Behaviour");
    for (header, controls) in [
        (
            "Behaviour flags",
            &[
                "Remove in vehicle",
                "Armoured",
                "Hide in first person",
                "Flags as a number",
            ][..],
        ),
        (
            "Drawable properties",
            &[
                "Textures follow the ped's skin",
                "Has transparent parts, drawn last",
            ][..],
        ),
        ("Audio", &["Audio id", "Second audio id"][..]),
    ] {
        click_on(&mut shell, header);
        for text in controls {
            assert!(
                shows(&shell, text),
                "{header} lacks `{text}`: {:?}",
                names(&shell)
            );
        }
        mute(&shell, header, &mut found);
        if header == "Behaviour flags" {
            // The flags no staged document states are not offered by name.
            for flag in Flag::ALL.into_iter().filter(|f| !f.measured()) {
                assert!(
                    !shows(&shell, flag.label()),
                    "{} is not measured",
                    flag.name()
                );
            }
        }
        if header == "Drawable properties" {
            // The pack ships the u or r form of its skin and never the m form, so a mask that does not state the match bit is not offered it.
            assert!(
                !shows(&shell, Property::MatchPrev.label()),
                "{:?}",
                names(&shell)
            );
        }
        click_on(&mut shell, header);
    }
    for text in [
        "First person alternate",
        "Name of a new set of alternate variations",
        "Make a set with this garment",
    ] {
        assert!(
            shows(&shell, text),
            "the Garment panel lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    mute(&shell, "the Garment panel with its behaviour", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_flag_ticked_is_a_bit_of_the_garment_and_undo_takes_it_back() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    assert_eq!(flags::word(&first_garment(&shell)), 0);
    click_on(&mut shell, "Behaviour");
    click_on(&mut shell, "Behaviour flags");
    click_on(&mut shell, "Remove in vehicle");
    until(&mut shell, "the flag", |s| {
        flags::has(&first_garment(s), Flag::NotInCar)
    });
    until(&mut shell, "its step", |s| shows(s, "Undo behaviour"));
    assert_eq!(flags::word(&first_garment(&shell)), Flag::NotInCar.mask());
    click_on(&mut shell, "Undo behaviour");
    until(&mut shell, "the undo", |s| {
        flags::word(&first_garment(s)) == 0
    });
}

#[test]
fn a_stated_match_property_is_offered_with_its_reason_and_can_be_taken_off() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let id = first_garment(&shell).id;
    shell.state.edit_garment(
        id,
        Edit::Behaviour(Behaviour::Property(Property::MatchPrev, true)),
    );
    until(&mut shell, "the property", |s| {
        mask::has_property(&first_garment(s), Property::MatchPrev)
    });
    click_on(&mut shell, "Behaviour");
    click_on(&mut shell, "Drawable properties");
    for text in [Property::MatchPrev.label(), "which a pack never ships"] {
        assert!(shows(&shell, text), "`{text}`: {:?}", names(&shell));
    }
    // The property is under the fold of the panel: on screen for a screen reader, and out of reach of a click until the panel is scrolled.
    let over = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Drawable properties"))
        .expect("the section's heading")
        .bounds
        .center();
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -300.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw(vec![egui::Event::PointerMoved(over), wheel]);
    shell.settled();
    click_on(&mut shell, Property::MatchPrev.label());
    until(&mut shell, "the property taken off", |s| {
        !mask::has_property(&first_garment(s), Property::MatchPrev)
    });
    assert_eq!(first_garment(&shell).meta.prop_mask, None);
    assert!(!shows(&shell, Property::MatchPrev.label()));
}

#[test]
fn a_set_made_from_the_garment_is_a_set_of_the_pack_it_causes_and_undo_takes_it_out() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    assert!(sets(&shell).is_empty());
    // Type the name in its field, then make the set.
    click_on(&mut shell, "Behaviour");
    click_on(&mut shell, "Name of a new set of alternate variations");
    shell.draw(vec![egui::Event::Text("hair under a hood".into())]);
    shell.settled();
    click_on(&mut shell, "Make a set with this garment");
    until(&mut shell, "the set", |s| sets(s).len() == 1);
    let id = first_garment(&shell).id;
    let made = sets(&shell).remove(0);
    assert_eq!(made.name, "hair under a hood");
    assert!(made.has_source(&id));
    until(&mut shell, "its step", |s| shows(s, "Undo alternates"));
    click_on(&mut shell, "Undo alternates");
    until(&mut shell, "the undo", |s| sets(s).is_empty());
}

#[test]
fn the_sets_of_the_pack_are_listed_in_its_settings_with_names_a_screen_reader_can_speak() {
    use crate::state::workbench::AlternatesEdit;
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    shell.state.edit_alternates(AlternatesEdit::New {
        ped: or_wardrobe::Ped::Male,
        name: "hair under a hood".into(),
        join: None,
    });
    until(&mut shell, "the set", |s| sets(s).len() == 1);
    // With no garment selected the panel shows the pack's own settings.
    shell.state.workbench.wardrobe.selected = None;
    shell.settled();
    assert!(shows(&shell, "Alternate variations"), "{:?}", names(&shell));
    let header = names(&shell)
        .into_iter()
        .find(|n| n.starts_with("hair under a hood ("))
        .unwrap_or_else(|| panic!("the set is not listed: {:?}", names(&shell)));
    click_on(&mut shell, &header);
    for text in [
        "Name of the set hair under a hood",
        "Add a switch to hair under a hood",
        "Remove the set hair under a hood",
        "Slot of the drawable that changes",
        "Name of a new set of alternate variations",
    ] {
        assert!(
            shows(&shell, text),
            "the settings lack `{text}`: {:?}",
            names(&shell)
        );
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the pack's settings with a set", &mut found);
    assert!(found.is_empty(), "{found:?}");
    click_on(&mut shell, "Remove the set hair under a hood");
    until(&mut shell, "the removal", |s| sets(s).is_empty());
}
