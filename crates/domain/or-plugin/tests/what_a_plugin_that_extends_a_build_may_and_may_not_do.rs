//! A validator and an emitter get a plan in and give findings and files out, and reach nothing else.
//!
//! The guests are WAT written beside the assertion. Each refusal here is a case in which the host
//! would otherwise keep what a guest handed it without having looked.

use or_plugin::extension::{
    self, capability_for, functions_for, Emitted, Finding, Level, MAX_EMITTED, MAX_FILE, MAX_FILES,
    MAX_FINDINGS, MAX_PLAN,
};
use or_plugin::{Capability, Grants, HostError, Limits, Status};

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

fn granting(capabilities: &[Capability]) -> Grants {
    capabilities
        .iter()
        .fold(Grants::none(), |grants, c| grants.granting(*c))
}

/// `bytes` as a WAT data segment at `at`.
fn data(at: usize, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|b| format!("\\{b:02x}")).collect();
    format!("(data (i32.const {at}) \"{escaped}\")")
}

const IMPORTS: &str = r#"
  (import "openrage" "plan_size" (func $plan_size (param i32) (result i32)))
  (import "openrage" "read_plan" (func $read_plan (param i32 i32 i32) (result i32)))
  (import "openrage" "report" (func $report (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "openrage" "add_file" (func $add_file (param i32 i32 i32 i32) (result i32)))
"#;

const ADD_FILE: &str =
    r#"(import "openrage" "add_file" (func $add_file (param i32 i32 i32 i32) (result i32)))"#;

const READ_PLAN: &str =
    r#"(import "openrage" "read_plan" (func $read_plan (param i32 i32 i32) (result i32)))"#;

fn reading_reporting_and_adding() -> Grants {
    granting(&[
        Capability::ReadPlan,
        Capability::ReportFindings,
        Capability::AddFiles,
    ])
}

/// A validator that reads the whole plan into memory at 1024 and reports it back as the message.
fn echo_validator() -> Vec<u8> {
    wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 2)
  {code}
  (func (export "validate")
    (drop (call $plan_size (i32.const 16)))
    (if (i32.ne (call $read_plan (i32.const 0) (i32.const 1024) (i32.load (i32.const 16))) (i32.const 0))
      (then unreachable))
    (if (i32.ne (call $report (i32.const 64) (i32.const 10) (i32.const 2) (i32.const 0) (i32.const 0)
                              (i32.const 1024) (i32.load (i32.const 16))) (i32.const 0))
      (then unreachable))))"#,
        code = data(64, b"pack.empty")
    ))
}

#[test]
fn a_validator_reads_the_plan_it_is_handed_and_reports_what_it_found() {
    let plan = br#"{"format":"openrage-wardrobe-plan","files":[]}"#;
    let found = extension::validate(
        &echo_validator(),
        &reading_reporting_and_adding(),
        Limits::default(),
        plan,
    )
    .expect("a validator runs");

    assert_eq!(
        found.findings,
        [Finding {
            code: "pack.empty".into(),
            level: Level::Error,
            subject: None,
            message: String::from_utf8(plan.to_vec()).unwrap(),
            fix: None,
        }],
        "the message is the plan the guest read, so the plan crossed in whole and a finding crossed out"
    );
    assert!(found.files.is_empty() && found.failures.is_empty());
}

#[test]
fn an_emitter_adds_the_files_it_names_and_the_plan_gets_them_in_order() {
    let emitter = wasm(&format!(
        r#"(module {ADD_FILE}
  (memory (export "memory") 1)
  {a}{b}{c}{d}
  (func (export "emit")
    (drop (call $add_file (i32.const 0) (i32.const 11) (i32.const 32) (i32.const 2)))
    (drop (call $add_file (i32.const 16) (i32.const 8) (i32.const 40) (i32.const 3)))))"#,
        a = data(0, b"config/a.js"),
        b = data(16, b"b/b.json"),
        c = data(32, b"{}"),
        d = data(40, b"[1]"),
    ));
    let found = extension::emit(
        &emitter,
        &granting(&[Capability::AddFiles]),
        Limits::default(),
        b"{}",
    )
    .expect("an emitter runs");

    assert_eq!(
        found.files,
        [
            Emitted {
                path: "config/a.js".into(),
                text: "{}".into()
            },
            Emitted {
                path: "b/b.json".into(),
                text: "[1]".into()
            }
        ]
    );
}

