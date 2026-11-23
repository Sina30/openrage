//! A conservative decimator: edge collapses that keep every surviving vertex exactly as it was.
//!
//! ⚠ **Nothing is invented.** A collapse moves one position onto another and gives the triangles around the first the
//! vertices of the second, so every vertex of the result is a vertex of the source with all its channels (normal, tangent,
//! texture coordinates, colours, bones and weights) untouched, and the channel set, which a material's vertex
//! declaration names, is the source's.
//!
//! ⚠ **Seams and borders hold.** Vertices are welded by position only to find the surface; a collapse is allowed when
//! every vertex the removed position has can be given a vertex of the kept one by a triangle they share, so a texture
//! seam, a hard edge or a skin boundary never tears, and a border (a hem, a cuff) only slides along itself. A collapse
//! that flips a triangle or would make the surface non-manifold is refused, and what the surface is cut by is priced
//! (positions by the sum of squared distances to the planes of the faces, the gap in texture coordinates, normals and
//! weights between the vertices a collapse joins).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::scene::Primitive;

/// A geometry of this many triangles or fewer is copied as it is: there is no level of detail worth making of it.
pub const KEEP_WHOLE: usize = 8;

/// No geometry is cut below this many triangles, so a low level of detail is never a handful of slivers.
pub const FLOOR: usize = 8;

/// The weight of a border edge's plane, so that a hem only slides along itself.
const BORDER: f64 = 100.0;

/// The smallest cosine between a triangle's normal before a collapse and after it.
const FLIP: f64 = 0.35;

/// The most neighbours a position may have after a collapse, so that no position grows into a fan of slivers.
const VALENCE: usize = 16;

/// What a unit of deviation in the texture coordinates, the normals and the skin costs against a unit of the box.
const PRICE_UV: f64 = 1.0;
const PRICE_NORMAL: f64 = 0.25;
const PRICE_SKIN: f64 = 0.25;

type V3 = [f64; 3];

/// How a geometry came out.
#[derive(Debug, Clone)]
pub struct Decimated {
    pub primitive: Primitive,
    pub triangles_before: usize,
    pub triangles_after: usize,
    /// The triangles asked for: the ratio of the source's, rounded up, and no fewer than [`FLOOR`].
    pub target: usize,
    pub vertices_before: usize,
    pub vertices_after: usize,
    /// Whether the result holds no more than the target; false when no collapse that is safe was left.
    pub reached: bool,
    /// The largest root-mean-square distance, in the geometry's own units, from where a collapse put a position to the planes of the faces it joined.
    pub error: f32,
}

/// The triangles `ratio` of `triangles` is: rounded up, and never below [`FLOOR`] nor above what there is.
pub fn target_of(triangles: usize, ratio: f32) -> usize {
    let wanted = (triangles as f64 * f64::from(ratio.clamp(0.0, 1.0))).ceil() as usize;
    wanted.max(FLOOR).min(triangles)
}

/// `source` with about `ratio` of its triangles: no fewer than [`FLOOR`], and fewer than the target only by the last collapse's second triangle.
pub fn decimate(source: &Primitive, ratio: f32) -> Decimated {
    let triangles = source.indices.len() / 3;
    let vertices = source.positions.len();
    let target = target_of(triangles, ratio);
    let whole = |reached| Decimated {
        primitive: source.clone(),
        triangles_before: triangles,
        triangles_after: triangles,
        target,
        vertices_before: vertices,
        vertices_after: vertices,
        reached,
        error: 0.0,
    };
    if triangles <= KEEP_WHOLE || triangles <= target || !source.indices.len().is_multiple_of(3) {
        return whole(triangles <= target);
    }
    if source.indices.iter().any(|i| *i as usize >= vertices) {
        return whole(false);
    }
    let mut mesh = Collapsing::new(source);
    // A geometry whose every triangle has no area has nothing to cut down to.
    if mesh.live == 0 {
        return whole(false);
    }
    let error = mesh.run(target);
    let kept = mesh.live;
    let (primitive, vertices_after) = mesh.result();
    Decimated {
        primitive,
        triangles_before: triangles,
        triangles_after: kept,
        target,
        vertices_before: vertices,
        vertices_after,
        reached: kept <= target,
        error: error as f32,
    }
}

