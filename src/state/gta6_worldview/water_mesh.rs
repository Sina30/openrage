//! The mesh of the water about the camera: each quad, or the triangle of it a cut leaves, cut along
//! one grid of the world so that quads meet, and coloured at each vertex by the water's depth.

use glam::Vec3;
use or_io::gta6::water::height::WaterGround;
use or_io::gta6::water::WaterQuad;
use or_renderer::MeshVertex;

/// The side of a cell the quads are cut into, and of one within `NEAR` of the camera, in metres.
// ponytail: two sizes of cell; a cell size that grows with distance if far shores show their steps.
const COARSE: f32 = 64.0;
const FINE: f32 = 16.0;
const NEAR: f32 = 1_000.0;
/// The depth over which a shore fades in, in metres.
const SHORE: f32 = 1.0;
/// What turns the day's extinction into one a metre: its unit is not known.
// ponytail: chosen so that red is gone in about a metre and blue-green in about two.
const PATH_SCALE: f32 = 0.1;
/// The water's colour where the day states no medium.
const TINT: [f32; 3] = [0.05, 0.18, 0.22];

/// The water as a medium light passes through, from the day's water variables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Medium {
    /// How fast each channel is taken out along a path, in the day's unit.
    pub extinction: [f32; 3],
    /// The share of each channel taken out that comes back as the water's own colour.
    pub albedo: [f32; 3],
}

impl Medium {
    /// The medium the variables `get` names state; `None` where they state no positive extinction.
    pub fn of(get: impl Fn(&str) -> Option<f32>) -> Option<Self> {
        let rgb = |stem: &str| -> Option<[f32; 3]> {
            Some([
                get(&format!("{stem}_r"))?,
                get(&format!("{stem}_g"))?,
                get(&format!("{stem}_b"))?,
            ])
        };
        let scale = |name: &str| get(name).unwrap_or(1.0);
        let extinction =
            rgb("water_clip_extinction")?.map(|e| e * scale("water_clip_extinction_scale"));
        let scattering = rgb("water_clip_scattering")
            .unwrap_or_default()
            .map(|s| s * scale("water_clip_scattering_scale"));
        let tint = rgb("water_clip_fog_color_tint").unwrap_or([1.0; 3]);
        let brightness = scale("water_clip_fog_brightness");
        if !extinction.iter().all(|e| *e > 0.0) {
            return None;
        }
        Some(Self {
            extinction,
            albedo: std::array::from_fn(|c| {
                scattering[c] / extinction[c] * tint[c] * brightness
            }),
        })
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The colour and opacity of the water at a point: `cover` the quad's own opacity there, `depth` the water over the ground where that is known.
fn shade(cover: f32, depth: Option<f32>, medium: Option<&Medium>) -> [f32; 4] {
    // Dry ground draws no water, and a shore fades in over its first metre.
    let shore = depth.map_or(1.0, |d| smoothstep(d / SHORE));
    let Some(medium) = medium else {
        return [TINT[0], TINT[1], TINT[2], cover * shore];
    };
    // What the water lets through of the ground under it, and what of the light it holds instead.
    let opaque: [f32; 3] = std::array::from_fn(|c| {
        let through = depth.map_or(1.0, |d| {
            (-PATH_SCALE * medium.extinction[c] * d.max(0.0)).exp()
        });
        1.0 - (1.0 - cover) * through
    });
    // ⚠ A blend has one opacity: the channels share their mean, and the colour carries the rest.
    let alpha = opaque.iter().sum::<f32>() / 3.0;
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let colour: [f32; 3] = std::array::from_fn(|c| opaque[c] * medium.albedo[c] / alpha);
    [colour[0], colour[1], colour[2], alpha * shore]
}

/// Where a point at `u`, `v` across a quad of `kind` is on its water: at or above nought.
// A cut and its mirror are exact negations, so that the two meet at the very points of their diagonal.
fn side(kind: u8, u: f32, v: f32) -> f32 {
    match kind {
        1 => 1.0 - u - v,
        2 => v - u,
        3 => -(1.0 - u - v),
        4 => -(v - u),
        _ => 1.0,
    }
}

/// The pieces of `lo..hi` cut at every multiple of `step`.
fn spans(lo: f32, hi: f32, step: f32) -> impl Iterator<Item = (f32, f32)> {
    let mut at = lo;
    std::iter::from_fn(move || {
        let end = (((at / step).floor() + 1.0) * step).min(hi);
        // A piece that would not move on ends the pieces, as one past `hi` does.
        if at >= hi || end <= at {
            return None;
        }
        let piece = (at, end);
        at = end;
        Some(piece)
    })
}

/// A quad's corners in metres, and how to colour a point of it.
struct Quad<'a> {
    quad: &'a WaterQuad,
    min: [f32; 2],
    max: [f32; 2],
    ground: Option<&'a WaterGround>,
    medium: Option<&'a Medium>,
}

impl Quad<'_> {
    fn across(&self, x: f32, y: f32) -> (f32, f32) {
        (
            (x - self.min[0]) / (self.max[0] - self.min[0]),
            (y - self.min[1]) / (self.max[1] - self.min[1]),
        )
    }

