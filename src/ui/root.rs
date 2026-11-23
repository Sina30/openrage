//! The one `Ui` a window's top-level panels are children of.
//!
//! `Panel::show_inside` takes its space from a parent `Ui`, so every panel in
//! a window has to be inside the same one, in the order they should stack.
//!
//! ⚠ Not [`Context::run_ui`](egui::Context::run_ui): it re-runs its closure
//! when a pass is discarded, so an `egui_extras` sizing pass would run the
//! whole shell twice against the `&mut AppState` in one frame.
//!
//! ⚠ **A root `Ui` is the whole window, so there is one per window and it
//! opens once.** Two of them in a pass would each start from the full
//! content rect and draw over each other.

/// Run `add_contents` against a root `Ui` covering the window.
pub fn with_root_ui<R>(ctx: &egui::Context, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let mut ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::new((ctx.viewport_id(), "openrage_root_ui")),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.content_rect()),
    );
    add_contents(&mut ui)
}
