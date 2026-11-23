use glam::{Quat, Vec3, Vec4};
use or_particle::behaviour::{Behaviour, LightBehaviour};
use or_renderer::LineVertex;
use or_simulation::PtfxManager;

const MAX_GIZMOS_PER_EMITTER: usize = 64;

pub fn append_gizmo_lines(out: &mut Vec<LineVertex>, manager: &PtfxManager) {
    for pool in &manager.liquid_batch().pools {
        let colour = [
            pool.color.x,
            pool.color.y,
            pool.color.z,
            pool.color.w.max(0.4),
        ];
        let radius = pool.radius.max(0.05);
        push_wire_disc_oriented(out, pool.pos, pool.normal, radius, colour);
    }
    for decal in &manager.decal_pool_batch().decals {
        let colour = [
            decal.color.x,
            decal.color.y,
            decal.color.z,
            decal.color.w.max(0.4),
        ];
        let radius = decal.radius.max(0.05);
        push_wire_disc_oriented(out, decal.pos, decal.normal, radius, colour);
    }
    for emitter in manager.emitters() {
        let kind = detect_gizmo_kind(&emitter.rules.behaviours);
        let Some(kind) = kind else {
            continue;
        };
        let n = emitter.points.len();
        let stride = n.div_ceil(MAX_GIZMOS_PER_EMITTER).max(1);
        for i in (0..n).step_by(stride) {
            let pos = emitter.points.curr_pos[i];
            let dim = emitter.points.dimensions[i];
            let rgba =
                emitter.points.color[i] * Vec4::new(1.0, 1.0, 1.0, emitter.points.alpha_mult[i]);
            let colour = [rgba.x, rgba.y, rgba.z, rgba.w.max(0.4)];
            let rot = emitter.points.curr_rot[i];
            match kind {
                GizmoKind::LightSphere => {
                    let radius = dim.x.max(0.05);
                    push_wire_sphere(out, pos, radius, colour);
                }
                GizmoKind::LightSpotCone(intensity) => {
                    let radius = dim.x.max(0.05);
                    push_wire_cone(out, pos, rot, radius, intensity.max(0.5), colour);
                }
                GizmoKind::FogBox => {
                    push_wire_box(out, pos, Vec3::new(dim.x, dim.y, dim.z).abs(), colour);
                }
                GizmoKind::DecalQuad => {
                    push_wire_quad_oriented(
                        out,
                        pos,
                        rot,
                        dim.x.max(0.05),
                        dim.y.max(0.05),
                        colour,
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum GizmoKind {
    LightSphere,
    LightSpotCone(f32),
    FogBox,
    DecalQuad,
}

fn detect_gizmo_kind(behaviours: &[Behaviour]) -> Option<GizmoKind> {
    for b in behaviours {
        match b {
            Behaviour::Light(l) => {
                return Some(if is_spot_light(l) {
                    let length = l
                        .corona_size_keyframes
                        .as_ref()
                        .and_then(|k| k.entries.first().map(|e| e.value[0]))
                        .unwrap_or(2.0)
                        .max(0.5);
                    GizmoKind::LightSpotCone(length)
                } else {
                    GizmoKind::LightSphere
                });
            }
            Behaviour::FogVolume(_) => return Some(GizmoKind::FogBox),
            Behaviour::Decal(_) => return Some(GizmoKind::DecalQuad),
            _ => {}
        }
    }
    None
}

fn is_spot_light(l: &LightBehaviour) -> bool {
    l.light_type != 0
}

fn push_wire_disc_oriented(
    out: &mut Vec<LineVertex>,
    centre: Vec3,
    normal: Vec3,
    radius: f32,
    colour: [f32; 4],
) {
    const SEGMENTS: usize = 24;
    let n = normal.normalize_or_zero();
    let n = if n.length_squared() < 1e-6 {
        Vec3::Z
    } else {
        n
    };
    let helper = if n.dot(Vec3::Z).abs() > 0.95 {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let tangent = n.cross(helper).normalize_or_zero();
    let bitangent = n.cross(tangent).normalize_or_zero();
    let two_pi = 2.0 * std::f32::consts::PI;
    let mut prev = centre + tangent * radius;
    for i in 1..=SEGMENTS {
        let t = (i as f32) / (SEGMENTS as f32) * two_pi;
        let (s, c) = t.sin_cos();
        let p = centre + tangent * (c * radius) + bitangent * (s * radius);
        out.push(LineVertex::new([prev.x, prev.y, prev.z], colour));
        out.push(LineVertex::new([p.x, p.y, p.z], colour));
        prev = p;
    }
}

fn push_wire_sphere(out: &mut Vec<LineVertex>, centre: Vec3, radius: f32, colour: [f32; 4]) {
    const SEGMENTS: usize = 24;
    let two_pi = 2.0 * std::f32::consts::PI;
    for axis in 0..3 {
        for i in 0..SEGMENTS {
            let t0 = (i as f32) / (SEGMENTS as f32) * two_pi;
            let t1 = ((i + 1) as f32) / (SEGMENTS as f32) * two_pi;
            let (a, b) = (t0.cos() * radius, t0.sin() * radius);
            let (c, d) = (t1.cos() * radius, t1.sin() * radius);
            let (p0, p1) = match axis {
                0 => (
                    Vec3::new(centre.x + a, centre.y + b, centre.z),
                    Vec3::new(centre.x + c, centre.y + d, centre.z),
                ),
                1 => (
                    Vec3::new(centre.x + a, centre.y, centre.z + b),
                    Vec3::new(centre.x + c, centre.y, centre.z + d),
                ),
                _ => (
                    Vec3::new(centre.x, centre.y + a, centre.z + b),
                    Vec3::new(centre.x, centre.y + c, centre.z + d),
                ),
            };
            out.push(LineVertex::new([p0.x, p0.y, p0.z], colour));
            out.push(LineVertex::new([p1.x, p1.y, p1.z], colour));
        }
    }
}

fn push_wire_cone(
    out: &mut Vec<LineVertex>,
    apex: Vec3,
    rotation: Quat,
    base_radius: f32,
    length: f32,
    colour: [f32; 4],
) {
    const SEGMENTS: usize = 16;
    let forward = rotation * Vec3::Z;
    let up = rotation * Vec3::Y;
    let right = forward.cross(up).normalize_or_zero();
    let true_up = right.cross(forward).normalize_or_zero();
    let base_centre = apex + forward * length;
    let two_pi = 2.0 * std::f32::consts::PI;
    let mut prev_pt: Option<Vec3> = None;
    let mut first_pt: Option<Vec3> = None;
    for i in 0..SEGMENTS {
        let t = (i as f32) / (SEGMENTS as f32) * two_pi;
        let pt = base_centre + (right * t.cos() + true_up * t.sin()) * base_radius;
        if let Some(prev) = prev_pt {
            out.push(LineVertex::new([prev.x, prev.y, prev.z], colour));
            out.push(LineVertex::new([pt.x, pt.y, pt.z], colour));
        } else {
            first_pt = Some(pt);
        }
        if i % 4 == 0 {
            out.push(LineVertex::new([apex.x, apex.y, apex.z], colour));
            out.push(LineVertex::new([pt.x, pt.y, pt.z], colour));
        }
        prev_pt = Some(pt);
    }
    if let (Some(prev), Some(first)) = (prev_pt, first_pt) {
        out.push(LineVertex::new([prev.x, prev.y, prev.z], colour));
        out.push(LineVertex::new([first.x, first.y, first.z], colour));
    }
}

fn push_wire_box(out: &mut Vec<LineVertex>, centre: Vec3, half_extents: Vec3, colour: [f32; 4]) {
    let h = half_extents.max(Vec3::splat(0.001));
    let corners = [
        Vec3::new(-h.x, -h.y, -h.z),
        Vec3::new(h.x, -h.y, -h.z),
        Vec3::new(h.x, h.y, -h.z),
        Vec3::new(-h.x, h.y, -h.z),
        Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(h.x, -h.y, h.z),
        Vec3::new(h.x, h.y, h.z),
        Vec3::new(-h.x, h.y, h.z),
    ];
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    for (a, b) in edges {
        let pa = centre + corners[a];
        let pb = centre + corners[b];
        out.push(LineVertex::new([pa.x, pa.y, pa.z], colour));
        out.push(LineVertex::new([pb.x, pb.y, pb.z], colour));
    }
}

fn push_wire_quad_oriented(
    out: &mut Vec<LineVertex>,
    centre: Vec3,
    rotation: Quat,
    w: f32,
    h: f32,
    colour: [f32; 4],
) {
    let hx = w * 0.5;
    let hy = h * 0.5;
    let local = [
        Vec3::new(-hx, -hy, 0.0),
        Vec3::new(hx, -hy, 0.0),
        Vec3::new(hx, hy, 0.0),
        Vec3::new(-hx, hy, 0.0),
    ];
    let p: Vec<Vec3> = local.iter().map(|v| centre + rotation * *v).collect();
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0)];
    for (a, b) in edges {
        let pa = p[a];
        let pb = p[b];
        out.push(LineVertex::new([pa.x, pa.y, pa.z], colour));
        out.push(LineVertex::new([pb.x, pb.y, pb.z], colour));
    }
    let tip = centre + rotation * Vec3::new(0.0, 0.0, hx.max(hy));
    out.push(LineVertex::new([centre.x, centre.y, centre.z], colour));
    out.push(LineVertex::new([tip.x, tip.y, tip.z], colour));
}