/// The sum of squared distances to a set of planes, as a symmetric 4 by 4 matrix.
#[derive(Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    fn plane(n: V3, d: f64, w: f64) -> Self {
        Self([
            w * n[0] * n[0],
            w * n[0] * n[1],
            w * n[0] * n[2],
            w * n[0] * d,
            w * n[1] * n[1],
            w * n[1] * n[2],
            w * n[1] * d,
            w * n[2] * n[2],
            w * n[2] * d,
            w * d * d,
        ])
    }

    fn add(&mut self, other: &Self) {
        for (a, b) in self.0.iter_mut().zip(other.0) {
            *a += b;
        }
    }

    fn eval(&self, p: V3) -> f64 {
        let q = &self.0;
        let [x, y, z] = p;
        q[0] * x * x
            + 2.0 * q[1] * x * y
            + 2.0 * q[2] * x * z
            + 2.0 * q[3] * x
            + q[4] * y * y
            + 2.0 * q[5] * y * z
            + 2.0 * q[6] * y
            + q[7] * z * z
            + 2.0 * q[8] * z
            + q[9]
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// The surface being collapsed.
struct Collapsing<'a> {
    source: &'a Primitive,
    /// The position each vertex has, numbered.
    node: Vec<u32>,
    at: Vec<V3>,
    tris: Vec<[u32; 3]>,
    alive: Vec<bool>,
    /// The triangles that touch each position; they may hold dead ones.
    around: Vec<Vec<u32>>,
    quadric: Vec<Quadric>,
    gone: Vec<bool>,
    live: usize,
    /// The squared diagonal of the box, which the geometric price is divided by.
    scale: f64,
}

/// The vertices a position gives up and the vertex of the kept position each is given.
type Given = Vec<(u32, u32)>;

/// A collapse waiting its turn: its price when it was offered, the position it gives up and the one it goes to.
type Entry = Reverse<(u64, u32, u32)>;

