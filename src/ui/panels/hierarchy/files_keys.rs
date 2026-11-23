//! The Files view from the keyboard: the arrows, Home and End move, Enter
//! opens, Space shows the preview, Ctrl+A selects all and typing jumps to a
//! name; F2 is Rename's and Delete is Delete's, which the key dispatcher runs
//! on the keys the user bound them to.
//!
//! ⚠ Read only while the pointer is over the listing and no field has focus,
//! as every panel's keys are: a letter typed into the address bar is text.

use super::files_view::view_ui;
use super::folder_view::{open_row, Listing};
use super::rows::select_row;
use super::*;

/// How long after a letter the next one still adds to the name being typed.
const TYPE_AHEAD_PAUSE: f64 = 1.0;

/// What the keys asked of the listing this frame.
#[derive(Debug, Default, PartialEq)]
struct Keys {
    /// Rows to move the focus by, or to the first or last.
    step: Option<isize>,
    home: bool,
    end: bool,
    extend: bool,
    open: bool,
    preview: bool,
    all: bool,
    typed: String,
}

fn read(ctx: &egui::Context, per_row: usize) -> Keys {
    let across = per_row as isize;
    ctx.input_mut(|i| {
        let mut keys = Keys {
            extend: i.modifiers.shift,
            ..Default::default()
        };
        for (key, step) in [
            (egui::Key::ArrowDown, across),
            (egui::Key::ArrowUp, -across),
            (egui::Key::ArrowRight, 1),
            (egui::Key::ArrowLeft, -1),
            (egui::Key::PageDown, across * 8),
            (egui::Key::PageUp, -across * 8),
        ] {
            let horizontal = matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight);
            if horizontal && per_row == 1 {
                continue;
            }
            if i.consume_key(egui::Modifiers::NONE, key)
                || i.consume_key(egui::Modifiers::SHIFT, key)
            {
                keys.step = Some(step);
            }
        }
        keys.home = i.consume_key(egui::Modifiers::NONE, egui::Key::Home)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::Home);
        keys.end = i.consume_key(egui::Modifiers::NONE, egui::Key::End)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::End);
        keys.open = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        keys.preview = i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
        keys.all = i.consume_key(egui::Modifiers::COMMAND, egui::Key::A);
        if !(i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt) {
            for event in &i.events {
                if let egui::Event::Text(text) = event {
                    keys.typed
                        .extend(text.chars().filter(|c| !c.is_whitespace()));
                }
            }
        }
        keys
    })
}

pub(super) fn handle(ui: &egui::Ui, state: &mut AppState, listing: &Listing, per_row: usize) {
    let keys = read(ui.ctx(), per_row);
    if keys == Keys::default()
        || keys
            == (Keys {
                extend: true,
                ..Default::default()
            })
    {
        return;
    }
    let doc = listing.doc;
    let order = &listing.order;
    let focus = state
        .selection
        .selected(doc)
        .and_then(|f| order.iter().position(|&id| id == f));

    let target = if keys.home {
        Some(0)
    } else if keys.end {
        order.len().checked_sub(1)
    } else if let Some(step) = keys.step {
        match focus {
            Some(at) => Some(
                at.saturating_add_signed(step)
                    .min(order.len().saturating_sub(1)),
            ),
            None if step > 0 => Some(0),
            None => order.len().checked_sub(1),
        }
    } else {
        None
    };
    if let Some(&to) = target.and_then(|at| order.get(at)) {
        if keys.extend && focus.is_some() {
            state.selection.select_range(doc, to, order);
        } else {
            select_row(state, doc, to);
        }
        view_ui(state).scroll_to_focus = true;
    }

    if keys.all {
        if let (Some(&first), Some(&last)) = (order.first(), order.last()) {
            state.selection.select(doc, first);
            state.selection.select_range(doc, last, order);
        }
    }
    if keys.preview {
        let view = view_ui(state);
        view.preview_hidden = !view.preview_hidden;
    }
    if !keys.typed.is_empty() {
        type_ahead(ui.input(|i| i.time), state, listing, &keys.typed);
    }

    let Some(focused) = state.selection.selected(doc).filter(|f| order.contains(f)) else {
        return;
    };
    if keys.open {
        open_row(state, listing, focused);
    }
}

/// Jump to the first row whose name starts with what has been typed; the same
/// letter again goes on to the next row it starts.
fn type_ahead(now: f64, state: &mut AppState, listing: &Listing, typed: &str) {
    let view = view_ui(state);
    if now - view.typed_at > TYPE_AHEAD_PAUSE {
        view.typed.clear();
    }
    view.typed.push_str(&typed.to_lowercase());
    view.typed_at = now;
    let wanted = view.typed.clone();

    let doc = listing.doc;
    let names: Vec<(RpfNodeId, String)> = match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => listing
            .order
            .iter()
            .filter_map(|&id| Some((id, rpf.session.node(id)?.name.to_lowercase())))
            .collect(),
        _ => return,
    };
    let focus = state
        .selection
        .selected(doc)
        .and_then(|f| names.iter().position(|(id, _)| *id == f));
    let whole = names.iter().position(|(_, name)| name.starts_with(&wanted));
    let one_letter = wanted
        .chars()
        .next()
        .filter(|&c| wanted.chars().all(|l| l == c));
    let hit = match one_letter {
        // ⚠ A letter alone, or pressed again with no name starting so, steps
        // on from the selection rather than back to the first it starts.
        Some(letter) if wanted.chars().count() == 1 || whole.is_none() => {
            let letter = letter.to_string();
            let start = focus.map_or(0, |at| at + 1);
            (0..names.len())
                .map(|k| (start + k) % names.len())
                .find(|&at| names[at].1.starts_with(&letter))
        }
        _ => whole,
    };
    if let Some(at) = hit {
        select_row(state, doc, names[at].0);
        view_ui(state).scroll_to_focus = true;
    }
}
