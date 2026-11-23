//! What an archive will weigh, before anything is written.
//!
//! The number is exact, not estimated: [`RpfBuilder::planned_size`] runs the
//! save through a sink that counts and discards, so the alignment, trailing pad
//! and per-entry compression decision have no second copy to drift from, and
//! `the_preview_is_the_size_the_save_writes` asserts equality, not a tolerance.

use std::sync::Arc;

use or_io::container::rpf::crypto::NoOpCryptoProvider;
use or_io::container::rpf::{Defrag, RpfArchive, RpfBuilder, RpfEncryption};

/// A body that compresses well, so the deflate path is exercised rather than
/// skipped as not worth it.
fn compressible(len: usize) -> Vec<u8> {
    b"the quick brown fox "
        .iter()
        .copied()
        .cycle()
        .take(len)
        .collect()
}

/// An RSC7-looking body, so the **resource** branch of the save is exercised.
///
/// ⚠ The builder sniffs `RSC7` at save time and emits a resource entry with the
/// `system_info` and `graphics_info` read out of the body itself. A test using
/// only binary bodies would never reach that branch, and it is the branch that
/// writes a different entry shape and a different body prologue.
fn resource_body(len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&0x3752_4352u32.to_le_bytes()); // "RSC7"
    out.extend_from_slice(&165u32.to_le_bytes()); // version
    out.extend_from_slice(&0x9000_0000u32.to_le_bytes()); // system_info
    out.extend_from_slice(&0x9000_0000u32.to_le_bytes()); // graphics_info
    out.extend(compressible(len.saturating_sub(16)));
    out
}

fn a_builder(encryption: RpfEncryption) -> RpfBuilder {
    let mut builder =
        RpfBuilder::default().encrypted(encryption, Arc::new(NoOpCryptoProvider::new()));
    builder
        .insert_with("readme.txt", compressible(4096), true)
        .unwrap()
        .insert_with("data/one.dat", compressible(100), false)
        .unwrap()
        .insert_with("data/nested/deep.dat", compressible(70_000), true)
        .unwrap()
        .insert_with("models/a.ydr", resource_body(9_000), false)
        .unwrap()
        .insert_with("models/b.ytd", resource_body(300), false)
        .unwrap();
    builder
}

#[test]
fn the_preview_is_the_size_the_save_writes() {
    let builder = a_builder(RpfEncryption::Open);

    let planned = builder.planned_size().expect("a preview");
    let written = builder.to_bytes().expect("a save").len() as u64;

    eprintln!("\n=== planned {planned} bytes, wrote {written} ===");
    assert_eq!(
        planned, written,
        "the preview and the save disagree. They run the same code, so this can \
         only be the counting sink — check that it reports the high-water mark \
         rather than the final position, because the builder seeks backwards to \
         patch the header after writing every body."
    );
    // A preview over an empty builder agrees with its save about zero, so the
    // output must exceed the page that header and table alone could fill. The
    // bodies compress 70 KB to a few hundred bytes, which is why an estimate
    // cannot stand in for the preview.
    assert!(
        written > 8192,
        "the fixture produced {written} bytes, which is one page and could be \
         all header and table — the two numbers would then agree about nothing"
    );
}

/// ⚠ The number is not the same under every mode, so the preview must be asked
/// the same question the save will be.
#[test]
fn a_preview_for_one_encryption_mode_is_not_a_preview_for_another() {
    let open = a_builder(RpfEncryption::Open).planned_size().expect("open");
    let none = a_builder(RpfEncryption::None).planned_size().expect("none");

    // `NoOpCryptoProvider` pads neither mode, so these agree. The builder
    // carries the mode because a *real* provider block-pads, and a preview that
    // ignored it would look correct here and be wrong when encrypted.
    assert_eq!(
        open, none,
        "the no-op provider pads nothing, so the two modes weigh the same \
         through it. If this ever differs, the preview has started depending on \
         the mode in a way the save does not."
    );
}

/// A defrag preview reports the slack a rebuild would reclaim.
///
/// ⚠ A defrag must carry each entry's compression decision
/// (`RpfBinaryEntry::is_compressed`; a resource entry carries its own).
/// Rebuilding with a fresh choice turned a 12,288-byte archive into 81,920,
/// a change of encoding the preview would report as slack.
#[test]
fn a_rebuild_reports_the_slack_it_would_reclaim() {
    let original = a_builder(RpfEncryption::Open).to_bytes().expect("save");

    // An archive this builder wrote has no slack, so rebuilding it from its own
    // contents must give the same size.
    let archive = RpfArchive::from_bytes(original.clone()).expect("reopens");
    let rebuild = Defrag::plan(&archive, |_, _| {}).expect("a preview of the rebuild");
    assert_eq!(rebuild.files, 5, "every file came back out");
    let planned = rebuild.planned;

    eprintln!(
        "\n=== defrag preview: {} bytes now, {planned} after a rebuild ({:+}) ===",
        original.len(),
        planned as i64 - original.len() as i64
    );
    assert_eq!(
        planned,
        original.len() as u64,
        "an archive this builder just wrote was rebuilt to a different size, so \
         the rebuild is not idempotent and a defrag preview would be reporting a \
         saving that is really a change of layout"
    );
}
