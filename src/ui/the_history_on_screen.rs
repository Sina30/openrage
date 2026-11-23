//! The project's History as it is drawn: a line for each step, the one the project stands at selected, the steps
//! that were undone dim, and a click on a step goes back or forward to it.

use or_project::OutputKind;

use super::the_project_screens_on_screen::{a_project_shell, names, shows};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;
use crate::ui::theme;

#[test]
fn a_click_on_a_step_of_the_history_goes_back_to_it_and_a_click_on_a_later_one_goes_forward() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell
        .state
        .add_build("police_dlc", OutputKind::SingleplayerDlc, None)
        .unwrap();
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::HISTORY);
    shell.settled();
    let builds = |shell: &super::frames_of_the_shell::Shell| {
        shell.state.project_carried.container.outputs.len()
    };
    let with = builds(&shell);
    assert!(
        shows(&shell, "Added the build police_dlc"),
        "{:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "The project as it was opened"),
        "{:?}",
        names(&shell)
    );

    click_on(&mut shell, "The project as it was opened");
    assert_eq!(
        builds(&shell),
        with - 1,
        "the click went back past the build"
    );
    assert!(
        shows(&shell, "Added the build police_dlc"),
        "the step it undid is still listed, dim, to go forward to"
    );

    click_on(&mut shell, "Added the build police_dlc");
    assert_eq!(builds(&shell), with, "and forward again");
}

fn click_on(shell: &mut super::frames_of_the_shell::Shell, name: &str) {
    let widget = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    shell.click(at);
    shell.settled();
}
