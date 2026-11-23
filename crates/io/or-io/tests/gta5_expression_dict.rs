//! A `.yed` authored as a plain Rust value, with invented content: one
//! expression set holding one program whose three runs are 3, 4 and 2 bytes,
//! saved whole, reopened, edited, and refused where its stated scratch is
//! shorter than its program. Needs no game file.

use or_io::gta5::expression_dict::key_of;
use or_io::gta5::formats;
use or_io::gta5::*;
use or_io::Generation;

const NAME: &str = "pack:/wheel.expr";
const DATA: [u8; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];

fn authored(scratch: u32) -> CrExpressionsDictionary {
    let expressions = CrExpressions {
        expression_streams: vec![Some(Box::new(CrExpressionStream::new(
            0xABCD_0001,
            5,
            &DATA[..3],
            &DATA[3..7],
            &DATA[7..],
        )))],
        tracks: vec![Track {
            id: 0xD325,
            track: 0,
            flags: Track::CONTINUES,
            ..Default::default()
        }],
        name: AtString::new(NAME),
        max_packed_size: scratch,
        ..Default::default()
    };
    CrExpressionsDictionary {
        codes: vec![key_of(NAME)],
        entries: vec![Some(Box::new(expressions))],
        ..Default::default()
    }
}

fn stream(dict: &CrExpressionsDictionary) -> &CrExpressionStream {
    dict.entries[0].as_ref().unwrap().expression_streams[0]
        .as_ref()
        .unwrap()
}

fn payload(file: &[u8]) -> Vec<u8> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    let o = Rsc7::open(file).unwrap();
    let mut out = o.seal.header_bytes();
    out.extend_from_slice(&o.data);
    out
}

/// ⚠ A length taken from any one of the three counts would carry 3, 4 or 2
/// bytes and drop the rest of the program on save.
#[test]
fn an_authored_yed_saves_every_run_and_reaches_a_fixed_point() {
    let dict = authored(0x19);
    let first = dict.to_bytes().unwrap();
    let again = CrExpressionsDictionary::from_bytes(&first).unwrap();
    assert_eq!(again, dict);
    assert_eq!(stream(&again).data, DATA);
    assert_eq!(stream(&again).size(), 0x19);
    let e = again.entries[0].as_ref().unwrap();
    assert_eq!(e.name.data, NAME);
    assert_eq!(
        (
            e.tracks[0].id,
            e.tracks[0].format(),
            e.tracks[0].continues()
        ),
        (0xD325, 0, true)
    );
    assert_eq!(payload(&again.to_bytes().unwrap()), payload(&first));
}

#[test]
fn a_peek_reads_the_three_runs_as_one() {
    let bytes = authored(0x19).to_bytes().unwrap();
    let peek = CrExpressionsDictionary::peek(&bytes).unwrap();
    let root = peek.root().unwrap();
    let e = root.entries().unwrap().follow(0).unwrap().unwrap();
    let s = e.expression_streams().unwrap().follow(0).unwrap().unwrap();
    assert_eq!(s.data().unwrap().as_slice(), &DATA);
    assert_eq!(expression_dict::longest_program(&e).unwrap(), 0x19);
}

/// A dictionary built in code states every reference count as 1, as every shipped one does.
#[test]
fn an_authored_dictionary_states_a_reference_count_of_one() {
    let bytes = authored(0x19).to_bytes().unwrap();
    let peek = CrExpressionsDictionary::peek(&bytes).unwrap();
    let root = peek.root().unwrap();
    assert_eq!({ root.ref_count }, 1, "the dictionary");
    let e = root.entries().unwrap().follow(0).unwrap().unwrap();
    assert_eq!({ e.ref_count }, 1, "the expressions");
}

#[test]
fn an_expression_whose_scratch_is_shorter_than_its_program_is_refused() {
    let why = authored(0x18).to_bytes().unwrap_err().to_string();
    assert!(why.contains("holds a 25-byte program"), "{why}");
}

/// A run longer than its count can state still builds; the save refuses it.
#[test]
fn an_operation_run_longer_than_its_count_is_refused_at_save() {
    let mut dict = authored(u32::MAX);
    let operations = vec![0u8; usize::from(u16::MAX) + 1];
    dict.entries[0].as_mut().unwrap().expression_streams[0] = Some(Box::new(
        CrExpressionStream::new(0xABCD_0001, 5, &DATA[..3], &DATA[3..7], &operations),
    ));
    let why = dict.to_bytes().unwrap_err().to_string();
    assert!(
        why.contains("align_param_size + param_size + num_operations"),
        "{why}"
    );
}

/// Patching one count in place would move where the next run begins, so all three are structural.
#[test]
fn every_count_a_program_sums_is_structural() {
    let pack = formats::format("yed")
        .unwrap()
        .description(Generation::Gen8)
        .unwrap();
    let program = pack.type_by_name("CrExpressionStream").unwrap();
    for field in ["align_param_size", "param_size", "num_operations"] {
        assert!(program.is_structural(field), "{field}");
    }
    assert!(!program.is_structural("max_stack_depth"));
}
