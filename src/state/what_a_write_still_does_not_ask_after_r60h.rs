//! What a write still did not ask after R60h: an extraction's, an Export as…'s, an import's, a
//! New file's and a paste's write into a folder a job reads is refused naming the job, a
//! destination the disk spells otherwise than the job's path is the same place, a file written
//! under the install makes Uses read it again, and a read begun right after a save waits for the
//! one the save dropped.
//!
//! ⚠ A job reads until the per-frame tick takes what it ended with, so nothing here ticks the
//! jobs before it writes, and a fast job still holds its files.

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use or_io::container::rpf::BodyForm;

use super::a_moved_or_deleted_archive_row_leaves_nothing_behind::folder_of;
use super::compare::Ended;
use super::converting_from_the_app::written_by_the_job;
use super::dependencies::Answer;
use super::document::{DocumentId, DocumentKind, RpfNodeId};
use super::every_add_lands_where_the_tree_shows_it::opened;
use super::every_write_asks_the_jobs_reading_it::{
    all_there, begin_uses, building, installed, made, pack_built_from, read_again, read_for_uses,
    uses_of_the_other, BUILD,
};
use super::extract_job::TypeFilter;
use super::jobs::JobKind;
use super::nested_archives_survive_a_save::errors;
use super::no_save_replaces_a_file_a_job_reads::{ended, named, save_the_note, shipped};
use super::opening_a_ydd::node_at;
use super::AppState;
use crate::platform::recycle_bin;

const DEADLINE: Duration = Duration::from_secs(60);

/// A folder holding `a.rpf` opened in the app and a pack being built from a folder beside it: the state, its tree, `a.rpf`'s note and the folder inside the pack that the build reads.
fn beside_a_pack_build(scratch: &Path) -> (AppState, DocumentId, RpfNodeId, PathBuf) {
    let (mut state, tree) = folder_of(scratch, &[("a.rpf", shipped())]);
    let props = pack_built_from(&mut state, scratch);
    let notes = node_at(&mut state, tree, "a.rpf/notes.txt");
    (state, tree, notes, props)
}

#[test]
fn an_extraction_into_a_folder_a_pack_is_built_from_is_refused_naming_the_build() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (mut state, tree, notes, props) = beside_a_pack_build(scratch.path());
    let elsewhere = scratch.path().join("elsewhere");
    let extract = |state: &mut AppState, into: &Path| {
        let into = into.to_path_buf();
        state.extract_rpf_node(tree, notes, into, BodyForm::Decoded, TypeFilter::All);
        written_by_the_job(state);
    };

    extract(&mut state, &props);
    assert!(
        !props.join("notes.txt").exists(),
        "the extraction wrote into the pack's folder"
    );
    assert!(
        named(&state, BUILD),
        "the refusal names no build: {:?}",
        errors(&state)
    );

    extract(&mut state, &elsewhere);
    assert!(
        all_there(&[elsewhere.join("notes.txt")]),
        "an extraction outside the pack's folder: {:?}",
        errors(&state)
    );

    ended(&mut state, JobKind::DlcPack);
    extract(&mut state, &props);
    assert!(
        all_there(&[props.join("notes.txt")]),
        "the extraction once the pack was built: {:?}",
        errors(&state)
    );
}

