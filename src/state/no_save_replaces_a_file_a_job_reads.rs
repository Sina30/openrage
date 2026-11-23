//! No save replaces a file a job reads: a save of an archive, or of a file
//! inside a folder, is refused with the job named while a conversion, a DLC
//! pack's build or a comparison reads it.
//!
//! ⚠ A job reads until the per-frame tick takes what it ended with, so nothing
//! here ticks the jobs before it saves, and a fast job still holds its files.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::rpf::BodyForm;

use super::a_moved_or_deleted_archive_row_leaves_nothing_behind::folder_of;
use super::document::{DocumentId, TextDocument};
use super::document_io::empty_file_bytes_for_kind;
use super::every_add_lands_where_the_tree_shows_it::opened;
use super::jobs::JobKind;
use super::nested_archives_survive_a_save::{archive, body, errors, on_disk, save, EDITED};
use super::opening_a_ydd::{finish_load, node_at};
use super::save_destination::SaveDestination;
use super::{AppState, DocPayload, DocumentKind};
use crate::platform::recycle_bin;

const DEADLINE: Duration = Duration::from_secs(60);
pub(super) const CONVERSION: &str = "conversion of prop.ydr";

pub(super) fn running(state: &AppState, kind: JobKind) -> bool {
    (state.running_jobs().iter()).any(|job| job.kind == kind)
}

