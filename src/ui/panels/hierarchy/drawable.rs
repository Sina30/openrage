use super::*;
use crate::ui::panels::inspector::plural;
use crate::ui::panels::selection_ids::{payload_index, section_of};
use or_io::decl::{Gen8, Gen9};
use or_io::gta5::{gen8, gen9, DrawableTarget, RmcDrawable};

/// A texture as an outline row shows it: name, width, height and format.
pub(super) type TextureRow = (String, u16, u16, String);

/// A bound as an outline row shows it, with its children.
pub(super) struct BoundRow {
    pub label: String,
    pub composite: bool,
    pub polygonal: bool,
    pub stats: String,
    pub children: Vec<BoundRow>,
}

impl BoundRow {
    /// A drawable's owned bound.
    pub(super) fn of(bound: &PhBoundAny) -> Self {
        let children = match bound {
            PhBoundAny::PhBoundComposite(c) => {
                c.bounds.iter().flatten().map(|b| Self::of(b)).collect()
            }
            _ => Vec::new(),
        };
        Self {
            label: bound_type_name(bound),
            composite: matches!(bound, PhBoundAny::PhBoundComposite(_)),
            polygonal: bound_geometry(bound).is_some(),
            stats: bound_stats(bound),
            children,
        }
    }

    /// How many rows the bound's tree has: itself and every node below it.
    fn count(&self) -> u64 {
        1 + self.children.iter().map(Self::count).sum::<u64>()
    }
}

/// Whether the Outline is revealing one of the `count` bound nodes numbered from `first`.
fn bound_revealed(state: &AppState, doc_id: DocumentId, first: u64, count: u64) -> bool {
    revealing(state, doc_id)
        .filter(|row| section_of(*row) == SECT_BOUND)
        .is_some_and(|row| (first..first + count).contains(&u64::from(payload_index(row))))
}

/// What a drawable's outline shows, gathered from either target's drawable.
pub(super) struct DrawableOutline<'a> {
    pub name: &'a str,
    pub radius: f32,
    pub lods: LodCounts,
    /// Each shader's effect hash; `None` without a shader group.
    pub effects: Option<Vec<u32>>,
    /// The embedded textures; `None` without a dictionary.
    pub textures: Option<Vec<TextureRow>>,
    /// The skeleton's bone names; `None` without one.
    pub bones: Option<Vec<&'a str>>,
    /// Each light's type.
    pub lights: Vec<u8>,
    pub bound: Option<BoundRow>,
}

/// What an outline reads of a target's materials and textures.
pub(super) trait Outlined: DrawableTarget + Sized {
    fn effect_hash(shader: &Self::InstanceData) -> u32;
    fn texture_row(texture: &Self::Texture) -> TextureRow;
}

impl Outlined for Gen8 {
    fn effect_hash(shader: &gen8::GrcInstanceData) -> u32 {
        shader.basis_hash.get()
    }

    fn texture_row(t: &gen8::GrcTexture) -> TextureRow {
        let format = GrcTextureFormat::from(t.format);
        (t.name.clone(), t.width, t.height, format!("{format:?}"))
    }
}

impl Outlined for Gen9 {
    fn effect_hash(shader: &gen9::GrcInstanceDataG9) -> u32 {
        shader.name.get()
    }

    fn texture_row(t: &gen9::GrcTextureG9) -> TextureRow {
        (t.name.clone(), t.width, t.height, format!("{:?}", t.format))
    }
}

impl<'a> DrawableOutline<'a> {
    /// A drawable, with the lights and the bound its container adds.
    pub(super) fn of<T: Outlined>(
        drawable: &'a RmcDrawable<T>,
        lights: &[or_io::gta5::CLightAttr],
        bound: Option<&PhBoundAny>,
    ) -> Self {
        let group = drawable.shader_group.as_deref();
        Self {
            name: &drawable.name,
            radius: drawable.lod_group.bounds_sphere.center_and_radius.w,
            lods: owned_lod_counts(drawable),
            effects: group.map(|g| g.shaders.iter().map(|s| T::effect_hash(s)).collect()),
            textures: group
                .and_then(|g| g.texture_dictionary.as_deref())
                .map(|td| {
                    (td.entries.iter().flatten())
                        .map(|t| T::texture_row(t))
                        .collect()
                }),
            bones: (drawable.skeleton.as_deref())
                .map(|s| s.bones.iter().map(|b| b.name.as_str()).collect()),
            lights: lights.iter().map(|l| l.light_type).collect(),
            bound: bound.map(BoundRow::of),
        }
    }
}

