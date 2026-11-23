//! A drawable's skeleton: its bones, each with its name, the name's hash an animation's DOF names
//! it by, its parent and its local bind transform.

use super::bytes::{f32_at, sections, u32_at, Bytes};
use super::{HeaderlessError, Result};

const BONE_LEN: usize = 0x50;
const SKELETON_LEN: usize = 0x70;

#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    /// In the order the skeleton stores them, each parent before its children.
    pub bones: Vec<Bone>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    /// The 32-bit hash a DOF's id carries for the bone it moves: its name's, until it is renamed.
    pub hash: u32,
    /// `None` for a root: a skeleton can hold several.
    pub parent: Option<u16>,
    /// A unit quaternion, x, y, z, w.
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    /// The inverse of the bone's bind world: the x, y and z of its four columns, the last its translation.
    pub inverse_bind: [[f32; 3]; 4],
}

/// The drawable whose skeleton pointer, at its 0x18, a reader or a writer follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkeletonOwner {
    /// The drawable at the root of the virtual section.
    Drawable,
    /// A fragment's drawable, the pointer at 0x18 of the root.
    Fragment,
    /// The drawable entry of the dictionary at the root that this code keys.
    Entry(u32),
}

impl From<bool> for SkeletonOwner {
    fn from(fragment: bool) -> Self {
        match fragment {
            true => Self::Fragment,
            false => Self::Drawable,
        }
    }
}

/// Where `owner`'s drawable starts in `v`.
pub(super) fn owner_at(v: Bytes, owner: SkeletonOwner) -> Result<usize> {
    let code = match owner {
        SkeletonOwner::Drawable => return Ok(0),
        SkeletonOwner::Fragment => return v.to("drawable", 0x18),
        SkeletonOwner::Entry(code) => code,
    };
    let count = usize::from(v.u16("entry count", 0x38)?);
    let codes = v.get("codes", v.to("codes", 0x20)?, 4 * count)?;
    let entries = v.to("entries", 0x30)?;
    let i = (0..count)
        .find(|&i| u32_at(codes, 4 * i) == code)
        .ok_or(HeaderlessError::Layout {
            shape: "dictionary entry asked for",
            why: "no entry of the dictionary has its code",
        })?;
    let at = v.to("entry", entries + 8 * i)?;
    if !super::drawable::is_drawable_root(v, at, false) {
        return Err(HeaderlessError::NotADrawable { at: at as u64 });
    }
    Ok(at)
}

/// The skeleton of `owner`'s drawable in `virtual_section`; `None` where its 0x18 is null.
pub fn read_skeleton_of(virtual_section: &[u8], owner: SkeletonOwner) -> Result<Option<Skeleton>> {
    sections(virtual_section, None, |v, _| {
        let drawable = owner_at(v, owner)?;
        v.ptr("skeleton", drawable + 0x18)?
            .map(|at| skeleton_at(v, at))
            .transpose()
    })
}

/// The skeleton of the drawable at the root of `virtual_section`, or of a fragment's drawable
/// (the pointer at 0x18) where `fragment`; `None` where the drawable's 0x18 is null.
pub fn read_skeleton(virtual_section: &[u8], fragment: bool) -> Result<Option<Skeleton>> {
    read_skeleton_of(virtual_section, fragment.into())
}

/// The skeleton of each drawable entry of the dictionary at the root of `virtual_section`, by code.
pub fn read_dictionary_skeletons(virtual_section: &[u8]) -> Result<Vec<(u32, Skeleton)>> {
    sections(virtual_section, None, |v, _| {
        let count = usize::from(v.u16("entry count", 0x38)?);
        let codes = v.array("codes", v.to("codes", 0x20)?, count, 4)?;
        let entries = v.to("entries", 0x30)?;
        v.array("entries", entries, count, 8)?;
        let mut skeletons = Vec::new();
        for i in 0..count {
            let drawable = v.to("entry", entries + 8 * i)?;
            if !super::drawable::is_drawable_root(v, drawable, false) {
                continue;
            }
            if let Some(at) = v.ptr("skeleton", drawable + 0x18)? {
                skeletons.push((u32_at(codes, 4 * i), skeleton_at(v, at)?));
            }
        }
        Ok(skeletons)
    })
}

/// The skeleton at `at` of `virtual_section`, laid out as a drawable's.
pub fn read_skeleton_at(virtual_section: &[u8], at: usize) -> Result<Skeleton> {
    sections(virtual_section, None, |v, _| skeleton_at(v, at))
}

/// The skeleton at `at`: its bone count at 0x6C and again at 0x6E, its bones at the pointer at 0x28
/// and their inverse binds, 0x40 bytes each, at the pointer at 0x30.
pub(super) fn skeleton_at(v: Bytes, at: usize) -> Result<Skeleton> {
    v.get("skeleton", at, SKELETON_LEN)?;
    let count = v.u16("bone count", at + 0x6C)?;
    let again = v.u16("bone count", at + 0x6E)?;
    if count != again {
        return Err(HeaderlessError::Disagree {
            what: "skeleton's two bone counts",
            first: count.into(),
            second: again.into(),
        });
    }
    let count = usize::from(count);
    let (bones_at, inverses_at) = match count {
        0 => (0, 0),
        _ => (v.to("bones", at + 0x28)?, v.to("inverse binds", at + 0x30)?),
    };
    let records = v.array("bones", bones_at, count, BONE_LEN)?;
    let inverses = v.array("inverse binds", inverses_at, count, 0x40)?;
    let mut bones = Vec::with_capacity(count);
    for (i, (record, inverse)) in (records.as_chunks::<BONE_LEN>().0.iter())
        .zip(inverses.as_chunks::<0x40>().0)
        .enumerate()
    {
        let index = u16::from_le_bytes([record[0x48], record[0x49]]);
        if usize::from(index) != i {
            return Err(HeaderlessError::Disagree {
                what: "bone's stated index and its place",
                first: index.into(),
                second: i as u64,
            });
        }
        let parent = i16::from_le_bytes([record[0x46], record[0x47]]);
        let parent = match u16::try_from(parent) {
            Ok(p) if usize::from(p) < i => Some(p),
            Err(_) if parent == -1 => None,
            _ => {
                return Err(HeaderlessError::IndexPastCount {
                    what: "bone's parent, which comes before it",
                    index: u64::from(parent as u16),
                    count: i as u64,
                })
            }
        };
        let name = v.c_str(
            "bone name",
            v.to("bone name", bones_at + i * BONE_LEN + 0x30)?,
        )?;
        // A renamed bone keeps the hash it is found by, so the hash is not checked against the name.
        let hash = u32_at(record, 0x1C);
        bones.push(Bone {
            name,
            hash,
            parent,
            rotation: std::array::from_fn(|k| f32_at(record, 4 * k)),
            translation: std::array::from_fn(|k| f32_at(record, 0x10 + 4 * k)),
            scale: std::array::from_fn(|k| f32_at(record, 0x20 + 4 * k)),
            inverse_bind: std::array::from_fn(|c| {
                std::array::from_fn(|r| f32_at(inverse, 16 * c + 4 * r))
            }),
        });
    }
    Ok(Skeleton { bones })
}