impl<'a> Collapsing<'a> {
    fn new(source: &'a Primitive) -> Self {
        let mut ids: HashMap<[u32; 3], u32> = HashMap::new();
        let mut node = Vec::with_capacity(source.positions.len());
        let mut at: Vec<V3> = Vec::new();
        for p in &source.positions {
            // -0.0 and 0.0 are one place.
            let key = p.map(|c| if c == 0.0 { 0 } else { c.to_bits() });
            let next = ids.len() as u32;
            let id = *ids.entry(key).or_insert(next);
            if id == next {
                at.push(p.map(f64::from));
            }
            node.push(id);
        }
        let tris: Vec<[u32; 3]> = source
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        let mut alive = vec![true; tris.len()];
        let mut around = vec![Vec::new(); at.len()];
        for (t, tri) in tris.iter().enumerate() {
            let n = tri.map(|v| node[v as usize]);
            // A triangle with two corners at one place has no area, and is gone before anything is asked of it.
            if n[0] == n[1] || n[1] == n[2] || n[0] == n[2] {
                alive[t] = false;
                continue;
            }
            for place in n {
                around[place as usize].push(t as u32);
            }
        }
        let live = alive.iter().filter(|a| **a).count();

        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for p in &at {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let diagonal = length(sub(hi, lo));
        let mut this = Self {
            source,
            node,
            quadric: vec![Quadric::default(); at.len()],
            gone: vec![false; at.len()],
            at,
            tris,
            alive,
            around,
            live,
            scale: (diagonal * diagonal).max(1e-30),
        };
        this.price_surface();
        this
    }

    fn place(&self, vertex: u32) -> u32 {
        self.node[vertex as usize]
    }

    fn corners(&self, tri: [u32; 3]) -> [V3; 3] {
        tri.map(|v| self.at[self.place(v) as usize])
    }

    /// The planes of every face, by area, and of every border edge, steeply.
    fn price_surface(&mut self) {
        let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
        for t in 0..self.tris.len() {
            if !self.alive[t] {
                continue;
            }
            let tri = self.tris[t];
            let [p0, p1, p2] = self.corners(tri);
            let n = cross(sub(p1, p0), sub(p2, p0));
            let double_area = length(n);
            if double_area > 1e-30 {
                let unit = [n[0] / double_area, n[1] / double_area, n[2] / double_area];
                let plane = Quadric::plane(unit, -dot(unit, p0), double_area / 2.0);
                for v in tri {
                    let place = self.place(v) as usize;
                    self.quadric[place].add(&plane);
                }
            }
            for k in 0..3 {
                let (a, b) = (self.place(tri[k]), self.place(tri[(k + 1) % 3]));
                let entry = edges.entry((a.min(b), a.max(b))).or_insert((0, t as u32));
                entry.0 += 1;
            }
        }
        for ((a, b), (count, t)) in edges {
            if count != 1 {
                continue;
            }
            let [p0, p1, p2] = self.corners(self.tris[t as usize]);
            let n = cross(sub(p1, p0), sub(p2, p0));
            let (pa, pb) = (self.at[a as usize], self.at[b as usize]);
            let edge = sub(pb, pa);
            let side = cross(edge, n);
            let reach = length(side);
            if reach < 1e-30 {
                continue;
            }
            let unit = [side[0] / reach, side[1] / reach, side[2] / reach];
            let wall = Quadric::plane(unit, -dot(unit, pa), BORDER * dot(edge, edge));
            self.quadric[a as usize].add(&wall);
            self.quadric[b as usize].add(&wall);
        }
    }

    /// The triangles that touch `place`, dead ones let go.
    fn star(&mut self, place: u32) -> Vec<u32> {
        let alive = &self.alive;
        let list = &mut self.around[place as usize];
        list.retain(|t| alive[*t as usize]);
        list.clone()
    }

    /// The positions that share a triangle with `place`, and how many triangles each shares.
    fn neighbours(&self, place: u32, star: &[u32]) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        for t in star {
            for v in self.tris[*t as usize] {
                let other = self.place(v);
                if other == place {
                    continue;
                }
                match out.iter_mut().find(|o| o.0 == other) {
                    Some(o) => o.1 += 1,
                    None => out.push((other, 1)),
                }
            }
        }
        out
    }

    /// How far the channels of a vertex `own` that a collapse takes away are from what the triangle `kept` that replaces it states at the same place, with the area `own` was the corner of.
    ///
    /// ⚠ The channels are read at the point the vertex sat at, off the new triangle's plane: a surface whose texture
    /// runs linearly over it, or whose normals turn evenly, loses nothing however long its edges are.
    fn deviation(&self, own: u32, kept: [u32; 3], at: V3) -> f64 {
        let s = self.source;
        let [q0, q1, q2] = self.corners(kept);
        let lambda = barycentric(at, q0, q1, q2);
        let all = s.positions.len();
        let own = own as usize;
        let mix2 = |c: &[[f32; 2]]| -> f64 {
            let predicted = [0, 1].map(|k| {
                (0..3)
                    .map(|i| lambda[i] * f64::from(c[kept[i] as usize][k]))
                    .sum::<f64>()
            });
            let (du, dv) = (
                f64::from(c[own][0]) - predicted[0],
                f64::from(c[own][1]) - predicted[1],
            );
            du * du + dv * dv
        };
        let mut worst = 0.0f64;
        for uv in s.uvs.iter().filter(|c| c.len() == all) {
            worst = worst.max(PRICE_UV * mix2(uv));
        }
        if s.normals.len() == all {
            let predicted = [0, 1, 2].map(|k| {
                (0..3)
                    .map(|i| lambda[i] * f64::from(s.normals[kept[i] as usize][k]))
                    .sum::<f64>()
            });
            let d = sub(s.normals[own].map(f64::from), predicted);
            worst += PRICE_NORMAL * dot(d, d);
        }
        if s.joints.len() == all && s.weights.len() == all {
            // Four lanes of the vertex and of each corner, so sixteen bones at most, held on the stack.
            let weight_on = |v: usize, bone: u16| -> f64 {
                (0..4)
                    .filter(|k| s.joints[v][*k] == bone)
                    .map(|k| f64::from(s.weights[v][k]))
                    .sum()
            };
            let mut bones = [0u16; 16];
            let mut count = 0;
            for v in std::iter::once(own).chain(kept.iter().map(|k| *k as usize)) {
                for k in 0..4 {
                    let bone = s.joints[v][k];
                    if s.weights[v][k] > 0.0 && !bones[..count].contains(&bone) {
                        bones[count] = bone;
                        count += 1;
                    }
                }
            }
            let mut l1 = 0.0f64;
            for bone in &bones[..count] {
                let predicted: f64 = (0..3)
                    .map(|i| lambda[i] * weight_on(kept[i] as usize, *bone))
                    .sum();
                l1 += (weight_on(own, *bone) - predicted).abs();
            }
            worst += PRICE_SKIN * l1 * l1;
        }
        worst
    }