/// A guest that imports `name` and nothing else, and does nothing with it.
fn importing(name: &str) -> Vec<u8> {
    let signature = match name {
        "plan_size" => "(param i32) (result i32)",
        "read_plan" => "(param i32 i32 i32) (result i32)",
        "report" => "(param i32 i32 i32 i32 i32 i32 i32) (result i32)",
        "add_file" => "(param i32 i32 i32 i32) (result i32)",
        other => panic!("no such function: {other}"),
    };
    wasm(&format!(
        r#"(module (import "openrage" "{name}" (func {signature}))
  (memory (export "memory") 1)
  (func (export "validate")))"#
    ))
}

#[test]
fn a_function_is_linked_only_for_the_capability_that_buys_it() {
    assert!(
        functions_for(&Grants::none()).is_empty(),
        "nothing is granted, so nothing is linked"
    );
    let all = functions_for(&reading_reporting_and_adding());
    assert_eq!(all, ["plan_size", "read_plan", "report", "add_file"]);

    for (name, needs) in [
        ("plan_size", Capability::ReadPlan),
        ("read_plan", Capability::ReadPlan),
        ("report", Capability::ReportFindings),
        ("add_file", Capability::AddFiles),
    ] {
        assert_eq!(
            capability_for(name),
            Some(needs),
            "`{name}` belongs to a capability"
        );
        let guest = importing(name);
        let everything_else = Capability::ALL
            .into_iter()
            .filter(|c| *c != needs)
            .fold(Grants::none(), |g, c| g.granting(c));
        let refused = extension::validate(&guest, &everything_else, Limits::default(), b"{}")
            .expect_err(&format!("`{name}` linked without {needs:?}"));
        assert!(
            matches!(&refused, HostError::Unresolved { import, needs: Some(c) }
                if import == name && *c == needs),
            "a missing grant names the import and the capability that buys it: {refused}"
        );
        assert!(
            refused.to_string().contains(needs.asks_for()),
            "and says what to allow: {refused}"
        );
        extension::validate(&guest, &granting(&[needs]), Limits::default(), b"{}")
            .unwrap_or_else(|e| panic!("`{name}` is bought by {needs:?}: {e}"));
    }
}

#[test]
fn a_plugin_that_extends_a_build_gets_neither_a_document_nor_a_write_gate() {
    let extending = [
        Capability::ReadPlan,
        Capability::ReadPack,
        Capability::ReportFindings,
        Capability::SuggestFixes,
        Capability::AddFiles,
    ];
    for capability in extending {
        assert!(capability.extends_a_build());
        assert!(
            !capability.writes(),
            "{capability:?} adds to a plan or suggests a change, which is written only when the user builds or accepts it"
        );
        assert!(!capability.adds_to_the_editor());
    }
    for capability in Capability::ALL {
        if !extending.contains(&capability) {
            assert!(
                !capability.extends_a_build(),
                "{capability:?} is not one of the five"
            );
        }
    }
}

/// A validator that calls `report` once with the arguments given, then ends.
fn reporting(code: &[u8], level: i32, subject: &[u8], message: &[u8]) -> Vec<u8> {
    wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 1)
  {c}{s}{m}
  (func (export "validate")
    (drop (call $report (i32.const 0) (i32.const {cl}) (i32.const {level})
                        (i32.const 128) (i32.const {sl}) (i32.const 256) (i32.const {ml})))))"#,
        c = data(0, code),
        s = data(128, subject),
        m = data(256, message),
        cl = code.len(),
        sl = subject.len(),
        ml = message.len(),
    ))
}

