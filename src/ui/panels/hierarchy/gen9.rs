use super::*;

pub(super) fn draw_ytd_gen9(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let rows: Vec<(String, u16, u16, String)> = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::TextureDictGen9(td))) => (td.entries.iter())
            .flatten()
            .map(|t| (t.name.clone(), t.width, t.height, format!("{:?}", t.format)))
            .collect(),
        _ => return,
    };

    ui.add_enabled(false, egui::Button::new(tr!("Add texture from image…")))
        .on_disabled_hover_text(tr!(
            "An Enhanced dictionary holds GrcTextureG9, a different layout with a \
             different field set, and the encoder produces the Legacy one."
        ));
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    if rows.is_empty() {
        empty_centered(ui, tr!("No textures in this dictionary"));
        return;
    }

    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();
    let visible = rows
        .iter()
        .filter(|(name, ..)| !filter_active || name.to_ascii_lowercase().contains(&filter_lc))
        .count();
    let count_text = if filter_active {
        tr!(
            "{visible} of {count} textures match",
            visible = visible,
            count = rows.len()
        )
    } else {
        tr!(
            "{count} · Enhanced",
            count = tr_n!("{count} texture", "{count} textures", rows.len())
        )
    };
    ui.label(egui::RichText::new(count_text).color(theme::TEXT_SECONDARY));
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    for (name, width, height, format) in &rows {
        if filter_active && !name.to_ascii_lowercase().contains(&filter_lc) {
            continue;
        }
        let sel_id = name_hash(name);
        let selected = state.selection.is_selected(doc_id, sel_id);
        let label_color = if selected {
            theme::ACCENT_TEXT
        } else {
            theme::TEXT_PRIMARY
        };
        let job = icons::icon_label(&icons::ytd_texture_icon(), name, label_color.into());
        let clicked = ui
            .horizontal(|ui| {
                let resp = ui.selectable_label(selected, job);
                ui.add(detail_label(format!(
                    "{}\u{00d7}{}  {}",
                    width, height, format
                )));
                resp.clicked()
            })
            .inner;
        if clicked {
            state.selection.select(doc_id, sel_id);
            state.active_workspace = crate::ui::dock::Workspace::Texture;
        }
    }
}

pub(super) fn draw_ydr_gen9(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let drawable = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::DrawableGen9(d))) => d.clone(),
        _ => return,
    };
    let outline = DrawableOutline::of(&drawable.base, &drawable.lights, drawable.bound.as_deref());

    let id_root = ui.make_persistent_id("hierarchy_ydr");

    leaf_row(ui, |ui| {
        let job = icons::icon_label(
            &icons::doc_kind_icon(DocumentKind::Ydr, false),
            if outline.name.is_empty() {
                tr!("(unnamed)")
            } else {
                outline.name
            },
            theme::ACCENT_TEXT.get(),
        );
        ui.add(egui::Label::new(job).selectable(false));
        ui.add(detail_label(format!(
            "Enhanced · bounds radius {:.2}",
            outline.radius
        )));
    });

    section_drawable_tree(ui, state, doc_id, &outline, id_root);
}

pub(super) fn draw_ydd_gen9(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let dict = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::DrawableDictGen9(d))) => d.clone(),
        _ => return,
    };
    let entries: Vec<(usize, u32, DrawableOutline<'_>)> = (dict.codes.iter().zip(&dict.entries))
        .enumerate()
        .filter_map(|(i, (hash, entry))| {
            let d = entry.as_deref()?;
            Some((
                i,
                *hash,
                DrawableOutline::of(&d.base, &d.lights, d.bound.as_deref()),
            ))
        })
        .collect();
    super::ydd::draw_ydd_dictionary(ui, state, doc_id, &entries, or_io::Generation::Gen9);
}
