//! The versions of a pack: `wardrobe.json` as every write left it, kept in the pack's private folder, and what brings one back.
//!
//! A write of the pack ([`Wardrobe::write`]) keeps what it wrote as a version, so a rotating set of the last
//! [`KEPT`] is always beside the pack and a damaged or mistaken `wardrobe.json` costs one version and not the pack.
//! The folder is `.openrage` in the pack's folder, which ignores itself, and the manifest is only ever replaced by
//! the atomic write: a version is a copy beside it and never the file itself.
//!
//! ⚠ **Best effort.** A version that cannot be kept is logged and the write stands: a full disk must not stop an edit.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use or_project::snapshot::{Snapshots, PRIVATE_DIR};

use crate::edit::Error;
use crate::model::{Garment, Wardrobe, FILE};
use crate::rules::name_of;

/// How many versions a pack keeps; the oldest go first.
pub const KEPT: usize = 50;

const VERSIONS: &str = "versions";
const UNREADABLE: &str = "unreadable";

/// The private folder of a pack's folder: what belongs to this machine and never to a repository.
pub fn private_dir(folder: &Path) -> PathBuf {
    folder.join(PRIVATE_DIR)
}

/// Make the private folder, with a `.gitignore` that ignores everything in it, and give its path.
pub(crate) fn make_private(folder: &Path) -> io::Result<PathBuf> {
    let dir = private_dir(folder);
    std::fs::create_dir_all(&dir)?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, b"*\n")?;
    }
    Ok(dir)
}

/// One version of a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// The number it is asked for by; never reused.
    pub n: u64,
    /// What the write that made it changed, in a few words.
    pub label: String,
    pub at: SystemTime,
}

fn store(folder: &Path) -> Snapshots {
    Snapshots::open(&private_dir(folder).join(VERSIONS)).keeping(KEPT)
}

/// The versions the pack keeps, oldest first.
pub fn versions(folder: &Path) -> Vec<Version> {
    (store(folder).steps().iter())
        .map(|step| Version {
            n: step.n,
            label: step.label.clone(),
            at: step.at(),
        })
        .collect()
}

/// The pack as version `n` held it.
pub fn read_version(folder: &Path, n: u64) -> Result<Wardrobe, Error> {
    let store = store(folder);
    let at = (store.steps().iter())
        .position(|step| step.n == n)
        .ok_or_else(|| Error::Refused("that version is not kept any more".into()))?;
    let bytes = store
        .read(at)
        .map_err(|e| Error::Io(format!("version {n}"), e))?;
    Ok(Wardrobe::from_json(&bytes, &format!("version {n}"))?)
}

/// Keep `after`, which was just written over `before`, as a version; an `before` that is not the newest version is kept first, as a change made outside OpenRage.
///
/// ⚠ Nothing is kept twice: bytes the newest version already holds add nothing.
pub(crate) fn keep(folder: &Path, before: Option<&[u8]>, after: &Wardrobe, text: &str) {
    let mut store = store(folder);
    let newest = |store: &Snapshots| {
        store
            .steps()
            .len()
            .checked_sub(1)
            .and_then(|last| store.read(last).ok())
    };
    let mut held = newest(&store);
    let prior = before.and_then(|bytes| Wardrobe::from_json(bytes, "the pack").ok());
    let mut pushed = |label: &str, bytes: &[u8]| {
        if let Err(why) = make_private(folder).and_then(|_| store.push(label, bytes).map(drop)) {
            tracing::warn!(%why, "a version of the pack was not kept");
        }
    };
    if let (Some(bytes), Some(_)) = (before, &prior) {
        if held.as_deref() != Some(bytes) {
            let label = if held.is_none() {
                "The pack as it was found"
            } else {
                "Changed outside OpenRage"
            };
            pushed(label, bytes);
            held = Some(bytes.to_vec());
        }
    }
    if held.as_deref() != Some(text.as_bytes()) {
        pushed(&summary(prior.as_ref(), after), text.as_bytes());
    }
}

