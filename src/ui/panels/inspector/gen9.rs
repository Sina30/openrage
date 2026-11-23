use or_io::codec::texture::TextureSource as _;
use or_io::gta5::gen9::GrcInstanceDataG9;
use or_io::gta5::shaders::shader_defs::{self, ShaderParamDef, Spelling};
use or_io::gta5::TextureSlot;
use or_io::Generation;

use super::*;
use crate::state::{enhanced_drawable_mut, Change};

/// What an Enhanced texture's header declares, read from `GrcTextureG9`.
///
/// ⚠ **No `cut_mip_levels` row, and its absence is the point.** That field is
/// Legacy's, `gen9::convert` zeroes it on the way across, and a Gen9 grid that
/// showed one would be reporting a Gen8 fact about an Enhanced file. What
/// Enhanced states instead is `usage_data`, which has no Legacy counterpart
/// either.
///
/// Read-only, for the same reason [`usage_rows`] is: a header that
/// calls a texture a render target writes a file the game reads as one.
pub(super) fn draw_enhanced_texture(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) -> bool {
    let Some(doc) = state.document(doc_id) else {
        return false;
    };
    if !doc.payload.is_enhanced() {
        return false;
    }
    let Some(selection) = selection.filter(|&s| s != DOC_ROOT_SEL) else {
        return false;
    };
    let crate::state::TextureResolution::Found { name, source } =
        crate::state::textures::texture_for_selection(&doc.payload, selection)
    else {
        return false;
    };
    let Some(tex) = crate::state::textures::enhanced_texture_at(&doc.payload, source, &name) else {
        return false;
    };

    let internals = shows_internals(state);
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Texture (Enhanced)")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "gen9_tex", |ui| {
        row(ui, "Name", &tex.name);
        enhanced_texture_rows(ui, tex, internals);
    });
    true
}

/// What an Enhanced texture's header declares besides its name; the internals follow only where asked.
pub(super) fn enhanced_texture_rows(
    ui: &mut egui::Ui,
    tex: &or_io::gta5::gen9::GrcTextureG9,
    internals: bool,
) {
    row(
        ui,
        "Dimensions",
        &format!(
            "{} \u{00d7} {} \u{00d7} {} px",
            tex.width, tex.height, tex.depth
        ),
    );
    row(ui, "Type", &format!("{:?}", tex.dimension));
    row(ui, "Mip levels", &tex.mip_levels.to_string());
    row(ui, "Format", &format!("{:?}", tex.format));
    row(ui, "sRGB", if tex.is_srgb() { "yes" } else { "no" });
    row(ui, "Memory", &or_job::human_bytes(tex.pixels.len() as u64));
    if !internals {
        return;
    }
    row_with_note(
        ui,
        "Tile mode",
        &format!("{:?}", tex.tile_mode),
        Some(
            "How the payload is swizzled in graphics memory. Anything but \
             Linear or Auto has no untiling pass, so the preview reports it \
             rather than decoding it.",
        ),
    );
    row_with_note(
        ui,
        "Usage",
        &format!("0x{:08X}", tex.usage_data),
        Some("What the Enhanced header declares this texture is used for."),
    );
    let stride = tex.block_stride();
    let blocks = (tex.pixels.len() as u64).checked_div(u64::from(stride));
    row(
        ui,
        "Blocks",
        &blocks.map_or("—".to_string(), |b| b.to_string()),
    );
    row(ui, "Block stride", &stride.to_string());
}

/// An Enhanced material: what its descriptor states, and the one thing here
/// that can be edited.
///
/// ⚠ **Its parameters are named through the *Legacy* shader definition
/// database.** Enhanced renamed nothing — a
/// Legacy file carries the effect variable's declared name and an Enhanced
/// one carries that variable's semantic, both out of the same `.fxc` — so
/// the database answers for both and this grid shows the Enhanced spelling
/// with the Legacy one beside it where they differ. See
/// `or_io::gta5::shaders::shader_defs::remap`.
///
/// ⚠ **Only constants are editable, and the two refusals are different.** A
/// texture binding is an address into the resource rather than a name on the
/// material, so re-pointing one is not an edit this surface can make. A
/// sampler's state byte has no measured meaning. Both say so rather than
/// offering a control that does nothing.
pub(super) fn draw_enhanced_material(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) -> bool {
    let Some(doc) = state.document(doc_id) else {
        return false;
    };
    if !doc.payload.is_enhanced() {
        return false;
    }
    let Some(sel) = selection.filter(|&s| s != DOC_ROOT_SEL) else {
        return false;
    };

    let (owner, shader_index) = match &doc.payload {
        DocPayload::Resource(or_io::Asset::DrawableGen9(_)) if section_of(sel) == SECT_SHADER => {
            (0u32, payload_index(sel))
        }
        DocPayload::Resource(or_io::Asset::DrawableDictGen9(_)) => {
            let Some(entry) = ydd_selected_entry_index(&doc.payload, sel) else {
                return false;
            };
            let (_, kind, sub) = dict_entry_unpack_part(sel);
            if kind != YPT_MODEL_KIND_SHADER {
                return false;
            }
            (entry as u32, sub)
        }
        _ => return false,
    };

    let Some(shader) = crate::state::enhanced_drawables_of(&doc.payload)
        .into_iter()
        .find(|(i, _)| *i as u32 == owner)
        .and_then(|(_, d)| d.shader_group.as_deref())
        .and_then(|g| g.shaders.get(shader_index as usize))
        .cloned()
    else {
        return false;
    };

    let mut edits: Vec<(u32, Vec<f32>)> = Vec::new();
    draw_enhanced_material_header(ui, &shader, shows_internals(state));
    draw_enhanced_material_params(ui, &shader, &mut edits);

    if !edits.is_empty() {
        apply_enhanced_edits(state, doc_id, owner, shader_index as usize, edits);
    }
    true
}

