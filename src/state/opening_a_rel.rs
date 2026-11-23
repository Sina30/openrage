//! A `.rel`, and a DLC's `.datNN` that is the same container, opens as its own
//! kind: its items typed and grouped by type, and a save that changes nothing.
//!
//! ⚠ Every container here is built by hand, so nothing under test produced
//! the bytes it is checked against.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use or_io::rel::curve::CurveItem;
use or_io::rel::sound::SoundItem;
use or_io::rel::TypedItem;

use super::{AppState, DocPayload, DocumentId, DocumentKind, LoadingState, RelDocument};

fn word(out: &mut Vec<u8>, w: u32) {
    out.extend_from_slice(&w.to_le_bytes());
}

/// A curve item: its class id and name table offset in one word, flags, min and max input, then `rest`.
fn curve(class: u8, name_table_offset: u32, rest: &[f32]) -> Vec<u8> {
    let mut out = Vec::new();
    word(&mut out, u32::from(class) | name_table_offset << 8);
    word(&mut out, 0xA5A5_0001);
    for v in [0.0f32, 1.0].iter().chain(rest) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

const KEYS: [u32; 3] = [0x1111_1111, 0x2222_2222, 0x3333_3333];

/// A container of `items` after four unindexed filler bytes, two names, and
/// a hash table entry at the second item when there is one.
fn container(rel_type: u32, items: &[Vec<u8>]) -> Vec<u8> {
    let mut data = vec![0xEE; 4];
    let mut spans = Vec::new();
    for item in items {
        spans.push((data.len() as u32, item.len() as u32));
        data.extend_from_slice(item);
    }

    let mut out = Vec::new();
    word(&mut out, rel_type);
    word(&mut out, data.len() as u32);
    out.extend_from_slice(&data);
    let names: [&str; 2] = ["curve_a", "curve_b"];
    let strings: usize = names.iter().map(|n| n.len() + 1).sum();
    word(&mut out, (4 + 4 * names.len() + strings) as u32);
    word(&mut out, names.len() as u32);
    let mut at = 0;
    for n in names {
        word(&mut out, at);
        at += n.len() as u32 + 1;
    }
    for n in names {
        out.extend_from_slice(n.as_bytes());
        out.push(0);
    }
    word(&mut out, spans.len() as u32);
    for (key, (offset, length)) in KEYS.iter().zip(&spans) {
        word(&mut out, *key);
        word(&mut out, *offset);
        word(&mut out, *length);
    }
    // The hash table states its offsets from the file's start, 8 bytes before the data block.
    match spans.get(1) {
        Some((offset, _)) => {
            word(&mut out, 1);
            word(&mut out, offset + 8);
        }
        None => word(&mut out, 0),
    }
    word(&mut out, 0);
    out
}

/// A Constant, a Linear and a Constant curve.
fn three_curves() -> Vec<u8> {
    container(
        16,
        &[
            curve(1, 0, &[0.25]),
            curve(2, 8, &[0.5, 0.75, 1.5, 2.5]),
            curve(1, 0, &[4.0]),
        ],
    )
}

const DAT54_BYTES: [u8; 3] = [0x6C, 0x65, 0x01];

/// A `.dat54` holding one `SoundHashList` of one hash, with three bytes after its class id.
fn one_sound_hash_list() -> Vec<u8> {
    let mut item = vec![35];
    item.extend_from_slice(&DAT54_BYTES);
    word(&mut item, 0x0000_0004);
    item.extend_from_slice(&7u16.to_le_bytes());
    word(&mut item, 1);
    word(&mut item, 0xAABB_CCDD);
    container(54, &[item])
}

fn open(state: &mut AppState, path: PathBuf) -> DocumentId {
    state.load_file(path);
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.load_receiver.is_some() && Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(state.loading, LoadingState::Idle),
        "the load did not finish: {}",
        state.error_dialog_message
    );
    let doc = state
        .active_document
        .expect("the .rel opened as a document");
    assert_eq!(state.document(doc).map(|d| d.kind), Some(DocumentKind::Rel));
    doc
}

fn opened(name: &str, bytes: &[u8]) -> (tempfile::TempDir, AppState, DocumentId) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("the container is written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    (dir, state, doc)
}

fn rel(state: &AppState, doc: DocumentId) -> &RelDocument {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rel(rel)) => rel,
        _ => panic!("not a .rel document"),
    }
}