/// What a write changed, in a few words: the garments added, removed or changed, the waivers, the pack's own settings.
pub fn summary(before: Option<&Wardrobe>, after: &Wardrobe) -> String {
    let Some(before) = before else {
        return "Saved".into();
    };
    let held = |w: &Wardrobe, g: &Garment| w.garments.iter().any(|h| h.id == g.id);
    let added = after.garments.iter().filter(|g| !held(before, g)).count();
    let removed = before.garments.iter().filter(|g| !held(after, g)).count();
    let changed: Vec<&Garment> = (after.garments.iter())
        .filter(|g| before.garment(&g.id).is_some_and(|was| was != *g))
        .collect();
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("Added {added} garment(s)"));
    }
    if removed > 0 {
        parts.push(format!("Removed {removed} garment(s)"));
    }
    match changed.as_slice() {
        [] => {}
        [one] => parts.push(format!("Changed {}", name_of(one))),
        many => parts.push(format!("Changed {} garments", many.len())),
    }
    if before.waivers != after.waivers {
        parts.push("Changed the waivers".into());
    }
    if before.queries != after.queries {
        parts.push(
            match after.queries.len().cmp(&before.queries.len()) {
                std::cmp::Ordering::Greater => "Saved a query",
                std::cmp::Ordering::Less => "Forgot a query",
                std::cmp::Ordering::Equal => "Changed a saved query",
            }
            .into(),
        );
    }
    if before.pack != after.pack {
        parts.push("Renamed the pack".into());
    } else if (before.shop != after.shop) || (before.collections != after.collections) {
        parts.push("Changed the pack's settings".into());
    }
    match parts.is_empty() {
        true => "Saved".into(),
        false => parts.join("; "),
    }
}

/// The newest version that reads, and the pack it holds.
pub fn last_good(folder: &Path) -> Option<(Version, Wardrobe)> {
    versions(folder)
        .into_iter()
        .rev()
        .find_map(|version| Some((version.clone(), read_version(folder, version.n).ok()?)))
}

/// The newest kept version that is not `current`, and the pack it holds: what the pack was before its latest change.
pub fn previous(folder: &Path, current: &Wardrobe) -> Option<(Version, Wardrobe)> {
    versions(folder).into_iter().rev().find_map(|version| {
        let pack = read_version(folder, version.n).ok()?;
        (pack != *current).then_some((version, pack))
    })
}

/// Put the last version that reads in place of a `wardrobe.json` that does not; what was there is kept in `.openrage/unreadable`, so a typo can still be mended by hand.
pub fn restore_last_good(folder: &Path) -> Result<Version, Error> {
    let (version, pack) = last_good(folder)
        .ok_or_else(|| Error::Refused("no earlier version of this pack reads".into()))?;
    let path = folder.join(FILE);
    if let Ok(bytes) = std::fs::read(&path) {
        let at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let kept = make_private(folder)
            .map(|dir| dir.join(UNREADABLE))
            .and_then(|dir| std::fs::create_dir_all(&dir).map(|()| dir))
            .and_then(|dir| std::fs::write(dir.join(format!("wardrobe-{at}.json")), bytes));
        kept.map_err(|e| Error::Io(path.display().to_string(), e))?;
    }
    pack.write(folder)?;
    Ok(version)
}

/// Make the pack what version `pack` held it.
pub fn restore_pack(wardrobe: &mut Wardrobe, version: &Wardrobe) {
    wardrobe.clone_from(version);
}

/// Put garment `id` back to what `version` held of it, and no other garment: the pack's other garments, its waivers and its settings stay as they are.
pub fn roll_back_garment(
    wardrobe: &mut Wardrobe,
    version: &Wardrobe,
    id: &str,
) -> Result<(), Error> {
    let then = version
        .garment(id)
        .ok_or_else(|| Error::Refused("that version did not have this garment".into()))?;
    let now = (wardrobe.garments.iter_mut())
        .find(|g| g.id == id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    now.clone_from(then);
    Ok(())
}

/// Put texture `variation` of garment `id` back to what `version` held of it, and no other texture or garment.
pub fn roll_back_variation(
    wardrobe: &mut Wardrobe,
    version: &Wardrobe,
    id: &str,
    variation: usize,
) -> Result<(), Error> {
    let then = (version.garment(id))
        .and_then(|g| g.variations.get(variation))
        .ok_or_else(|| Error::Refused("that version did not have this texture".into()))?;
    let now = (wardrobe.garments.iter_mut())
        .find(|g| g.id == id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let held = now.variations.len();
    let at = now.variations.get_mut(variation).ok_or_else(|| {
        Error::Refused(format!(
            "the garment has {held} texture(s), not {}",
            variation + 1
        ))
    })?;
    at.clone_from(then);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_query_kept_or_forgotten_is_said_in_the_label_of_the_version() {
        let before = Wardrobe::new("demo");
        let mut after = before.clone();
        after.queries.push(crate::query::SavedQuery {
            name: "Heavy tops".into(),
            query: "slot:jbib polys>3000".into(),
        });
        assert_eq!(summary(Some(&before), &after), "Saved a query");
        assert_eq!(summary(Some(&after), &before), "Forgot a query");
        let mut renamed = after.clone();
        renamed.queries[0].query = "slot:lowr".into();
        assert_eq!(summary(Some(&after), &renamed), "Changed a saved query");
    }
}
