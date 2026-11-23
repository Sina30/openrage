//! Body skin added to a garment that has holes: the skin geometries of a donor drawable, moved into the garment.
//!
//! A ped garment that replaces the legs draws nothing where it has a hole, so the packs that did not want to model the skin under
//! a ripped pair of jeans filed them as undershirts (which draw over the legs the game keeps). The way to make such a garment a
//! legs garment is the one the game's own are made: a geometry of the body with the skin material, drawn under the garment.
//!
//! ⚠ **Nothing is made.** The donor's geometries, vertex buffers, skinning palettes and shaders are the donor's own, moved as they
//! are. A level the garment has no model for gets nothing (its holes at that distance show nothing). The garment's own geometries,
//! shaders, textures and levels are not touched.
//!
//! ⚠ **A ped component draws every material with the texture variation, whatever name the material states** (a naked legs drawable's
//! variations are skin pictures), so the grafted skin reads the garment's own pictures at its texture coordinates. `uv` collapses
//! the grafted geometries' first texture coordinates to one point of the texture: the caller paints skin there in every variation.

use std::collections::{BTreeMap, HashMap, HashSet};

use or_io::codec::mesh::{encode, GrcFvfChannel, VertexReader, VertexSource};
use or_io::decl::{Gen8, Shared};
use or_io::error::{IoError, Result};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::{GrmGeometry, RmcDrawable};

use crate::import::{recompute_aabbs, Imported};

/// Writes `values` (one per vertex) into a channel of `geometry`; why not, when it could not (an interleaved channel in a buffer that holds its own bytes).
fn write_channel(
    geometry: &mut GrmGeometry<Gen8>,
    channel: GrcFvfChannel,
    values: &[[f32; 4]],
) -> std::result::Result<(), String> {
    let layout = geometry
        .vertex_buffer
        .vertex_layout()
        .ok_or("the vertex buffer declares no layout")?;
    let slot = layout
        .slot(channel)
        .ok_or("the geometry lacks the channel")?;
    let size = slot.size.byte_size();
    let stride = geometry.vertex_buffer.vertex_stride();
    if stride == 0 {
        return Err("the vertex buffer states no stride".into());
    }
    if slot.stride != 0 {
        return Err(format!(
            "the channel is a run of its own (step {})",
            slot.stride
        ));
    }
    // The buffer states its vertices twice (`lock_ptr` is the same run again): a copy of its own is edited with the first, or it would be written back stale.
    let buffer = &mut *geometry.vertex_buffer;
    let mut second = match &mut buffer.lock_ptr {
        Shared::Own(copy) if copy.is_empty() => None,
        Shared::Own(copy) if copy.len() == buffer.data.len() => Some(copy),
        Shared::Own(copy) => {
            return Err(format!(
                "the vertex buffer keeps a second copy of {} bytes next to its {}",
                copy.len(),
                buffer.data.len()
            ))
        }
        _ => None,
    };
    for (vertex, value) in values.iter().enumerate() {
        let at = vertex * stride + slot.offset;
        let bytes = buffer
            .data
            .get_mut(at..at + size)
            .ok_or("a vertex runs past the buffer")?;
        encode(slot.size, *value, bytes);
        if let Some(copy) = second.as_deref_mut() {
            copy[at..at + size].copy_from_slice(&buffer.data[at..at + size]);
        }
    }
    Ok(())
}

