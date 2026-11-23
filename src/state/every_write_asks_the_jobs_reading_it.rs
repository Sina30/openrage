//! Every write asks the jobs reading what it would replace: a conversion's and a glTF
//! export's write are refused naming the job, an overlay save goes ahead while a job reads
//! the shipped archive it writes beside, and a save inside the install the Uses read mounts
//! is never held back, the index being read again.
//!
//! ⚠ A job reads until the per-frame tick takes what it ended with, so nothing here ticks
//! the jobs before it writes, and a fast job still holds its files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_interchange::gltf::{self, Container};
use or_interchange::{Image, Scene};
use or_io::container::rpf::BodyForm;
use or_io::Generation;

use super::compare::Ended;
use super::convert::{Converted, ConvertedFile};
use super::converting_from_the_app::written_by_the_job;
use super::defragmenting_an_archive::{fragmented, settle};
use super::dependencies::Answer;
use super::document::{DocumentId, DocumentKind};
use super::gltf::{Asked, Export, ExportChoice};
use super::jobs::{JobKind, Outcome};
use super::nested_archives_survive_a_save::{body, errors, on_disk, EDITED};
use super::no_save_replaces_a_file_a_job_reads::{
    edit, ended, kept, named, running, save_the_note, shipped, written,
};
use super::opening_a_ydd::{finish_load, node_at};
use super::save_destination::SaveDestination;
use super::uses_and_used_by_in_the_app::cold;
use super::uses_reaches_every_pack::{install_with_a_map, MAPS};
use super::{AppState, EditMode, Game, Prefs};
use crate::platform::recycle_bin;

pub(super) const BUILD: &str = "DLC pack mypack";

/// A DLC pack `mypack` being built in `state` from a folder of `scratch`: the folder inside it that the build reads.
pub(super) fn pack_built_from(state: &mut AppState, scratch: &Path) -> PathBuf {
    let source = scratch.join("mypack_src");
    let props = source.join("x64").join("levels").join("gta5").join("props");
    std::fs::create_dir_all(&props).expect("the folders");
    std::fs::write(props.join("props.ityp"), b"archetypes").expect("a file of the pack");
    state.ask_dlc_pack(source);
    if let Some(asked) = state.dlc_pack.asked.as_mut() {
        asked.spec.name = "mypack".into();
        asked.recheck();
    }
    state.create_dlc_pack(scratch.join("built"));
    assert!(
        running(state, JobKind::DlcPack),
        "the build did not start: {:?}",
        errors(state)
    );
    props
}

/// A DLC pack `mypack` being built from a folder: the state, and the folder inside it that the build reads.
pub(super) fn building(scratch: &Path) -> (AppState, PathBuf) {
    let mut state = AppState::new();
    let props = pack_built_from(&mut state, scratch);
    (state, props)
}

/// What a conversion made of files called `names`, as its worker hands it over.
pub(super) fn made(names: &[&str]) -> Converted {
    let file = |name: &&str| ConvertedFile {
        name: name.to_string(),
        outcome: Ok(or_cli::Retargeted {
            from: Generation::Gen8,
            to: Generation::Gen9,
            bytes: b"converted".to_vec(),
            report: Default::default(),
        }),
    };
    Converted {
        files: names.iter().map(file).collect(),
        without_database: true,
        of_document: false,
    }
}

/// A glTF export of `scene`, worked out and waiting on where it goes.
pub(super) fn exported(scene: Scene) -> Asked {
    Asked::Export(Export {
        name: "prop.ydr".into(),
        bytes: Arc::new(Vec::new()),
        choice: ExportChoice {
            entry: None,
            every_lod: false,
            textures: true,
            glb: true,
        },
        drawables: Vec::new(),
        made: Some(Ok(scene)),
        gta6: None,
    })
}

/// Whether `files` are all there, as `files` lists them.
pub(super) fn all_there(files: &[PathBuf]) -> bool {
    files.iter().all(|file| file.is_file())
}

