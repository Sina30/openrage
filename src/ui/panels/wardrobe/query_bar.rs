//! The query bar: what the filter field understands, the queries the pack keeps by name as groups of the list, and what to do with the garments a query lists.
//!
//! ⚠ **Thin by design.** A line is read as a query by `or-wardrobe` and the list is laid out from it; this says where a term cannot be read,
//! offers the saved queries as chips, and asks `state` to keep a query, forget one or edit the garments listed.

use egui_phosphor::regular as icon;
use or_wardrobe::query::{Query, SavedQuery, Subject};

use super::WardrobeUi;
use crate::state::workbench::Question;
use crate::state::AppState;
use crate::ui::theme;

/// What the filter field says while it is empty: short enough to be read whole, with the terms it understands in its tip.
pub(super) const HINT: &str = tr_key!("Filter garments");

/// What the field understands, for the hover over it.
pub(super) const HELP: &str = tr_key!("Type part of a name to find garments by it. Or ask with terms, and mix them: \
tag:wool group:coats ped:male slot:jbib polys>5000 lod:missing is:duplicate duplicate-of:jacket problem:model-missing is:problem. \
A - in front of a term turns it round (-tag:wool). Save a query to keep it as a group of the list");

/// How many garments each saved query selects, as the pack stood at the scan it was counted at.
#[derive(Clone, PartialEq)]
pub(super) struct Smart {
    generation: u64,
    queries: Vec<SavedQuery>,
    counts: Vec<usize>,
}

/// Whether two lines state the same query: the same words, whatever the space between them.
fn same_line(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

/// How many garments of `read` each of its saved queries selects.
fn count(read: &crate::state::workbench::PackRead) -> Vec<usize> {
    let subject = Subject::new(
        &read.wardrobe,
        &read.allocation,
        &read.findings,
        &read.counts,
    );
    (read.wardrobe.queries.iter())
        .map(|saved| Query::parse(&saved.query).map_or(0, |q| q.select(&subject).len()))
        .collect()
}

pub(super) fn draw(ui: &mut egui::Ui, state: &mut AppState, shown: &mut WardrobeUi, module: &str) {
    let parsed = Query::parse(&shown.filter);
    let generation = state.workbench.wardrobe.generation;
    let Some(read) = state
        .workbench
        .packs()
        .iter()
        .find(|p| p.module == module)
        .and_then(|p| p.read.as_ref().ok())
    else {
        return;
    };
    let kept = &read.wardrobe.queries;
    if shown
        .smart
        .as_ref()
        .is_none_or(|s| s.generation != generation || s.queries != *kept)
    {
        shown.smart = Some(Smart {
            generation,
            queries: kept.clone(),
            counts: count(read),
        });
    }
    let writable = !state.wardrobe_is_linked() && !state.wardrobe_locked_out();
    let listed: Vec<String> = (shown.cache.as_ref()).map_or_else(Vec::new, |c| c.order.clone());
    let smart = shown.smart.clone().unwrap_or(Smart {
        generation,
        queries: Vec::new(),
        counts: Vec::new(),
    });
    // ⚠ Only a term makes a query: the bar of a part of a name typed in the filter would push the list down by a row at every first letter.
    let is_query = parsed.as_ref().is_ok_and(|q| q.has_terms());
    let is_saved = (kept.iter()).any(|q| same_line(&q.query, &shown.filter));

    if let Err(why) = &parsed {
        ui.add(egui::Label::new(egui::RichText::new(why).small().color(theme::ERROR)).wrap());
    }
    if kept.is_empty() && !is_query {
        return;
    }
    let mut asked: Option<Asked> = None;
    ui.horizontal_wrapped(|ui| {
        if !smart.queries.is_empty() {
            saved_menu(ui, &smart, &shown.filter, writable, &mut asked);
        }
        if writable && is_query && !is_saved && ui.small_button(tr!("Save query…")).on_hover_text(
            tr!("Keep this query in the pack under a name, and the list offers it as a group; what it selects is read again each time, so a garment tagged later joins it"),
        ).clicked() {
            asked = Some(Asked::Save);
        }
        if writable && is_query && !listed.is_empty() {
            let words = match listed.len() {
                1 => tr!("Edit the listed garment…").to_string(),
                n => tr!("Edit the {n} listed…", n = n),
            };
            let edit = ui.small_button(words).on_hover_text(
                tr!("Make one change to every garment the list shows, saying first what it would change in how many of them. One Undo takes all of it back"),
            );
            if edit.clicked() {
                asked = Some(Asked::Edit);
            }
        }
    });
    match asked {
        Some(Asked::Toggle(text, active)) => {
            shown.filter = if active { String::new() } else { text };
        }
        Some(Asked::Forget(name)) => state.forget_query(module.to_string(), name),
        Some(Asked::Save) => {
            state.workbench.asking = Some(Question::SaveQuery {
                module: module.to_string(),
                query: shown
                    .filter
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                name: String::new(),
                error: None,
                focus: true,
            });
        }
        Some(Asked::Edit) => {
            let query = shown.filter.trim().to_string();
            state.ask_to_bulk_edit(module.to_string(), query, listed);
        }
        None => {}
    }
}

/// The queries the pack keeps, as one menu whose button names the one in force, so that a pack with several does not spend a row on each.
fn saved_menu(
    ui: &mut egui::Ui,
    smart: &Smart,
    filter: &str,
    writable: bool,
    asked: &mut Option<Asked>,
) {
    let title = match (smart.queries.iter()).find(|q| same_line(&q.query, filter)) {
        Some(on) => format!("{} {}", icon::FUNNEL, on.name),
        None => tr!("{icon} Saved queries", icon = icon::FUNNEL),
    };
    let menu = ui.menu_button(title, |ui| {
        ui.set_min_width(200.0);
        for (saved, n) in smart.queries.iter().zip(&smart.counts) {
            let active = same_line(&saved.query, filter);
            let item = ui
                .add(egui::Button::new(format!("{} · {n}", saved.name)).selected(active))
                .on_hover_text(tr!("{query}\nClick to list only these garments, and again to list all; right-click to forget it", query = saved.query));
            item.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr!("Saved query: {name}", name = saved.name),
                )
            });
            if item.clicked() {
                *asked = Some(Asked::Toggle(saved.query.clone(), active));
                ui.close_kind(egui::UiKind::Menu);
            }
            item.context_menu(|ui| {
                if writable && ui.button(tr!("Forget this query")).clicked() {
                    *asked = Some(Asked::Forget(saved.name.clone()));
                    ui.close();
                }
            });
        }
    });
    menu.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Saved queries"))
    });
    menu.response.on_hover_text(tr!(
        "The queries this pack keeps by name: pick one to list only the garments it selects"
    ));
}

/// What a click on the bar asked for.
enum Asked {
    /// List the garments of a saved query, or all again where it is already the listing.
    Toggle(String, bool),
    Forget(String),
    Save,
    Edit,
}
