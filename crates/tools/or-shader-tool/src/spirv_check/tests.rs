use super::*;
use crate::spirv_testing::{dwords, entry, fragment_with_a_demote, op};

/// A fragment shader that does nothing, which naga validates and writes.
fn empty_fragment() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 5, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(4, 1));
    words.extend(op(16, &[1, 7]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[4]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn a_module_that_naga_reads_validates_and_writes_reaches_msl() {
    let verdict = check(&empty_fragment());
    assert_eq!(verdict.reached, Reached::MslWritten, "{:?}", verdict.cause);
    assert_eq!(verdict.stage, Some(4));
    assert_eq!(verdict.cause, None);
}

#[test]
fn a_translators_module_reaches_msl_only_through_the_legalizer() {
    let verdict = check(&fragment_with_a_demote(true));
    assert_eq!(verdict.reached, Reached::MslWritten, "{:?}", verdict.cause);
    assert_eq!(verdict.lowerings.demotes_to_kill, 1);
}

#[test]
fn bytes_that_are_not_a_module_stop_at_the_reader_with_a_cause() {
    let verdict = check(&[0, 1, 2, 3]);
    assert_eq!(verdict.reached, Reached::Refused);
    assert!(verdict.cause.is_some());
}

#[test]
fn a_vertex_shader_with_no_position_stops_after_the_parse_and_says_so() {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 5, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(0, 1));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[4]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    let verdict = check(&dwords(&words));
    assert_eq!(verdict.reached, Reached::Parsed);
    assert!(
        verdict
            .cause
            .as_deref()
            .is_some_and(|c| c.contains("position")),
        "{:?}",
        verdict.cause
    );
}

#[test]
fn causes_fold_their_numbers_and_the_tally_counts_each_step() {
    assert_eq!(
        fold("unsupported execution mode %4462\nmore"),
        "unsupported execution mode %N"
    );
    let mut tally = Tally::default();
    tally.add(&check(&empty_fragment()));
    tally.add(&check(&empty_fragment()));
    tally.add(&check(&[1, 2, 3, 4]));
    assert_eq!(tally.modules(), 3);
    assert_eq!(tally.stages[&Some(4)].msl, 2);
    let report = tally.report();
    assert!(
        report.contains("3 modules: 2 parsed, 2 validated, 2 written as MSL"),
        "{report}"
    );
    assert_eq!(tally.to_json()["modules"], 3);
}

#[test]
fn a_folder_is_checked_file_by_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.spv"), empty_fragment()).unwrap();
    std::fs::write(tmp.path().join("b.spv"), fragment_with_a_demote(false)).unwrap();
    let tally = check_path(tmp.path()).unwrap();
    assert_eq!(tally.modules(), 2);
    assert_eq!(tally.stages[&Some(4)].msl, 2);
}

#[test]
fn a_modules_stage_is_named_from_its_execution_model_not_a_d3d_program_type() {
    let mut tally = Tally::default();
    tally.add(&check(&empty_fragment()));
    let report = tally.report();
    assert!(
        report.contains("\n  PS "),
        "a fragment entry point reads as PS: {report}"
    );
    assert_eq!(tally.to_json()["stages"][0]["stage"], "PS");
    assert_eq!(
        [0, 1, 2, 3, 4, 5].map(spirv_stage_label),
        ["VS", "HS", "DS", "GS", "PS", "CS"]
    );
}
