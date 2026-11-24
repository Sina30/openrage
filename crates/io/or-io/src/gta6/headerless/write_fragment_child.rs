//! A fragment's child moved, or given another group, in the value read from its body and in the
//! body's sections. A move sets the child's own transform: the composite's matrix for it is its
//! group bone's bind world times that transform, the composite's box the union of its children's
//! placed boxes, and its BVH is fitted again. A group change keeps where the child lies: its
//! record moves into the new group's range and the composite's per-child arrays, its BVH leaves
//! and the vehicle glass's windows follow the children's new order.

use super::bound::bound_at;
use super::bytes::{sections, u64_at};
use super::fragment::{apply, bind_worlds, mul, rotation, M3};
use super::write_bound_reshape::put_child_flags;
use super::write_bvh::{corners, union, Span, NODE};
use super::write_physics::{
    local_box, put, put_vec3, refuse, write_bound_at, write_fragment, write_fragment_bound,
};
use super::{
    read_drawable_with_bound, Bone, Bound, BoundChild, DrawableWithBound, Fragment, FragmentChild,
    GlassWindow, HeaderlessError, Result, VehicleGlass,
};

/// A fragment child record's bytes.
const CHILD: usize = 0x50;
/// A fragment group record's bytes.
const GROUP: usize = 0x70;
/// No place among the composite's children.
const NONE: u16 = 0xFFFF;
/// Where a vehicle glass window's header holds the index of the child it is.
const WINDOW_CHILD: usize = 0x44;

const NO_SUCH_CHILD: &str = "the fragment holds no such child";
const NO_SUCH_GROUP: &str = "the fragment holds no such group";
const GLASS: &str =
    "its vehicle glass names its children by index and does not read, so its windows cannot follow a group change";

/// A bone's bind world: its rotation and scale, then its translation.
type World = (M3, [f32; 3]);

/// The bind world of group `group`'s bone in `bones`, which must be the skeleton the groups name.
fn world_of(f: &Fragment, bones: &[Bone], group: u16) -> Result<World> {
    let g = (f.groups.get(usize::from(group))).ok_or(refuse(NO_SUCH_GROUP))?;
    let bone = usize::from(g.bone);
    if bones.get(bone).map(|b| b.hash) != Some(g.bone_hash) {
        return Err(refuse(
            "the skeleton given is not the one the fragment's groups name",
        ));
    }
    (bind_worlds(bones).get(bone).copied()).ok_or(refuse(NO_SUCH_GROUP))
}

/// Where a child of own transform `q` and `t` lies off a bone of bind world `world`, rows as a
/// composite's matrix with each fourth lane zero, as the reader places it.
fn placed((rw, tw): &World, q: [f32; 4], t: [f32; 3]) -> [[f32; 4]; 4] {
    let r2 = mul(rw, &rotation(q));
    let moved = apply(rw, t);
    let t2: [f32; 3] = std::array::from_fn(|k| moved[k] + tw[k]);
    std::array::from_fn(|c| match c {
        3 => [t2[0], t2[1], t2[2], 0.0],
        c => [r2[0][c], r2[1][c], r2[2][c], 0.0],
    })
}

fn transform_holds(q: [f32; 4], t: [f32; 3]) -> Result<()> {
    if q.iter().chain(&t).any(|x| !x.is_finite()) {
        return Err(refuse(
            "a child's rotation and translation are finite numbers",
        ));
    }
    let length = q.iter().map(|x| x * x).sum::<f32>().sqrt();
    if (length - 1.0).abs() >= 1e-3 {
        return Err(refuse("a child's rotation is a unit quaternion"));
    }
    Ok(())
}

