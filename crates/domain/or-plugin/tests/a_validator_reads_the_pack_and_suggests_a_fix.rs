//! A validator may read the pack as data and not only the plan, and a finding may carry a suggested
//! fix: files that are written together if a person accepts it, and not before. Each case is a guest
//! written in WAT beside the assertion.

use or_plugin::extension::{self, Level, MAX_EMITTED, MAX_PLAN};
use or_plugin::{
    Capability, FileWrite, Finding, Grants, HostError, Limits, Status, SuggestedFix, Transaction,
};

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

fn data(at: usize, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|b| format!("\\{b:02x}")).collect();
    format!("(data (i32.const {at}) \"{escaped}\")")
}

fn granting(capabilities: &[Capability]) -> Grants {
    capabilities
        .iter()
        .fold(Grants::none(), |grants, c| grants.granting(*c))
}

fn reading_and_suggesting() -> Grants {
    granting(&[Capability::ReadPack, Capability::SuggestFixes])
}

fn fix_of(path: &str, text: &str) -> Transaction {
    Transaction {
        files: vec![FileWrite {
            path: path.into(),
            bytes: text.as_bytes().to_vec(),
        }],
    }
}

const IMPORTS: &str = r#"
  (import "openrage" "pack_size" (func $pack_size (param i32) (result i32)))
  (import "openrage" "read_pack" (func $read_pack (param i32 i32 i32) (result i32)))
  (import "openrage" "report_with_fix"
    (func $report_with_fix (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
"#;

/// A validator that reads the whole pack into memory at 4096 and reports it back as the message of a
/// finding `pack.echo`, with the fix `label` that writes `fix`.
fn echoing(label: &str, fix: &Transaction) -> Vec<u8> {
    let files = fix.encode();
    wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 2)
  {code}{label_data}{fix_data}
  (func (export "validate")
    (drop (call $pack_size (i32.const 16)))
    (if (i32.ne (call $read_pack (i32.const 0) (i32.const 4096) (i32.load (i32.const 16))) (i32.const 0))
      (then unreachable))
    (drop (call $report_with_fix
      (i32.const 64) (i32.const 9) (i32.const 1) (i32.const 0) (i32.const 0)
      (i32.const 4096) (i32.load (i32.const 16))
      (i32.const 128) (i32.const {label_len})
      (i32.const 512) (i32.const {files_len})))))"#,
        code = data(64, b"pack.echo"),
        label_data = data(128, label.as_bytes()),
        label_len = label.len(),
        fix_data = data(512, &files),
        files_len = files.len(),
    ))
}

#[test]
fn a_validator_reads_the_pack_and_returns_a_finding_with_a_fix_a_person_can_accept() {
    let pack =
        br#"{"packs":[{"content":"clothing","folder":"clothing","wardrobe":{"garments":[]}}]}"#;
    let fix = fix_of(
        "clothing/wardrobe.json",
        r#"{"garments":[],"pack":"named"}"#,
    );

    let found = extension::validate_with_pack(
        &echoing("Name the pack", &fix),
        &reading_and_suggesting(),
        Limits::default(),
        b"{}",
        pack,
    )
    .expect("a validator runs");

    assert_eq!(
        found.findings,
        [Finding {
            code: "pack.echo".into(),
            level: Level::Warning,
            subject: None,
            message: String::from_utf8(pack.to_vec()).unwrap(),
            fix: Some(SuggestedFix {
                label: "Name the pack".into(),
                files: fix
            }),
        }],
        "the message is the pack the guest read, so the pack crossed in whole, and the fix crossed out beside it"
    );
    assert!(found.files.is_empty() && found.failures.is_empty());
}

#[test]
fn without_a_pack_to_read_the_size_is_zero_and_a_plan_alone_is_unchanged() {
    let guest = wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 1)
  (func (export "validate")
    (drop (call $pack_size (i32.const 16)))
    (if (i32.ne (i32.load (i32.const 16)) (i32.const 0)) (then unreachable))))"#
    ));

    let found = extension::validate(&guest, &reading_and_suggesting(), Limits::default(), b"{}")
        .expect("a validator with no pack to read is told it is empty");

    assert!(found.findings.is_empty());
}