    /// The price of the channels over the triangles `a` is given up from, by their area.
    fn price_channels(&self, a: u32, b: u32, given: &Given, star: &[u32]) -> f64 {
        let at = self.at[a as usize];
        let mut total = 0.0;
        for t in star {
            let tri = self.tris[*t as usize];
            if tri.iter().any(|v| self.place(*v) == b) {
                continue;
            }
            let Some(slot) = tri.iter().position(|v| self.place(*v) == a) else {
                continue;
            };
            let own = tri[slot];
            let Some(kept) = given.iter().find(|g| g.0 == own).map(|g| g.1) else {
                continue;
            };
            let mut replaced = tri;
            replaced[slot] = kept;
            let old = self.corners(tri);
            let area = length(cross(sub(old[1], old[0]), sub(old[2], old[0]))) / 2.0;
            total += area * self.deviation(own, replaced, at);
        }
        total
    }

    /// What it costs to give `a` up to `b`, and the vertices it hands over; none when it is not safe.
    fn price(&mut self, a: u32, b: u32) -> Option<(f64, Given)> {
        for place in [a, b] {
            let alive = &self.alive;
            self.around[place as usize].retain(|t| alive[*t as usize]);
        }
        self.price_now(a, b)
    }

    /// [`Self::price`] over triangles lists that hold no dead triangle.
    fn price_now(&self, a: u32, b: u32) -> Option<(f64, Given)> {
        let ta = &self.around[a as usize];
        let tb = &self.around[b as usize];
        let ca = self.neighbours(a, ta);
        let cb = self.neighbours(b, tb);
        // A position on an edge that more than two faces share is left as it is.
        if ca.iter().chain(&cb).any(|c| c.1 >= 3) {
            return None;
        }
        let shared = ca.iter().find(|c| c.0 == b)?.1;
        // A border position slides along its border and goes nowhere else.
        if ca.iter().any(|c| c.1 == 1) && shared != 1 {
            return None;
        }
        // The two share the neighbours of the triangles they share and no others, or the surface would pinch.
        let common = ca
            .iter()
            .filter(|c| c.0 != b && cb.iter().any(|d| d.0 == c.0))
            .count();
        if common as u32 != shared {
            return None;
        }
        if ca.len() + cb.len() - 2 - common > VALENCE {
            return None;
        }

        let mut given: Given = Vec::new();
        for t in ta {
            let tri = self.tris[*t as usize];
            let Some(own) = tri.iter().copied().find(|v| self.place(*v) == a) else {
                continue;
            };
            let Some(kept) = tri.iter().copied().find(|v| self.place(*v) == b) else {
                continue;
            };
            match given.iter().find(|g| g.0 == own) {
                Some(g) if g.1 != kept => return None,
                Some(_) => {}
                None => given.push((own, kept)),
            }
        }
        // Every vertex the position has must have a successor, or a seam would tear.
        for t in ta {
            for v in self.tris[*t as usize] {
                if self.place(v) == a && !given.iter().any(|g| g.0 == v) {
                    return None;
                }
            }
        }

        let to = self.at[b as usize];
        for t in ta {
            let tri = self.tris[*t as usize];
            if tri.iter().any(|v| self.place(*v) == b) {
                continue;
            }
            let before = self.corners(tri);
            let mut after = before;
            for k in 0..3 {
                if self.place(tri[k]) == a {
                    after[k] = to;
                }
            }
            let was = cross(sub(before[1], before[0]), sub(before[2], before[0]));
            let is = cross(sub(after[1], after[0]), sub(after[2], after[0]));
            let (was_len, is_len) = (length(was), length(is));
            if was_len > 1e-30 && (is_len <= 0.0 || dot(was, is) < FLIP * was_len * is_len) {
                return None;
            }
        }

        let mut joined = self.quadric[a as usize];
        joined.add(&self.quadric[b as usize]);
        let geometric = joined.eval(to).max(0.0) / self.scale;
        let channels = self.price_channels(a, b, &given, ta);
        Some((geometric + channels, given))
    }

