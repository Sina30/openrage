//! The ground under the GTA VI world's water: the install's height map, read once on a worker and
//! held. Its archive's own table is ciphered, so its dictionaries are found by a scan of the span the
//! level archive gives it, and each tile is laid on the grid by its content.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use or_io::container::rpf::{
    read_rpf9_body, GtaViKeys, Rpf9BodyKind, Rpf9Role, Rpf9Scan, Rpf9ZlibRun,
};
use or_io::gta6::headerless::{read_dictionary, Entry};
use or_io::gta6::ttpf::Texture;
use or_io::gta6::water::height::{Heights, WaterGround};
use or_io::gta6::water::GridAtlas;
use or_title::gta6_mount::table::{entry_offset, Table};

/// The archive and the document that state the grid of the height map.
const ATLAS_ARCHIVE: &str = "common.rpf";
const ATLAS_ENTRY: &str = "data/levels/americas/water/watersystemheightmap.meta";
/// The archive the height map's own archive is nested in, and that archive's path in it.
const ARCHIVE: &str = "levels.rpf";
const HEIGHTS: &str = "levels/americas/watersystemheightmap/watersystemheightmap";
/// The bytes a body is read to: the fallback, the largest, is a few MiB.
const BODY_LIMIT: u64 = 16 << 20;
/// The texels a side a tile is held at.
// ponytail: every tile is held at 128 a side, 4 m a texel of a 512 m tile;
// stream the tiles by the grid's radius at their own size if a shore needs the metre.
const KEPT: u32 = 128;

type Read = Arc<OnceLock<Option<Arc<WaterGround>>>>;

static HELD: Mutex<Vec<(PathBuf, Read)>> = Mutex::new(Vec::new());

/// The ground under the water of the install at `root`, read once on a worker and held: `None` until it is read, and where the install's tables give none or it does not read.
pub fn ground(root: &Path) -> Option<Arc<WaterGround>> {
    held(&HELD, root, "the ground under the water", read)
}

/// What `read` makes of the install at `root`, read once on a worker and held in `slots`: `None` until it is read, and where it does not read.
pub(super) fn held<T: Send + Sync + 'static>(
    slots: &'static Mutex<Vec<(PathBuf, Arc<OnceLock<Option<Arc<T>>>>)>>,
    root: &Path,
    what: &'static str,
    read: fn(&Path, &GtaViKeys) -> Result<T, String>,
) -> Option<Arc<T>> {
    let mut held = slots.lock().unwrap_or_else(|p| p.into_inner());
    let slot = match held.iter().find(|(path, _)| path == root) {
        Some((_, slot)) => Arc::clone(slot),
        None => {
            let slot = Arc::<OnceLock<Option<Arc<T>>>>::default();
            held.push((root.to_path_buf(), Arc::clone(&slot)));
            let (filled, root) = (Arc::clone(&slot), root.to_path_buf());
            // ⚠ Where the key snapshot lies is found here, on the caller's thread, as the index finds it.
            let keys = super::super::index::keys();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let read = match keys {
                    Some(keys) => read(&root, &keys),
                    None => Err("no key snapshot deciphers the install's tables".to_string()),
                };
                let value = match read {
                    Ok(value) => {
                        let seconds = started.elapsed().as_secs_f32();
                        tracing::info!(what, seconds, "a part of GTA VI's water was read");
                        Some(Arc::new(value))
                    }
                    Err(why) => {
                        tracing::warn!(root = %root.display(), what, %why, "a part of GTA VI's water is not read");
                        None
                    }
                };
                let _ = filled.set(value);
            });
            slot
        }
    };
    slot.get()?.clone()
}

/// Whether what `slots` holds for `root` was asked for and is still being read.
pub(super) fn reading<T>(
    slots: &Mutex<Vec<(PathBuf, Arc<OnceLock<Option<Arc<T>>>>)>>,
    root: &Path,
) -> bool {
    let held = slots.lock().unwrap_or_else(|p| p.into_inner());
    (held.iter()).any(|(path, slot)| path == root && slot.get().is_none())
}

/// Whether the ground under the water of the install at `root` was asked for and is still being read.
pub fn ground_reading(root: &Path) -> bool {
    reading(&HELD, root)
}

