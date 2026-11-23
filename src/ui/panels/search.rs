//! The search panel: a content search over the active install that runs on a
//! worker and can be stopped.

use crate::state::search_session::{split_extensions, SearchMode, SearchRequest};
use crate::state::{AppState, DocumentId, Selection};
use crate::ui::commands::{self, Action, Command, Hits, Target};
use crate::ui::theme;

/// The document a result list's [`Selection`] files its rows under; the list holds nothing else.
pub(crate) const RESULTS: DocumentId = 0;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if state.search.is_running() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }

    let install = state
        .prefs
        .active_game
        .and_then(|game| state.prefs.game_paths.get(game).cloned());
    if install.is_none() {
        ui.add_space(theme::SPACING_RELAXED);
        ui.vertical_centered(|ui| {
            ui.colored_label(
                theme::TEXT_DIM,
                tr!("Choose a game install before searching it (Settings ▸ Games)."),
            );
        });
        return;
    }

    draw_query(ui, state, install.unwrap());
    ui.add_space(theme::SPACING_SNUG);
    draw_status(ui, state);
    ui.separator();
    draw_results(ui, state);
}

fn draw_query(ui: &mut egui::Ui, state: &mut AppState, install: std::path::PathBuf) {
    let mut submit = false;
    ui.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.search_query.pattern)
                .desired_width(280.0)
                .hint_text(match state.search_query.mode {
                    SearchMode::Hex => "52 50 46 37",
                    _ => "WEAPON_RAILGUN",
                }),
        );
        submit |= resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        for mode in SearchMode::ALL {
            let selected = state.search_query.mode == mode;
            if ui.selectable_label(selected, mode.label()).clicked() {
                state.search_query.mode = mode;
            }
        }
        ui.add_enabled(
            state.search_query.mode != SearchMode::Hex,
            egui::Checkbox::new(&mut state.search_query.case_sensitive, "Aa"),
        )
        .on_hover_text(tr!("Case sensitive. Hex is always exact."));
    });

    ui.horizontal(|ui| {
        ui.label(tr!("Only"));
        ui.add(
            egui::TextEdit::singleline(&mut state.search_query.extensions)
                .desired_width(120.0)
                .hint_text("meta,ytd"),
        )
        .on_hover_text(tr!(
            "Extensions to open. Everything else is skipped without being read, which is \
             most of the cost."
        ));
        ui.label(tr!("under"));
        ui.add(
            egui::TextEdit::singleline(&mut state.search_query.under)
                .desired_width(200.0)
                .hint_text(tr!("whole install")),
        )
        .on_hover_text(tr!(
            "A container or path prefix, e.g. `x64a.rpf` or `common/data`."
        ));

        ui.separator();
        if state.search.is_running() {
            if ui.button(tr!("Stop")).clicked() {
                state.cancel_job(crate::state::jobs::JobKind::Search);
            }
        } else if ui.button(tr!("Search")).clicked() {
            submit = true;
        }
        if !state.search.hits.is_empty() {
            if ui.button(tr!("Copy results")).clicked() {
                ui.ctx().copy_text(state.search.export());
            }
            if ui.button(tr!("Export…")).clicked() {
                export_to_file(state);
            }
        }
    });

    if submit && !state.search.is_running() {
        let request = SearchRequest {
            pattern: state.search_query.pattern.clone(),
            mode: state.search_query.mode,
            case_sensitive: state.search_query.case_sensitive,
            extensions: split_extensions(&state.search_query.extensions),
            under: state.search_query.under.clone(),
            max_hits: 0,
        };
        let provider = state.keys.provider();
        state.search.start(request, install, provider);
    }
}

fn draw_status(ui: &mut egui::Ui, state: &AppState) {
    if let Some(error) = &state.search.error {
        ui.colored_label(theme::ERROR, error);
        return;
    }
    let progress = &state.search.progress;
    let line = format!(
        "{} match(es) · {} file(s) read of {} seen · {}",
        progress.hits,
        progress.files_read,
        progress.files_seen,
        if state.search.is_running() {
            "searching…"
        } else if progress.stopped {
            "stopped"
        } else if progress.files_seen == 0 {
            "idle"
        } else {
            "done"
        }
    );
    ui.colored_label(theme::TEXT_SECONDARY, egui::RichText::new(line).small());
    if state.search.truncated() {
        ui.colored_label(
            theme::WARNING,
            egui::RichText::new(tr!(
                "Result list is full; narrow the search to see the rest."
            ))
            .small(),
        );
    }
}

