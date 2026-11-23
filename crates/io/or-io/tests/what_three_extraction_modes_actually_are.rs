//! Three extraction forms for a binary entry, two for a resource.
//!
//! ⚠ **The raw bytes of a resource are not a file.** A resource's sixteen-byte
//! header slot on disk holds **filler the writer put there**, and the real
//! `system_info` and `graphics_info` live in the *entry table*;
//! a decoded read rebuilds those sixteen bytes. So "save raw content"
//! on a `.ydr` produces something of the right length that no loader will open,
//! OpenRage's own included, and resources are two thirds of an install's
//! entries. [`BodyForm::is_loadable_for`] carries that.
//!
//! Every assertion here is over an archive **built in the test**, so it needs no
//! corpus and the header filler is known rather than inferred.

use std::sync::Arc;

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::container::rpf::{BodyForm, RpfBuilder, RpfEncryption, RpfEntry, RpfEntryKind};

/// A bare RSC7 header from the envelope, so this suite never spells the magic
/// (`nothing_outside_the_envelope_names_the_rsc7_shape` refuses one that does).
fn rsc7_header(version: u32) -> Vec<u8> {
    Rsc7::bare_header(version)
}

/// The header slot's length, taken from the envelope rather than asserted.
fn header_size() -> usize {
    Rsc7::bare_header(165).len()
}

/// Compresses well, so the deflate path is taken rather than skipped.
fn compressible(len: usize) -> Vec<u8> {
    b"reclaim the slack "
        .iter()
        .copied()
        .cycle()
        .take(len)
        .collect()
}

const SYSTEM_INFO: u32 = 0x9000_0042;
const GRAPHICS_INFO: u32 = 0xA000_0007;

fn resource_body() -> Vec<u8> {
    // The builder reads the two flag words out of the body to fill the entry in.
    let mut out = rsc7_header(165);
    out[8..12].copy_from_slice(&SYSTEM_INFO.to_le_bytes());
    out[12..16].copy_from_slice(&GRAPHICS_INFO.to_le_bytes());
    out.extend(compressible(6000));
    out
}

struct Built {
    archive: or_io::container::rpf::RpfArchive,
    binary: u32,
    resource: u32,
}

fn built() -> Built {
    let archive = RpfBuilder::default()
        .insert_with("data/notes.txt", compressible(9000), true)
        .expect("a compressed binary")
        .insert_with("models/thing.ydr", resource_body(), false)
        .expect("a resource")
        .build()
        .expect("save and reopen");

    let mut binary = None;
    let mut resource = None;
    for node in &archive.nodes {
        if node.is_dir() {
            continue;
        }
        match archive.entries.get(node.entry_index() as usize) {
            Some(RpfEntry::Binary(_)) => binary = Some(node.entry_index()),
            Some(RpfEntry::Resource(_)) => resource = Some(node.entry_index()),
            _ => {}
        }
    }
    Built {
        archive,
        binary: binary.expect("a binary entry"),
        resource: resource.expect("a resource entry"),
    }
}

#[test]
fn a_binary_entry_has_three_genuinely_different_forms() {
    let b = built();

    let stored = b
        .archive
        .read_form(b.binary, BodyForm::AsStored)
        .expect("as-stored");
    let decrypted = b
        .archive
        .read_form(b.binary, BodyForm::Decrypted)
        .expect("decrypted");
    let decoded = b
        .archive
        .read_form(b.binary, BodyForm::Decoded)
        .expect("decoded");

    eprintln!(
        "\n=== binary entry: as-stored {} / decrypted {} / decoded {} bytes ===",
        stored.len(),
        decrypted.len(),
        decoded.len()
    );

    assert_eq!(decoded, compressible(9000), "decoded is the original bytes");
    assert!(
        stored.len() < decoded.len(),
        "the stored form must still be deflated, or `--mode as-stored` is doing \
         the decompression it exists to avoid: {} against {}",
        stored.len(),
        decoded.len()
    );
    assert_eq!(
        stored, decrypted,
        "this archive is OPEN, so there is nothing to decrypt and those two \
         forms coincide. ⚠ That is why an encrypted fixture is the one below — \
         testing only OPEN would let `Decrypted` be a no-op alias forever."
    );
    assert_eq!(
        b.archive.read_entry(b.binary).expect("plain"),
        decoded,
        "the plain entry point must still mean `Decoded`, or every \
         caller in the tree quietly changed behaviour"
    );
}

