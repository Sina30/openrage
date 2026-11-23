//! Whether a name table's names are the names its sibling `.dat` hashed.
//!
//! Point `OPENRAGE_GAME_ROOT` at one or more install roots, `;`-separated.
//! Nothing here runs without one, and the skip is recorded rather than
//! silent — this is a question about a whole install and no fixture can
//! stand in for one.
//!
//! ⚠ A hash that resolves is weak evidence. A hash that resolves *and* whose
//! number is sitting in the file the name came with is the evidence this
//! suite exists to collect, and it is what pins which of the two RAGE hash
//! functions the game writes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use or_io::gta5::data::nametable;
use or_io::names::{HashFlavour, NameIndex};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// A name table and the `.dat` it belongs to, both already on disk.
struct Pair {
    label: String,
    names: Vec<String>,
    skipped: usize,
    words: HashSet<u32>,
}

/// Name tables live inside archives, so a plain walk of the install root
/// finds only the ones somebody has already extracted beside their `.dat`;
/// `openrage-cli names` is the whole-install ingest.
fn pairs(what: &str) -> Vec<Pair> {
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(
            what,
            format_args!("no install root; set {GAME_ROOT_ENV} to a ;-separated list"),
        );
        return Vec::new();
    }

    let mut out = Vec::new();
    for root in &roots {
        collect(root, &mut out);
    }
    if out.is_empty() {
        skip_install(
            what,
            format_args!(
                "no .nametable sitting beside its .dat under {}; extract one with \
                 `openrage-cli rpf extract`",
                roots
                    .iter()
                    .map(|r| r.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    out
}

fn collect(root: &Path, out: &mut Vec<Pair>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("nametable") {
                continue;
            }
            let dat = PathBuf::from(path.to_string_lossy().trim_end_matches(".nametable"));
            let (Ok(table_bytes), Ok(dat_bytes)) = (std::fs::read(&path), std::fs::read(&dat))
            else {
                continue;
            };
            let table = nametable::parse(&table_bytes)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            out.push(Pair {
                label: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                names: table.names,
                skipped: table.skipped.len(),
                words: words_of(&dat_bytes),
            });
        }
    }
}

/// Every 32-bit little-endian word in the `.dat`, at four-byte alignment.
fn words_of(bytes: &[u8]) -> HashSet<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// ⚠ The case-folding hash is the one the game writes, and the literal one
/// finds nothing. Without the second half the first is a coincidence nobody
/// measured against an alternative.
#[test]
fn a_game_dat_holds_the_case_folding_hash_of_every_name_beside_it() {
    let pairs = pairs("name table against its own .dat");
    let mut checked = 0usize;
    let mut tables = 0usize;

    for pair in &pairs {
        // Only the `game.datNNN` family stores every one of its names as a
        // top-level hash; a `sounds.dat` names things it addresses by index
        // too, so a miss there is not a hash-function fact.
        if !pair.label.contains("game.dat") {
            continue;
        }
        tables += 1;
        for name in &pair.names {
            checked += 1;
            assert!(
                pair.words.contains(&HashFlavour::CaseFolding.of(name)),
                "{}: the case-folding hash of {name} is not in the .dat beside it",
                pair.label
            );
            assert!(
                !pair.words.contains(&HashFlavour::Literal.of(name)),
                "{}: the literal hash of {name} is in the .dat too, so this pair cannot \
                 tell the two functions apart",
                pair.label
            );
        }
    }

    if tables == 0 && !pairs.is_empty() {
        skip_install(
            "name table against its own .dat",
            "extracted name tables exist, but none is a game.datNNN pair",
        );
        return;
    }
    if tables > 0 {
        assert!(checked > 0, "{tables} table(s) and not one name in them");
    }
}

/// Every name in every extracted table resolves back to itself, and the
/// index says which function did it.
#[test]
fn every_name_in_a_shipped_table_survives_the_round_trip() {
    for pair in pairs("name table round trip") {
        let mut index = NameIndex::new("gta5");
        index.extend(pair.names.iter());

        for name in &pair.names {
            let found = index
                .resolve(HashFlavour::CaseFolding.of(name))
                .unwrap_or_else(|| panic!("{}: {name} did not resolve", pair.label));
            assert_eq!(
                found.flavour,
                HashFlavour::CaseFolding,
                "{}: {name} resolved under the wrong function",
                pair.label
            );
            // A collision is allowed and the first name wins, so the answer
            // is "a name that hashes the same", not necessarily this one.
            assert_eq!(
                HashFlavour::CaseFolding.of(found.name),
                HashFlavour::CaseFolding.of(name),
                "{}: {name} resolved to {} which does not hash the same",
                pair.label,
                found.name
            );
        }

        let folded_collisions = index
            .collisions()
            .iter()
            .filter(|c| c.flavour == HashFlavour::CaseFolding)
            .count();
        assert_eq!(
            index.sorted_names().len() + folded_collisions,
            index.names(),
            "{}: the index lost a name it did not report as a collision",
            pair.label
        );
        // Retail ships exactly one malformed entry across both installs;
        // more than one in a table means the parser changed.
        assert!(
            pair.skipped <= 1,
            "{}: {} entries skipped, which is not the shape retail ships",
            pair.label,
            pair.skipped
        );
    }
}
