use super::*;

use or_io::decl::Gen8;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::{CLightAttr, PhBoundAny, RmcDrawable};

/// A Legacy drawable's embedded texture, as the inspector shows one.
pub(super) fn draw_drawable_texture(ui: &mut egui::Ui, tex: &GrcTexture, internals: bool) {
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Drawable Texture")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    texture_grid(ui, "ydr_drawable_tex", tex, internals);
}

pub(super) fn draw_ydr_drawable_texture(
    ui: &mut egui::Ui,
    drawable: &RmcDrawable<Gen8>,
    sel: u64,
    internals: bool,
) {
    let Some(group) = drawable.shader_group.as_deref() else {
        empty(ui, "(drawable has no shader group)");
        return;
    };
    let Some(td) = group.texture_dictionary.as_deref() else {
        empty(ui, "(shader group has no texture dictionary)");
        return;
    };
    let target = sel & !crate::ui::panels::selection_ids::SECT_MASK;
    let found = (td.entries.iter().flatten()).find(|t| {
        (crate::ui::panels::selection_ids::name_hash(&t.name)
            & !crate::ui::panels::selection_ids::SECT_MASK)
            == target
    });
    match found {
        Some(tex) => draw_drawable_texture(ui, tex, internals),
        None => empty(ui, "Selection is no longer valid"),
    }
}

pub(super) fn draw_ydr_lod_geom(
    ui: &mut egui::Ui,
    drawable: &RmcDrawable<Gen8>,
    sel: u64,
    internals: bool,
) {
    use or_io::codec::mesh::VertexSource;

    let (lod_idx, model_idx, geom_idx) = unpack_selection(sel);
    let Some(lod) = drawable.lod_group.lods.get(lod_idx as usize) else {
        return;
    };
    let Some(model) = lod.models.get(model_idx as usize) else {
        return;
    };

    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Model")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    let total_verts: usize = (model.geometries.iter())
        .map(|g| g.vertex_buffer.vertex_count())
        .sum();
    grid(ui, "ydr_model", |ui| {
        row(ui, "LOD", &lod_idx.to_string());
        row(ui, "Geometries", &model.geometries.len().to_string());
        row(ui, "Total verts", &total_verts.to_string());
        row(
            ui,
            "Skinning",
            if model.matrix_count > 0 { "yes" } else { "no" },
        );
    });

    if let Some(g_idx) = geom_idx {
        if let Some(geom) = model.geometries.get(g_idx as usize) {
            ui.add_space(crate::ui::theme::SPACING_TIGHT);
            ui.label(
                egui::RichText::new("Geometry")
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            grid(ui, "ydr_geom", |ui| {
                row(
                    ui,
                    "Vertex count",
                    &geom.vertex_buffer.vertex_count().to_string(),
                );
                row(
                    ui,
                    "Index count",
                    &geom.index_buffer.indices.len().to_string(),
                );
                row(ui, "Primitive count", &geom.primitive_count().to_string());
                if internals {
                    row(
                        ui,
                        "Vertex stride",
                        &geom.vertex_buffer.vertex_stride().to_string(),
                    );
                }
            });
        }
    }
}

pub(super) fn draw_ydr_shader(ui: &mut egui::Ui, drawable: &RmcDrawable<Gen8>, idx: usize) {
    let Some(group) = drawable.shader_group.as_deref() else {
        return;
    };
    let Some(shader) = group.shaders.get(idx) else {
        empty(ui, "Selected shader is no longer valid");
        return;
    };
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Shader")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    let effect = shader.basis_hash.get();
    grid(ui, "ydr_shader", |ui| {
        row(ui, "Index", &idx.to_string());
        let display_name = shader_name(effect);
        row(ui, "Name", &display_name);
        row(ui, "Effect hash", &format!("0x{:08X}", effect));
        row(
            ui,
            "Material hash",
            &format!("0x{:08X}", shader.material_hash.get()),
        );
        row(ui, "Draw bucket", &shader.draw_bucket.to_string());
        row(
            ui,
            "Texture entries",
            &shader.textures().count().to_string(),
        );
        row(ui, "Total entries", &shader.entries.len().to_string());
    });
}

pub(super) fn draw_ydr_light(ui: &mut egui::Ui, lights: &[CLightAttr], idx: usize) {
    let Some(light) = lights.get(idx) else {
        empty(ui, "Selected light is no longer valid");
        return;
    };
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Light")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ydr_light", |ui| {
        row(ui, "Index", &idx.to_string());
        row(
            ui,
            "Type",
            match light.light_type {
                1 => "Point",
                2 => "Spot",
                4 => "Capsule",
                _ => "Other",
            },
        );
        row(ui, "Intensity", &format!("{:.3}", light.intensity));
        row(ui, "Falloff", &format!("{:.3}", light.falloff));
        row(ui, "Bone tag", &light.bone_tag.to_string());
        row(ui, "Flags", &format!("0x{:08X}", light.flags));
    });
}

pub(super) fn draw_ydr_bound(ui: &mut egui::Ui, bound: Option<&PhBoundAny>, sel: u64) {
    let Some(root) = bound else {
        return;
    };
    let target_idx = payload_index(sel) as u64;
    let target = if target_idx == root_selection_id() {
        root
    } else {
        let mut idx = 0u64;
        find_child_bound(root, target_idx, &mut idx).unwrap_or(root)
    };
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Bound (embedded)")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    draw_bound_grid(ui, "ydr_bound", target);
}