/// ⚠ Each way is tried on the files as they stood, so one that wrote would mark
/// the ways after it: `wrote` is judged against `files`, which every way leaves.
#[test]
fn a_conversion_written_into_a_folder_a_pack_is_built_from_is_refused_naming_the_build() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (mut state, props) = building(scratch.path());
    let elsewhere = scratch.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a folder outside the pack");

    let mut failed = Vec::new();
    let ways: [(&str, &[&str], PathBuf); 2] = [
        ("one file", &["prop.ydr"], props.join("prop.ydr")),
        ("several files", &["a.ydr", "b.ydr"], props.clone()),
    ];
    for (what, names, dest) in &ways {
        let names: &[&str] = names;
        let files: Vec<PathBuf> = match names {
            [_] => vec![dest.clone()],
            _ => names.iter().map(|name| dest.join(name)).collect(),
        };
        state.convert.done = Some(made(names));
        state.write_conversion(dest.clone(), None);
        written_by_the_job(&mut state);
        if files.iter().any(|file| file.exists()) {
            failed.push(format!("{what}: written into the pack's folder"));
        } else if !named(&state, BUILD) {
            failed.push(format!(
                "{what}: the refusal names no build: {:?}",
                errors(&state)
            ));
        } else if state.convert.done.is_none() {
            failed.push(format!("{what}: the refusal dropped the question"));
        }
        state.toasts.clear();
    }
    assert!(failed.is_empty(), "while the pack was built: {failed:?}");

    // Nothing the build reads is replaced here, so it goes ahead.
    let beside = elsewhere.join("prop.ydr");
    state.convert.done = Some(made(&["prop.ydr"]));
    state.write_conversion(beside.clone(), None);
    written_by_the_job(&mut state);
    assert!(
        all_there(&[beside]),
        "a conversion written outside the pack's folder: {:?}",
        errors(&state)
    );

    ended(&mut state, JobKind::DlcPack);
    state.convert.done = Some(made(&["prop.ydr"]));
    let dest = props.join("prop.ydr");
    state.write_conversion(dest.clone(), None);
    written_by_the_job(&mut state);
    assert!(
        all_there(&[dest]),
        "the conversion once the pack was built: {:?}",
        errors(&state)
    );
}

#[test]
fn a_gltf_export_written_into_a_folder_a_pack_is_built_from_is_refused_naming_the_build() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (mut state, props) = building(scratch.path());
    let elsewhere = scratch.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a folder outside the pack");

    state.gltf.asked = Some(exported(Scene::default()));
    let inside = props.join("prop.glb");
    state.write_gltf(inside.clone());
    written_by_the_job(&mut state);
    assert!(
        !inside.exists(),
        "the export was written into the pack's folder"
    );
    assert!(
        named(&state, BUILD),
        "the refusal names no build: {:?}",
        errors(&state)
    );
    assert!(
        state.gltf.asked.is_some(),
        "the refusal dropped the question"
    );

    let beside = elsewhere.join("prop.glb");
    state.write_gltf(beside.clone());
    written_by_the_job(&mut state);
    assert!(
        all_there(&[beside]),
        "an export written outside the pack's folder: {:?}",
        errors(&state)
    );

    ended(&mut state, JobKind::DlcPack);
    state.gltf.asked = Some(exported(Scene::default()));
    state.write_gltf(inside.clone());
    written_by_the_job(&mut state);
    assert!(
        all_there(&[inside]),
        "the export once the pack was built: {:?}",
        errors(&state)
    );
}