pub(super) fn section_drawable_textures(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    rows: &[TextureRow],
    id_root: egui::Id,
) {
    use crate::ui::dock::Workspace;
    use crate::ui::panels::selection_ids::{SECT_TEXTURE, YDR_TEXTURE_DICT};

    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();
    let header_selected = state.selection.is_selected(doc_id, YDR_TEXTURE_DICT);
    let header_color = if header_selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let header_resp = collapsing_row(
        ui,
        id_root.with("ydr_drawable_textures"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::TextureDict),
                format!("Textures ({})", rows.len()),
                header_color.into(),
            );
            ui.selectable_label(header_selected, job)
        },
        |ui| {
            for (name, width, height, format) in rows {
                if filter_active && !name.to_ascii_lowercase().contains(&filter_lc) {
                    continue;
                }
                let sel =
                    SECT_TEXTURE | (name_hash(name) & !crate::ui::panels::selection_ids::SECT_MASK);
                let selected = state.selection.is_selected(doc_id, sel);
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(&icons::ytd_texture_icon(), name, label_color.into());
                let clicked = ui
                    .horizontal(|ui| {
                        let r = ui.selectable_label(selected, job);
                        ui.add(detail_label(format!(
                            "{}\u{00d7}{}  {}",
                            width, height, format
                        )));
                        r.clicked()
                    })
                    .inner;
                if clicked {
                    state.selection.select(doc_id, sel);
                    state.active_workspace = Workspace::Texture;
                }
            }
        },
    );
    if header_resp.clicked() {
        state.selection.select(doc_id, YDR_TEXTURE_DICT);
        state.active_workspace = Workspace::Texture;
    }
}

pub(super) fn section_drawable_tree(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    outline: &DrawableOutline<'_>,
    id_root: egui::Id,
) {
    section_lods(ui, state, doc_id, &outline.lods, id_root);

    if let Some(effects) = &outline.effects {
        section_shaders(ui, state, doc_id, effects, id_root);
        if let Some(rows) = &outline.textures {
            section_drawable_textures(ui, state, doc_id, rows, id_root);
        }
    }

    if let Some(bones) = &outline.bones {
        section_skeleton(ui, state, doc_id, bones, id_root);
    }

    if !outline.lights.is_empty() {
        section_lights(ui, state, doc_id, &outline.lights, id_root);
    }

    if let Some(bound) = &outline.bound {
        section_bound(ui, state, doc_id, bound, id_root);
    }
}

pub(crate) const LOD_LABELS: [&str; 4] = ["High", "Medium", "Low", "VeryLow"];

