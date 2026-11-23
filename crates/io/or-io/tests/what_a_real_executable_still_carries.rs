//! What key material a shipped game executable actually contains.
//!
//! Point `OPENRAGE_GAME_ROOT` at one or more install roots, `;`-separated.
//! Nothing here runs without one, and the skip is recorded rather than
//! silent: no fixture can stand in for an executable. The two checks that
//! compare against NG material measure the snapshot `OPENRAGE_KEY_SNAPSHOT`
//! names, else the bundled one.

use std::path::{Path, PathBuf};

use or_io::container::rpf::crypto::{
    decode_magic_file, extract_keys_with, extract_keys_with_snapshot, key_coverage, ExeVerdict,
    ExtractionMode, KeyOrigin, SnapshotAgreement, SnapshotSource, SNAPSHOT_ENV,
};
use or_io::container::Gta5Keys;
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// The executables the tool tells a user to point at.
const GAME_EXECUTABLES: &[&str] = &["GTA5.exe", "GTA5_Enhanced.exe"];

fn user_snapshot(what: &str) -> Option<SnapshotSource> {
    match SnapshotSource::from_env().or_bundled() {
        SnapshotSource::None => {
            skip_install(
                what,
                format_args!("no key snapshot; set {SNAPSHOT_ENV} to one of your own"),
            );
            None
        }
        source => Some(source),
    }
}

fn decode(source: &SnapshotSource, aes_key: &[u8; 32]) -> Gta5Keys {
    let decoded = match source {
        SnapshotSource::File(path) => decode_magic_file(path, aes_key),
        #[cfg(feature = "bundled-keys")]
        SnapshotSource::Bundled => or_io::container::rpf::crypto::decode_magic_blob(
            or_io::container::rpf::crypto::magic::BUNDLED_SNAPSHOT,
            aes_key,
        ),
        SnapshotSource::None => panic!("user_snapshot never returns None"),
    };
    decoded.expect("the snapshot decodes under this executable's own AES key")
}

struct Subject {
    path: PathBuf,
    image: Vec<u8>,
    keys: Gta5Keys,
}

