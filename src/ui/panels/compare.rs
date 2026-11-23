//! A comparison's document: each side named and placed, then how the two
//! differ, stepped through from one difference to the next.

use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;

use crate::state::compare::{By, Change, Comparison, FieldChange, Found, LinePair};
use crate::state::{AppState, DocPayload, DocumentId};
use crate::ui::theme;

const ROW: f32 = 18.0;
const NUMBER: f32 = 48.0;

/// The stop Previous or Next difference stepped to last, and whether the table still has to scroll to it.
#[derive(Clone, Copy, Default)]
struct Stepped {
    at: Option<usize>,
    scroll: bool,
}

/// The active comparison, in the centre.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(doc) = state.active_document else {
        return;
    };
    let Some(DocPayload::Compare(compared)) = state.document(doc).map(|d| &d.payload) else {
        return;
    };
    let id = egui::Id::new(("comparison", doc));
    let mut stepped: Stepped = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    let stops = compared.stops();
    sides(ui, "comparison_sides", compared);
    ui.horizontal(|ui| {
        ui.label(format!("Compared {}", compared.summary()));
        ui.add_enabled_ui(!stops.is_empty(), |ui| {
            if ui
                .button(format!("{} Previous difference", ph::ARROW_UP))
                .clicked()
            {
                step(&mut stepped, stops.len(), false);
            }
            if ui
                .button(format!("{} Next difference", ph::ARROW_DOWN))
                .clicked()
            {
                step(&mut stepped, stops.len(), true);
            }
        });
        if let Some(at) = stepped.at {
            ui.label(
                egui::RichText::new(format!("{} of {}", at + 1, stops.len()))
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
    unread(ui, compared);
    ui.separator();
    let current = stepped.at.and_then(|at| stops.get(at).copied());
    let scroll = current.filter(|_| stepped.scroll);
    match &compared.found {
        Found::Fields { by, changes, .. } => {
            fields(ui, doc, compared, *by, changes, current, scroll)
        }
        Found::Lines(lines) => side_by_side(ui, doc, compared, lines, current, scroll),
        Found::Bytes { .. } => {
            ui.label(
                egui::RichText::new(
                    "OpenRage reads a resource by part and field, metadata by record and field, a \
                     texture dictionary by texture, an archive by entry and text by line; it \
                     compares any other file by its bytes.",
                )
                .color(theme::TEXT_SECONDARY),
            );
        }
    }
    stepped.scroll = false;
    ui.ctx().data_mut(|d| d.insert_temp(id, stepped));
}

/// What the inspector says of a comparison the centre draws.
pub fn facts(ui: &mut egui::Ui, state: &mut AppState, doc: DocumentId, _selection: Option<u64>) {
    let Some(DocPayload::Compare(compared)) = state.document(doc).map(|d| &d.payload) else {
        return;
    };
    ui.heading("Comparison");
    ui.separator();
    sides(ui, "comparison_facts", compared);
    ui.label(format!("Compared {}", compared.summary()));
    unread(ui, compared);
}

/// What a limit of the outline left out, which no difference listed here covers.
fn unread(ui: &mut egui::Ui, compared: &Comparison) {
    if let Some(unread) = compared.unread() {
        ui.label(egui::RichText::new(unread).color(theme::TEXT_SECONDARY));
    }
}

/// Each side's heading and where it is.
fn sides(ui: &mut egui::Ui, id: &str, compared: &Comparison) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (heading, place) in compared.headings.iter().zip(&compared.places) {
                ui.label(egui::RichText::new(heading).strong());
                ui.add(
                    egui::Label::new(egui::RichText::new(place).color(theme::TEXT_SECONDARY))
                        .truncate(),
                );
                ui.end_row();
            }
        });
}

fn step(stepped: &mut Stepped, stops: usize, forward: bool) {
    let at = match (stepped.at, forward) {
        (None, true) => 0,
        (None, false) => stops - 1,
        (Some(at), true) => (at + 1) % stops,
        (Some(at), false) => (at + stops - 1) % stops,
    };
    *stepped = Stepped {
        at: Some(at),
        scroll: true,
    };
}

/// The colour a side's text takes where it changed: what only the left holds, what only the right holds, or both.
fn tint(change: Option<Change>, left: bool) -> Option<theme::Themed> {
    match (change?, left) {
        (Change::Removed, true) => Some(theme::ERROR),
        (Change::Added, false) => Some(theme::SUCCESS),
        (Change::Changed, _) => Some(theme::WARNING),
        _ => None,
    }
}

