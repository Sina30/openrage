//! What each mode's world-state data switches on at rest: the selectors of the game-shape archives
//! its packs register, found by a scan of each archive's span, and from them the hours each group
//! is on under each set of modes.
//!
//! ⚠ With no key snapshot no table names a game-shape archive, so no group is on at rest.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use or_io::container::rpf::{
    self, Gta6Mount, GtaViKeys, Rpf9BodyKind, Rpf9Content, Rpf9Scan, Rpf9ZlibRun,
};
use or_io::gta6::hash::name_hash64;
use or_io::gta6::world_state::{layers_loaded, words_on, Selector, WorldState, WorldStateError};
use or_job::Cancel;
use or_title::gta6_mount;

use super::mount::Mount;
use super::GroupRow;

/// The inflated bytes kept of a body: more than a world-state body was measured to hold.
const KEEP: usize = 32 << 20;

/// Every selector of the game-shape archives the packs register, in layers the level loads, with the bit of its pack's mode; `None` once `cancel` is set.
pub(super) fn selectors(
    mounted: &Mount,
    every: &[PathBuf],
    cancel: &Cancel,
) -> Option<Vec<(u8, Selector)>> {
    let listed = mounted.listed(every, every);
    let keys = super::mount::keys();
    let mut found = Vec::new();
    for (mode, archive, entry, registered) in
        gta6_mount::game_shapes(&mounted.registry.packs, &listed)
    {
        let Some(path) = every.get(archive as usize) else {
            continue;
        };
        // With no manifest every layer counts, as before one was read.
        let layers = (keys.as_ref())
            .map(|keys| layers(path, entry.offset, &registered, keys))
            .unwrap_or_else(|| Err("no key snapshot".into()))
            .inspect_err(|error| {
                tracing::warn!(path = %path.display(), %error, "a game-shape layer manifest was not read")
            })
            .unwrap_or_default();
        let span = (entry.offset, entry.offset.saturating_add(entry.stored));
        match bodies(path, span, cancel) {
            Ok(Some(states)) => found.extend(
                (loaded_at_rest(states, &layers).into_iter())
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

/// The layers of the game-shape archive registered at `registered`, whose header is at `at` in the file at `path`: the hash of each one's name and whether the level loads it.
fn layers(
    path: &Path,
    at: u64,
    registered: &str,
    keys: &GtaViKeys,
) -> Result<Vec<(u32, bool)>, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let archive = rpf::open_at(&mut file, at, keys).map_err(|e| e.to_string())?;
    // The manifest is the archive's `<its name>/layermanifest`.
    let stem = registered.rsplit('/').next().unwrap_or(registered);
    let word = name_hash64(
        format!("{stem}/layermanifest")
            .to_ascii_lowercase()
            .as_bytes(),
    );
    let mount = Gta6Mount { base: at, archive };
    let entry = (mount.archive.entries.iter())
        .find(|e| e.name64 == word)
        .ok_or("the archive lists no layer manifest")?;
    let bytes = mount
        .read_entry(&mut file, entry)
        .map_err(|e| e.to_string())?;
    layers_loaded(&bytes).map_err(|e| e.to_string())
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
/// The bodies of `states` whose layer the level loads: all but those a manifest of `layers` states a script or a system loads.
fn loaded_at_rest(mut states: Vec<WorldState>, layers: &[(u32, bool)]) -> Vec<WorldState> {
    states.retain(|s| !layers.contains(&(s.layer, false)));
    states
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
