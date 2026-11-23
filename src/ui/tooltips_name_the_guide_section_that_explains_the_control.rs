//! A control the Workbench guide explains says which section does in its tooltip, and Shift+F1 with the pointer on it opens the guide there.

use super::frames_of_the_shell::Shell;
use super::guide_links::{self, ADD_YOUR_CLOTHES, BUILD, CHECK_ON_THE_PED, FIX_IT};
use super::the_project_screens_on_screen::{a_project_shell, centre_of};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

/// Shift+F1 pressed with the pointer at `at`.
fn shift_f1_over(shell: &mut Shell, at: egui::Pos2) {
    let press = egui::Event::Key {
        key: egui::Key::F1,
        physical_key: Some(egui::Key::F1),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    };
    shell.draw(vec![egui::Event::PointerMoved(at)]);
    shell.draw(vec![egui::Event::PointerMoved(at), press]);
}

#[test]
fn every_section_a_control_leads_to_is_a_heading_of_the_guide() {
    let headings = crate::ui::panels::guide::headings();
    for section in [ADD_YOUR_CLOTHES, FIX_IT, CHECK_ON_THE_PED, BUILD] {
        assert!(headings.contains(&section), "{section} is no heading");
    }
}

#[test]
fn the_commands_the_guide_explains_end_their_tooltip_with_its_section() {
    for (id, section) in [
        ("add_clothes", ADD_YOUR_CLOTHES),
        ("fix_all_safe", FIX_IT),
        ("optimise_textures", FIX_IT),
        ("build_project", BUILD),
    ] {
        assert_eq!(guide_links::section_of(id), Some(section), "{id}");
    }
    assert_eq!(guide_links::section_of("exit"), None);
    assert!(guide_links::hover("Does a thing", FIX_IT).ends_with(&guide_links::line(FIX_IT)));
}

#[test]
fn shift_f1_on_a_tool_of_the_dressed_ped_opens_the_guide_where_it_is_explained() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Wardrobe;
    shell.settled();
    assert!(shell.state.front_door.guide.is_none());

    // F1 alone is the command palette's and with no pointer on a control opens no guide.
    let before = centre_of(&shell, "Before");
    shell.draw(vec![egui::Event::PointerMoved(egui::pos2(2.0, 2.0))]);
    assert!(shell.state.front_door.guide.is_none());

    shift_f1_over(&mut shell, before);

    let guide = shell
        .state
        .front_door
        .guide
        .as_ref()
        .expect("the guide opens");
    assert_eq!(guide.section, CHECK_ON_THE_PED);
}
