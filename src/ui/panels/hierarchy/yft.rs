use super::*;

pub(super) fn draw_yft(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::Fragment(f))) => {
            let frag = f.clone();
            draw_fragment(ui, state, doc_id, &frag);
        }
        Some(DocPayload::Resource(or_io::Asset::FragmentGen9(f))) => {
            let frag = f.clone();
            draw_fragment(ui, state, doc_id, &frag);
        }
        _ => {}
    }
}

fn draw_fragment<T: Outlined>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    frag: &or_io::gta5::GtaFragType<T>,
) {
    let id_root = ui.make_persistent_id("hierarchy_yft");
    let bones = (frag.common_drawable.as_deref())
        .and_then(|d| d.skeleton.as_deref())
        .map_or(0, |s| s.bones.len());
    let children = frag.child_count();

    leaf_row(ui, |ui| {
        let job = icons::icon_label(
            &icons::doc_kind_icon(DocumentKind::Yft, false),
            if frag.tune_name.is_empty() {
                "(unnamed fragment)"
            } else {
                frag.tune_name.as_str()
            },
            theme::ACCENT_TEXT.get(),
        );
        ui.add(egui::Label::new(job).selectable(false));
        ui.add(detail_label(format!(
            "r {:.2} · {} bones · {} children",
            frag.bounding_sphere.w, bones, children,
        )));
    });

    if let Some(common) = frag.common_drawable.as_deref() {
        let outline = DrawableOutline::of(&common.base, &[], common.bound.as_deref());
        let hash = crate::state::textures::FRAGMENT_ENTRY_HASH;
        draw_entry_sections(ui, state, doc_id, hash, &outline, id_root);
    }

    if !frag.extra_drawables.is_empty() {
        collapsing_row(
            ui,
            id_root.with("yft_extras"),
            true,
            |ui, _open| {
                ui.label(egui::RichText::new(format!(
                    "Extra drawables ({})",
                    frag.extra_drawables.len()
                )));
            },
            |ui| {
                for (i, d) in frag.extra_drawables.iter().enumerate() {
                    let label = frag
                        .extra_drawable_names
                        .get(i)
                        .filter(|n| !n.is_empty())
                        .cloned()
                        .unwrap_or_else(|| {
                            if d.name.is_empty() {
                                format!("extra #{i}")
                            } else {
                                d.name.clone()
                            }
                        });
                    leaf_row(ui, |ui| {
                        ui.label(label);
                        if frag.damaged_drawable == i as i32 {
                            ui.add(detail_label("damaged"));
                        }
                    });
                }
            },
        );
    }

    if !frag.lights.is_empty() {
        let lights: Vec<u8> = frag.lights.iter().map(|l| l.light_type).collect();
        section_lights(ui, state, doc_id, &lights, id_root);
    }

    if let Some(group) = frag.physics_lod_group.as_deref() {
        let bound = crate::ui::panels::selection_ids::dict_entry_part_id(
            crate::ui::panels::selection_ids::physics_bound_hash(0),
            crate::ui::panels::selection_ids::YPT_MODEL_KIND_BOUND,
            0,
        );
        consume_scroll_to(state, doc_id, bound, ui.ctx(), id_root.with("yft_physics"));
        collapsing_row(
            ui,
            id_root.with("yft_physics"),
            true,
            |ui, _open| {
                ui.label(egui::RichText::new(format!(
                    "Physics ({} LOD, {} children)",
                    group.lods().count(),
                    children,
                )));
            },
            |ui| {
                let slots = [&group.high_lod, &group.medium_lod, &group.low_lod];
                for (lod_i, lod) in slots.into_iter().enumerate() {
                    let Some(lod) = lod.as_deref() else { continue };
                    let label = match lod_i {
                        0 => "High",
                        1 => "Medium",
                        _ => "Low",
                    };
                    leaf_row(ui, |ui| {
                        ui.label(format!(
                            "{label}: {} groups, {} children",
                            lod.groups.len(),
                            lod.children.len()
                        ));
                    });
                    // ⚠ The viewport draws the High level's bound alone, so a row of another level would select a line that is not there
                    if let Some(bound) = lod.composite_bounds.as_deref().filter(|_| lod_i == 0) {
                        ydd_entry_bound_row(
                            ui,
                            state,
                            doc_id,
                            crate::ui::panels::selection_ids::physics_bound_hash(lod_i),
                            &bound_type_name(bound),
                            id_root.with(("yft_phys_bound", lod_i)),
                        );
                    }
                }
            },
        );
    }
}