fn groups(rel: &RelDocument) -> Vec<(&str, Vec<usize>)> {
    rel.groups
        .iter()
        .map(|(name, items)| (name.as_str(), items.clone()))
        .collect()
}

fn saved(state: &mut AppState, path: PathBuf) -> Vec<u8> {
    assert!(state.save_active_document_as(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::read(&path).expect("the save wrote the file")
}

#[test]
fn a_rel_opens_with_its_items_typed_and_grouped_by_type() {
    let (_dir, state, doc) = opened("curves.dat16.rel", &three_curves());
    let rel = rel(&state, doc);

    assert_eq!(rel.file.rel_type, 16);
    assert_eq!(rel.taxonomy(), "curves");
    assert_eq!(
        groups(rel),
        vec![("Constant", vec![0, 2]), ("Linear", vec![1])]
    );
    assert_eq!(
        (0..3).map(|i| rel.key(i)).collect::<Vec<_>>(),
        ["0x11111111", "0x22222222", "0x33333333"]
    );
    let Some(TypedItem::Curve(CurveItem::Linear(linear))) = rel.item(1) else {
        panic!("item 1 is {:?}", rel.item(1));
    };
    assert_eq!(linear.base.name_table_offset.0, 8);
    assert_eq!(linear.base.flags, 0xA5A5_0001);
    assert_eq!(
        (linear.left_hand_pair.x, linear.right_hand_pair.y),
        (0.5, 2.5)
    );
    let Some(TypedItem::Curve(CurveItem::Constant(last))) = rel.item(2) else {
        panic!("item 2 is {:?}", rel.item(2));
    };
    assert_eq!(last.value, 4.0);
    assert!(!state.document(doc).expect("doc").dirty);
}

#[test]
fn a_rel_saves_unchanged() {
    let bytes = three_curves();
    let (dir, mut state, doc) = opened("curves.dat16.rel", &bytes);
    let path = dir.path().join("saved.dat16.rel");
    assert_eq!(saved(&mut state, path.clone()), bytes);

    let mut again = AppState::new();
    let reopened = open(&mut again, path);
    assert_eq!(groups(rel(&again, reopened)), groups(rel(&state, doc)));
}

/// ⚠ The same bytes named `.dat54.rel` stay bytes: only a `.dat54` holds the three bytes after the class id.
#[test]
fn a_dat54_types_its_sounds_past_the_three_bytes_its_rel_sibling_lacks() {
    let bytes = one_sound_hash_list();
    let (dir, mut state, doc) = opened("dlctest_sounds.dat54", &bytes);
    {
        let rel = rel(&state, doc);
        assert_eq!(groups(rel), vec![("SoundHashList", vec![0])]);
        assert_eq!(rel.dat54_bytes, vec![DAT54_BYTES]);
        let Some(TypedItem::Sound(SoundItem::SoundHashList(list))) = rel.item(0) else {
            panic!("item 0 is {:?}", rel.item(0));
        };
        assert_eq!(list.current_sound_idx, 7);
        assert_eq!(list.sound_hashes.len(), 1);
        assert_eq!(list.sound_hashes[0].sound_hash, 0xAABB_CCDD);
    }
    let path = dir.path().join("saved.dat54");
    assert_eq!(saved(&mut state, path), bytes);

    let (_dir, state, doc) = opened("dlctest_sounds.dat54.rel", &bytes);
    let rel = rel(&state, doc);
    assert!(rel.typed.is_err());
    assert_eq!(groups(rel), vec![("class id 35", vec![0])]);
    assert!(rel.body(0).is_some_and(|body| body.len() == 18));
}

/// A `rel_type` with no types here still opens, says why, and saves unchanged.
#[test]
fn a_rel_type_with_no_types_opens_as_bytes_and_saves_unchanged() {
    let bytes = container(99, &[vec![3, 1, 2, 3]]);
    let (dir, mut state, doc) = opened("odd.rel", &bytes);
    let why = rel(&state, doc).typed.clone().expect_err("no types");
    assert!(why.contains("99"), "{why}");
    assert_eq!(groups(rel(&state, doc)), vec![("class id 3", vec![0])]);
    assert_eq!(saved(&mut state, dir.path().join("saved.rel")), bytes);
}
