//! The cache loader's `*_cache_y.dat` files, `metadata.dat` and
//! `audioworldsectorsinfo.dat` over real bytes: every one both installs ship
//! opens and saves byte-identical.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated.
//! The cache files sit in `common.rpf`, `update/*.rpf` and the DLC packs'
//! archives, so each install's own executable supplies the keys;
//! `metadata.dat` is loose under `x64/` and `update/x64/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::audio_world_sectors::AudioWorldSectors;
use or_io::cache_loader::CacheFile;
use or_io::container::rpf::crypto::{extract_keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::RpfArchive;
use or_io::social_club_metadata::SocialClubMetadata;
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// `common.rpf`, `update/*.rpf` and every archive under `update/x64/dlcpacks`.
fn archives(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("rpf"))
            {
                out.push(path);
            }
        }
    }
    let mut out = vec![root.join("common.rpf")];
    if let Ok(entries) = std::fs::read_dir(root.join("update")) {
        out.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rpf"))),
        );
    }
    walk(&root.join("update/x64/dlcpacks"), &mut out);
    out
}

/// Every top-level archive entry whose lowercase name `wanted` accepts.
fn shipped(root: &Path, wanted: impl Fn(&str) -> bool) -> Vec<(String, Vec<u8>)> {
    let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let mut out = Vec::new();
    for archive_path in archives(root) {
        let opened = RpfArchive::open_with(
            &archive_path,
            Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys))),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", archive_path.display()));
        for node in opened.nodes.iter().filter(|n| !n.is_dir()) {
            if wanted(&node.name().to_ascii_lowercase()) {
                let bytes = opened
                    .read_entry(node.entry_index())
                    .unwrap_or_else(|e| panic!("{}/{}: {e}", archive_path.display(), node.path()));
                out.push((format!("{}/{}", archive_path.display(), node.path()), bytes));
            }
        }
    }
    out
}

#[test]
fn every_shipped_cache_file_reads_and_saves_byte_identical() {
    let what = "every shipped *_cache_y.dat byte-identical";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let files = shipped(&root, |name| name.ends_with("_cache_y.dat"));
        assert!(
            files.iter().any(|(l, _)| l.ends_with("gta5_cache_y.dat")),
            "{}: no gta5_cache_y.dat",
            root.display()
        );
        let (mut entries, mut dated) = (0, 0);
        for (label, bytes) in &files {
            let cache = CacheFile::open(bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
            entries += cache.map_data_store.len()
                + cache.interior_proxies.len()
                + cache.bounds_store.len();
            dated += cache.file_dates.len();
            assert!(
                &cache.save().expect("saves") == bytes,
                "{label}: the save differs"
            );
        }
        eprintln!(
            "{}: {} cache files, {entries} records, {dated} archive dates",
            root.display(),
            files.len()
        );
        assert!(files.len() > 1, "{}: no DLC cache file", root.display());
    }
}

#[test]
fn every_shipped_metadata_dat_reads_and_saves_byte_identical() {
    let what = "every shipped metadata.dat byte-identical";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let mut read = 0;
        for at in ["x64/metadata.dat", "update/x64/metadata.dat"] {
            let Ok(bytes) = std::fs::read(root.join(at)) else {
                continue;
            };
            let label = format!("{}/{at}", root.display());
            let metadata =
                SocialClubMetadata::open(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
            eprintln!(
                "{label}: built {}, {} body bytes",
                metadata.built,
                metadata.body.len()
            );
            assert!(
                metadata.save().expect("saves") == bytes,
                "{label}: the save differs"
            );
            read += 1;
        }
        assert!(read > 0, "{}: no metadata.dat", root.display());
    }
}

#[test]
fn every_shipped_audio_world_sectors_file_reads_and_saves_byte_identical() {
    let what = "every shipped audioworldsectorsinfo.dat byte-identical";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let files = shipped(&root, |name| name == "audioworldsectorsinfo.dat");
        assert!(
            !files.is_empty(),
            "{}: no audioworldsectorsinfo.dat",
            root.display()
        );
        for (label, bytes) in files {
            let sectors =
                AudioWorldSectors::open(&bytes).unwrap_or_else(|e| panic!("{label}: {e}"));
            let water = sectors.sectors.iter().filter(|s| s.is_water_sector).count();
            eprintln!(
                "{label}: {} sectors, {water} over water",
                sectors.sectors.len()
            );
            assert!(water > 0 && water < sectors.sectors.len(), "{label}");
            assert!(
                sectors.save().expect("saves") == bytes,
                "{label}: the save differs"
            );
        }
    }
}
