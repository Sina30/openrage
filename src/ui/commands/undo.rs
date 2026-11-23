//! Edit ▸ Undo and Redo: the one pair of keys, which reaches the stack of whatever is on screen.

use egui::Key;

use super::table::{none_if, spec, Action, Spec, BASE};
use crate::state::{AppState, EditHistory};
use crate::ui::shortcuts::ShortcutMods;

const CTRL: ShortcutMods = ShortcutMods::ctrl();
const CTRL_SHIFT: ShortcutMods = ShortcutMods::ctrl_shift();

/// The undo, or the redo, of the task screen on show where its row names one: Edit ▸ Undo and Redo are that command while the screen is on screen, so their keys take back what was just done there.
fn of_the_screen(state: &AppState, undo: bool) -> Option<&'static Spec> {
    state.project_path.as_ref()?;
    let screen = crate::ui::task_screens::of_workspace_alone(state.active_workspace)?;
    let (back, forth) = screen.undo?;
    spec(if undo { back } else { forth })
}

/// What `spec` reads now.
fn label_of(spec: &Spec, state: &AppState) -> String {
    (spec.label_now).map_or_else(|| crate::i18n::tr(spec.label).to_string(), |now| now(state))
}

pub(super) const UNDO: Spec = Spec {
    action: Action::Undo,
    id: "undo",
    label: tr_key!("Undo"),
    category: tr_key!("Edit"),
    defaults: &[(CTRL, Key::Z)],
    hover: tr_key!(
        "Undo the last edit of the document the focused panel shows: \
            a pinned inspector's, the archive or folder Files shows, else the active one"
    ),
    enabled: |state| {
        if let Some(screen) = of_the_screen(state, true) {
            return (screen.enabled)(state);
        }
        if super::project::undo_is_the_projects(state) {
            return super::project::can_undo_project(state);
        }
        if super::wardrobe::undo_is_the_wardrobes(state) {
            return super::wardrobe::can_undo_in_the_wardrobe(state);
        }
        if crate::ui::panels::text_editor::offers(state, true) {
            return Ok(());
        }
        let none = !state.focused_history().is_some_and(EditHistory::can_undo);
        none_if(none, tr!("Nothing to undo yet"))
    },
    label_now: Some(|state| {
        if let Some(screen) = of_the_screen(state, true) {
            return label_of(screen, state);
        }
        if super::project::undo_is_the_projects(state) {
            return (state.project_undo_label()).map_or_else(
                || tr!("Undo").to_string(),
                |last| tr!("Undo \"{last}\"", last = last),
            );
        }
        if super::wardrobe::undo_is_the_wardrobes(state) {
            return (state.joined_undo_label()).map_or_else(
                || tr!("Undo").to_string(),
                |last| tr!("Undo \"{last}\"", last = last),
            );
        }
        let last = state.focused_history().and_then(EditHistory::undo_label);
        last.map_or_else(
            || tr!("Undo").to_string(),
            |last| tr!("Undo \"{last}\"", last = last),
        )
    }),
    run: |state, ctx| {
        if let Some(screen) = of_the_screen(state, true) {
            return (screen.run)(state, ctx);
        }
        if super::project::undo_is_the_projects(state) {
            return super::project::undo_the_project(state);
        }
        if super::wardrobe::undo_is_the_wardrobes(state) {
            return state.undo_joined();
        }
        // The text editor's steps are its own: it takes one at its next paint.
        if crate::ui::panels::text_editor::offers(state, true) {
            crate::ui::panels::text_editor::ask(state, true);
            return ctx.request_repaint();
        }
        if let Some(label) = state.undo() {
            state.show_toast(tr!("Undid '{label}'", label = label));
        }
    },
    ..BASE
};

pub(super) const REDO: Spec = Spec {
    action: Action::Redo,
    id: "redo",
    label: tr_key!("Redo"),
    category: tr_key!("Edit"),
    defaults: &[(CTRL_SHIFT, Key::Z), (CTRL, Key::Y)],
    hover: tr_key!(
        "Redo the last undone edit of the document the focused panel shows: \
            a pinned inspector's, the archive or folder Files shows, else the active one"
    ),
    enabled: |state| {
        if let Some(screen) = of_the_screen(state, false) {
            return (screen.enabled)(state);
        }
        if super::project::undo_is_the_projects(state) {
            return super::project::can_redo_project(state);
        }
        if super::wardrobe::undo_is_the_wardrobes(state) {
            return super::wardrobe::can_redo_in_the_wardrobe(state);
        }
        if crate::ui::panels::text_editor::offers(state, false) {
            return Ok(());
        }
        let none = !state.focused_history().is_some_and(EditHistory::can_redo);
        none_if(none, tr!("Nothing to redo"))
    },
    label_now: Some(|state| {
        if let Some(screen) = of_the_screen(state, false) {
            return label_of(screen, state);
        }
        if super::project::undo_is_the_projects(state) {
            return (state.project_redo_label()).map_or_else(
                || tr!("Redo").to_string(),
                |next| tr!("Redo \"{next}\"", next = next),
            );
        }
        if super::wardrobe::undo_is_the_wardrobes(state) {
            return (state.joined_redo_label()).map_or_else(
                || tr!("Redo").to_string(),
                |next| tr!("Redo \"{next}\"", next = next),
            );
        }
        let last = state.focused_history().and_then(EditHistory::redo_label);
        last.map_or_else(
            || tr!("Redo").to_string(),
            |last| tr!("Redo \"{last}\"", last = last),
        )
    }),
    run: |state, ctx| {
        if let Some(screen) = of_the_screen(state, false) {
            return (screen.run)(state, ctx);
        }
        if super::project::undo_is_the_projects(state) {
            return super::project::redo_the_project(state);
        }
        if super::wardrobe::undo_is_the_wardrobes(state) {
            return state.redo_joined();
        }
        if crate::ui::panels::text_editor::offers(state, false) {
            crate::ui::panels::text_editor::ask(state, false);
            return ctx.request_repaint();
        }
        if let Some(label) = state.redo() {
            state.show_toast(tr!("Redid '{label}'", label = label));
        }
    },
    ..BASE
};
