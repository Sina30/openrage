//! A fragment's physics level, folded into its root: the archetype, the groups each on a bone of
//! its drawable's skeleton, and the children each placed at its group's bone.

use std::ops::Range;

use super::bound::{child_bound, Bound, ChildBound};
use super::bytes::{f32_at, u32_at, u64_at, Bytes};
use super::fragment_records::{read_records, GlassPane, GroupLinks, Lattice, VehicleGlass, Wheel};
use super::skeleton::{skeleton_at, Bone};
use super::{HeaderlessError, Result};

const GROUP: usize = 0x70;
const CHILD: usize = 0x50;
const BONE_BOUND: usize = 0x40;
/// No group, and no index into the composite.
const NONE: u16 = 0xFFFF;

#[derive(Debug, Default)]
pub struct Fragment {
    /// Root u16 0x30: 1, or 2 where most fragments use a second child set; what it counts is not known.
    pub sets: u16,
    pub archetype: Option<Archetype>,
    pub groups: Vec<FragmentGroup>,
    pub children: Vec<FragmentChild>,
    /// The bounds the root lists at 0x38 by bone, none of them a child of the composite.
    pub bone_bounds: Vec<BoneBound>,
    /// The matrix set's count, the skeleton's bone count; its matrices are not the bind pose.
    pub matrices: Option<u16>,
    /// Each record the root points at whose layout was not measured: its slot and where it lies.
    pub records: Vec<(u16, u64)>,
    /// The glass panes the record at 0x98 points at, counted at 0xDA.
    pub glass_panes: u8,
    /// The vehicle glass block at 0xF0, read or refused on its own.
    pub vehicle_glass: Option<Result<VehicleGlass>>,
    /// The glass panes the record at 0x98 lists, read or refused on their own.
    pub panes: Option<Result<Vec<GlassPane>>>,
    /// The group pairs at 0x58, GTA V's self-collision pairs.
    pub self_collision: Option<Result<Vec<[u16; 2]>>>,
    /// The group pairs with a value at 0x50.
    pub group_links: Option<Result<GroupLinks>>,
    /// The lattice of cells at 0xA0, with the vehicle record at 0xE8 checked against it.
    pub lattice: Option<Result<Lattice>>,
    /// The groups on a bone whose name starts `wheel_`.
    pub wheels: Vec<Wheel>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Archetype {
    pub mass: f32,
    pub angular_inertia: [f32; 3],
    pub type_flags: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentGroup {
    /// A bone of the drawable's skeleton, and that bone's name hash.
    pub bone: u16,
    pub bone_hash: u32,
    pub parent: Option<u16>,
    /// The groups whose parent it is.
    pub child_groups: Range<u16>,
    /// Its children the composite lists.
    pub children: Range<u16>,
    /// Its children of the second set, each with a bound of its own.
    pub second_set: Range<u16>,
}

#[derive(Debug)]
pub struct FragmentChild {
    pub group: u16,
    /// Local to its group's bone: a unit quaternion x, y, z, w.
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub angular_inertia: [f32; 3],
    pub mass: f32,
    /// Its index among the composite's children; `None` in the second set.
    pub composite: Option<u16>,
    /// A second-set child's bound, which the composite does not list.
    pub bound: Option<Result<ChildBound>>,
    /// Its type and include entry, the composite's for a child it lists.
    pub flags: [u64; 2],
    /// Where it lies in the drawable's frame, rows as a composite's matrix: its bone's bind world
    /// times its own transform.
    pub matrix: [[f32; 4]; 4],
}

/// A bound the root lists by the hash of the bone it belongs to.
#[derive(Debug)]
pub struct BoneBound {
    pub bone_hash: u32,
    /// A unit quaternion x, y, z, w, and a translation, in a frame that was not measured.
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub bound: Result<ChildBound>,
}

pub(super) type M3 = [[f32; 3]; 3];

fn rotation([x, y, z, w]: [f32; 4]) -> M3 {
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn mul(a: &M3, b: &M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

pub(super) fn apply(a: &M3, t: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| (0..3).map(|k| a[i][k] * t[k]).sum())
}

/// Each bone's bind world: its parent's times its own rotation, scale and translation.
pub(super) fn bind_worlds(bones: &[Bone]) -> Vec<(M3, [f32; 3])> {
    let mut worlds: Vec<(M3, [f32; 3])> = Vec::with_capacity(bones.len());
    for b in bones {
        let r = rotation(b.rotation);
        let local: M3 = std::array::from_fn(|i| std::array::from_fn(|j| r[i][j] * b.scale[j]));
        // The skeleton's reader refuses a parent that does not come before its bone.
        worlds.push(match b.parent.and_then(|p| worlds.get(usize::from(p))) {
            Some((pr, pt)) => {
                let t = apply(pr, b.translation);
                (mul(pr, &local), std::array::from_fn(|a| t[a] + pt[a]))
            }
            None => (local, b.translation),
        });
    }
    worlds
}

fn refuse<T>(why: &'static str) -> Result<T> {
    Err(HeaderlessError::Fragment(why))
}

fn is_unit(q: [f32; 4]) -> bool {
    (q.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs() < 1e-3
}

/// The physics level of the fragment at the root of `v`, whose drawable is at `drawable` and whose
/// archetype's composite reads to `composite`.
pub(super) fn fragment_at(
    v: Bytes,
    drawable: usize,
    composite: Option<&Bound>,
) -> Result<Fragment> {
    let counts: [u16; 4] = [
        v.u16("child set count", 0x30)?,
        v.u16("root group count", 0x32)?,
        v.u16("group count", 0x34)?,
        v.u16("child count", 0x36)?,
    ];
    let (archetype, groups, children) = (
        v.ptr("archetype", 0x10)?,
        v.ptr("groups", 0x20)?,
        v.ptr("children", 0x28)?,
    );
    if archetype.is_some() != groups.is_some() || archetype.is_some() != children.is_some() {
        return refuse("its archetype, groups and children are not all null or all set");
    }
    if archetype.is_none() != (counts[1..] == [0, 0, 0]) {
        return refuse("its group and child counts are not zero exactly where it has no archetype");
    }
    let bones = match v.ptr("skeleton", drawable + 0x18)? {
        Some(at) => skeleton_at(v, at)?.bones,
        None => Vec::new(),
    };
    let mut fragment = Fragment {
        sets: counts[0],
        records: records(v)?,
        glass_panes: match v.ptr("glass panes", 0x98)? {
            Some(_) => v.u8("glass pane count", 0xDA)?,
            None => 0,
        },
        ..Fragment::default()
    };
    if let Some(set) = v.ptr("matrix set", 0x90)? {
        let head = v.get("matrix set", set, 0x20)?;
        let count = u16::from_le_bytes([head[0x10], head[0x11]]);
        let capacity = u16::from_le_bytes([head[0x12], head[0x13]]);
        let zero = head[..0x10].iter().chain(&head[0x14..]).all(|b| *b == 0);
        if count != capacity || usize::from(count) != bones.len() || !zero {
            return refuse(
                "its matrix set's header is not zero about two counts of the skeleton's bones",
            );
        }
        v.get("matrix set", set + 0x20, 0x30 * usize::from(count))?;
        fragment.matrices = Some(count);
    }
    let (Some(archetype), Some(groups), Some(children)) = (archetype, groups, children) else {
        if v.ptr("bone-bound list", 0x38)?.is_some() {
            return refuse("it lists bounds by bone and has no archetype");
        }
        read_records(v, &mut fragment, &bones)?;
        return Ok(fragment);
    };
    let composite_at = v.to("archetype's bound", archetype + 0x20)?;
    let composite = composite.ok_or(HeaderlessError::Null {
        what: "archetype's bound",
        at: (archetype + 0x20) as u64,
    })?;
    let a = v.get("archetype", archetype, 0xD0)?;
    let fixed = [0x30, 0x34, 0x38, 0x3C].map(|o| u32_at(a, o));
    if a[..0x10].iter().any(|b| *b != 0)
        || (u32_at(a, 0x10), u32_at(a, 0x14)) != (2, 0)
        || u64_at(a, 0x18) != v.u64("name", 0xA8)?
        || fixed != [u32::MAX, u32::MAX, 0x1_0000, 0xFFFF]
    {
        return refuse("its archetype's fixed words are not those every measured one holds");
    }
    let inertia: [f32; 4] = std::array::from_fn(|k| f32_at(a, 0x40 + 4 * k));
    let inverse: [f32; 4] = std::array::from_fn(|k| f32_at(a, 0x50 + 4 * k));
    if !(0..4).all(|k| inertia[k] == 0.0 || (inertia[k] * inverse[k] - 1.0).abs() < 1e-3) {
        return refuse("its archetype's inverse inertia and mass do not undo its inertia and mass");
    }
    let mass = inertia[3];
    fragment.archetype = Some(Archetype {
        mass,
        angular_inertia: [inertia[0], inertia[1], inertia[2]],
        type_flags: u64_at(a, 0x28),
    });
    fragment.groups = groups_at(v, groups, counts, &bones)?;
    let worlds = bind_worlds(&bones);
    let records = v.array("children", children, usize::from(counts[3]), CHILD)?;
    let pointers = match composite.children.len() {
        0 => 0,
        _ => v.to("composite's children", composite_at + 0x70)?,
    };
    let mut rank = 0usize;
    let mut set_mass = 0f32;
    for (g, group) in fragment.groups.iter().enumerate() {
        let (rw, tw) = worlds[usize::from(group.bone)];
        for i in group.children.start..group.second_set.end {
            let at = usize::from(i) * CHILD;
            let r = &records[at..at + CHILD];
            let u16_at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
            let q: [f32; 4] = std::array::from_fn(|k| f32_at(r, 4 * k));
            let t: [f32; 3] = std::array::from_fn(|k| f32_at(r, 0x10 + 4 * k));
            let listed = i < group.children.end;
            let index = u16_at(0x4A);
            let expected = if listed { rank as u16 } else { NONE };
            if usize::from(u16_at(0x48)) != g || index != expected {
                return refuse("a child names another group, or another place among the composite's children, than the group's ranges give it");
            }
            if !is_unit(q) || u32_at(r, 0x4C) != 0 {
                return refuse(
                    "a child's rotation is not a unit quaternion, or its word at +0x4C is set",
                );
            }
            let r2 = mul(&rw, &rotation(q));
            let moved = apply(&rw, t);
            let t2: [f32; 3] = std::array::from_fn(|k| moved[k] + tw[k]);
            let matrix = std::array::from_fn(|c| match c {
                3 => [t2[0], t2[1], t2[2], 0.0],
                c => [r2[0][c], r2[1][c], r2[2][c], 0.0],
            });
            let flags = [u64_at(r, 0x38), u64_at(r, 0x40)];
            let mut bound = None;
            if listed {
                let Some(stated) = composite.children.get(rank) else {
                    return refuse("the composite lists fewer children than the groups give it");
                };
                let pointer = v.u64("composite's child", pointers + 8 * rank)?;
                if u64_at(r, 0x30) != pointer || stated.flags.is_some_and(|f| f != flags) {
                    return refuse("a child's bound or type and include entry is not the composite's child it names");
                }
                let m = stated.matrix;
                let reach = t2.iter().fold(1f32, |a, x| a.max(x.abs()));
                let rotated = (0..3).all(|c| (0..3).all(|a| (m[c][a] - r2[a][c]).abs() < 2e-3));
                if !rotated || (0..3).any(|k| (m[3][k] - t2[k]).abs() >= 1e-3 * reach) {
                    return refuse("a composite's matrix is not the bind world of its child's bone times the child's own transform");
                }
                set_mass += f32_at(r, 0x2C);
                rank += 1;
            } else {
                let at = v.to("child's bound", children + at + 0x30)?;
                bound = Some(child_bound(v, at).map(|(shape, _)| shape));
            }
            fragment.children.push(FragmentChild {
                group: g as u16,
                rotation: q,
                translation: t,
                angular_inertia: std::array::from_fn(|k| f32_at(r, 0x20 + 4 * k)),
                mass: f32_at(r, 0x2C),
                composite: listed.then_some(index),
                bound,
                flags,
                matrix,
            });
        }
    }
    if rank != composite.children.len() {
        return refuse("the composite lists more children than the groups give it");
    }
    if (set_mass - mass).abs() > 1e-3 * mass.max(1.0) {
        return refuse(
            "the masses of the children the composite lists do not sum to the archetype's",
        );
    }
    fragment.bone_bounds = bone_bounds(v, &bones)?;
    read_records(v, &mut fragment, &bones)?;
    Ok(fragment)
}

/// The groups at `at`, each on a bone of `bones`, their trees and ranges as the measured ones hold.
fn groups_at(v: Bytes, at: usize, counts: [u16; 4], bones: &[Bone]) -> Result<Vec<FragmentGroup>> {
    let count = usize::from(counts[2]);
    let records = v.array("groups", at, count, GROUP)?;
    let mut groups: Vec<FragmentGroup> = Vec::with_capacity(count);
    let mut kids = vec![0u16; count];
    for (i, r) in records.as_chunks::<GROUP>().0.iter().enumerate() {
        let u16_at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
        let (bone, hash) = (u16_at(0x10), u32_at(r, 0x08));
        if bones.get(usize::from(bone)).map(|b| b.hash) != Some(hash) {
            return refuse("a group's bone hash is not the hash of the bone it names");
        }
        let parent = match u16_at(0x16) {
            NONE => None,
            p if usize::from(p) < i => Some(p),
            _ => return refuse("a group's parent does not come before it"),
        };
        if let Some(p) = parent {
            kids[usize::from(p)] += 1;
        }
        let (first, child_groups) = (u16_at(0x14), u16_at(0x18));
        let ends = [0x1A, 0x1C, 0x1E, 0x20].map(u16_at);
        let previous = groups.last().map_or(0, |g| g.second_set.end);
        if ends[0] != previous || ends.windows(2).any(|w| w[0] > w[1]) || ends[2] != ends[3] {
            return refuse(
                "a group's child ranges do not ascend from where the group before it ends",
            );
        }
        if (first == NONE) != (child_groups == 0) || u32_at(r, 0x24) != u32::MAX {
            return refuse("a group's first child group and its count disagree, or its word at +0x24 is not all ones");
        }
        let first = if child_groups == 0 { 0 } else { first };
        groups.push(FragmentGroup {
            bone,
            bone_hash: hash,
            parent,
            child_groups: first..first.saturating_add(child_groups),
            children: ends[0]..ends[1],
            second_set: ends[1]..ends[2],
        });
    }
    let roots = groups.iter().filter(|g| g.parent.is_none()).count();
    let last = groups.last().map_or(0, |g| g.second_set.end);
    if roots != usize::from(counts[1]) || last != counts[3] {
        return refuse("its root groups or its children are not the counts the root states");
    }
    for (i, g) in groups.iter().enumerate() {
        let inside = |c: &FragmentGroup| c.parent == Some(i as u16);
        let held = g
            .child_groups
            .clone()
            .all(|c| groups.get(usize::from(c)).is_some_and(inside));
        if !held || g.child_groups.len() != usize::from(kids[i]) {
            return refuse("a group's child groups are not the groups whose parent it is");
        }
    }
    Ok(groups)
}

/// The bounds the root lists at 0x38 by bone, each bone one of `bones`.
fn bone_bounds(v: Bytes, bones: &[Bone]) -> Result<Vec<BoneBound>> {
    let Some(list) = v.ptr("bone-bound list", 0x38)? else {
        return Ok(Vec::new());
    };
    let head = v.get("bone-bound list", list, 0x18)?;
    let count = u32_at(head, 0x10) as usize;
    if (u32_at(head, 0x08), u32_at(head, 0x0C), u32_at(head, 0x14)) != (0, 0x10, 0) || count == 0 {
        return refuse("its bone-bound list's head is not the measured one");
    }
    let entries_at = v.to("bone bounds", list)?;
    let entries = v.array("bone bounds", entries_at, count, BONE_BOUND)?;
    let mut out = Vec::with_capacity(count);
    for (k, e) in entries.as_chunks::<BONE_BOUND>().0.iter().enumerate() {
        let q: [f32; 4] = std::array::from_fn(|i| f32_at(e, 4 * i));
        let words = [0x1C, 0x28, 0x2C, 0x30, 0x34, 0x3C].map(|o| u32_at(e, o));
        let hash = u32_at(e, 0x38);
        if !is_unit(q)
            || words != [0, 0, 0x10, 0x03E0_0000, 0, 0]
            || !bones.iter().any(|b| b.hash == hash)
        {
            return refuse("a bone bound is not a unit rotation, the measured words and a bone of the skeleton");
        }
        let at = v.to("bone bound", entries_at + BONE_BOUND * k + 0x20)?;
        out.push(BoneBound {
            bone_hash: hash,
            rotation: q,
            translation: std::array::from_fn(|i| f32_at(e, 0x10 + 4 * i)),
            bound: child_bound(v, at).map(|(shape, _)| shape),
        });
    }
    Ok(out)
}

/// The records of slots 0x50, 0x58, 0x98, 0xA0, 0xE8 and 0xF0, each with the head every measured
/// one has.
fn records(v: Bytes) -> Result<Vec<(u16, u64)>> {
    let mut out = Vec::new();
    for slot in [0x50u16, 0x58, 0x98, 0xA0, 0xE8, 0xF0] {
        let Some(at) = v.ptr("root record", slot.into())? else {
            continue;
        };
        let holds = match slot {
            0x98 => {
                let panes = usize::from(v.u8("glass pane count", 0xDA)?);
                panes > 0
                    && (0..panes).try_fold(true, |all, k| {
                        Ok::<_, HeaderlessError>(all && v.ptr("glass pane", at + 8 * k)?.is_some())
                    })?
            }
            0xA0 => v.u32("record's head", at)? & 0xFFFF == 0x02A3,
            0xE8 => v.u32("record's head", at)? == 0x3E9,
            0xF0 => {
                let head = v.get("vehicle glass", at, 6)?;
                head[..4] == *b"HWGV" && u16::from_le_bytes([head[4], head[5]]) == 0x70
            }
            _ => true,
        };
        if !holds {
            return refuse(
                "a record its root points at does not start as every measured one of its slot does",
            );
        }
        out.push((slot, at as u64));
    }
    Ok(out)
}