/// The composite's radius and box once child `moved` lies at `matrix`: the box the union of every
/// child's box placed by its matrix, and the box as read where a child's box does not read; the
/// radius raised to reach the moved child.
fn fitted_root(b: &Bound, moved: usize, matrix: &[[f32; 4]; 4]) -> Result<(f32, Span)> {
    let mut held: Option<Span> = None;
    let mut every = true;
    let mut reach = b.radius;
    for (i, c) in b.children.iter().enumerate() {
        let m = if i == moved { matrix } else { &c.matrix };
        let shape = (c.bound.as_ref()).and_then(|s| s.as_ref().ok());
        let Some(span) = shape.and_then(local_box) else {
            every = false;
            continue;
        };
        for p in corners(m, span) {
            held = Some(held.map_or((p, p), |h| union(h, (p, p))));
            if i == moved {
                let d: f32 = (0..3).map(|a| (p[a] - b.centroid[a]).powi(2)).sum();
                reach = reach.max(d.sqrt());
            }
        }
    }
    let mut held = held.ok_or(refuse("no child of the composite has a box that reads"))?;
    if !every {
        held = union(held, (b.box_min, b.box_max));
    }
    if (0..3).any(|a| !(held.0[a]..=held.1[a]).contains(&b.centroid[a])) {
        return Err(refuse(
            "the composite's centroid would lie outside the box of its children",
        ));
    }
    Ok((reach, held))
}

/// Child `child` of `level` given its own `q` and `t`, local to its group's bone in `bones`.
// Where it lies, the composite's matrix for it and the composite's box and radius follow.
pub fn place_fragment_child(
    level: &mut DrawableWithBound,
    bones: &[Bone],
    child: usize,
    q: [f32; 4],
    t: [f32; 3],
) -> Result<()> {
    transform_holds(q, t)?;
    let c = (level.fragment.children.get(child)).ok_or(refuse(NO_SUCH_CHILD))?;
    let matrix = placed(&world_of(&level.fragment, bones, c.group)?, q, t);
    let listed = match c.composite {
        Some(rank) => {
            let rank = usize::from(rank);
            let b = (level.bound.as_ref()).ok_or(refuse("the fragment holds no bound"))?;
            let held = (b.children.get(rank)).ok_or(refuse(
                "the composite lists fewer children than the fragment names",
            ))?;
            // Each row's fourth lane, whose meaning is not known, is kept.
            let m: [[f32; 4]; 4] = std::array::from_fn(|r| {
                [matrix[r][0], matrix[r][1], matrix[r][2], held.matrix[r][3]]
            });
            let root = fitted_root(b, rank, &m)?;
            Some((rank, m, root))
        }
        None => None,
    };
    if let Some(c) = level.fragment.children.get_mut(child) {
        (c.rotation, c.translation, c.matrix) = (q, t, matrix);
    }
    if let (Some((rank, m, (radius, (lo, hi)))), Some(b)) = (listed, level.bound.as_mut()) {
        if let Some(held) = b.children.get_mut(rank) {
            held.matrix = m;
        }
        (b.radius, b.box_min, b.box_max) = (radius, lo, hi);
    }
    Ok(())
}

fn inverse(m: &M3) -> Option<[[f64; 3]; 3]> {
    let a: [[f64; 3]; 3] = m.map(|r| r.map(f64::from));
    // Each cofactor with its sign, from the rows and columns after its own taken in turn.
    let cofactor = |i: usize, j: usize| {
        let (r0, r1, c0, c1) = ((i + 1) % 3, (i + 2) % 3, (j + 1) % 3, (j + 2) % 3);
        a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0]
    };
    let det: f64 = (0..3).map(|j| a[0][j] * cofactor(0, j)).sum();
    if !det.is_finite() || det.abs() <= 1e-12 {
        return None;
    }
    Some(std::array::from_fn(|i| {
        std::array::from_fn(|j| cofactor(j, i) / det)
    }))
}

/// The unit quaternion x, y, z, w of the rotation `m`, which turns a column as the reader's does.
fn quaternion(m: &[[f64; 3]; 3]) -> [f32; 4] {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            s / 4.0,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [
            s / 4.0,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [
            (m[0][1] + m[1][0]) / s,
            s / 4.0,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        ]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            s / 4.0,
            (m[1][0] - m[0][1]) / s,
        ]
    };
    let length = q.iter().map(|x| x * x).sum::<f64>().sqrt();
    q.map(|x| (x / length) as f32)
}

