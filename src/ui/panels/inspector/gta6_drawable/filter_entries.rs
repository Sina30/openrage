//! A scanned GTA VI dictionary's picked frame filter or animation record copied under a new name
//! or removed whole, each a step of undo.

use super::*;

/// What the row asks of the entry picked.
pub(super) enum EntryAction {
    Copy { code: u32, name: String },
    Remove { code: u32 },
}

/// The row under the picked entry `code` of a dictionary holding `named` filters and records: a
/// name, Copy and Remove; what was clicked.
pub(super) fn draw(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    code: u32,
    named: usize,
) -> Option<EntryAction> {
    let key = egui::Id::new(("gta6_filter_entry_name", doc_id));
    let mut name = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_default();
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(tr!("New name"));
        let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
        if field.changed() {
            ui.data_mut(|d| d.insert_temp(key, name.clone()));
        }
        let copy = ui.add_enabled(
            !name.trim().is_empty(),
            egui::Button::new(tr!("Copy the entry")).small(),
        );
        if copy.clicked() {
            let name = name.trim().to_string();
            action = Some(EntryAction::Copy { code, name });
        }
        // A dictionary keeps one filter or record.
        let remove = ui.add_enabled(
            named > 1,
            egui::Button::new(tr!("Remove the entry")).small(),
        );
        if remove.clicked() {
            action = Some(EntryAction::Remove { code });
        }
    });
    ui.label(
        egui::RichText::new(tr!("A copy takes the key its name hashes to and the signature of the entry it copies; Save As places it and its tracks or bones after the body's last object."))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    action
}

/// `action` made in document `doc_id`; a refusal is shown.
pub(super) fn apply(state: &mut AppState, doc_id: DocumentId, action: EntryAction) {
    let done = match action {
        EntryAction::Copy { code, name } => state.copy_gta6_dictionary_entry(doc_id, code, &name),
        EntryAction::Remove { code } => state.remove_gta6_dictionary_entry(doc_id, code),
    };
    if let Err(why) = done {
        state.show_warning_toast(why);
    }
}
