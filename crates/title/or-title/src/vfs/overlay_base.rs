//! What an overlay copy of an archive was made from, and whether the install's archive has changed since.
//!
//! The copy is the user's and a save rebuilds it whole, so staleness is read from the install's own
//! archive against a record taken when the copy was made; `an_overlay_copy_the_title_has_outgrown`
//! says why the record is the original's size and time plus its entry table.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::container::rpf::RpfArchive;
use serde::{Deserialize, Serialize};

use super::game_mount::facts_for;
use super::overlay::{install_path_for, overlay_root};

/// What an archive's entry table says about one file: everything a patch that replaces it moves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMark {
    pub path: String,
    pub stored: u32,
    pub size: u32,
    pub compressed: bool,
    pub paging: Option<(u32, u32)>,
}

/// The install's archive as it stood when its overlay copy was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseRecord {
    pub size: u64,
    /// Nanoseconds since the Unix epoch; 0 where the file system gave none.
    pub modified: u64,
    /// Every file, by path.
    pub entries: Vec<EntryMark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    Fresh,
    /// The install's archive is not the one the copy was made from.
    Stale,
    /// The install's archive is gone or unreadable, and its size does not say.
    Unchecked,
}

/// An overlay copy whose original the title has since changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleCopy {
    pub copy: PathBuf,
    pub base: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Edited,
    Added,
    Removed,
}

/// One change of the user's that a refresh would throw away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discard {
    pub path: String,
    pub change: Change,
}

