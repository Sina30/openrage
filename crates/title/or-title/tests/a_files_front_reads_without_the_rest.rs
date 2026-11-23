//! The front of a file in an archive reads without the rest of it.
//!
//! ⚠ A resource's front is the front of its stored body, which is deflated, so what a
//! reader of a header alone gets is a file cut short, which `Rsc7::open` opens to the front of
//! its payload.

use std::path::PathBuf;
use std::sync::Arc;

use or_io::container::resource::{Compression, ResourceEnvelope as _, Rsc7, YBN_FILE_VERSION};
use or_io::container::rpf::crypto::{NoOpCryptoProvider, RpfCryptoProvider};
use or_io::container::rpf::RpfBuilder;
use or_title::vfs::{build_mounted_game, DlcCutoff};
use or_title::{GtaInstall, MountedGame};

const MODEL: &str = "common:/levels/street_0.ybn";
const NOTES: &str = "common:/data/notes.txt";
/// What each stands in front of: 100,000 bytes a street's collision holds past its header.
const BODY: usize = 100_000;

fn noise(len: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9u32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

/// An RSC7 file of `BODY` bytes that do not deflate to less, at the version of a `.ybn`.
fn a_resource() -> Vec<u8> {
    let mut payload = noise(BODY);
    let seal = Rsc7::invent_single_chunk_seal(YBN_FILE_VERSION, payload.len()).expect("a page");
    payload.resize(seal.total_size() as usize, 0);
    Rsc7::reseal(&payload, &seal, Compression::fast()).expect("a file")
}

/// An install holding one resource and one text file, and the folder's guard.
fn install(tag: &str) -> (tempfile::TempDir, PathBuf, Vec<u8>) {
    let scratch = tempfile::Builder::new()
        .prefix(&format!("or-title-front-{tag}-"))
        .tempdir()
        .expect("mkdir");
    let root = scratch.path().to_path_buf();
    let resource = a_resource();
    RpfBuilder::default()
        .insert_with("levels/street_0.ybn", resource.clone(), false)
        .expect("a resource")
        .insert_with("data/notes.txt", b"0123456789".repeat(1000), false)
        .expect("a binary file")
        .save(root.join("common.rpf"))
        .expect("common.rpf");
    (scratch, root, resource)
}

fn mount(root: &std::path::Path) -> MountedGame {
    let crypto: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
    build_mounted_game(
        &GtaInstall {
            root: root.to_path_buf(),
        },
        crypto,
        DlcCutoff::BaseGame,
    )
    .expect("the synthetic install mounts")
}

#[test]
fn a_resources_front_is_the_front_of_its_file_and_its_header_reads_from_it() {
    let (_scratch, root, resource) = install("resource");
    let game = mount(&root);
    let whole = game.vfs.open(MODEL).expect("the file opens");
    assert!(
        whole.len() > BODY * 9 / 10,
        "{} bytes is not the file built",
        whole.len()
    );

    let front = game.vfs.open_head(MODEL, 4096).expect("its front reads");

    assert_eq!(front.len(), 4096, "no more of the archive is read");
    assert_eq!(front[..], whole[..4096], "and it is the front of the file");
    let cut = Rsc7::open(&front).expect("a payload cut short opens to its front");
    let opened = Rsc7::open(&resource).expect("the file built opens");
    assert!(cut.data.len() > 1000 && cut.data.len() < opened.data.len());
    assert_eq!(cut.data, opened.data[..cut.data.len()]);
}

#[test]
fn a_binary_files_front_is_cut_and_a_limit_past_a_file_gives_all_of_it() {
    let (_scratch, root, _) = install("binary");
    let game = mount(&root);
    let whole = game.vfs.open(NOTES).expect("the file opens");

    assert_eq!(
        game.vfs.open_head(NOTES, 12).as_deref(),
        Some(&whole[..12]),
        "a binary file is cut to the limit"
    );
    assert_eq!(game.vfs.open_head(NOTES, usize::MAX), Some(whole));
    assert_eq!(game.vfs.open_head("common:/data/none.txt", 12), None);
}