/// The grid the document `entry` of the install at `root` states.
pub(super) fn atlas(root: &Path, keys: &GtaViKeys, entry: &str) -> Result<GridAtlas, String> {
    let archive = root.join(ATLAS_ARCHIVE);
    let at = entry_offset(&archive, entry, keys).ok_or("the table names no grid")?;
    let file = File::open(&archive).map_err(|e| e.to_string())?;
    let (_, text) = read_rpf9_body(&file, at, BODY_LIMIT).map_err(|e| e.to_string())?;
    GridAtlas::parse(&String::from_utf8_lossy(&text)).map_err(|e| e.to_string())
}

/// What `take` makes of each texture of the dictionaries of the archive `nested` that the archive `archive` of the install at `root` holds, read on several workers.
pub(super) fn textures<T: Send>(
    root: &Path,
    keys: &GtaViKeys,
    (archive, nested): (&str, &str),
    take: &(dyn Fn(&str, &Texture) -> Option<T> + Sync),
) -> Result<Vec<T>, String> {
    let archive = root.join(archive);
    let table = Table::open(&archive, keys).map_err(|e| e.to_string())?;
    let nested = table
        .find_archive(nested)
        .ok_or("the table names no such nested archive")?;
    let span = nested.offset..nested.offset.saturating_add(nested.stored);
    let scanned = File::open(&archive).map_err(|e| e.to_string())?;
    let mut dictionaries = Vec::new();
    for find in Rpf9Scan::new(&scanned, span).map_err(|e| e.to_string())? {
        let find = match find {
            Ok(find) => find,
            Err(why) => {
                tracing::warn!(%why, "the scan of a water archive stopped");
                break;
            }
        };
        let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
            resource: Some(_),
            signature: Some(signature),
            ..
        }) = find.kind
        else {
            continue;
        };
        if signature.role() == Some(Rpf9Role::Dictionary) {
            dictionaries.push(find.offset);
        }
    }
    // The bodies are inflated and read on a worker each, in runs of the scan's order.
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let run = dictionaries.len().div_ceil(workers).max(1);
    let path = &archive;
    let read: Vec<Vec<T>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (dictionaries.chunks(run))
            .map(|offsets| scope.spawn(move || textures_at(path, offsets, take)))
            .collect();
        handles.into_iter().flat_map(|h| h.join().ok()).collect()
    });
    Ok(read.into_iter().flatten().collect())
}

/// What `take` makes of each texture of the dictionaries at `offsets` of `archive`.
fn textures_at<T>(
    archive: &Path,
    offsets: &[u64],
    take: &(dyn Fn(&str, &Texture) -> Option<T> + Sync),
) -> Vec<T> {
    let Ok(file) = File::open(archive) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &offset in offsets {
        let Ok((body, bytes)) = read_rpf9_body(&file, offset, BODY_LIMIT) else {
            continue;
        };
        let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
            resource: Some(resource),
            ..
        }) = body.kind
        else {
            continue;
        };
        let (virtual_section, physical) = resource.sections(&bytes);
        let Ok(dictionary) = read_dictionary(virtual_section, Some(physical)) else {
            continue;
        };
        for (_, entry) in dictionary.entries {
            let Ok(Entry::Texture(texture)) = entry else {
                continue;
            };
            if let Some(taken) =
                (texture.texture.as_ref().ok()).and_then(|t| take(&texture.name, t))
            {
                out.push(taken);
            }
        }
    }
    out
}

/// The height map of the install at `root`: its fallback, and every tile laid where it matches.
fn read(root: &Path, keys: &GtaViKeys) -> Result<WaterGround, String> {
    let atlas = atlas(root, keys, ATLAS_ENTRY)?;
    let named = atlas.fallback.as_str();
    // A tile is held shrunk until it is laid.
    let take = |name: &str, texture: &Texture| {
        let heights = Heights::of(texture)?;
        Some(match name == named {
            true => (true, heights),
            false => (false, heights.shrunk(KEPT).unwrap_or(heights)),
        })
    };
    let read = textures(root, keys, (ARCHIVE, HEIGHTS), &take)?;
    let (mut fallback, mut tiles) = (None, Vec::new());
    for (is_fallback, heights) in read {
        match is_fallback {
            true => fallback = Some(heights),
            false => tiles.push(heights),
        }
    }
    let fallback = fallback.ok_or("the height map's archive holds no fallback")?;
    let mut ground = WaterGround::new(&atlas, fallback).map_err(|e| e.to_string())?;
    for tile in tiles {
        ground.add_tile(tile);
    }
    Ok(ground)
}
