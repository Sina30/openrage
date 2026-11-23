//! A skeleton's bones renamed: each new name written at the end of the virtual section, which grows
//! by the resource rule, and its record's pointer at +0x30 aimed at it. A bone keeps the hash at
//! +0x1C, which a clip's tracks and a geometry's palette find it by, and its old text stays as bytes
//! the record no longer names.

use super::bytes::sections;
use super::skeleton::{owner_at, read_skeleton_of, SkeletonOwner};
use super::write_material::based;
use super::{HeaderlessError, Result, Skeleton};
use crate::container::rpf::{grow_section, Rpf9GrowError, Rpf9Section, Rpf9Sections};

/// The base of a virtual-section pointer.
const VIRTUAL: u64 = 0x5000_0000;
const BONE_LEN: usize = 0x50;

/// Why a skeleton's names are not written.
#[derive(Debug, thiserror::Error)]
pub enum BoneNameError {
    #[error(transparent)]
    Read(#[from] HeaderlessError),
    #[error(transparent)]
    Grow(#[from] Rpf9GrowError),
    #[error("bone {bone} is not renamed: {why}")]
    Refused { bone: usize, why: &'static str },
}

/// Whether `name` can be written as a bone's name: text of one byte or more with no NUL.
pub fn bone_name_holds(name: &str) -> std::result::Result<(), &'static str> {
    if name.is_empty() {
        return Err("a bone's name holds at least one character");
    }
    if name.contains('\0') {
        return Err("a bone's name ends at its first NUL, so it holds none");
    }
    Ok(())
}

/// `bytes` over `v` at `at`.
fn put(v: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    let len = v.len() as u64;
    let to = (at.checked_add(bytes.len()))
        .and_then(|end| v.get_mut(at..end))
        .ok_or(HeaderlessError::PastEnd {
            what: "renamed bone",
            at: at as u64,
            section: "virtual",
            len,
        })?;
    to.copy_from_slice(bytes);
    Ok(())
}

/// `owner`'s skeleton (`true` a fragment's, `false` the root drawable's) given `edited`'s names.
// Every other value of a bone is left to `write_skeleton`, which holds the names to the record's.
pub fn rename_bones(
    s: &mut Rpf9Sections,
    owner: impl Into<SkeletonOwner>,
    edited: &Skeleton,
) -> std::result::Result<(), BoneNameError> {
    let owner = owner.into();
    let refuse = HeaderlessError::NotInPlace;
    let read = read_skeleton_of(&s.virtual_section, owner)?
        .ok_or(refuse("the drawable's skeleton pointer is null"))?;
    if read.bones.len() != edited.bones.len() {
        return Err(refuse("the skeleton holds another number of bones").into());
    }
    let mut renamed: Vec<(usize, &[u8])> = Vec::new();
    for (bone, (r, e)) in read.bones.iter().zip(&edited.bones).enumerate() {
        if (r.hash, r.parent) != (e.hash, e.parent) {
            let why = "a rename keeps the bone's hash and its parent";
            return Err(BoneNameError::Refused { bone, why });
        }
        if r.name != e.name {
            bone_name_holds(&e.name).map_err(|why| BoneNameError::Refused { bone, why })?;
            renamed.push((bone, e.name.as_bytes()));
        }
    }
    if renamed.is_empty() {
        return Ok(());
    }
    let mut text = Vec::new();
    let mut starts = Vec::with_capacity(renamed.len());
    for (_, name) in &renamed {
        starts.push(text.len());
        text.extend_from_slice(name);
        text.push(0);
    }
    text.resize(text.len().next_multiple_of(16), 0);
    let mut grown = s.clone();
    let pointers = based(&grown);
    let at = grown.virtual_section.len();
    let base = grow_section(
        &mut grown,
        &pointers,
        Rpf9Section::Virtual,
        at,
        text.len() as isize,
    )?;
    // The block map's slots may have grown before the names, so the records are found again.
    let records = sections(&grown.virtual_section, None, |v, _| {
        let drawable = owner_at(v, owner)?;
        let skeleton = v.to("skeleton", drawable + 0x18)?;
        v.to("bones", skeleton + 0x28)
    })?;
    let v = &mut grown.virtual_section;
    put(v, base, &text)?;
    for (&(bone, _), &start) in renamed.iter().zip(&starts) {
        let word = VIRTUAL + (base + start) as u64;
        put(v, records + BONE_LEN * bone + 0x30, &word.to_le_bytes())?;
    }
    let back = read_skeleton_of(&grown.virtual_section, owner)?
        .ok_or(refuse("the drawable's skeleton pointer is null"))?;
    let names = |k: &Skeleton| k.bones.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    if names(&back) != names(edited) {
        return Err(refuse("the written skeleton does not read back with the names given").into());
    }
    *s = grown;
    Ok(())
}