/// Rewrites the first texture coordinates of every vertex of `geometry` through `f`.
fn edit_uv(
    geometry: &mut GrmGeometry<Gen8>,
    f: impl Fn([f32; 2]) -> [f32; 2],
) -> std::result::Result<(), String> {
    let values: Vec<[f32; 4]> = {
        let reader =
            VertexReader::new(&*geometry.vertex_buffer).ok_or("the vertices cannot be read")?;
        (0..reader.count())
            .map(|v| {
                reader.get(GrcFvfChannel::Tex0, v).map(|x| {
                    let uv = f([x[0], x[1]]);
                    [uv[0], uv[1], 0.0, 0.0]
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("a vertex has no texture coordinates")?
    };
    write_channel(geometry, GrcFvfChannel::Tex0, &values)
}

// ---------- the skin that pokes through the garment
// The donor's skin lies on the body surface, so where the garment's surface is nearer the body than that (a hip band, a fold of a pleat) the skin, and the thong with it, show through the cloth.
// Test, per skin vertex, with the vertex's outward direction: cloth OUTSIDE it (a ray out meets the garment) = hidden, left alone (and pushed in when the cloth is less than a margin away: z-fight);
// cloth BEHIND it (nothing out, a short ray in meets the garment) = poking through, pulled back inside the cloth; cloth on neither side = a hole, the skin is meant to show there.
const OUT_REACH: f32 = 0.40;
const IN_REACH: f32 = 0.06;
/// How far under the cloth a pulled-in vertex ends (metres).
const MARGIN: f32 = 0.003;
const CELL: f32 = 0.04;

/// The garment's triangles in a grid, for the rays.
struct Surface {
    triangles: Vec<[[f32; 3]; 3]>,
    cells: HashMap<(i32, i32, i32), Vec<u32>>,
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl Surface {
    fn of<'a>(geometries: impl Iterator<Item = &'a GrmGeometry<Gen8>>) -> Self {
        let mut surface = Surface {
            triangles: Vec::new(),
            cells: HashMap::new(),
        };
        for geometry in geometries {
            let mesh = geometry.mesh();
            for t in mesh.indices.as_chunks::<3>().0 {
                let (Some(&a), Some(&b), Some(&c)) = (
                    mesh.positions.get(t[0] as usize),
                    mesh.positions.get(t[1] as usize),
                    mesh.positions.get(t[2] as usize),
                ) else {
                    continue;
                };
                let id = surface.triangles.len() as u32;
                surface.triangles.push([a, b, c]);
                let lo = |k: usize| (a[k].min(b[k]).min(c[k]) / CELL).floor() as i32;
                let hi = |k: usize| (a[k].max(b[k]).max(c[k]) / CELL).floor() as i32;
                for x in lo(0)..=hi(0) {
                    for y in lo(1)..=hi(1) {
                        for z in lo(2)..=hi(2) {
                            surface.cells.entry((x, y, z)).or_default().push(id);
                        }
                    }
                }
            }
        }
        surface
    }

    /// The nearest distance along the ray (`origin`, unit `dir`) at which the surface is crossed, between `min` and `reach` (either side of a triangle counts).
    fn hit(&self, origin: [f32; 3], dir: [f32; 3], min: f32, reach: f32) -> Option<f32> {
        let mut seen: HashSet<u32> = HashSet::new();
        let mut best: Option<f32> = None;
        let step = CELL * 0.5;
        let mut t = 0.0f32;
        while t <= reach + step {
            let p = [
                origin[0] + dir[0] * t,
                origin[1] + dir[1] * t,
                origin[2] + dir[2] * t,
            ];
            let cell = (
                (p[0] / CELL).floor() as i32,
                (p[1] / CELL).floor() as i32,
                (p[2] / CELL).floor() as i32,
            );
            if let Some(list) = self.cells.get(&cell) {
                for &id in list {
                    if !seen.insert(id) {
                        continue;
                    }
                    let [a, b, c] = self.triangles[id as usize];
                    // Moller-Trumbore, two sided
                    let (e1, e2) = (sub3(b, a), sub3(c, a));
                    let h = cross3(dir, e2);
                    let det = dot3(e1, h);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let s = sub3(origin, a);
                    let u = dot3(s, h) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = cross3(s, e1);
                    let v = dot3(dir, q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let d = dot3(e2, q) * inv;
                    if d >= min && d <= reach && best.is_none_or(|b| d < b) {
                        best = Some(d);
                    }
                }
            }
            if best.is_some_and(|b| b <= t) {
                break;
            }
            t += step;
        }
        best
    }
}

/// The skin's triangles whose vertices are not all under the cloth (some are, some are not: the skin comes out from under the cloth there) are cut smaller `rounds` times, an edge that has one
/// vertex under the cloth and one outside it at its middle: the skin's triangles are far larger than the cloth's folds and hem, and a large triangle that crosses the cloth cuts it in a zigzag. The legs
/// are not squeezed to avoid that: the skin keeps its shape and has more vertices where it meets the cloth. Every triangle that touches a cut edge is cut with it, so that no crack opens; the new
/// vertex copies the bones of one end of the edge and takes the mean of the position, normal, colours and coordinates. Returns the vertices added.
fn refine(
    skin: &mut GrmGeometry<Gen8>,
    surface: &Surface,
    rounds: usize,
) -> std::result::Result<usize, String> {
    let mut added = 0;
    for _ in 0..rounds {
        let n = refine_once(skin, surface)?;
        if n == 0 {
            break;
        }
        added += n;
    }
    Ok(added)
}

fn refine_once(
    skin: &mut GrmGeometry<Gen8>,
    surface: &Surface,
) -> std::result::Result<usize, String> {
    let mesh = skin.mesh();
    if mesh.normals.len() != mesh.positions.len() {
        return Err("the skin has no normals to tell outside from inside".into());
    }
    let count = mesh.positions.len();
    let (group, gdirs) = welded(&mesh.positions, &mesh.normals);
    let mut at: Vec<[f32; 3]> = vec![[0.0; 3]; gdirs.len()];
    for (i, p) in mesh.positions.iter().enumerate() {
        at[group[i]] = *p;
    }
    let under_group: Vec<bool> = (0..gdirs.len())
        .map(|gi| surface.hit(at[gi], gdirs[gi], 0.0, OUT_REACH).is_some())
        .collect();
    let under: Vec<bool> = group.iter().map(|gi| under_group[*gi]).collect();
    let key = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    let mut cut: Vec<(u32, u32)> = Vec::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if under.get(a as usize) != under.get(b as usize) {
                cut.push(key(a, b));
            }
        }
    }
    for (ti, poking) in poking_triangles(skin, surface).into_iter().enumerate() {
        if poking {
            let t = &mesh.indices[ti * 3..ti * 3 + 3];
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                cut.push(key(a, b));
            }
        }
    }
    cut.sort_unstable();
    cut.dedup();
    // the twin of an edge cut on one side of a seam (the same two positions) is cut as well, or the two sides would not meet
    {
        let mut cut_groups: HashSet<(usize, usize)> = HashSet::new();
        for &(a, b) in &cut {
            let (ga, gb) = (group[a as usize], group[b as usize]);
            cut_groups.insert(if ga < gb { (ga, gb) } else { (gb, ga) });
        }
        for t in mesh.indices.as_chunks::<3>().0 {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (ga, gb) = (group[a as usize], group[b as usize]);
                if cut_groups.contains(&(if ga < gb { (ga, gb) } else { (gb, ga) })) {
                    cut.push(key(a, b));
                }
            }
        }
        cut.sort_unstable();
        cut.dedup();
    }
    if cut.is_empty() || count + cut.len() > usize::from(u16::MAX) {
        return Ok(0);
    }
    let layout = skin
        .vertex_buffer
        .vertex_layout()
        .ok_or("the skin declares no layout")?;
    let stride = skin.vertex_buffer.vertex_stride();
    let old: Vec<u8> = skin.vertex_buffer.data.clone();
    if stride == 0 || old.len() != count * stride {
        return Ok(0);
    }
    let mut middle: HashMap<(u32, u32), u32> = HashMap::new();
    let mut fresh: Vec<u8> = Vec::new();
    {
        let reader =
            VertexReader::new(&*skin.vertex_buffer).ok_or("the skin's vertices cannot be read")?;
        for &(a, b) in &cut {
            let mut bytes = old[a as usize * stride..(a as usize + 1) * stride].to_vec();
            for channel in [
                GrcFvfChannel::Position,
                GrcFvfChannel::Normal,
                GrcFvfChannel::Diffuse,
                GrcFvfChannel::Specular,
                GrcFvfChannel::Tex0,
                GrcFvfChannel::Tex1,
                GrcFvfChannel::Tangent0,
                GrcFvfChannel::Tangent1,
            ] {
                let Some(slot) = layout.slot(channel) else {
                    continue;
                };
                if slot.stride != 0 {
                    continue;
                }
                let (Some(va), Some(vb)) = (
                    reader.get(channel, a as usize),
                    reader.get(channel, b as usize),
                ) else {
                    continue;
                };
                let mut v = [
                    (va[0] + vb[0]) * 0.5,
                    (va[1] + vb[1]) * 0.5,
                    (va[2] + vb[2]) * 0.5,
                    (va[3] + vb[3]) * 0.5,
                ];
                if matches!(
                    channel,
                    GrcFvfChannel::Normal | GrcFvfChannel::Tangent0 | GrcFvfChannel::Tangent1
                ) {
                    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    if len > 1e-6 {
                        v[0] /= len;
                        v[1] /= len;
                        v[2] /= len;
                    }
                    if !matches!(channel, GrcFvfChannel::Normal) {
                        v[3] = va[3];
                    }
                }
                let size = slot.size.byte_size();
                encode(slot.size, v, &mut bytes[slot.offset..slot.offset + size]);
            }
            middle.insert((a, b), (count + middle.len()) as u32);
            fresh.extend_from_slice(&bytes);
        }
    }
    let mid = |a: u32, b: u32| middle.get(&key(a, b)).map(|m| *m as u16);
    let mut indices: Vec<u16> = Vec::with_capacity(mesh.indices.len() + cut.len() * 6);
    for t in mesh.indices.as_chunks::<3>().0 {
        let (v0, v1, v2) = (t[0] as u16, t[1] as u16, t[2] as u16);
        let (m01, m12, m20) = (mid(t[0], t[1]), mid(t[1], t[2]), mid(t[2], t[0]));
        // the triangle is turned until the cut edges are the first ones, which keeps its winding
        match (m01, m12, m20) {
            (None, None, None) => indices.extend([v0, v1, v2]),
            (Some(m), None, None) => indices.extend([v0, m, v2, m, v1, v2]),
            (None, Some(m), None) => indices.extend([v1, m, v0, m, v2, v0]),
            (None, None, Some(m)) => indices.extend([v2, m, v1, m, v0, v1]),
            (Some(a), Some(b), None) => indices.extend([a, v1, b, v0, a, b, v0, b, v2]),
            (None, Some(a), Some(b)) => indices.extend([a, v2, b, v1, a, b, v1, b, v0]),
            (Some(b), None, Some(a)) => indices.extend([a, v0, b, v2, a, b, v2, b, v1]),
            (Some(a), Some(b), Some(c)) => indices.extend([v0, a, c, a, v1, b, c, b, v2, a, b, c]),
        }
    }
    let mut data = old;
    data.extend_from_slice(&fresh);
    <Gen8 as Imported>::set_vertices(&mut skin.vertex_buffer, data);
    skin.vertex_data = Shared::Same;
    *<Gen8 as Imported>::indices_mut(&mut skin.index_buffer) = indices;
    skin.take_counts_from_runs();
    Ok(middle.len())
}

/// What `tuck_in` found.
struct Tucked {
    moved: usize,
    total: usize,
    /// The deepest pull, metres.
    deepest: f32,
}

/// Pulls the vertices of a skin geometry that poke through the garment back inside it.
/// The vertices of a skin that share a position are one: `group[i]` is the number of the position of vertex `i`, and `dirs` the unit direction out of the body of each position (the mean of the
/// normals of the vertices that stand there).
fn welded(positions: &[[f32; 3]], normals: &[[f32; 3]]) -> (Vec<usize>, Vec<[f32; 3]>) {
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut group = Vec::with_capacity(positions.len());
    let mut sums: Vec<[f32; 3]> = Vec::new();
    for (p, n) in positions.iter().zip(normals) {
        let key = (
            (p[0] * 20000.0).round() as i64,
            (p[1] * 20000.0).round() as i64,
            (p[2] * 20000.0).round() as i64,
        );
        let id = *ids.entry(key).or_insert_with(|| {
            sums.push([0.0; 3]);
            sums.len() - 1
        });
        let len = dot3(*n, *n).sqrt();
        if len > 1e-6 {
            for k in 0..3 {
                sums[id][k] += n[k] / len;
            }
        }
        group.push(id);
    }
    let dirs = sums
        .iter()
        .map(|s| {
            let len = dot3(*s, *s).sqrt();
            if len > 1e-6 {
                [s[0] / len, s[1] / len, s[2] / len]
            } else {
                [0.0, 0.0, 1.0]
            }
        })
        .collect();
    (group, dirs)
}

/// The most a leg is squeezed to fit under the cloth: two fifths of its width about its own axis, at the one slice of a tight hem, fading away along the leg (see `FADE`).
const SQUEEZE: f32 = 0.40;
/// What would need more than this is cut away, the cloth being in front of it. ⚠ Off (a share of a radius cannot pass 1): the ray of a slice meets the strands of a frayed hem as cloth, and the skin cut behind a rip
/// leaves a hole in it; the squeeze limit alone is what keeps a leg a leg.
const HIDE: f32 = 2.0;
/// How far a pelvis vertex is pushed in along its normal at the most (metres).
const PUSH: f32 = 0.02;
/// How the squeeze fades along the skin: each round a vertex takes the larger of what it needs and this share of the mean of its neighbours' squeeze.
const FADE: f32 = 0.90;
const FADE_ROUNDS: usize = 20;
/// The height of a slice of a leg (metres).
const SLICE: f32 = 0.005;

/// Cuts away the skin's triangles that have two or more hidden vertices (`hidden[g]` per position group): the cloth is in front of them and is what is seen. Returns how many.
fn drop_hidden(skin: &mut GrmGeometry<Gen8>, group: &[usize], hidden: &[bool]) -> usize {
    let indices = <Gen8 as Imported>::indices_mut(&mut skin.index_buffer);
    let before = indices.len() / 3;
    let kept: Vec<u16> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| {
            t.iter()
                .filter(|&&i| group.get(usize::from(i)).is_some_and(|g| hidden[*g]))
                .count()
                < 2
        })
        .flatten()
        .copied()
        .collect();
    *indices = kept;
    skin.take_counts_from_runs();
    before - skin.index_buffer.indices.len() / 3
}

