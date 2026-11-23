//! Frozen positions: the lock that keeps a pack from renumbering what it has built.
//!
//! A build that succeeds writes `wardrobe.lock.json` beside `wardrobe.json`: the drawable of every garment, by its
//! stable id. From then on a garment that the allocator would put at another drawable is the error `renumbered`,
//! which stops a build until the garment is pinned back, a dummy holds the place of the garment that left, or a
//! waiver says why. Saved outfits, shop tables and scripts name a drawable by its number, and a number that
//! quietly follows the garment before it out of the pack changes what they show.
//!
//! The lock is a text file for Git, canonical like every source: keys sorted, two-space indent, LF.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::alloc::{Allocation, Placed};
use crate::model::{Garment, Wardrobe};
use crate::names::{Ped, Slot};

/// The file a pack keeps its lock in.
pub const FILE: &str = "wardrobe.lock.json";
pub const FORMAT: &str = "openrage-wardrobe-lock";
pub const SCHEMA: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0} is not a lock: {1}")]
    Json(String, serde_json::Error),
    #[error("{0} is not a lock: its format is `{1}`, not `{FORMAT}`")]
    Format(String, String),
    #[error(
        "{0} was written by a newer OpenRage (schema {1}) than this one reads (schema {SCHEMA})"
    )]
    Newer(String, u32),
}

/// Where a garment was when the pack was last built, as the game sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locked {
    /// The garment's stable id.
    pub id: String,
    pub ped: Ped,
    pub slot: Slot,
    /// Which collection of the ped held it: 0 is the pack's own and n is `<collection>_<n>`.
    pub part: u32,
    /// Its drawable number in that collection.
    pub position: u32,
}

/// The drawables a pack built, which a later build must not move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    pub format: String,
    pub schema: u32,
    pub places: Vec<Locked>,
}

/// A garment the lock holds at one drawable and the allocator puts at another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renumbered {
    /// Its index in the pack's list.
    pub garment: usize,
    pub was: Locked,
    pub now: Placed,
}

impl Lock {
    /// The lock of `wardrobe` as it stands: every garment that has a place, a place held for a dummy included.
    pub fn of(wardrobe: &Wardrobe, allocation: &Allocation) -> Self {
        let mut places: Vec<Locked> = allocation
            .placed
            .iter()
            // ⚠ The files of a game collection are the game's own numbers, which no pack moves.
            .filter(|p| wardrobe.replaces(p.ped).is_none())
            .map(|p| Locked {
                id: wardrobe.garments[p.garment].id.clone(),
                ped: p.ped,
                slot: p.slot,
                part: p.part,
                position: p.position,
            })
            .collect();
        places.sort_by(|a, b| {
            (a.ped, a.slot, a.part, a.position, &a.id)
                .cmp(&(b.ped, b.slot, b.part, b.position, &b.id))
        });
        Self {
            format: FORMAT.to_string(),
            schema: SCHEMA,
            places,
        }
    }

    pub fn from_json(bytes: &[u8], shown: &str) -> Result<Self, Error> {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| Error::Json(shown.to_string(), e))?;
        if let Some(format) = value["format"].as_str().filter(|f| *f != FORMAT) {
            return Err(Error::Format(shown.to_string(), format.to_string()));
        }
        if let Some(schema) = value["schema"].as_u64().filter(|s| *s > u64::from(SCHEMA)) {
            return Err(Error::Newer(
                shown.to_string(),
                u32::try_from(schema).unwrap_or(u32::MAX),
            ));
        }
        serde_json::from_value(value).map_err(|e| Error::Json(shown.to_string(), e))
    }

    /// The canonical text of the file.
    pub fn to_json(&self) -> String {
        let value = serde_json::to_value(self).expect("a lock is JSON");
        let mut text = serde_json::to_string_pretty(&value).expect("a JSON value serialises");
        text.push('\n');
        text
    }

    /// The garments of `wardrobe` that the lock holds at one drawable and `allocation` puts at another.
    ///
    /// A garment that is not in the pack any more is not here, and neither is one the allocator gives no place: the rules
    /// that name those say so.
    pub fn renumbered(&self, wardrobe: &Wardrobe, allocation: &Allocation) -> Vec<Renumbered> {
        let mut moved = Vec::new();
        for was in &self.places {
            let Some(index) = wardrobe.garments.iter().position(|g| g.id == was.id) else {
                continue;
            };
            let Some(now) = allocation.of(index) else {
                continue;
            };
            if (now.ped, now.slot, now.part, now.position)
                != (was.ped, was.slot, was.part, was.position)
            {
                moved.push(Renumbered {
                    garment: index,
                    was: was.clone(),
                    now: *now,
                });
            }
        }
        moved.sort_by_key(|m| m.garment);
        moved
    }

    /// The place in its slot that gives garment `was` the drawable it had, under the cap the pack has now.
    pub fn place_of(&self, wardrobe: &Wardrobe, was: &Locked) -> u32 {
        was.part * wardrobe.rules.slot_cap() + was.position
    }
}

