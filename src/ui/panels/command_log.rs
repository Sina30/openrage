//! The commands run in this window, newest first, to read, to run again and to copy as a command line.

use crate::state::AppState;
use crate::ui::commands::{self, Command};
use crate::ui::panels::registry::ids;
use crate::ui::theme;

/// How many commands the log keeps; the oldest goes when one more is run.
const KEPT: usize = 200;

/// A command that was run: what the palette called it, and the command line that does the same where it has one.
#[derive(Clone)]
struct Ran {
    id: String,
    label: String,
    category: String,
    cli: Option<String>,
    /// The document in front when it ran, which is what a command of a file acted on.
    on: Option<String>,
}

/// The commands run in this window, oldest first.
#[derive(Default)]
pub struct CommandLog {
    ran: Vec<Ran>,
}

impl crate::ui::panel_state::PanelState for CommandLog {
    const KEY: &'static str = "command-log";
}

/// Keep `command` as run.
///
/// ⚠ Not the palette, Go to File or the log itself: they lead to a command and are none to run again.
pub fn note(state: &mut AppState, command: &Command) {
    if matches!(
        command.id.as_str(),
        "command_palette" | "go_to_file" | "command_log"
    ) {
        return;
    }
    let on = state.active_document().map(|doc| state.tab_title(doc));
    let log = state.panels.get::<CommandLog>();
    if log.ran.len() >= KEPT {
        log.ran.remove(0);
    }
    log.ran.push(Ran {
        id: command.id.clone(),
        label: command.label.clone(),
        category: command.category.clone(),
        cli: command.cli.clone(),
        on,
    });
}

/// Bring the log forward in the side column of the screen in front, adding it there if absent.
pub fn reveal(state: &mut AppState) {
    let dock = state.workspace_docks.get_mut(state.active_workspace);
    crate::ui::dock_commands::reveal_in_the_side(dock, ids::COMMAND_LOG);
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ran = state.panels.get::<CommandLog>().ran.clone();
    if ran.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(tr!(
                    "Nothing has been run in this window yet. Each command you run is listed here, \
                     with the command line that does the same where there is one."
                ))
                .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
        return;
    }
    let mut again: Option<String> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (at, ran) in ran.iter().enumerate().rev() {
                ui.push_id(at, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{}: {}", ran.category, ran.label));
                        if let Some(on) = &ran.on {
                            ui.label(egui::RichText::new(on).weak());
                        }
                        if ui.small_button(tr!("Run again")).clicked() {
                            again = Some(ran.id.clone());
                        }
                        if let Some(line) = &ran.cli {
                            let copy = ui.small_button(tr!("Copy as command"));
                            if copy.on_hover_text(line.as_str()).clicked() {
                                ui.ctx().copy_text(line.clone());
                            }
                        }
                    });
                    if let Some(line) = &ran.cli {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .small()
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                });
            }
        });
    if let Some(id) = again {
        // ⚠ The command as it is offered now, on what is selected now: the log keeps what was run and not what it was run on.
        match commands::all(state).into_iter().find(|c| c.id == id) {
            Some(command) => match command.enabled {
                Ok(()) => commands::run(state, ui.ctx(), &command),
                Err(why) => state.show_warning_toast(why.to_string()),
            },
            None => state.show_warning_toast(tr!("That command is not offered now").to_string()),
        }
    }
}
