//! The Log panel: everything `tracing` emitted this session, filterable and
//! copyable, with no terminal required.

use std::sync::Arc;

use egui_phosphor::regular as icon;
use tracing::Level;

use crate::diagnostics::crash;
use crate::diagnostics::log_buffer::{self, LogBuffer, LogRecord, Snapshot};
use crate::state::AppState;
use crate::ui::panel_state::PanelState;
use crate::ui::panels::registry::ids;
use crate::ui::theme;

/// The quietest level a view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Threshold {
    Errors,
    Warnings,
    #[default]
    Info,
    Everything,
}

impl Threshold {
    pub const ALL: [Threshold; 4] = [Self::Errors, Self::Warnings, Self::Info, Self::Everything];

    pub fn label(self) -> &'static str {
        match self {
            Self::Errors => tr!("Errors"),
            Self::Warnings => tr!("Warnings and errors"),
            Self::Info => tr!("Info and above"),
            Self::Everything => tr!("Everything"),
        }
    }

    pub fn admits(self, level: Level) -> bool {
        match self {
            Self::Errors => level <= Level::ERROR,
            Self::Warnings => level <= Level::WARN,
            Self::Info => level <= Level::INFO,
            Self::Everything => true,
        }
    }
}

/// What the panel shows, separate from drawing it.
#[derive(Default)]
pub struct LogView {
    pub threshold: Threshold,
    pub text: String,
    /// Entries at or below this sequence number are hidden by Clear.
    ///
    /// ⚠ Clear hides rather than deletes: the buffer is what a crash report
    /// copies from, and a user tidying the panel should not empty that.
    pub cleared_through: u64,
    snapshot: Snapshot,
    seen: Option<u64>,
}

impl PanelState for LogView {
    const KEY: &'static str = "log";
}

impl LogView {
    pub fn refresh(&mut self, buffer: &LogBuffer) {
        let generation = buffer.generation();
        if self.seen != Some(generation) {
            self.snapshot = buffer.snapshot();
            self.seen = Some(generation);
        }
    }