/// ⚠ The three forms must be three under **encryption**, which is the only
/// configuration where `Decrypted` is distinguishable from `AsStored`.
#[test]
fn the_decrypted_form_is_not_an_alias_when_there_is_a_key() {
    // A provider that "encrypts" by inverting every byte: enough to make the
    // stored and decrypted forms differ without needing a retail key.
    struct Flip;
    impl or_io::container::rpf::crypto::RpfCryptoProvider for Flip {
        fn supports(&self, e: RpfEncryption) -> bool {
            matches!(e, RpfEncryption::Open | RpfEncryption::Aes)
        }
        fn decrypt(
            &self,
            _e: RpfEncryption,
            _t: or_io::container::CryptoTarget<'_>,
            buffer: &mut [u8],
        ) -> or_io::container::Result<()> {
            buffer.iter_mut().for_each(|b| *b = !*b);
            Ok(())
        }
        fn encrypt(
            &self,
            _e: RpfEncryption,
            _t: or_io::container::CryptoTarget<'_>,
            buffer: &mut [u8],
        ) -> or_io::container::Result<()> {
            buffer.iter_mut().for_each(|b| *b = !*b);
            Ok(())
        }
    }

    let archive = RpfBuilder::default()
        .encrypted(RpfEncryption::Aes, Arc::new(Flip))
        .insert_with("data/notes.txt", compressible(9000), true)
        .expect("add")
        .build()
        .expect("save encrypted and reopen");

    let index = archive
        .nodes
        .iter()
        .find(|n| !n.is_dir())
        .expect("one file")
        .entry_index();

    let stored = archive
        .read_form(index, BodyForm::AsStored)
        .expect("as-stored");
    let decrypted = archive
        .read_form(index, BodyForm::Decrypted)
        .expect("decrypted");
    let decoded = archive
        .read_form(index, BodyForm::Decoded)
        .expect("decoded");

    assert_ne!(
        stored, decrypted,
        "with a key in play the stored and decrypted forms must differ, or \
         `--mode as-stored` is silently handing back decrypted bytes"
    );
    assert_ne!(decrypted, decoded, "and decrypted is still deflated");
    assert_eq!(decoded, compressible(9000), "decoded is the original");
}

#[test]
fn the_stored_form_of_a_resource_is_not_a_loadable_file() {
    let b = built();

    let stored = b
        .archive
        .read_form(b.resource, BodyForm::AsStored)
        .expect("as-stored");
    let decoded = b
        .archive
        .read_form(b.resource, BodyForm::Decoded)
        .expect("decoded");

    eprintln!(
        "\n=== resource: as-stored begins {:02x?}, decoded begins {:02x?} ===",
        &stored[..4],
        &decoded[..4]
    );

    assert_eq!(
        stored.len(),
        decoded.len(),
        "the two forms are the same length — nothing inflates a resource, which \
         is why it has two forms and not three"
    );
    assert_ne!(
        &stored[..header_size()],
        &decoded[..header_size()],
        "the stored form must NOT hold the RSC7 header. If these agree, the \
         writer has stopped filling the header slot and this whole hazard has \
         gone away — in which case delete the `AsStored` special case for \
         resources rather than this test."
    );
    // Asked through the envelope, because
    // `nothing_outside_the_envelope_names_the_rsc7_shape` refuses a spelled magic.
    assert!(
        !Rsc7::detect(&stored),
        "raw resource bytes are not an RSC7 file, so nothing will open them"
    );
    assert!(Rsc7::detect(&decoded), "and the decoded form is one");
    assert_eq!(
        u32::from_le_bytes(decoded[8..12].try_into().expect("four bytes")),
        SYSTEM_INFO,
        "the page flags come from the entry table, which is the reason the \
         header has to be rebuilt at all"
    );
    assert_eq!(
        &stored[header_size()..],
        &decoded[header_size()..],
        "past the header the two are the same bytes, so the only thing `AsStored` \
         loses is the thing that makes it openable"
    );
}

