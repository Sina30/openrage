use super::*;

/// One row per texture of a pack, drawing only the rows in view: a pack can list ten thousand.
pub(super) fn draw_ttpf(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(DocPayload::Ttpf(ttpf)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    let ttpf = ttpf.clone();
    let count = ttpf.pack.entries.len();
    ui.label(
        egui::RichText::new(tr_n!("{count} texture", "{count} textures", count))
            .color(theme::TEXT_SECONDARY),
    );
    // A pack holds no names: one typed whose hash keys a texture names it from then on.
    let id = egui::Id::new(("ttpf_name", doc_id));
    let mut typed: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let field = ui.add(
        egui::TextEdit::singleline(&mut typed).hint_text(tr!("Type a texture's name to find it")),
    );
    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !typed.is_empty() {
        match ttpf.learn(&typed) {
            Some(index) => state.selection.select(doc_id, index as u64),
            None => state.show_info_toast(tr!(
                "No texture of this pack is keyed by the hash of {name}",
                name = typed
            )),
        }
    }
    ui.data_mut(|d| d.insert_temp(id, typed));
    let load = ui.button(tr!("Load names…")).on_hover_text(tr!(
        "Name the textures of the open packs from a text file: each line a texture's name, or a drawable's name the naming rules expand"
    ));
    if load.clicked() {
        let picked = crate::platform::file_dialogs::ask(tr!("Load texture names"), |dialog| {
            (dialog.add_filter("Text", &["txt"]))
                .add_filter("All Files", &["*"])
                .pick_file()
        });
        if let Some(path) = picked {
            state.load_texture_names(&path);
        }
    }
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    let row_height = ui.spacing().interact_size.y;
    let mut clicked = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = ui.cursor().top();
        let clip = ui.clip_rect();
        let at = |y: f32| (((y - top) / row_height).max(0.0) as usize).min(count);
        let (first, end) = (at(clip.top()), (at(clip.bottom()) + 1).min(count));
        let first = first.min(end);
        ui.add_space(first as f32 * row_height);
        for (index, entry) in ttpf.pack.entries[first..end].iter().enumerate() {
            let index = first + index;
            let format = entry
                .format()
                .map_or_else(|| format!("{:#05x}", entry.format_code), |f| f.name());
            let status = ttpf
                .refusal(index)
                .unwrap_or_else(|| tr!("readable").to_string());
            let selected = state.selection.is_selected(doc_id, index as u64);
            let response = ui
                .horizontal(|ui| {
                    ui.set_height(row_height);
                    let response = ui.selectable_label(selected, ttpf.name(index));
                    ui.add(detail_label(format!(
                        "{}\u{00d7}{}  {format}  {status}",
                        entry.width, entry.height
                    )));
                    response
                })
                .inner;
            if response.clicked() {
                clicked = Some(index);
            }
        }
        ui.add_space((count - end) as f32 * row_height);
    });
    if let Some(index) = clicked {
        state.selection.select(doc_id, index as u64);
        state.active_workspace = crate::ui::dock::Workspace::Texture;
    }
}
