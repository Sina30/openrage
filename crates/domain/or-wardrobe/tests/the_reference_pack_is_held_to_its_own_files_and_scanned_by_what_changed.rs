//! A clothing resource somebody built and a server runs is scanned with the rules of the Wardrobe: counted from its files
//! once, and then from what a session remembered of them.
//!
//! The reference is in no repository, so `OPENRAGE_REFERENCE_PACK` names its folder, the one that holds `stream/`. Without
//! it this suite skips, which is not a pass. The pack is only read: nothing is written in it, so what a session remembers
//! is handed from one look to the next in memory.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use or_project::content::facts::{files_read, forget_under, remember_from, remembered_under};
use or_wardrobe::import::{import, Options};
use or_wardrobe::{allocate, check, inspect, Depth};

const WHAT: &str = "the_reference_pack_is_held_to_its_own_files_and_scanned_by_what_changed";

fn reference() -> Option<PathBuf> {
    let path = std::env::var_os("OPENRAGE_REFERENCE_PACK").map(PathBuf::from);
    match path.filter(|p| p.join("stream").is_dir()) {
        Some(path) => Some(path),
        None => {
            or_test_support::skip_corpus(
                WHAT,
                "OPENRAGE_REFERENCE_PACK names no clothing resource; it is a pack of the owner's server, in no repository",
            );
            None
        }
    }
}

#[test]
fn the_pack_of_a_server_is_counted_once_and_then_read_by_what_changed() {
    let Some(root) = reference() else {
        return;
    };
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let w = &imported.wardrobe;

    let (reads, started) = (files_read(), Instant::now());
    let facts = inspect(&root, w, Depth::Measure);
    let cold = started.elapsed();
    let read = files_read() - reads;
    let found = check(w, &allocate(w), &facts);
    eprintln!(
        "cold: {} garments, {read} files read, {cold:?}; the rules found {} things",
        w.garments.len(),
        found.len()
    );
    assert!(read > 1000, "a pack of a server is a pack of many files");

    let mut by_rule: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &found {
        *by_rule.entry(f.code).or_default() += 1;
    }
    eprintln!("by rule: {by_rule:?}");

    // A new session is handed what the last one read, and reads nothing that did not change.
    let remembered = remembered_under(&root);
    forget_under(&root);
    assert!(remember_from(&remembered) > 1000);
    let (reads, started) = (files_read(), Instant::now());
    let again = inspect(&root, w, Depth::Measure);
    let warm = started.elapsed();
    eprintln!("warm: {} files read, {warm:?}", files_read() - reads);
    assert_eq!(files_read() - reads, 0, "the next session read no file");
    assert_eq!(again, facts, "and says what the first said");
    assert!(warm < cold);
}
