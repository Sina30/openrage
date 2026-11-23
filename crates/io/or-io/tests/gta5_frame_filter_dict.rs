//! A `.yfd` authored as a plain Rust value, with invented content: saved as a
//! whole file, reopened, edited, saved to a fixed point, peeked in place, and
//! refused where the game would misread it. Needs no game file.

use or_io::decl::{Open, Raw};
use or_io::gta5::formats;
use or_io::gta5::frame_filter_dict::checksum_raw;
use or_io::gta5::*;
use or_io::Generation;

fn filter(track_id_indices: Vec<TrackIdIndex>, weights: Vec<f32>) -> Option<Box<CrFrameFilterAny>> {
    Some(Box::new(CrFrameFilterAny::CrFrameFilterMultiWeight(
        CrFrameFilterMultiWeight {
            base: CrFrameFilter {
                r#type: Open::new(EFrameFilterType::MultiWeight),
                ..Default::default()
            },
            track_id_indices,
            weights,
            ..Default::default()
        },
    )))
}

/// Two filters: the first with two entries sharing one weight.
fn authored(first: [TrackIdIndex; 2]) -> CrFrameFilterDictionary {
    CrFrameFilterDictionary {
        codes: vec![0x100, 0x200],
        entries: vec![
            filter(first.to_vec(), vec![0.5]),
            filter(vec![TrackIdIndex::new(0, 3, 0)], vec![1.0]),
        ],
        ..Default::default()
    }
}

fn sorted() -> [TrackIdIndex; 2] {
    [TrackIdIndex::new(0, 10, 0), TrackIdIndex::new(1, 5, 0)]
}

fn multi(dict: &CrFrameFilterDictionary, i: usize) -> &CrFrameFilterMultiWeight {
    match dict.entries[i].as_deref() {
        Some(CrFrameFilterAny::CrFrameFilterMultiWeight(w)) => w,
        other => panic!("filter {i} is {other:?}"),
    }
}

fn payload(file: &[u8]) -> Vec<u8> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    let o = Rsc7::open(file).unwrap();
    let mut out = o.seal.header_bytes();
    out.extend_from_slice(&o.data);
    out
}

#[test]
fn an_authored_yfd_saves_reopens_and_reaches_a_fixed_point() {
    let dict = authored(sorted());
    let first = dict.to_bytes().unwrap();
    let reopened = CrFrameFilterDictionary::from_bytes(&first).unwrap();
    assert_eq!(reopened, dict);
    assert_eq!(multi(&reopened, 1).track_id_indices[0].id, 3);
    assert_eq!(multi(&reopened, 0).weights, [0.5]);
    assert_eq!(payload(&reopened.to_bytes().unwrap()), payload(&first));
}

#[test]
fn an_edit_is_a_field_assignment_and_a_vec_operation() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("owned_yfd");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("filters.yfd");
    authored(sorted()).save_as(&path).unwrap();

    let mut dict = CrFrameFilterDictionary::open(&path).unwrap();
    if let Some(CrFrameFilterAny::CrFrameFilterMultiWeight(w)) = dict.entries[0].as_deref_mut() {
        w.weights[0] = 0.25;
        w.track_id_indices.push(TrackIdIndex::new(2, 1, 0));
    }
    dict.save().unwrap();

    let again = CrFrameFilterDictionary::open(&path).unwrap();
    let w = multi(&again, 0);
    assert_eq!(w.weights, [0.25]);
    assert_eq!(w.track_id_indices.len(), 3);
    for i in 0..2 {
        assert_eq!(
            signature(&std::fs::read(&path).unwrap(), i),
            signed(&std::fs::read(&path).unwrap(), i),
            "filter {i}'s signature is written from its tables"
        );
    }
}

/// The signature filter `i` of `file` states.
fn signature(file: &[u8], i: usize) -> u32 {
    let peek = CrFrameFilterDictionary::peek(file).unwrap();
    let root = peek.root().unwrap();
    let f = root.entries().unwrap().follow(i).unwrap().unwrap();
    f.signature
}

/// The signature filter `i` of `file` should state: its tables' CRC.
fn signed(file: &[u8], i: usize) -> u32 {
    let peek = CrFrameFilterDictionary::peek(file).unwrap();
    let root = peek.root().unwrap();
    let f = root.entries().unwrap().follow(i).unwrap().unwrap();
    let w = or_io::decl::View::variant::<Raw<CrFrameFilterMultiWeight>>(&f)
        .unwrap()
        .unwrap();
    checksum_raw(
        w.track_id_indices().unwrap().as_slice(),
        w.weights().unwrap().as_slice(),
    )
}

/// A dictionary built in code states every reference count as 1, as every shipped one does.
#[test]
fn an_authored_dictionary_states_a_reference_count_of_one() {
    let file = authored(sorted()).to_bytes().unwrap();
    let peek = CrFrameFilterDictionary::peek(&file).unwrap();
    let root = peek.root().unwrap();
    assert_eq!({ root.ref_count }, 1, "the dictionary");
    let entries = root.entries().unwrap();
    for i in 0..entries.len() {
        let f = entries.follow(i).unwrap().unwrap();
        assert_eq!({ f.ref_count }, 1, "filter {i}");
    }
}