/// ⚠ A `.gltf` writes its texture beside itself, under a name the scene
/// decides, so a job can read the texture and not the export.
#[test]
fn a_gltf_export_whose_texture_a_job_reads_beside_it_is_refused_naming_the_job() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let folder = scratch.path().join("out");
    std::fs::create_dir_all(&folder).expect("a folder for the export");
    let scene = Scene {
        images: vec![Image {
            name: "tex".into(),
            png: b"texels".to_vec(),
        }],
        ..Scene::default()
    };
    let beside = gltf::write(&scene, "prop", Container::Separate).sidecars;
    let [(texture, _)] = beside.as_slice() else {
        panic!("a .gltf of one texture writes one file beside it: {beside:?}");
    };
    let (export, texture) = (folder.join("prop.gltf"), folder.join(texture));

    let mut state = AppState::new();
    let reads = vec![texture.clone()];
    state
        .compare
        .start("prop".into(), reads, |_| Ended::Failed(String::new()));
    state.gltf.asked = Some(exported(scene));
    state.write_gltf(export.clone());
    written_by_the_job(&mut state);
    assert!(
        !export.exists() && !texture.exists(),
        "the export was written over a texture a job reads"
    );
    assert!(
        named(&state, "the comparison of prop"),
        "the refusal names no comparison: {:?}",
        errors(&state)
    );
    assert!(
        state.gltf.asked.is_some(),
        "the refusal dropped the question"
    );

    state.toasts.clear();
    ended(&mut state, JobKind::Compare);
    state.write_gltf(export.clone());
    written_by_the_job(&mut state);
    assert!(
        all_there(&[export, texture]),
        "the export once the comparison ended: {:?}",
        errors(&state)
    );
}

/// An install at `root` holding `a.rpf` and `other.rpf`, open with every edit permitted: the state, its tree and `a.rpf`'s path.
pub(super) fn installed(root: &Path) -> (AppState, DocumentId, PathBuf) {
    let path = root.join("a.rpf");
    std::fs::write(&path, shipped()).expect("a.rpf");
    std::fs::write(root.join("other.rpf"), shipped()).expect("other.rpf");
    let mut prefs = Prefs::default();
    (prefs.game_paths).set(Game::GtaVLegacy, Some(root.to_path_buf()));
    prefs.active_game = Some(Game::GtaVLegacy);
    prefs.edit_mode = EditMode::Everything;
    let mut state = AppState::from_prefs(prefs);
    state.retail_edit_session_allow = true;
    let tree = (state.documents.iter())
        .find(|d| d.kind == DocumentKind::GameRoot)
        .expect("the install's tree")
        .id;
    (state, tree, path)
}

#[test]
fn an_overlay_save_of_an_archive_goes_ahead_while_a_job_reads_the_shipped_one() {
    let dir = tempfile::tempdir().expect("an install folder");
    let (mut state, tree, path) = installed(dir.path());
    let prop = node_at(&mut state, tree, "a.rpf/prop.ydr");
    state.convert_rows(tree, vec![prop]);
    assert!(
        running(&state, JobKind::Conversion),
        "the conversion did not start: {:?}",
        errors(&state)
    );

    note_ready_to_save(&mut state, tree);
    state.choose_save_destination(SaveDestination::Overlay);

    let overlay = dir.path().join("mods").join("a.rpf");
    assert!(
        overlay.is_file(),
        "the overlay copy was not written: {:?}",
        errors(&state)
    );
    assert_eq!(
        body(&on_disk(&state, &overlay), "notes.txt", BodyForm::Decoded),
        EDITED,
        "the overlay copy holds the edit"
    );
    assert!(kept(&path), "the shipped archive was written");
}

/// What Uses says of `other.rpf`'s drawable, which no test here writes.
pub(super) fn uses_of_the_other(state: &mut AppState, tree: DocumentId) -> Answer {
    let prop = node_at(state, tree, "other.rpf/prop.ydr");
    state.uses_of(tree, prop)
}

/// The Uses read of `state`'s install, begun or going on and, if `finish`, ended and answered.
pub(super) fn begin_uses(state: &mut AppState, tree: DocumentId, finish: bool) {
    assert!(
        matches!(uses_of_the_other(state, tree), Answer::Looking),
        "Uses did not start reading the install"
    );
    if finish {
        ended(state, JobKind::Dependencies);
        assert!(
            !matches!(uses_of_the_other(state, tree), Answer::Looking),
            "the read ended without an answer"
        );
    }
}

/// The Uses read of the install at `root`, begun and, if `finish`, ended and answered.
pub(super) fn read_for_uses(root: &Path, finish: bool) -> (AppState, DocumentId, PathBuf) {
    let (mut state, tree, path) = installed(root);
    begin_uses(&mut state, tree, finish);
    (state, tree, path)
}

pub(super) fn read_again(state: &mut AppState, tree: DocumentId) -> bool {
    matches!(uses_of_the_other(state, tree), Answer::Looking)
        && running(state, JobKind::Dependencies)
}