/// The own transform that keeps child `child` where it lies on group `group`'s bone in `bones`.
// That bone's bind world undone from where the child lies.
pub fn fragment_child_local_in(
    f: &Fragment,
    bones: &[Bone],
    child: usize,
    group: u16,
) -> Result<([f32; 4], [f32; 3])> {
    let c = f.children.get(child).ok_or(refuse(NO_SUCH_CHILD))?;
    let (rg, tg) = world_of(f, bones, c.group)?;
    let (rh, th) = world_of(f, bones, group)?;
    let r = mul(&rg, &rotation(c.rotation));
    let moved = apply(&rg, c.translation);
    let at: [f64; 3] = std::array::from_fn(|k| f64::from(moved[k] + tg[k]) - f64::from(th[k]));
    let undo = inverse(&rh).ok_or(refuse("the group's bone has no inverse bind"))?;
    let turned: [[f64; 3]; 3] = std::array::from_fn(|i| {
        std::array::from_fn(|j| (0..3).map(|k| undo[i][k] * f64::from(r[k][j])).sum())
    });
    let unit = (0..3).all(|i| {
        (0..3).all(|j| {
            let d: f64 = (0..3).map(|k| turned[k][i] * turned[k][j]).sum();
            (d - f64::from(u8::from(i == j))).abs() < 1e-3
        })
    });
    if !unit {
        return Err(refuse(
            "the two groups' bones are scaled otherwise, so no rotation keeps the child where it lies",
        ));
    }
    let t: [f32; 3] =
        std::array::from_fn(|i| (0..3).map(|k| undo[i][k] * at[k]).sum::<f64>() as f32);
    Ok((quaternion(&turned), t))
}

/// Where child `child` lies among its group's children of its set.
// Its set is those the composite lists, or those of the second set.
pub fn fragment_child_place(f: &Fragment, child: usize) -> Option<usize> {
    let c = f.children.get(child)?;
    let g = f.groups.get(usize::from(c.group))?;
    let range = match c.composite {
        Some(_) => &g.children,
        None => &g.second_set,
    };
    child.checked_sub(usize::from(range.start))
}

/// Child `child` of `level` given group `group`, at `place` among that group's children of its set,
/// with its own `q` and `t`; its index after.
// The children lie group by group, those the composite lists first, and the composite lists them in
// that order: a child it lists takes its new rank, and the composite's children follow.
pub fn regroup_fragment_child(
    level: &mut DrawableWithBound,
    child: usize,
    group: u16,
    place: usize,
    q: [f32; 4],
    t: [f32; 3],
) -> Result<usize> {
    transform_holds(q, t)?;
    let f = &level.fragment;
    let c = f.children.get(child).ok_or(refuse(NO_SUCH_CHILD))?;
    let target = (f.groups.get(usize::from(group))).ok_or(refuse(NO_SUCH_GROUP))?;
    let range = match c.composite {
        Some(_) => &target.children,
        None => &target.second_set,
    };
    let (start, mut len) = (usize::from(range.start), range.len());
    let start = start - usize::from(child < start);
    if c.group == group {
        len = len.saturating_sub(1);
    }
    if place > len || start + place >= f.children.len() {
        return Err(refuse(
            "the group holds fewer children of the child's set than that place",
        ));
    }
    let mut order: Vec<usize> = (0..f.children.len()).filter(|&i| i != child).collect();
    order.insert(start + place, child);
    let glass = match &f.vehicle_glass {
        Some(Ok(g)) => Some(glass_renumbered(g, &new_of(&order))?.0),
        Some(Err(_)) => return Err(refuse(GLASS)),
        None => None,
    };
    let groups_of: Vec<u16> = (order.iter())
        .map(|&i| match i == child {
            true => group,
            false => f.children[i].group,
        })
        .collect();
    let listed: Vec<bool> = order
        .iter()
        .map(|&i| f.children[i].composite.is_some())
        .collect();
    let mut groups = f.groups.clone();
    let mut next = 0u16;
    for (j, g) in groups.iter_mut().enumerate() {
        let count = |second: bool| {
            (groups_of.iter().zip(&listed))
                .filter(|(of, l)| usize::from(**of) == j && **l != second)
                .count() as u16
        };
        let (own, second) = (count(false), count(true));
        g.children = next..next + own;
        g.second_set = next + own..next + own + second;
        next += own + second;
    }
    let mut wheels = f.wheels.clone();
    for w in &mut wheels {
        let g = groups
            .get(usize::from(w.group))
            .ok_or(refuse(NO_SUCH_GROUP))?;
        if g.children.is_empty() {
            return Err(refuse(
                "a group on a wheel bone keeps a child the composite lists",
            ));
        }
        w.child = g.children.start;
    }
    let mut rank = 0u16;
    let ranks: Vec<Option<u16>> = (listed.iter())
        .map(|l| {
            l.then(|| {
                rank += 1;
                rank - 1
            })
        })
        .collect();
    // The rank each of the composite's children had, in their new order.
    let was: Vec<usize> = (order.iter())
        .filter_map(|&i| f.children[i].composite)
        .map(usize::from)
        .collect();
    if let Some(b) = &level.bound {
        let mut seen = was.clone();
        seen.sort_unstable();
        if seen != (0..b.children.len()).collect::<Vec<_>>() {
            return Err(refuse(
                "the composite's children are not the children the fragment lists",
            ));
        }
    }

    let mut old: Vec<Option<FragmentChild>> = std::mem::take(&mut level.fragment.children)
        .into_iter()
        .map(Some)
        .collect();
    let mut children = Vec::with_capacity(old.len());
    for (k, &i) in order.iter().enumerate() {
        let Some(mut c) = old.get_mut(i).and_then(Option::take) else {
            continue;
        };
        (c.group, c.composite) = (groups_of[k], ranks[k]);
        if i == child {
            (c.rotation, c.translation) = (q, t);
        }
        children.push(c);
    }
    level.fragment.children = children;
    level.fragment.groups = groups;
    level.fragment.wheels = wheels;
    if let Some(g) = glass {
        level.fragment.vehicle_glass = Some(Ok(g));
    }
    if let Some(b) = level.bound.as_mut() {
        let mut old: Vec<Option<BoundChild>> = std::mem::take(&mut b.children)
            .into_iter()
            .map(Some)
            .collect();
        b.children = (was.iter())
            .filter_map(|&r| old.get_mut(r).and_then(Option::take))
            .collect();
    }
    Ok(start + place)
}

