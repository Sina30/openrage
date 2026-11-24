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
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    let slot = match held.iter().find(|(path, _)| path == root) {
        Some((_, slot)) => Arc::clone(slot),
        None => {
            let slot = Read::default();
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
                let ground = match read {
                    Ok(ground) => {
                        tracing::info!(
                            tiles = ground.tiles(),
                            seconds = started.elapsed().as_secs_f32(),
                            "GTA VI's ground under the water was read"
                        );
                        Some(Arc::new(ground))
                    }
                    Err(why) => {
                        tracing::warn!(root = %root.display(), %why, "GTA VI's ground under the water is not read");
                        None
                    }
                };
                let _ = filled.set(ground);
            });
            slot
        }
    };
    slot.get()?.clone()
}

/// The grid the height map of the install at `root` is tiled by.
fn atlas(root: &Path, keys: &GtaViKeys) -> Result<GridAtlas, String> {
    let archive = root.join(ATLAS_ARCHIVE);
    let at = entry_offset(&archive, ATLAS_ENTRY, keys)
        .ok_or("the table names no grid of the height map")?;
    let file = File::open(&archive).map_err(|e| e.to_string())?;
    let (_, text) = read_rpf9_body(&file, at, BODY_LIMIT).map_err(|e| e.to_string())?;
    GridAtlas::parse(&String::from_utf8_lossy(&text)).map_err(|e| e.to_string())
}

/// The height map of the install at `root`: its fallback, and every tile laid where it matches.
fn read(root: &Path, keys: &GtaViKeys) -> Result<WaterGround, String> {
    let atlas = atlas(root, keys)?;
    let archive = root.join(ARCHIVE);
    let table = Table::open(&archive, keys).map_err(|e| e.to_string())?;
    let nested = table
        .find_archive(HEIGHTS)
        .ok_or("the table names no archive of the height map")?;
    let span = nested.offset..nested.offset.saturating_add(nested.stored);
    let scanned = File::open(&archive).map_err(|e| e.to_string())?;
    let mut dictionaries = Vec::new();
    for find in Rpf9Scan::new(&scanned, span).map_err(|e| e.to_string())? {
        let find = match find {
            Ok(find) => find,
            Err(why) => {
                tracing::warn!(%why, "the scan of the height map's archive stopped");
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
    let (path, named) = (&archive, atlas.fallback.as_str());
    let read: Vec<Vec<(bool, Heights)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (dictionaries.chunks(run))
            .map(|offsets| scope.spawn(move || heights_at(path, offsets, named)))
            .collect();
        handles.into_iter().flat_map(|h| h.join().ok()).collect()
    });
    let (mut fallback, mut tiles) = (None, Vec::new());
    for (is_fallback, heights) in read.into_iter().flatten() {
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

/// The height textures of the dictionaries at `offsets` of `archive`, each marked whether it is the fallback named `fallback`; a tile is held shrunk until it is laid.
fn heights_at(archive: &Path, offsets: &[u64], fallback: &str) -> Vec<(bool, Heights)> {
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
            let Some(heights) = texture.texture.as_ref().ok().and_then(Heights::of) else {
                continue;
            };
            match texture.name == fallback {
                true => out.push((true, heights)),
                false => out.push((false, heights.shrunk(KEPT).unwrap_or(heights))),
            }
        }
    }
    out
}
