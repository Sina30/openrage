//! The occluders nearest the camera drawn on the CPU into a small depth buffer each time the view
//! changes, and the test of a sphere against it.
//!
//! ⚠ The buffer never hides more than the occluders do: what reaches the near plane is cut off
//! there, an occluder that holds the eye is left out, and a texel counts as covered only where
//! every corner of it is, at the farthest of their depths.

use std::ops::RangeInclusive;
use std::sync::Arc;

use glam::{Mat4, Vec2, Vec3, Vec4};
use or_io::gta6::map::occluders::{MapOccluders, OccludeModel, BOX_TRIANGLES};
use rayon::prelude::*;

/// The raster the occluders are drawn into, in texels.
const WIDE: usize = 128;
const HIGH: usize = 72;
/// The corners of the raster's texels, a row of them after another.
const CORNERS_WIDE: usize = WIDE + 1;
const CORNERS: usize = CORNERS_WIDE * (HIGH + 1);
/// A sphere is tested against texels of 2 x 2 of the raster's.
const HIZ_WIDE: usize = WIDE / 2;
const HIZ_HIGH: usize = HIGH / 2;
const GRID: Vec2 = Vec2::new(WIDE as f32, HIGH as f32);
/// The screen area, in texels of the raster, a box and a model must pass to be drawn.
const BOX_AREA: f32 = 20.0;
const MODEL_AREA: f32 = 96.0;
/// The most boxes and models drawn, the largest on the screen first.
const MOST_BOXES: usize = 512;
const MOST_MODELS: usize = 416;
/// How far along the view a point must lie ahead of the eye to be drawn or tested, in metres.
const NEAR: f32 = 0.1;
/// The rows of corners a core fills at a time.
const BAND: usize = 4;

/// A view's matrix, by its bits, and the occluders drawn for it.
type DrawnFor = ([u32; 16], Arc<[Arc<MapOccluders>]>);

/// The occluders as drawn for one view.
#[derive(Default)]
pub(super) struct Occlusion {
    /// The view's matrix and the occluders the buffer was drawn for.
    drawn_for: Option<DrawnFor>,
    view_proj: Mat4,
    /// One over the depth of the nearest occluder at each corner of a texel; 0 where none is.
    corners: Vec<f32>,
    /// The farthest of the 3 x 3 corners of each 2 x 2 texels, the same way.
    hiz: Vec<f32>,
    /// Whether any texel is covered.
    any: bool,
    /// How many boxes and models were drawn.
    pub drawn: (usize, usize),
}

/// The corners of the box from `min` to `max`, numbered as a box occluder numbers its own.
fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|k| {
        let pick = |bit: usize, axis: usize| {
            if k >> bit & 1 == 0 {
                min[axis]
            } else {
                max[axis]
            }
        };
        Vec3::new(pick(0, 0), pick(1, 1), pick(2, 2))
    })
}

/// Whether the box of these corners holds `point`; a box with no thickness holds nothing.
fn holds(corners: &[Vec3; 8], point: Vec3) -> bool {
    let from = point - corners[0];
    [1, 2, 4].iter().all(|&k| {
        let axis = corners[k] - corners[0];
        let length = axis.length_squared();
        length > 0.0 && (0.0..=length).contains(&from.dot(axis))
    })
}

/// Where a point of clip space falls on the raster, in texels, rows down from the top.
fn screen(clip: Vec4) -> Vec2 {
    let ndc = Vec2::new(clip.x, clip.y) / clip.w;
    Vec2::new((ndc.x + 1.0) * 0.5, (1.0 - ndc.y) * 0.5) * GRID
}