/// The new index of each child, `order` naming the old index of each new one.
fn new_of(order: &[usize]) -> Vec<usize> {
    let mut to = vec![0; order.len()];
    for (k, &i) in order.iter().enumerate() {
        if let Some(slot) = to.get_mut(i) {
            *slot = k;
        }
    }
    to
}

/// `glass` with each window's child `i` named `to[i]`, the windows kept ascending by child as every
/// measured block holds them; whether a window changed place.
fn glass_renumbered(glass: &VehicleGlass, to: &[usize]) -> Result<(VehicleGlass, bool)> {
    let mut out = glass.clone();
    for w in &mut out.windows {
        let new = (to.get(usize::from(w.child())))
            .and_then(|n| u16::try_from(*n).ok())
            .ok_or(refuse(
                "a vehicle glass window names a child past the fragment's children",
            ))?;
        w.header[WINDOW_CHILD..WINDOW_CHILD + 2].copy_from_slice(&new.to_le_bytes());
    }
    let moved = out.windows.windows(2).any(|w| w[0].child() > w[1].child());
    out.windows.sort_by_key(GlassWindow::child);
    Ok((out, moved))
}

/// The vehicle glass `edited` holds written over `read`'s, which it must be with its windows'
/// children renumbered by `order`, the old index of each new child.
// The windows keep their bytes and only change place, so the block keeps its length.
fn glass_written(v: &mut [u8], read: &Fragment, edited: &Fragment, order: &[usize]) -> Result<()> {
    let (r, e) = match (&read.vehicle_glass, &edited.vehicle_glass) {
        (None, None) => return Ok(()),
        (Some(Ok(r)), Some(Ok(e))) => (r, e),
        _ => return Err(refuse(GLASS)),
    };
    let (renumbered, moved) = glass_renumbered(r, &new_of(order))?;
    if renumbered != *e {
        return Err(refuse(
            "the edit's vehicle glass is not the fragment's with its windows' children renumbered",
        ));
    }
    let (at, stated) = sections(v, None, |b, _| {
        let at = b.to("vehicle glass", 0xF0)?;
        Ok((at, b.u32("vehicle glass", at + 8)? as usize))
    })?;
    let bytes = e.encode();
    if bytes.len() != stated {
        return Err(refuse(
            "the vehicle glass would encode to another length than the block states",
        ));
    }
    if moved {
        aimed_inside(v, at, stated)?;
    }
    put(v, at, &bytes)
}

/// `len` bytes of `v` at `at`, copied.
fn copied(v: &[u8], at: usize, len: usize) -> Result<Vec<u8>> {
    (at.checked_add(len))
        .and_then(|end| v.get(at..end))
        .map(<[u8]>::to_vec)
        .ok_or(HeaderlessError::PastEnd {
            what: "records reordered",
            at: at as u64,
            section: "virtual",
            len: v.len() as u64,
        })
}