pub(super) fn section_lods(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    lods: &LodCounts,
    id_root: egui::Id,
) {
    use crate::ui::panels::selection_ids::{section_of, unpack_selection, SECT_LOD};
    let total_models: usize = lods.iter().flatten().map(Vec::len).sum();
    if let Some(row) = revealing(state, doc_id).filter(|row| section_of(*row) == SECT_LOD) {
        force_open_collapsing(ui.ctx(), id_root.with("lods"));
        let lod = usize::from(unpack_selection(row).0);
        force_open_collapsing(ui.ctx(), id_root.with(("lod", lod)));
    }
    collapsing_row(
        ui,
        id_root.with("lods"),
        true,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Lods),
                format!("LODs ({})", plural(total_models, "model")),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for (lod_idx, models) in lods.iter().enumerate() {
                let Some(models) = models else {
                    continue;
                };
                if models.is_empty() {
                    continue;
                }
                let header = format!(
                    "{} ({} model{})",
                    LOD_LABELS.get(lod_idx).copied().unwrap_or("?"),
                    models.len(),
                    if models.len() == 1 { "" } else { "s" },
                );
                collapsing_row(
                    ui,
                    id_root.with(("lod", lod_idx)),
                    lod_idx == 0,
                    |ui, _open| {
                        let job = icons::icon_label(
                            &icons::ydr_section_icon(YdrSection::Lod),
                            header,
                            theme::TEXT_PRIMARY.get(),
                        );
                        ui.add(
                            egui::Label::new(job)
                                .selectable(false)
                                .sense(egui::Sense::click()),
                        )
                    },
                    |ui| {
                        for (m_idx, model) in models.iter().enumerate() {
                            let model_sel = pack_selection(lod_idx as u8, m_idx as u16, None);
                            let model_selected = state.selection.is_selected(doc_id, model_sel);
                            let total_verts: u32 = model.iter().map(|(v, _)| v).sum();
                            let label_color = if model_selected {
                                theme::ACCENT_TEXT
                            } else {
                                theme::TEXT_PRIMARY
                            };
                            let job = icons::icon_label(
                                &icons::ydr_section_icon(YdrSection::Model),
                                format!(
                                    "Model {} \u{2022} {} geo \u{2022} {} verts",
                                    m_idx,
                                    model.len(),
                                    total_verts,
                                ),
                                label_color.into(),
                            );
                            let resp = ui.selectable_label(model_selected, job);
                            pulse_scroll_into_view(state, doc_id, model_sel, &resp);
                            if resp.clicked() {
                                state.selection.select(doc_id, model_sel);
                            }
                            for (g_idx, (verts, indices)) in model.iter().enumerate() {
                                let g_sel =
                                    pack_selection(lod_idx as u8, m_idx as u16, Some(g_idx as u32));
                                let g_selected = state.selection.is_selected(doc_id, g_sel);
                                let label_color = if g_selected {
                                    theme::ACCENT_TEXT
                                } else {
                                    theme::TEXT_SECONDARY
                                };
                                let job = icons::icon_label_small(
                                    &icons::ydr_section_icon(YdrSection::Geometry),
                                    format!(
                                        "Geometry {} \u{2022} {} verts \u{2022} {} idx",
                                        g_idx, verts, indices
                                    ),
                                    label_color.into(),
                                );
                                let resp = ui.selectable_label(g_selected, job);
                                pulse_scroll_into_view(state, doc_id, g_sel, &resp);
                                if resp.clicked() {
                                    state.selection.select(doc_id, g_sel);
                                }
                            }
                        }
                    },
                );
            }
        },
    );
}

pub(super) fn section_shaders(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    effects: &[u32],
    id_root: egui::Id,
) {
    collapsing_row(
        ui,
        id_root.with("shaders"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Shaders),
                format!("Shaders ({})", effects.len()),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for (i, effect_hash) in effects.iter().enumerate() {
                let sel = pack_index(SECT_SHADER, i as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let resolved = crate::ui::panels::effect_name(*effect_hash);
                let display_name = if resolved.is_empty() {
                    format!("Shader {}", i)
                } else {
                    format!("Shader {} \u{2014} {}", i, resolved)
                };
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ydr_section_icon(YdrSection::Shader),
                    display_name,
                    label_color.into(),
                );
                if ui.selectable_label(selected, job).clicked() {
                    state.selection.select(doc_id, sel);
                }
            }
        },
    );
}

pub(super) fn section_skeleton(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    bones: &[&str],
    id_root: egui::Id,
) {
    if revealing(state, doc_id).is_some_and(|row| section_of(row) == SECT_SKEL) {
        force_open_collapsing(ui.ctx(), id_root.with("skeleton"));
    }
    collapsing_row(
        ui,
        id_root.with("skeleton"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Skeleton),
                format!("Skeleton ({} bones)", bones.len()),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for (i, name) in bones.iter().enumerate() {
                let sel = pack_index(SECT_SKEL, i as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let display_name = if name.is_empty() {
                    format!("Bone {}", i)
                } else {
                    format!("{} ({})", name, i)
                };
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ydr_section_icon(YdrSection::Bone),
                    display_name,
                    label_color.into(),
                );
                let resp = ui.selectable_label(selected, job);
                pulse_scroll_into_view(state, doc_id, sel, &resp);
                if resp.clicked() {
                    state.selection.select(doc_id, sel);
                }
            }
        },
    );
}

