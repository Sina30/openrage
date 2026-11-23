use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Form, Rpf9Sections};
use or_io::gta6::hash::name_hash32;
use or_io::gta6::headerless::{read_drawable, read_skeleton};

use super::*;
use crate::state::gta6_edit::tests::drawable_body;
use crate::state::opening_an_rpf9_archive::settle;

const V: u64 = 0x5000_0000;
/// Where the body's skeleton record, its bones, their inverse binds and their locals lie.
const SKELETON: usize = 0x400;
const RECORDS: usize = 0x480;
const INVERSES: usize = 0x520;
const LOCALS: usize = 0x5A0;

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

fn floats(v: &mut [u8], at: usize, xs: &[f32]) {
    for (i, x) in xs.iter().enumerate() {
        put(v, at + 4 * i, &x.to_le_bytes());
    }
}

fn ptr(v: &mut [u8], at: usize, to: usize) {
    put(v, at, &(V + to as u64).to_le_bytes());
}

/// A drawable of one shader whose 0x18 names a skeleton of two bones, `SKEL_ROOT` 1 m up and
/// `SKEL_Head` 0.5 m above it; their inverse binds are made-up numbers, distinct lane by lane.
fn skinned_body() -> Vec<u8> {
    let mut v = drawable_body();
    v.resize(0x700, 0);
    ptr(&mut v, 0x18, SKELETON);
    ptr(&mut v, SKELETON + 0x28, RECORDS);
    ptr(&mut v, SKELETON + 0x30, INVERSES);
    ptr(&mut v, SKELETON + 0x38, LOCALS);
    put(&mut v, SKELETON + 0x6C, &[2, 0, 2, 0]);
    let bones = [
        ("SKEL_ROOT", -1i16, [0.0f32, 0.0, 1.0], 0x620usize),
        ("SKEL_Head", 0, [0.0, 0.0, 0.5], 0x640),
    ];
    for (i, (name, parent, translation, text)) in bones.into_iter().enumerate() {
        let at = RECORDS + 0x50 * i;
        floats(&mut v, at, &[0.0, 0.0, 0.0, 1.0]);
        floats(&mut v, at + 0x10, &translation);
        put(
            &mut v,
            at + 0x1C,
            &name_hash32(name.as_bytes()).to_le_bytes(),
        );
        floats(&mut v, at + 0x20, &[1.0; 3]);
        put(&mut v, text, name.as_bytes());
        ptr(&mut v, at + 0x30, text);
        put(&mut v, at + 0x44, &(-1i16).to_le_bytes());
        put(&mut v, at + 0x46, &parent.to_le_bytes());
        put(&mut v, at + 0x48, &(i as u16).to_le_bytes());
        for c in 0..4 {
            let lane = (10 * i + c) as f32 + 0.5;
            floats(
                &mut v,
                INVERSES + 0x40 * i + 16 * c,
                &[lane, lane + 0.25, lane + 0.125],
            );
        }
    }
    v
}

fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the save never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The virtual section a read at offset 0 of the file at `path` returns.
fn read_back(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (find, read) = read_rpf9_body(Cursor::new(bytes), 0, u64::MAX).expect("it reads");
    Rpf9Sections::of(&find, &read)
        .expect("a resource")
        .virtual_section
}

fn held(path: &Path) -> or_io::gta6::headerless::Skeleton {
    crate::state::rpf9_scan::scanned_skeleton(path).expect("the skeleton it opened with")
}

