//! A skeleton's bind transforms written back in place: a bone's rotation, translation and scale,
//! its local matrix at the pointer at 0x38, and the inverse bind world of it and of every bone
//! under it, each world its parent's world times its local.

use super::bytes::sections;
use super::skeleton::{owner_at, read_skeleton_of, SkeletonOwner};
use super::{Bone, HeaderlessError, Result, Skeleton};

/// Four columns, x, y and z, the last the translation: the lanes a skeleton's matrices use.
type Affine = [[f32; 3]; 4];

const BONE_LEN: usize = 0x50;
const MATRIX_LEN: usize = 0x40;

fn refuse(why: &'static str) -> HeaderlessError {
    HeaderlessError::NotInPlace(why)
}

/// The bone's local bind transform: translation, then rotation, then scale.
fn local(b: &Bone) -> Affine {
    let [x, y, z, w] = b.rotation;
    let [sx, sy, sz] = b.scale;
    let r = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
        ],
        [
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
        ],
        [
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    let scaled = |c: [f32; 3], s: f32| c.map(|lane| lane * s);
    [
        scaled(r[0], sx),
        scaled(r[1], sy),
        scaled(r[2], sz),
        b.translation,
    ]
}

/// `a` applied to the column `c`, with `a`'s translation where `point`.
fn apply(a: &Affine, c: [f32; 3], point: bool) -> [f32; 3] {
    let t = if point { a[3] } else { [0.0; 3] };
    std::array::from_fn(|r| a[0][r] * c[0] + a[1][r] * c[1] + a[2][r] * c[2] + t[r])
}

fn times(a: &Affine, b: &Affine) -> Affine {
    [
        apply(a, b[0], false),
        apply(a, b[1], false),
        apply(a, b[2], false),
        apply(a, b[3], true),
    ]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The inverse of `m`; refused where its axes are not independent or not finite.
fn inverse(m: &Affine) -> Result<Affine> {
    // The inverse of the matrix of columns c0, c1, c2 has rows c1×c2, c2×c0, c0×c1 over its
    // determinant.
    let rows = [cross(m[1], m[2]), cross(m[2], m[0]), cross(m[0], m[1])];
    let det = dot(m[0], rows[0]);
    if !det.is_normal() {
        return Err(refuse(
            "a bone's scale or rotation leaves its bind world with no inverse",
        ));
    }
    let column = |c: usize| -> [f32; 3] { std::array::from_fn(|r| rows[r][c] / det) };
    let mut inv = [column(0), column(1), column(2), [0.0; 3]];
    inv[3] = apply(&inv, m[3], false).map(|lane| -lane);
    Ok(inv)
}

/// Each bone's bind world, parents before their children.
fn worlds(bones: &[Bone]) -> Result<Vec<Affine>> {
    let mut worlds: Vec<Affine> = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let world = match b.parent.map(usize::from) {
            None => local(b),
            Some(p) => {
                let parent = worlds.get(p).ok_or(HeaderlessError::IndexPastCount {
                    what: "bone's parent, which comes before it",
                    index: p as u64,
                    count: i as u64,
                })?;
                times(parent, &local(b))
            }
        };
        worlds.push(world);
    }
    Ok(worlds)
}

/// The inverse binds of bone `bone` and of every bone under it, from the local transforms.
// Nothing changes where one is refused.
pub fn rebind_under(skeleton: &mut Skeleton, bone: usize) -> Result<()> {
    let count = skeleton.bones.len();
    if bone >= count {
        return Err(HeaderlessError::IndexPastCount {
            what: "bone",
            index: bone as u64,
            count: count as u64,
        });
    }
    let worlds = worlds(&skeleton.bones)?;
    let mut moved = vec![false; count];
    let mut inverses = Vec::new();
    for (i, b) in skeleton.bones.iter().enumerate() {
        let under = i == bone || b.parent.is_some_and(|p| moved[usize::from(p)]);
        moved[i] = under;
        if under {
            inverses.push((i, inverse(&worlds[i])?));
        }
    }
    for (i, inverse) in inverses {
        skeleton.bones[i].inverse_bind = inverse;
    }
    Ok(())
}

