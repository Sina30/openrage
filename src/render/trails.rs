use glam::{Vec3, Vec4};
use or_io::gta5::particle::{PtxParticleRule, ShaderVar};
use or_io::gta5::PtxFxList;
use or_particle::behaviour::Behaviour;
use or_renderer::trails::{
    trail_tessellation_enabled, TrailBatch, TrailVertex, MAX_TRAIL_TESSELLATION,
};
use or_renderer::ParticleBlendMode;
use or_simulation::PtfxManager;

fn camera_radial_axis(node_pos: Vec3, camera_pos: Vec3, fallback: Vec3) -> Vec3 {
    let v = node_pos - camera_pos;
    let len_sq = v.length_squared();
    if len_sq > 1e-8 {
        v / len_sq.sqrt()
    } else {
        fallback
    }
}

use crate::render::particles::ParticleTextureCache;

#[allow(clippy::too_many_arguments)]
pub fn build_trail_batches(
    cache: &mut ParticleTextureCache,
    asset: Option<&PtxFxList>,
    manager: &PtfxManager,
    camera_dir: Vec3,
    camera_pos: Vec3,
    retail_parity: bool,
) -> Vec<TrailBatch> {
    let mut out = Vec::new();
    for (effect, emitter) in crate::render::particles::iter_effect_emitter_pairs(manager) {
        if emitter.draw_kind != or_simulation::DrawKind::Trail {
            continue;
        }
        if emitter.points.is_empty() {
            continue;
        }
        let rules = &emitter.rules;
        let rule = &rules.particle;
        let blend = if retail_parity {
            decode_blend_mode(rule).retail_equivalent()
        } else {
            decode_blend_mode(rule)
        };

        let trail_data = find_draw_trail(&rules.behaviours);
        if let Some(d) = trail_data {
            if d.disable_draw {
                continue;
            }
        }
        let tess_u = trail_data
            .map(|d| tessellation_u_steps(d.tessellation_u))
            .unwrap_or(1);
        let tess_v_raw = trail_data
            .map(|d| tessellation_v_rows(d.tessellation_v))
            .unwrap_or(0);
        let tess_v = if trail_tessellation_enabled(tess_v_raw) {
            tess_v_raw as usize
        } else {
            0
        };
        let (wrap_length, tex_offset) = if let Some(d) = trail_data {
            tex_info_at(d, 0.0, or_particle::list::code(&rules.rule.name))
        } else {
            (1.0, 0.0)
        };
        use or_io::gta5::particle::AlignmentMode;
        let trail_align_axis = trail_data.and_then(|d| match d.alignment_mode {
            AlignmentMode::Camera | AlignmentMode::Velocity => None,
            AlignmentMode::World => Some(
                glam::Vec3::new(d.align_axis.x, d.align_axis.y, d.align_axis.z)
                    .normalize_or(Vec3::Z),
            ),
            AlignmentMode::Effect | AlignmentMode::Emitter => {
                let m = emitter.creation_domain.local_to_world;
                let v = glam::Vec3::new(d.align_axis.x, d.align_axis.y, d.align_axis.z)
                    .normalize_or(Vec3::Z);
                Some(m.transform_vector3(v).normalize_or(Vec3::Z))
            }
        });
        let flip_u = trail_data.map(|d| d.flip_u).unwrap_or(false);
        let wrap_over_life = trail_data
            .map(|d| d.wrap_texture_over_particle_life)
            .unwrap_or(false);

        let anchor_pos = emitter.creation_domain.world_position();
        let head_idx = emitter.points.first_in_chain().unwrap_or(0);
        let anchor_color = emitter.points.color[head_idx];
        let anchor_dim = emitter.points.dimensions[head_idx];

        let strip_v_rows = tess_v.max(1);
        let vertices = build_strip_vertices(
            &emitter.points,
            camera_dir,
            camera_pos,
            trail_align_axis,
            flip_u,
            wrap_over_life,
            anchor_pos,
            anchor_color,
            anchor_dim,
            tess_u,
            strip_v_rows,
            wrap_length,
            tex_offset,
        );
        if vertices.len() < 4 {
            continue;
        }
        let texture_name = first_texture_name(&rules.shader_vars);
        let diffuse_image = match (asset, texture_name.as_deref()) {
            (Some(asset), Some(name)) => cache.get_or_decode_image(asset, name),
            _ => None,
        };
        out.push(TrailBatch {
            blend,
            vertices,
            constants: crate::render::particles::rule_constants(
                &rules.shader_vars,
                effect.time_ratio(),
            ),
            shader_technique: rule.shader_inst.shader_template_technique_name.clone(),
            diffuse_image,
        });
    }
    out
}

fn find_draw_trail(
    behaviours: &[Behaviour],
) -> Option<&or_particle::behaviour::DrawTrailBehaviour> {
    for b in behaviours {
        if let Behaviour::DrawTrail(d) = b {
            return Some(d);
        }
    }
    None
}

