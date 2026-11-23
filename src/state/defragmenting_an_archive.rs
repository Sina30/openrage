//! Defragmenting an archive from the explorer: a size preview, a gated write and the one-time `.bak`.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::rpf::{RpfArchive, RpfBuilder, RpfEntry};

use super::document::{DocumentId, RpfNodeId};
use super::prefs::EditMode;
use super::{AppState, DocPayload, PendingRetailEdit, RetailEditDecision};

/// Every file's decoded bytes, by path.
fn contents(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let archive = RpfArchive::from_bytes(bytes.to_vec()).expect("the archive opens");
    let mut out = Vec::new();
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let body = archive.read_entry(node.entry_index());
        out.push((node.path().to_string(), body.expect("every entry extracts")));
    }
    out.sort();
    out
}

/// An archive whose largest file was moved to the end, leaving a hole where it was.
pub(super) fn fragmented() -> Vec<u8> {
    let mut bytes = RpfBuilder::default()
        .insert_with("a.bin", vec![0xA5; 8192], false)
        .expect("add")
        .insert_with("dir/b.txt", b"compressible ".repeat(400), true)
        .expect("add")
        .insert_with("c.bin", vec![0x5A; 1000], false)
        .expect("add")
        .to_bytes()
        .expect("save");

    let mut archive = RpfArchive::from_bytes(bytes.clone()).expect("opens");
    let moved = archive
        .entries
        .iter_mut()
        .find_map(|e| match e {
            RpfEntry::Binary(b) if b.uncompressed_size == 8192 => Some(b),
            _ => None,
        })
        .expect("a.bin");
    let at = moved.file_offset as usize;
    let body = bytes[at..at + 8192].to_vec();
    let end = bytes.len().next_multiple_of(512);
    bytes.resize(end, 0);
    bytes.extend_from_slice(&body);
    bytes.resize(bytes.len().next_multiple_of(512), 0);
    moved.file_offset = end as u64;

    // An `Open` archive's entry table is plain, straight after the 16-byte header.
    let mut toc = Cursor::new(&mut bytes[16..]);
    for entry in &archive.entries {
        entry.write(&mut toc).expect("entry");
    }
    bytes
}

/// The document `path` lies under, with `path` as an unopened archive node under its root.
fn with_archive(state: &mut AppState, path: &Path) -> (DocumentId, RpfNodeId) {
    let doc_id = state
        .documents
        .iter()
        .find(|d| d.path.as_deref().is_some_and(|root| path.starts_with(root)))
        .expect("a document holding the archive")
        .id;
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc_id).map(|d| &mut d.payload) else {
        panic!("a folder or install opens as an archive tree");
    };
    let root = rpf.session.root_id();
    let name = path.file_name().and_then(|n| n.to_str()).expect("a name");
    let node = rpf
        .session
        .add_unopened_disk_archive(root, name.to_string(), path.to_path_buf())
        .expect("the archive's node");
    (doc_id, node)
}