/// ⚠ A job reading one file of a folder holds back the files an extraction writes beside it.
#[test]
fn an_extraction_beside_a_file_a_job_reads_goes_ahead_and_over_it_is_refused() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (mut state, tree) = folder_of(scratch.path(), &[("a.rpf", shipped())]);
    let notes = node_at(&mut state, tree, "a.rpf/notes.txt");
    let (beside, over) = (scratch.path().join("beside"), scratch.path().join("over"));
    for folder in [&beside, &over] {
        std::fs::create_dir(folder).expect("a folder to extract into");
    }
    let reads = vec![beside.join("other.txt")];
    (state.compare).start("a pair".into(), reads, |_| Ended::Failed(String::new()));
    state.extract_rpf_node(
        tree,
        notes,
        beside.clone(),
        BodyForm::Decoded,
        TypeFilter::All,
    );
    written_by_the_job(&mut state);
    assert!(
        all_there(&[beside.join("notes.txt")]),
        "an extraction beside the file the comparison reads: {:?}",
        errors(&state)
    );

    ended(&mut state, JobKind::Compare);
    let reads = vec![over.join("notes.txt")];
    (state.compare).start("a pair".into(), reads, |_| Ended::Failed(String::new()));
    state.extract_rpf_node(
        tree,
        notes,
        over.clone(),
        BodyForm::Decoded,
        TypeFilter::All,
    );
    written_by_the_job(&mut state);
    assert!(
        !over.join("notes.txt").exists() && named(&state, "the comparison of a pair"),
        "an extraction over the file the comparison reads: {:?}",
        errors(&state)
    );
}

#[test]
fn an_export_as_into_a_folder_a_pack_is_built_from_is_refused_naming_the_build() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (mut state, tree, notes, props) = beside_a_pack_build(scratch.path());
    let export = |state: &mut AppState, to: PathBuf| {
        state.export_rpf_node_as(tree, notes, to, Some(false));
        written_by_the_job(state);
    };

    let inside = props.join("notes.txt");
    export(&mut state, inside.clone());
    assert!(
        !inside.exists(),
        "the export was written into the pack's folder"
    );
    assert!(
        named(&state, BUILD),
        "the refusal names no build: {:?}",
        errors(&state)
    );

    let beside = scratch.path().join("notes.txt");
    export(&mut state, beside.clone());
    assert!(
        all_there(&[beside]),
        "an export outside the pack's folder: {:?}",
        errors(&state)
    );

    ended(&mut state, JobKind::DlcPack);
    export(&mut state, inside.clone());
    assert!(
        all_there(&[inside]),
        "the export once the pack was built: {:?}",
        errors(&state)
    );
}

/// ⚠ A folder the disk takes for the one a job reads is that folder: `starts_with` compares
/// spellings, and a `..` or another case is another spelling. The build reads `mypack_src`, so
/// each spelling differs inside that name or before it, not below it.
#[test]
fn a_write_into_a_folder_a_job_reads_is_refused_however_the_disk_is_asked_for_it() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (mut state, props) = building(scratch.path());
    let aside = scratch.path().join("aside");
    std::fs::create_dir(&aside).expect("a folder beside the pack's");
    let below = props
        .strip_prefix(scratch.path().join("mypack_src"))
        .expect("the folder is inside the pack's");
    let shouted = scratch.path().join("MYPACK_SRC");
    // Whether the disk keeps the two cases apart is the disk's to say: a folder it does not
    // find is another folder, and a write into it is not the build's.
    let shouted_is_the_pack = shouted.is_dir();
    let spellings = [
        (
            "through a folder beside it",
            aside.join("..").join("mypack_src").join(below),
            true,
        ),
        ("in another case", shouted.join(below), shouted_is_the_pack),
    ];

    let mut failed = Vec::new();
    for (what, folder, refused) in spellings {
        state.convert.done = Some(made(&["prop.ydr"]));
        state.write_conversion(folder.join("prop.ydr"), None);
        written_by_the_job(&mut state);
        if named(&state, BUILD) != refused {
            failed.push(format!(
                "{what}: refused is {}, not {refused}: {:?}",
                !refused,
                errors(&state)
            ));
        }
        state.toasts.clear();
    }
    assert!(failed.is_empty(), "while the pack was built: {failed:?}");
}

