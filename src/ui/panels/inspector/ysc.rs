use super::*;

pub(super) fn draw_ysc(ui: &mut egui::Ui, state: &AppState, doc_id: crate::state::DocumentId) {
    let Some(script) = state
        .document(doc_id)
        .and_then(|d| crate::state::ScriptDocument::in_payload(&d.payload))
    else {
        return;
    };

    ui.heading("Script");
    ui.separator();
    ui.label(egui::RichText::new(&script.sentence).color(theme::TEXT_SECONDARY));
    if !script.named {
        draw_natives_download(ui);
    }
    ui.add_space(crate::ui::theme::SPACING_SNUG);

    // Rows, not one label, so a frame lays out only the visible lines.
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both()
        .id_salt(("ysc_listing", doc_id))
        .auto_shrink([false, false])
        .show_rows(ui, row_height, script.listing.len(), |ui, rows| {
            for line in &script.listing[rows] {
                ui.add(egui::Label::new(egui::RichText::new(line).monospace()).extend());
            }
        });
}

/// What a download the user started has come to: `None` while it runs.
type DownloadSlot = std::sync::Arc<std::sync::Mutex<Option<Result<String, String>>>>;

/// ⚠ Fetches only on the click: OpenRage bundles no natives list, so this
/// button is the one way the app reaches the network for one.
fn draw_natives_download(ui: &mut egui::Ui) {
    let id = egui::Id::new("ysc_natives_download");
    let slot: Option<DownloadSlot> = ui.data(|d| d.get_temp(id));
    match slot {
        None => {
            let button = ui.button("Download natives database").on_hover_text(
                "Fetches alloc8or's natives lists and CSY0N's hash crossmap from GitHub into \
                     the configuration directory, recording each file's commit. The lists \
                     carry no licence, so OpenRage does not bundle them.",
            );
            if button.clicked() {
                let Some(dir) = crate::state::natives_dir() else {
                    return;
                };
                let slot = DownloadSlot::default();
                let done = slot.clone();
                std::thread::spawn(move || {
                    let outcome = or_cli::download_natives(&dir).map(|files| {
                        let labels: Vec<String> = files.iter().map(|f| f.label()).collect();
                        labels.join(", ")
                    });
                    if let Ok(mut slot) = done.lock() {
                        *slot = Some(outcome);
                    }
                });
                ui.data_mut(|d| d.insert_temp(id, slot));
            }
        }
        Some(slot) => match slot.lock().ok().and_then(|s| s.clone()) {
            None => {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(150));
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Downloading the natives database…");
                });
            }
            Some(Ok(labels)) => {
                ui.colored_label(
                    theme::SUCCESS,
                    format!("Downloaded {labels}. Reopen the script to name its natives."),
                );
            }
            Some(Err(e)) => {
                ui.colored_label(theme::ERROR, format!("Download failed: {e}"));
                if ui.button("Try again").clicked() {
                    ui.data_mut(|d| d.remove::<DownloadSlot>(id));
                }
            }
        },
    }
}