fn draw_enhanced_material_header(ui: &mut egui::Ui, shader: &GrcInstanceDataG9, internals: bool) {
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Material (Enhanced)")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    let effect = shader.name.get();
    let described = shader_defs::describes_shader(effect);
    grid(ui, "gen9_material", |ui| {
        row(ui, "Shader", &shader_name(effect));
        row(ui, "Effect hash", &format!("0x{effect:08X}"));
        row(ui, "Preset hash", &format!("0x{:08X}", shader.preset.get()));
        row(ui, "Draw bucket", &shader.draw_bucket.to_string());
        row(
            ui,
            "Parameters",
            &format!(
                "{} ({} texture, {} sampler, {} constant buffer)",
                shader.param_infos.params.len(),
                shader.params.textures.len(),
                shader.params.samplers.len(),
                shader.params.buffers.len()
            ),
        );
        if internals {
            row_with_note(
                ui,
                "Copies",
                &shader.param_infos.buffer_copies.to_string(),
                Some(
                    "The parameter block is held once per worker thread, and a save \
                     writes an edit to every copy.",
                ),
            );
        }
    });

    if !described {
        ui.label(
            egui::RichText::new(
                "No shader definition database describes this shader, so its parameters \
                 are hashes. Build one from a Legacy install with \
                 `or-shader-tool defs build` — Enhanced states the same parameters under \
                 the same effects' semantics.",
            )
            .small()
            .color(theme::WARNING),
        );
    }
}

/// The Enhanced spelling, with the Legacy one beside it where they differ.
fn labels(def: Option<&ShaderParamDef>, hash: u32) -> (String, Option<String>) {
    match def {
        None => (format!("0x{hash:08X}"), None),
        Some(def) => {
            let here = Spelling::of(Generation::Gen9).name_of(def).to_string();
            let there = Spelling::of(Generation::Gen8).name_of(def).to_string();
            let second = (there != here).then_some(there);
            (here, second)
        }
    }
}