pub(super) fn section_lights(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    lights: &[u8],
    id_root: egui::Id,
) {
    collapsing_row(
        ui,
        id_root.with("lights"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Lights),
                format!("Lights ({})", lights.len()),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for (i, light_type) in lights.iter().enumerate() {
                let sel = pack_index(SECT_LIGHT, i as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let kind = light_type_name(*light_type);
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ydr_section_icon(YdrSection::Light),
                    format!("Light {} \u{2014} {}", i, kind),
                    label_color.into(),
                );
                if ui.selectable_label(selected, job).clicked() {
                    state.selection.select(doc_id, sel);
                }
            }
        },
    );
}

pub(super) fn light_type_name(kind: u8) -> &'static str {
    match kind {
        1 => "Point",
        2 => "Spot",
        4 => "Capsule",
        _ => "Other",
    }
}

pub(super) fn section_bound(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    bound: &BoundRow,
    id_root: egui::Id,
) {
    if bound_revealed(state, doc_id, 0, bound.count()) {
        force_open_collapsing(ui.ctx(), id_root.with("bound"));
    }
    let header_resp = collapsing_row(
        ui,
        id_root.with("bound"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Bound),
                "Bound",
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            let mut next_id: u64 = 0;
            draw_bound_node(
                ui,
                state,
                doc_id,
                bound,
                &mut next_id,
                id_root.with("bound_tree"),
                "Root",
            );
        },
    );
    let root_sel = SECT_BOUND | root_selection_id();
    if header_resp.clicked() {
        state.selection.select(doc_id, root_sel);
    }
    if header_resp.double_clicked() {
        state.selection.select(doc_id, root_sel);
        state.active_workspace = Workspace::Model;
    }
}

fn draw_bound_node(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    bound: &BoundRow,
    next_id: &mut u64,
    parent_id: egui::Id,
    label_prefix: &str,
) {
    let id = *next_id;
    *next_id += 1;
    let sel = SECT_BOUND | id;

    let header_text = format!("{} ({})", label_prefix, bound.label);

    let selected = state.selection.is_selected(doc_id, sel);
    let row_id = parent_id.with(("bound", id));

    let label_color = if selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let icon = icons::bound_icon(bound.composite, bound.polygonal);
    if bound.composite {
        if bound_revealed(state, doc_id, id, bound.count()) {
            force_open_collapsing(ui.ctx(), row_id);
        }
        let job = icons::icon_label(&icon, header_text.clone(), label_color.into());
        let response = collapsing_row(
            ui,
            row_id,
            id == 0,
            |ui, _open| {
                let resp = ui.selectable_label(selected, job);
                ui.add(detail_label(bound.stats.clone()));
                resp
            },
            |ui| {
                for (i, child) in bound.children.iter().enumerate() {
                    draw_bound_node(
                        ui,
                        state,
                        doc_id,
                        child,
                        next_id,
                        row_id.with("children"),
                        &format!("Child {}", i),
                    );
                }
            },
        );
        // ⚠ A closed composite draws none of its children, so the numbers its subtree holds are
        // skipped here, as the pick numbers every node.
        *next_id = id + bound.count();
        pulse_scroll_into_view(state, doc_id, sel, &response);
        if response.clicked() {
            state.selection.select(doc_id, sel);
        }
        if response.double_clicked() {
            state.selection.select(doc_id, sel);
            state.active_workspace = Workspace::Model;
        }
    } else {
        let job = icons::icon_label(&icon, header_text, label_color.into());
        let resp = leaf_row(ui, |ui| {
            let r = ui.selectable_label(selected, job);
            ui.add(detail_label(bound.stats.clone()));
            r
        });
        pulse_scroll_into_view(state, doc_id, sel, &resp);
        if resp.clicked() {
            state.selection.select(doc_id, sel);
        }
        if resp.double_clicked() {
            state.selection.select(doc_id, sel);
            state.active_workspace = Workspace::Model;
        }
    }
}

