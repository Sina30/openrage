//! A save leaves nothing else reading the archive it replaced at the old
//! file's offsets: neither a file moved out of it nor an extraction from it.

use std::path::Path;

use or_io::container::rpf::BodyForm;

use super::document::{DocumentId, RpfNodeId, TextDocument};
use super::extract_job::{ExtractStart, TypeFilter};
use super::nested_archives_survive_a_save::{archive, body, errors, on_disk, save, EDITED};
use super::nothing_extracts_into_an_install::run_to_completion;
use super::opening_a_ydd::{finish_load, node_at};
use super::save_destination::SaveDestination;
use super::{AppState, DocPayload, DocumentKind};

/// `archives` written into `root`, opened as an install's tree.
pub(super) fn install(root: &Path, archives: &[(&str, Vec<u8>)]) -> (AppState, DocumentId) {
    for (name, bytes) in archives {
        std::fs::write(root.join(name), bytes).expect("the archive is written");
    }
    let mut state = AppState::new();
    state.open_game_root(root.to_path_buf());
    let tree = state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::GameRoot)
        .expect("the install's tree")
        .id;
    (state, tree)
}

/// `entry` edited in its tab and saved back into its archive, which saves that archive alone.
pub(super) fn save_alone(state: &mut AppState, tree: DocumentId, entry: RpfNodeId) {
    state.open_rpf_entry(tree, entry);
    let tab = finish_load(state).expect("the entry opens");
    let edited = state.document_mut(tab).expect("the entry's tab");
    edited.payload = DocPayload::Text(TextDocument::decode(EDITED));
    edited.mark_edited();
    assert!(state.save_active_document());
    state.choose_save_destination(SaveDestination::Archive);
}

/// ⚠ `moved.bin` is read by nothing before the saves, so no read cache holds
/// it, and once `a.rpf` is written without it, the entry it was listed at is
/// `stays.txt`'s.
#[test]
fn a_file_moved_between_two_archives_saves_whole_when_the_archive_it_left_is_saved_first() {
    const MOVED: &[u8] = b"the moved file";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("moved.bin", MOVED), ("stays.txt", b"stays")]),
            ),
            ("b.rpf", archive(&[("data/kept.txt", b"kept")])),
        ],
    );
    let moved = node_at(&mut state, tree, "a.rpf/moved.bin");
    let stays = node_at(&mut state, tree, "a.rpf/stays.txt");
    let into = node_at(&mut state, tree, "b.rpf/data");
    state.rpf_move_node(tree, moved, into).expect("the move");

    save_alone(&mut state, tree, stays);
    let a = on_disk(&state, &dir.path().join("a.rpf"));
    assert_eq!(
        body(&a, "stays.txt", BodyForm::Decoded),
        EDITED,
        "a.rpf was not saved alone: {:?}",
        errors(&state)
    );
    assert!(
        a.nodes.iter().all(|n| n.name() != "moved.bin"),
        "the moved file is still in a.rpf"
    );

    save(&mut state, tree);
    let b = on_disk(&state, &dir.path().join("b.rpf"));
    assert_eq!(
        body(&b, "moved.bin", BodyForm::Decoded),
        MOVED,
        "b.rpf wrote the moved file from the archive it left: {:?}",
        errors(&state)
    );
    assert_eq!(body(&b, "kept.txt", BodyForm::Decoded), b"kept");
}

/// ⚠ `m.rpf` has no bytes but the ones `a.rpf` stores for it, so `a.rpf`
/// saved alone before `b.rpf` would leave `b.rpf` writing it from a file that
/// no longer holds it. One save of the tree saves `b.rpf` first.
#[test]
fn a_nested_archive_moved_between_two_archives_holds_the_one_it_left_until_the_other_is_saved() {
    let nested = archive(&[("inside.txt", b"inside")]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("m.rpf", &nested), ("stays.txt", b"stays")]),
            ),
            ("b.rpf", archive(&[("data/kept.txt", b"kept")])),
        ],
    );
    let m = node_at(&mut state, tree, "a.rpf/m.rpf");
    let stays = node_at(&mut state, tree, "a.rpf/stays.txt");
    let into = node_at(&mut state, tree, "b.rpf/data");
    state.rpf_move_node(tree, m, into).expect("the move");

    save_alone(&mut state, tree, stays);
    assert!(
        errors(&state)
            .iter()
            .any(|e| e.contains("b.rpf/data/m.rpf")),
        "a.rpf saved alone did not name the archive moved out of it: {:?}",
        errors(&state)
    );
    let a_path = dir.path().join("a.rpf");
    assert!(
        on_disk(&state, &a_path)
            .nodes
            .iter()
            .any(|n| n.name() == "m.rpf"),
        "a.rpf was written while b.rpf still read m.rpf from it"
    );

    save(&mut state, tree);
    let b = on_disk(&state, &dir.path().join("b.rpf"));
    assert_eq!(body(&b, "m.rpf", BodyForm::AsStored), nested);
    let a = on_disk(&state, &a_path);
    assert!(a.nodes.iter().all(|n| n.name() != "m.rpf"));
    assert_eq!(body(&a, "stays.txt", BodyForm::Decoded), EDITED);
}

/// ⚠ Enough files that the job still runs once a save has rebuilt and written
/// the archive, and an edit that does not compress, so it moves every body
/// after it and a save that went ahead leaves the job reading the new file at
/// the old offsets.
#[test]
fn an_extraction_running_while_its_archive_is_saved_extracts_every_file_whole() {
    let files: Vec<(String, Vec<u8>)> = (0..600)
        .map(|n| {
            (
                format!("f{n:03}.bin"),
                format!("file {n} ").repeat(400).into_bytes(),
            )
        })
        .collect();
    let entries: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let edit: Vec<u8> = (0..64 * 1024u32)
        .map(|n| (n.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = install(dir.path(), &[("a.rpf", archive(&entries))]);
    let path = dir.path().join("a.rpf");
    let first = node_at(&mut state, tree, "a.rpf/f000.bin");
    let row = node_at(&mut state, tree, "a.rpf");
    state
        .update_rpf_entry_bytes(tree, first, edit.clone())
        .expect("the edit");

    let out = dir.path().join("out");
    let started =
        state.extract_rpf_node(tree, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Started { planned: 600 });
    save(&mut state, tree);
    run_to_completion(&mut state);

    let broken: Vec<&str> = files
        .iter()
        .filter(|(name, bytes)| {
            let started_from = if name == "f000.bin" { &edit } else { bytes };
            std::fs::read(out.join(name)).ok().as_ref() != Some(started_from)
        })
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        broken.is_empty(),
        "{} of {} extracted files are not what the extraction started from, among them {:?}",
        broken.len(),
        files.len(),
        broken.iter().take(5).collect::<Vec<_>>()
    );
    assert!(
        errors(&state)
            .iter()
            .any(|e| e.contains("extraction of a.rpf")),
        "a save during the extraction did not name it: {:?}",
        errors(&state)
    );

    save(&mut state, tree);
    assert_eq!(
        body(&on_disk(&state, &path), "f000.bin", BodyForm::Decoded),
        edit,
        "the save once the extraction was done: {:?}",
        errors(&state)
    );
}