/// Refused where a word of `v` aims inside the `len` bytes at `start` other than at their first:
/// what it names would move with the records.
fn aimed_inside(v: &[u8], start: usize, len: usize) -> Result<()> {
    let end = start.saturating_add(len);
    for o in (0..v.len().saturating_sub(7)).step_by(8) {
        let word = u64_at(v, o);
        let target = (word & 0x0FFF_FFFF) as usize;
        if word >> 32 == 0 && word >> 28 == 5 && target > start && target < end {
            return Err(refuse(
                "a pointer aims inside the records a group change reorders, so what it names would move",
            ));
        }
    }
    Ok(())
}

/// The composite at `at`'s children put in a new order, child `k` going to `to[k]`: its pointers,
/// its one matrix array, its type and include entries and its BVH leaves' children.
fn reorder_composite(v: &mut [u8], at: usize, to: &[usize]) -> Result<()> {
    let (pointers, matrices, again, flags, bvh) = sections(v, Some(&[]), |b, _| {
        Ok((
            b.to("child pointers", at + 0x70)?,
            b.to("child matrices", at + 0x78)?,
            b.ptr("child matrices", at + 0x80)?,
            b.ptr("child type and include entries", at + 0x88)?,
            b.ptr("composite BVH", at + 0xA0)?,
        ))
    })?;
    if again.is_some_and(|a| a != matrices) {
        return Err(refuse(
            "the composite's two matrix pointers aim at two arrays, where every measured one aims at one",
        ));
    }
    for (base, stride) in [(Some(pointers), 8), (Some(matrices), 64), (flags, 16)] {
        let Some(base) = base else {
            continue;
        };
        let len = stride * to.len();
        aimed_inside(v, base, len)?;
        let old = copied(v, base, len)?;
        for (k, new) in to.iter().enumerate() {
            put(v, base + stride * new, &old[stride * k..stride * (k + 1)])?;
        }
    }
    let Some(bvh) = bvh else {
        return Ok(());
    };
    let (nodes, count) = sections(v, Some(&[]), |b, _| {
        Ok((
            b.to("BVH nodes", bvh + 0x50)?,
            b.u32("BVH node count", bvh + 0x58)? as usize,
        ))
    })?;
    let len = count
        .checked_mul(NODE)
        .ok_or(refuse("the BVH states more nodes than a section holds"))?;
    let read = copied(v, nodes, len)?;
    for (i, node) in read.as_chunks::<NODE>().0.iter().enumerate() {
        let word = |o: usize| usize::from(u16::from_le_bytes([node[o], node[o + 1]]));
        match word(0xE) {
            0 => continue,
            1 => {}
            _ => {
                return Err(refuse(
                    "a composite BVH leaf holds more than one child, so the children's new order is no order of its leaves",
                ))
            }
        }
        let new = (to.get(word(0xC))).ok_or(refuse(
            "a composite BVH's leaf names a child past its children",
        ))?;
        let new = u16::try_from(*new)
            .map_err(|_| refuse("the composite lists more children than a leaf names"))?;
        put(v, nodes + NODE * i + 0xC, &new.to_le_bytes())?;
    }
    Ok(())
}

