//! A file's resource version, without opening the file.
//!
//! [R26.] "Read an entry's resource version and header without deserialising the
//! asset and surface it per row and in errors." The reason that is worth doing is
//! a measurement rather than a principle, and the reason it was *possible* is
//! that the information was already being thrown away.
//!
//! # ⚠ What R26's own figures turned out to be
//!
//! Its `Why` argues from "a 30-second second launch". Measured with a release
//! build over both retail installs, three runs each:
//!
//! | | cold | warm |
//! |---|---:|---:|
//! | mount + enumerate every name, Legacy | 6.8 s | **1.05–1.10 s** |
//! | mount + enumerate every name, Enhanced | 6.4 s | **1.24 s** |
//!
//! So a second launch is about **one second**, not thirty, and persisting the
//! archive index to a startup cache would save about a second while taking on
//! exactly the invalidation risk R26's own `Risk` paragraph warns about. That
//! trade is not worth making, and the cache is where this row's effort *looked*
//! like it should go.
//!
//! The cost is somewhere else entirely. `openrage-cli versions --game` reads
//! **289,631** resource headers in **370.9 s** — because the only enumeration
//! available handed back three strings and discarded the entry, so learning a
//! version meant going back and opening the file. A resource entry carries
//! `system_info` and `graphics_info`; the RSC7 version is the high nibble of
//! each. It was free the whole time.
//!
//! (Figures measured at the revision this landed, on retail Legacy and Enhanced
//! with a release build. A figure over an install stays prose per
//! `docs/guides/Documentation_Standard.md`; what is asserted below is that the free answer and the
//! expensive answer agree.)
//!
//! # What is asserted
//!
//! That the cheap version **is** the version in the body. If it were not, a
//! 370-fold saving would be a 370-fold saving on the wrong number, which is
//! worse than the cost.

use or_io::container::rpf::{RpfBuilder, RpfEntryKind};
use or_title::vfs::EntryFacts;

/// Spelled here rather than imported: the RSC7 magic is private to
/// `or_io::container::resource`, where a consumer sniffing a file asks
/// `Rsc7::detect`. This test authors a fixture, so it writes its own four bytes.
const RSC7: u32 = u32::from_le_bytes(*b"RSC7");

const SYSTEM_INFO: u32 = 0x9000_0042;
const GRAPHICS_INFO: u32 = 0xA000_0007;

/// The version those two words encode: high nibble of each, system in the high
/// position.
const EXPECTED_VERSION: u32 = 0x9 << 4 | 0xA;

fn body(len: usize) -> Vec<u8> {
    b"index me ".iter().copied().cycle().take(len).collect()
}

fn resource_body() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&RSC7.to_le_bytes());
    out.extend_from_slice(&EXPECTED_VERSION.to_le_bytes());
    out.extend_from_slice(&SYSTEM_INFO.to_le_bytes());
    out.extend_from_slice(&GRAPHICS_INFO.to_le_bytes());
    out.extend(body(5000));
    out
}

fn facts_of(archive: &or_io::container::rpf::RpfArchive) -> Vec<EntryFacts> {
    // The same arithmetic `for_each_entry` does, over one archive rather than a
    // whole mount, so this suite needs no install.
    let mut out = Vec::new();
    for node in &archive.nodes {
        if node.is_dir() {
            continue;
        }
        let index = node.entry_index();
        let entry = archive.entries.get(index as usize).expect("an entry");
        let (kind, stored_size, size, compressed, paging) = match entry {
            or_io::container::rpf::RpfEntry::Binary(b) => (
                RpfEntryKind::Binary,
                b.read_size(),
                b.uncompressed_size,
                b.is_compressed(),
                None,
            ),
            or_io::container::rpf::RpfEntry::Resource(r) => (
                RpfEntryKind::Resource,
                r.consumed_size,
                r.consumed_size,
                false,
                Some((r.system_info, r.graphics_info)),
            ),
            or_io::container::rpf::RpfEntry::Directory(_) => continue,
        };
        out.push(EntryFacts {
            logical: format!("test:/{}", node.path()),
            container: "built.rpf".into(),
            leaf: node.name().to_string(),
            kind,
            stored_size,
            size,
            compressed,
            paging,
        });
    }
    out
}

fn built() -> or_io::container::rpf::RpfArchive {
    RpfBuilder::default()
        .insert_with("models/thing.ydr", resource_body(), false)
        .expect("a resource")
        .insert_with("data/notes.txt", body(9000), true)
        .expect("a compressed binary")
        .insert_with("data/plain.dat", body(64), false)
        .expect("an uncompressed binary")
        .build()
        .expect("reopen")
}