    fn offer(&mut self, heap: &mut BinaryHeap<Entry>, a: u32, b: u32) {
        for (from, to) in [(a, b), (b, a)] {
            if let Some((cost, _)) = self.price(from, to) {
                heap.push(Reverse((cost.to_bits(), from, to)));
            }
        }
    }

    /// Collapse until no more than `target` triangles are left or nothing safe is; the largest distance a collapse put a surface off by.
    ///
    /// A collapse prices again only the edges of the position it keeps, so one that a collapse nearby made safe is not offered
    /// until every edge is priced afresh, which is done again for as long as a pass collapses anything.
    fn run(&mut self, target: usize) -> f64 {
        let mut worst = 0.0f64;
        loop {
            let was = self.live;
            let mut heap: BinaryHeap<Entry> = BinaryHeap::new();
            for p in 0..self.at.len() as u32 {
                if self.gone[p as usize] {
                    continue;
                }
                let star = self.star(p);
                for (q, _) in self.neighbours(p, &star) {
                    if p < q {
                        self.offer(&mut heap, p, q);
                    }
                }
            }
            worst = worst.max(self.drain(&mut heap, target));
            if self.live <= target || self.live == was {
                return worst.sqrt();
            }
        }
    }

    /// Collapse in the order of the prices in `heap` until `target` is reached or it is empty; the largest squared price paid.
    fn drain(&mut self, heap: &mut BinaryHeap<Entry>, target: usize) -> f64 {
        let mut worst = 0.0f64;
        while self.live > target {
            let Some(Reverse((offered, a, b))) = heap.pop() else {
                break;
            };
            if self.gone[a as usize] || self.gone[b as usize] {
                continue;
            }
            // What has happened around it since is priced when it comes up: one that costs more now waits its turn again.
            let Some((cost, given)) = self.price(a, b) else {
                continue;
            };
            if cost > f64::from_bits(offered) * (1.0 + 1e-9) + 1e-18 {
                heap.push(Reverse((cost.to_bits(), a, b)));
                continue;
            }
            let mut joined = self.quadric[a as usize];
            joined.add(&self.quadric[b as usize]);
            // The quadric's planes have unit normals, so its first three diagonal terms add up to the weight of them all.
            let weight = joined.0[0] + joined.0[4] + joined.0[7];
            if weight > 0.0 {
                worst = worst.max(joined.eval(self.at[b as usize]).max(0.0) / weight);
            }

            self.collapse(a, b, &given);
            // Everything that touches the position kept is priced again; the rest are priced as they come up.
            let star = self.star(b);
            for (other, _) in self.neighbours(b, &star) {
                self.offer(heap, b, other);
            }
        }
        worst
    }