#[test]
fn an_import_a_new_file_and_a_paste_into_a_folder_a_pack_is_built_from_are_refused_naming_the_build(
) {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::new();
    let props = pack_built_from(&mut state, scratch.path());
    let source = scratch.path().join("mypack_src");
    std::fs::write(source.join("readme.txt"), b"a loose file").expect("a file to copy");
    let outside = scratch.path().join("outside.txt");
    std::fs::write(&outside, b"a file to import").expect("a file to import");
    let tree = opened(&mut state, &source);
    let folder = node_at(&mut state, tree, "x64/levels/gta5/props");
    let loose = node_at(&mut state, tree, "readme.txt");

    let mut failed = Vec::new();
    let mut check = |what: &str, said: String, state: &mut AppState| {
        if !said.contains(BUILD) {
            failed.push(format!("{what}: the refusal names no build: {said}"));
        }
        state.toasts.clear();
    };
    let imported = state.rpf_import_file_to_folder(tree, folder, outside.clone());
    check("an import", format!("{imported:?}"), &mut state);
    let added = state.rpf_add_empty_file(tree, folder, DocumentKind::Ydr);
    check("a New file", format!("{added:?}"), &mut state);
    state.copy_rows(tree, &[loose], false).expect("the copy");
    state.paste_rows(tree, folder).expect("the paste");
    let said = errors(&state).join("; ");
    check("a paste", said, &mut state);
    assert!(failed.is_empty(), "while the pack was built: {failed:?}");

    let held: Vec<_> = std::fs::read_dir(&props)
        .expect("the pack's folder")
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(
        held,
        ["props.ityp"],
        "something was written into the pack's folder"
    );

    ended(&mut state, JobKind::DlcPack);
    state
        .rpf_import_file_to_folder(tree, folder, outside)
        .expect("the import once the pack was built");
    assert!(
        all_there(&[props.join("outside.txt")]),
        "the import once the pack was built: {:?}",
        errors(&state)
    );
}

/// ⚠ A move writes where the row lands, though the row itself is a file no job reads: the rename
/// asks for the source only, so a file outside the pack's folder went in.
#[test]
fn a_drag_and_a_cut_paste_into_a_folder_a_pack_is_built_from_are_refused_naming_the_build() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::new();
    let props = pack_built_from(&mut state, scratch.path());
    let aside = scratch.path().join("aside");
    std::fs::create_dir(&aside).expect("a folder beside the pack's source");
    for name in ["dragged.txt", "cut.txt"] {
        std::fs::write(aside.join(name), name.as_bytes()).expect("a file to move");
    }
    let tree = opened(&mut state, scratch.path());
    let folder = node_at(&mut state, tree, "mypack_src/x64/levels/gta5/props");
    let dragged = node_at(&mut state, tree, "aside/dragged.txt");
    let cut = node_at(&mut state, tree, "aside/cut.txt");

    let dropped = state.rpf_move_node(tree, dragged, folder);
    assert!(
        dropped.as_ref().is_err_and(|said| said.contains(BUILD)),
        "the drop did not name the build: {dropped:?}"
    );
    state.set_active_document(tree);
    state.copy_rows(tree, &[cut], true).expect("the cut");
    state.paste_rows(tree, folder).expect("the paste");
    assert!(
        named(&state, BUILD),
        "the paste did not name the build: {:?}",
        errors(&state)
    );
    assert!(
        all_there(&[aside.join("dragged.txt"), aside.join("cut.txt")]),
        "a file left its folder for the pack's"
    );
    assert!(
        !props.join("dragged.txt").exists() && !props.join("cut.txt").exists(),
        "a file was moved into the pack's folder"
    );

    ended(&mut state, JobKind::DlcPack);
    state
        .rpf_move_node(tree, dragged, folder)
        .expect("the drop once the pack was built");
    assert!(
        all_there(&[props.join("dragged.txt")]) && !aside.join("dragged.txt").exists(),
        "the drop once the pack was built: {:?}",
        errors(&state)
    );
}