/// Fits the skin of a legs geometry under the garment.
///
/// A leg is a stack of slices (`SLICE` high) about its own axis, the left and the right apart. For each vertex a ray from the axis of its slice, straight out through the vertex, finds the cloth around the leg
/// at that height: where the cloth is nearer the axis than the skin (less a margin), the vertex needs to come in by the share `1 - allowed / radius` of its radius. Nothing found, or the cloth beyond the skin
/// (a loose skirt, a hole), asks nothing: the leg keeps its shape. The squeeze is limited to `SQUEEZE`: a vertex that needs more is HIDDEN (the cloth is in front of it and is what is seen), and a triangle with
/// two hidden vertices is cut away. The rest is spread along the leg by a fading diffusion that never gives a vertex less than it needs, so the leg narrows and widens as a leg does, and each vertex is
/// moved straight in towards its axis (a round leg stays round: nothing is pushed along a normal that points the wrong way).
fn tuck_in(
    skin: &mut GrmGeometry<Gen8>,
    surface: &Surface,
    margin: f32,
) -> std::result::Result<Tucked, String> {
    let (positions, normals, indices) = {
        let mesh = skin.mesh();
        if mesh.normals.len() != mesh.positions.len() {
            return Err("the skin has no normals to tell outside from inside".into());
        }
        (mesh.positions, mesh.normals, mesh.indices)
    };
    let count = positions.len();
    let (group, gdirs) = welded(&positions, &normals);
    let groups = gdirs.len();
    let mut at: Vec<[f32; 3]> = vec![[0.0; 3]; groups];
    for (i, p) in positions.iter().enumerate() {
        at[group[i]] = *p;
    }
    // the axis of each slice of each leg: the mean of the positions in it, empty slices take the nearest one, then a moving mean takes the jitter out
    let zlo = at.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
    let zhi = at.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
    let bins = (((zhi - zlo) / SLICE) as usize + 2).max(2);
    let bin = |z: f32| (((z - zlo) / SLICE) as usize).min(bins - 1);
    let mut axis = [vec![None::<[f32; 2]>; bins], vec![None::<[f32; 2]>; bins]];
    {
        let mut sum = [vec![[0.0f32; 3]; bins], vec![[0.0f32; 3]; bins]];
        for p in &at {
            let (leg, b) = (usize::from(p[0] < 0.0), bin(p[2]));
            sum[leg][b][0] += p[0];
            sum[leg][b][1] += p[1];
            sum[leg][b][2] += 1.0;
        }
        for leg in 0..2 {
            for b in 0..bins {
                if sum[leg][b][2] > 0.0 {
                    axis[leg][b] = Some([
                        sum[leg][b][0] / sum[leg][b][2],
                        sum[leg][b][1] / sum[leg][b][2],
                    ]);
                }
            }
            let known: Vec<usize> = (0..bins).filter(|&b| axis[leg][b].is_some()).collect();
            if known.is_empty() {
                continue;
            }
            for b in 0..bins {
                if axis[leg][b].is_none() {
                    let nearest = *known
                        .iter()
                        .min_by_key(|&&k| k.abs_diff(b))
                        .unwrap_or(&known[0]);
                    axis[leg][b] = axis[leg][nearest];
                }
            }
            let raw: Vec<[f32; 2]> = axis[leg].iter().map(|a| a.unwrap_or([0.0; 2])).collect();
            for (b, slot) in axis[leg].iter_mut().enumerate() {
                let (lo, hi) = (b.saturating_sub(4), (b + 4).min(bins - 1));
                let n = (hi - lo + 1) as f32;
                *slot = Some([
                    raw[lo..=hi].iter().map(|a| a[0]).sum::<f32>() / n,
                    raw[lo..=hi].iter().map(|a| a[1]).sum::<f32>() / n,
                ]);
            }
        }
    }
    let centre = |p: [f32; 3]| axis[usize::from(p[0] < 0.0)][bin(p[2])].unwrap_or([p[0], p[1]]);
    // ⚠ Where the legs are two, a slice about the axis of each is a leg; where they join (the crotch and above) it is a pelvis, which has no axis of its own to squeeze about (the two halves would pull apart
    // along the seam, and a slit would open): the crotch is the first slice from the feet up in which the two sides touch, and from a hand below it the skin is fitted along its normals instead.
    let crotch = {
        let mut lowest = vec![(f32::MAX, f32::MAX); bins];
        for p in &at {
            let b = bin(p[2]);
            if p[0] < 0.0 {
                lowest[b].0 = lowest[b].0.min(-p[0]);
            } else {
                lowest[b].1 = lowest[b].1.min(p[0]);
            }
        }
        (0..bins)
            .find(|&b| {
                lowest[b].0 < f32::MAX && lowest[b].1 < f32::MAX && lowest[b].0 + lowest[b].1 < 0.02
            })
            .map_or(f32::MAX, |b| zlo + b as f32 * SLICE)
    };
    let pelvis = |p: [f32; 3]| p[2] > crotch - 0.03;
    // what each position needs, as a share of its radius about its axis
    let mut need = vec![0.0f32; groups];
    let mut push_in = vec![0.0f32; groups];
    for gi in 0..groups {
        let p = at[gi];
        let out = surface.hit(p, gdirs[gi], 0.0, OUT_REACH);
        if pelvis(p) {
            // along the normal: cloth just outside is a fight for the pixel, cloth behind the skin pokes through it
            let n = gdirs[gi];
            push_in[gi] = match out {
                Some(d) if d < margin => margin - d,
                Some(_) => 0.0,
                None => surface
                    .hit(p, [-n[0], -n[1], -n[2]], 0.0, IN_REACH)
                    .map_or(0.0, |d| d + margin),
            };
            continue;
        }
        let c = centre(p);
        let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
        let r = (dx * dx + dy * dy).sqrt();
        if r < 0.01 {
            continue;
        }
        let dir = [dx / r, dy / r, 0.0];
        if let Some(h) = surface.hit([c[0], c[1], p[2]], dir, 0.0, r + 0.06) {
            let allowed = (h - margin).max(0.0);
            if allowed < r {
                need[gi] = 1.0 - allowed / r;
            }
        }
    }
    let hidden: Vec<bool> = need.iter().map(|n| *n > HIDE).collect();
    let target: Vec<f32> = need.iter().map(|n| n.min(SQUEEZE)).collect();
    // the neighbours of each position
    let mut near: Vec<Vec<usize>> = vec![Vec::new(); groups];
    for t in indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if (a as usize) < count && (b as usize) < count {
                let (ga, gb) = (group[a as usize], group[b as usize]);
                if ga != gb {
                    near[ga].push(gb);
                    near[gb].push(ga);
                }
            }
        }
    }
    for list in near.iter_mut() {
        list.sort_unstable();
        list.dedup();
    }
    let mut depth = target.clone();
    let push_target: Vec<f32> = push_in.iter().map(|d| d.min(PUSH)).collect();
    let mut push_depth = push_target.clone();
    if target.iter().any(|d| *d > 0.0) {
        for _ in 0..FADE_ROUNDS {
            let before = depth.clone();
            for gi in 0..groups {
                if near[gi].is_empty() {
                    continue;
                }
                let mean = near[gi].iter().map(|&j| before[j]).sum::<f32>() / near[gi].len() as f32;
                depth[gi] = target[gi].max(FADE * mean);
            }
        }
    }
    if push_target.iter().any(|d| *d > 0.0) {
        for _ in 0..FADE_ROUNDS {
            let before = push_depth.clone();
            for gi in 0..groups {
                if near[gi].is_empty() {
                    continue;
                }
                let mean = near[gi].iter().map(|&j| before[j]).sum::<f32>() / near[gi].len() as f32;
                push_depth[gi] = push_target[gi].max(FADE * mean);
            }
        }
    }
    let mut moved = 0usize;
    let mut deepest = 0.0f32;
    let mut values: Vec<[f32; 4]> = Vec::with_capacity(count);
    for (p, gi) in positions.iter().zip(&group) {
        let d = if depth[*gi] < 0.002 { 0.0 } else { depth[*gi] };
        let by_n = if push_depth[*gi] < 0.0005 {
            0.0
        } else {
            push_depth[*gi]
        };
        let c = centre(*p);
        let n = gdirs[*gi];
        let to = if pelvis(*p) {
            [p[0] - n[0] * by_n, p[1] - n[1] * by_n, p[2] - n[2] * by_n]
        } else {
            [
                c[0] + (p[0] - c[0]) * (1.0 - d),
                c[1] + (p[1] - c[1]) * (1.0 - d),
                p[2],
            ]
        };
        let by = dot3(sub3(to, *p), sub3(to, *p)).sqrt();
        if by > 1e-5 {
            moved += 1;
            deepest = deepest.max(by);
        }
        values.push([to[0], to[1], to[2], 0.0]);
    }
    if moved > 0 {
        write_channel(skin, GrcFvfChannel::Position, &values)?;
    }
    if hidden.iter().any(|h| *h) {
        drop_hidden(skin, &group, &hidden);
    }
    Ok(Tucked {
        moved,
        total: count,
        deepest,
    })
}