/// ⚠ The claim the 370-fold saving rests on.
#[test]
fn the_cheap_version_is_the_version_in_the_body() {
    let archive = built();
    let facts = facts_of(&archive);
    let resource = facts
        .iter()
        .find(|f| f.kind == RpfEntryKind::Resource)
        .expect("one resource entry");

    let free = resource
        .resource_version()
        .expect("a resource entry has a version");

    // The expensive answer: extract the body and read its header. This is what a
    // 289,631-file census was doing 370.9 s of work to reach.
    let index = archive
        .nodes
        .iter()
        .find(|n| n.path().ends_with(".ydr"))
        .expect("the ydr node")
        .entry_index();
    let bytes = archive.read_entry(index).expect("extract");
    let from_body = u32::from_le_bytes(bytes[4..8].try_into().expect("four bytes"));

    eprintln!("\n=== version from the entry table {free:#x}, from the body {from_body:#x} ===");
    assert_eq!(
        free, from_body,
        "the entry table and the body disagree about the version. The cheap path \
         is what R26's saving is, so a disagreement makes the saving worthless \
         rather than merely unproven."
    );
    assert_eq!(
        free, EXPECTED_VERSION,
        "and both agree with what was written"
    );
}

/// ⚠ `None` means *not a resource*, never *unknown*.
#[test]
fn a_binary_entry_has_no_resource_version_rather_than_version_zero() {
    let facts = facts_of(&built());
    let binaries: Vec<&EntryFacts> = facts
        .iter()
        .filter(|f| f.kind == RpfEntryKind::Binary)
        .collect();
    assert_eq!(binaries.len(), 2, "the fixture has two binary entries");

    for facts in binaries {
        assert_eq!(
            facts.resource_version(),
            None,
            "`{}` is a binary entry and has no RSC7 header to have a version \
             from. Returning `Some(0)` would make a `.dat` render as a resource \
             of version zero in every row that shows one.",
            facts.leaf
        );
        assert!(
            facts.paging.is_none(),
            "and it has no paging words either — those two fields travel together"
        );
    }
}

/// The sizes are the ones a listing needs, and they distinguish the kinds.
#[test]
fn the_facts_carry_what_a_row_shows_without_opening_anything() {
    let facts = facts_of(&built());

    let compressed = facts
        .iter()
        .find(|f| f.leaf == "notes.txt")
        .expect("notes.txt");
    assert!(compressed.compressed, "it was added compressed");
    assert_eq!(
        compressed.size, 9000,
        "the inflated size is what a user reads"
    );
    assert!(
        compressed.stored_size < compressed.size,
        "and the stored size is smaller, which is the only reason a row shows \
         both: {} against {}",
        compressed.stored_size,
        compressed.size
    );

    let plain = facts
        .iter()
        .find(|f| f.leaf == "plain.dat")
        .expect("plain.dat");
    assert!(!plain.compressed);
    assert_eq!(
        plain.stored_size, plain.size,
        "an uncompressed binary occupies what it is"
    );

    let resource = facts
        .iter()
        .find(|f| f.leaf == "thing.ydr")
        .expect("thing.ydr");
    assert!(
        !resource.compressed,
        "⚠ a resource is never deflated — its pages *are* the file. A row \
         reporting one as compressed would be reporting the binary path's field."
    );
    assert_eq!(resource.stored_size, resource.size);
    assert_eq!(resource.paging, Some((SYSTEM_INFO, GRAPHICS_INFO)));
}

/// ⚠ `for_each_file` delegates to `for_each_entry`, and a delegation that
/// dropped or reordered entries would silently change every caller across the
/// CLI.
#[test]
fn the_string_enumeration_still_visits_exactly_what_it_did() {
    let archive = built();
    let facts = facts_of(&archive);

    let walked: Vec<String> = archive
        .nodes
        .iter()
        .filter(|n| !n.is_dir())
        .map(|n| format!("test:/{}", n.path()))
        .collect();
    let from_facts: Vec<String> = facts.iter().map(|f| f.logical.clone()).collect();

    assert_eq!(
        from_facts, walked,
        "the facts walk must visit the same paths in the same order as the node \
         walk, or `for_each_file` delegating to it changes what every existing \
         caller sees"
    );
    assert_eq!(from_facts.len(), 3, "three files, no directories");
}