// ---------------------------------------------------------------------------
// The same sections again, keyed by a container entry's hash
// ---------------------------------------------------------------------------
//
// Parameterised on a `model_hash` so every selection id is unique per model.
// Named `ypt_*`, and three formats call them: `ydd.rs` for a dictionary entry,
// `yft.rs` for a fragment's common drawable, `ypt.rs` for an embedded particle
// mesh.
//
// Each differs from its `section_*` twin above only in **the selection-id
// expression and the `egui::Id` key** (twelve substantive differences across
// 231 lines, plus an early return the YDR forms deliberately do not have).
// ⚠ Both the ids and the keys are persisted state, so getting either wrong in
// a merge of the pairs changes what a user sees without changing anything a
// test looks at.
//
// `Lights`, `Tint` and the YDR-style `Bound` (PhBound) sections are
// deliberately NOT ported; the bound info that *is* meaningful for a particle
// drawable is surfaced by the `ypt_model_bound_row` leaf in `ypt/models.rs`.

/// Per level of detail, `None` where the slot is empty; per model, each geometry's vertex and index counts.
pub(super) type LodCounts = Vec<Option<Vec<Vec<(u32, u32)>>>>;

/// An owned drawable's level-of-detail counts, as the model rows show them.
pub(super) fn owned_lod_counts<T: DrawableTarget>(mesh: &RmcDrawable<T>) -> LodCounts {
    use or_io::codec::mesh::VertexSource;
    mesh.lod_group
        .lods
        .iter()
        .map(|lod| {
            Some(
                lod.models
                    .iter()
                    .map(|m| {
                        m.geometries
                            .iter()
                            .map(|g| {
                                (
                                    g.vertex_buffer.vertex_declared() as u32,
                                    T::indices(&g.index_buffer).len() as u32,
                                )
                            })
                            .collect()
                    })
                    .collect(),
            )
        })
        .collect()
}

pub(super) fn ypt_model_section_lods(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    model_hash: u32,
    lods: &LodCounts,
    id_root: egui::Id,
) {
    use crate::ui::panels::selection_ids::{dict_entry_unpack_part, unpack_lod_sub_index};
    let total_models: usize = lods.iter().flatten().map(Vec::len).sum();
    if total_models == 0 {
        return;
    }
    let revealed = revealing(state, doc_id)
        .filter(|row| is_dict_entry_selection(*row))
        .map(dict_entry_unpack_part);
    if let Some((_, _, sub)) =
        revealed.filter(|(hash, kind, _)| (*hash, *kind) == (model_hash, YPT_MODEL_KIND_LOD))
    {
        force_open_collapsing(ui.ctx(), id_root.with("ypt_model_lods"));
        let lod = usize::from(unpack_lod_sub_index(sub).0);
        force_open_collapsing(ui.ctx(), id_root.with(("ypt_model_lod", lod)));
    }
    collapsing_row(
        ui,
        id_root.with("ypt_model_lods"),
        true,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Lods),
                format!("LODs ({})", plural(total_models, "model")),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for (lod_idx, models) in lods.iter().enumerate() {
                let Some(models) = models else {
                    continue;
                };
                if models.is_empty() {
                    continue;
                }
                let header = format!(
                    "{} ({} model{})",
                    LOD_LABELS.get(lod_idx).copied().unwrap_or("?"),
                    models.len(),
                    if models.len() == 1 { "" } else { "s" },
                );
                collapsing_row(
                    ui,
                    id_root.with(("ypt_model_lod", lod_idx)),
                    lod_idx == 0,
                    |ui, _open| {
                        let job = icons::icon_label(
                            &icons::ydr_section_icon(YdrSection::Lod),
                            header,
                            theme::TEXT_PRIMARY.get(),
                        );
                        ui.add(
                            egui::Label::new(job)
                                .selectable(false)
                                .sense(egui::Sense::click()),
                        )
                    },
                    |ui| {
                        for (m_idx, model) in models.iter().enumerate() {
                            let sub = pack_lod_sub_index(lod_idx as u8, m_idx as u8, None);
                            let model_sel = ypt_model_part_id(model_hash, YPT_MODEL_KIND_LOD, sub);
                            let model_selected = state.selection.is_selected(doc_id, model_sel);
                            let total_verts: u32 = model.iter().map(|(v, _)| v).sum();
                            let label_color = if model_selected {
                                theme::ACCENT_TEXT
                            } else {
                                theme::TEXT_PRIMARY
                            };
                            let job = icons::icon_label(
                                &icons::ydr_section_icon(YdrSection::Model),
                                format!(
                                    "Model {} \u{2022} {} geo \u{2022} {} verts",
                                    m_idx,
                                    model.len(),
                                    total_verts,
                                ),
                                label_color.into(),
                            );
                            let resp = ui.selectable_label(model_selected, job);
                            pulse_scroll_into_view(state, doc_id, model_sel, &resp);
                            if resp.clicked() {
                                state.selection.select(doc_id, model_sel);
                            }
                            for (g_idx, (verts, indices)) in model.iter().enumerate() {
                                let sub = pack_lod_sub_index(
                                    lod_idx as u8,
                                    m_idx as u8,
                                    Some(g_idx as u16),
                                );
                                let g_sel = ypt_model_part_id(model_hash, YPT_MODEL_KIND_LOD, sub);
                                let g_selected = state.selection.is_selected(doc_id, g_sel);
                                let label_color = if g_selected {
                                    theme::ACCENT_TEXT
                                } else {
                                    theme::TEXT_SECONDARY
                                };
                                let job = icons::icon_label_small(
                                    &icons::ydr_section_icon(YdrSection::Geometry),
                                    format!(
                                        "Geometry {} \u{2022} {} verts \u{2022} {} idx",
                                        g_idx, verts, indices
                                    ),
                                    label_color.into(),
                                );
                                let resp = ui.selectable_label(g_selected, job);
                                pulse_scroll_into_view(state, doc_id, g_sel, &resp);
                                if resp.clicked() {
                                    state.selection.select(doc_id, g_sel);
                                }
                            }
                        }
                    },
                );
            }
        },
    );
}