fn tex_info_at(
    d: &or_particle::behaviour::DrawTrailBehaviour,
    time: f32,
    rand_seed: u32,
) -> (f32, f32) {
    let v = match d.tex_info_keyframes.as_ref() {
        Some(k) if !k.entries.is_empty() => or_simulation::kfp::query_raw_entries(&k.entries, time),
        _ => Vec4::new(1.0, 1.0, 0.0, 0.0),
    };
    let bias = {
        let n = rand_seed
            .wrapping_mul(2654435761)
            .wrapping_add(0xC9_E5_22_99);
        (n as f32) / (u32::MAX as f32)
    };
    let wrap = (v.x + (v.y - v.x) * bias).max(0.001);
    let offset = v.z + (v.w - v.z) * bias;
    (wrap, offset)
}

#[allow(clippy::too_many_arguments)]
fn build_strip_vertices(
    buf: &or_simulation::PointBuffer,
    camera_dir: Vec3,
    camera_pos: Vec3,
    align_axis: Option<Vec3>,
    flip_u: bool,
    wrap_over_life: bool,
    anchor_pos: Vec3,
    anchor_color: Vec4,
    anchor_dim: Vec4,
    tess_u: usize,
    tess_v_rows: usize,
    wrap_length: f32,
    tex_offset: f32,
) -> Vec<TrailVertex> {
    let mut nodes: Vec<TrailNode> = Vec::with_capacity(buf.len() + 1);
    nodes.push(TrailNode {
        pos: anchor_pos,
        color: anchor_color,
        half_width: (anchor_dim.x.abs() + anchor_dim.y.abs()).max(0.05) * 0.5,
        is_broken: false,
        life_ratio: 0.0,
    });
    let mut current = buf.first_in_chain();
    while let Some(i) = current {
        nodes.push(TrailNode {
            pos: buf.curr_pos[i],
            color: buf.color[i] * Vec4::new(1.0, 1.0, 1.0, buf.alpha_mult[i]),
            half_width: (buf.dimensions[i].x.abs() + buf.dimensions[i].y.abs()).max(0.05) * 0.5,
            is_broken: buf.is_broken_trail[i],
            life_ratio: buf.life_ratio[i].clamp(0.0, 1.0),
        });
        current = buf.next_in_chain[i];
    }

    let tessellated = if tess_u <= 1 || nodes.len() < 2 {
        nodes
    } else {
        let mut out = Vec::with_capacity(nodes.len() * tess_u);
        let nlen = nodes.len();
        for i in 0..nlen {
            let n = nodes[i];
            out.push(n);
            if i + 1 < nlen && !nodes[i + 1].is_broken {
                let p1 = nodes[i].pos;
                let p2 = nodes[i + 1].pos;
                let p0 = if i > 0 && !nodes[i].is_broken {
                    nodes[i - 1].pos
                } else {
                    p1 + (p1 - p2)
                };
                let p3 = if i + 2 < nlen && !nodes[i + 2].is_broken {
                    nodes[i + 2].pos
                } else {
                    p2 + (p2 - p1)
                };
                let next = nodes[i + 1];
                for sub in 1..tess_u {
                    let t = sub as f32 / tess_u as f32;
                    let pos = catmull_rom(p0, p1, p2, p3, t);
                    out.push(TrailNode {
                        pos,
                        color: n.color.lerp(next.color, t),
                        half_width: n.half_width * (1.0 - t) + next.half_width * t,
                        is_broken: false,
                        life_ratio: n.life_ratio * (1.0 - t) + next.life_ratio * t,
                    });
                }
            }
        }
        out
    };

    #[derive(Clone, Copy)]
    struct NodeFrame {
        pos: Vec3,
        perp: Vec3,
        half_width: f32,
        u: f32,
        color: [f32; 4],
        is_broken: bool,
    }
    let mut frames: Vec<NodeFrame> = Vec::with_capacity(tessellated.len());
    let mut cumulative: f32 = 0.0;
    for i in 0..tessellated.len() {
        let n = tessellated[i];
        if n.is_broken {
            cumulative = 0.0;
        }
        if i > 0 {
            cumulative += (n.pos - tessellated[i - 1].pos).length();
        }
        let tangent = match (i.checked_sub(1), tessellated.get(i + 1)) {
            (Some(p), Some(next)) => (next.pos - tessellated[p].pos).normalize_or_zero(),
            (Some(p), None) => (n.pos - tessellated[p].pos).normalize_or_zero(),
            (None, Some(next)) => (next.pos - n.pos).normalize_or_zero(),
            (None, None) => Vec3::Z,
        };
        let basis = match align_axis {
            Some(axis) => axis,
            None => camera_radial_axis(n.pos, camera_pos, camera_dir),
        };
        let perp = tangent.cross(basis).normalize_or_zero();
        let perp = if perp == Vec3::ZERO { Vec3::X } else { perp };
        let mut u = if wrap_over_life {
            n.life_ratio * wrap_length + tex_offset
        } else {
            cumulative / wrap_length + tex_offset
        };
        if flip_u {
            u = -u;
        }
        frames.push(NodeFrame {
            pos: n.pos,
            perp,
            half_width: n.half_width,
            u,
            color: [n.color.x, n.color.y, n.color.z, n.color.w],
            is_broken: n.is_broken,
        });
    }

    let row_pairs = if tess_v_rows >= 2 {
        let n = tess_v_rows - 1;
        (0..n)
            .map(|r| {
                let v_lo = r as f32 / n as f32;
                let v_hi = (r + 1) as f32 / n as f32;
                let off_lo = 2.0 * v_lo - 1.0;
                let off_hi = 2.0 * v_hi - 1.0;
                (v_lo, v_hi, off_lo, off_hi)
            })
            .collect::<Vec<_>>()
    } else {
        vec![(0.0, 1.0, -1.0, 1.0)]
    };

    let mut out: Vec<TrailVertex> = Vec::with_capacity(frames.len() * row_pairs.len() * 2 + 16);
    for (pair_idx, (v_lo, v_hi, off_lo, off_hi)) in row_pairs.iter().copied().enumerate() {
        let mut last_hi: Option<TrailVertex> = None;
        let mut strip_started = false;
        if pair_idx > 0 {
            if let Some(first_frame) = frames.first() {
                let half = first_frame.perp * first_frame.half_width * off_lo;
                let bridge = TrailVertex {
                    position: (first_frame.pos + half).into(),
                    color: first_frame.color,
                    uv: [first_frame.u, v_lo],
                };
                if let Some(prev_last) = out.last().copied() {
                    out.push(prev_last);
                    out.push(bridge);
                }
            }
        }
        for f in &frames {
            let half_lo = f.perp * f.half_width * off_lo;
            let half_hi = f.perp * f.half_width * off_hi;
            let lo = TrailVertex {
                position: (f.pos + half_lo).into(),
                color: f.color,
                uv: [f.u, v_lo],
            };
            let hi = TrailVertex {
                position: (f.pos + half_hi).into(),
                color: f.color,
                uv: [f.u, v_hi],
            };
            if f.is_broken && strip_started {
                if let Some(prev_hi) = last_hi {
                    out.push(prev_hi);
                    out.push(lo);
                    out.push(lo);
                    out.push(lo);
                }
            }
            out.push(lo);
            out.push(hi);
            last_hi = Some(hi);
            strip_started = true;
        }
    }
    out
}

