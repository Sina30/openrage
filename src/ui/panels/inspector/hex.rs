use super::*;

pub(super) fn draw_hex(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(DocPayload::Hex(hex)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("Bytes");
    ui.separator();
    ui.colored_label(theme::WARNING, &hex.banner);
    let size = or_job::human_bytes(hex.bytes.len() as u64);
    match hex.magic() {
        Some(magic) => ui.label(format!("{size}, starting with the {magic} magic")),
        None => ui.label(format!("{size}, with no leading magic OpenRage knows")),
    };
    ui.separator();

    // ⚠ Its own scroll area: `show_rows` lays out only the rows in view, where
    // the inspector's scroll would lay out every row of a large file.
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical()
        .id_salt(("hex_rows", doc_id))
        .auto_shrink([false, true])
        .show_rows(ui, row_height, hex.rows(), |ui, rows| {
            for i in rows {
                ui.monospace(hex.row(i));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_draws_the_banner_and_only_the_rows_in_view() {
        let mut state = AppState::default();
        let doc = state.new_empty_document(DocumentKind::Text);
        let d = state.document_mut(doc).expect("the blank document");
        let mut bytes = b"RBF0".to_vec();
        bytes.resize(1 << 20, 0xAB);
        d.payload = DocPayload::Hex(crate::state::HexDocument {
            bytes,
            banner: "no reader for probe.bin".to_string(),
        });
        d.kind = DocumentKind::Hex;

        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        let mut laid_out = 0.0;
        let output = ctx.run_ui(input, |ui| {
            laid_out = egui::ScrollArea::both()
                .show(ui, |ui| draw_hex(ui, &mut state, doc))
                .content_size
                .y;
        });
        assert!(
            laid_out < 1200.0,
            "the rows took {laid_out} points: every row was laid out, not the visible ones"
        );
        let text = format!("{:?}", output.shapes);
        assert!(text.contains("no reader for probe.bin"), "the banner");
        assert!(text.contains("RBF0 metadata"), "the magic");
        assert!(text.contains("00000000  52 42 46 30 AB"), "the first row");
        assert!(
            !text.contains("000FFFF0"),
            "the last row is painted out of view"
        );
    }
}