fn subjects(what: &str) -> Vec<Subject> {
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
        for name in GAME_EXECUTABLES {
            let path = root.join(name);
            if !path.is_file() {
                continue;
            }
            let keys = extract_keys_with_snapshot(
                &path,
                ExtractionMode::Full,
                &SnapshotSource::None,
                &|_| {},
            )
            .unwrap_or_else(|e| panic!("{} yielded no keys at all: {e}", path.display()));
            let image = std::fs::read(&path).expect("read the executable");
            out.push(Subject { path, image, keys });
        }
    }

    if out.is_empty() {
        skip_install(
            what,
            format_args!(
                "none of {GAME_EXECUTABLES:?} under {}",
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

fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into()
}

/// The AES key and the hash LUT are the whole of what a fingerprint search
/// still recovers, and they are recovered from every shipped executable.
#[test]
fn the_aes_key_and_the_lut_come_from_the_executable() {
    for subject in subjects("AES + LUT from a shipped executable") {
        let provenance = subject.keys.provenance;
        assert_eq!(
            provenance.aes_key,
            KeyOrigin::Executable,
            "{}: AES key did not come from the executable",
            name(&subject.path)
        );
        assert_eq!(
            provenance.lut,
            KeyOrigin::Executable,
            "{}: hash LUT did not come from the executable",
            name(&subject.path)
        );
        assert!(subject.keys.has_aes());
    }
}

/// ⚠ **No NG key material is in a shipped executable**, so no search of any
/// kind recovers it and every NG key in the tool comes from a snapshot. The
/// AWC key is, and this is where the fingerprint that finds it is checked.
///
/// It searches the whole image for each of the 101 NG key blocks and 272
/// decrypt tables the snapshot holds, verbatim and at any alignment, and
/// requires the AES key it *does* find as a control. A fingerprint search
/// missing them would leave the verbatim search finding them; both missing is
/// absence.
#[test]
fn no_ng_key_material_is_in_a_shipped_executable() {
    let what = "NG key material in a shipped executable";
    let subjects = subjects(what);
    if subjects.is_empty() {
        return;
    }
    let Some(source) = user_snapshot(what) else {
        return;
    };
    for subject in subjects {
        let label = name(&subject.path);

        let snapshot = decode(&source, &subject.keys.aes_key);

        let mut needles: Vec<Vec<u8>> = vec![subject.keys.aes_key.to_vec()];
        let control = 0;
        let ng_keys = 1..1 + snapshot.ng_keys.len();
        needles.extend(snapshot.ng_keys.iter().map(|block| block.to_vec()));
        let tables = ng_keys.end..ng_keys.end + snapshot.ng_decrypt_tables.len() * 16;
        needles.extend(
            snapshot
                .ng_decrypt_tables
                .iter()
                .flat_map(|round| round.iter())
                .map(|lane| lane.iter().flat_map(|v| v.to_le_bytes()).collect()),
        );
        let awc = tables.end;
        needles.push(
            snapshot
                .awc_key
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect(),
        );

        let found = find_all(&subject.image, &needles);

        assert!(
            found[control],
            "{label}: the control failed — the AES key is not in the image either, so a \
             miss below would say nothing"
        );

        let ng_keys_present = found[ng_keys.clone()].iter().filter(|f| **f).count();
        assert_eq!(
            ng_keys_present,
            0,
            "{label}: {ng_keys_present} of {} NG key blocks are in the image after all",
            snapshot.ng_keys.len()
        );

        let tables_present = found[tables].iter().filter(|f| **f).count();
        assert_eq!(
            tables_present, 0,
            "{label}: {tables_present} NG decrypt tables are in the image after all"
        );
        // ⚠ The AWC key is the opposite case: verbatim in the image, and the
        // extractor's own fingerprint has to recover exactly the snapshot's.
        assert!(found[awc], "{label}: the AWC key is not in the image");
        assert_eq!(
            subject.keys.provenance.awc_key,
            KeyOrigin::Executable,
            "{label}"
        );
        assert_eq!(subject.keys.awc_key, snapshot.awc_key, "{label}");

        assert_eq!(
            subject.keys.provenance.ng_keys,
            KeyOrigin::Absent,
            "{label}: NG keys claim a source when no snapshot was supplied"
        );
        assert_eq!(subject.keys.provenance.ng_decrypt_tables, KeyOrigin::Absent);
        assert!(!subject.keys.has_ng());
    }
}

/// The snapshot and the executable overlap on exactly one field, and it is
/// the only independent evidence that the snapshot describes this build.
#[test]
fn the_snapshot_agrees_with_the_executable_on_the_one_field_both_supply() {
    let what = "snapshot agreement with a shipped executable";
    let subjects = subjects(what);
    if subjects.is_empty() {
        return;
    }
    let Some(snapshot) = user_snapshot(what) else {
        return;
    };
    let origin = match snapshot {
        SnapshotSource::File(_) => KeyOrigin::SnapshotFile,
        _ => KeyOrigin::EmbeddedSnapshot,
    };
    for subject in subjects {
        let keys =
            extract_keys_with_snapshot(&subject.path, ExtractionMode::Full, &snapshot, &|_| {})
                .expect("extraction with the user's snapshot");
        assert_eq!(
            keys.provenance.snapshot_agreement,
            SnapshotAgreement::Agrees,
            "{}: the snapshot describes a different build",
            name(&subject.path)
        );
        assert_eq!(keys.provenance.ng_keys, origin);
        assert_eq!(keys.provenance.awc_key, KeyOrigin::Executable);
        assert!(keys.has_ng() && keys.has_awc());
        assert!(
            find(&subject.image, &keys.lut).is_some(),
            "{}: the LUT kept is not the one in the image",
            name(&subject.path)
        );
    }
}

/// A user's snapshot that does not decode falls back to the bundled one
/// rather than leaving every NG archive shut.
#[cfg(feature = "bundled-keys")]
#[test]
fn an_undecodable_user_snapshot_falls_back_to_the_bundled_one() {
    let what = "an undecodable user snapshot";
    let subjects = subjects(what);
    if subjects.is_empty() {
        return;
    }
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let broken = scratch.path().join("undecodable_magic.dat");
    std::fs::write(&broken, b"not a key snapshot").expect("write the broken snapshot");
    let snapshot = SnapshotSource::File(broken);
    for subject in subjects {
        let label = name(&subject.path);
        let keys =
            extract_keys_with_snapshot(&subject.path, ExtractionMode::Full, &snapshot, &|_| {})
                .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(keys.has_ng(), "{label}: the bundled snapshot was not tried");
        assert_eq!(keys.provenance.ng_keys, KeyOrigin::EmbeddedSnapshot);
        assert_eq!(
            keys.provenance.snapshot_agreement,
            SnapshotAgreement::Agrees
        );
    }
}

/// The extractor's coverage of its five key classes, per executable.
///
/// ⚠ It covers every `GTA5*.exe` under the roots rather than only the two a
/// user is told to pick, because the unpacked dumps beside some installs are
/// the only places an older build's material could still sit. What it must
/// hold for the two shipped executables is asserted; the rest is recorded.
#[test]
fn what_the_extractor_recovers_per_executable() {
    let what = "the extractor's coverage per executable";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(
            what,
            format_args!("no install root; set {GAME_ROOT_ENV} to a ;-separated list"),
        );
        return;
    }
    let mut shipped = 0usize;
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let file = name(p);
                file.starts_with("GTA5") && file.ends_with(".exe")
            })
            .collect();
        paths.sort();
        for path in paths {
            let coverage = key_coverage(&path).expect("scan the executable");
            println!("{}: {}", path.display(), coverage.report());
            if GAME_EXECUTABLES.contains(&name(&path).as_str()) {
                shipped += 1;
                assert_eq!(coverage.aes_key, (1, 1), "{}", path.display());
                assert_eq!(coverage.lut, (1, 1), "{}", path.display());
                assert_eq!(coverage.awc_key, (1, 1), "{}", path.display());
                assert_eq!(coverage.ng_keys.0, 0, "{}", path.display());
                assert_eq!(coverage.ng_decrypt_tables.0, 0, "{}", path.display());
            }
        }
    }
    assert!(shipped > 0, "no shipped game executable under {roots:?}");
}

