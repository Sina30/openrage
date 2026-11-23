use super::*;
use crate::state::textures::Texture;
use or_io::codec::mesh::VertexSource;
use or_io::decl::{Gen8, Gen9};
use or_io::gta5::PtxFxList;
use or_particle::list;

/// What the Inspector states of a model's shader, which each target lays out its own way.
pub(super) trait ShaderRows: ParticleTarget {
    fn rows(ui: &mut egui::Ui, shader: &Self::InstanceData);
}

impl ShaderRows for Gen8 {
    fn rows(ui: &mut egui::Ui, shader: &or_io::gta5::gen8::GrcInstanceData) {
        let effect_hash = shader.basis_hash.get();
        row(ui, "Name", &shader_name(effect_hash));
        row(ui, "Effect hash", &format!("0x{:08X}", effect_hash));
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
    }
}

impl ShaderRows for Gen9 {
    fn rows(ui: &mut egui::Ui, shader: &or_io::gta5::gen9::GrcInstanceDataG9) {
        let effect_hash = shader.name.get();
        row(ui, "Name", &shader_name(effect_hash));
        row(ui, "Effect hash", &format!("0x{:08X}", effect_hash));
        row(ui, "Preset hash", &format!("0x{:08X}", shader.preset.get()));
        row(ui, "Draw bucket", &shader.draw_bucket.to_string());
        row(
            ui,
            "Texture entries",
            &shader.textures().count().to_string(),
        );
        row(
            ui,
            "Total entries",
            &shader.param_infos.params.len().to_string(),
        );
    }
}

pub(super) fn draw_ypt_texture<T: ParticleTarget>(
    ui: &mut egui::Ui,
    payload: &DocPayload,
    sel: u64,
    internals: bool,
) {
    let Some(asset) = T::list(payload) else {
        return;
    };
    let Some(dict) = asset.texture_dict.as_deref() else {
        empty(ui, tr!("This YPT has no embedded texture dictionary"));
        return;
    };
    let Some(name) = ypt_texture_name_for_selection(payload, sel) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };
    let Some(tex) = (dict.entries.iter().flatten()).find(|t| T::name_of(t) == name) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };

    ui.label(
        egui::RichText::new(tr!("Embedded Texture"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ypt_tex", |ui| {
        row(ui, "Name", &name);
        texture_rows(ui, &T::wrap(tex), internals);
    });
}