/// ⚠ Each way on an install of its own, after the Uses read has ended: a way that wrote
/// would otherwise leave the next one a read already begun.
#[test]
fn a_file_written_under_the_install_after_the_uses_read_has_passed_it_is_read_again() {
    type Way = fn(&mut AppState, DocumentId, &Path);
    let ways: [(&str, Way); 7] = [
        ("a file imported into a folder", |state, tree, root| {
            let source = root.parent().expect("the scratch").join("source.txt");
            std::fs::write(&source, b"imported").expect("a file to import");
            let sub = node_at(state, tree, "sub");
            state
                .rpf_import_file_to_folder(tree, sub, source)
                .expect("the import");
        }),
        ("a New file", |state, tree, _| {
            let sub = node_at(state, tree, "sub");
            state
                .rpf_add_empty_file(tree, sub, DocumentKind::Ydr)
                .expect("the new file");
        }),
        ("a file pasted", |state, tree, _| {
            let loose = node_at(state, tree, "readme.txt");
            let sub = node_at(state, tree, "sub");
            state.copy_rows(tree, &[loose], false).expect("the copy");
            state.paste_rows(tree, sub).expect("the paste");
        }),
        ("a cut file sent to the Recycle Bin", |state, tree, root| {
            let loose = node_at(state, tree, "readme.txt");
            let archive = node_at(state, tree, "a.rpf");
            state.set_active_document(tree);
            state.copy_rows(tree, &[loose], true).expect("the cut");
            state.paste_rows(tree, archive).expect("the paste");
            assert!(
                !root.join("readme.txt").exists(),
                "the cut file stayed where it was"
            );
        }),
        ("an extraction", |state, tree, root| {
            let notes = node_at(state, tree, "a.rpf/notes.txt");
            let into = root.join("extracted");
            state.extract_rpf_node(tree, notes, into, BodyForm::Decoded, TypeFilter::All);
            written_by_the_job(state);
        }),
        ("an Export as…", |state, tree, root| {
            let notes = node_at(state, tree, "a.rpf/notes.txt");
            state.export_rpf_node_as(tree, notes, root.join("notes.txt"), Some(false));
            written_by_the_job(state);
        }),
        ("a conversion's write", |state, _, root| {
            state.convert.done = Some(made(&["prop.ydr"]));
            state.write_conversion(root.join("prop.ydr"), None);
            written_by_the_job(state);
        }),
    ];
    let dir = tempfile::tempdir().expect("a scratch folder");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let mut failed = Vec::new();
    for (at, (what, way)) in ways.iter().enumerate() {
        let root = dir.path().join(at.to_string());
        std::fs::create_dir_all(root.join("sub")).expect("a folder in the install");
        std::fs::write(root.join("readme.txt"), b"a loose file").expect("a loose file");
        let (mut state, tree, _) = read_for_uses(&root, true);
        way(&mut state, tree, &root);
        if !read_again(&mut state, tree) {
            failed.push(format!(
                "{what}: Uses answered from the index read before, or nothing was written: {:?}",
                errors(&state)
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "after the read passed the install: {failed:#?}"
    );
}

/// ⚠ A mount cannot stop, so the read a save drops runs on unheard; the stand-in here holds
/// until told and ignores what it is told to stop.
#[test]
fn uses_asked_right_after_a_save_begins_its_read_once_the_dropped_one_has_ended() {
    let dir = tempfile::tempdir().expect("an install folder");
    let (mut state, tree, _) = installed(dir.path());
    // The first ask names the install; the stand-in then takes the place of the read it began.
    begin_uses(&mut state, tree, false);
    let (mounting, began) = channel::<()>();
    let (release, held) = channel::<()>();
    state.dependencies.start(move |_, _| {
        let _ = mounting.send(());
        let _ = held.recv_timeout(DEADLINE);
        Err("held".to_string())
    });
    began
        .recv_timeout(DEADLINE)
        .expect("the stand-in began its mount");

    save_the_note(&mut state, tree);
    assert!(
        matches!(uses_of_the_other(&mut state, tree), Answer::Looking),
        "Uses did not begin reading again after the save"
    );
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        state.poll_jobs();
        let read = (state.running_jobs().into_iter()).find(|job| job.kind == JobKind::Dependencies);
        assert!(
            read.as_ref().is_some_and(|read| read.detail.is_empty()),
            "a second read began while the one the save dropped still mounted: {read:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    release.send(()).expect("the stand-in is still waiting");
    ended(&mut state, JobKind::Dependencies);
    assert!(
        !matches!(uses_of_the_other(&mut state, tree), Answer::Looking),
        "the read ended without an answer"
    );
}
