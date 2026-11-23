use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{InstalledMod, ModPlace};
use crate::vfs::{FileDevice, Vfs, VfsPath};

const MOUNT: &str = "mods";

/// ⚠ Not a real path: a DLC pack's content is one namespace across packs, whatever the pack's own name.
const DLC_NAMESPACE: &str = "<dlc>/";

/// What two mods are both writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClaimKind {
    /// A file at the same path.
    File,
    /// A file inside an archive laid out as a folder named for it, `x64a.rpf/…`.
    ArchiveEntry,
    /// A file at the same path inside two DLC packs' archives.
    DlcContent,
}

/// Mods that write the same paths, and the one the VFS resolves them to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ClaimKind,
    /// The mods writing every one of `targets`, in load order.
    pub mods: Vec<String>,
    /// The mod the stack resolves `targets` to: the last of `mods`.
    pub winner: String,
    /// Sorted, spelled as the winner spells them.
    pub targets: Vec<String>,
}

/// An archive a mod carries that could not be looked into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    pub mod_name: String,
    pub archive: String,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    pub conflicts: Vec<Conflict>,
    pub unread: Vec<Unread>,
}

impl Resolution {
    /// ⚠ An archive that could not be read makes `conflicts` a short list that looks exhaustive.
    pub fn is_complete(&self) -> bool {
        self.unread.is_empty()
    }
}

/// One mod's claim on a path: how it is keyed in the stack, what it is, and how the mod spells it.
struct Claim {
    kind: ClaimKind,
    shown: String,
}

/// A mod's claims, by the lowercase key the stack looks a path up by.
fn claims_of(m: &InstalledMod) -> BTreeMap<String, Claim> {
    let mut out = BTreeMap::new();
    for file in &m.files {
        let inside_archive = VfsPath::parse(&format!("{MOUNT}:/{file}"))
            .is_some_and(|path| path.is_inside_archive());
        let kind = if inside_archive {
            ClaimKind::ArchiveEntry
        } else {
            ClaimKind::File
        };
        out.insert(
            file.to_ascii_lowercase(),
            Claim {
                kind,
                shown: file.clone(),
            },
        );
    }
    for entry in m.packs.iter().flat_map(|pack| &pack.entries) {
        out.insert(
            format!("{DLC_NAMESPACE}{}", entry.to_ascii_lowercase()),
            Claim {
                kind: ClaimKind::DlcContent,
                shown: entry.clone(),
            },
        );
    }
    out
}

/// A mod's claims as a device: it answers for the paths it writes and holds no bytes.
#[derive(Debug)]
struct ClaimDevice {
    name: String,
    keys: BTreeSet<String>,
}

impl FileDevice for ClaimDevice {
    fn open(&self, _relative_path: &str) -> Option<Vec<u8>> {
        None
    }

    fn exists(&self, relative_path: &str) -> bool {
        self.keys.contains(&relative_path.to_ascii_lowercase())
    }

    fn locate(&self, relative_path: &str) -> Option<String> {
        self.exists(relative_path).then(|| self.name.clone())
    }
}

/// Stack the enabled mods of `mods`, in the order given, and report every path two or more write.
///
/// The winner of each is what the VFS answers for it, which is the newest
/// device: the last enabled mod writing it.
pub fn resolve(mods: &[InstalledMod]) -> Resolution {
    let enabled: Vec<&InstalledMod> = mods
        .iter()
        .filter(|m| m.place == ModPlace::Overlay)
        .collect();
    let claims: Vec<BTreeMap<String, Claim>> = enabled.iter().map(|m| claims_of(m)).collect();

    let mut vfs = Vfs::new();
    let mut claimed_by: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, (m, owned)) in enabled.iter().zip(&claims).enumerate() {
        vfs.mount(
            MOUNT,
            Arc::new(ClaimDevice {
                name: m.name.clone(),
                keys: owned.keys().cloned().collect(),
            }),
        );
        for key in owned.keys() {
            claimed_by.entry(key).or_default().push(index);
        }
    }

    // Paths the same mods write, of one kind, are one conflict: who wins is a function of who writes them.
    let mut groups: BTreeMap<(ClaimKind, Vec<usize>), (String, Vec<String>)> = BTreeMap::new();
    for (key, writers) in claimed_by.iter().filter(|(_, w)| w.len() > 1) {
        let Some(winner) = vfs.locate(&format!("{MOUNT}:/{key}")) else {
            continue;
        };
        let Some(claim) = writers
            .iter()
            .find(|&&i| enabled[i].name == winner)
            .map(|&i| &claims[i][*key])
        else {
            continue;
        };
        let (_, targets) = groups
            .entry((claim.kind, writers.clone()))
            .or_insert_with(|| (winner, Vec::new()));
        targets.push(claim.shown.clone());
    }

    let conflicts = groups
        .into_iter()
        .map(|((kind, writers), (winner, targets))| Conflict {
            kind,
            mods: writers.iter().map(|&i| enabled[i].name.clone()).collect(),
            winner,
            targets,
        })
        .collect();

    let unread = enabled
        .iter()
        .flat_map(|m| {
            m.packs.iter().filter_map(|pack| {
                Some(Unread {
                    mod_name: m.name.clone(),
                    archive: pack.archive.clone(),
                    why: pack.unread.clone()?,
                })
            })
        })
        .collect();

    Resolution { conflicts, unread }
}