fn run_reporting(code: &[u8], level: i32, subject: &[u8], message: &[u8]) -> extension::Extended {
    extension::validate(
        &reporting(code, level, subject, message),
        &reading_reporting_and_adding(),
        Limits::default(),
        b"{}",
    )
    .expect("the guest runs")
}

#[test]
fn a_finding_the_host_cannot_keep_is_refused_with_the_sentence_a_user_reads() {
    for (what, code, level, message) in [
        ("an empty code", &b""[..], 2, &b"x"[..]),
        ("a code with a space", b"two words", 2, b"x"),
        ("a code with a slash", b"a/b", 1, b"x"),
        ("a message that is blank", b"ok.code", 2, b"   "),
    ] {
        let found = run_reporting(code, level, b"", message);
        assert!(found.findings.is_empty(), "{what} was kept");
        let [failure] = found.failures.as_slice() else {
            panic!("{what}: one refusal, got {:?}", found.failures);
        };
        assert_eq!(failure.status, Status::Refused, "{what}");
        assert!(
            failure.because.as_deref().is_some_and(|s| s.len() > 20),
            "{what}: the refusal carries the sentence: {failure:?}"
        );
    }

    let found = run_reporting(b"ok.code", 3, b"", b"x");
    assert!(found.findings.is_empty());
    assert_eq!(
        found.failures.first().map(|f| f.status),
        Some(Status::BadArgument),
        "a level that is none of the three is the guest's bug, not a refusal"
    );

    let found = run_reporting(b"ok.code", 0, b"item-1", b"a note about an item");
    assert_eq!(
        found.findings,
        [Finding {
            code: "ok.code".into(),
            level: Level::Note,
            subject: Some("item-1".into()),
            message: "a note about an item".into(),
            fix: None
        }]
    );
}

#[test]
fn a_call_keeps_so_many_findings_and_refuses_the_rest() {
    let many = wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 1)
  {a}{m}
  (func (export "validate") (local $i i32)
    (loop $again
      (drop (call $report (i32.const 0) (i32.const 1) (i32.const 1)
                          (i32.const 0) (i32.const 0) (i32.const 8) (i32.const 1)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $again (i32.lt_u (local.get $i) (i32.const {more}))))))"#,
        a = data(0, b"a"),
        m = data(8, b"m"),
        more = MAX_FINDINGS + 3,
    ));
    let found = extension::validate(
        &many,
        &reading_reporting_and_adding(),
        Limits::default(),
        b"{}",
    )
    .expect("the guest runs");
    assert_eq!(found.findings.len(), MAX_FINDINGS);
    assert_eq!(found.failures.len(), 3, "{:?}", found.failures);
    assert!(found
        .failures
        .iter()
        .all(|f| f.status == Status::Refused && f.function == "report"));
}

/// An emitter that calls `add_file` with the path at 0 and a text of `len` bytes of zeros at 4096.
fn adding(path: &[u8], len: usize, times: usize) -> Vec<u8> {
    let pages = (4096 + len) / 65536 + 2;
    wasm(&format!(
        r#"(module {ADD_FILE}
  (memory (export "memory") {pages})
  {p}
  (func (export "emit") (local $i i32)
    (loop $again
      (drop (call $add_file (i32.const 0) (i32.const {pl}) (i32.const 4096) (i32.const {len})))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $again (i32.lt_u (local.get $i) (i32.const {times}))))))"#,
        p = data(0, path),
        pl = path.len(),
    ))
}

fn run_adding(path: &[u8], len: usize, times: usize) -> extension::Extended {
    extension::emit(
        &adding(path, len, times),
        &granting(&[Capability::AddFiles]),
        Limits::default(),
        b"{}",
    )
    .expect("the guest runs")
}

