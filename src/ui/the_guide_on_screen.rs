//! The Workbench guide as it is drawn: a window the Help menu and the start page open, and the way to it from a first launch.
//!
//! Read through AccessKit from whole frames of the shell, and clicked where the names are.

use super::frames_of_the_shell::key;
use super::the_front_door_on_screen::{a_first_launch, click_on, names, shows};
use crate::ui::commands::{self, Action};
use crate::ui::panels::guide;

#[test]
fn the_guide_has_the_sections_the_controls_lead_to() {
    let headings = guide::headings();
    assert!(headings.len() >= 5, "{headings:?}");
}

#[test]
fn the_help_menu_opens_the_guide_from_the_top_and_escape_closes_it() {
    let mut shell = a_first_launch();
    assert!(
        !shows(&shell, "Drop the files or the folder on the Wardrobe"),
        "the guide is not open yet: {:?}",
        names(&shell)
    );

    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::OpenGuide);
    shell.settled();

    assert!(
        shows(&shell, "Add your clothes"),
        "the guide's own headings are drawn: {:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "Drop the files or the folder on the Wardrobe"),
        "its text is the document's"
    );

    shell.draw(vec![key(egui::Key::Escape)]);
    shell.settled();

    assert!(
        shell.state.front_door.guide.is_none(),
        "Escape left the guide open"
    );
    assert!(!shows(
        &shell,
        "Drop the files or the folder on the Wardrobe"
    ));
}

#[test]
fn a_first_launch_has_one_way_to_the_guide_and_it_opens_it_from_the_top() {
    let mut shell = a_first_launch();
    let way = (shell.described.iter())
        .filter(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.ends_with("Read the guide"))
        })
        .count();
    assert_eq!(way, 1, "{:?}", names(&shell));
    click_on(&mut shell, "Read the guide");
    let at = shell
        .state
        .front_door
        .guide
        .as_ref()
        .map(|g| g.section.as_str());
    assert_eq!(at, Some(""));
    assert!(shows(
        &shell,
        "Drop the files or the folder on the Wardrobe"
    ));
}