/// ⚠ Legacy and Enhanced ship the **same** AES key at different offsets, so
/// the per-edition keys caches hold identical payloads. Relocation is the one
/// thing the fingerprint search buys over a fixed offset, and this is where
/// it pays.
#[test]
fn both_editions_ship_the_same_aes_key_in_different_places() {
    let subjects = subjects("AES key across editions");
    if subjects.len() < 2 {
        if !subjects.is_empty() {
            skip_install(
                "AES key across editions",
                "only one edition is installed; this needs both",
            );
        }
        return;
    }

    let first = &subjects[0];
    let mut offsets = Vec::new();
    for subject in &subjects {
        assert_eq!(
            subject.keys.aes_key,
            first.keys.aes_key,
            "{} ships a different AES key from {}",
            name(&subject.path),
            name(&first.path)
        );
        offsets.push(find(&subject.image, &subject.keys.aes_key).expect("control"));
    }
    offsets.sort_unstable();
    offsets.dedup();
    assert!(
        offsets.len() > 1,
        "every edition holds the key at the same offset, so a fixed offset would have \
         done and this test is not measuring what it claims"
    );
}

/// Every executable an install ships lands on a verdict, and the verdicts
/// partition the way the diagnosis claims they do.
///
/// ⚠ Keying "is this the game" on the hash LUT calls `PlayGTAV.exe` a patched
/// game, because every Rockstar binary in the folder carries that table.
#[test]
fn every_executable_beside_the_game_gets_the_verdict_it_deserves() {
    let what = "verdicts over a whole install folder";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(
            what,
            format_args!("no install root; set {GAME_ROOT_ENV} to a ;-separated list"),
        );
        return;
    }

    // The LUT the verdict is told about has to be the one it would really
    // see, or this test cannot reproduce the condition it exists for: a
    // launcher that carries the table and no key.
    let lut = subjects(what)
        .first()
        .map(|subject| subject.keys.lut)
        .expect("a game executable to take the LUT from");

    let mut games = 0usize;
    let mut sharing_the_lut = 0usize;
    let mut unrelated = 0usize;
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("exe") {
                continue;
            }
            let Ok(image) = std::fs::read(&path) else {
                continue;
            };
            let lut_found = find(&image, &lut).is_some();
            let verdict = ExeVerdict::of(&image, lut_found);
            let yields_keys = extract_keys_with(&path, ExtractionMode::AesOnly, &|_| {}).is_ok();

            if yields_keys {
                games += 1;
                assert_eq!(
                    verdict,
                    ExeVerdict::KeyMaterialRotated,
                    "{}: yields keys but is not recognised as a game executable",
                    name(&path)
                );
                continue;
            }

            assert_ne!(
                verdict,
                ExeVerdict::KeyMaterialRotated,
                "{}: yields no keys yet is diagnosed as a patched game",
                name(&path)
            );
            if lut_found {
                sharing_the_lut += 1;
                assert_eq!(
                    verdict,
                    ExeVerdict::RockstarButNotTheGame,
                    "{}",
                    name(&path)
                );
            } else {
                unrelated += 1;
            }
        }
    }

    assert!(games > 0, "no game executable under {roots:?}");
    assert!(
        sharing_the_lut > 0,
        "no keyless binary carried the hash LUT, so this run did not exercise the case \
         the diagnosis was rebuilt for and a regression to the LUT would pass"
    );
    assert!(unrelated > 0, "nothing unrelated to compare against");
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Which of `needles` occur anywhere in `haystack`, at any alignment.
///
/// One pass, anchored on an 8-byte prefix, because the naive form is
/// `len(haystack) * len(needles)` and the real call is 374 needles over a
/// 47 MiB image.
fn find_all(haystack: &[u8], needles: &[Vec<u8>]) -> Vec<bool> {
    use std::collections::HashMap;

    const ANCHOR: usize = 8;

    let mut by_prefix: HashMap<[u8; ANCHOR], Vec<usize>> = HashMap::new();
    let mut found = vec![false; needles.len()];
    for (i, needle) in needles.iter().enumerate() {
        assert!(
            needle.len() >= ANCHOR,
            "needle {i} is shorter than the anchor, so it would never be looked up"
        );
        let mut prefix = [0u8; ANCHOR];
        prefix.copy_from_slice(&needle[..ANCHOR]);
        by_prefix.entry(prefix).or_default().push(i);
    }

    if haystack.len() < ANCHOR {
        return found;
    }
    for (at, window) in haystack.windows(ANCHOR).enumerate() {
        let prefix: [u8; ANCHOR] = window.try_into().expect("windows(ANCHOR)");
        let Some(candidates) = by_prefix.get(&prefix) else {
            continue;
        };
        for &i in candidates {
            let needle = &needles[i];
            if haystack[at..].starts_with(needle) {
                found[i] = true;
            }
        }
    }
    found
}

#[test]
fn the_absence_search_finds_a_planted_needle() {
    let mut image = vec![0xCCu8; 1 << 20];
    let planted: Vec<u8> = (0..=255u8).cycle().take(272).collect();
    // Deliberately off an 8-byte boundary: the fingerprint scan strides by 8
    // and this search must not inherit that blind spot.
    image[4097..4097 + planted.len()].copy_from_slice(&planted);

    let absent: Vec<u8> = planted.iter().map(|b| b ^ 0x01).collect();
    let found = find_all(&image, &[planted, absent]);
    assert_eq!(found, vec![true, false]);
}