/// Where in the donor's picture the underwear is drawn with skin instead: a plain stretch of the skin of the legs (the donor's own coordinates, before they move into the garment's box).
const SKIN_POINT: [f32; 2] = [0.19, 0.44];

/// Gives the skin of the underwear the texture of the skin: the vertices of `skin` that lie in the island of texture `u >= from[0]` or `v < from[1]` (the donor's underwear and the button of its
/// waistband in the stock atlas) take the coordinates of a plain stretch of skin. ⚠ Dropping the underwear instead leaves the pelvis open, and below a short skirt or an open side the inside of the legs
/// shows (black bars; in the game, nothing at all). Returns how many vertices were turned to skin.
fn skin_island(skin: &mut GrmGeometry<Gen8>, from: [f32; 2]) -> std::result::Result<usize, String> {
    let inside = {
        let reader =
            VertexReader::new(&*skin.vertex_buffer).ok_or("the vertices cannot be read")?;
        (0..reader.count())
            .map(|v| {
                reader
                    .get(GrcFvfChannel::Tex0, v)
                    .map(|x| usize::from(x[0] >= from[0] || x[1] < from[1]))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("a vertex has no texture coordinates")?
            .into_iter()
            .sum::<usize>()
    };
    if inside > 0 {
        edit_uv(skin, |uv| {
            if uv[0] >= from[0] || uv[1] < from[1] {
                SKIN_POINT
            } else {
                uv
            }
        })?;
    }
    Ok(inside)
}

/// What a graft is asked to do.
#[derive(Default)]
pub struct GraftOptions<'a> {
    /// The donor's shaders that sample a texture whose name starts with this are moved; empty: every shader of the donor.
    pub prefix: &'a str,
    /// Refuse a garment that already samples a texture whose name starts with this (default: `prefix`).
    pub marker: Option<&'a str>,
    /// `[x0, y0, w, h]`: the grafted geometries' first texture coordinates `(u, v)` become `(x0 + u*w, y0 + v*h)`.
    pub skin_uv: Option<[f32; 4]>,
    /// `[su, sv, ou, ov]`: the garment's own geometries' first texture coordinates `(u, v)` become `(u*su + ou, v*sv + ov)` (a texture made wider).
    pub target_uv: Option<[f32; 4]>,
    /// Textures put into the garment's dictionary in place of the donor's own of the same name (the skin's normal and specular maps, made neutral).
    pub textures: Vec<GrcTexture>,
    /// Pull the skin vertices that poke through the garment back inside it.
    pub tuck: bool,
    /// With `tuck`: the skin triangles whose texture `u` is at least `[0]` or whose `v` is under `[1]` (the donor's underwear) are dropped where the garment covers them.
    pub hide_island: Option<[f32; 2]>,
    /// How far under the cloth the skin is kept, metres (default 3 mm): more where the garment's folds and hem are finer than the skin's triangles.
    pub margin: Option<f32>,
}

/// What a graft did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraftReport {
    /// The donor shaders moved into the garment's shader group.
    pub shaders: usize,
    /// The donor's own textures (the skin's normal and specular maps) put into the garment's dictionary.
    pub carried: Vec<String>,
    /// `level 0: 1 geometry, 7710 triangles`, one per level that got skin.
    pub levels: Vec<String>,
    /// Levels the donor has skin at and the garment has no model for.
    pub skipped: Vec<String>,
}

