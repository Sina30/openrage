//! The status bar's count of background jobs, and the list of them it opens.

use crate::state::jobs::{EndedJob, JobKind, Outcome, RunningJob, Then};
use crate::state::navigation::Arrival;
use crate::state::AppState;
use crate::ui::theme;

/// Whether the list of background jobs is open, and where this frame drew it.
#[derive(Default)]
pub struct JobList {
    pub open: bool,
    pub drawn: Option<egui::Rect>,
}

impl crate::ui::panel_state::PanelState for JobList {
    const KEY: &'static str = "job-list";
}

pub fn open(state: &mut AppState) {
    state.panels.get::<JobList>().open = true;
}

/// How tall a spinner is beside a line of small text: its own size would stand over the line and sit off it.
const SPINNER: f32 = 12.0;

/// What a click in the list asked for, applied once it is drawn.
enum Click {
    Cancel(JobKind),
    Then(Then),
    Log,
    Clear,
}

/// How many jobs run, and the list a click opens; nothing until a job has run or the list is asked for.
pub fn draw_indicator(ui: &mut egui::Ui, state: &mut AppState) {
    use egui_phosphor::regular as icon;

    let running = state.running_jobs();
    let list = state.panels.get::<JobList>();
    list.drawn = None;
    let mut open = list.open;
    if running.is_empty() && state.jobs.ended().is_empty() && !open {
        return;
    }
    let (text, colour) = match running.len() {
        0 => (
            tr!("{icon} Jobs", icon = icon::CHECK_CIRCLE),
            theme::TEXT_SECONDARY,
        ),
        1 => (tr!("1 job running").to_string(), theme::INFO),
        n => (tr!("{n} jobs running", n = n), theme::INFO),
    };
    let hover = if running.is_empty() {
        tr!("No background job is running. Click for the ones that finished.").to_string()
    } else {
        let doing: Vec<&str> = running.iter().map(|job| job.doing.as_str()).collect();
        tr!(
            "{doing}\n\nClick for their progress and a Cancel.",
            doing = doing.join("\n")
        )
    };
    let response = ui
        .add(egui::Button::new(egui::RichText::new(text).small().color(colour)).frame(false))
        .on_hover_text(hover);
    if !running.is_empty() {
        ui.add(egui::Spinner::new().size(SPINNER));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    if response.clicked() {
        open = !open;
    }
    let mut click = None;
    let shown = egui::Popup::from_response(&response)
        .id(egui::Id::new("background_jobs"))
        .align(egui::RectAlign::TOP_END)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .open_bool(&mut open)
        .show(|ui| click = draw_list(ui, &running, state.jobs.ended(), Some(440.0)));
    let list = state.panels.get::<JobList>();
    list.open = open;
    list.drawn = shown.map(|shown| shown.response.rect);
    ui.separator();

    handle(state, click);
}

fn handle(state: &mut AppState, click: Option<Click>) {
    match click {
        Some(Click::Cancel(kind)) => state.cancel_job(kind),
        Some(Click::Then(then)) => apply(state, then),
        Some(Click::Log) => crate::ui::panels::log::reveal(state),
        Some(Click::Clear) => state.jobs.clear(),
        None => {}
    }
}

fn draw_list(
    ui: &mut egui::Ui,
    running: &[RunningJob],
    ended: &[EndedJob],
    width: Option<f32>,
) -> Option<Click> {
    if let Some(width) = width {
        ui.set_width(width);
    }
    let mut click = None;
    ui.label(egui::RichText::new(tr!("Background jobs")).strong());
    if running.is_empty() {
        ui.label(egui::RichText::new(tr!("Nothing is running.")).color(theme::TEXT_SECONDARY));
    }
    for job in running {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(&job.doing);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if job.stopping {
                    ui.label(egui::RichText::new(tr!("Stopping…")).color(theme::WARNING))
                        .on_hover_text(tr!("It stops once it has finished what it cannot drop"));
                    return;
                }
                let cancel = ui.button(tr!("Cancel"));
                let name = tr!("Cancel {kind}", kind = job.kind.label());
                cancel.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                });
                if cancel.clicked() {
                    click = Some(Click::Cancel(job.kind));
                }
            });
        });
        match job.fraction {
            Some(fraction) => {
                ui.add(egui::ProgressBar::new(fraction).text(job.detail.as_str()));
            }
            None => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(SPINNER));
                    ui.label(egui::RichText::new(&job.detail).small());
                });
            }
        }
    }
    if ended.is_empty() {
        return click;
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("Finished")).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(tr!("Clear")).clicked() {
                click = Some(Click::Clear);
            }
        });
    });
    for job in ended {
        if let Some(asked) = draw_ended(ui, job) {
            click = Some(asked);
        }
    }
    click
}

