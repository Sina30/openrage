//! What a session remembers of a pack's files, kept in the pack's private folder so that the next session reads only what changed.
//!
//! A pack of thousands of files costs minutes to read cold, and a person who opens it every day should not pay that
//! each time. [`save`] keeps what the facts of [`crate::facts`] remembered, by each file's size and time, and [`load`] hands
//! it back before the first look: a file that was not touched is not read again, and one that was is.
//!
//! ⚠ **Best effort, and never written where another window writes.** A cache that cannot be written or read costs the
//! reading it would have saved and nothing else. A pack that another project links is read, never written, so [`save`] is
//! for the folder of a pack that this one edits, and a pack that another window holds the lock of is that window's to
//! write, so [`save`] keeps nothing in it.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use or_project::content::facts::{
    files_read, forget_under, remember_from, remembered_count, remembered_under,
};

use crate::backup::{make_private, private_dir};
use crate::lock::held_elsewhere;

const FILE: &str = "files.json";

/// How many files a folder must have been read of before what was read is worth a file of its own: a small pack is read again in a moment.
const WORTH_KEEPING: usize = 200;

/// What this process has done with the cache of each folder: that it took it, the hash of the text it last kept, and how many files the process had read when it last settled what to keep.
#[derive(Default)]
struct Dealt {
    taken: BTreeSet<PathBuf>,
    kept: BTreeMap<PathBuf, u64>,
    settled: BTreeMap<PathBuf, usize>,
}

fn dealt() -> &'static Mutex<Dealt> {
    static DEALT: OnceLock<Mutex<Dealt>> = OnceLock::new();
    DEALT.get_or_init(Default::default)
}

/// Hand back what an earlier session kept of the files in `folder`, once for each folder in this process; how many files it holds.
pub fn load(folder: &Path) -> usize {
    let Ok(mut dealt) = dealt().lock() else {
        return 0;
    };
    if !dealt.taken.insert(folder.to_path_buf()) {
        return 0;
    }
    match std::fs::read_to_string(private_dir(folder).join(FILE)) {
        Ok(text) => {
            // What was just handed back is what the folder holds, so a session that reads no file has nothing to keep.
            dealt.settled.insert(folder.to_path_buf(), files_read());
            remember_from(&text)
        }
        Err(_) => 0,
    }
}

/// Keep what this process remembers of the files in `folder`, unless no file was read since it last settled what to keep, another window holds the pack, or it is what was last kept.
///
/// ⚠ A scan runs again every few seconds and finds nothing changed, so the count of files read decides before anything is written out as text.
/// The count is the process's, so a read in another pack asks for one more look here; and a window that holds the lock
/// reads itself as another window, so it keeps nothing and the next session that only looks keeps what its edits changed.
pub fn save(folder: &Path) {
    let read = files_read();
    let settled = |dealt: &Dealt| dealt.settled.get(folder) == Some(&read);
    if dealt().lock().map_or(true, |dealt| settled(&dealt)) {
        return;
    }
    if remembered_count(folder) < WORTH_KEEPING || held_elsewhere(folder) {
        return;
    }
    let text = remembered_under(folder);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    let hash = hasher.finish();
    let Ok(mut dealt) = dealt().lock() else {
        return;
    };
    if dealt.kept.get(folder) == Some(&hash) {
        dealt.settled.insert(folder.to_path_buf(), read);
        return;
    }
    let kept = make_private(folder)
        .and_then(|dir| or_project::content::folder::write_atomic(&dir.join(FILE), &text));
    match kept {
        Ok(()) => {
            dealt.kept.insert(folder.to_path_buf(), hash);
            dealt.settled.insert(folder.to_path_buf(), read);
        }
        Err(why) => tracing::warn!(
            "what was read of {} could not be kept for the next session: {why}",
            folder.display()
        ),
    }
}

/// Forget what this process remembers of the files in `folder`, as a new session would, so that the next look takes the cache the folder holds.
pub fn forget(folder: &Path) {
    forget_under(folder);
    if let Ok(mut dealt) = dealt().lock() {
        dealt.taken.remove(folder);
        dealt.kept.remove(folder);
        dealt.settled.remove(folder);
    }
}