/// The front of a resource reads without the rest of its body, which is what a reader of a
/// header alone asks for.
#[test]
fn a_resources_front_reads_without_the_rest_of_its_body() {
    let b = built();
    let whole = b.archive.read_entry(b.resource).expect("whole");
    assert!(
        whole.len() > 1000,
        "{} bytes is not a body to cut",
        whole.len()
    );

    let front = b.archive.read_prefix(b.resource, 1000).expect("front");

    assert_eq!(front.len(), 1000, "no more than the limit is read");
    assert_eq!(front[..], whole[..1000], "and it is the front of the file");
    assert_eq!(
        &front[8..12],
        &SYSTEM_INFO.to_le_bytes(),
        "under the header the entry table rebuilt"
    );
    assert_eq!(
        b.archive
            .read_prefix(b.resource, 3)
            .expect("a header")
            .len(),
        header_size(),
        "a limit under the header slot still reads it, which the entry table rebuilds"
    );
    assert_eq!(
        b.archive.read_prefix(b.resource, usize::MAX).expect("all"),
        whole,
        "a limit past the body reads all of it"
    );
}

/// `Decrypted` and `Decoded` are one answer for a resource, and that is stated.
#[test]
fn a_resource_has_no_separate_decrypted_form() {
    let b = built();
    let decrypted = b
        .archive
        .read_form(b.resource, BodyForm::Decrypted)
        .expect("decrypted");
    let decoded = b
        .archive
        .read_form(b.resource, BodyForm::Decoded)
        .expect("decoded");
    assert_eq!(
        decrypted, decoded,
        "nothing deflates a resource, so asking for `Decrypted` can only mean \
         the same thing as `Decoded`. A caller offering a user three choices for \
         a resource is offering two and a duplicate."
    );
}

/// The question a bulk job asks before writing thousands of files.
#[test]
fn a_form_says_whether_it_produces_something_openable() {
    assert!(BodyForm::Decoded.is_loadable_for(RpfEntryKind::Binary));
    assert!(BodyForm::Decoded.is_loadable_for(RpfEntryKind::Resource));

    assert!(
        !BodyForm::AsStored.is_loadable_for(RpfEntryKind::Binary),
        "still deflated"
    );
    assert!(
        !BodyForm::AsStored.is_loadable_for(RpfEntryKind::Resource),
        "no RSC7 header"
    );

    assert!(
        !BodyForm::Decrypted.is_loadable_for(RpfEntryKind::Binary),
        "decrypted but still deflated"
    );
    assert!(
        BodyForm::Decrypted.is_loadable_for(RpfEntryKind::Resource),
        "for a resource this form *is* the decoded one, so it is openable — the \
         asymmetry this predicate exists to carry"
    );

    for form in BodyForm::ALL {
        assert_eq!(
            BodyForm::parse(form.name()),
            Some(form),
            "{} must round-trip through its own name, because that name is what \
             a CLI flag and a UI label will carry",
            form.name()
        );
    }
    assert_eq!(
        BodyForm::parse("raw"),
        Some(BodyForm::AsStored),
        "`raw` is the word OpenIV uses and users will type; it has to resolve"
    );
    assert_eq!(BodyForm::parse("nonsense"), None);
}

/// What `read_prefix` asks of the file, measured by what the file can still answer: the archive is
/// read from disk each time, so cutting the file short after the front leaves a read of the whole
/// body to fail and a read of the front alone to go on.
#[test]
fn a_resources_front_is_asked_of_the_file_and_no_more() {
    const FRONT: usize = 1000;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("a.rpf");
    RpfBuilder::default()
        .insert_with("data/notes.txt", compressible(9000), true)
        .expect("a compressed binary")
        .insert_with("models/thing.ydr", resource_body(), false)
        .expect("a resource")
        .save(&path)
        .expect("saved");
    let archive = or_io::container::rpf::RpfArchive::open(&path).expect("opens");
    let (index, offset) = archive
        .nodes
        .iter()
        .filter(|node| !node.is_dir())
        .find_map(
            |node| match archive.entries.get(node.entry_index() as usize) {
                Some(RpfEntry::Resource(entry)) => Some((node.entry_index(), entry.file_offset)),
                _ => None,
            },
        )
        .expect("a resource entry");
    let whole = archive.read_entry(index).expect("the whole body");
    assert!(
        whole.len() > FRONT,
        "{} bytes is not a body to cut",
        whole.len()
    );

    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_len(offset + FRONT as u64))
        .expect("the file is cut after the front");

    assert!(
        archive.read_entry(index).is_err(),
        "the whole body is still readable from a file cut after its front, so the cut proves nothing"
    );
    let front = archive
        .read_prefix(index, FRONT)
        .expect("the front is asked of the file and nothing past it");
    assert_eq!(front[..], whole[..FRONT]);
}