#[test]
fn an_archive_saved_after_the_uses_read_has_passed_it_is_saved_and_the_index_is_read_again() {
    let dir = tempfile::tempdir().expect("an install folder");
    let (mut state, tree, path) = read_for_uses(dir.path(), true);

    save_the_note(&mut state, tree);
    assert!(!kept(&path), "a.rpf was not saved: {:?}", errors(&state));
    assert!(
        read_again(&mut state, tree),
        "Uses answered from the index read before the save"
    );
}

#[test]
fn an_archive_saved_while_uses_reads_its_install_is_saved_and_that_read_is_begun_again() {
    let dir = tempfile::tempdir().expect("an install folder");
    let (mut state, tree, path) = read_for_uses(dir.path(), false);

    save_the_note(&mut state, tree);
    assert!(
        !kept(&path),
        "a.rpf was not saved while Uses read the install: {:?}",
        errors(&state)
    );
    assert!(
        !running(&state, JobKind::Dependencies),
        "the read that began before the save goes on"
    );
    assert!(
        (state.jobs.ended().iter())
            .any(|job| job.kind == JobKind::Dependencies
                && matches!(job.outcome, Outcome::Stopped(_))),
        "the job list does not say the read stopped"
    );
    assert!(
        read_again(&mut state, tree),
        "Uses did not read the install again"
    );
}

/// ⚠ Each on an install of its own: a rename takes `a.rpf` from the move and the delete.
#[test]
fn an_archive_renamed_moved_or_deleted_after_the_uses_read_has_passed_it_is_read_again() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let mut failed = Vec::new();
    for what in ["renamed", "moved", "deleted"] {
        let root = dir.path().join(what);
        std::fs::create_dir_all(root.join("sub")).expect("a folder in the install");
        let (mut state, tree, _) = read_for_uses(&root, true);
        let row = node_at(&mut state, tree, "a.rpf");
        let done = match what {
            "renamed" => state.rpf_rename_node(tree, row, "c.rpf"),
            "moved" => {
                let sub = node_at(&mut state, tree, "sub");
                state.rpf_move_node(tree, row, sub)
            }
            _ => state
                .rpf_delete_node(tree, row)
                .map(|()| state.answer_delete(true)),
        };
        if let Err(why) = done {
            failed.push(format!("{what}: it was refused: {why}"));
        } else if !read_again(&mut state, tree) {
            failed.push(format!("{what}: Uses answered from the index read before"));
        }
    }
    assert!(failed.is_empty(), "after the read passed a.rpf: {failed:?}");
}

/// ⚠ The paste sends `readme.txt` to the Recycle Bin, so its undo writes the file back
/// under the install and its redo takes it away again, each after the read has ended.
#[test]
fn an_undo_and_a_redo_of_a_cut_in_the_install_after_the_uses_read_has_passed_it_read_it_again() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let (game, bin) = (dir.path().join("game"), dir.path().join("bin"));
    std::fs::create_dir_all(&game).expect("the install");
    std::fs::create_dir_all(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let readme = game.join("readme.txt");
    std::fs::write(&readme, b"a loose file").expect("a loose file");
    let (mut state, tree, _) = installed(&game);
    let (loose, a) = (
        node_at(&mut state, tree, "readme.txt"),
        node_at(&mut state, tree, "a.rpf"),
    );
    state.set_active_document(tree);
    state.copy_rows(tree, &[loose], true).expect("the cut");
    state.paste_rows(tree, a).expect("the paste");
    assert!(!readme.exists(), "the cut file stayed where it was");
    begin_uses(&mut state, tree, true);

    assert_eq!(
        state.undo().as_deref(),
        Some("Paste 1 item(s)"),
        "{:?}",
        errors(&state)
    );
    assert!(readme.is_file(), "the undo did not write the file back");
    assert!(
        read_again(&mut state, tree),
        "Uses answered from the index read before the undo"
    );

    begin_uses(&mut state, tree, true);
    assert_eq!(
        state.redo().as_deref(),
        Some("Paste 1 item(s)"),
        "{:?}",
        errors(&state)
    );
    assert!(!readme.exists(), "the redo did not take the file away");
    assert!(
        read_again(&mut state, tree),
        "Uses answered from the index read before the redo"
    );
}

