//! Settings ▸ Games: what a launch opens besides the install.

use crate::state::AppState;

/// Whether a launch reopens the project that was open when OpenRage last ran.
pub fn draw_launch_section(ui: &mut egui::Ui, state: &mut AppState) {
    let mut reopen = state.prefs.reopen_last_project;
    let ticked = ui
        .checkbox(&mut reopen, tr!("Reopen the last project at launch"))
        .on_hover_text(tr!(
            "A launch with no file named opens the project that was open when OpenRage last \
             ran, on the workspace it was left on. Off, it opens the install, and the start \
             page is one action away: View ▸ Start page."
        ));
    if ticked.changed() {
        state.prefs.reopen_last_project = reopen;
        crate::state::save_prefs(&state.prefs);
    }
}