/// The children of `edited` in another order or group than `read`'s written: each record moved to
/// its place with its group, rank and own transform, the groups' ranges, and the composite's
/// arrays and BVH leaves in the new order; whether a child the composite lists took another
/// transform.
fn regroup_written(
    v: &mut [u8],
    read: &Fragment,
    edited: &Fragment,
    at: Option<usize>,
) -> Result<bool> {
    let (r, e) = (&read.children, &edited.children);
    if r.len() != e.len() {
        return Err(refuse("the fragment holds another number of children"));
    }
    let same = |(a, b): (&FragmentChild, &FragmentChild)| {
        (a.bound_word, a.group) == (b.bound_word, b.group)
    };
    if r.iter().zip(e).all(same) {
        return Ok(false);
    }
    let kept = |(a, b): (&super::FragmentGroup, &super::FragmentGroup)| {
        (a.bone, a.bone_hash, a.parent, &a.child_groups)
            == (b.bone, b.bone_hash, b.parent, &b.child_groups)
    };
    if read.groups.len() != edited.groups.len() || !read.groups.iter().zip(&edited.groups).all(kept)
    {
        return Err(refuse(
            "a group change keeps every group's bone, parent and child groups",
        ));
    }
    let mut order = Vec::with_capacity(e.len());
    let mut taken = vec![false; r.len()];
    for c in e {
        let mut found = (r.iter().enumerate()).filter(|(_, x)| x.bound_word == c.bound_word);
        let (i, x) = found.next().ok_or(refuse(
            "an edited child names a bound no child of the fragment names",
        ))?;
        if found.next().is_some() || taken[i] {
            return Err(refuse(
                "two children name one bound, so which record a group change moves is not known",
            ));
        }
        if x.composite.is_some() != c.composite.is_some() {
            return Err(refuse("a group change keeps a child in its set"));
        }
        taken[i] = true;
        order.push(i);
    }
    let keys: Vec<(u16, bool)> = e.iter().map(|c| (c.group, c.composite.is_none())).collect();
    if keys.windows(2).any(|w| w[0] > w[1])
        || keys.iter().any(|k| usize::from(k.0) >= edited.groups.len())
    {
        return Err(refuse(
            "the edit's children are not in their groups' order, those the composite lists first",
        ));
    }
    let mut start = 0u16;
    for (j, g) in edited.groups.iter().enumerate() {
        let count = |second: bool| {
            keys.iter()
                .filter(|k| usize::from(k.0) == j && k.1 == second)
                .count() as u16
        };
        let (own, second) = (count(false), count(true));
        if (g.children.clone(), g.second_set.clone())
            != (start..start + own, start + own..start + own + second)
        {
            return Err(refuse("the edit's group ranges are not its children's"));
        }
        start += own + second;
    }
    let mut to = vec![0usize; r.iter().filter(|c| c.composite.is_some()).count()];
    let mut rank = 0u16;
    for (c, &i) in e.iter().zip(&order) {
        let Some(was) = r[i].composite else {
            continue;
        };
        if c.composite != Some(rank) {
            return Err(refuse(
                "the edit's places among the composite's children are not its children's order",
            ));
        }
        let slot = to.get_mut(usize::from(was)).ok_or(refuse(
            "a child names a place past the composite's children",
        ))?;
        *slot = usize::from(rank);
        rank += 1;
    }
    glass_written(v, read, edited, &order)?;

    let (groups, children) = sections(v, None, |b, _| {
        Ok((b.to("groups", 0x20)?, b.to("children", 0x28)?))
    })?;
    aimed_inside(v, children, CHILD * r.len())?;
    let records = copied(v, children, CHILD * r.len())?;
    // The transform is written here, so `moves_written` finds no change after it: a listed
    // child's new transform is said to the caller, which refits the composite for it.
    let mut listed = false;
    for (k, (&i, c)) in order.iter().zip(e).enumerate() {
        listed |= c.composite.is_some()
            && (r[i].rotation, r[i].translation) != (c.rotation, c.translation);
        let at = children + CHILD * k;
        put(v, at, &records[CHILD * i..CHILD * (i + 1)])?;
        put(v, at + 0x48, &c.group.to_le_bytes())?;
        put(v, at + 0x4A, &c.composite.unwrap_or(NONE).to_le_bytes())?;
        for (n, x) in c.rotation.iter().enumerate() {
            put(v, at + 4 * n, &x.to_le_bytes())?;
        }
        put_vec3(v, at + 0x10, c.translation)?;
    }
    for (j, g) in edited.groups.iter().enumerate() {
        let ends = [
            g.children.start,
            g.children.end,
            g.second_set.end,
            g.second_set.end,
        ];
        for (o, x) in [0x1A, 0x1C, 0x1E, 0x20].into_iter().zip(ends) {
            put(v, groups + GROUP * j + o, &x.to_le_bytes())?;
        }
    }
    if to.iter().enumerate().any(|(k, n)| k != *n) {
        let at = at.ok_or(refuse("the fragment holds no bound"))?;
        reorder_composite(v, at, &to)?;
    }
    Ok(listed)
}

