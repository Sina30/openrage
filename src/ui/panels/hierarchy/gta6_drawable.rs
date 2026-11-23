//! A scanned GTA VI drawable's Outline through the rows GTA V's drawables draw: its levels' models
//! and geometries, its shaders and its skeleton; a dictionary's drawables each under its code.

use std::collections::BTreeMap;
use std::sync::Arc;

use or_io::gta6::headerless::{Drawable, Resource};

use super::*;
use crate::state::gta6_resource::dictionary_drawables;

/// Each geometry's vertex and index counts, per model of each level, as GTA V's rows show them.
fn lod_counts(d: &Drawable) -> LodCounts {
    let counts = |g: &Result<or_io::gta6::headerless::Geometry, _>| match g {
        Ok(g) => (g.vertex_count, 3 * g.triangles.len() as u32),
        Err(_) => (0, 0),
    };
    (d.levels.iter())
        .map(|level| {
            let models = level.as_ref()?;
            Some(
                (models.iter())
                    .map(|m| m.geometries.iter().map(counts).collect())
                    .collect(),
            )
        })
        .collect()
}

fn outline_of<'a>(d: &'a Drawable, bones: Option<Vec<&'a str>>) -> DrawableOutline<'a> {
    let (min, max) = (glam::Vec3::from(d.box_min), glam::Vec3::from(d.box_max));
    DrawableOutline {
        name: &d.name,
        radius: (max - min).length() / 2.0,
        lods: lod_counts(d),
        effects: Some(d.shaders.iter().map(|s| s.name_hash).collect()),
        textures: None,
        bones,
        lights: Vec::new(),
        bound: None,
    }
}

/// The bone names of `doc`'s scanned skeleton, kept while its payload is the same.
fn bone_names(ui: &egui::Ui, doc: &Document) -> Option<Arc<Vec<String>>> {
    let id = egui::Id::new(("gta6_outline_bones", doc.id));
    let epoch = doc.payload_epoch();
    type Kept = (u64, Option<Arc<Vec<String>>>);
    if let Some((at, names)) = ui.data(|d| d.get_temp::<Kept>(id)) {
        if at == epoch {
            return names;
        }
    }
    let skeleton = crate::state::rpf9_scan::scanned_skeleton(doc.path.as_deref()?).ok();
    let names = skeleton.map(|s| Arc::new(s.bones.into_iter().map(|b| b.name).collect()));
    ui.data_mut(|d| d.insert_temp(id, (epoch, names.clone())));
    names
}

/// The bone names of each skeleton `doc`'s dictionary entries held when it opened, by code, kept
/// while its payload is the same.
fn entry_bone_names(
    ui: &egui::Ui,
    doc: &Document,
    resource: &Resource,
) -> Arc<BTreeMap<u32, Vec<String>>> {
    let id = egui::Id::new(("gta6_outline_entry_bones", doc.id));
    let epoch = doc.payload_epoch();
    type Kept = (u64, Arc<BTreeMap<u32, Vec<String>>>);
    if let Some((at, names)) = ui.data(|d| d.get_temp::<Kept>(id)) {
        if at == epoch {
            return names;
        }
    }
    let path = doc.path.as_deref();
    let names: BTreeMap<u32, Vec<String>> = dictionary_drawables(resource)
        .filter_map(|(code, _)| {
            let skeleton = crate::state::gta6_entry_skeletons::of(path?, code)?;
            Some((code, skeleton.bones.into_iter().map(|b| b.name).collect()))
        })
        .collect();
    let names = Arc::new(names);
    ui.data_mut(|d| d.insert_temp(id, (epoch, Arc::clone(&names))));
    names
}

/// The textures the drawable's shaders bind, each with the size and format an open pack states for its key.
fn texture_rows(state: &AppState, doc_id: DocumentId) -> Vec<TextureRow> {
    use crate::state::textures::{self, TextureListing};
    let Some(doc) = state.document(doc_id) else {
        return Vec::new();
    };
    let listed = textures::textures_in(&doc.payload, state.selection.selected(doc_id));
    let TextureListing::Textures { textures, .. } = textures::sized_for_gta6(state, doc_id, listed)
    else {
        return Vec::new();
    };
    (textures.into_iter())
        .map(|t| {
            let format = match t.width {
                0 => tr!("unresolved").to_string(),
                _ => format!("{:?}", t.format),
            };
            (t.name, t.width as u16, t.height as u16, format)
        })
        .collect()
}

pub(super) fn draw_gta6_drawable(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(doc) = state.document(doc_id) else {
        return;
    };
    let DocPayload::Gta6Drawable(resource) = &doc.payload else {
        return;
    };
    let resource = Arc::clone(resource);
    let bones = bone_names(ui, doc);
    let drawable = match &*resource {
        Resource::Drawable(d) => d,
        Resource::DrawableWithBound(r) => &r.drawable,
        Resource::Dictionary(_) => {
            // Each entry lists the bones of the skeleton it holds, as a `.ydd`'s entries do.
            let names = entry_bone_names(ui, doc, &resource);
            let bones_of = |code: u32| {
                (names.get(&code)).map(|n| n.iter().map(String::as_str).collect::<Vec<_>>())
            };
            let entries: Vec<(usize, u32, DrawableOutline<'_>)> = dictionary_drawables(&resource)
                .enumerate()
                .map(|(i, (code, d))| (i, code, outline_of(d, bones_of(code))))
                .collect();
            let target = or_io::Generation::from_id("gta-vi");
            // A dictionary of frame filters and animation records lists those instead.
            if !super::gta6_filters::holds_filters(&resource) || !entries.is_empty() {
                draw_ydd_dictionary(ui, state, doc_id, &entries, target);
            }
            return super::gta6_filters::draw_gta6_filters(ui, state, doc_id, &resource);
        }
        _ => return,
    };
    let bones = bones
        .as_deref()
        .map(|b| b.iter().map(String::as_str).collect());
    let mut outline = outline_of(drawable, bones);
    outline.textures = Some(texture_rows(state, doc_id));
    leaf_row(ui, |ui| {
        let job = icons::icon_label(
            &icons::doc_kind_icon(DocumentKind::Ydr, false),
            outline.name,
            theme::ACCENT_TEXT.get(),
        );
        ui.add(egui::Label::new(job).selectable(false));
        ui.add(detail_label(format!(
            "Bounds radius: {:.2}",
            outline.radius
        )));
    });
    let id_root = ui.make_persistent_id("hierarchy_gta6_drawable");
    section_drawable_tree(ui, state, doc_id, &outline, id_root);
    if let Resource::DrawableWithBound(r) = &*resource {
        super::gta6_fragment::section_fragment(ui, &r.fragment, outline.bones.as_deref(), id_root);
    }
}
