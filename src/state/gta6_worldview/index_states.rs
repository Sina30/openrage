//! What each mode's world-state data switches on at rest: the selectors of the game-shape archives
//! its packs register, found by a scan of each archive's span, and from them the hours each group
//! is on under each set of modes.
//!
//! ⚠ With no key snapshot no table names a game-shape archive, so no group is on at rest.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use or_io::container::rpf::{Rpf9BodyKind, Rpf9Content, Rpf9Scan, Rpf9ZlibRun};
use or_io::gta6::world_state::{words_on, Selector, WorldState, WorldStateError};
use or_job::Cancel;
use or_title::gta6_mount;

use super::mount::Mount;
use super::GroupRow;

/// The inflated bytes kept of a body: more than a world-state body was measured to hold.
const KEEP: usize = 32 << 20;

/// Every selector of the game-shape archives the packs register, with the bit of its pack's mode; `None` once `cancel` is set.
pub(super) fn selectors(
    mounted: &Mount,
    every: &[PathBuf],
    cancel: &Cancel,
) -> Option<Vec<(u8, Selector)>> {
    let listed = mounted.listed(every, every);
    let mut found = Vec::new();
    for (mode, archive, entry) in gta6_mount::game_shapes(&mounted.registry.packs, &listed) {
        let Some(path) = every.get(archive as usize) else {
            continue;
        };
        let span = (entry.offset, entry.offset.saturating_add(entry.stored));
        match bodies(path, span, cancel) {
            Ok(Some(states)) => found.extend(
                (states.into_iter())
                    .flat_map(|state| state.selectors)
                    .map(|selector| (mode.bit(), selector)),
            ),
            Ok(None) => return None,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "a game-shape archive was not read")
            }
        }
    }
    Some(found)
}

/// The world-state bodies a scan of `from..to` of the archive at `path` finds; `None` once `cancel` is set.
fn bodies(
    path: &Path,
    (from, to): (u64, u64),
    cancel: &Cancel,
) -> io::Result<Option<Vec<WorldState>>> {
    let file = File::open(path)?;
    let mut scan = (Rpf9Scan::new(&file, from..to).map_err(io::Error::other)?).keeping(KEEP);
    let mut found = Vec::new();
    while let Some(find) = scan.next() {
        if cancel.is_set() {
            return Ok(None);
        }
        // A scan that fails ends: the rest of the span is not read.
        let find = match find {
            Ok(find) => find,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "the scan of a game-shape archive stopped");
                break;
            }
        };
        let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
            content: Rpf9Content::Pso,
            ..
        }) = find.kind
        else {
            continue;
        };
        match WorldState::open(scan.kept(&find)) {
            Ok(state) => found.push(state),
            Err(WorldStateError::NotWorldState(_)) => {}
            Err(error) => {
                tracing::debug!(offset = find.offset, %error, "a world-state body was not read")
            }
        }
    }
    Ok(Some(found))
}

/// Each group of `words` (its id and word, as its maps state them) with the hours it is on under each set of modes, by `selectors` with their modes' bits.
pub(super) fn switches(words: &[(u32, u64)], selectors: &[(u8, Selector)]) -> Vec<GroupRow> {
    let mut rows: Vec<GroupRow> = (words.iter())
        .map(|&(id, word)| GroupRow {
            word,
            id,
            pad: 0,
            hours: [0; 8],
        })
        .collect();
    rows.sort_unstable_by_key(|r| (r.id, r.word));
    rows.dedup_by_key(|r| r.id);
    for modes in 0..8u8 {
        let of_modes: Vec<&Selector> = (selectors.iter())
            .filter(|(mode, _)| mode & modes != 0)
            .map(|(_, selector)| selector)
            .collect();
        for hour in 0..24 {
            let on = words_on(of_modes.iter().copied(), hour);
            for row in rows.iter_mut().filter(|r| on.contains(&r.word)) {
                row.hours[usize::from(modes)] |= 1 << hour;
            }
        }
    }
    rows
}

#[cfg(test)]
#[path = "index_states_tests.rs"]
mod tests;