#[test]
fn a_bone_edit_undoes_and_saves_over_its_loose_resource_with_the_inverse_binds_under_it() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path: PathBuf = dir.path().join("bones.rsc");
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: skinned_body(),
        physical: Vec::new(),
    };
    std::fs::write(&path, write_rpf9_resource(&sections).expect("writes")).expect("written");
    let stored = read_back(&path);
    let mut state = AppState::new();
    state.load_file(path.clone());
    settle(&mut state);
    let doc =
        (state.active_document).unwrap_or_else(|| panic!("opened: {}", state.error_dialog_message));
    let opened_as = held(&path);
    assert_eq!(opened_as.bones.len(), 2);

    // Refused, and nothing changes: an empty name, a scale of 0, a bone past the count.
    let refused = [
        (1, BoneEdit::Name(String::new())),
        (1, BoneEdit::Scale([0.0, 1.0, 1.0])),
        (1, BoneEdit::Position([f32::NAN, 0.0, 0.0])),
        (2, BoneEdit::Position([0.0; 3])),
    ];
    for (bone, edit) in refused {
        assert!(
            state.edit_gta6_bone(doc, bone, edit.clone()).is_err(),
            "{edit:?}"
        );
    }
    assert_eq!(held(&path), opened_as);
    assert!(state.document(doc).is_some_and(|d| !d.dirty));

    // The head moves a quarter metre along x: its inverse bind undoes the root's metre and its own.
    state
        .edit_gta6_bone(doc, 1, BoneEdit::Position([0.25, 0.0, 0.5]))
        .expect("a finite position");
    let edited = held(&path);
    assert_eq!(edited.bones[1].translation, [0.25, 0.0, 0.5]);
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let [x, y, z] = identity;
    assert_eq!(edited.bones[1].inverse_bind, [x, y, z, [-0.25, 0.0, -1.5]]);
    assert_eq!(
        edited.bones[0], opened_as.bones[0],
        "the root is not under the head"
    );
    assert!(state.document(doc).is_some_and(|d| d.dirty));

    state.active_document = Some(doc);
    assert!(state.undo().is_some());
    assert_eq!(held(&path), opened_as, "undone, it is as it opened");
    assert!(state.redo().is_some());
    assert_eq!(held(&path), edited, "redone, the edit is back");

    // Saved over its own file: the skeleton reads back as edited, the shaders as they were.
    assert!(state.save_active_document());
    finished(&mut state);
    let saved = read_back(&path);
    let read = read_skeleton(&saved, false)
        .expect("reads")
        .expect("a skeleton");
    assert_eq!(read, edited);
    let shaders = |v: &[u8]| read_drawable(v, Some(&[])).expect("a drawable").shaders;
    assert_eq!(shaders(&saved), shaders(&stored));
    assert!(saved != stored);
    // A copy of the saved file writes the same bytes: bones as read write as nothing.
    let copy = dir.path().join("copy.rsc");
    assert!(state.save_active_document_as(copy.clone()));
    finished(&mut state);
    assert!(std::fs::read(&copy).ok() == std::fs::read(&path).ok());
}

#[test]
fn a_renamed_bone_keeps_its_hash_undoes_and_saves_with_its_new_name() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path: PathBuf = dir.path().join("renamed.rsc");
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: skinned_body(),
        physical: Vec::new(),
    };
    std::fs::write(&path, write_rpf9_resource(&sections).expect("writes")).expect("written");
    let mut state = AppState::new();
    state.load_file(path.clone());
    settle(&mut state);
    let doc =
        (state.active_document).unwrap_or_else(|| panic!("opened: {}", state.error_dialog_message));
    let opened_as = held(&path);
    for name in ["", "SKEL\0Neck"] {
        let edit = BoneEdit::Name(name.into());
        assert!(state.edit_gta6_bone(doc, 1, edit).is_err(), "{name:?}");
    }
    assert_eq!(held(&path), opened_as);

    let name = "SKEL_Head_Renamed_Longer";
    state
        .edit_gta6_bone(doc, 1, BoneEdit::Name(name.into()))
        .expect("a name");
    let renamed = held(&path);
    assert_eq!(renamed.bones[1].name, name);
    assert_eq!(renamed.bones[1].hash, name_hash32(b"SKEL_Head"));
    assert_eq!(
        renamed.bones[1].inverse_bind,
        opened_as.bones[1].inverse_bind
    );
    state.active_document = Some(doc);
    assert!(state.undo().is_some());
    assert_eq!(held(&path), opened_as, "undone, it is as it opened");
    assert!(state.redo().is_some());

    // Saved over its own file: the name reads back, the hash and the numbers as they were.
    assert!(state.save_active_document());
    finished(&mut state);
    let read = read_skeleton(&read_back(&path), false)
        .expect("reads")
        .expect("a skeleton");
    assert_eq!(read, renamed);
}