/// Tick the jobs until the one of `kind` has ended.
pub(super) fn ended(state: &mut AppState, kind: JobKind) {
    let until = Instant::now() + DEADLINE;
    while running(state, kind) {
        assert!(Instant::now() < until, "the {kind:?} job never ended");
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn named(state: &AppState, job: &str) -> bool {
    errors(state).iter().any(|e| e.contains(job))
}

/// Wait for a save on a worker, if one started.
pub(super) fn written(state: &mut AppState) {
    let until = Instant::now() + DEADLINE;
    while state.save_receiver.is_some() {
        assert!(Instant::now() < until, "the save never finished");
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// `doc` edited to hold `bytes`.
pub(super) fn edit(state: &mut AppState, doc: DocumentId, bytes: &[u8]) {
    let doc = state.document_mut(doc).expect("the document is open");
    match &mut doc.payload {
        DocPayload::Hex(hex) => hex.bytes = bytes.to_vec(),
        payload => *payload = DocPayload::Text(TextDocument::decode(bytes)),
    }
    doc.mark_edited();
}

/// `a.rpf`: a drawable to convert, two texts to compare and a note to edit.
pub(super) fn shipped() -> Vec<u8> {
    let ydr = empty_file_bytes_for_kind(DocumentKind::Ydr).expect("a blank drawable");
    archive(&[
        ("prop.ydr", &ydr),
        ("a.txt", b"left"),
        ("b.txt", b"right"),
        ("notes.txt", b"shipped"),
    ])
}

/// `a.rpf` in a folder opened in the app, with its drawable being converted: the state, the folder's tree and the archive's path.
pub(super) fn converting(root: &Path) -> (AppState, DocumentId, PathBuf) {
    let (mut state, tree) = folder_of(root, &[("a.rpf", shipped())]);
    let prop = node_at(&mut state, tree, "a.rpf/prop.ydr");
    state.convert_rows(tree, vec![prop]);
    assert!(
        running(&state, JobKind::Conversion),
        "the conversion did not start: {:?}",
        errors(&state)
    );
    (state, tree, root.join("mods").join("a.rpf"))
}

/// `a.rpf`'s note edited and its tree saved, while a job reads `a.rpf`.
pub(super) fn save_the_note(state: &mut AppState, tree: DocumentId) {
    let notes = node_at(state, tree, "a.rpf/notes.txt");
    state
        .update_rpf_entry_bytes(tree, notes, EDITED.to_vec())
        .expect("the edit");
    save(state, tree);
}

/// Whether the archive at `path` still holds what it was written with.
pub(super) fn kept(path: &Path) -> bool {
    std::fs::read(path).ok() == Some(shipped())
}

#[test]
fn an_archive_saved_while_a_conversion_reads_it_is_refused_naming_the_conversion() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree, path) = converting(dir.path());

    save_the_note(&mut state, tree);
    assert!(
        kept(&path),
        "a.rpf was written while the conversion read it: {:?}",
        errors(&state)
    );
    assert!(
        named(&state, CONVERSION),
        "the refusal does not name the conversion: {:?}",
        errors(&state)
    );

    ended(&mut state, JobKind::Conversion);
    save(&mut state, tree);
    assert_eq!(
        body(&on_disk(&state, &path), "notes.txt", BodyForm::Decoded),
        EDITED,
        "the save once the conversion ended: {:?}",
        errors(&state)
    );
}

/// ⚠ Each on an archive of its own: a defragment that went ahead would
/// refuse the delete itself, and a delete that went ahead would leave nothing
/// to defragment.
#[test]
fn a_delete_or_a_defragment_of_an_archive_a_conversion_reads_is_refused_naming_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let mut failed = Vec::new();

    let (mut state, tree, _) = converting(&dir.path().join("defragmented"));
    let row = node_at(&mut state, tree, "a.rpf");
    state.defragment_archive(tree, row);
    if state.defrag.is_running() || state.defrag.offer.is_some() {
        failed.push("a.rpf was defragmented".to_string());
    } else if !named(&state, CONVERSION) {
        failed.push(format!("the defragment's refusal: {:?}", errors(&state)));
    }

    let (mut state, tree, path) = converting(&dir.path().join("deleted"));
    let row = node_at(&mut state, tree, "a.rpf");
    state.rpf_delete_node(tree, row).expect("the delete asks");
    state.answer_delete(true);
    if !path.is_file() {
        failed.push("a.rpf went to the bin".to_string());
    } else if !named(&state, CONVERSION) {
        failed.push(format!("the delete's refusal: {:?}", errors(&state)));
    }

    assert!(
        failed.is_empty(),
        "while the conversion read a.rpf: {failed:?}"
    );
}

/// ⚠ Every way a save reaches a file inside the folder: the folder's tree
/// saving an archive in it, a file opened from it saved in place, another
/// saved as one of its files, an archive entry saved as a loose file over one
/// and back into its archive, and an archive opened from it on its own taking
/// an entry back. Each runs before any is judged, so one run names each that
/// is not refused.
#[test]
fn a_file_inside_a_folder_saved_while_a_pack_is_built_from_it_is_refused_naming_the_build() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let source = scratch.path().join("mypack_src");
    let props = source.join("x64").join("levels").join("gta5").join("props");
    std::fs::create_dir_all(&props).expect("the folders");
    let files = [
        ("models.rpf", archive(&[("a.txt", b"in models")])),
        ("more.rpf", archive(&[("b.txt", b"in more")])),
        ("props.ityp", b"archetypes".to_vec()),
    ];
    for (name, bytes) in &files {
        std::fs::write(props.join(name), bytes).expect("a file of the pack");
    }
    let other = scratch.path().join("other.ityp");
    std::fs::write(&other, b"elsewhere").expect("a file outside the folder");

    let mut state = AppState::new();
    let tree = opened(&mut state, &source);
    let in_models = node_at(&mut state, tree, "x64/levels/gta5/props/models.rpf/a.txt");
    state.open_rpf_entry(tree, in_models);
    let entry = finish_load(&mut state).expect("a.txt opens");
    state.load_file(props.join("props.ityp"));
    let ityp = finish_load(&mut state).expect("props.ityp opens");
    state.load_file(other.clone());
    let elsewhere = finish_load(&mut state).expect("other.ityp opens");
    state.load_file(props.join("more.rpf"));
    let more = finish_load(&mut state).expect("more.rpf opens on its own");
    let in_more = node_at(&mut state, more, "b.txt");
    state.open_rpf_entry(more, in_more);
    let more_entry = finish_load(&mut state).expect("b.txt opens");

    state.ask_dlc_pack(source.clone());
    if let Some(asked) = state.dlc_pack.asked.as_mut() {
        asked.spec.name = "mypack".into();
        asked.recheck();
    }
    state.create_dlc_pack(scratch.path().join("built"));
    assert!(
        running(&state, JobKind::DlcPack),
        "the build did not start: {:?}",
        errors(&state)
    );

    // ⚠ Each route is judged against the files as they stood before it, so
    // one that writes does not mark the routes after it.
    let held = || -> Vec<Option<Vec<u8>>> {
        (files.iter())
            .map(|(name, _)| std::fs::read(props.join(name)).ok())
            .collect()
    };
    let mut failed = Vec::new();
    let mut route = |state: &mut AppState, what: &str, act: &dyn Fn(&mut AppState)| {
        let before = held();
        act(state);
        written(state);
        if held() != before {
            failed.push(format!("{what} wrote over the folder's file"));
        } else if !named(state, "DLC pack mypack") {
            failed.push(format!(
                "{what} was refused naming no build: {:?}",
                errors(state)
            ));
        }
        state.toasts.clear();
    };

    route(
        &mut state,
        "the folder's tree saving models.rpf",
        &|state| {
            state
                .update_rpf_entry_bytes(tree, in_models, EDITED.to_vec())
                .expect("the edit");
            save(state, tree);
        },
    );
    route(&mut state, "props.ityp saved in place", &|state| {
        edit(state, ityp, EDITED);
        state.set_active_document(ityp);
        state.save_active_document();
    });
    route(&mut state, "other.ityp saved as props.ityp", &|state| {
        state.set_active_document(elsewhere);
        state.save_active_document_as(props.join("props.ityp"));
    });
    route(
        &mut state,
        "a.txt saved as a loose file over props.ityp",
        &|state| {
            edit(state, entry, EDITED);
            state.set_active_document(entry);
            state.save_active_document();
            let loose = SaveDestination::LooseFile(props.join("props.ityp"));
            state.choose_save_destination(loose);
        },
    );
    route(&mut state, "a.txt saved back into models.rpf", &|state| {
        // Not what the tree's save holds for it, so a write shows even after that save wrote.
        edit(state, entry, b"saved back");
        state.set_active_document(entry);
        state.save_active_document();
        state.choose_save_destination(SaveDestination::Archive);
    });
    route(
        &mut state,
        "more.rpf, open on its own, taking b.txt back",
        &|state| {
            edit(state, more_entry, EDITED);
            state.set_active_document(more_entry);
            state.save_active_document();
            state.choose_save_destination(SaveDestination::Archive);
        },
    );

    let moved = state.document(elsewhere).and_then(|d| d.path.clone());
    if moved.as_ref() != Some(&other) {
        failed.push(format!("the refused Save As left other.ityp at {moved:?}"));
    }
    assert!(failed.is_empty(), "while the pack was built: {failed:?}");

    ended(&mut state, JobKind::DlcPack);
    state.set_active_document(ityp);
    state.save_active_document();
    written(&mut state);
    assert_eq!(
        std::fs::read(props.join("props.ityp")).expect("props.ityp"),
        EDITED,
        "the save once the pack was built: {:?}",
        errors(&state)
    );
}

#[test]
fn an_archive_saved_while_a_comparison_reads_it_is_refused_naming_the_comparison() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = folder_of(dir.path(), &[("a.rpf", shipped())]);
    let left = node_at(&mut state, tree, "a.rpf/a.txt");
    let right = node_at(&mut state, tree, "a.rpf/b.txt");
    state.select_for_compare(tree, left);
    state.compare_with_selected(tree, right);
    assert!(
        running(&state, JobKind::Compare),
        "the comparison did not start: {:?}",
        errors(&state)
    );

    save_the_note(&mut state, tree);
    assert!(
        kept(&dir.path().join("mods").join("a.rpf")),
        "a.rpf was written while the comparison read it: {:?}",
        errors(&state)
    );
    assert!(
        named(&state, "comparison of a.txt"),
        "the refusal does not name the comparison: {:?}",
        errors(&state)
    );
}