#[test]
fn the_pack_and_a_fix_are_two_capabilities_each_with_its_own_functions() {
    let reads = granting(&[Capability::ReadPack]);
    assert_eq!(extension::functions_for(&reads), ["pack_size", "read_pack"]);
    assert_eq!(
        extension::capability_for("report_with_fix"),
        Some(Capability::SuggestFixes)
    );

    let fix = fix_of("a.txt", "1");
    let no_pack = extension::validate_with_pack(
        &echoing("Fix", &fix),
        &granting(&[Capability::SuggestFixes]),
        Limits::default(),
        b"{}",
        b"{}",
    )
    .expect_err("reading the pack is not suggesting a fix");
    assert!(
        matches!(&no_pack, HostError::Unresolved { import, needs: Some(Capability::ReadPack) } if import == "pack_size"),
        "{no_pack:?}"
    );

    let no_fix = extension::validate_with_pack(
        &echoing("Fix", &fix),
        &granting(&[Capability::ReadPack]),
        Limits::default(),
        b"{}",
        b"{}",
    )
    .expect_err("suggesting a fix is not reading the pack");
    assert!(
        matches!(&no_fix, HostError::Unresolved { import, needs: Some(Capability::SuggestFixes) } if import == "report_with_fix"),
        "{no_fix:?}"
    );
}

#[test]
fn a_fix_that_cannot_be_kept_costs_the_whole_finding_and_the_guest_is_told_why() {
    for (label, fix, why) in [
        ("Fix", fix_of("../outside.txt", "1"), "leaves the project"),
        ("Fix", fix_of("/rooted.txt", "1"), "rooted"),
        ("   ", fix_of("a.txt", "1"), "no label"),
        ("Fix", Transaction::default(), "none"),
    ] {
        let found = extension::validate_with_pack(
            &echoing(label, &fix),
            &reading_and_suggesting(),
            Limits::default(),
            b"{}",
            b"{}",
        )
        .expect("a validator runs");

        assert!(found.findings.is_empty(), "{why}: {:?}", found.findings);
        let [failure] = found.failures.as_slice() else {
            panic!("{why}: one refusal expected: {:?}", found.failures);
        };
        assert_eq!(failure.status, Status::Refused, "{why}");
        assert!(
            failure.because.as_deref().unwrap().contains(why),
            "{why}: {:?}",
            failure.because
        );
    }
}

#[test]
fn the_fixes_of_one_call_hold_at_most_the_bytes_the_files_of_a_call_do() {
    let big = MAX_EMITTED / 2 + 1;
    // Two fixes of just over half the allowance each: the second is refused and the first stands.
    // The header of one file is written as data and its body filled in, so the fixture stays small.
    let mut head = fix_of("a.txt", "").encode();
    let at = head.len() - 4;
    head[at..].copy_from_slice(&(big as u32).to_le_bytes());
    let guest = wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 80)
  {code}{label}{message}{head}
  (func (export "validate")
    (memory.fill (i32.const {body_at}) (i32.const 120) (i32.const {big}))
    (drop (call $report_with_fix (i32.const 64) (i32.const 9) (i32.const 1) (i32.const 0) (i32.const 0)
      (i32.const 4096) (i32.const 1) (i32.const 128) (i32.const 3) (i32.const 8192) (i32.const {len})))
    (drop (call $report_with_fix (i32.const 64) (i32.const 9) (i32.const 1) (i32.const 0) (i32.const 0)
      (i32.const 4096) (i32.const 1) (i32.const 128) (i32.const 3) (i32.const 8192) (i32.const {len})))))"#,
        code = data(64, b"pack.echo"),
        label = data(128, b"Fix"),
        message = data(4096, b"m"),
        head = data(8192, &head),
        body_at = 8192 + head.len(),
        len = head.len() + big,
    ));

    let found = extension::validate(&guest, &reading_and_suggesting(), Limits::default(), b"{}")
        .expect("a validator runs");

    assert_eq!(found.findings.len(), 1);
    assert_eq!(found.failures.len(), 1);
    assert!(found.failures[0]
        .because
        .as_deref()
        .unwrap()
        .contains("at most"));
}

#[test]
fn a_pack_over_what_a_plugin_is_handed_is_refused_before_the_guest_runs() {
    let pack = vec![b' '; MAX_PLAN + 1];
    let refused = extension::validate_with_pack(
        &echoing("Fix", &fix_of("a.txt", "1")),
        &reading_and_suggesting(),
        Limits::default(),
        b"{}",
        &pack,
    )
    .expect_err("too much to hand over");
    assert!(refused.to_string().contains("pack"), "{refused}");
}

#[test]
fn a_finding_without_a_fix_is_what_it_always_was() {
    let guest = wasm(
        r#"(module
  (import "openrage" "report" (func $report (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "a.b")
  (data (i32.const 8) "hello")
  (func (export "validate")
    (drop (call $report (i32.const 0) (i32.const 3) (i32.const 2) (i32.const 0) (i32.const 0)
                        (i32.const 8) (i32.const 5)))))"#,
    );

    let found = extension::validate(
        &guest,
        &granting(&[Capability::ReportFindings]),
        Limits::default(),
        b"{}",
    )
    .expect("a validator runs");

    assert_eq!(found.findings[0].fix, None);
    assert_eq!(found.findings[0].level, Level::Error);
}
