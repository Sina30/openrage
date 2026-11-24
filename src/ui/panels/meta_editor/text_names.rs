//! A GTA VI hash value no table names, shown beside it by the name the harvested text holds.

use or_io::meta::Value;

use super::ctx::EditorCtx;
use crate::ui::theme;

/// The vocabulary's marked name for `value`, where the form draws a GTA VI form's document.
pub(super) fn draw(ui: &mut egui::Ui, ctx: &EditorCtx, value: &Value) {
    let Value::Hash(hash) = value else {
        return;
    };
    if let Some(name) = (ctx.text_names)
        .then(|| crate::state::meta_text_name(hash))
        .flatten()
    {
        ui.label(egui::RichText::new(name).small().color(theme::TEXT_DIM));
    }
}
