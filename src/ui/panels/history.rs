//! The History panel: the edits of the document the focused panel shows, oldest first, with where it stands marked; a click goes back or forward to one.

use egui_phosphor::regular as ph;

use crate::state::AppState;
use crate::ui::commands::Action;
use crate::ui::theme;

/// One position of the history: the step that leads to it, or the base.
struct Row {
    id: u64,
    label: String,
    /// Undone, so redone by a click.
    undone: bool,
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    // ⚠ The Project workspace is about the project, not about the document last clicked.
    if state.active_workspace.needs_a_project() {
        project(ui, state);
        return;
    }
    let shown = crate::ui::commands::focused_document(state).and_then(|doc| state.document(doc));
    let Some(doc) = shown else {
        theme::empty_message(ui, tr!("Open a file to see its edits"));
        return;
    };
    let (doc_id, history) = (doc.id, &doc.history);
    let base = Row {
        id: history.base(),
        label: if history.base() == 0 {
            tr!("Opened {file_name}", file_name = doc.file_name())
        } else {
            tr!("Earlier edits").to_string()
        },
        undone: false,
    };
    let rows: Vec<Row> = std::iter::once(base)
        .chain(history.done().iter().map(|step| Row {
            id: step.id,
            label: step.label.clone(),
            undone: false,
        }))
        .chain(history.undone().map(|step| Row {
            id: step.id,
            label: step.label.clone(),
            undone: true,
        }))
        .collect();
    let (at, saved, empty) = (history.at(), history.saved(), rows.len() == 1);
    let refused = history.refused().map(str::to_owned);

    let mut target = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for row in &rows {
                let mut text = egui::RichText::new(&row.label);
                if row.undone {
                    // Italic as well as dim: the dim colour is too near the others' to say it alone.
                    text = text.italics().color(theme::TEXT_DIM);
                }
                ui.horizontal(|ui| {
                    if ui.selectable_label(row.id == at, text).clicked() {
                        target = Some(row.id);
                    }
                    if row.id == saved {
                        ui.label(egui::RichText::new(ph::FLOPPY_DISK).color(theme::TEXT_SECONDARY))
                            .on_hover_text(tr!(
                                "The file on disk holds the document as it stands here"
                            ));
                    }
                });
            }
            if let Some(refused) = &refused {
                ui.add(egui::Label::new(egui::RichText::new(refused).color(theme::WARNING)).wrap());
            }
            if empty {
                let undo = crate::ui::shortcuts::label_for(Action::Undo, &state.prefs)
                    .map(|key| tr!(", and Undo ({key}) steps back one", key = key))
                    .unwrap_or_default();
                ui.add_space(theme::SPACING_DEFAULT);
                ui.label(
                    egui::RichText::new(tr!(
                        "Each edit of this file is listed here: a click goes back or forward \
                         to it{undo}.",
                        undo = undo
                    ))
                    .small()
                    .color(theme::TEXT_SECONDARY),
                );
            }
        });
    if let Some(target) = target {
        state.go_to_step(doc_id, target);
    }
}

/// The events newest first, each with the words it is listed in: a run of compiles one after another is one line, `Compiled 5 sources`, so the steps among them are not buried.
fn merged(
    events: &[crate::state::workbench::Event],
) -> Vec<(&crate::state::workbench::Event, String)> {
    let mut lines: Vec<(&crate::state::workbench::Event, String)> = Vec::new();
    let mut run = 0usize;
    for event in events.iter().rev() {
        let compiled = event.step.is_none() && event.text.starts_with("Compiled ");
        if compiled && run > 0 {
            run += 1;
            if let Some(last) = lines.last_mut() {
                last.1 = tr!("Compiled {run} sources", run = run);
            }
            continue;
        }
        run = usize::from(compiled);
        lines.push((event, event.text.clone()));
    }
    lines
}

/// What happened to the project, newest first: each change is a step a click goes back or forward to, and each build and compile is a line.
fn project(ui: &mut egui::Ui, state: &mut AppState) {
    if state.project_path.is_none() {
        theme::empty_message(ui, tr!("No project is open"));
        return;
    }
    let events = state.workbench.history().to_vec();
    if events.is_empty() {
        ui.label(
            egui::RichText::new(tr!(
                "Each change to what the project holds is a step here that a click goes back to, \
                 and each build and recook is listed."
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        return;
    }
    let (here, saved) = (state.workbench.step_here(), state.workbench.step_saved());
    let mut target = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (event, said) in merged(&events) {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::state::prefs::ago(event.at))
                            .small()
                            .color(theme::TEXT_SECONDARY),
                    );
                    let Some(step) = event.step else {
                        // ⚠ Level with the text of a step, which a button's padding sets in.
                        ui.add_space(ui.spacing().button_padding.x);
                        ui.add(egui::Label::new(said).wrap());
                        return;
                    };
                    // ⚠ A dot marks the lines a click goes back to, so the lines that only say what happened are not taken for steps.
                    ui.label(
                        egui::RichText::new(ph::CIRCLE)
                            .small()
                            .color(theme::TEXT_SECONDARY),
                    );
                    let mut text = egui::RichText::new(said);
                    if here.is_some_and(|here| step > here) {
                        // Italic as well as dim: the dim colour is too near the others' to say it alone.
                        text = text.italics().color(theme::TEXT_DIM);
                    }
                    let tip = if Some(step) == here {
                        tr!("The project stands here")
                    } else {
                        tr!("Go back or forward to here: the steps between are undone or made again")
                    };
                    if ui
                        .selectable_label(Some(step) == here, text)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        target = Some(step);
                    }
                    if Some(step) == saved {
                        ui.label(egui::RichText::new(ph::FLOPPY_DISK).color(theme::TEXT_SECONDARY))
                            .on_hover_text(
                                tr!("The project file on disk holds the project as it stands here"),
                            );
                    }
                });
            }
        });
    if let Some(step) = target {
        // ⚠ A click on a row that would remove an import's source folders asks first; a Ctrl+Z does not, as it takes back one step it just showed.
        if let Err(why) = state.roll_back_project_asking(step) {
            state.show_warning_toast(why);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::merged;
    use crate::state::workbench::Event;

    fn event(text: &str, step: Option<u64>) -> Event {
        Event {
            at: SystemTime::now(),
            text: text.to_string(),
            step,
        }
    }

    #[test]
    fn a_run_of_compiles_is_one_line_and_the_steps_between_the_runs_stay_lines_of_their_own() {
        let events = [
            event("The project as it was opened", Some(1)),
            event("Compiled a.textures.src", None),
            event("Compiled b.textures.src", None),
            event("Compiled c.textures.src", None),
            event("Imported 4 files: 4 became sources", Some(2)),
            event("Compiled d.textures.src", None),
            event("Compiled e.textures.src", None),
        ];
        let said: Vec<String> = merged(&events).into_iter().map(|(_, text)| text).collect();
        assert_eq!(
            said,
            [
                "Compiled 2 sources",
                "Imported 4 files: 4 became sources",
                "Compiled 3 sources",
                "The project as it was opened",
            ]
        );
        // One compile stays as it is said.
        let one = [event("Compiled a.textures.src", None)];
        assert_eq!(merged(&one)[0].1, "Compiled a.textures.src");
    }
}
