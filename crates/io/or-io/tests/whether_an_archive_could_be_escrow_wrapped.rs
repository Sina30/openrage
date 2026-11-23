//! If an escrow-wrapped resource ever arrives, does anything notice?
//!
//! # ⚠ The row is struck, and this is what survives it
//!
//! U-203 asked for escrow detection. R14 struck it on the grounds that the word
//! appears nowhere in this tree and no staged fixture carries one — an argument
//! from the **corpus**, which R11 established is usually the wrong denominator.
//!
//! So it was asked of two shipped installs instead, with
//! `openrage-cli archives --game <root>`:
//!
//! | install | archives read | entries | unknown archive mode | unknown entry marker |
//! |---|---:|---:|---:|---:|
//! | Legacy | 5,067 | 383,939 | **0** | **0** |
//! | Enhanced | 5,075 | 382,754 | **0** | **0** |
//!
//! Zero archives unreadable in either. Every archive is NG, AES or OPEN; every
//! binary entry's high word is 0 or 1. **There is no example on this machine to
//! write a detector against**, across 10,142 archives and 766,693 entries — a
//! denominator two orders of magnitude past any corpus, and the reason the row
//! is struck rather than deferred.
//!
//! (Measured at the revision this landed, on retail Legacy and Enhanced. A
//! figure over an install stays prose per `docs/guides/Documentation_Standard.md`; what is
//! asserted below is the *mechanism*, which is what a future example would meet.)
//!
//! # What is asserted instead
//!
//! Not the zero — a test cannot reach an install. What it can hold is that the
//! two places an escrow marker would appear **surface it rather than swallow
//! it**, so the day a title update ships one the tool says so instead of
//! guessing. That is worth more than a detector written against nothing, and it
//! is the part that would otherwise rot: both are one-line behaviours that a
//! well-meaning simplification could remove.

use or_io::container::rpf::{RpfEncryption, RpfEntry};

/// An unknown encryption mode stays unknown, carrying its raw value.
///
/// ⚠ Collapsing the `Other` arm into a default — treating an unrecognised magic
/// as OPEN, or as an error with the value discarded — would make an escrow
/// archive indistinguishable from a corrupt one.
#[test]
fn an_unrecognised_encryption_mode_keeps_its_raw_value() {
    // Not any of the three, and not zero.
    const UNKNOWN: u32 = 0x0E5C_0000;

    let mode = RpfEncryption::from_raw(UNKNOWN);
    assert_eq!(
        mode,
        RpfEncryption::Other(UNKNOWN),
        "an unknown mode must be `Other`, not folded into a known one"
    );
    assert_eq!(
        mode.to_raw(),
        UNKNOWN,
        "and it must round-trip, or the evidence is gone by the time anybody \
         reads the report"
    );
    assert!(
        mode.needs_crypto(),
        "an unknown mode must be assumed to need a key. Treating it as \
         plaintext would hand a caller encrypted bytes and call them content."
    );

    for known in [
        RpfEncryption::None,
        RpfEncryption::Open,
        RpfEncryption::Aes,
        RpfEncryption::Ng,
    ] {
        assert_ne!(
            RpfEncryption::from_raw(known.to_raw()),
            RpfEncryption::Other(known.to_raw()),
            "{known:?} must not read as unknown, or every archive is a candidate \
             and the census says nothing"
        );
    }
}

/// A binary entry's high word survives as a number, not as a bool.
///
/// ⚠ The reader sets `encrypted = hi != 0`, so with the bool alone a mode
/// distinguished by a *marker* in that word would silently read as ordinary
/// encryption. `encrypted_raw` keeps the whole word.
#[test]
fn a_binary_entrys_marker_word_survives_as_a_number() {
    use std::io::Cursor;

    // An entry block with a high word that is neither 0 nor 1. Layout per
    // `RpfEntry::read`: name/size/offset in the first eight bytes, then `lo`
    // and `hi`.
    const MARKER: u32 = 0x0000_0002;
    let mut block = [0u8; 16];
    // A plausible non-directory, non-resource tag: name offset 0x10, a small
    // consumed size, and a file offset that is not the directory marker.
    block[0..2].copy_from_slice(&0x0010u16.to_le_bytes());
    block[2] = 0x40; // consumed size, low byte
    block[5..8].copy_from_slice(&[0x02, 0x00, 0x00]);
    block[8..12].copy_from_slice(&0x1000u32.to_le_bytes()); // lo: uncompressed size
    block[12..16].copy_from_slice(&MARKER.to_le_bytes()); // hi: the marker

    let entry = RpfEntry::read(&mut Cursor::new(block)).expect("an entry parses");
    let RpfEntry::Binary(binary) = entry else {
        panic!("the fixture must decode as a binary entry, got {entry:?}");
    };

    assert_eq!(
        binary.encrypted_raw, MARKER,
        "the whole high word must survive. A `bool` alone cannot tell an escrow \
         marker from `encrypted = 1`, which is the distinction U-203 was asking \
         for and the reason the census could answer it."
    );
    assert!(
        binary.encrypted,
        "and a non-zero marker must still read as encrypted, so nothing hands \
         the bytes over as plaintext while the marker is unrecognised"
    );
}

/// The census that re-measures the header's zero is still in the tree.
///
/// A clean checkout and CI have no install, so only the instrument's presence
/// can be held here.
#[test]
fn the_census_that_produced_that_zero_is_still_in_the_tree() {
    let census = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-container")
        .join("crates/tools/or-cli/src/archive_census.rs");

    assert!(
        census.is_file(),
        "{} is gone. It is the only thing that can re-measure the escrow zero \
         against a real install; without it the table in this file's header is \
         a number nobody can check.",
        census.display()
    );

    let source = std::fs::read_to_string(&census).expect("read the census");
    for wanted in ["encrypted_raw", "RpfEncryption::Other", "ESCROW"] {
        assert!(
            source.contains(wanted),
            "the census no longer mentions `{wanted}`, so it is no longer asking \
             the question this file's header reports the answer to"
        );
    }
}