#[test]
fn a_file_the_host_cannot_keep_is_refused_and_the_rest_are_kept() {
    let empty = run_adding(b"", 1, 1);
    assert!(empty.files.is_empty());
    assert_eq!(empty.failures[0].status, Status::Refused);

    let big = run_adding(b"a.txt", MAX_FILE + 1, 1);
    assert!(big.files.is_empty(), "a file over {MAX_FILE} bytes is kept");
    assert_eq!(big.failures[0].status, Status::Refused);

    let just = run_adding(b"a.txt", MAX_FILE, 1);
    assert_eq!(just.files.len(), 1);
    assert_eq!(just.files[0].text.len(), MAX_FILE);

    let counted = run_adding(b"a.txt", 1, MAX_FILES + 2);
    assert_eq!(counted.files.len(), MAX_FILES);
    assert_eq!(counted.failures.len(), 2);

    // MAX_EMITTED / MAX_FILE whole files fit, and the next one does not.
    let fit = MAX_EMITTED / MAX_FILE;
    let total = run_adding(b"a.txt", MAX_FILE, fit + 1);
    assert_eq!(total.files.len(), fit);
    assert_eq!(total.failures.len(), 1);
    assert!(total.failures[0]
        .because
        .as_deref()
        .is_some_and(|s| s.contains("bytes of text")));
}

#[test]
fn a_plan_over_the_most_a_guest_is_handed_is_not_run() {
    let plan = vec![b' '; MAX_PLAN + 1];
    let refused = extension::validate(
        &echo_validator(),
        &reading_reporting_and_adding(),
        Limits::default(),
        &plan,
    )
    .expect_err("the plan is too large");
    assert!(matches!(&refused, HostError::Guest(why) if why.contains("at most")));
}

#[test]
fn a_read_past_the_end_of_the_plan_writes_nothing_and_says_so() {
    let reader = wasm(&format!(
        r#"(module {READ_PLAN}
  (memory (export "memory") 1)
  (func (export "validate")
    (i32.store8 (i32.const 100) (i32.const 77))
    (drop (call $read_plan (i32.const 2) (i32.const 100) (i32.const 5)))
    (if (i32.ne (i32.load8_u (i32.const 100)) (i32.const 77)) (then unreachable))))"#
    ));
    let found = extension::validate(
        &reader,
        &granting(&[Capability::ReadPlan]),
        Limits::default(),
        b"abcd",
    )
    .expect("the guest ran to its end, so the buffer was left as it was");
    assert_eq!(found.failures.len(), 1);
    assert_eq!(found.failures[0].status, Status::BadArgument);
    assert_eq!(found.failures[0].function, "read_plan");
}

#[test]
fn a_guest_that_traps_or_spins_hands_over_nothing() {
    let trapping = wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 1)
  {a}{m}
  (func (export "validate")
    (drop (call $report (i32.const 0) (i32.const 1) (i32.const 2)
                        (i32.const 0) (i32.const 0) (i32.const 8) (i32.const 1)))
    unreachable))"#,
        a = data(0, b"a"),
        m = data(8, b"m"),
    ));
    let trapped = extension::validate(
        &trapping,
        &reading_reporting_and_adding(),
        Limits::default(),
        b"{}",
    )
    .expect_err("a trap ends the run");
    assert!(matches!(trapped, HostError::Guest(_)), "{trapped}");

    let spinning = wasm(&format!(
        r#"(module {IMPORTS}
  (memory (export "memory") 1)
  (func (export "validate") (loop $forever (br $forever))))"#
    ));
    let stopped = extension::validate(
        &spinning,
        &reading_reporting_and_adding(),
        Limits {
            fuel: Some(50_000),
            ..Limits::default()
        },
        b"{}",
    )
    .expect_err("fuel ends a spin");
    assert!(matches!(stopped, HostError::Guest(_)), "{stopped}");
}

