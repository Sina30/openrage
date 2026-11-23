use std::time::{Duration, Instant};

use super::super::opening_a_gta_vi_awc::{gta_vi, opus_packets, opus_single};
use super::super::opening_an_rpf9_archive::{rows, rpf9_head, scanned, settle, written, zlib};
use super::*;
use crate::state::gta6_text::Gta6AudioDocument;
use crate::state::jobs::JobKind;
use crate::state::rpf9_scan::{body_path, View};
use crate::state::DocumentKind;
use or_io::gta6::audio_data::STAMP;
use or_io::gta6::hash::name_hash32;

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A body of `schema` whose items (key, bytes) follow the stamp, with no group, `names` encrypted
/// names, the wave fields `waves` (data offsets) and no other field.
fn body(schema: u32, items: &[(u32, Vec<u8>)], names: u32, waves: &[u32]) -> Vec<u8> {
    let mut data = STAMP.to_le_bytes().to_vec();
    let mut index = Vec::new();
    for (key, bytes) in items {
        index.push((*key, data.len() as u32));
        data.extend(bytes);
    }
    index.sort_by_key(|(key, _)| (key & 0xFF, key >> 8));
    let mut v = words(&[schema, data.len() as u32]);
    v.extend(&data);
    v.extend(words(&[4, 0, 4 + 4 * names, names]));
    v.extend(words(&vec![0x5A5A_5A5A; names as usize]));
    v.extend(words(&[index.len() as u32]));
    index
        .iter()
        .for_each(|(key, at)| v.extend(words(&[*key, *at])));
    v.extend(words(&[0, waves.len() as u32]));
    v.extend(words(&waves.iter().map(|w| w + 8).collect::<Vec<_>>()));
    v.extend(words(&[0, 0, 0]));
    v
}

/// A SimpleSound with no field mask: its bank, its wave, two zero bytes and no row.
fn simple(bank: u32, wave: u32) -> Vec<u8> {
    let mut item = vec![12, 0x00];
    item.extend(words(&[bank, wave]));
    item.extend([0, 0, 0]);
    item
}

/// Append `bytes` at the next 4 KiB page plus 16, as a scan finds a zlib body, and say where.
fn put(archive: &mut Vec<u8>, bytes: &[u8]) -> u64 {
    archive.resize(archive.len().next_multiple_of(0x1000) + 16, 0);
    let at = archive.len() as u64;
    archive.extend(zlib(bytes));
    at
}

fn linked(state: &AppState, doc: DocumentId) -> bool {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Gta6AudioData(audio)) => {
            (audio.linking.as_ref()).is_some_and(|l| l.ended().is_some())
        }
        _ => panic!("not audio game data"),
    }
}

