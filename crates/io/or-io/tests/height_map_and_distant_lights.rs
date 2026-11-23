//! `heightmap*.dat`, `distantlights*.dat`, `waterheight.dat`, `worldblanket.dat`
//! and `navprecalc.dat` over real bytes: every one both installs ship opens and
//! saves byte-identical.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated.
//! The files sit in `common.rpf` and `update/update.rpf`, so each install's own
//! executable supplies the keys.

use std::path::Path;
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::RpfArchive;
use or_io::distant_lights::{DistantLights, BLOCKS, BLOCKS_HD};
use or_io::height_map::HeightMap;
use or_io::nav_precalc::NavPrecalc;
use or_io::water_map::{WaterMap, WaterMapRefType};
use or_io::world_blanket::WorldBlanket;
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// Every `.dat` in the install's two top-level archives whose name starts with `prefix`.
fn shipped(root: &Path, prefix: &str) -> Vec<(String, Vec<u8>)> {
    let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let mut out = Vec::new();
    for archive in ["common.rpf", "update/update.rpf"] {
        let archive_path = root.join(archive);
        let opened = RpfArchive::open_with(
            &archive_path,
            Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys))),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", archive_path.display()));
        for node in opened.nodes.iter().filter(|n| !n.is_dir()) {
            let name = node.name().to_ascii_lowercase();
            if name.starts_with(prefix) && name.ends_with(".dat") {
                let bytes = opened
                    .read_entry(node.entry_index())
                    .unwrap_or_else(|e| panic!("{archive}/{}: {e}", node.path()));
                out.push((format!("{archive}/{}", node.path()), bytes));
            }
        }
    }
    out
}

#[test]
fn every_shipped_heightmap_reads_and_saves_byte_identical() {
    let what = "every shipped heightmap.dat byte-identical";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let files = shipped(&root, "heightmap");
        assert!(!files.is_empty(), "{}: no heightmap.dat", root.display());
        for (label, bytes) in files {
            let map = HeightMap::open(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
            let live = map.max.iter().filter(|&&h| h != 0).count();
            eprintln!(
                "{label}: {}x{} cells over {:?}..{:?}, flags {:#x}, {live} with a maximum",
                map.num_cols, map.num_rows, map.bounds_min, map.bounds_max, map.flags
            );
            assert!(live > 0, "{label}: every cell is empty");
            assert!(
                map.save().expect("saves") == bytes,
                "{label}: the save differs"
            );
        }
    }
}

#[test]
fn every_shipped_distant_lights_file_reads_and_saves_byte_identical() {
    let what = "every shipped distantlights*.dat byte-identical";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let files = shipped(&root, "distantlights");
        assert!(
            files.len() >= 2,
            "{}: {} distant lights files",
            root.display(),
            files.len()
        );
        for (label, bytes) in files {
            let lights = DistantLights::open(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
            let expected = if label.ends_with("_hd.dat") {
                BLOCKS_HD
            } else {
                BLOCKS
            };
            assert_eq!(lights.blocks(), expected, "{label}");
            let lanes: std::collections::BTreeSet<u8> =
                lights.groups.iter().map(|g| g.num_lanes()).collect();
            eprintln!(
                "{label}: {} blocks a side, {} points, {} groups, lane counts {lanes:?}, \
                 {} two-way, {} street lights",
                lights.blocks(),
                lights.points.len(),
                lights.groups.len(),
                lights.groups.iter().filter(|g| g.two_way()).count(),
                lights
                    .groups
                    .iter()
                    .filter(|g| g.use_streetlights())
                    .count(),
            );
            assert!(
                lights.save().expect("saves") == bytes,
                "{label}: the save differs"
            );
        }
    }
}

/// Each of `prefix`'s files in every install, which must ship at least one.
fn every_shipped(what: &str, prefix: &str) -> Vec<(String, Vec<u8>)> {
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
    }
    let mut out = Vec::new();
    for root in roots {
        let files = shipped(&root, prefix);
        assert!(!files.is_empty(), "{}: no {prefix}*.dat", root.display());
        out.extend(files);
    }
    out
}

#[test]
fn every_shipped_water_height_map_reads_and_saves_byte_identical() {
    for (label, bytes) in every_shipped(
        "every shipped waterheight.dat byte-identical",
        "waterheight",
    ) {
        let map = WaterMap::open(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
        let kinds = [
            WaterMapRefType::River,
            WaterMapRefType::Lake,
            WaterMapRefType::Pool,
        ]
        .map(|kind| map.ref_ids.iter().filter(|r| r.kind() == kind).count());
        let wet = (0..map.num_tile_rows())
            .flat_map(|row| (0..map.num_tile_cols).map(move |col| (col, row)))
            .filter(|&(col, row)| map.tile_refs(col, row).is_some())
            .count();
        eprintln!(
            "{label}: {}x{} tiles of {}x{} from ({}, {}), {wet} with water, \
             {} river points, {} rivers, {} lake boxes, {} lakes, {} pools, \
             references to rivers, lakes and pools {kinds:?}",
            map.num_tile_cols,
            map.num_tile_rows(),
            map.tile_size_x,
            map.tile_size_y,
            map.offset_x,
            map.offset_y,
            map.river_points.len(),
            map.rivers.len(),
            map.lake_boxes.len(),
            map.lakes.len(),
            map.pools.len(),
        );
        assert!(wet > 0, "{label}: no tile holds water");
        assert!(
            map.save().expect("saves") == bytes,
            "{label}: the save differs"
        );
    }
}

#[test]
fn every_shipped_world_blanket_reads_and_saves_byte_identical() {
    for (label, bytes) in every_shipped(
        "every shipped worldblanket.dat byte-identical",
        "worldblanket",
    ) {
        let blanket = WorldBlanket::open(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
        let set: std::collections::BTreeSet<u16> = blanket.cells.iter().copied().collect();
        eprintln!(
            "{label}: {0}x{0} cells, values {set:?}, {1} non-zero",
            blanket.side(),
            blanket.cells.iter().filter(|&&c| c != 0).count()
        );
        assert!(blanket.save() == bytes, "{label}: the save differs");
    }
}

#[test]
fn every_shipped_nav_precalc_reads_and_saves_byte_identical() {
    for (label, bytes) in every_shipped("every shipped navprecalc.dat byte-identical", "navprecalc")
    {
        let precalc = NavPrecalc::open(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
        let filled = precalc.cells.iter().filter(|c| c.entries() > 0).count();
        eprintln!(
            "{label}: header {:?}, {} cells, {filled} with entries, {} entries, {} BITS bytes",
            precalc.header,
            precalc.cells.len(),
            precalc.cells.iter().map(|c| c.entries()).sum::<usize>(),
            precalc.bits.len()
        );
        assert!(filled > 0, "{label}: every cell is empty");
        assert!(
            precalc.save().expect("saves") == bytes,
            "{label}: the save differs"
        );
    }
}
