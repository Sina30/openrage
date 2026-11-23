//! The Outline panel: the active document's own tree, which follows its tab.

use super::*;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(doc_id) = state.active_document.filter(|&id| has_outline(state, id)) else {
        empty_centered(ui, &nothing_to_outline(state));
        return;
    };
    let scroll_to_at_start = ui_state(state).scroll_to;
    if scroll_to_at_start.is_some() {
        ui.ctx().request_repaint();
    }

    let mut outcome = RowOutcome::default();
    // ⚠ One scroll offset per document: a shared one carries over to the next document's tree.
    egui::ScrollArea::both()
        .id_salt(("outline", doc_id))
        .auto_shrink([false, false])
        .show(ui, |ui| document_row(ui, state, doc_id, &mut outcome));
    outcome.apply(state, ui.ctx());

    // ⚠ Kept until its row scrolls into view: a header forced open animates, so the row under
    // it draws a frame or two later.
    let reached = (ui.ctx())
        .data_mut(|d| d.remove_temp::<bool>(egui::Id::new(REVEALED_KEY)))
        .unwrap_or(false);
    let ui_state = ui_state(state);
    let target = ui_state.scroll_to;
    let waited = match ui_state.outline_reveal {
        Some((row, frames)) if Some(row) == target => frames + 1,
        _ => 0,
    };
    if target == scroll_to_at_start && (reached || waited > REVEAL_PATIENCE) {
        ui_state.scroll_to = None;
    }
    ui_state.outline_reveal = ui_state.scroll_to.map(|row| (row, waited));
    if ui_state.scroll_to.is_some() {
        ui.ctx().request_repaint();
    }
}

/// Whether the Outline lists `doc_id`'s parts: an install, a folder or an archive is Files'.
fn has_outline(state: &AppState, doc_id: DocumentId) -> bool {
    state.document(doc_id).is_some_and(|doc| {
        !matches!(doc.payload, DocPayload::Rpf(_))
            && (capabilities::for_payload(&doc.payload).is_some_and(|row| row.outline.is_some())
                || described_outline(doc).is_some()
                || crate::ui::panels::gta6_anim_scene::holds_one(doc)
                || gta6_mission::has_tables(&doc.payload))
    })
}

/// What the Outline says while the active document has no parts to list.
fn nothing_to_outline(state: &AppState) -> String {
    const OPEN: &str = tr_key!(
        "Open a model, a texture dictionary, a clip dictionary or particle \
                        effects to see its parts here"
    );
    // ⚠ In the Project workspace the install tab is not what this is about: an asset opened from the table is.
    if state.project_path.is_some() && state.active_workspace == crate::ui::dock::Workspace::Project
    {
        return tr!("Nothing to outline here\n\nDouble-click an asset to open it; its parts are listed here")
            .to_string();
    }
    match state.active_document() {
        None => tr!("Nothing open\n\n{open}", open = crate::i18n::tr(OPEN)),
        // ⚠ Files is not a panel of the Project workspace: the install tab behind it is not what the person is looking at.
        Some(doc)
            if matches!(doc.payload, DocPayload::Rpf(_))
                && state.project_path.is_some()
                && state.active_workspace == crate::ui::dock::Workspace::Project =>
        {
            tr!("Nothing selected\n\nSelect an asset in Assets and press Enter to see its parts here")
                .to_string()
        }
        Some(doc) if matches!(doc.payload, DocPayload::Rpf(_)) => tr!("{file_name} is in Files\n\nOpen a file from it to see that file's parts here", file_name = doc.file_name()),
        Some(doc) if matches!(doc.payload, DocPayload::Compare(_)) => tr!("{file_name} compares two files\n\nIts differences are in its tab: Previous and Next step between them", file_name = doc.file_name()),
        Some(doc) => tr!("{file_name} has nothing to outline\n\n{open}", open = crate::i18n::tr(OPEN), file_name = doc.file_name()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::compare::{Comparison, Found};
    use crate::state::{Document, DocumentKind};

    #[test]
    fn the_outline_of_a_document_with_nothing_to_outline_says_what_it_is_not_that_it_has_no_parts()
    {
        let mut state = AppState::default();
        state.documents.clear();
        let comparison = Comparison {
            headings: ["a.ymt".into(), "b.ymt".into()],
            places: [r"C:\mods\a.ymt".into(), r"C:\mods\b.ymt".into()],
            found: Found::Lines(Vec::new()),
        };
        let mut doc = Document::new(7, None, DocPayload::Compare(comparison));
        doc.name = "a.ymt ↔ b.ymt".into();
        state.documents.push(doc);
        state.set_active_document(7);
        let said = nothing_to_outline(&state);
        assert!(
            !said.contains("no parts to list") && said.contains("a.ymt ↔ b.ymt compares"),
            "the Outline of a comparison does not say what it is: {said}"
        );

        let text = state.new_empty_document(DocumentKind::Text);
        state.set_active_document(text);
        let said = nothing_to_outline(&state);
        assert!(
            !said.contains("no parts to list") && said.contains("nothing to outline"),
            "the Outline of a text document says: {said}"
        );
    }
}