impl GraftReport {
    /// The report as lines a person reads.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "skin grafted: {} shader(s) moved with {}, {}",
            self.shaders,
            if self.carried.is_empty() {
                "no texture".to_string()
            } else {
                self.carried.join(", ")
            },
            if self.levels.is_empty() {
                "no level".to_string()
            } else {
                self.levels.join("; ")
            }
        )];
        lines.extend(self.skipped.iter().map(|s| format!("not grafted: {s}")));
        lines
    }
}

fn refuse<T>(context: impl Into<String>) -> Result<T> {
    Err(IoError::InvalidData {
        context: context.into(),
    })
}

/// The shaders of `drawable` that sample a texture whose name starts with `prefix`, whatever the case: `mp_fm_skin_f_lo` is the game's skin of a woman's legs.
pub fn skin_shaders(drawable: &RmcDrawable<Gen8>, prefix: &str) -> Vec<usize> {
    let Some(group) = drawable.shader_group.as_deref() else {
        return Vec::new();
    };
    // ⚠ Several names, comma apart: the skin of the coarser levels is drawn with a simpler shader that binds the donor's diffuse and no normal map, so the marker of the first level
    // alone would leave their skin treated as cloth (never tucked in, its underwear kept).
    let prefixes: Vec<String> = prefix
        .split(',')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    group
        .shaders
        .iter()
        .enumerate()
        .filter(|(_, shader)| {
            shader.textures().any(|slot| {
                group.slot_name(slot).is_some_and(|name| {
                    let name = name.to_ascii_lowercase();
                    prefixes.iter().any(|p| name.starts_with(p))
                })
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// The skin shaders of an already grafted garment at every level: the ones that bind the marker, and the shaders that only the coarser levels use (the donor's coarser levels are drawn with a simpler
/// shader that binds its diffuse and no normal map, which a marker cannot name; no geometry of the first level uses it).
fn skin_shaders_all_levels(drawable: &RmcDrawable<Gen8>, marker: &str) -> Vec<usize> {
    let mut skin = skin_shaders(drawable, marker);
    if skin.is_empty() {
        return skin;
    }
    let first: HashSet<usize> = drawable.lod_group.lods[0]
        .models
        .iter()
        .flat_map(|m| m.shader_indices.iter().map(|s| usize::from(*s)))
        .collect();
    for lod in drawable.lod_group.lods.iter().skip(1).take(3) {
        for model in lod.models.iter() {
            for s in model.shader_indices.iter().map(|s| usize::from(*s)) {
                if !first.contains(&s) && !skin.contains(&s) {
                    skin.push(s);
                }
            }
        }
    }
    skin
}

/// Moves the skin geometries of `donor` (the ones that draw with a skin shader, at every level) into `target`, with their shaders.
///
/// Refuses, and leaves both as they were, a donor with no skin shader, a target that already draws with one, a donor whose dictionary lacks a texture its skin embeds and a
/// target whose first level is also held a second time.
pub fn graft_skin(
    target: &mut RmcDrawable<Gen8>,
    donor: &mut RmcDrawable<Gen8>,
    options: &GraftOptions,
) -> Result<GraftReport> {
    let prefix = options.prefix;
    let skin: Vec<usize> = if prefix.is_empty() {
        donor
            .shader_group
            .as_deref()
            .map_or_else(Vec::new, |g| (0..g.shaders.len()).collect())
    } else {
        skin_shaders(donor, prefix)
    };
    if skin.is_empty() {
        return refuse(format!(
            "the donor draws with no texture whose name starts with {prefix}"
        ));
    }
    let marker = options.marker.unwrap_or(prefix);
    if !marker.is_empty() && !skin_shaders(target, marker).is_empty() {
        return refuse("the drawable already draws with a skin texture");
    }
    if matches!(&target.container, Shared::Own(Some(_))) {
        return refuse(
            "the drawable holds its first level twice, and a graft would have to feed both",
        );
    }
    if target.shader_group.is_none() {
        return refuse("the drawable has no shader group");
    }
    // ⚠ Some packs' models carry two textures under one key (0, where the maker's tool wrote none) and bind it: the writer refuses such a dictionary. The first keeps the key, which the shaders
    // keep resolving to as they did, and each of the others goes back under the key of its own name.
    if let Some(dictionary) = target
        .shader_group
        .as_deref_mut()
        .and_then(|g| g.texture_dictionary.as_deref_mut())
    {
        let mut seen = HashSet::new();
        let mut again = Vec::new();
        let mut at = 0;
        while at < dictionary.codes.len() {
            if seen.insert(dictionary.codes[at]) {
                at += 1;
            } else {
                dictionary.codes.remove(at);
                if let Some(texture) = dictionary.entries.remove(at) {
                    again.push(*texture);
                }
            }
        }
        for texture in again {
            dictionary.insert(texture);
        }
    }
    // The textures a skin shader carries in the donor's own dictionary (the skin's normal and specular maps) travel with it: each is taken out of
    // the donor (its shaders then bind it by name) and put into the garment's dictionary after the shaders. A key naming no entry of the donor's
    // dictionary is a reference by hash, which the game resolves the same way for the garment.
    let mut carried = Vec::new();
    {
        let group = donor
            .shader_group
            .as_deref_mut()
            .expect("a skin shader came from it");
        let mut names: Vec<String> = Vec::new();
        for &s in &skin {
            for slot in group.shaders[s].textures() {
                if matches!(slot, or_io::gta5::TextureSlot::Embedded(_)) {
                    if let Some(name) = group.slot_name(slot) {
                        if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                            names.push(name.to_string());
                        }
                    }
                }
            }
        }
        for name in names {
            match group.unembed(&name) {
                Some(texture) => {
                    if !options
                        .textures
                        .iter()
                        .any(|t| t.name.eq_ignore_ascii_case(&name))
                    {
                        carried.push((name, texture));
                    }
                }
                None => return refuse(format!("the donor's dictionary holds no {name}")),
            }
        }
    }

    if let Some([su, sv, ou, ov]) = options.target_uv {
        for (level, lod) in target.lod_group.lods.iter_mut().enumerate().take(4) {
            for (m, model) in lod.models.iter_mut().enumerate() {
                for (g, geometry) in model.geometries.iter_mut().enumerate() {
                    edit_uv(geometry, |[u, v]| [u * su + ou, v * sv + ov])
                        .or_else(|why| refuse(format!("level {level} model {m} geometry {g}: the garment's texture coordinates could not be moved: {why}")))?;
                }
            }
        }
    }

    // which donor geometries draw with skin, per level and model
    let mut picks: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    for (level, lod) in donor.lod_group.lods.iter().enumerate().take(4) {
        for (m, model) in lod.models.iter().enumerate() {
            let geometries: Vec<usize> = model
                .shader_indices
                .iter()
                .enumerate()
                .filter(|(_, s)| skin.contains(&usize::from(**s)))
                .map(|(g, _)| g)
                .collect();
            if !geometries.is_empty() {
                picks.push((level, m, geometries));
            }
        }
    }

    // the shaders go first, so that a geometry names the index it will have in the target
    let mut moved = BTreeMap::new();
    {
        let group = donor.shader_group.as_deref_mut().expect("checked above");
        for &s in skin.iter().rev() {
            moved.insert(s, group.shaders.remove(s));
        }
    }
    let mut index_of: BTreeMap<usize, u16> = BTreeMap::new();
    {
        let group = target.shader_group.as_deref_mut().expect("checked above");
        for (s, shader) in moved {
            let Ok(at) = u16::try_from(group.shaders.len()) else {
                return refuse("the shader group holds more shaders than a geometry can name");
            };
            index_of.insert(s, at);
            group.shaders.push(shader);
        }
    }

    let mut report = GraftReport {
        shaders: index_of.len(),
        ..GraftReport::default()
    };
    {
        let group = target.shader_group.as_deref_mut().expect("checked above");
        for texture in options.textures.iter().cloned() {
            let name = texture.name.clone();
            group.embed(texture);
            report.carried.push(format!("{name} (made neutral)"));
        }
        for (name, texture) in carried {
            // ⚠ A garment that carries a texture of that name keeps its own: the skin binds it, and is not drawn with the donor's.
            let held = group.texture_dictionary.as_deref().is_some_and(|d| {
                let code = or_io::hash::string_hash_str(&name);
                d.codes.contains(&code)
            });
            if held {
                report.skipped.push(format!(
                    "{name}: the garment already carries one, the skin uses it"
                ));
            } else {
                group.embed(texture);
                report.carried.push(name);
            }
        }
    }
    for (level, m, geometries) in picks {
        let name = ["High", "Medium", "Low", "Very low"][level];
        let Some(into) = target.lod_group.lods[level].models.get_mut(m) else {
            report
                .skipped
                .push(format!("{name} level: the drawable has no model {m} there"));
            continue;
        };
        let from = &mut donor.lod_group.lods[level].models[m];
        let mut taken = Vec::new();
        for &g in geometries.iter().rev() {
            let geometry = from.geometries.remove(g);
            let shader = from.shader_indices.remove(g);
            taken.push((geometry, usize::from(shader)));
        }
        taken.reverse();
        let mut triangles = 0usize;
        let count = taken.len();
        // the garment as it is before the skin is added: what the skin must stay inside of
        let surface = options
            .tuck
            .then(|| Surface::of(into.geometries.iter().map(|g| &**g)));
        let (mut pulled, mut vertices, mut deepest, mut dropped) = (0usize, 0usize, 0.0f32, 0usize);
        for (mut geometry, shader) in taken {
            if let Some(surface) = &surface {
                if let Err(why) = refine(&mut geometry, surface, 5) {
                    report
                        .skipped
                        .push(format!("{name} level: the skin was not refined: {why}"));
                }
                match tuck_in(&mut geometry, surface, options.margin.unwrap_or(MARGIN)) {
                    Ok(t) => {
                        pulled += t.moved;
                        vertices += t.total;
                        deepest = deepest.max(t.deepest);
                        // before the texture coordinates move into the garment's box, while the island is still where the donor drew it
                        if let Some(from) = options.hide_island {
                            match skin_island(&mut geometry, from) {
                                Ok(n) => dropped += n,
                                Err(why) => report.skipped.push(format!(
                                    "{name} level: the underwear was not turned to skin: {why}"
                                )),
                            }
                        }
                    }
                    Err(why) => report
                        .skipped
                        .push(format!("{name} level: the skin was not tucked in: {why}")),
                }
            }
            if let Some([x0, y0, w, h]) = options.skin_uv {
                if let Err(why) = edit_uv(&mut geometry, |[u, v]| [x0 + u * w, y0 + v * h]) {
                    return refuse(format!(
                        "{name} level: the skin's texture coordinates could not be moved into the box {:?}: {why}",
                        [x0, y0, w, h]
                    ));
                }
            }
            triangles += geometry.primitive_count() as usize;
            into.geometries.push(geometry);
            into.shader_indices.push(index_of[&shader]);
        }
        if !recompute_aabbs(into) {
            return refuse(format!(
                "{name} level: the boxes of the geometries could not be read back"
            ));
        }
        let tucked = if surface.is_some() {
            format!(", {pulled} of {vertices} skin vertices pulled in (deepest {:.1} mm), {dropped} underwear vertices turned to skin", deepest * 1000.0)
        } else {
            String::new()
        };
        report.levels.push(format!(
            "{name}: {count} geometry, {triangles} triangles{tucked}"
        ));
    }
    Ok(report)
}

/// Pushes the skin geometries of an already grafted garment (the ones whose shaders bind a texture that starts with `marker`) under its cloth again, `margin` metres deep; the cloth is every other geometry of the level.
/// Returns one line a person reads per level.
pub fn retuck(target: &mut RmcDrawable<Gen8>, marker: &str, margin: f32) -> Result<Vec<String>> {
    let skin = skin_shaders_all_levels(target, marker);
    if skin.is_empty() {
        return refuse(format!(
            "the drawable draws with no texture whose name starts with {marker}"
        ));
    }
    let mut lines = Vec::new();
    for (level, lod) in target.lod_group.lods.iter_mut().enumerate().take(4) {
        for model in lod.models.iter_mut() {
            let is_skin: Vec<bool> = model
                .shader_indices
                .iter()
                .map(|s| skin.contains(&usize::from(*s)))
                .collect();
            if !is_skin.iter().any(|b| *b) {
                continue;
            }
            let surface = Surface::of(
                model
                    .geometries
                    .iter()
                    .zip(&is_skin)
                    .filter(|(_, s)| !**s)
                    .map(|(g, _)| &**g),
            );
            let (mut pulled, mut vertices, mut deepest, mut added) =
                (0usize, 0usize, 0.0f32, 0usize);
            for (geometry, skin_part) in model.geometries.iter_mut().zip(&is_skin) {
                if !*skin_part {
                    continue;
                }
                match refine(geometry, &surface, 5).and_then(|n| {
                    added += n;
                    tuck_in(geometry, &surface, margin)
                }) {
                    Ok(t) => {
                        pulled += t.moved;
                        vertices += t.total;
                        deepest = deepest.max(t.deepest);
                    }
                    Err(why) => {
                        return refuse(format!("level {level}: the skin was not tucked in: {why}"))
                    }
                }
            }
            if !recompute_aabbs(model) {
                return refuse(format!(
                    "level {level}: the boxes of the geometries could not be read back"
                ));
            }
            lines.push(format!("level {level}: {pulled} of {vertices} skin vertices pulled in (deepest {:.1} mm), {added} vertices added where the skin meets the cloth", deepest * 1000.0));
            let (mut points, mut poking, mut lo, mut hi) = (0usize, 0usize, f32::MAX, f32::MIN);
            for (geometry, skin_part) in model.geometries.iter().zip(&is_skin) {
                if *skin_part {
                    let (p, k, l, h) = probe(geometry, &surface);
                    points += p;
                    poking += k;
                    lo = lo.min(l);
                    hi = hi.max(h);
                }
            }
            lines.push(format!("level {level}: {poking} of {points} points of the skin still show through the cloth{}", if poking > 0 { format!(" (z {lo:.2} to {hi:.2})") } else { String::new() }));
        }
    }
    Ok(lines)
}

/// How many points of the skin triangles (a point every ~5 mm) lie in front of the cloth, with the cloth behind them within 6 cm: skin that shows through. Returns (points, poking, lowest and highest `z` of the poking ones).
fn probe(skin: &GrmGeometry<Gen8>, surface: &Surface) -> (usize, usize, f32, f32) {
    let mesh = skin.mesh();
    if mesh.normals.len() != mesh.positions.len() {
        return (0, 0, 0.0, 0.0);
    }
    let (mut points, mut poking, mut lo, mut hi) = (0usize, 0usize, f32::MAX, f32::MIN);
    for t in mesh.indices.as_chunks::<3>().0 {
        let (Some(&a), Some(&b), Some(&c), Some(&na), Some(&nb), Some(&nc)) = (
            mesh.positions.get(t[0] as usize),
            mesh.positions.get(t[1] as usize),
            mesh.positions.get(t[2] as usize),
            mesh.normals.get(t[0] as usize),
            mesh.normals.get(t[1] as usize),
            mesh.normals.get(t[2] as usize),
        ) else {
            continue;
        };
        let edge = dot3(sub3(b, a), sub3(b, a))
            .sqrt()
            .max(dot3(sub3(c, a), sub3(c, a)).sqrt())
            .max(dot3(sub3(c, b), sub3(c, b)).sqrt());
        let n = ((edge / 0.005).ceil() as usize).clamp(1, 12);
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                let w = 1.0 - u - v;
                let p = [
                    a[0] * w + b[0] * u + c[0] * v,
                    a[1] * w + b[1] * u + c[1] * v,
                    a[2] * w + b[2] * u + c[2] * v,
                ];
                let q = [
                    na[0] * w + nb[0] * u + nc[0] * v,
                    na[1] * w + nb[1] * u + nc[1] * v,
                    na[2] * w + nb[2] * u + nc[2] * v,
                ];
                let len = dot3(q, q).sqrt();
                if len < 1e-6 {
                    continue;
                }
                let q = [q[0] / len, q[1] / len, q[2] / len];
                points += 1;
                if surface.hit(p, q, 0.0, OUT_REACH).is_none()
                    && surface
                        .hit(p, [-q[0], -q[1], -q[2]], 0.0, IN_REACH)
                        .is_some()
                {
                    poking += 1;
                    lo = lo.min(p[2]);
                    hi = hi.max(p[2]);
                }
            }
        }
    }
    (points, poking, lo, hi)
}

/// Turns the underwear (the vertices of the island `island` = [u, v] of the donor's atlas) of the skin of a garment that was already grafted into skin, whatever the garment covers.
/// The skin's coordinates were moved into a box of the garment's picture; the box is read back from the highest level's skin and the island is looked for in the donor's own coordinates.
pub fn drop_underwear(
    target: &mut RmcDrawable<Gen8>,
    marker: &str,
    island: [f32; 2],
) -> Result<Vec<String>> {
    let skin = skin_shaders_all_levels(target, marker);
    if skin.is_empty() {
        return refuse(format!(
            "the drawable draws with no texture whose name starts with {marker}"
        ));
    }
    let mut lines = Vec::new();
    let mut bounds: Option<([f32; 2], [f32; 2])> = None;
    for (level, lod) in target.lod_group.lods.iter_mut().enumerate().take(4) {
        for model in lod.models.iter_mut() {
            let is_skin: Vec<bool> = model
                .shader_indices
                .iter()
                .map(|s| skin.contains(&usize::from(*s)))
                .collect();
            let mut turned = 0usize;
            for (geometry, skin_part) in model.geometries.iter_mut().zip(&is_skin) {
                if !*skin_part {
                    continue;
                }
                let uvs: Vec<[f32; 2]> = {
                    let reader = VertexReader::new(&*geometry.vertex_buffer).ok_or_else(|| {
                        IoError::InvalidData {
                            context: "the skin's vertices cannot be read".into(),
                        }
                    })?;
                    (0..reader.count())
                        .map(|v| reader.get(GrcFvfChannel::Tex0, v).map(|x| [x[0], x[1]]))
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| IoError::InvalidData {
                            context: "a skin vertex has no texture coordinates".into(),
                        })?
                };
                if bounds.is_none() {
                    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                    for uv in &uvs {
                        for k in 0..2 {
                            lo[k] = lo[k].min(uv[k]);
                            hi[k] = hi[k].max(uv[k]);
                        }
                    }
                    bounds = Some((lo, hi));
                }
                let (lo, hi) = bounds.unwrap_or(([0.0; 2], [1.0; 2]));
                // the donor's coordinates ran from about 0.01 to 0.99
                let (su, sv) = ((hi[0] - lo[0]).max(1e-6), (hi[1] - lo[1]).max(1e-6));
                let donor = |uv: [f32; 2]| {
                    [
                        0.01 + 0.98 * (uv[0] - lo[0]) / su,
                        0.01 + 0.98 * (uv[1] - lo[1]) / sv,
                    ]
                };
                let skin_at = [
                    lo[0] + (SKIN_POINT[0] - 0.01) / 0.98 * su,
                    lo[1] + (SKIN_POINT[1] - 0.01) / 0.98 * sv,
                ];
                turned += uvs
                    .iter()
                    .filter(|uv| {
                        let d = donor(**uv);
                        d[0] >= island[0] || d[1] < island[1]
                    })
                    .count();
                edit_uv(geometry, |uv| {
                    let d = donor(uv);
                    if d[0] >= island[0] || d[1] < island[1] {
                        skin_at
                    } else {
                        uv
                    }
                })
                .map_err(|why| IoError::InvalidData { context: why })?;
            }
            lines.push(format!(
                "level {level}: {turned} underwear vertices turned to skin"
            ));
        }
    }
    Ok(lines)
}

/// Takes the skin out of an already grafted garment: its geometries at every level, and the shaders they draw with (the last ones of the group, where a graft puts them). Returns the box of the
/// lowest and highest texture coordinates of the skin, `[u0, v0, u1, v1]` (the donor's own extents, moved into the garment's box: the caller knows them), and a line per level.
pub fn unskin(
    target: &mut RmcDrawable<Gen8>,
    marker: &str,
    island: Option<[f32; 2]>,
) -> Result<([f32; 6], Vec<String>)> {
    let skin = skin_shaders_all_levels(target, marker);
    if skin.is_empty() {
        return refuse(format!(
            "the drawable draws with no texture whose name starts with {marker}"
        ));
    }
    if matches!(&target.container, Shared::Own(Some(_))) {
        return refuse("the drawable holds its first level twice");
    }
    let base = skin.iter().copied().min().unwrap_or(0);
    // a shader after the first skin one that no geometry draws with is a donor shader the garment had no level for: it goes with the others
    let after_base_is_skin = target.lod_group.lods.iter().all(|l| {
        l.models.iter().all(|m| {
            m.shader_indices
                .iter()
                .map(|s| usize::from(*s))
                .all(|s| s < base || skin.contains(&s))
        })
    });
    if !after_base_is_skin {
        return refuse("a geometry that is not skin draws with a shader after the first skin one");
    }
    // the box, from the first skin geometry of the highest level (its vertices, the ones the underwear left included)
    #[allow(clippy::type_complexity)]
    let mut bounds: Option<([f32; 2], [f32; 2], Option<[f32; 2]>)> = None;
    for model in target.lod_group.lods[0].models.iter() {
        for (geometry, shader) in model.geometries.iter().zip(&model.shader_indices) {
            if usize::from(*shader) < base || bounds.is_some() {
                continue;
            }
            let reader = VertexReader::new(&*geometry.vertex_buffer).ok_or_else(|| {
                IoError::InvalidData {
                    context: "the skin's vertices cannot be read".into(),
                }
            })?;
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for v in 0..reader.count() {
                let x = reader
                    .get(GrcFvfChannel::Tex0, v)
                    .ok_or_else(|| IoError::InvalidData {
                        context: "a skin vertex has no texture coordinates".into(),
                    })?;
                for k in 0..2 {
                    lo[k] = lo[k].min(x[k]);
                    hi[k] = hi[k].max(x[k]);
                }
            }
            // ⚠ A skin whose underwear was turned to skin has its coordinates gathered in one place, and the extent of the donor's picture is no longer in them: it cannot be read back
            let mut same: HashMap<(u32, u32), usize> = HashMap::new();
            for &i in geometry.index_buffer.indices.iter() {
                if let Some(x) = reader.get(GrcFvfChannel::Tex0, usize::from(i)) {
                    *same.entry((x[0].to_bits(), x[1].to_bits())).or_insert(0) += 1;
                }
            }
            // the place they were gathered at (the donor's plain stretch of skin, moved into the box) tells the box where the extents no longer do
            let common = same
                .iter()
                .filter(|(_, n)| **n >= 60)
                .max_by_key(|(_, n)| **n)
                .map(|(k, _)| [f32::from_bits(k.0), f32::from_bits(k.1)]);
            // ⚠ A graft of ours has its underwear dropped or turned to skin: where triangles still draw it, the skin is the pack author's own (it binds the stock marker too) and is left alone
            if let (Some(island), None) = (island, common) {
                let (su, sv) = ((hi[0] - lo[0]).max(1e-6), (hi[1] - lo[1]).max(1e-6));
                let drawn = geometry
                    .index_buffer
                    .indices
                    .iter()
                    .filter(|&&i| {
                        reader
                            .get(GrcFvfChannel::Tex0, usize::from(i))
                            .is_some_and(|x| {
                                let d = [
                                    0.01 + 0.98 * (x[0] - lo[0]) / su,
                                    0.01 + 0.98 * (x[1] - lo[1]) / sv,
                                ];
                                d[0] >= island[0] || d[1] < island[1]
                            })
                    })
                    .count();
                if drawn > 0 {
                    return refuse(
                        "the underwear is still drawn: this skin is not a graft of ours",
                    );
                }
            }
            bounds = Some((lo, hi, common));
        }
    }
    let Some((lo, hi, common)) = bounds else {
        return refuse("the first level holds no skin geometry");
    };
    let box_ = [
        lo[0],
        lo[1],
        hi[0],
        hi[1],
        common.map_or(-1.0, |c| c[0]),
        common.map_or(-1.0, |c| c[1]),
    ];
    let mut lines = Vec::new();
    for (level, lod) in target.lod_group.lods.iter_mut().enumerate().take(4) {
        for model in lod.models.iter_mut() {
            let mut taken = 0usize;
            let mut at = 0;
            while at < model.geometries.len() {
                if usize::from(model.shader_indices[at]) >= base {
                    model.geometries.remove(at);
                    model.shader_indices.remove(at);
                    taken += 1;
                } else {
                    at += 1;
                }
            }
            if taken > 0 && !recompute_aabbs(model) {
                return refuse(format!(
                    "level {level}: the boxes of the geometries could not be read back"
                ));
            }
            lines.push(format!("level {level}: {taken} skin geometries taken out"));
        }
    }
    if let Some(group) = target.shader_group.as_deref_mut() {
        group.shaders.truncate(base);
    }
    Ok((box_, lines))
}

/// ⚠ A ped component fills the transparent texels of its picture with the wearer's skin tone (the alpha is a skin mask), so a garment made with holes in its texture (a torn hem, a lace) draws a flat
/// skin-coloured surface there instead of letting the legs show. A cutout material draws only where the texel is opaque: the shaders of the garment that the donor `stock` material has the effect of
/// (the same shader, drawn in the cutout bucket by a stock mask) take its draw bucket and its preset. The skin shaders (every level) are left as they are. Returns a line per shader changed.
pub fn cutout(
    target: &mut RmcDrawable<Gen8>,
    stock: &RmcDrawable<Gen8>,
    marker: &str,
) -> Result<Vec<String>> {
    let skin = skin_shaders_all_levels(target, marker);
    let Some(from) = stock.shader_group.as_deref() else {
        return refuse("the stock drawable has no shader group");
    };
    let Some(model) = from.shaders.iter().find(|s| s.draw_bucket != 0) else {
        return refuse(
            "the stock drawable has no material drawn in another bucket than the opaque one",
        );
    };
    let Some(group) = target.shader_group.as_deref_mut() else {
        return refuse("the garment has no shader group");
    };
    let mut lines = Vec::new();
    for (i, shader) in group.shaders.iter_mut().enumerate() {
        if skin.contains(&i) || shader.basis_hash != model.basis_hash {
            continue;
        }
        let was = shader.draw_bucket;
        shader.draw_bucket = model.draw_bucket;
        shader.draw_bucket_mask = model.draw_bucket_mask;
        shader.material_hash = model.material_hash;
        shader.material_hash_code = model.material_hash_code;
        lines.push(format!("shader {i}: bucket {was} -> {}", model.draw_bucket));
    }
    if lines.is_empty() {
        return refuse("no shader of the garment is the stock cutout material's effect");
    }
    Ok(lines)
}

/// For each triangle of the skin: whether a point of it (every ~5 mm) lies in front of the cloth with the cloth behind it, within 6 cm.
fn poking_triangles(skin: &GrmGeometry<Gen8>, surface: &Surface) -> Vec<bool> {
    let mesh = skin.mesh();
    let mut flags = vec![false; mesh.indices.len() / 3];
    if mesh.normals.len() != mesh.positions.len() {
        return flags;
    }
    for (ti, t) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let (Some(&a), Some(&b), Some(&c), Some(&na), Some(&nb), Some(&nc)) = (
            mesh.positions.get(t[0] as usize),
            mesh.positions.get(t[1] as usize),
            mesh.positions.get(t[2] as usize),
            mesh.normals.get(t[0] as usize),
            mesh.normals.get(t[1] as usize),
            mesh.normals.get(t[2] as usize),
        ) else {
            continue;
        };
        let edge = dot3(sub3(b, a), sub3(b, a))
            .sqrt()
            .max(dot3(sub3(c, a), sub3(c, a)).sqrt())
            .max(dot3(sub3(c, b), sub3(c, b)).sqrt());
        if edge < 0.006 {
            continue;
        }
        let n = ((edge / 0.005).ceil() as usize).clamp(1, 12);
        'triangle: for i in 0..=n {
            for j in 0..=(n - i) {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                let w = 1.0 - u - v;
                let p = [
                    a[0] * w + b[0] * u + c[0] * v,
                    a[1] * w + b[1] * u + c[1] * v,
                    a[2] * w + b[2] * u + c[2] * v,
                ];
                let q = [
                    na[0] * w + nb[0] * u + nc[0] * v,
                    na[1] * w + nb[1] * u + nc[1] * v,
                    na[2] * w + nb[2] * u + nc[2] * v,
                ];
                let len = dot3(q, q).sqrt();
                if len < 1e-6 {
                    continue;
                }
                let q = [q[0] / len, q[1] / len, q[2] / len];
                if surface.hit(p, q, 0.0, OUT_REACH).is_none()
                    && surface
                        .hit(p, [-q[0], -q[1], -q[2]], 0.0, IN_REACH)
                        .is_some()
                {
                    flags[ti] = true;
                    break 'triangle;
                }
            }
        }
    }
    flags
}