/// One row per shader, by the hash of its effect's name.
pub(super) fn ypt_model_section_shaders(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    model_hash: u32,
    shaders: &[u32],
    id_root: egui::Id,
) {
    if shaders.is_empty() {
        return;
    }
    collapsing_row(
        ui,
        id_root.with("ypt_model_shaders"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Shaders),
                format!("Shaders ({})", shaders.len()),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for (i, effect_hash) in shaders.iter().enumerate() {
                let sel = ypt_model_part_id(model_hash, YPT_MODEL_KIND_SHADER, i as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let resolved = crate::ui::panels::effect_name(*effect_hash);
                let display_name = if resolved.is_empty() {
                    format!("Shader {}", i)
                } else {
                    format!("Shader {} \u{2014} {}", i, resolved)
                };
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ydr_section_icon(YdrSection::Shader),
                    display_name,
                    label_color.into(),
                );
                if ui.selectable_label(selected, job).clicked() {
                    state.selection.select(doc_id, sel);
                }
            }
        },
    );
}

pub(super) fn ypt_model_section_skeleton(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    model_hash: u32,
    bones: &[&str],
    id_root: egui::Id,
) {
    if bones.is_empty() {
        return;
    }
    let revealed = revealing(state, doc_id)
        .filter(|row| is_dict_entry_selection(*row))
        .map(crate::ui::panels::selection_ids::dict_entry_unpack_part);
    if revealed.is_some_and(|(hash, kind, _)| (hash, kind) == (model_hash, YPT_MODEL_KIND_BONE)) {
        force_open_collapsing(ui.ctx(), id_root.with("ypt_model_skeleton"));
    }
    collapsing_row(
        ui,
        id_root.with("ypt_model_skeleton"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Skeleton),
                format!("Skeleton ({} bones)", bones.len()),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for (i, name) in bones.iter().enumerate() {
                let sel = ypt_model_part_id(model_hash, YPT_MODEL_KIND_BONE, i as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let display_name = if name.is_empty() {
                    format!("Bone {}", i)
                } else {
                    format!("{} ({})", name, i)
                };
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ydr_section_icon(YdrSection::Bone),
                    display_name,
                    label_color.into(),
                );
                let resp = ui.selectable_label(selected, job);
                pulse_scroll_into_view(state, doc_id, sel, &resp);
                if resp.clicked() {
                    state.selection.select(doc_id, sel);
                }
            }
        },
    );
}
