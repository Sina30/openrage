use super::*;

/// Rows a group's list shows before it scrolls.
const GROUP_ROWS: f32 = 12.0;

pub(super) fn draw_rel(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(DocPayload::Rel(rel)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("Audio Metadata");
    ui.separator();
    ui.label(format!(
        "relType {} ({}): {} items",
        rel.file.rel_type,
        rel.taxonomy(),
        rel.item_count()
    ));
    if let Err(why) = &rel.typed {
        ui.colored_label(theme::WARNING, format!("Items shown as bytes: {why}"));
    }

    let mut picked = None;
    let row_height = ui.spacing().interact_size.y;
    for (name, items) in &rel.groups {
        egui::CollapsingHeader::new(format!("{name} ({})", items.len()))
            .id_salt(("rel_group", name))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("rel_group_rows", name))
                    .max_height(row_height * GROUP_ROWS)
                    .auto_shrink([false, true])
                    .show_rows(ui, row_height, items.len(), |ui, range| {
                        for &i in &items[range] {
                            let label = egui::RichText::new(rel.key(i)).monospace();
                            if ui
                                .selectable_label(rel.selected == Some(i), label)
                                .clicked()
                            {
                                picked = Some(i);
                            }
                        }
                    });
            });
    }

    if let Some(i) = rel.selected {
        ui.separator();
        let fields = match (rel.item(i), rel.body(i)) {
            (Some(item), _) => format!("{item:#?}"),
            (None, Some(body)) => hex(body),
            (None, None) => String::new(),
        };
        ui.strong(rel.key(i));
        if let Some(bytes) = rel.dat54_bytes.get(i) {
            ui.label(format!("After the class id: {}", hex(bytes)));
        }
        egui::ScrollArea::both()
            .id_salt("rel_item_fields")
            .auto_shrink([false, true])
            .show(ui, |ui| ui.monospace(fields));
    }

    if let Some(i) = picked {
        if let Some(DocPayload::Rel(rel)) = state.document_mut(doc_id).map(|d| &mut d.payload) {
            rel.selected = Some(i);
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .map(|row| {
            row.iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