/// `a.rpf`'s note, opened from the install's tree and edited, with its save asked for: the destination prompt is up.
fn note_ready_to_save(state: &mut AppState, tree: DocumentId) {
    let notes = node_at(state, tree, "a.rpf/notes.txt");
    state.open_rpf_entry(tree, notes);
    let entry = finish_load(state).expect("notes.txt opens");
    edit(state, entry, EDITED);
    state.set_active_document(entry);
    assert!(state.save_active_document());
}

/// ⚠ Each way on an install of its own, after the Uses read has ended: a way
/// that wrote would otherwise leave the next one a read already begun.
#[test]
fn a_file_written_by_any_save_or_a_defragment_after_the_uses_read_has_passed_it_is_read_again() {
    type Way = fn(&mut AppState, DocumentId, &Path);
    let ways: [(&str, Way); 5] = [
        ("a loose file saved in place", |state, _, root| {
            state.load_file(root.join("readme.txt"));
            let loose = finish_load(state).expect("readme.txt opens");
            edit(state, loose, EDITED);
            state.set_active_document(loose);
            state.save_active_document();
            written(state);
        }),
        ("an entry saved as a loose file", |state, tree, root| {
            note_ready_to_save(state, tree);
            state.choose_save_destination(SaveDestination::LooseFile(root.join("copy.txt")));
        }),
        ("an entry saved into the overlay", |state, tree, _| {
            note_ready_to_save(state, tree);
            state.choose_save_destination(SaveDestination::Overlay);
        }),
        (
            "an archive open on its own taking an entry back",
            |state, _, root| {
                state.load_file(root.join("other.rpf"));
                let archive = finish_load(state).expect("other.rpf opens on its own");
                let notes = node_at(state, archive, "notes.txt");
                state.open_rpf_entry(archive, notes);
                let entry = finish_load(state).expect("notes.txt opens");
                edit(state, entry, EDITED);
                state.set_active_document(entry);
                state.save_active_document();
                state.choose_save_destination(SaveDestination::Archive);
            },
        ),
        ("an archive defragmented", |state, tree, _| {
            let row = node_at(state, tree, "frag.rpf");
            state.defragment_archive(tree, row);
            settle(state);
            state.answer_defrag(true);
            settle(state);
        }),
    ];
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut failed = Vec::new();
    for (at, (what, way)) in ways.iter().enumerate() {
        let root = dir.path().join(at.to_string());
        std::fs::create_dir_all(&root).expect("an install folder");
        std::fs::write(root.join("readme.txt"), b"a loose file").expect("a loose file");
        std::fs::write(root.join("frag.rpf"), fragmented()).expect("a fragmented archive");
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

/// Uses of `plain_map.ymap` as the menu asks it.
fn uses_of_the_map(state: &mut AppState, tree: DocumentId) -> Answer {
    let map = node_at(state, tree, &format!("{MAPS}/plain_map.ymap"));
    state.uses_of(tree, map)
}

/// Uses of the map, asked until it answers: the index is read, then the map's bounds, each a read of its own.
fn uses_of_the_map_read_through(state: &mut AppState, tree: DocumentId) -> Answer {
    for read in ["the index", "the map's bounds"] {
        let answer = uses_of_the_map(state, tree);
        assert!(
            matches!(answer, Answer::Looking),
            "{read} was not read: {answer:?}"
        );
        ended(state, JobKind::Dependencies);
    }
    uses_of_the_map(state, tree)
}

#[test]
fn a_maps_bounds_read_before_a_file_was_replaced_under_the_install_are_read_again() {
    let root = install_with_a_map();
    let (mut state, tree) = cold(root.path());
    let first = uses_of_the_map_read_through(&mut state, tree);
    assert!(matches!(first, Answer::Listed { .. }), "{first:?}");

    state.file_replaced(&root.path().join("common.rpf"));
    let again = uses_of_the_map_read_through(&mut state, tree);
    assert!(matches!(again, Answer::Listed { .. }), "{again:?}");
}
