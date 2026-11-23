//! The owner's server pack is read for the checks that need a real resource, and it is never part of the repository: no tracked
//! file is a copy of one of its files, and no tracked file names the folder it sits in.
//!
//! The pack is found through `OPENRAGE_REFERENCE_PACK`. Without it the comparison of files skips, which is not a pass; the
//! scan for its place on this machine needs no pack and runs always. Each half is shown to find what it looks for in a folder
//! built for the purpose before it is let loose on the repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const WHAT: &str = "no_file_of_the_reference_pack_is_in_the_repository";

/// A file shorter than this is too common to say anything about where it came from.
const SHORTEST: u64 = 64;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-wardrobe")
        .to_path_buf()
}

/// The files git tracks, as absolute paths, or none when this is not a git checkout.
fn tracked() -> Option<Vec<PathBuf>> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let names = String::from_utf8_lossy(&out.stdout);
    Some(
        names
            .split('\0')
            .filter(|n| !n.is_empty())
            .map(|n| root().join(n))
            .collect(),
    )
}

fn digest(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(
        Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

fn files_of(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => out.extend(files_of(&path)),
            Ok(kind) if kind.is_file() => out.push(path),
            _ => {}
        }
    }
    out
}

/// The tracked files that are a copy of a file of `reference`, by size and then by digest.
fn copies_of(reference: &Path, tracked: &[PathBuf]) -> Vec<String> {
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for file in files_of(reference) {
        let size = std::fs::metadata(&file).map_or(0, |m| m.len());
        if size >= SHORTEST {
            by_size.entry(size).or_default().push(file);
        }
    }
    let mut seen: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut copies = Vec::new();
    for file in tracked {
        let size = std::fs::metadata(file).map_or(0, |m| m.len());
        let Some(candidates) = by_size.get(&size) else {
            continue;
        };
        let Some(ours) = digest(file) else {
            continue;
        };
        let same = candidates
            .iter()
            .any(|c| *seen.entry(c.clone()).or_insert_with(|| digest(c)) == Some(ours.clone()));
        if same {
            copies.push(file.display().to_string());
        }
    }
    copies
}

/// The tracked text files that contain any of `needles`.
fn naming(tracked: &[PathBuf], needles: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for file in tracked {
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        if bytes.len() > 8 << 20 || bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        if let Some(needle) = needles.iter().find(|n| text.contains(n.as_str())) {
            found.push(format!("{} names {needle}", file.display()));
        }
    }
    found
}

#[test]
fn a_copy_of_a_file_of_the_reference_and_a_file_that_names_its_folder_are_found() {
    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("somewhere-else");
    std::fs::create_dir_all(reference.join("stream")).unwrap();
    let body = vec![7u8; 4096];
    std::fs::write(reference.join("stream/a.ydd"), &body).unwrap();
    std::fs::write(reference.join("stream/tiny.txt"), b"x").unwrap();

    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let copy = repo.join("fixtures/renamed.bin");
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::write(&copy, &body).unwrap();
    let same_size = repo.join("other.bin");
    std::fs::write(&same_size, vec![8u8; 4096]).unwrap();
    let tiny = repo.join("tiny.txt");
    std::fs::write(&tiny, b"x").unwrap();
    let doc = repo.join("notes.md");
    std::fs::write(&doc, format!("the pack is at {}", reference.display())).unwrap();

    let tracked = [copy.clone(), same_size, tiny, doc.clone()];
    assert_eq!(
        copies_of(&reference, &tracked),
        [copy.display().to_string()],
        "a renamed copy is found by its bytes, and a file of the same size is not one, nor a file too short to say"
    );
    let named = naming(&tracked, &[reference.display().to_string()]);
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(named[0].starts_with(&doc.display().to_string()));
}

#[test]
fn no_tracked_file_names_the_folder_the_reference_pack_sits_in() {
    let Some(tracked) = tracked() else {
        or_test_support::skip_corpus(
            WHAT,
            "this is not a git checkout, so there is no list of tracked files",
        );
        return;
    };
    // ⚠ Written in two parts, so that this file does not name what it looks for.
    let mut needles = vec![["old-unity-packs", "-backup"].concat()];
    if let Some(place) = std::env::var_os("OPENRAGE_REFERENCE_PACK") {
        needles.push(place.to_string_lossy().replace('\\', "/"));
        needles.push(place.to_string_lossy().into_owned());
    }
    needles.retain(|n| n.len() > 8);
    let named = naming(&tracked, &needles);
    assert!(named.is_empty(), "{named:#?}");
}

#[test]
fn no_tracked_file_is_a_copy_of_a_file_of_the_reference_pack() {
    let Some(reference) = std::env::var_os("OPENRAGE_REFERENCE_PACK")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        or_test_support::skip_corpus(
            WHAT,
            "OPENRAGE_REFERENCE_PACK names no folder; it is the owner's server pack, in no repository",
        );
        return;
    };
    let Some(tracked) = tracked() else {
        or_test_support::skip_corpus(
            WHAT,
            "this is not a git checkout, so there is no list of tracked files",
        );
        return;
    };
    let copies = copies_of(&reference, &tracked);
    assert!(
        copies.is_empty(),
        "these tracked files are copies of files of the reference pack, which is never copied into the repository: {copies:#?}"
    );
}
