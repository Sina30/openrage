//! The RPF9 archive explorer reads a build's archives as entry listings: every archive opens, lists
//! its name64 column (at most the header's entry count, since the engine deduplicates columns), and —
//! given the recovered-names corpus — resolves known entries to their paths. The nested scan runs
//! clean over a small archive.
//!
//! Install-gated and game-derived, so it needs the same env vars as the TOC oracle test and skips
//! cleanly without them, which is why a clean checkout (`just test-no-pack`) passes:
//!   `OPENRAGE_GTA6_KEYS`   the `gtavi_magic.dat` key snapshot (never committed)
//!   `OPENRAGE_GTA6_BUILD`  the build root holding `*.rpf` and `pack/*/*.rpf`
//!   `OPENRAGE_GTA6_NAMES`  optional: a folder with `resolved-names.tsv` and `toc-entries-resolved.tsv`

#![cfg(feature = "gta6")]

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use or_io::container::rpf::{
    Gta6Archive, GtaViKeys, NameResolver, GROUP_NAME64, GTA6_SNAPSHOT_ENV,
};
use or_test_support::skip_install;

const BUILD_ENV: &str = "OPENRAGE_GTA6_BUILD";
const NAMES_ENV: &str = "OPENRAGE_GTA6_NAMES";

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

fn read_toc_prefix(path: &Path) -> Vec<u8> {
    let mut file = std::fs::File::open(path).expect("open the archive");
    let mut head = [0u8; 64];
    file.read_exact(&mut head).expect("read the RPF9 header");
    let toc_end = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let mut buf = vec![0u8; toc_end];
    file.seek(SeekFrom::Start(0)).expect("rewind");
    file.read_exact(&mut buf)
        .expect("read the table of contents");
    buf
}

/// Every `*.rpf` under `root` and every `pack/*/*.rpf`.
fn build_archives(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut rpfs_in = |dir: &Path| {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("rpf") {
                    out.push(path);
                }
            }
        }
    };
    rpfs_in(root);
    if let Ok(packs) = std::fs::read_dir(root.join("pack")) {
        for pack in packs.flatten() {
            if pack.path().is_dir() {
                rpfs_in(&pack.path());
            }
        }
    }
    out.sort();
    out
}

fn load_names(dir: &Path) -> NameResolver {
    let mut resolver = NameResolver::new();
    if let Ok(text) = std::fs::read_to_string(dir.join("toc-entries-resolved.tsv")) {
        resolver.add_toc_entries_tsv(&text);
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("resolved-names.tsv")) {
        resolver.add_hash_name_tsv(&text);
    }
    resolver
}

#[test]
fn every_build_archive_lists_its_name64_column() {
    let what = "GTA VI RPF9 archive explorer over a build";
    let (Some(snapshot), Some(build)) = (env_path(GTA6_SNAPSHOT_ENV), env_path(BUILD_ENV)) else {
        skip_install(
            what,
            format_args!(
                "set {GTA6_SNAPSHOT_ENV} (the gtavi_magic.dat) and {BUILD_ENV} (the build root)"
            ),
        );
        return;
    };

    let keys = GtaViKeys::from_file(&snapshot).expect("the key snapshot parses");
    let archives = build_archives(&build);
    assert!(!archives.is_empty(), "no .rpf under {}", build.display());

    for path in &archives {
        let prefix = read_toc_prefix(path);
        let archive = Gta6Archive::open(&prefix, &keys)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap();

        // The listing holds the name64 column's own rows, which is at most entry_count because the
        // engine deduplicates columns.
        assert!(
            archive.entries.len() <= archive.toc.entry_count as usize,
            "{stem}: listed {} rows, more than the {} the header states",
            archive.entries.len(),
            archive.toc.entry_count
        );
        // It equals the name64 column's row count, or is empty when the archive has no such column.
        match archive
            .groups()
            .iter()
            .find(|g| g.name_hash == GROUP_NAME64)
        {
            Some(g) => assert_eq!(
                archive.entries.len(),
                g.count as usize,
                "{stem}: listed rows disagree with the name64 column"
            ),
            None => assert!(
                archive.entries.is_empty(),
                "{stem}: entries with no name64 column"
            ),
        }
        // A descriptor is either absent (a non-co-indexed column) or a full 24-byte record.
        for entry in &archive.entries {
            assert!(
                entry.descriptor.is_empty() || entry.descriptor.len() == 24,
                "{stem}: entry {} has a {}-byte descriptor",
                entry.index,
                entry.descriptor.len()
            );
        }
        // name64 values are distinct (the column is deduplicated).
        let mut hashes: Vec<u64> = archive.entries.iter().map(|e| e.name64).collect();
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(
            hashes.len(),
            archive.entries.len(),
            "{stem}: a name64 repeats"
        );
    }
    println!("{} RPF9 archives listed", archives.len());
}