fn finished(state: &AppState, doc: DocumentId) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !linked(state, doc) {
        assert!(Instant::now() < deadline, "the look never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn with_linked<T>(state: &AppState, doc: DocumentId, f: impl FnOnce(&Linked) -> T) -> T {
    let Some(DocPayload::Gta6AudioData(audio)) = state.document(doc).map(|d| &d.payload) else {
        panic!("not audio game data");
    };
    let linking = audio.linking.as_ref().expect("a look");
    let ended = linking.ended();
    f(ended.as_ref().expect("ended"))
}

#[test]
fn what_gta_vi_audio_game_data_links_to_is_found_in_its_scan_as_a_job_and_opens() {
    let _serial = crate::ui::theme::serial();
    let (linked_bank, absent_bank) = (0xB0B0_0001, 0xB0B0_0002);
    let mut bytes = rpf9_head(1, 0x1A0);
    bytes.resize(0x1000, 0);
    let packets = opus_packets(0xB8, 20);
    bytes.extend(gta_vi(0xFF21, &[(0x0B02, opus_single(&packets, 1, -1))]));
    let items = [
        (0x1111_1101, simple(linked_bank, 0xE000_0B02)),
        (0x2222_2202, simple(absent_bank, 0x4000_0777)),
    ];
    let sounds = body(166, &items, 2, &[4 + 2, 17 + 2]);
    let sounds_at = put(&mut bytes, &sounds);
    let (one, two) = (name_hash32(b"cfg_one"), name_hash32(b"cfg_two"));
    let config_item = |at: u8| vec![1, at, 0, 0, 0xAA, 0xAA, 0xAA, 0xAA];
    let config = body(26, &[(two, config_item(8)), (one, config_item(0))], 0, &[]);
    let config_at = put(&mut bytes, &config);
    let pool_at = put(&mut bytes, b"Cfg_One\0cfg_two\0");
    let dir = tempfile::tempdir().expect("a scratch directory");
    let archive = written(dir.path(), &bytes);
    let mut state = AppState::new();
    let scan = scanned(&mut state, archive.clone());
    assert!(
        rows(&state, scan)
            .iter()
            .any(|r| matches!(r, Row::Body(f) if f.offset == pool_at)),
        "the pool is a body of the scan"
    );
    let open = |state: &mut AppState, at: u64, body: &[u8]| {
        let audio = Gta6AudioDocument::open(body).expect("audio game data");
        let path = body_path(&archive, at, View::AudioData);
        state.insert_document(path, DocPayload::Gta6AudioData(Box::new(audio)), None, None)
    };

    let doc = open(&mut state, sounds_at, &sounds);
    state.link_gta6_audio(doc).expect("a scan to look in");
    finished(&state, doc);
    with_linked(&state, doc, |linked| {
        let Linked::Waves {
            banks,
            found,
            containers,
            linked,
            fields,
        } = linked
        else {
            panic!("the waves were not looked for");
        };
        assert_eq!(banks.get(&linked_bank), Some(&BankLink::Container(0x1000)));
        assert_eq!(banks.get(&absent_bank), Some(&BankLink::Absent));
        assert_eq!(found.iter().copied().collect::<Vec<_>>(), [0x0B02]);
        assert_eq!((*containers, *linked, *fields), (1, 1, 2));
    });

    // A look stopped from the job list ends.
    state.link_gta6_audio(doc).expect("a scan to look in");
    state.cancel_job(JobKind::AudioLinks);
    finished(&state, doc);
    assert!(
        state.audio_linking_jobs().next().is_none(),
        "no look is listed once ended"
    );

    state.open_gta6_stream(&archive, 0x1000, 0x0B02);
    settle(&mut state);
    let container = (state.active_document).expect("the container opened");
    assert_eq!(
        state.document(container).map(|d| d.kind),
        Some(DocumentKind::Awc)
    );

    let doc = open(&mut state, config_at, &config);
    state.link_gta6_audio(doc).expect("a scan to look in");
    finished(&state, doc);
    with_linked(&state, doc, |linked| {
        assert!(
            matches!(linked, Linked::Names { pool, count: 2 } if *pool == pool_at),
            "the name pool after the body"
        );
    });
    assert_eq!(pool_names().get(&one).map(String::as_str), Some("Cfg_One"));
    assert_eq!(pool_names().get(&two).map(String::as_str), Some("cfg_two"));
}

#[test]
fn audio_game_data_that_is_no_scanned_body_says_no_scan_holds_its_links() {
    let _serial = crate::ui::theme::serial();
    let mut state = AppState::new();
    let items = [(0x1111_1101, simple(0xB0B0_0001, 0xE000_0B02))];
    let audio = Gta6AudioDocument::open(&body(166, &items, 1, &[6])).expect("audio game data");
    let dir = tempfile::tempdir().expect("a scratch directory");
    let doc = state.insert_document(
        dir.path().join("sounds.rel"),
        DocPayload::Gta6AudioData(Box::new(audio)),
        None,
        None,
    );
    let why = state.link_gta6_audio(doc).expect_err("refused");
    assert!(why.contains("no body of a scanned archive"), "{why}");
}
