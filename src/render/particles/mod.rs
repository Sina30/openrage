mod texture_cache;

pub use texture_cache::ParticleTextureCache;

mod shader_vars;

pub(crate) mod gta6;

use shader_vars::warn_unknown_shader_vars;

use or_io::decl::Gen8;
use or_io::gta5::data::ptx_clip_regions::ClipRegionTable;
use or_io::gta5::particle::{PtxEffectRule, PtxParticleRule, ShaderVar};
use or_io::gta5::PtxFxList;
use or_particle::behaviour::Behaviour;
use or_renderer::particles::{
    ParticleAlignment, ParticleBatch, ParticleBlendMode, ParticleCullMode, ParticleInstance,
};
use or_simulation::PtfxManager;

/// The highest effect draw list the preview draws.
const MAX_DRAW_LIST: u8 = 2;

pub fn build_batches(
    cache: &mut ParticleTextureCache,
    asset: Option<&PtxFxList>,
    manager: &PtfxManager,
    clip_regions: &ClipRegionTable,
    retail_parity: bool,
) -> Vec<ParticleBatch> {
    let mut out = Vec::new();
    for (effect, emitter) in iter_effect_emitter_pairs(manager) {
        if effect.rule.draw_list_id > MAX_DRAW_LIST {
            continue;
        }
        if emitter.points.is_empty() {
            continue;
        }
        let rules = &emitter.rules;
        let rule = &rules.particle;
        let disable_draw = rules.behaviours.iter().any(|b| match b {
            Behaviour::DrawSprite(s) => s.disable_draw,
            _ => false,
        });
        if disable_draw {
            continue;
        }
        if !matches!(
            or_simulation::DrawKind::from_raw(rule.draw_type),
            or_simulation::DrawKind::Sprite
        ) {
            continue;
        }
        let blend = ParticleBlendMode::from_engine_byte(rule.render_state.blend_set as u8);
        let alignment = decode_alignment(&rules.behaviours);
        let scanned = gta6::drawn(cache, &rule.name);
        let diffuse = diffuse_texture_name(&rules.shader_vars)
            .or_else(|| scanned.as_ref().map(|d| d.texture.clone()));
        let sort_type = SortType::from_engine(rule.sort_type);
        let effect_tint = if rules
            .behaviours
            .iter()
            .any(|b| matches!(b, Behaviour::Colour(_)))
        {
            None
        } else {
            Some(effect_rule_colour_tint(&effect.rule))
        };
        let instances = build_instances(&emitter.points, blend, sort_type, effect_tint);
        warn_unknown_shader_vars(rules);
        let (atlas_rows, atlas_cols) = match &scanned {
            Some(d) => (d.rows, d.cols),
            None => resolve_atlas_dims(rule, diffuse.as_deref(), clip_regions),
        };
        let mut batch =
            ParticleBatch::new(blend, alignment, instances).with_uv_atlas(atlas_rows, atlas_cols);
        // The game's own passes draw it with the technique the rule names and the texture decoded once
        batch.shader_technique = rule.shader_inst.shader_template_technique_name.clone();
        batch.diffuse_image = match (asset, diffuse.as_deref()) {
            (Some(asset), Some(name)) => cache.get_or_decode_image(asset, name),
            _ => None,
        };
        batch.refract = rule.shader_inst.technique_desc.is_refract.get();
        batch.cull = ParticleCullMode::from_engine(rule.render_state.cull_mode as u32);
        batch.depth_write = rule.render_state.depth_write.get();
        batch.depth_test = rule.render_state.depth_test.get();
        batch.constants = rule_constants(&rules.shader_vars, effect.time_ratio());
        let mut authored = glam::Vec3::Z;
        for b in &rules.behaviours {
            if let Behaviour::DrawSprite(s) = b {
                authored = glam::Vec3::new(s.align_axis.x, s.align_axis.y, s.align_axis.z);
                break;
            }
        }
        let authored_norm = authored.normalize_or(glam::Vec3::Z);
        batch.align_axis = match alignment {
            ParticleAlignment::World => [authored_norm.x, authored_norm.y, authored_norm.z],
            ParticleAlignment::Effect => {
                let m = emitter.creation_domain.local_to_world;
                let world = m.transform_vector3(authored_norm).normalize_or_zero();
                if world.length_squared() > 1e-6 {
                    [world.x, world.y, world.z]
                } else {
                    [0.0, 0.0, 1.0]
                }
            }
            ParticleAlignment::Emitter => {
                let m = emitter.creation_domain.local_to_world;
                let world = m.transform_vector3(authored_norm).normalize_or_zero();
                if world.length_squared() > 1e-6 {
                    [world.x, world.y, world.z]
                } else {
                    [0.0, 0.0, 1.0]
                }
            }
            _ => [0.0, 0.0, 1.0],
        };
        out.push(batch);
    }
    if retail_parity {
        for batch in out.iter_mut() {
            batch.collapse_to_retail();
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortType {
    ClosestFirst,
    FarthestFirst,
    NewestFirst,
    OldestFirst,
    /// Per-particle random ordering (engine `g_DrawRand.GetRanged(0,100)`).
    Random,
}

impl SortType {
    pub fn from_engine(raw: u8) -> Self {
        match raw {
            0 => Self::ClosestFirst,
            1 => Self::FarthestFirst,
            2 => Self::NewestFirst,
            3 => Self::OldestFirst,
            4 => Self::Random,
            _ => Self::FarthestFirst,
        }
    }
}

fn build_instances(
    buf: &or_simulation::PointBuffer,
    blend: ParticleBlendMode,
    sort_type: SortType,
    effect_tint: Option<[f32; 4]>,
) -> Vec<ParticleInstance> {
    use or_simulation::point::PointFlags;
    let raw = buf.to_gpu_instances();
    let mut out: Vec<ParticleInstance> = raw
        .into_iter()
        .enumerate()
        .filter(|(i, _)| {
            !buf.flags
                .get(*i)
                .map(|f| f.contains(PointFlags::CLIPPED_BY_DISTANCE))
                .unwrap_or(false)
        })
        .map(|(i, inst)| {
            let color = match effect_tint {
                Some(t) => [
                    inst.color[0] * t[0],
                    inst.color[1] * t[1],
                    inst.color[2] * t[2],
                    inst.color[3] * t[3],
                ],
                None => inst.color,
            };
            let custom1 = match effect_tint {
                Some(_) => [inst.life_ratio, color[0], color[1], color[2]],
                None => inst.custom1,
            };
            let custom2 = match effect_tint {
                Some(_) => [color[3], 0.0, 0.0, 0.0],
                None => inst.custom2,
            };
            let mut p = ParticleInstance {
                position: inst.position,
                life_ratio: inst.life_ratio,
                dimensions: inst.dimensions,
                rotation: inst.rotation,
                color,
                velocity: inst.velocity,
                _pad0: inst._pad0,
                tex_frame_a: inst.tex_frame_a,
                tex_frame_b: inst.tex_frame_b,
                tex_blend: inst.tex_blend,
                _pad1: inst._pad1,
                uv_offset: inst.uv_offset,
                _pad2: inst._pad2,
                custom1,
                custom2,
            };
            // The sort key rides in `_pad0` and is zeroed before upload.
            // `sort_val` already holds camera distance² or the engine's
            // `g_DrawRand.GetRanged(0,100)`; NEWEST/OLDEST use the live
            // `life_ratio` as a stable age proxy.
            p._pad0 = match sort_type {
                SortType::ClosestFirst | SortType::FarthestFirst => {
                    buf.sort_val.get(i).copied().unwrap_or(0.0)
                }
                SortType::NewestFirst | SortType::OldestFirst => inst.life_ratio,
                SortType::Random => buf.sort_val.get(i).copied().unwrap_or(0.0),
            };
            p
        })
        .collect();
    let needs_sort = !blend.is_commutative();
    if needs_sort {
        match sort_type {
            SortType::FarthestFirst => out.sort_by(|a, b| {
                b._pad0
                    .partial_cmp(&a._pad0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortType::ClosestFirst => out.sort_by(|a, b| {
                a._pad0
                    .partial_cmp(&b._pad0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortType::NewestFirst => out.sort_by(|a, b| {
                b._pad0
                    .partial_cmp(&a._pad0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortType::OldestFirst => out.sort_by(|a, b| {
                a._pad0
                    .partial_cmp(&b._pad0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortType::Random => out.sort_by(|a, b| {
                a._pad0
                    .partial_cmp(&b._pad0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }
    }
    for p in out.iter_mut() {
        p._pad0 = 0.0;
    }
    out
}

/// Every emitter of the playing effects with the effect it belongs to.
pub(crate) fn iter_effect_emitter_pairs(
    manager: &or_simulation::PtfxManager,
) -> Vec<(&or_simulation::EffectInst, &or_simulation::EmitterInst)> {
    let mut out = Vec::new();
    for effect in manager.effect.iter().chain(&manager.spawned_effects) {
        for emitter in &effect.emitters {
            out.push((effect, emitter));
        }
    }
    out
}

/// The values the rule's vector and keyframe shader variables hold at progress `t` of its effect, by the hash of each one's name.
pub(crate) fn rule_constants(shader_vars: &[ShaderVar], t: f32) -> Vec<(u32, [f32; 4])> {
    shader_vars
        .iter()
        .filter_map(|var| match var {
            ShaderVar::Vector {
                name_hash, value, ..
            } => Some((*name_hash, *value)),
            ShaderVar::Keyframe {
                name_hash, entries, ..
            } => Some((
                *name_hash,
                or_simulation::kfp::query_raw_entries(entries, t).to_array(),
            )),
            ShaderVar::Texture { .. } => None,
        })
        .collect()
}

pub(crate) fn effect_rule_colour_tint(rule: &PtxEffectRule<Gen8>) -> [f32; 4] {
    if let Some(first) = rule.colour_tint_min_kfp.keyframe.entries.first() {
        let v = &first.value;
        [v.x, v.y, v.z, v.w.max(0.001)]
    } else {
        [1.0, 1.0, 1.0, 1.0]
    }
}

pub(crate) fn resolve_atlas_dims(
    rule: &PtxParticleRule<Gen8>,
    diffuse_texture_name: Option<&str>,
    clip_regions: &ClipRegionTable,
) -> (u32, u32) {
    if let Some(name) = diffuse_texture_name {
        let hash = or_particle::rage::hash::string_hash(name.as_bytes());
        if let Some(entry) = clip_regions.get_by_hash(hash) {
            trace_once_atlas_engine(&rule.name, name, entry.num_rows, entry.num_columns);
            return (entry.num_rows.max(1), entry.num_columns.max(1));
        }
        warn_once_atlas_inference(
            &rule.name,
            name,
            rule.tex_frame_id_max as u32,
            clip_regions.is_empty(),
        );
    }
    infer_uv_atlas(rule.tex_frame_id_max as u32)
}

fn trace_once_atlas_engine(rule_name: &str, texture: &str, rows: u32, cols: u32) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashSet<(String, String)>>> = OnceLock::new();
    let lock = SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    let mut seen = lock.lock().unwrap();
    if seen.insert((rule_name.to_string(), texture.to_string())) {
        tracing::debug!(
            rule = rule_name,
            texture,
            rows,
            cols,
            "atlas dims sourced from engine ptxclipregions.dat"
        );
    }
}

fn warn_once_atlas_inference(
    rule_name: &str,
    texture: &str,
    tex_frame_id_max: u32,
    table_empty: bool,
) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashSet<(String, String)>>> = OnceLock::new();
    let lock = SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    let mut seen = lock.lock().unwrap();
    if seen.insert((rule_name.to_string(), texture.to_string())) {
        let (rows, cols) = infer_uv_atlas(tex_frame_id_max);
        if table_empty {
            tracing::warn!(
                rule = rule_name,
                texture,
                tex_frame_id_max,
                inferred_rows = rows,
                inferred_cols = cols,
                "ptxclipregions.dat not loaded; using inferred atlas dims (set GTA install + keys to fix)"
            );
        } else {
            tracing::debug!(
                rule = rule_name,
                texture,
                tex_frame_id_max,
                inferred_rows = rows,
                inferred_cols = cols,
                "texture not in ptxclipregions.dat; using inferred atlas dims"
            );
        }
    }
}

pub(crate) fn infer_uv_atlas(tex_frame_id_max: u32) -> (u32, u32) {
    let total = tex_frame_id_max.saturating_add(1).max(1);
    if total <= 1 {
        return (1, 1);
    }
    let root = (total as f32).sqrt().round() as u32;
    if root >= 1 && root * root == total {
        (root, root)
    } else {
        (1, total)
    }
}

fn decode_alignment(behaviours: &[Behaviour]) -> ParticleAlignment {
    for b in behaviours {
        if let Behaviour::DrawSprite(s) = b {
            return ParticleAlignment::from_core(s.alignment_mode);
        }
    }
    ParticleAlignment::Camera
}

/// The distance over which a soft sprite fades into depth.
/// Which of the texture variables a rule sets the pass binds as its diffuse: the first named for the diffuse, or a second one named for another slot where the diffuse has none.
fn diffuse_texture_name(shader_vars: &[ShaderVar]) -> Option<String> {
    let mut taken = [false; TEXTURE_SLOTS];
    let mut diffuse = None;
    for var in shader_vars {
        let ShaderVar::Texture {
            texture_name,
            name_hash,
            ..
        } = var
        else {
            continue;
        };
        if texture_name.is_empty() {
            continue;
        }
        let slot = texture_slot(*name_hash);
        if !taken[slot] {
            taken[slot] = true;
            if slot == 0 {
                diffuse = Some(texture_name.clone());
            }
        } else if slot != 0 && diffuse.is_none() {
            diffuse = Some(texture_name.clone());
        }
    }
    diffuse
}

/// How many slots the game's particle shaders name their textures by: the diffuse, the normal map, the dirt and the projection.
const TEXTURE_SLOTS: usize = 4;

/// The slot a texture variable is for, by the hash of its name; a name no slot knows is the diffuse.
fn texture_slot(name_hash: u32) -> usize {
    match name_hash {
        0x0df47048 | 0xc8e8c282 => 0,
        0xdf7cc018 | 0xdfb42a92 | 0x3eb8a8a1 | 0x1e77bf4c => 1,
        0xa6f4cf58 | 0x239e7dab | 0x0fa200e2 | 0xc6b753db => 2,
        0x1aee5eac | 0xf730f78b | 0x89c523ec | 0x0e3738bb => 3,
        _ => 0,
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn texture_var(name_hash: u32, name: &str) -> ShaderVar {
        ShaderVar::Texture {
            name_hash,
            id: 0,
            is_keyframeable: false,
            owns_info: false,
            texture_name: name.to_string(),
            texture_hash_name: 0,
            external_reference: false,
        }
    }

    #[test]
    fn the_diffuse_is_the_texture_named_for_the_diffuse() {
        let vars = vec![
            // 0xdfb42a92 = atHashValue("NormalMap") → slot 1
            texture_var(0xdfb42a92, "blood_normal"),
            // 0x0df47048 = atHashValue("DiffuseTex2") → slot 0
            texture_var(0x0df47048, "blood_diffuse"),
            // 0xa6f4cf58 = atHashValue("DirtTex") → slot 2
            texture_var(0xa6f4cf58, "blood_dirt"),
        ];
        assert_eq!(
            diffuse_texture_name(&vars).as_deref(),
            Some("blood_diffuse")
        );
    }

    #[test]
    fn a_name_no_slot_knows_is_the_diffuse() {
        let vars = vec![texture_var(0xDEADBEEF, "weird_name")];
        assert_eq!(texture_slot(0xDEADBEEF), 0);
        assert_eq!(diffuse_texture_name(&vars).as_deref(), Some("weird_name"));
    }

    #[test]
    fn a_rule_with_only_a_normal_map_has_no_diffuse_and_an_empty_name_is_skipped() {
        let vars = vec![
            texture_var(0x0df47048, ""),
            texture_var(0xdfb42a92, "real_normal"),
        ];
        assert_eq!(diffuse_texture_name(&vars), None);
    }

    #[test]
    fn a_second_texture_for_another_slot_stands_in_where_the_diffuse_has_none() {
        let vars = vec![
            texture_var(0xdfb42a92, "first_normal"),
            texture_var(0xdfb42a92, "second_normal"),
        ];
        assert_eq!(
            diffuse_texture_name(&vars).as_deref(),
            Some("second_normal")
        );
    }

    #[test]
    fn clip_regions_table_drives_atlas_when_present() {
        let fixture = "1\n8\nblood_drop 4 2\n\
            0.00 0.25 0.0 0.5  0.25 0.50 0.0 0.5  0.50 0.75 0.0 0.5  0.75 1.0 0.0 0.5\n\
            0.00 0.25 0.5 1.0  0.25 0.50 0.5 1.0  0.50 0.75 0.5 1.0  0.75 1.0 0.5 1.0\n";
        let table = or_io::gta5::data::ptx_clip_regions::ClipRegionTable::parse(fixture).unwrap();

        let mut rule = PtxParticleRule::<Gen8>::default();
        rule.tex_frame_id_max = 7;

        let (rows, cols) = resolve_atlas_dims(&rule, Some("blood_drop"), &table);
        assert_eq!((rows, cols), (2, 4));

        let (rows, cols) = resolve_atlas_dims(&rule, Some("missing_atlas"), &table);
        assert_eq!((rows, cols), (1, 8));

        let empty = or_io::gta5::data::ptx_clip_regions::ClipRegionTable::empty();
        let (rows, cols) = resolve_atlas_dims(&rule, Some("blood_drop"), &empty);
        assert_eq!((rows, cols), (1, 8));

        let (rows, cols) = resolve_atlas_dims(&rule, None, &table);
        assert_eq!((rows, cols), (1, 8));
    }

    #[test]
    fn clip_regions_lookup_is_case_insensitive() {
        let fixture = "1\n1\nptxblood 1 1\n0 1 0 1\n";
        let table = or_io::gta5::data::ptx_clip_regions::ClipRegionTable::parse(fixture).unwrap();
        let mut rule = PtxParticleRule::<Gen8>::default();
        rule.tex_frame_id_max = 0;
        let (rows, cols) = resolve_atlas_dims(&rule, Some("PTXBLOOD"), &table);
        assert_eq!((rows, cols), (1, 1));
    }

    #[test]
    fn infer_uv_atlas_picks_square_when_possible() {
        assert_eq!(infer_uv_atlas(0), (1, 1));
        assert_eq!(infer_uv_atlas(3), (2, 2));
        assert_eq!(infer_uv_atlas(8), (3, 3));
        assert_eq!(infer_uv_atlas(15), (4, 4));
        assert_eq!(infer_uv_atlas(63), (8, 8));
        assert_eq!(infer_uv_atlas(4), (1, 5));
        assert_eq!(infer_uv_atlas(6), (1, 7));
    }

    #[test]
    fn a_rules_vectors_and_keyframes_are_constants_by_the_hash_of_their_names_and_textures_are_not()
    {
        use or_io::gta5::particle::RawKeyframeEntry;
        let vars = vec![
            ShaderVar::Vector {
                name_hash: 1,
                var_type: 0,
                id: 0,
                is_keyframeable: false,
                owns_info: false,
                value: [2.5, 0.0, 0.0, 0.0],
            },
            ShaderVar::Keyframe {
                name_hash: 2,
                id: 1,
                is_keyframeable: true,
                owns_info: false,
                entries: vec![
                    RawKeyframeEntry {
                        time: [0.0; 4],
                        value: [0.0, 10.0, 0.0, 1.0],
                    },
                    RawKeyframeEntry {
                        time: [1.0; 4],
                        value: [1.0, 20.0, 0.0, 1.0],
                    },
                ],
            },
            texture_var(3, "diffuse"),
        ];
        let at_half = rule_constants(&vars, 0.5);
        assert_eq!(at_half.len(), 2, "a texture is not a constant: {at_half:?}");
        assert_eq!(at_half[0], (1, [2.5, 0.0, 0.0, 0.0]));
        assert_eq!(at_half[1].0, 2);
        assert!((at_half[1].1[0] - 0.5).abs() < 1e-5, "{:?}", at_half[1].1);
        assert!((at_half[1].1[1] - 15.0).abs() < 1e-4, "{:?}", at_half[1].1);
        // The keyframe runs over the effect: its start and its end
        assert_eq!(rule_constants(&vars, 0.0)[1].1[1], 10.0);
        assert_eq!(rule_constants(&vars, 1.0)[1].1[1], 20.0);
    }

    #[test]
    fn sort_type_engine_decode() {
        assert_eq!(SortType::from_engine(0), SortType::ClosestFirst);
        assert_eq!(SortType::from_engine(1), SortType::FarthestFirst);
        assert_eq!(SortType::from_engine(2), SortType::NewestFirst);
        assert_eq!(SortType::from_engine(3), SortType::OldestFirst);
        assert_eq!(SortType::from_engine(4), SortType::Random);
        assert_eq!(SortType::from_engine(99), SortType::FarthestFirst);
    }
}