fn draw_ended(ui: &mut egui::Ui, job: &EndedJob) -> Option<Click> {
    let (verdict, colour) = match job.outcome {
        Outcome::Done(_) => (tr!("Done"), theme::SUCCESS),
        Outcome::Stopped(_) => (tr!("Stopped"), theme::WARNING),
        Outcome::Failed => (tr!("Failed"), theme::ERROR),
    };
    let mut click = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{}: {verdict}", job.kind.label())).color(colour));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, asked) = match &job.outcome {
                Outcome::Failed => (tr!("Log"), Click::Log),
                Outcome::Done(then) | Outcome::Stopped(then) => {
                    let label = match then {
                        Then::Nothing => return,
                        Then::OpenFolder(_) => tr!("Open folder"),
                        Then::Reveal { .. } => tr!("Show in Files"),
                        Then::Show(_) => tr!("Show"),
                        Then::Panel(_) => tr!("Show results"),
                    };
                    (label, Click::Then(then.clone()))
                }
            };
            if ui.small_button(label).clicked() {
                click = Some(asked);
            }
        });
    });
    ui.add(
        egui::Label::new(
            egui::RichText::new(&job.said)
                .small()
                .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
    click
}

fn apply(state: &mut AppState, then: Then) {
    match then {
        Then::Nothing => {}
        Then::OpenFolder(path) => crate::platform::shell::open_folder(&path),
        Then::Reveal { source, inner } => state.walk_to(&source, vec![inner], Arrival::Reveal),
        Then::Show(id) => state.set_active_document(id),
        Then::Panel(id) => state.workspace_docks.reveal(state.active_workspace, id),
    }
}

/// The Jobs panel: the list the status bar opens, drawn where it can stay.
pub fn draw_panel(ui: &mut egui::Ui, state: &mut AppState) {
    let running = state.running_jobs();
    if !running.is_empty() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    let click = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| draw_list(ui, &running, state.jobs.ended(), None))
        .inner;
    handle(state, click);
}

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;

    use super::*;

    /// Each node a frame described: what it is, what it says, and where it is.
    fn drawn(
        ctx: &egui::Context,
        mut draw: impl FnMut(&mut egui::Ui),
    ) -> Vec<(Role, String, egui::Rect)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 300.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| draw(ui));
        (output.platform_output.accesskit_update.into_iter())
            .flat_map(|update| update.nodes)
            .filter_map(|(_, node)| {
                let b = node.bounds()?;
                let text = node.label().or_else(|| node.value()).unwrap_or_default();
                let rect = egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32, b.y0 as f32),
                    egui::pos2(b.x1 as f32, b.y1 as f32),
                );
                Some((node.role(), text.to_string(), rect))
            })
            .collect()
    }

    /// How far the middle of the first spinner is, up or down, from the middle of the first text that has `with`, and how tall the spinner is.
    fn off_by(nodes: &[(Role, String, egui::Rect)], with: &str) -> (f32, f32) {
        let spinner = nodes
            .iter()
            .find(|(role, ..)| *role == Role::ProgressIndicator)
            .unwrap_or_else(|| panic!("no spinner is drawn: {nodes:?}"));
        let text = nodes
            .iter()
            .find(|(role, text, _)| *role != Role::ProgressIndicator && text.contains(with))
            .unwrap_or_else(|| panic!("nothing says `{with}`: {nodes:?}"));
        (
            (spinner.2.center().y - text.2.center().y).abs(),
            spinner.2.height(),
        )
    }

    /// The tallest a spinner may be and sit on a line of small text without standing over it.
    const A_LINE: f32 = 14.0;

    #[test]
    fn a_spinner_sits_on_the_line_of_the_words_it_goes_with() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let job = RunningJob {
            kind: JobKind::ProjectRead,
            doing: "Reading police".into(),
            fraction: None,
            detail: "the sources of the project".into(),
            stopping: false,
        };
        let list = drawn(&ctx, |ui| {
            draw_list(ui, std::slice::from_ref(&job), &[], Some(440.0));
        });
        let (off, height) = off_by(&list, "the sources of the project");
        assert!(
            off <= 1.0 && height <= A_LINE,
            "the spinner in the list is {height} points tall and off its line by {off}"
        );

        // The status bar's own: a project being read is the job that runs with no end to measure.
        let mut state = AppState::from_prefs(crate::state::Prefs::default());
        state.documents.clear();
        state.active_document = None;
        state.project_path = Some(std::path::PathBuf::from("C:/mods/police.openrage-proj"));
        assert_eq!(
            state.running_jobs().len(),
            1,
            "a project being read is a job"
        );
        let bar = drawn(&ctx, |ui| {
            crate::ui::panels::status_bar::draw(ui, &mut state)
        });
        let (off, height) = off_by(&bar, "job running");
        assert!(
            off <= 1.0 && height <= A_LINE,
            "the spinner in the bar is {height} points tall and off its line by {off}"
        );
    }
}