impl Discard {
    pub fn new(path: &str, change: Change) -> Self {
        Self {
            path: path.to_string(),
            change,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshPlan {
    /// Changes only the copy made, sorted by path.
    pub yours: Vec<Discard>,
    /// Files the update changed too: the copy's version is replaced by the update's, whoever else edited it.
    pub by_update: Vec<String>,
}

/// Where the record of `copy`'s original lives under `records_dir`.
pub fn record_file(records_dir: &Path, copy: &Path) -> PathBuf {
    let key = copy
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let name = copy.file_name().map(|n| n.to_string_lossy().into_owned());
    records_dir.join(format!(
        "{}-{:08x}.json",
        name.unwrap_or_default(),
        or_io::desc::plan::fnv1a(&key)
    ))
}

fn open(path: &Path, crypto: &Arc<dyn RpfCryptoProvider>) -> Result<RpfArchive, String> {
    RpfArchive::open_with(path, Arc::clone(crypto))
        .map_err(|e| format!("Read `{}`: {e}", path.display()))
}

/// Every file of `archive`, by path, with its entry index.
fn files_of(archive: &RpfArchive) -> Vec<(EntryMark, u32)> {
    let mut files: Vec<(EntryMark, u32)> = (archive.nodes.iter())
        .filter(|node| !node.is_dir())
        .filter_map(|node| {
            let facts = facts_for(archive, node.entry_index(), node.path().to_string(), "", "")?;
            let mark = EntryMark {
                path: facts.logical,
                stored: facts.stored_size,
                size: facts.size,
                compressed: facts.compressed,
                paging: facts.paging,
            };
            Some((mark, node.entry_index()))
        })
        .collect();
    files.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    files
}

fn modified_ns(meta: &std::fs::Metadata) -> u64 {
    (meta.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as u64)
}

impl BaseRecord {
    /// The record of the archive at `base`, read from its entry table and no body.
    pub fn of(base: &Path, crypto: &Arc<dyn RpfCryptoProvider>) -> Result<Self, String> {
        let meta =
            std::fs::metadata(base).map_err(|e| format!("Read `{}`: {e}", base.display()))?;
        let archive = open(base, crypto)?;
        Ok(Self {
            size: meta.len(),
            modified: modified_ns(&meta),
            entries: files_of(&archive)
                .into_iter()
                .map(|(mark, _)| mark)
                .collect(),
        })
    }

    /// `None` for a file that is missing or damaged, which is no record.
    pub fn read(file: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(file).ok()?).ok()
    }

    pub fn write(&self, file: &Path) -> Result<(), String> {
        let json =
            serde_json::to_vec(self).map_err(|e| format!("Record {}: {e}", file.display()))?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Create `{}`: {e}", parent.display()))?;
        }
        std::fs::write(file, json).map_err(|e| format!("Write `{}`: {e}", file.display()))
    }

    /// Whether the install's archive at `base` is the one this record was taken of.
    ///
    /// ⚠ A stat that differs is not yet a patch: a touch, a verify or a move rewrites the time and
    /// leaves the archive, so only a different entry table says stale.
    pub fn verdict(&self, base: &Path, crypto: &Arc<dyn RpfCryptoProvider>) -> Staleness {
        let Ok(meta) = std::fs::metadata(base) else {
            return Staleness::Unchecked;
        };
        if (meta.len(), modified_ns(&meta)) == (self.size, self.modified) {
            return Staleness::Fresh;
        }
        match open(base, crypto) {
            Ok(now)
                if files_of(&now)
                    .into_iter()
                    .map(|(m, _)| m)
                    .eq(self.entries.iter().cloned()) =>
            {
                Staleness::Fresh
            }
            Ok(_) => Staleness::Stale,
            // A table that read then and does not now is a different archive; the same length may be damage.
            Err(_) if meta.len() != self.size => Staleness::Stale,
            Err(_) => Staleness::Unchecked,
        }
    }
}

/// The copies under the install's overlay that have a record and whose original has changed since.
pub fn stale_copies(
    install_root: &Path,
    records_dir: &Path,
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Vec<StaleCopy> {
    let mut stale = Vec::new();
    let mut folders = vec![overlay_root(install_root)];
    while let Some(folder) = folders.pop() {
        let Ok(read) = std::fs::read_dir(&folder) else {
            continue;
        };
        for path in read.flatten().map(|entry| entry.path()) {
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let is_archive = (path.extension()).is_some_and(|e| e.eq_ignore_ascii_case("rpf"));
            let base = install_path_for(install_root, &path).filter(|_| is_archive);
            let record = base
                .as_ref()
                .and_then(|_| BaseRecord::read(&record_file(records_dir, &path)));
            if let (Some(base), Some(record)) = (base, record) {
                if record.verdict(&base, crypto) == Staleness::Stale {
                    stale.push(StaleCopy { copy: path, base });
                }
            }
        }
    }
    stale.sort_by(|a, b| a.copy.cmp(&b.copy));
    stale
}

/// What replacing `copy` with the patched archive at `base` would throw away, read from both archives' decoded files.
///
/// ⚠ **A file the update changed is not the user's to list.** The record says which files the update
/// moved; a file it did not move that differs from the copy is the user's. One it did move is told
/// apart only by size, so a same-size edit to it is counted with the update's, in `by_update`.
/// A file that cannot be read on either side counts as differing: nothing is under-reported.
pub fn refresh_plan(
    copy: &Path,
    base: &Path,
    record: &BaseRecord,
    crypto: &Arc<dyn RpfCryptoProvider>,
    stop: &dyn Fn() -> bool,
) -> Result<RefreshPlan, String> {
    let (mine, theirs) = (open(copy, crypto)?, open(base, crypto)?);
    let (mine_files, their_files) = (files_of(&mine), files_of(&theirs));
    let then: HashMap<&str, &EntryMark> = (record.entries.iter())
        .map(|m| (m.path.as_str(), m))
        .collect();
    let now: HashMap<&str, (&EntryMark, u32)> = (their_files.iter())
        .map(|(m, index)| (m.path.as_str(), (m, *index)))
        .collect();
    let held: HashSet<&str> = mine_files.iter().map(|(m, _)| m.path.as_str()).collect();
    let decoded = |archive: &RpfArchive, index: u32| archive.read_entry(index).ok();

    let mut plan = RefreshPlan::default();
    for (mark, index) in &mine_files {
        if stop() {
            return Err("Stopped before the refresh was weighed".to_string());
        }
        let path = mark.path.as_str();
        let moved_by_update = then.get(path).copied() != now.get(path).map(|(m, _)| *m);
        match now.get(path) {
            None if moved_by_update => plan.by_update.push(path.to_string()),
            None => plan.yours.push(Discard::new(path, Change::Added)),
            Some((_, their_index)) => {
                let same = matches!(
                    (decoded(&mine, *index), decoded(&theirs, *their_index)),
                    (Some(a), Some(b)) if a == b
                );
                if same {
                    continue;
                }
                if moved_by_update {
                    plan.by_update.push(path.to_string());
                } else {
                    plan.yours.push(Discard::new(path, Change::Edited));
                }
            }
        }
    }
    for (mark, _) in &their_files {
        let path = mark.path.as_str();
        if held.contains(path) {
            continue;
        }
        match then.get(path) {
            Some(before) if *before == mark => plan.yours.push(Discard::new(path, Change::Removed)),
            Some(_) => plan.by_update.push(path.to_string()),
            None => {}
        }
    }
    plan.yours.sort_by(|a, b| a.path.cmp(&b.path));
    plan.by_update.sort();
    Ok(plan)
}