/// The own transforms `edited` gives its children written where they changed; whether a child the
/// composite lists moved.
fn moves_written(v: &mut [u8], read: &Fragment, edited: &Fragment) -> Result<bool> {
    let children = sections(v, None, |b, _| b.ptr("children", 0x28))?;
    let mut listed = false;
    for (i, (r, e)) in read.children.iter().zip(&edited.children).enumerate() {
        if (r.group, r.composite) != (e.group, e.composite) {
            return Err(refuse("the edit's children are not the fragment's"));
        }
        if (r.rotation, r.translation) == (e.rotation, e.translation) {
            continue;
        }
        transform_holds(e.rotation, e.translation)?;
        let at = children.ok_or(refuse("the fragment has no children"))? + CHILD * i;
        for (k, x) in e.rotation.iter().enumerate() {
            put(v, at + 4 * k, &x.to_le_bytes())?;
        }
        put_vec3(v, at + 0x10, e.translation)?;
        listed |= r.composite.is_some();
    }
    Ok(listed)
}

/// The composite at `at`, read as `read`, written from `edited` with its children free to move.
fn composite_moved(v: &mut [u8], at: usize, read: &Bound, edited: &Bound) -> Result<()> {
    let (first, again) = sections(v, Some(&[]), |b, _| {
        Ok((
            b.ptr("child matrices", at + 0x78)?,
            b.ptr("child matrices", at + 0x80)?,
        ))
    })?;
    if again.is_some_and(|a| Some(a) != first) {
        return Err(refuse(
            "the composite's two matrix pointers aim at two arrays, where every measured one aims at one",
        ));
    }
    write_bound_at(v, at, read, edited, true)
}

/// The type and include entries `edited` gives its children written where they changed: each
/// child's copy at +0x38, and for a child the composite lists, the composite's entry.
fn flags_written(
    v: &mut [u8],
    read: &Fragment,
    edited: &Fragment,
    at: Option<usize>,
) -> Result<()> {
    let children = sections(v, None, |b, _| b.ptr("children", 0x28))?;
    for (i, (r, e)) in read.children.iter().zip(&edited.children).enumerate() {
        if r.flags == e.flags {
            continue;
        }
        let record = children.ok_or(refuse("the fragment has no children"))? + CHILD * i;
        put(v, record + 0x38, &e.flags[0].to_le_bytes())?;
        put(v, record + 0x40, &e.flags[1].to_le_bytes())?;
        if let Some(rank) = r.composite {
            let at = at.ok_or(refuse("the fragment holds no bound"))?;
            put_child_flags(v, at, usize::from(rank), Some(e.flags))?;
        }
    }
    Ok(())
}

/// `edited`'s physics level written over the fragment `virtual_section` holds.
// Its children's groups and order, their own transforms and the composite that follows them, their
// type and include entries, then what `write_fragment` and `write_fragment_bound` write.
pub fn write_fragment_level(
    virtual_section: &mut [u8],
    physical: &[u8],
    edited: &DrawableWithBound,
) -> Result<()> {
    let at = sections(virtual_section, Some(&[]), |v, _| {
        match v.ptr("record that holds the bound", 0x10)? {
            Some(record) => Ok(Some(v.to("bound", record + 0x20)?)),
            None => Ok(None),
        }
    })?;
    let read = read_drawable_with_bound(virtual_section, Some(physical))?;
    if regroup_written(virtual_section, &read.fragment, &edited.fragment, at)? {
        // A listed child's new transform is written and its matrix is not yet: the level reads
        // again only once the composite follows.
        let (Some(e), Some(at)) = (&edited.bound, at) else {
            return Err(refuse("the fragment holds no bound"));
        };
        let reordered = sections(virtual_section, Some(&[]), |v, _| bound_at(v, at))?;
        composite_moved(virtual_section, at, &reordered, e)?;
    }
    let read = read_drawable_with_bound(virtual_section, Some(physical))?;
    if moves_written(virtual_section, &read.fragment, &edited.fragment)? {
        let (Some(r), Some(e), Some(at)) = (&read.bound, &edited.bound, at) else {
            return Err(refuse("the fragment holds no bound"));
        };
        composite_moved(virtual_section, at, r, e)?;
    }
    flags_written(virtual_section, &read.fragment, &edited.fragment, at)?;
    write_fragment(virtual_section, physical, &edited.fragment)?;
    match &edited.bound {
        Some(b) => write_fragment_bound(virtual_section, physical, b),
        None => Ok(()),
    }
}
