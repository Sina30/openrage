//! What shape a snapshot is serialised in, and which generator wrote it.

/// The shape a snapshot is serialised in, as the directory it is kept under.
///
/// ⚠ **A snapshot is `bincode` of the type model, so a reader of another shape
/// cannot decode it.** A directory per shape keeps a regenerated file from
/// breaking every tree still on the older code: raise this when
/// `snapshot::the_snapshot_format_names_every_shape` stops compiling, with
/// `or_title::schema::STORE_FILE`. The first shape had no directory, so older
/// trees read the store's root.
pub const SNAPSHOT_FORMAT: &str = "v3";

/// The sources that decide what a snapshot holds.
///
/// ⚠ `build_library` is `src/meta/schema` and nothing else — it parses PSC and
/// inserts, so a change to the parser, the normaliser, the type model or the
/// library changes what a regenerated snapshot would contain and an existing
/// one silently keeps the old answer.
pub const GENERATOR_SOURCE_DIR: &str = "src/meta/schema";

/// The file a snapshot's generator stamp lives in, beside the snapshot.
pub fn stamp_path(snapshot: &std::path::Path) -> std::path::PathBuf {
    snapshot.with_extension("stamp")
}

/// Every `.rs` under [`GENERATOR_SOURCE_DIR`], sorted.
pub fn generator_sources(manifest_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    collect_rust_sources(&manifest_dir.join(GENERATOR_SOURCE_DIR), &mut found);
    found.sort();
    found
}

fn collect_rust_sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// A digest of those sources, as the stamp a snapshot carries.
///
/// ⚠ **FNV-1a rather than `DefaultHasher`.** The standard hasher's output is
/// explicitly not stable between Rust releases, so a toolchain bump would
/// report every snapshot on the machine as stale.
///
/// Keys must be **relative** to the crate, and carriage returns are dropped:
/// an absolute path differs per worktree and a line ending differs per
/// checkout, and either would make the stamp disagree with itself.
pub fn generator_fingerprint<'a>(sources: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> String {
    let mut sorted: Vec<(&str, &[u8])> = sources.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |byte: u8| {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for (name, bytes) in sorted {
        for byte in name.as_bytes() {
            eat(*byte);
        }
        eat(0);
        for byte in bytes.iter().filter(|b| **b != b'\r') {
            eat(*byte);
        }
        eat(0);
    }
    format!("{hash:016x}")
}

/// Read every generator source under `manifest_dir` and digest it.
pub fn fingerprint_of(manifest_dir: &std::path::Path) -> String {
    let read: Vec<(String, Vec<u8>)> = generator_sources(manifest_dir)
        .iter()
        .filter_map(|path| {
            let key = path
                .strip_prefix(manifest_dir)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            std::fs::read(path).ok().map(|bytes| (key, bytes))
        })
        .collect();
    generator_fingerprint(read.iter().map(|(k, b)| (k.as_str(), b.as_slice())))
}