pub(super) fn draw_ypt_model<T: ParticleTarget>(
    ui: &mut egui::Ui,
    asset: &PtxFxList<T>,
    hash: u32,
) {
    let Some(mesh) = list::model(asset, hash) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };
    let display_name = display_name_for_entry(asset, hash);
    ui.label(
        egui::RichText::new(tr!("Embedded Model"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ypt_model_meta", |ui| {
        row(ui, "Name", &display_name);
        row(ui, "Hash", &format!("0x{:08X}", hash));
    });

    ui.add_space(crate::ui::theme::SPACING_SNUG);
    ui.label(
        egui::RichText::new(tr!("Bound"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    let bound = list::model_info(asset, hash)
        .map(|d| d.bound_info.clone())
        .unwrap_or_default();
    grid(ui, "ypt_model_bound", |ui| {
        row(
            ui,
            "AABB (W \u{00d7} H \u{00d7} D)",
            &format!(
                "{:.2} \u{00d7} {:.2} \u{00d7} {:.2}",
                bound.x, bound.y, bound.z
            ),
        );
        row(ui, "Sphere radius", &format!("{:.3}", bound.w));
    });

    let lod_group = &mesh.lod_group;
    let total_models: usize = lod_group.lods.iter().map(|l| l.models.len()).sum();
    ui.add_space(crate::ui::theme::SPACING_SNUG);
    ui.label(
        egui::RichText::new(tr!("Mesh"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ypt_model_mesh", |ui| {
        row(
            ui,
            "Mesh radius",
            &format!("{:.3}", lod_group.bounds_sphere.center_and_radius.w),
        );
        row(ui, "Total models", &total_models.to_string());
        row(
            ui,
            "Has skeleton",
            if mesh.skeleton.is_some() { "yes" } else { "no" },
        );
        row(
            ui,
            "Shader group",
            if mesh.shader_group.is_some() {
                "yes"
            } else {
                "no"
            },
        );
    });
}

pub(super) fn draw_ypt_model_lod<T: ParticleTarget>(
    ui: &mut egui::Ui,
    asset: &PtxFxList<T>,
    hash: u32,
    sub: u32,
    internals: bool,
) {
    let Some(mesh) = list::model(asset, hash) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };
    let (lod_idx, model_idx, geom_idx) = unpack_lod_sub_index(sub);
    let lod_group = &mesh.lod_group;
    let Some(lod) = lod_group.lods.get(lod_idx as usize) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };
    let Some(model) = lod.models.get(model_idx as usize) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };

    ui.label(
        egui::RichText::new(tr!("Model"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ypt_model_lod", |ui| {
        row(ui, "Drawable", &display_name_for_entry(asset, hash));
        row(ui, "LOD", &lod_idx.to_string());
        row(ui, "Geometries", &model.geometries.len().to_string());
        row(
            ui,
            "Total verts",
            &model
                .geometries
                .iter()
                .map(|g| g.vertex_buffer.vertex_declared() as u32)
                .sum::<u32>()
                .to_string(),
        );
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
                egui::RichText::new(tr!("Geometry"))
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            grid(ui, "ypt_model_geom", |ui| {
                let indices = T::indices(&geom.index_buffer).len();
                row(
                    ui,
                    "Vertex count",
                    &geom.vertex_buffer.vertex_declared().to_string(),
                );
                row(ui, "Index count", &indices.to_string());
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

pub(super) fn draw_ypt_model_shader<T: ShaderRows>(
    ui: &mut egui::Ui,
    asset: &PtxFxList<T>,
    hash: u32,
    idx: u32,
) {
    let Some(mesh) = list::model(asset, hash) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };
    let Some(group) = &mesh.shader_group else {
        empty(ui, tr!("(no shader group)"));
        return;
    };
    let idx = idx as usize;
    let Some(shader) = group.shaders.get(idx) else {
        empty(ui, tr!("Selected shader is no longer valid"));
        return;
    };
    ui.label(
        egui::RichText::new(tr!("Shader"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ypt_model_shader", |ui| {
        row(ui, "Drawable", &display_name_for_entry(asset, hash));
        row(ui, "Index", &idx.to_string());
        T::rows(ui, shader);
    });
}

pub(super) fn draw_ypt_model_bone<T: ParticleTarget>(
    ui: &mut egui::Ui,
    asset: &PtxFxList<T>,
    hash: u32,
    idx: u32,
) {
    let Some(mesh) = list::model(asset, hash) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };
    let Some(skel) = &mesh.skeleton else {
        empty(ui, tr!("(no skeleton)"));
        return;
    };
    let idx = idx as usize;
    let Some(bone) = skel.bones.get(idx) else {
        empty(ui, tr!("Selected bone is no longer valid"));
        return;
    };
    ui.label(
        egui::RichText::new(tr!("Bone"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ypt_model_bone", |ui| {
        row(ui, "Drawable", &display_name_for_entry(asset, hash));
        row(ui, "Index", &idx.to_string());
        row(ui, "Name", &bone.name);
        row(ui, "Bone id", &bone.bone_id.to_string());
        row(ui, "Parent", &bone.parent_index.to_string());
        row(ui, "Next sibling", &bone.next_index.to_string());
        row(ui, "Mirror", &bone.mirror_index.to_string());
    });
}

pub(super) fn draw_ypt_model_texture<T: ParticleTarget>(
    ui: &mut egui::Ui,
    payload: &DocPayload,
    hash: u32,
    sel: u64,
    internals: bool,
) {
    use crate::ui::panels::selection_ids::ypt_model_texture_lookup;

    let Some(asset) = T::list(payload) else {
        return;
    };
    let Some(name) = ypt_model_texture_name_for_selection(payload, sel) else {
        empty(ui, tr!("Selection is no longer valid"));
        return;
    };
    ui.label(
        egui::RichText::new(tr!("Drawable Texture"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    let resolved = ypt_model_texture_lookup(asset, hash, &name);
    grid(ui, "ypt_model_tex", |ui| {
        row(ui, "Drawable", &display_name_for_entry(asset, hash));
        row(ui, "Name", &name);
        if let Some(tex) = resolved {
            texture_rows(ui, &T::wrap(tex), internals);
        } else {
            row(
                ui,
                "Source",
                "(unresolved \u{2014} not in any in-asset dictionary)",
            );
        }
    });
}

fn display_name_for_entry<T: ParticleTarget>(asset: &PtxFxList<T>, hash: u32) -> String {
    list::model_info(asset, hash)
        .map(|d| d.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| hash_text(hash, None))
}

pub(super) fn draw_ypt_models_overview<T: ParticleTarget>(ui: &mut egui::Ui, asset: &PtxFxList<T>) {
    if asset.model_dict.is_none() {
        empty(ui, tr!("This YPT has no embedded model dictionary"));
        return;
    }
    let total = list::models(asset).count();
    let named = list::models(asset)
        .filter(|(hash, _)| list::model_info(asset, *hash).is_some_and(|d| !d.name.is_empty()))
        .count();
    ui.label(
        egui::RichText::new(tr!("Embedded Model Dictionary"))
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "ypt_models_overview", |ui| {
        row(ui, "Drawables", &total.to_string());
        row(ui, "Resolved names", &format!("{} / {}", named, total));
    });
    ui.add_space(crate::ui::theme::SPACING_SNUG);
    ui.label(
        egui::RichText::new(tr!("Entries"))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .max_height(280.0)
        .show(ui, |ui| {
            grid(ui, "ypt_models_overview_list", |ui| {
                for (hash, mesh) in list::models(asset) {
                    let display = display_name_for_entry(asset, hash);
                    let radius = mesh.lod_group.bounds_sphere.center_and_radius.w;
                    row(ui, &display, &format!("0x{:08X}  •  r {:.2}", hash, radius));
                }
            });
        });
}

/// What an embedded texture states besides its name.
fn texture_rows(ui: &mut egui::Ui, tex: &Texture<'_>, internals: bool) {
    match tex {
        Texture::Owned(tex) => owned_texture_rows(ui, tex),
        Texture::Enhanced(tex) => enhanced_texture_rows(ui, tex, internals),
    }
}

fn owned_texture_rows(ui: &mut egui::Ui, tex: &or_io::gta5::gen8::GrcTexture) {
    use or_io::codec::texture::TextureSource;
    row(ui, "Width", &tex.width.to_string());
    row(ui, "Height", &tex.height.to_string());
    row(ui, "Mips", &tex.mip_count.to_string());
    row(ui, "Format", &format!("{:?}", tex.pixel_format()));
    row(ui, "sRGB", if tex.is_srgb() { "yes" } else { "no" });
    row(
        ui,
        "Pixel data",
        &format!("{} bytes", tex.backing_store.len()),
    );
    usage_rows(
        ui,
        tex.resource_type_and_conversion_flags,
        tex.cut_mip_levels,
    );
}