fn cell(ui: &mut egui::Ui, text: &str, colour: Option<theme::Themed>, mono: bool) {
    let mut text = egui::RichText::new(text);
    if mono {
        text = text.monospace();
    }
    if let Some(colour) = colour {
        text = text.color(colour);
    }
    ui.add(egui::Label::new(text).truncate());
}

fn fields(
    ui: &mut egui::Ui,
    doc: DocumentId,
    compared: &Comparison,
    by: By,
    changes: &[FieldChange],
    current: Option<usize>,
    scroll: Option<usize>,
) {
    if changes.is_empty() {
        ui.label(egui::RichText::new(by.nothing_differs()).color(theme::TEXT_SECONDARY));
        return;
    }
    // A resource's field is a path through its parts, which a record's field is not, and an archive's entry holds one where a resource differs.
    let widths = match by {
        By::PartAndField => [180.0, 400.0, 140.0],
        By::Entry => [250.0, 290.0, 100.0],
        _ => [320.0, 130.0, 220.0],
    };
    let mut table = TableBuilder::new(ui)
        .id_salt(("comparison_fields", doc))
        .striped(true)
        .resizable(true)
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(widths[0]).at_least(80.0).clip(true))
        .column(Column::initial(widths[1]).at_least(60.0).clip(true))
        .column(Column::initial(widths[2]).at_least(80.0).clip(true))
        .column(Column::remainder().at_least(80.0).clip(true));
    if let Some(row) = scroll {
        table = table.scroll_to_row(row, Some(egui::Align::Center));
    }
    let (what, of) = by.columns();
    let titles = [
        what,
        of,
        compared.headings[0].as_str(),
        compared.headings[1].as_str(),
    ];
    table
        .header(ROW + 2.0, |mut header| {
            for title in titles {
                header.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(ROW, changes.len(), |mut row| {
                let change = &changes[row.index()];
                row.set_selected(current == Some(row.index()));
                row.col(|ui| {
                    let (glyph, colour) = match change.change {
                        Change::Added => (ph::PLUS, theme::SUCCESS),
                        Change::Removed => (ph::MINUS, theme::ERROR),
                        Change::Changed => (ph::ARROWS_LEFT_RIGHT, theme::WARNING),
                    };
                    ui.label(egui::RichText::new(glyph).color(colour));
                    ui.add(
                        egui::Label::new(egui::RichText::new(&change.record).monospace())
                            .truncate(),
                    );
                    if !change.named.is_empty() {
                        cell(ui, &change.named, Some(theme::TEXT_SECONDARY), false);
                    }
                });
                row.col(|ui| cell(ui, &change.field, None, true));
                row.col(|ui| cell(ui, &change.left, tint(Some(change.change), true), true));
                row.col(|ui| cell(ui, &change.right, tint(Some(change.change), false), true));
            });
        });
}

fn side_by_side(
    ui: &mut egui::Ui,
    doc: DocumentId,
    compared: &Comparison,
    lines: &[LinePair],
    current: Option<usize>,
    scroll: Option<usize>,
) {
    let mut table = TableBuilder::new(ui)
        .id_salt(("comparison_lines", doc))
        .striped(true)
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(NUMBER))
        .column(Column::remainder().clip(true))
        .column(Column::exact(NUMBER))
        .column(Column::remainder().clip(true));
    if let Some(row) = scroll {
        table = table.scroll_to_row(row, Some(egui::Align::Center));
    }
    // The run of changed lines Previous and Next stepped to.
    let run = current.map(|first| {
        let len = lines[first..]
            .iter()
            .take_while(|l| l.change.is_some())
            .count();
        first..first + len
    });
    table
        .header(ROW + 2.0, |mut header| {
            for title in [
                "",
                compared.headings[0].as_str(),
                "",
                compared.headings[1].as_str(),
            ] {
                header.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(ROW, lines.len(), |mut row| {
                let line = &lines[row.index()];
                row.set_selected(run.as_ref().is_some_and(|run| run.contains(&row.index())));
                for (side, left) in [(&line.left, true), (&line.right, false)] {
                    let (number, text) = match side {
                        Some((number, text)) => (number.to_string(), text.as_str()),
                        None => (String::new(), ""),
                    };
                    row.col(|ui| cell(ui, &number, Some(theme::TEXT_SECONDARY), true));
                    row.col(|ui| cell(ui, text, tint(line.change, left), true));
                }
            });
        });
}