#[test]
fn a_guest_without_the_export_the_kind_calls_is_told_so() {
    let other = wasm(r#"(module (memory (export "memory") 1) (func (export "go")))"#);
    let refused = extension::validate(&other, &Grants::none(), Limits::default(), b"{}")
        .expect_err("a validator exports `validate`");
    assert!(
        matches!(&refused, HostError::Guest(why) if why.contains("`validate`")),
        "{refused}"
    );
    let refused = extension::emit(&other, &Grants::none(), Limits::default(), b"{}")
        .expect_err("an emitter exports `emit`");
    assert!(
        matches!(&refused, HostError::Guest(why) if why.contains("`emit`")),
        "{refused}"
    );
}
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

const WASM32: &str = "wasm32-unknown-unknown";

/// The sample guest built with `extend`, whose exports are `validate` and `emit`.
fn sample_guest() -> &'static [u8] {
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    BUILT.get_or_init(|| {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugins-extend");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&plugins)
            .args([
                "build",
                "--release",
                "-p",
                "or-plugin-sample",
                "--features",
                "extend",
                "--target",
                WASM32,
            ])
            .arg("--target-dir")
            .arg(&target_dir)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "the sample guest did not build for {WASM32} with `extend`; if the target \
             is missing, `rustup target add {WASM32}`"
        );
        std::fs::read(
            target_dir
                .join(WASM32)
                .join("release/or_plugin_sample.wasm"),
        )
        .expect("the build wrote the module")
    })
}

/// The plan of a FiveM resource as the command line prints it: one file with a space, one large one, one fine.
const PLAN: &str = r#"{"format":"openrage-wardrobe-plan","schema":1,"output":"fivem","resource":"pack",
"kind":"fivem-resource","target":null,
"files":[{"path":"a b.lua","how":"copy","bytes":10,"from":null},
         {"path":"big.dat","how":"copy","bytes":2000000,"from":null},
         {"path":"ok.lua","how":"copy","bytes":1,"from":null}],
"totals":{"files":3,"bytes":2000011},"budgets":[],
"items":[{"id":"g1","name":"pack^jbib_000","digest":"d"}],"problems":[]}"#;

#[test]
fn the_sample_validator_reports_on_the_plan_it_is_handed() {
    let found = extension::validate(
        sample_guest(),
        &reading_reporting_and_adding(),
        Limits::default(),
        PLAN.as_bytes(),
    )
    .expect("the sample runs under the default fuel");

    assert_eq!(found.failures, [], "the host refused nothing");
    let by_code: Vec<(&str, Level, Option<&str>)> = found
        .findings
        .iter()
        .map(|f| (f.code.as_str(), f.level, f.subject.as_deref()))
        .collect();
    assert_eq!(
        by_code,
        [
            ("space-in-path", Level::Error, Some("a b.lua")),
            ("large-file", Level::Warning, Some("big.dat")),
        ]
    );
    assert!(
        found.findings[1].message.contains("2000000"),
        "the size the guest read out of the document is in its message: {}",
        found.findings[1].message
    );
}

#[test]
fn the_sample_emitter_adds_a_catalogue_of_the_items_of_the_plan() {
    let found = extension::emit(
        sample_guest(),
        &reading_reporting_and_adding(),
        Limits::default(),
        PLAN.as_bytes(),
    )
    .expect("the sample runs under the default fuel");

    let [file] = found.files.as_slice() else {
        panic!("one file: {:?}", found.files);
    };
    assert_eq!(file.path, "config/catalogue.json");
    let catalogue: serde_json::Value = serde_json::from_str(&file.text).expect("it is JSON");
    assert_eq!(
        catalogue,
        serde_json::json!({
            "resource": "pack",
            "files": 3,
            "items": [{ "id": "g1", "name": "pack^jbib_000", "digest": "d" }],
        })
    );
}

#[test]
fn the_sample_without_the_grant_that_buys_what_it_calls_does_not_start() {
    let refused = extension::validate(
        sample_guest(),
        &granting(&[Capability::ReadPlan]),
        Limits::default(),
        PLAN.as_bytes(),
    )
    .expect_err("it imports what the other two grants buy");
    assert!(
        matches!(
            &refused,
            HostError::Unresolved {
                needs: Some(Capability::ReportFindings | Capability::AddFiles),
                ..
            }
        ),
        "{refused}"
    );
}