/// `values` over `section` at `at`, four bytes each.
fn put(section: &mut [u8], at: usize, values: &[f32]) -> Result<()> {
    let len = section.len() as u64;
    let to = (at.checked_add(4 * values.len()))
        .and_then(|end| section.get_mut(at..end))
        .ok_or(HeaderlessError::PastEnd {
            what: "edited bone",
            at: at as u64,
            section: "virtual",
            len,
        })?;
    for (bytes, value) in to.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

/// The x, y and z lanes of the four columns of the 0x40-byte matrix at `at`; the w lanes hold
/// other words and are kept.
fn put_matrix(section: &mut [u8], at: usize, m: &Affine) -> Result<()> {
    for (c, column) in m.iter().enumerate() {
        put(section, at + 16 * c, column)?;
    }
    Ok(())
}

/// `owner`'s skeleton (`true` a fragment's, `false` the root drawable's) written from `edited`.
// Each changed bone's rotation, translation, scale and local matrix, and the inverse bind world of
// it and of every bone under it. A bone keeps its name, hash and parent, which its record's text
// and the bones' order follow.
pub fn write_skeleton(
    virtual_section: &mut [u8],
    owner: impl Into<SkeletonOwner>,
    edited: &Skeleton,
) -> Result<()> {
    let owner = owner.into();
    let read = read_skeleton_of(virtual_section, owner)?
        .ok_or(refuse("the drawable's skeleton pointer is null"))?;
    if read.bones.len() != edited.bones.len() {
        return Err(refuse("the skeleton holds another number of bones"));
    }
    let named = |b: &Bone| (b.name.clone(), b.hash, b.parent);
    let pairs = read.bones.iter().zip(&edited.bones);
    if pairs.clone().any(|(r, e)| named(r) != named(e)) {
        return Err(refuse(
            "it renames a bone or gives it another parent, which its record's text and the bones' order follow",
        ));
    }
    let numbers = |b: &Bone| (b.rotation, b.translation, b.scale);
    let changed: Vec<bool> = pairs.map(|(r, e)| numbers(r) != numbers(e)).collect();
    if !changed.contains(&true) {
        return Ok(());
    }
    let finite = |b: &Bone| {
        let (rotation, translation, scale) = numbers(b);
        (rotation.iter().chain(&translation).chain(&scale)).all(|x| x.is_finite())
    };
    if (edited.bones.iter().zip(&changed)).any(|(b, c)| *c && !finite(b)) {
        return Err(refuse(
            "a bone's rotation, translation or scale is not finite",
        ));
    }
    let worlds = worlds(&edited.bones)?;
    let count = edited.bones.len();
    let (records, inverses, locals) = sections(virtual_section, None, |v, _| {
        let drawable = owner_at(v, owner)?;
        let at = v.to("skeleton", drawable + 0x18)?;
        let records = v.to("bones", at + 0x28)?;
        let inverses = v.to("inverse binds", at + 0x30)?;
        let locals = v.ptr("local binds", at + 0x38)?;
        v.get("bones", records, count * BONE_LEN)?;
        v.get("inverse binds", inverses, count * MATRIX_LEN)?;
        if let Some(locals) = locals {
            v.get("local binds", locals, count * MATRIX_LEN)?;
        }
        Ok((records, inverses, locals))
    })?;
    let mut moved = vec![false; count];
    let mut writes = Vec::new();
    for (i, b) in edited.bones.iter().enumerate() {
        let under = changed[i] || b.parent.is_some_and(|p| moved[usize::from(p)]);
        moved[i] = under;
        if under {
            writes.push((i, inverse(&worlds[i])?));
        }
    }
    for (i, inverse) in writes {
        let b = &edited.bones[i];
        if changed[i] {
            let at = records + BONE_LEN * i;
            put(virtual_section, at, &b.rotation)?;
            put(virtual_section, at + 0x10, &b.translation)?;
            put(virtual_section, at + 0x20, &b.scale)?;
            if let Some(locals) = locals {
                put_matrix(virtual_section, locals + MATRIX_LEN * i, &local(b))?;
            }
        }
        put_matrix(virtual_section, inverses + MATRIX_LEN * i, &inverse)?;
    }
    read_skeleton_of(virtual_section, owner).map(|_| ())
}