#[derive(Clone, Copy)]
struct TrailNode {
    pos: Vec3,
    color: Vec4,
    half_width: f32,
    is_broken: bool,
    life_ratio: f32,
}

fn tessellation_u_steps(raw: u32) -> usize {
    (raw.min(MAX_TRAIL_TESSELLATION) as usize)
        .saturating_mul(4)
        .saturating_add(1)
}

fn tessellation_v_rows(raw: u32) -> u32 {
    raw.min(MAX_TRAIL_TESSELLATION)
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

fn decode_blend_mode(rule: &PtxParticleRule<or_io::decl::Gen8>) -> ParticleBlendMode {
    ParticleBlendMode::from_engine_byte(rule.render_state.blend_set as u8)
}

fn first_texture_name(shader_vars: &[ShaderVar]) -> Option<String> {
    for var in shader_vars {
        if let ShaderVar::Texture { texture_name, .. } = var {
            if !texture_name.is_empty() {
                return Some(texture_name.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_renderer::trails::MAX_TRAIL_CONTROL_POINTS;

    /// ⚠ The distinct values across 85 `DrawTrail` behaviours in the staged
    /// `.ypt` corpus (see [`MAX_TRAIL_TESSELLATION`]) must pass through
    /// untouched, or the clamp silently re-tessellates the game's own particles.
    #[test]
    fn the_tessellation_values_real_particles_use_are_untouched() {
        for raw in [1u32, 2, 3, 4] {
            assert_eq!(
                tessellation_u_steps(raw),
                raw as usize * 4 + 1,
                "tessellation_u {raw} is a value core.ypt ships and must not be clamped"
            );
        }
        for raw in [1u32, 2, 3, 4, 6] {
            assert_eq!(
                tessellation_v_rows(raw),
                raw,
                "tessellation_v {raw} is a value core.ypt ships and must not be clamped"
            );
        }
    }

    #[test]
    fn a_hostile_tessellation_cannot_decide_the_allocation() {
        let worst_u = tessellation_u_steps(u32::MAX);
        let worst_v = tessellation_v_rows(u32::MAX);

        let worst_nodes = MAX_TRAIL_CONTROL_POINTS.saturating_add(1);
        let worst_vertices = worst_nodes
            .saturating_mul(worst_u)
            .saturating_mul(worst_v.max(1) as usize);

        println!(
            "u32::MAX tessellation -> {worst_u} steps, {worst_v} rows, \
             {worst_vertices} vertices per chain"
        );

        assert!(
            worst_u <= MAX_TRAIL_TESSELLATION as usize * 4 + 1,
            "tessellation_u is unbounded: {worst_u} steps"
        );
        assert!(
            worst_v <= MAX_TRAIL_TESSELLATION,
            "tessellation_v is unbounded: {worst_v} rows"
        );
        assert!(
            worst_vertices < 2_000_000,
            "a single trail chain can reserve {worst_vertices} vertices"
        );
    }
}