fn draw_enhanced_material_params(
    ui: &mut egui::Ui,
    shader: &GrcInstanceDataG9,
    edits: &mut Vec<(u32, Vec<f32>)>,
) {
    let declarations = shader_defs::parameters_of(shader.name.get()).unwrap_or_default();
    ui.add_space(crate::ui::theme::SPACING_SNUG);

    for info in &shader.param_infos.params {
        let name = info.name.get();
        let def = declarations
            .iter()
            .find(|d| d.name_hash == name || d.semantic_hash == Some(name));
        let (label, legacy) = labels(def, name);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&label).strong());
            if let Some(legacy) = &legacy {
                ui.label(
                    egui::RichText::new(format!("Legacy {legacy}"))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
            if let Some(def) = def {
                ui.label(
                    egui::RichText::new(def.kind.as_str())
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
        });

        if let Some(slot) = info.texture() {
            let bound = match shader.params.textures.get(slot) {
                Some(TextureSlot::Embedded(k)) => {
                    format!("embedded 0x{:08X}", k.key.unwrap_or_default())
                }
                Some(TextureSlot::Reference(r)) => format!("references {}", r.name),
                Some(TextureSlot::Unbound) | None => "unbound".to_string(),
            };
            ui.label(
                egui::RichText::new(format!("texture slot {slot} · {bound}"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            )
            .on_hover_text("Re-pointing a texture binding is not an edit this panel can make.");
        } else if let Some(slot) = info.sampler() {
            ui.label(
                egui::RichText::new(format!("sampler {slot}"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        } else if let Some((buffer, range)) = info.constant() {
            let at = range.start;
            let Some(bytes) = shader.params.buffers.get(buffer).and_then(|b| b.get(range)) else {
                ui.label(
                    egui::RichText::new("no value read for this parameter")
                        .small()
                        .color(theme::WARNING),
                );
                continue;
            };
            let mut edited: Vec<f32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                for value in edited.iter_mut() {
                    changed |= drag_value(ui, value, |d| d.speed(0.01).max_decimals(4)).inner;
                }
                ui.label(
                    egui::RichText::new(format!("buffer {buffer} +{at:#x}"))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            });
            if changed {
                edits.push((name, edited));
            }
        } else {
            ui.label(
                egui::RichText::new("parameter type 1 — purpose not established")
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
    }
}

fn apply_enhanced_edits(
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    owner: u32,
    shader_index: usize,
    edits: Vec<(u32, Vec<f32>)>,
) {
    let mut changed = false;
    let mut failure: Option<String> = None;
    let before = {
        let Some(doc) = state.document_mut(doc_id) else {
            return;
        };
        let Some(shader) = enhanced_drawable_mut(&mut doc.payload, owner)
            .and_then(|d| d.shader_group.as_deref_mut())
            .and_then(|g| g.shaders.get_mut(shader_index))
        else {
            return;
        };
        let before = shader.clone();
        for (name_hash, floats) in edits {
            match set_floats(shader, name_hash, floats) {
                Ok(()) => changed = true,
                Err(e) => failure = Some(e),
            }
        }
        before
    };
    if let Some(message) = failure {
        state.show_warning_toast(message);
    }
    if changed {
        let key = format!("material {owner} {shader_index}");
        let part = Change::part(key, before, move |p| {
            let group = enhanced_drawable_mut(p, owner)?
                .shader_group
                .as_deref_mut()?;
            group.shaders.get_mut(shader_index)
        });
        state.record(doc_id, "Set a material value", part);
    }
}

/// Replaces a constant's floats at the width its descriptor states.
///
/// ⚠ The width is the descriptor's and not the caller's: a shorter run would
/// leave the tail of the region as it was, and a longer one would write into
/// the next parameter's.
fn set_floats(
    shader: &mut GrcInstanceDataG9,
    name_hash: u32,
    floats: Vec<f32>,
) -> Result<(), String> {
    let Some(bytes) = shader.constant_mut(name_hash) else {
        return Err(format!(
            "this material carries no constant-buffer parameter 0x{name_hash:08X}"
        ));
    };
    let wanted = bytes.len() / 4;
    if floats.len() != wanted {
        return Err(format!(
            "0x{name_hash:08X} occupies {wanted} float(s) in its constant buffer and {} were given",
            floats.len()
        ));
    }
    for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(floats) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::decl::{File as _, Gen9};
    use or_io::gta5::GtaDrawable;

    const WHAT: &str = "an_enhanced_material_edit_undoes_to_the_drawable_as_it_opened";

    /// A staged Enhanced drawable, its first material holding a constant, and the constant.
    fn a_constant() -> Option<(std::path::PathBuf, GtaDrawable<Gen9>, usize, u32, usize)> {
        let corpus = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr");
        for path in corpus.target(or_test_support::TARGET_GEN9).find() {
            let Ok(drawable) = GtaDrawable::<Gen9>::open(&path) else {
                continue;
            };
            let shaders = drawable.shader_group.as_deref().map(|g| &g.shaders[..]);
            let found = shaders
                .unwrap_or_default()
                .iter()
                .enumerate()
                .find_map(|(i, s)| {
                    let p = (s.param_infos.params.iter())
                        .find(|p| p.constant().is_some_and(|(_, r)| r.len() >= 4))?;
                    let floats = p.constant()?.1.len() / 4;
                    Some((i, p.name.get(), floats))
                });
            if let Some((shader, hash, floats)) = found {
                return Some((path, drawable, shader, hash, floats));
            }
        }
        None
    }

    #[test]
    fn an_enhanced_material_edit_undoes_to_the_drawable_as_it_opened() {
        let Some((path, drawable, shader, hash, floats)) = a_constant() else {
            or_test_support::skip_corpus(WHAT, "no staged Enhanced .ydr holds a material constant");
            return;
        };
        let mut state = AppState::new();
        state.documents.clear();
        let payload = DocPayload::Resource(or_io::Asset::DrawableGen9(drawable));
        state
            .documents
            .push(crate::state::Document::new(1, Some(path), payload));
        state.active_document = Some(1);
        let shown = |state: &AppState| match &state.document(1).expect("open").payload {
            DocPayload::Resource(or_io::Asset::DrawableGen9(d)) => format!("{:?}", d.shader_group),
            _ => panic!("not an Enhanced drawable"),
        };
        let opened = shown(&state);

        apply_enhanced_edits(&mut state, 1, 0, shader, vec![(hash, vec![1.5; floats])]);
        let edited = shown(&state);
        assert_ne!(edited, opened, "the edit changed nothing");

        assert_eq!(state.undo().as_deref(), Some("Set a material value"));
        assert_eq!(shown(&state), opened, "undone, it is not as it opened");
        assert!(!state.document(1).expect("open").dirty);
        state.redo();
        assert_eq!(shown(&state), edited, "redone, the edit is lost");
    }
}