/// Twice the signed area of the screen triangle `a`, `b`, `p`.
fn edge(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// The part of a triangle of the world `view_proj` sees ahead of the near plane, put on `flat`
/// as triangles of the raster whose `z` is one over depth.
fn cut(view_proj: Mat4, points: [Vec3; 3], flat: &mut Vec<[Vec3; 3]>) {
    let clips = points.map(|p| view_proj * p.extend(1.0));
    // Cut by one plane, a triangle leaves no point, three or four.
    let (mut ahead, mut count) = ([Vec3::ZERO; 4], 0);
    for k in 0..3 {
        let (p, q) = (clips[k], clips[(k + 1) % 3]);
        let mut put = |clip: Vec4| {
            ahead[count] = screen(clip).extend(1.0 / clip.w);
            count += 1;
        };
        if p.w >= NEAR {
            put(p);
        }
        if (p.w >= NEAR) != (q.w >= NEAR) {
            put(p.lerp(q, (NEAR - p.w) / (q.w - p.w)));
        }
    }
    for k in 1..count.saturating_sub(1) {
        flat.push([ahead[0], ahead[k], ahead[k + 1]]);
    }
}

/// The columns and the rows of corners a screen triangle may hold; none for one seen edge on
/// or off the raster.
fn bounds([a, b, c]: &[Vec3; 3]) -> Option<(RangeInclusive<usize>, RangeInclusive<usize>)> {
    let (lo, hi) = (a.min(*b).min(*c), a.max(*b).max(*c));
    if edge(*a, *b, *c).abs() <= f32::EPSILON
        || hi.x < 0.0
        || hi.y < 0.0
        || lo.x > GRID.x
        || lo.y > GRID.y
    {
        return None;
    }
    // Float to integer casts hold at the bounds, and a bound past the raster is held to it.
    let (x0, y0) = (lo.x.max(0.0).ceil() as usize, lo.y.max(0.0).ceil() as usize);
    let (x1, y1) = (hi.x.min(GRID.x) as usize, hi.y.min(GRID.y) as usize);
    (y0 <= y1).then_some((x0..=x1, y0..=y1))
}

/// Mark each corner in `rows`, the rows of corners from `first` on, that the screen triangle
/// `a`, `b`, `c` holds with its one over depth there, where that is nearer than what is marked.
fn fill(rows: &mut [f32], first: usize, [a, b, c]: [Vec3; 3]) {
    let Some((columns, held)) = bounds(&[a, b, c]) else {
        return;
    };
    let twice = edge(a, b, c);
    let last = first + rows.len() / CORNERS_WIDE;
    for j in *held.start().max(&first)..=*held.end().min(&(last - 1)) {
        for i in columns.clone() {
            let p = Vec3::new(i as f32, j as f32, 0.0);
            let (u, v, w) = (edge(b, c, p), edge(c, a, p), edge(a, b, p));
            let inside = if twice > 0.0 {
                u >= 0.0 && v >= 0.0 && w >= 0.0
            } else {
                u <= 0.0 && v <= 0.0 && w <= 0.0
            };
            if inside {
                let near = (u * a.z + v * b.z + w * c.z) / twice;
                let at = &mut rows[(j - first) * CORNERS_WIDE + i];
                *at = at.max(near);
            }
        }
    }
}

/// Keep the `most` largest of `list`, the largest first.
fn largest<T>(list: &mut Vec<(f32, T)>, most: usize) {
    list.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    list.truncate(most);
}

impl Occlusion {
    /// Draw `occluders` as `view_proj` sees them from `eye`, unless they are drawn for it already.
    pub(super) fn draw(
        &mut self,
        occluders: &Arc<[Arc<MapOccluders>]>,
        view_proj: Mat4,
        eye: Vec3,
    ) {
        let bits = view_proj.to_cols_array().map(f32::to_bits);
        if let Some((at, of)) = &self.drawn_for {
            if *at == bits && Arc::ptr_eq(of, occluders) {
                return;
            }
        }
        // ⚠ The occluders drawn for are held, so that no other list is ever met at their address.
        self.drawn_for = Some((bits, Arc::clone(occluders)));
        self.view_proj = view_proj;
        self.corners.clear();
        self.corners.resize(CORNERS, 0.0);
        let mut boxes: Vec<(f32, [Vec3; 8])> = Vec::new();
        let mut models: Vec<(f32, &OccludeModel)> = Vec::new();
        for map in occluders.iter() {
            for occluder in &map.boxes {
                let corners = occluder.corners.map(Vec3::from);
                if let Some(area) = self.area(&corners, eye).filter(|a| *a > BOX_AREA) {
                    boxes.push((area, corners));
                }
            }
            for model in &map.models {
                let (min, max) = (model.bounds.min.into(), model.bounds.max.into());
                let area = self.area(&box_corners(min, max), eye);
                if let Some(area) = area.filter(|a| *a > MODEL_AREA) {
                    models.push((area, model));
                }
            }
        }
        largest(&mut boxes, MOST_BOXES);
        largest(&mut models, MOST_MODELS);
        self.drawn = (boxes.len(), models.len());
        let view_proj = self.view_proj;
        let mut flat = Vec::new();
        for (_, corners) in &boxes {
            for triangle in BOX_TRIANGLES {
                cut(
                    view_proj,
                    triangle.map(|k| corners[usize::from(k)]),
                    &mut flat,
                );
            }
        }
        // ⚠ On this core: tens of thousands of triangles cut in a millisecond, where cores the streaming workers keep busy made a parallel cut wait on the slowest.
        for (_, model) in &models {
            for triangle in &model.triangles {
                let point = |k: u8| model.vertices.get(usize::from(k)).copied().map(Vec3::from);
                if let [Some(a), Some(b), Some(c)] = triangle.map(point) {
                    cut(view_proj, [a, b, c], &mut flat);
                }
            }
        }
        self.raster(&flat);
        self.reduce();
    }

    /// Mark every corner each screen triangle of `flat` holds, a band of rows on each core.
    // ⚠ A corner takes the nearest of the triangles that hold it, which is the same in any order: the buffer is the one a single core draws.
    fn raster(&mut self, flat: &[[Vec3; 3]]) {
        let mut binned = vec![Vec::new(); (HIGH + 1).div_ceil(BAND)];
        for (k, triangle) in flat.iter().enumerate() {
            if let Some((_, rows)) = bounds(triangle) {
                for band in &mut binned[*rows.start() / BAND..=*rows.end() / BAND] {
                    band.push(k);
                }
            }
        }
        (self.corners.par_chunks_mut(CORNERS_WIDE * BAND))
            .zip(binned.par_iter())
            .enumerate()
            .for_each(|(band, (rows, list))| {
                for k in list {
                    fill(rows, band * BAND, flat[*k]);
                }
            });
    }

    /// The screen area of the box of these corners in texels of the raster, the whole screen's
    /// where it reaches past the near plane; none where it holds `eye` or lies wholly behind it.
    fn area(&self, corners: &[Vec3; 8], eye: Vec3) -> Option<f32> {
        if holds(corners, eye) {
            return None;
        }
        let clips = corners.map(|c| self.view_proj * c.extend(1.0));
        match clips.iter().filter(|c| c.w >= NEAR).count() {
            0 => return None,
            8 => {}
            _ => return Some(GRID.x * GRID.y),
        }
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for clip in clips {
            let at = screen(clip);
            (lo, hi) = (lo.min(at), hi.max(at));
        }
        let size = (hi.min(GRID) - lo.max(Vec2::ZERO)).max(Vec2::ZERO);
        Some(size.x * size.y)
    }

    /// Reduce the corners to the farthest of each 2 x 2 texels.
    // ponytail: point samples at the corners; a gap between occluders narrower than a texel of 128 x 72 is not seen, so what shows through it alone is hidden. Sample finer if it shows.
    fn reduce(&mut self) {
        self.hiz.clear();
        for v in 0..HIZ_HIGH {
            for u in 0..HIZ_WIDE {
                let corners = (2 * v..=2 * v + 2)
                    .flat_map(|j| (2 * u..=2 * u + 2).map(move |i| j * CORNERS_WIDE + i));
                let farthest = corners
                    .map(|at| self.corners[at])
                    .fold(f32::INFINITY, f32::min);
                self.hiz.push(farthest);
            }
        }
        self.any = self.hiz.iter().any(|near| *near > 0.0);
    }

    /// Whether the occluders drawn hide every point of the sphere at `centre`.
    pub(super) fn hides(&self, centre: Vec3, radius: f32) -> bool {
        if !self.any {
            return false;
        }
        let depth = self.view_proj.row(3);
        let nearest = depth.dot(centre.extend(1.0)) - radius * depth.truncate().length();
        // A sphere that reaches the near plane is in front of everything.
        if nearest.is_nan() || nearest < NEAR {
            return false;
        }
        // The sphere is tested by its box, which holds it.
        let reach = Vec3::splat(radius);
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for corner in box_corners(centre - reach, centre + reach) {
            let clip = self.view_proj * corner.extend(1.0);
            if clip.w.is_nan() || clip.w < NEAR {
                return false;
            }
            let at = screen(clip);
            (lo, hi) = (lo.min(at), hi.max(at));
        }
        let (lo, hi) = (lo.max(Vec2::ZERO), hi.min(GRID));
        // Off the screen it is the frustum's to judge.
        if lo.x > hi.x || lo.y > hi.y {
            return false;
        }
        let texel = |at: f32, most: usize| ((at / 2.0) as usize).min(most - 1);
        let (u0, u1) = (texel(lo.x, HIZ_WIDE), texel(hi.x, HIZ_WIDE));
        let (v0, v1) = (texel(lo.y, HIZ_HIGH), texel(hi.y, HIZ_HIGH));
        let behind = 1.0 / nearest;
        (v0..=v1).all(|v| (u0..=u1).all(|u| self.hiz[v * HIZ_WIDE + u] > behind))
    }
}

#[cfg(test)]
#[path = "world_occlusion_tests.rs"]
mod tests;
