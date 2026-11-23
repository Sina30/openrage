//! The banner over an opened folder that offers to make it a project, and moves nothing.

use crate::state::front_door::{Offer, Preview};
use crate::state::AppState;
use crate::ui::commands::{self, Action};
use crate::ui::theme;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState, offer: &Offer) {
    ui.horizontal(|ui| match offer {
        Offer::Make(folder) => {
            ui.label(
                egui::RichText::new(tr!("This folder is not a project."))
                    .color(theme::TEXT_SECONDARY),
            );
            // ⚠ Read off the UI thread: a folder of a whole pack has thousands of files.
            let headline = state.front_door.previews.of(folder).map(Preview::headline);
            if state.front_door.previews.is_reading() {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            ui.label(
                egui::RichText::new(headline.unwrap_or_else(|| tr!("Reading the folder…").into()))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            if ui
                .button(tr!("Make this a project"))
                .on_hover_text(tr!(
                    "Write a project file in the folder and open it. Nothing in the folder is \
                     moved or rewritten; a .gitignore and a .gitattributes are added where the \
                     folder has neither, and a clothing pack in it becomes clothing content, with \
                     a wardrobe.json beside its files. What could not be placed is listed in \
                     the project's Properties"
                ))
                .clicked()
            {
                commands::run_action(state, ui.ctx(), Action::MakeFolderAProject);
            }
            if ui.button(tr!("Not now")).clicked() {
                state.decline_adoption(folder);
            }
        }
        Offer::Open(project) => {
            let name = project
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            ui.label(
                egui::RichText::new(tr!("This folder holds the project {name}.", name = name))
                    .color(theme::TEXT_SECONDARY),
            );
            if ui.button(tr!("Open project")).clicked() {
                state.open_project_asking(project.clone());
            }
            if ui.button(tr!("Not now")).clicked() {
                if let Some(folder) = project.parent() {
                    state.decline_adoption(folder);
                }
            }
        }
    });
}