#[test]
fn appdata0_and_textures_match_the_known_entry_names() {
    let what = "GTA VI RPF9 explorer against known entry names";
    let (Some(snapshot), Some(build)) = (env_path(GTA6_SNAPSHOT_ENV), env_path(BUILD_ENV)) else {
        skip_install(
            what,
            format_args!("set {GTA6_SNAPSHOT_ENV} and {BUILD_ENV}"),
        );
        return;
    };
    let keys = GtaViKeys::from_file(&snapshot).expect("the key snapshot parses");

    // The name64 values are fixed by the build and cross-checked against the reference decode. The
    // entry column is in the engine's order (not sorted), so these are checked by membership, not
    // position. appdata0 holds config.dat and install.dat; textures holds the weathermaps entry.
    let has = |a: &Gta6Archive, hash: u64| a.entries.iter().any(|e| e.name64 == hash);
    let appdata0 = build.join("appdata0.rpf");
    if appdata0.exists() {
        let a = Gta6Archive::open(&read_toc_prefix(&appdata0), &keys).expect("open appdata0");
        assert_eq!(a.toc.entry_count, 2);
        assert!(has(&a, 0x092D_F786_29F7_833A) && has(&a, 0x3363_73B5_5666_93F1));
    }
    let textures = build.join("textures.rpf");
    if textures.exists() {
        let t = Gta6Archive::open(&read_toc_prefix(&textures), &keys).expect("open textures");
        assert_eq!(t.toc.entry_count, 22);
        assert!(
            has(&t, 0x01D3_E030_56A1_A5C7),
            "textures lacks the weathermaps entry"
        );
    }

    // With the recovered-names corpus, the known hashes resolve to their paths (keyed by value, so
    // order does not matter).
    if let Some(dir) = env_path(NAMES_ENV) {
        let resolver = load_names(&dir);
        if !resolver.is_empty() && appdata0.exists() {
            let mut a =
                Gta6Archive::open(&read_toc_prefix(&appdata0), &keys).expect("open appdata0");
            a.resolve_names(&resolver);
            let name_of = |hash: u64| {
                a.entries
                    .iter()
                    .find(|e| e.name64 == hash)
                    .and_then(|e| e.name.clone())
            };
            assert_eq!(
                name_of(0x092D_F786_29F7_833A).as_deref(),
                Some("config.dat")
            );
            assert_eq!(
                name_of(0x3363_73B5_5666_93F1).as_deref(),
                Some("install.dat")
            );
        }
    }
}

#[test]
fn a_small_archive_scans_clean_for_nested_archives() {
    let what = "GTA VI RPF9 nested scan";
    let (Some(snapshot), Some(build)) = (env_path(GTA6_SNAPSHOT_ENV), env_path(BUILD_ENV)) else {
        skip_install(
            what,
            format_args!("set {GTA6_SNAPSHOT_ENV} and {BUILD_ENV}"),
        );
        return;
    };
    let keys = GtaViKeys::from_file(&snapshot).expect("the key snapshot parses");
    let appdata0 = build.join("appdata0.rpf");
    if !appdata0.exists() {
        skip_install(
            what,
            format_args!("no appdata0.rpf under {}", build.display()),
        );
        return;
    }
    // appdata0 is small and holds plain files, so the scan reads its whole content and finds no
    // nested RPF9 header — Some(empty), not None (too large) and not a spurious hit.
    let whole = std::fs::read(&appdata0).expect("read appdata0 whole");
    let archive = Gta6Archive::open(&whole, &keys).expect("open appdata0");
    let nested = archive.nested(&whole, &keys).expect("scan appdata0");
    assert!(
        matches!(nested.as_deref(), Some([])),
        "appdata0 should scan to an empty nested list, got {:?}",
        nested.map(|v| v.len())
    );
}