fn draw_results(ui: &mut egui::Ui, state: &mut AppState) {
    if state.search.hits.is_empty() {
        ui.add_space(theme::SPACING_RELAXED);
        ui.vertical_centered(|ui| {
            ui.colored_label(
                theme::TEXT_DIM,
                if state.search.is_running() {
                    tr!("No matches yet.")
                } else {
                    tr!("No results.")
                },
            );
        });
        return;
    }

    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let count = state.search.hits.len();
    let mut asked: Option<Command> = None;
    // ⚠ The selected hit's lines below keep their room whether or not one is
    // selected: the list fills the height it is given, and a layout that
    // changed on a click would move the rows under the next one.
    let small = ui.text_style_height(&egui::TextStyle::Small);
    let foot = 2.0 * small + 3.0 * ui.spacing().item_spacing.y + 6.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height((ui.available_height() - foot).max(row_height))
        .show_rows(ui, row_height, count, |ui, range| {
            for i in range {
                let hit = &state.search.hits[i];
                let label = format!(
                    "{}  ({} hit{})",
                    hit.path,
                    hit.offsets.len(),
                    if hit.offsets.len() == 1 { "" } else { "s" }
                );
                let text = egui::RichText::new(label).monospace();
                let container = hit.container.clone();
                let resp = result_row(ui, &mut state.search.selection, i, count, text);
                hits_menu(&resp, state, Hits::Search, i, &mut asked);
                if resp.on_hover_text(container).double_clicked() {
                    asked = open_hit(state, Hits::Search, i);
                }
            }
        });
    if let Some(command) = asked {
        commands::run(state, ui.ctx(), &command);
    }

    let Some(hit) = state
        .search
        .selection
        .selected(RESULTS)
        .and_then(|i| state.search.hits.get(i as usize))
    else {
        return;
    };
    ui.separator();
    ui.label(
        egui::RichText::new(format!("{}  <=  {}", hit.path, hit.container))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    if let Some(preview) = &hit.preview {
        ui.add(
            egui::Label::new(egui::RichText::new(preview).monospace().small())
                .wrap_mode(egui::TextWrapMode::Extend),
        );
    }
}

/// A result row, clicked as the tree's rows are.
pub(crate) fn result_row(
    ui: &mut egui::Ui,
    selection: &mut Selection,
    row: usize,
    rows: usize,
    text: egui::RichText,
) -> egui::Response {
    let id = row as u64;
    let resp = ui.selectable_label(selection.is_selected(RESULTS, id), text);
    if resp.clicked() {
        let modifiers = ui.input(|i| i.modifiers);
        if modifiers.shift {
            let order: Vec<u64> = (0..rows as u64).collect();
            selection.select_range(RESULTS, id, &order);
        } else if modifiers.command || modifiers.ctrl {
            selection.toggle(RESULTS, id);
        } else {
            selection.select(RESULTS, id);
        }
    }
    resp
}

/// What a double click on `row` runs: Open.
pub(crate) fn open_hit(state: &AppState, list: Hits, row: usize) -> Option<Command> {
    let target = Target::Hits {
        list,
        rows: vec![row],
    };
    commands::on(state, Action::OpenSelection, target)
}

/// `row`'s menu: Open, Reveal in tree, Extract… and Copy path, on every result selected when `row` is one of them.
pub(crate) fn hits_menu(
    resp: &egui::Response,
    state: &AppState,
    list: Hits,
    row: usize,
    asked: &mut Option<Command>,
) {
    resp.context_menu(|ui| {
        let selection = match list {
            Hits::Search => &state.search.selection,
            Hits::Inside(_) => &state.deep_find.selection,
        };
        let picked = selection.selected_ids(RESULTS);
        let mut rows = vec![row];
        if picked.len() > 1 && picked.contains(&(row as u64)) {
            ui.label(
                egui::RichText::new(format!("{} selected", picked.len()))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            ui.separator();
            rows.extend((picked.iter().map(|&r| r as usize)).filter(|&r| r != row));
        }
        let target = Target::Hits { list, rows };
        for action in [Action::OpenSelection, Action::RevealInTree, Action::Extract] {
            commands::offer(ui, state, action, &target, asked);
        }
        ui.separator();
        commands::offer(ui, state, Action::CopyPath, &target, asked);
    });
}

/// The tree on screen with nothing filtered out of it, so a revealed row is drawn.
pub(crate) fn show_tree(state: &mut AppState) {
    super::hierarchy::ui_state(state).filter.clear();
    state.deep_find.clear();
    let ws = state.active_workspace;
    state
        .workspace_docks
        .reveal(ws, super::registry::ids::FILES);
}

fn export_to_file(state: &mut AppState) {
    let Some(path) = crate::platform::file_dialogs::ask(tr!("Export Search Results"), |dialog| {
        dialog
            .set_file_name("search-results.tsv")
            .add_filter(tr!("Tab-separated"), &["tsv", "txt"])
            .save_file()
    }) else {
        return;
    };
    match std::fs::write(&path, state.search.export()) {
        Ok(()) => state.show_toast(tr!(
            "Wrote {count} results",
            count = state.search.hits.len()
        )),
        Err(e) => state.show_error_toast(tr!("Export failed: {e}", e = e)),
    }
}
