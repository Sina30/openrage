//! The RPF9 TOC decrypt reproduces, byte for byte, a verified reference reader's decrypted table of
//! contents for every archive of a GTA VI build.
//!
//! Install-gated and game-derived, so it needs three env vars and skips cleanly without them, which
//! is why a clean checkout (`just test-no-pack`) passes:
//!   `OPENRAGE_GTA6_KEYS`       the `gtavi_magic.dat` key snapshot (never committed)
//!   `OPENRAGE_GTA6_BUILD`      the build root holding `*.rpf` and `pack/*/*.rpf`
//!   `OPENRAGE_GTA6_TOC_ORACLE` a directory of `<stem>.toc.bin`, each the plaintext header plus the
//!                              fully decrypted TOC of its archive

#![cfg(feature = "gta6")]

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use or_io::container::rpf::{GtaViKeys, Rpf9Toc, GTA6_SNAPSHOT_ENV};
use or_test_support::skip_install;

const BUILD_ENV: &str = "OPENRAGE_GTA6_BUILD";
const ORACLE_ENV: &str = "OPENRAGE_GTA6_TOC_ORACLE";

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// Every `*.rpf` directly under `root`, then every `pack/*/*.rpf`, sorted as the reference reader
/// enumerated them.
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

/// Only `[0, toc_end)`, which is all the decrypt reads: a build archive can be many gigabytes, and
/// reading one whole would dwarf the work.
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

#[test]
fn every_build_archive_toc_decrypts_to_the_oracle_bytes() {
    let what = "GTA VI RPF9 TOC decrypt against the oracle";
    let (Some(snapshot), Some(build), Some(oracle)) = (
        env_path(GTA6_SNAPSHOT_ENV),
        env_path(BUILD_ENV),
        env_path(ORACLE_ENV),
    ) else {
        skip_install(
            what,
            format_args!(
                "set {GTA6_SNAPSHOT_ENV} (the gtavi_magic.dat), {BUILD_ENV} (the build root) \
                 and {ORACLE_ENV} (the decrypted-toc directory)"
            ),
        );
        return;
    };

    let keys = GtaViKeys::from_file(&snapshot).expect("the key snapshot parses");
    let archives = build_archives(&build);
    assert!(!archives.is_empty(), "no .rpf under {}", build.display());

    let mut matched = 0usize;
    for path in &archives {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("an archive stem");
        let oracle_file = oracle.join(format!("{stem}.toc.bin"));
        let expected = std::fs::read(&oracle_file)
            .unwrap_or_else(|e| panic!("read the oracle {}: {e}", oracle_file.display()));

        let prefix = read_toc_prefix(path);
        let toc = Rpf9Toc::decrypt(&prefix, &keys)
            .unwrap_or_else(|e| panic!("decrypt {}: {e}", path.display()));

        assert_eq!(
            toc.bytes.len(),
            expected.len(),
            "{}: decrypted TOC is {} bytes, the oracle {}",
            stem,
            toc.bytes.len(),
            expected.len()
        );
        if toc.bytes != expected {
            let at = toc
                .bytes
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(expected.len());
            panic!(
                "{stem}: first differs at {at:#X}: ours {:#04X}, oracle {:#04X}",
                toc.bytes[at], expected[at]
            );
        }
        // The decrypted index parsed into groups, and the entry count is non-zero.
        assert!(toc.entry_count > 0, "{stem}: no entries");
        assert!(!toc.groups.is_empty(), "{stem}: no index groups");
        matched += 1;
    }

    assert_eq!(
        matched,
        archives.len(),
        "{matched} of {} archives reproduced the oracle",
        archives.len()
    );
    println!(
        "{matched}/{} RPF9 TOCs reproduced the oracle byte for byte",
        archives.len()
    );
}