#[test]
fn a_dictionary_saves_its_codes_ascending_with_each_entry_beside_its_code() {
    let mut dict = authored(sorted());
    dict.codes.reverse();
    dict.entries.reverse();
    let saved = dict.to_bytes().unwrap();
    let again = CrFrameFilterDictionary::from_bytes(&saved).unwrap();
    assert_eq!(again.codes, [0x100, 0x200]);
    assert_eq!(multi(&again, 0).weights, [0.5]);
    assert_eq!(multi(&again, 1).track_id_indices[0].id, 3);
}

#[test]
fn a_peek_reads_in_place() {
    let bytes = authored(sorted()).to_bytes().unwrap();
    let peek = CrFrameFilterDictionary::peek(&bytes).unwrap();
    let root = peek.root().unwrap();
    assert_eq!(root.codes().unwrap().as_slice(), &[0x100, 0x200]);
    let first = root.entries().unwrap().follow(0).unwrap().unwrap();
    let w = or_io::decl::View::variant::<Raw<CrFrameFilterMultiWeight>>(&first)
        .unwrap()
        .unwrap();
    assert_eq!(w.track_id_indices().unwrap()[1].id, 5);
}

#[test]
fn an_unsorted_track_table_is_refused() {
    let [a, b] = sorted();
    let why = authored([b, a]).to_bytes().unwrap_err().to_string();
    assert!(why.contains("unsorted track table"), "{why}");
}

#[test]
fn a_variant_is_one_call_from_the_pointer_that_holds_it() {
    let mut dict = authored(sorted());
    dict.entries[0].as_multi_weight_mut().unwrap().weights[0] = 0.75;
    assert_eq!(dict.entries[0].as_multi_weight().unwrap().weights, [0.75]);
    assert_eq!(
        dict.entries[1]
            .as_deref()
            .and_then(CrFrameFilterAny::as_multi_weight),
        Some(multi(&dict, 1))
    );

    dict.entries[1] = Some(Box::new(CrFrameFilterAny::Base(CrFrameFilter::default())));
    dict.entries.push(None);
    assert!(
        dict.entries[1].as_multi_weight().is_none(),
        "another layout"
    );
    assert!(
        dict.entries[2].as_multi_weight_mut().is_none(),
        "a null pointer"
    );
}

#[test]
fn a_refused_save_carries_its_typed_reason() {
    use or_io::gta5::frame_filter_dict::Broken;
    let [a, b] = sorted();
    let err = authored([b, a]).to_bytes().unwrap_err();
    let reason = std::error::Error::source(&err).and_then(|r| r.downcast_ref::<Broken>());
    assert_eq!(reason, Some(&Broken::Unsorted { hash: 0x100 }), "{err}");
}

#[test]
fn an_index_past_the_weights_is_refused() {
    let why = authored([TrackIdIndex::new(0, 10, 0), TrackIdIndex::new(1, 5, 1)])
        .to_bytes()
        .unwrap_err()
        .to_string();
    assert!(why.contains("index is 1"), "{why}");
}

#[test]
fn the_wrong_kind_of_file_says_what_it_is() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("owned_yfd");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("not_a_filter.ydr");
    std::fs::write(&path, authored(sorted()).to_bytes().unwrap()).unwrap();
    let why = CrFrameFilterDictionary::open(&path)
        .unwrap_err()
        .to_string();
    assert_eq!(
        why,
        "this is a drawable (.ydr), not a frame filter dictionary (.yfd)"
    );
}

#[test]
fn a_value_not_opened_from_a_file_asks_for_a_path() {
    let why = authored(sorted()).save().unwrap_err().to_string();
    assert!(why.contains("save_as"), "{why}");
}

#[test]
fn the_description_is_what_offset_of_says() {
    let pack = formats::format("yfd")
        .unwrap()
        .description(Generation::Gen8)
        .unwrap();
    let filter = pack.type_by_name("CrFrameFilter").unwrap();
    let selector = filter.selector.as_ref().expect("a tagged base");
    assert_eq!(selector.tag, "type");
    assert_eq!(selector.type_for(4), Some("CrFrameFilterMultiWeight"));
    let weighted = pack.type_by_name("CrFrameFilterMultiWeight").unwrap();
    assert_eq!(
        weighted.size,
        std::mem::size_of::<Raw<CrFrameFilterMultiWeight>>()
    );
    assert_eq!(
        weighted.field_by_name("weights").unwrap().offset,
        std::mem::offset_of!(Raw<CrFrameFilterMultiWeight>, weights)
    );
    let dict = pack.type_by_name("PgDictionary<CrFrameFilter>").unwrap();
    assert_eq!(dict.size, 0x40);
    assert_eq!(dict.field_by_name("codes").unwrap().offset, 0x20);
    assert!(dict.field_by_name("ref_count").is_some());
    assert!(
        matches!(
            dict.field_by_name("_pad_after_ref_count").map(|f| &f.repr),
            Some(or_io::desc::FieldRepr::Padding { size: 4 })
        ),
        "padding is described as padding, and so written as zero"
    );
}

#[test]
fn an_enhanced_yfd_is_not_a_second_declaration() {
    let why = formats::format("yfd")
        .unwrap()
        .description(Generation::Gen9)
        .unwrap_err();
    assert!(why.to_string().contains("Enhanced"), "{why}");
}