    fn collapse(&mut self, a: u32, b: u32, given: &Given) {
        for t in std::mem::take(&mut self.around[a as usize]) {
            let t = t as usize;
            if !self.alive[t] {
                continue;
            }
            let tri = self.tris[t];
            if tri.iter().any(|v| self.place(*v) == b) {
                self.alive[t] = false;
                self.live -= 1;
                continue;
            }
            let mut moved = tri;
            for k in 0..3 {
                if self.place(tri[k]) == a {
                    moved[k] = given.iter().find(|g| g.0 == tri[k]).map_or(tri[k], |g| g.1);
                }
            }
            self.tris[t] = moved;
            self.around[b as usize].push(t as u32);
        }
        self.gone[a as usize] = true;
        let given_up = self.quadric[a as usize];
        self.quadric[b as usize].add(&given_up);
    }

    /// The primitive the surviving triangles make, with the vertices they name in the order the source had them.
    fn result(&self) -> (Primitive, usize) {
        let mut used = vec![false; self.source.positions.len()];
        for (t, tri) in self.tris.iter().enumerate() {
            if self.alive[t] {
                for v in tri {
                    used[*v as usize] = true;
                }
            }
        }
        let mut renumber = vec![u32::MAX; used.len()];
        let mut kept: Vec<u32> = Vec::new();
        for (v, u) in used.iter().enumerate() {
            if *u {
                renumber[v] = kept.len() as u32;
                kept.push(v as u32);
            }
        }
        let s = self.source;
        let all = s.positions.len();
        let primitive = Primitive {
            material: s.material,
            material_name: s.material_name.clone(),
            positions: carried(&s.positions, &kept, all),
            normals: carried(&s.normals, &kept, all),
            tangents: carried(&s.tangents, &kept, all),
            uvs: s.uvs.iter().map(|c| carried(c, &kept, all)).collect(),
            uv_channels: s.uv_channels.clone(),
            colours: s.colours.iter().map(|c| carried(c, &kept, all)).collect(),
            colour_channels: s.colour_channels.clone(),
            joints: carried(&s.joints, &kept, all),
            weights: carried(&s.weights, &kept, all),
            indices: self
                .tris
                .iter()
                .enumerate()
                .filter(|(t, _)| self.alive[*t])
                .flat_map(|(_, tri)| tri.iter().map(|v| renumber[*v as usize]))
                .collect(),
            lod: s.lod,
            model: s.model,
            geometry: s.geometry,
            stated_address: s.stated_address,
        };
        let count = kept.len();
        (primitive, count)
    }
}

/// The entries of `channel` that `kept` names, or none where the channel does not state every vertex.
fn carried<E: Copy>(channel: &[E], kept: &[u32], vertices: usize) -> Vec<E> {
    if channel.len() != vertices {
        return Vec::new();
    }
    kept.iter().map(|v| channel[*v as usize]).collect()
}

/// The barycentric coordinates of the projection of `p` on the triangle `a b c`, held inside it.
fn barycentric(p: V3, a: V3, b: V3, c: V3) -> [f64; 3] {
    let (v0, v1, v2) = (sub(b, a), sub(c, a), sub(p, a));
    let (d00, d01, d11) = (dot(v0, v0), dot(v0, v1), dot(v1, v1));
    let (d20, d21) = (dot(v2, v0), dot(v2, v1));
    let denominator = d00 * d11 - d01 * d01;
    if denominator.abs() < 1e-30 {
        return [1.0 / 3.0; 3];
    }
    let v = (d11 * d20 - d01 * d21) / denominator;
    let w = (d00 * d21 - d01 * d20) / denominator;
    let held = [(1.0 - v - w).max(0.0), v.max(0.0), w.max(0.0)];
    let sum: f64 = held.iter().sum();
    if sum <= 0.0 {
        return [1.0 / 3.0; 3];
    }
    held.map(|x| x / sum)
}