/// The lock in `folder`, none where the pack has not been built since it could be frozen.
pub fn read(folder: &Path) -> Result<Option<Lock>, Error> {
    let path = folder.join(FILE);
    let shown = path.display().to_string();
    match std::fs::read(&path) {
        Ok(bytes) => Lock::from_json(&bytes, &shown).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(shown, e)),
    }
}

/// What [`keep`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// The pack's rules say not to freeze, which takes a lock it had away, or it has no garment with a place.
    Off,
    /// The lock already said so.
    Same,
    /// The lock was written: for the first time, or again after a waived renumbering.
    Wrote { first: bool, places: usize },
}

/// Write the lock of the pack in `folder` as a build that succeeded left it.
///
/// ⚠ Only for a pack this window may write, and only after a build that went through: a build that stopped on a
/// renumbering must not make the renumbering the new lock.
pub fn keep(folder: &Path) -> Result<Kept, crate::model::Error> {
    let wardrobe = Wardrobe::read(folder)?;
    if !wardrobe.rules.freezes() {
        // ⚠ A lock the pack no longer wants goes, so that switching freezing on again is not held to drawables from long ago.
        thaw(folder)?;
        return Ok(Kept::Off);
    }
    let allocation = crate::alloc::allocate(&wardrobe);
    let lock = Lock::of(&wardrobe, &allocation);
    if lock.places.is_empty() {
        return Ok(Kept::Off);
    }
    if wardrobe.lock.as_ref() == Some(&lock) {
        return Ok(Kept::Same);
    }
    let path = folder.join(FILE);
    let first = wardrobe.lock.is_none();
    let places = lock.places.len();
    or_project::content::folder::write_atomic(&path, &lock.to_json())
        .map_err(|e| crate::model::Error::Io(path.display().to_string(), e))?;
    Ok(Kept::Wrote { first, places })
}

/// Freeze each clothing content that build `output` of the project took, after the build went through.
///
/// A content that is linked is read here and written by the project that holds it, so it is left to that project. The pack
/// is read as it stands now, which is the pack the build was made from unless somebody edited it while the build ran.
pub fn keep_for(
    container: &or_project::Container,
    project_folder: &Path,
    output: &str,
) -> Vec<(String, Result<Kept, crate::model::Error>)> {
    let Some(declared) = container.output(output) else {
        return Vec::new();
    };
    (container.modules.iter())
        .filter(|m| m.kind == or_project::ModuleKind::Clothing && !m.is_link())
        .filter(|m| declared.take.contains(&m.id))
        .map(|module| {
            let kept = crate::plan::module_folder(project_folder, module)
                .map_err(|e| {
                    crate::model::Error::Io(module.id.clone(), std::io::Error::other(e.to_string()))
                })
                .and_then(|folder| keep(&folder));
            (module.id.clone(), kept)
        })
        .collect()
}

/// Take the lock away, so the pack renumbers freely until its next build.
pub fn thaw(folder: &Path) -> Result<bool, crate::model::Error> {
    let path = folder.join(FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(crate::model::Error::Io(path.display().to_string(), e)),
    }
}

/// The sentence a finding of `renumbered` says about `garment`.
pub fn sentence(wardrobe: &Wardrobe, garment: &Garment, moved: &Renumbered) -> String {
    let was = &moved.was;
    let (was_at, now_at) = (
        format!(
            "{} {:03} of {}",
            was.slot,
            was.position,
            wardrobe.collection_of(was.ped, was.part)
        ),
        format!(
            "{} {:03} of {}",
            moved.now.slot,
            moved.now.position,
            wardrobe.collection_of(moved.now.ped, moved.now.part)
        ),
    );
    format!(
        "{} was {was_at} when the pack was last built and is {now_at} now, so a saved outfit, a shop table or a script that names the old drawable would show another garment",
        crate::rules::name_of(garment)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::Component;

    #[test]
    fn a_lock_is_written_canonically_and_reads_back_equal() {
        let lock = Lock {
            format: FORMAT.to_string(),
            schema: SCHEMA,
            places: vec![Locked {
                id: "a".into(),
                ped: Ped::Male,
                slot: Slot::Component(Component::Jbib),
                part: 1,
                position: 2,
            }],
        };
        let text = lock.to_json();
        assert!(text.ends_with("}\n") && !text.contains('\r'));
        assert_eq!(Lock::from_json(text.as_bytes(), "x").unwrap(), lock);
        assert_eq!(
            Lock::from_json(text.as_bytes(), "x").unwrap().to_json(),
            text
        );
    }

    #[test]
    fn a_file_of_another_format_or_a_newer_schema_is_not_read() {
        let other = br#"{"format":"x","schema":1,"places":[]}"#;
        assert!(matches!(
            Lock::from_json(other, "f"),
            Err(Error::Format(..))
        ));
        let newer = format!(
            r#"{{"format":"{FORMAT}","schema":{},"places":[]}}"#,
            SCHEMA + 1
        );
        assert!(matches!(
            Lock::from_json(newer.as_bytes(), "f"),
            Err(Error::Newer(..))
        ));
    }
}
