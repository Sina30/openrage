use super::*;
use crate::state::bone_numbers::BoneAddress;

pub(super) fn draw_ydd(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    let Some(sel) = selection.filter(|&s| s != DOC_ROOT_SEL) else {
        let Some(DocPayload::Resource(or_io::Asset::DrawableDict(dict))) =
            state.document(doc_id).map(|d| &d.payload)
        else {
            return;
        };
        let values = || dict.entries.iter().flatten();
        let facts: Facts = vec![
            ("Drawables".into(), dict.entries.len().to_string()),
            (
                "With geometry".into(),
                values().filter(|d| has_geometry(d)).count().to_string(),
            ),
            (
                "Skinned".into(),
                values()
                    .filter(|d| d.skeleton.is_some())
                    .count()
                    .to_string(),
            ),
            (
                "With collision".into(),
                values().filter(|d| d.bound.is_some()).count().to_string(),
            ),
            ("Lookup order".into(), lookup_order(&dict.codes).into()),
        ];
        draw_root_summary(ui, state, doc_id, "ydd_root", facts);
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        select_in(ui, state, doc_id, tr_key!("a drawable"));
        return;
    };
    let internals = shows_internals(state);
    let Some(payload) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    let DocPayload::Resource(or_io::Asset::DrawableDict(dict)) = payload else {
        return;
    };
    let Some(idx) = ydd_selected_entry_index(payload, sel) else {
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        select_in(ui, state, doc_id, tr_key!("a drawable"));
        return;
    };
    let Some(drawable) = dict.entries.get(idx).and_then(|d| d.as_deref()) else {
        return;
    };
    let (_, kind, sub) = dict_entry_unpack_part(sel);

    ui.heading("Drawable");
    ui.separator();
    let code = dict.codes.get(idx).copied().unwrap_or_default();
    let mut facts: Facts = vec![
        ("Name".into(), drawable.name.clone()),
        (
            "Entry".into(),
            format!("{} of {}", idx + 1, dict.entries.len()),
        ),
        (
            "Hash".into(),
            hash_text(code, state.name_index.name_of(code)),
        ),
    ];
    facts.extend(drawable_facts(
        &drawable.base,
        &drawable.lights,
        drawable.bound.as_deref(),
    ));
    draw_facts_grid(ui, "ydd_entry", &facts);

    match kind {
        YPT_MODEL_KIND_LOD => {
            let (lod, model, geom) = unpack_lod_sub_index(sub);
            let sel = pack_selection(lod, model as u16, geom.map(u32::from));
            draw_ydr_lod_geom(ui, drawable, sel, internals);
        }
        YPT_MODEL_KIND_SHADER => {
            // The entry index is the material's owner: two dictionary entries'
            // first materials are different materials.
            super::material::draw_material(ui, state, doc_id, idx, sub as usize);
        }
        YPT_MODEL_KIND_BONE => {
            let at = BoneAddress {
                owner: idx as u32,
                bone: sub as usize,
            };
            draw_bone(ui, state, doc_id, at);
        }
        YPT_MODEL_KIND_LIGHT => draw_ydr_light(ui, &drawable.lights, sub as usize),
        YPT_MODEL_KIND_BOUND => {
            draw_ydr_bound(ui, drawable.bound.as_deref(), SECT_BOUND | sub as u64)
        }
        YPT_MODEL_KIND_TEXTURE => {
            let texture = crate::state::textures::drawable_textures(drawable)
                .and_then(|td| td.entries.iter().flatten().nth(sub as usize));
            match texture {
                Some(tex) => draw_drawable_texture(ui, tex, internals),
                None => empty(ui, "Selection is no longer valid"),
            }
        }
        _ => {}
    }
}

/// Whether a dictionary's codes are in the ascending order the game's lookup assumes.
pub(super) fn lookup_order(codes: &[u32]) -> &'static str {
    if codes.windows(2).all(|w| w[0] <= w[1]) {
        "sorted"
    } else {
        "unsorted: the game may not find some entries"
    }
}

/// Whether any level of detail holds a model.
fn has_geometry(drawable: &or_io::gta5::GtaDrawable) -> bool {
    drawable.lod_group.lods.iter().any(|l| !l.models.is_empty())
}