pub(super) fn settle(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while state.defrag.is_running() {
        state.poll_defrag();
        assert!(Instant::now() < deadline, "the defragment never reported");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn defragmenting_shrinks_the_archive_and_keeps_every_entry() {
    let original = fragmented();
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("x.rpf");
    std::fs::write(&path, &original).expect("archive");

    let mut state = AppState::new();
    state.open_folder(tmp.path().to_path_buf());
    let (doc_id, node) = with_archive(&mut state, &path);

    state.defragment_archive(doc_id, node);
    settle(&mut state);
    let offer = state.defrag.offer.as_ref().expect("a size preview");
    assert_eq!(offer.before, original.len() as u64);
    assert!(
        offer.after < offer.before,
        "the hole a.bin left is slack: {} after, {} before",
        offer.after,
        offer.before
    );
    assert_eq!(
        std::fs::read(&path).expect("archive"),
        original,
        "nothing is written before the user answers the preview"
    );
    let after = offer.after;

    state.answer_defrag(true);
    settle(&mut state);
    let written = std::fs::read(&path).expect("the defragmented archive");
    assert_eq!(
        written.len() as u64,
        after,
        "the preview is what was written"
    );
    assert_eq!(contents(&written), contents(&original));
    let bak = path.with_extension("rpf.bak");
    assert_eq!(std::fs::read(&bak).expect("the .bak"), original);

    // A second run must leave the `.bak` holding the file from before the first.
    state.defragment_archive(doc_id, node);
    settle(&mut state);
    state.answer_defrag(true);
    settle(&mut state);
    assert_eq!(
        contents(&std::fs::read(&path).expect("archive")),
        contents(&original)
    );
    assert_eq!(std::fs::read(&bak).expect("the .bak"), original);
}

/// ⚠ An install in `ReadOnly` refuses, and in `Everything` asks first.
#[test]
fn defragmenting_asks_the_gate() {
    let original = fragmented();
    let tmp = tempfile::tempdir().expect("tempdir");
    let install = tmp.path().join("GTAV");
    std::fs::create_dir_all(install.join("update")).expect("install dirs");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("marker");
    let shipped: PathBuf = install.join("update").join("update.rpf");
    std::fs::write(&shipped, &original).expect("archive");

    let mut state = AppState::new();
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.open_game_root(install.clone());
    let (doc_id, node) = with_archive(&mut state, &shipped);

    state.defragment_archive(doc_id, node);
    assert!(
        !state.defrag.is_running() && state.defrag.offer.is_none(),
        "`ReadOnly` starts nothing"
    );

    state.prefs.edit_mode = EditMode::Everything;
    state.defragment_archive(doc_id, node);
    assert!(!state.defrag.is_running(), "a shipped archive asks first");
    assert!(matches!(
        state.pending_retail_edit.as_ref().map(|p| &p.action),
        Some(PendingRetailEdit::Defragment { .. })
    ));
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    settle(&mut state);
    assert!(
        state.defrag.offer.is_some(),
        "confirming starts the preview"
    );

    // The write asks again, so a mode changed while the preview was up still
    // holds. Settings drops the session's allow with the mode, as here.
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.retail_edit_session_allow = false;
    state.answer_defrag(true);
    settle(&mut state);
    assert_eq!(std::fs::read(&shipped).expect("archive"), original);
    assert!(!shipped.with_extension("rpf.bak").exists());
}

/// The archive at `node` opened in the session, and its files' nodes.
fn opened(
    state: &mut AppState,
    doc_id: DocumentId,
    node: RpfNodeId,
    path: &Path,
) -> Vec<RpfNodeId> {
    let archive =
        super::rpf_archive_open::open_archive_from_disk(path, state.keys.provider()).expect("open");
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc_id).map(|d| &mut d.payload) else {
        panic!("an archive tree");
    };
    let id = rpf
        .session
        .register_archive(archive.clone(), Some(path.to_path_buf()))
        .expect("an id for the archive");
    if let Some(super::RpfNodeBacking::Archive { archive_id, .. }) =
        rpf.session.node_mut(node).map(|n| &mut n.backing)
    {
        *archive_id = Some(id);
    }
    let listing = super::expander::list_archive(archive).expect("list");
    rpf.session
        .populate_archive_children(node, id, &listing)
        .expect("populate");
    rpf.session
        .children_of(node)
        .into_iter()
        .filter(|&c| !rpf.session.node(c).is_some_and(|n| n.is_dir()))
        .collect()
}

fn rpf(state: &mut AppState, doc_id: DocumentId) -> &mut super::RpfDocument {
    match state.document_mut(doc_id).map(|d| &mut d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf,
        _ => panic!("an archive tree"),
    }
}

/// A folder holding the fragmented archive, opened, with its file nodes.
fn staged() -> (
    tempfile::TempDir,
    PathBuf,
    Vec<u8>,
    AppState,
    DocumentId,
    RpfNodeId,
    Vec<RpfNodeId>,
) {
    let original = fragmented();
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("x.rpf");
    std::fs::write(&path, &original).expect("archive");
    let mut state = AppState::new();
    state.open_folder(tmp.path().to_path_buf());
    let (doc_id, node) = with_archive(&mut state, &path);
    let files = opened(&mut state, doc_id, node, &path);
    (tmp, path, original, state, doc_id, node, files)
}

/// ⚠ The rebuild reads the file, so an unsaved edit or an open tab inside refuses, at the start and at the answer.
#[test]
fn defragmenting_refuses_unsaved_edits_and_open_tabs() {
    type Spoil = fn(&mut super::RpfDocument, DocumentId, RpfNodeId);
    let spoilers: [(&str, Spoil); 2] = [
        ("an unsaved edit", |rpf, _, file| {
            rpf.session
                .set_bytes_for(file, b"edited".to_vec())
                .expect("edit");
        }),
        ("an open tab", |rpf, doc_id, file| {
            rpf.entry_documents.insert(file, doc_id);
        }),
    ];
    for (what, spoil) in spoilers {
        let (_tmp, path, original, mut state, doc_id, node, files) = staged();
        spoil(rpf(&mut state, doc_id), doc_id, files[0]);
        state.defragment_archive(doc_id, node);
        assert!(
            !state.defrag.is_running() && state.defrag.offer.is_none(),
            "{what} refuses the start"
        );

        let (_tmp, path2, original2, mut state2, doc_id2, node2, files2) = staged();
        state2.defragment_archive(doc_id2, node2);
        settle(&mut state2);
        assert!(state2.defrag.offer.is_some());
        spoil(rpf(&mut state2, doc_id2), doc_id2, files2[0]);
        state2.answer_defrag(true);
        assert!(!state2.defrag.is_running(), "{what} refuses the write");
        assert_eq!(std::fs::read(&path2).expect("archive"), original2);
        assert_eq!(std::fs::read(&path).expect("archive"), original);
    }
}

/// While a defragment is on offer, nothing else writes the archive it read.
#[test]
fn nothing_else_writes_an_archive_being_defragmented() {
    let (_tmp, path, original, mut state, doc_id, node, files) = staged();
    state.defragment_archive(doc_id, node);
    settle(&mut state);
    assert!(state.defrag.offer.is_some());

    assert!(state.rpf_rename_node(doc_id, files[0], "z.bin").is_err());
    assert!(state
        .update_rpf_entry_bytes(doc_id, files[0], b"edited".to_vec())
        .is_err());
    assert!(state.rpf_rename_node(doc_id, node, "y.rpf").is_err());
    rpf(&mut state, doc_id)
        .session
        .set_bytes_for(files[0], b"edited".to_vec())
        .expect("edit");
    state.save_dirty_archives_inner(doc_id);
    assert_eq!(std::fs::read(&path).expect("archive"), original);
}

/// ⚠ A rebuild of the file as the preview read it would write the old contents over a later change.
#[test]
fn a_file_changed_after_the_preview_is_not_replaced() {
    let (_tmp, path, original, mut state, doc_id, node, _) = staged();
    state.defragment_archive(doc_id, node);
    settle(&mut state);
    assert!(state.defrag.offer.is_some());

    let mut changed = original.clone();
    changed.extend_from_slice(&[0; 512]);
    std::fs::write(&path, &changed).expect("an outside write");
    state.answer_defrag(true);
    settle(&mut state);
    assert_eq!(std::fs::read(&path).expect("archive"), changed);
    assert!(!path.with_extension("rpf.bak").exists());
}

/// ⚠ A base-game prompt confirmed while another archive's write runs must not drop that write's hold.
#[test]
fn a_confirmed_prompt_does_not_replace_a_running_defragment() {
    let original = fragmented();
    let tmp = tempfile::tempdir().expect("tempdir");
    let install = tmp.path().join("GTAV");
    std::fs::create_dir_all(install.join("update")).expect("install dirs");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("marker");
    let shipped = install.join("update").join("update.rpf");
    std::fs::write(&shipped, &original).expect("archive");
    let mods = tmp.path().join("mods");
    std::fs::create_dir_all(&mods).expect("mods");
    let loose = mods.join("x.rpf");
    std::fs::write(&loose, &original).expect("archive");

    let mut state = AppState::new();
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = EditMode::Everything;
    state.open_game_root(install.clone());
    state.open_folder(mods.clone());
    let (shipped_doc, shipped_node) = with_archive(&mut state, &shipped);
    let (loose_doc, loose_node) = with_archive(&mut state, &loose);

    state.defragment_archive(shipped_doc, shipped_node);
    assert!(
        state.pending_retail_edit.is_some(),
        "a shipped archive asks first"
    );
    state.defragment_archive(loose_doc, loose_node);
    settle(&mut state);
    state.answer_defrag(true);
    assert!(state.defrag.holds(&loose), "the loose archive's write runs");

    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert!(
        state.defrag.holds(&loose),
        "confirming the other prompt keeps the running write's hold"
    );
    settle(&mut state);
    assert!(
        (std::fs::read(&loose).expect("archive").len() as u64) < original.len() as u64,
        "the running write finished"
    );
    assert_eq!(std::fs::read(&shipped).expect("archive"), original);
}