    pub fn visible(&self) -> Vec<&Arc<LogRecord>> {
        let needle = self.text.trim().to_lowercase();
        self.snapshot
            .records
            .iter()
            .filter(|r| r.seq > self.cleared_through)
            .filter(|r| self.threshold.admits(r.level))
            .filter(|r| {
                needle.is_empty()
                    || r.message.to_lowercase().contains(&needle)
                    || r.target.to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn copy_text(&self) -> String {
        let mut out = String::new();
        for record in self.visible() {
            out.push_str(&record.line());
            out.push('\n');
        }
        out
    }

    pub fn clear(&mut self) {
        if let Some(last) = self.snapshot.records.last() {
            self.cleared_through = last.seq;
        }
    }

    pub fn kept(&self) -> usize {
        self.snapshot.records.len()
    }

    pub fn session_counts(&self) -> String {
        tr!(
            "This session: {errors} error(s), {warnings} warning(s) from OpenRage.",
            errors = grouped(self.snapshot.errors),
            warnings = grouped(self.snapshot.warnings)
        )
    }

    /// The sentence that says the panel is bounded, and by how much.
    pub fn bound_note(&self) -> String {
        let dropped = self.snapshot.dropped_serious + self.snapshot.dropped_routine;
        let mut note = tr!("Keeps the newest {important} warnings and errors and the newest {others} other entries, \
             in memory only.", important = grouped(log_buffer::SERIOUS_CAPACITY as u64), others = grouped(log_buffer::ROUTINE_CAPACITY as u64));
        if dropped > 0 {
            note.push_str(&format!(
                " {} older entr{} dropped ({} of them warnings or errors).",
                grouped(dropped),
                if dropped == 1 { "y was" } else { "ies were" },
                grouped(self.snapshot.dropped_serious),
            ));
        }
        note
    }
}

pub(super) fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn level_colour(level: Level) -> egui::Color32 {
    match level {
        Level::ERROR => theme::ERROR.get(),
        Level::WARN => theme::WARNING.get(),
        Level::INFO => theme::TEXT_PRIMARY.get(),
        _ => theme::TEXT_SECONDARY.get(),
    }
}

/// Bring the Log panel forward in the active workspace, adding it if absent.
pub fn reveal(state: &mut AppState) {
    let dock = state.workspace_docks.get_mut(state.active_workspace);
    crate::ui::dock_commands::reveal(dock, ids::LOG);
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let view = state.panels.get::<LogView>();
    view.refresh(log_buffer::global());

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("log_threshold")
            .selected_text(view.threshold.label())
            .show_ui(ui, |ui| {
                for t in Threshold::ALL {
                    ui.selectable_value(&mut view.threshold, t, t.label());
                }
            });
        ui.add(
            egui::TextEdit::singleline(&mut view.text)
                .hint_text(tr!("Filter"))
                .desired_width(180.0),
        );
        if ui
            .button(tr!("{icon} Copy", icon = icon::COPY))
            .on_hover_text(tr!(
                "Copy every entry shown, one per line, to paste into a bug report"
            ))
            .clicked()
        {
            ui.ctx().copy_text(view.copy_text());
        }
        if ui
            .button(tr!("{icon} Clear", icon = icon::TRASH))
            .on_hover_text(tr!(
                "Hide what is shown now. A crash report still includes it."
            ))
            .clicked()
        {
            view.clear();
        }
    });

    let visible = view.visible();
    ui.label(
        egui::RichText::new(tr!(
            "{shown} of {total} shown. {session_counts} {bound_note}",
            shown = grouped(visible.len() as u64),
            total = grouped(view.kept() as u64),
            session_counts = view.session_counts(),
            bound_note = view.bound_note()
        ))
        .small()
        .color(theme::TEXT_SECONDARY),
    );
    ui.separator();

    let footer = 26.0;
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .max_height((ui.available_height() - footer).max(row_height))
        .show_rows(ui, row_height, visible.len(), |ui, rows| {
            for record in &visible[rows] {
                draw_row(ui, record);
            }
        });

    ui.separator();
    ui.horizontal(|ui| match crash::reports_dir() {
        Some(dir) => {
            ui.label(
                egui::RichText::new(tr!("Crash reports: {dir}", dir = dir.display()))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            if ui
                .small_button(tr!("{icon} Open folder", icon = icon::FOLDER_OPEN))
                .clicked()
            {
                crate::platform::shell::open_folder(&dir);
            }
        }
        None => {
            ui.label(
                egui::RichText::new(tr!(
                    "Crash reports cannot be saved: no configuration directory"
                ))
                .small()
                .color(theme::WARNING),
            );
        }
    });
}

fn draw_row(ui: &mut egui::Ui, record: &LogRecord) {
    let mut job = egui::text::LayoutJob::default();
    let mono = egui::TextFormat {
        font_id: egui::TextStyle::Monospace.resolve(ui.style()),
        color: theme::TEXT_DIM.get(),
        ..Default::default()
    };
    job.append(
        &format!("+{:>9.3}s ", record.uptime.as_secs_f64()),
        0.0,
        mono.clone(),
    );
    job.append(
        &format!("{:<5} ", record.level),
        0.0,
        egui::TextFormat {
            color: level_colour(record.level),
            ..mono.clone()
        },
    );
    job.append(
        &format!("{}: ", record.target),
        0.0,
        egui::TextFormat {
            color: theme::TEXT_SECONDARY.get(),
            ..mono.clone()
        },
    );
    job.append(
        &record.message,
        0.0,
        egui::TextFormat {
            color: level_colour(record.level),
            ..mono
        },
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.max_width = ui.available_width();

    let response = ui
        .add(egui::Label::new(job).sense(egui::Sense::click()))
        .on_hover_text(tr!(
            "{message}\n\nClick to copy this line",
            message = record.message
        ));
    if response.clicked() {
        ui.ctx().copy_text(record.line());
    }
}
