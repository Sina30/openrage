//! Open in offers every workspace: a workspace is a layout and not a kind of content, so none is greyed, and one that cannot show
//! the content says in a sentence why, under its name and again when it is pressed.

use super::the_project_screens_on_screen::{
    a_project_shell, click_nearest, names, right_click_on, shows,
};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

#[test]
fn every_workspace_of_open_in_can_be_pressed_and_the_one_that_shows_nothing_says_why() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let content = names(&shell)
        .into_iter()
        .find(|n| n.starts_with("Files") && n.contains(" assets"))
        .unwrap_or_else(|| panic!("no row for the Files content: {:?}", names(&shell)));

    let at = right_click_on(&mut shell, &content);
    click_nearest(&mut shell, "Open in", at);

    // None of them is greyed, and the Wardrobe, which shows clothing only, says so under its name.
    for ws in shell.state.workspaces_offered() {
        let item = (shell.described.iter())
            .filter(|w| w.name.as_deref() == Some(ws.label()))
            .min_by(|a, b| {
                a.bounds
                    .center()
                    .distance(at)
                    .total_cmp(&b.bounds.center().distance(at))
            })
            .unwrap_or_else(|| panic!("no `{}` in Open in: {:?}", ws.label(), names(&shell)));
        assert!(!item.disabled, "{} is greyed", ws.label());
    }
    assert!(
        shows(&shell, "Only clothing content is shown in the Wardrobe"),
        "{:?}",
        names(&shell)
    );

    // Pressed, it opens and the toast says the same.
    shell.state.toasts.clear();
    // ⚠ Below the click: the screen's tab at the top of the window has the name of the row of the menu.
    let row = (shell.described.iter())
        .filter(|w| w.name.as_deref() == Some("Wardrobe") && w.bounds.center().y > at.y)
        .map(|w| w.bounds.center())
        .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
        .unwrap_or_else(|| panic!("Open in lists no Wardrobe: {:?}", names(&shell)));
    shell.click(row);
    shell.settled();
    assert_eq!(shell.state.active_workspace, Workspace::Wardrobe);
    assert!(
        shell
            .state
            .toasts
            .iter()
            .any(|t| t.message.contains("Wardrobe does not show")
                && t.message
                    .contains("Only clothing content is shown in the Wardrobe")),
        "{:?}",
        shell.state.toasts
    );
}
