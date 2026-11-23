//! The tools of the dressed ped as a person meets them: each has a name a screen reader can speak, the way of drawing chosen is the one the
//! state keeps, and Hide hair states the scale staged hats state and Undo takes it back.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are.

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::the_wardrobe_operations_on_screen::{
    a_shell_with_two_jackets, click_on, garments, names, put, shows, until,
};
use crate::state::workbench::ViewMode;
use crate::ui::theme;
use or_wardrobe::{hair, Prop, Slot};

const C: &str = "mp_m_freemode_01_mp_m_demo";

#[test]
fn the_tools_of_the_dressed_ped_have_names_and_the_way_of_drawing_chosen_is_the_one_kept() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    for text in ["Draw", "Level", "Bones", "Grid", "Motion", "Pictures"] {
        assert!(
            shows(&shell, text),
            "the tools lack `{text}`: {:?}",
            names(&shell)
        );
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the tools of the dressed ped", &mut found);
    assert!(found.is_empty(), "{found:?}");

    assert_eq!(
        shell.state.workbench.wardrobe.dressed.view,
        ViewMode::Material
    );
    // The label beside the drop-down is named alike: the drop-down is the one that opens.
    let combo = (shell.described.iter())
        .find(|w| w.role == egui::accesskit::Role::ComboBox && w.name.as_deref() == Some("Draw"))
        .expect("the drop-down that chooses how the ped is drawn")
        .bounds
        .center();
    shell.click(combo);
    shell.settled();
    until(&mut shell, "the ways of drawing", |s| shows(s, "Wireframe"));
    click_on(&mut shell, "Wireframe");
    assert_eq!(
        shell.state.workbench.wardrobe.dressed.view,
        ViewMode::Wireframe
    );
}

#[test]
fn hide_hair_states_the_scale_staged_hats_state_for_a_hat_only_and_undo_takes_it_back() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    click_on(&mut shell, "Behaviour");
    assert!(
        !shows(&shell, "Hide hair"),
        "a jacket does not hide the hair: {:?}",
        names(&shell)
    );

    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("the panel is drawn");
    shell.drop_files(
        panel.center(),
        put(
            dir.path(),
            &[
                (&format!("{C}^p_head_000.ydd"), b"a hat"),
                (&format!("{C}^p_head_diff_000_a.ytd"), b"red"),
            ],
        ),
    );
    until(&mut shell, "the hat", |s| garments(s) == 3);
    let hat = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| {
            (read.wardrobe.garments.iter())
                .find(|g| g.slot == Some(Slot::Prop(Prop::Head)))
                .map(|g| g.id.clone())
        })
        .expect("the hat is a head prop");
    let module = shell.state.wardrobe_module_id().unwrap();
    shell.state.show_garment(&module, hat.clone());
    shell.settled();
    click_on(&mut shell, "Behaviour");
    let scale = |shell: &super::frames_of_the_shell::Shell| {
        (shell.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(&hat).and_then(hair::scale_of))
    };
    assert_eq!(scale(&shell), None);

    assert!(shows(&shell, "Hide hair"), "{:?}", names(&shell));
    click_on(&mut shell, "Hide hair");
    until(&mut shell, "the scale", |s| scale(s) == Some(hair::HIDES));
    click_on(&mut shell, "Undo behaviour");
    until(&mut shell, "the undo", |s| scale(s).is_none());
}