    fn vertex(&self, x: f32, y: f32) -> MeshVertex {
        let (u, v) = self.across(x, y);
        let [sw, se, ne, nw] = self.quad.alpha.map(|a| f32::from(a) / 255.0);
        // A cut quad's opacity is barycentric over the three corners it keeps.
        let cover = match self.quad.kind {
            1 => sw * (1.0 - u - v) + se * u + nw * v,
            2 => sw * (1.0 - v) + ne * u + nw * (v - u),
            3 => se * (1.0 - v) + ne * (u + v - 1.0) + nw * (1.0 - u),
            4 => sw * (1.0 - u) + se * (u - v) + ne * v,
            _ => sw * (1.0 - u) * (1.0 - v) + se * u * (1.0 - v) + ne * u * v + nw * (1.0 - u) * v,
        };
        let z = self.quad.z;
        let depth = self.ground.and_then(|g| g.ground(x, y)).map(|h| z - h);
        let colour = shade(cover, depth, self.medium);
        MeshVertex::new_coloured([x, y, z], [0.0, 0.0, 1.0], [x, y], colour)
    }

    /// The cell `x0..x1` by `y0..y1` of the quad, less what its cut leaves out, as a fan of triangles counter-clockwise from above.
    fn cell(&self, x: (f32, f32), y: (f32, f32), out: &mut (Vec<MeshVertex>, Vec<u32>)) {
        let corners = [(x.0, y.0), (x.1, y.0), (x.1, y.1), (x.0, y.1)];
        let kind = self.quad.kind;
        let on = |(px, py): (f32, f32)| {
            let (u, v) = self.across(px, py);
            side(kind, u, v)
        };
        let mut kept: Vec<(f32, f32)> = Vec::with_capacity(5);
        for k in 0..4 {
            let (p, q) = (corners[k], corners[(k + 1) % 4]);
            let (fp, fq) = (on(p), on(q));
            if fp >= 0.0 {
                kept.push(p);
            }
            if (fp > 0.0 && fq < 0.0) || (fp < 0.0 && fq > 0.0) {
                let t = fp / (fp - fq);
                kept.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
            }
        }
        if kept.len() < 3 {
            return;
        }
        let first = out.0.len() as u32;
        out.0.extend(kept.iter().map(|&(px, py)| self.vertex(px, py)));
        for k in 1..kept.len() as u32 - 1 {
            out.1.extend([first, first + k, first + k + 1]);
        }
    }
}

/// The mesh of the quads of `quads` that are drawn and lie within `reach` metres of `eye`, each cut into cells of one grid of the world: in the world, the surface up, the texture coordinate the place in metres, and in the colour the water's own colour and opacity, by its depth over `ground` and the day's `medium` where they are known.
pub fn mesh(
    quads: &[WaterQuad],
    eye: Vec3,
    reach: f32,
    ground: Option<&WaterGround>,
    medium: Option<&Medium>,
) -> (Vec<MeshVertex>, Vec<u32>) {
    let mut out = (Vec::new(), Vec::new());
    // A quad with no area is passed over before its bounds are clamped to, which a reversed pair panics.
    let whole = |q: &&WaterQuad| q.min[0] < q.max[0] && q.min[1] < q.max[1];
    for quad in (quads.iter()).filter(|q| q.kind <= 4 && !q.invisible && whole(q)) {
        let (min, max) = (quad.min.map(f32::from), quad.max.map(f32::from));
        let nearest = |lo: [f32; 2], hi: [f32; 2]| {
            Vec3::new(eye.x.clamp(lo[0], hi[0]), eye.y.clamp(lo[1], hi[1]), eye.z)
        };
        if nearest(min, max).distance(eye) > reach {
            continue;
        }
        let quad = Quad {
            quad,
            min,
            max,
            ground,
            medium,
        };
        // Only what lies within reach along each axis is cut into cells.
        let lo = [min[0].max(eye.x - reach), min[1].max(eye.y - reach)];
        let hi = [max[0].min(eye.x + reach), max[1].min(eye.y + reach)];
        for x in spans(lo[0], hi[0], COARSE) {
            for y in spans(lo[1], hi[1], COARSE) {
                let close = nearest([x.0, y.0], [x.1, y.1]).distance(eye) < NEAR;
                if !close {
                    quad.cell(x, y, &mut out);
                    continue;
                }
                for fine_x in spans(x.0, x.1, FINE) {
                    for fine_y in spans(y.0, y.1, FINE) {
                        quad.cell(fine_x, fine_y, &mut out);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "water_mesh_tests.rs"]
mod tests;
