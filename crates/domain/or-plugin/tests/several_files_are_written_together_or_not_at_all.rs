//! `write_files` keeps several files of the project as one set: every file or none of them, one call
//! to the embedder's check for the whole set, and a sentence for the guest that says nothing was
//! written. The guests are WAT written beside the assertion. The host never writes a file: what it
//! keeps is handed to the embedder as `Outcome::transaction`, to apply as one step.

use std::cell::RefCell;

use or_io::desc::Pack;
use or_plugin::{
    AcceptsAnything, AlwaysWritable, Call, Capability, CommandSpec, FileWrite, Grants, HostError,
    Limits, NeverWritable, Runtime, SaveCheck, Session, Status, Subject, Transaction,
    WouldBeInvalid,
};

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

/// `bytes` as a WAT data segment at `at`.
fn data(at: usize, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|b| format!("\\{b:02x}")).collect();
    format!("(data (i32.const {at}) \"{escaped}\")")
}

fn writing() -> Grants {
    Grants::none().granting(Capability::WriteFiles)
}

fn set(files: &[(&str, &[u8])]) -> Transaction {
    Transaction {
        files: files
            .iter()
            .map(|(path, bytes)| FileWrite {
                path: (*path).into(),
                bytes: bytes.to_vec(),
            })
            .collect(),
    }
}

const IMPORT: &str = r#"(import "openrage" "write_files" (func $write_files (param i32 i32 i32 i32 i32) (result i32)))"#;

/// A guest whose `go` calls `write_files` once with `set`, and the sentence slot at 8192.
fn writer(set: &Transaction, calls: usize) -> Vec<u8> {
    let bytes = set.encode();
    let call = format!(
        "(drop (call $write_files (i32.const 0) (i32.const {}) (i32.const 8192) (i32.const 512) (i32.const 8704)))",
        bytes.len()
    );
    wasm(&format!(
        r#"(module {IMPORT}
  (memory (export "memory") 2)
  {data}
  (func (export "go") {calls}))"#,
        data = data(0, &bytes),
        calls = call.repeat(calls)
    ))
}

fn pack() -> Pack {
    Pack::new("probe", "gen8")
}

fn run(
    guest: &[u8],
    grants: Grants,
    gate: impl or_plugin::WriteGate,
    check: &dyn SaveCheck,
) -> Result<or_plugin::Outcome, HostError> {
    let pack = pack();
    Runtime::new(Limits::default()).run(
        guest,
        "go",
        Subject::describing(&pack, "Root"),
        grants,
        gate,
        check,
    )
}

/// An embedder that refuses a set holding a file whose bytes are `bad`, and says which, remembering every set it was asked about.
struct RefusesBad {
    asked: RefCell<Vec<Vec<String>>>,
}

impl RefusesBad {
    fn new() -> Self {
        Self {
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl SaveCheck for RefusesBad {
    fn why_the_document_would_be_invalid(&self, _document: &[u8]) -> Option<String> {
        None
    }

    fn why_the_file_would_be_invalid(&self, _file: &[u8]) -> Option<String> {
        None
    }

    fn why_the_files_would_be_invalid(&self, files: &[(&str, &[u8])]) -> Option<String> {
        self.asked
            .borrow_mut()
            .push(files.iter().map(|(path, _)| (*path).to_string()).collect());
        files
            .iter()
            .find(|(_, bytes)| *bytes == b"bad")
            .map(|(path, _)| format!("`{path}` does not read back"))
    }
}

#[test]
fn three_files_that_all_pass_are_kept_together_in_the_order_they_were_written() {
    let three = set(&[
        ("a/one.txt", b"1"),
        ("b/two.json", b"{}"),
        ("c.bin", &[0, 1, 2]),
    ]);
    let check = RefusesBad::new();

    let outcome = run(&writer(&three, 1), writing(), AlwaysWritable, &check).expect("it runs");

    assert_eq!(outcome.transaction, Some(three));
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(
        check.asked.borrow().as_slice(),
        [["a/one.txt", "b/two.json", "c.bin"]],
        "the embedder is asked once, about the whole set"
    );
}

#[test]
fn three_files_with_the_third_failing_write_none_and_the_guest_is_told_so() {
    let three = set(&[("a.txt", b"1"), ("b.txt", b"2"), ("c.txt", b"bad")]);
    let check = RefusesBad::new();

    let outcome = run(&writer(&three, 1), writing(), AlwaysWritable, &check).expect("it runs");

    assert_eq!(
        outcome.transaction, None,
        "a refused set keeps none of its files"
    );
    let [failure] = outcome.failures.as_slice() else {
        panic!("one refusal expected: {:?}", outcome.failures);
    };
    assert_eq!(
        (failure.function, failure.status),
        ("write_files", Status::Refused)
    );
    let said = failure.because.as_deref().expect("a sentence");
    assert!(
        said.contains("no file was written") && said.contains("`c.txt` does not read back"),
        "{said}"
    );
}

#[test]
fn the_sentence_reaches_the_guest_in_the_slot_it_gave() {
    let three = set(&[("a.txt", b"1"), ("c.txt", b"bad")]);
    let bytes = three.encode();
    // After the call, the guest traps unless the slot holds a sentence starting with "no file was written".
    let guest = wasm(&format!(
        r#"(module {IMPORT}
  (memory (export "memory") 2)
  {set}
  (func (export "go")
    (drop (call $write_files (i32.const 0) (i32.const {len}) (i32.const 8192) (i32.const 512) (i32.const 8704)))
    (if (i32.eqz (i32.load (i32.const 8704))) (then unreachable))
    (if (i32.ne (i32.load8_u (i32.const 8192)) (i32.const 110)) (then unreachable))))"#,
        set = data(0, &bytes),
        len = bytes.len()
    ));

    run(&guest, writing(), AlwaysWritable, &RefusesBad::new())
        .expect("the guest found the sentence in its slot");
}

#[test]
fn a_host_that_has_not_said_how_it_checks_files_together_keeps_none() {
    struct Silent;
    impl SaveCheck for Silent {
        fn why_the_document_would_be_invalid(&self, _: &[u8]) -> Option<String> {
            None
        }
        fn why_the_file_would_be_invalid(&self, _: &[u8]) -> Option<String> {
            None
        }
    }

    let outcome = run(
        &writer(&set(&[("a.txt", b"1")]), 1),
        writing(),
        AlwaysWritable,
        &Silent,
    )
    .expect("it runs");

    assert_eq!(outcome.transaction, None);
    assert!(
        outcome.failures[0]
            .because
            .as_deref()
            .unwrap()
            .contains("does not accept files written together"),
        "{:?}",
        outcome.failures
    );
}

#[test]
fn the_edit_mode_gate_refuses_a_granted_write_before_the_set_is_read() {
    let check = RefusesBad::new();

    let outcome = run(
        &writer(&set(&[("a.txt", b"1")]), 1),
        writing(),
        NeverWritable("the project is read only".into()),
        &check,
    )
    .expect("it runs");

    assert_eq!(outcome.transaction, None);
    assert_eq!(outcome.failures[0].status, Status::NotWritable);
    assert!(
        check.asked.borrow().is_empty(),
        "the embedder was asked about a write the gate refused"
    );
    assert_eq!(outcome.refused.len(), 1);
}

#[test]
fn without_the_capability_the_function_is_not_linked() {
    let refused = run(
        &writer(&set(&[("a.txt", b"1")]), 1),
        Grants::default(),
        AlwaysWritable,
        &AcceptsAnything,
    )
    .expect_err("an ungranted capability is an unresolved import");

    match refused {
        HostError::Unresolved { import, needs } => {
            assert_eq!(import, "write_files");
            assert_eq!(needs, Some(Capability::WriteFiles));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_path_that_leaves_the_project_or_names_a_file_twice_keeps_nothing() {
    for bad in [
        set(&[("ok.txt", b"1"), ("../outside.txt", b"2")]),
        set(&[("/rooted.txt", b"1")]),
        set(&[("C:/drive.txt", b"1")]),
        set(&[("a/B.txt", b"1"), ("A/b.txt", b"2")]),
        set(&[]),
    ] {
        let outcome = run(
            &writer(&bad, 1),
            writing(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("it runs");
        assert_eq!(outcome.transaction, None, "{bad:?}");
        assert_eq!(outcome.failures[0].status, Status::Refused, "{bad:?}");
    }
}

#[test]
fn bytes_that_are_not_the_encoding_are_the_guests_bug() {
    let guest = wasm(&format!(
        r#"(module {IMPORT}
  (memory (export "memory") 1)
  {junk}
  (func (export "go")
    (drop (call $write_files (i32.const 0) (i32.const 5) (i32.const 64) (i32.const 64) (i32.const 200)))))"#,
        junk = data(0, &[9, 0, 0, 0, 1])
    ));

    let outcome = run(&guest, writing(), AlwaysWritable, &AcceptsAnything).expect("it runs");

    assert_eq!(outcome.transaction, None);
    assert_eq!(outcome.failures[0].status, Status::BadArgument);
}

#[test]
fn a_run_writes_its_files_in_one_call_and_a_second_is_refused_leaving_the_first() {
    let first = set(&[("a.txt", b"1")]);

    let outcome = run(
        &writer(&first, 2),
        writing(),
        AlwaysWritable,
        &AcceptsAnything,
    )
    .expect("it runs");

    assert_eq!(outcome.transaction, Some(first));
    assert_eq!(outcome.failures.len(), 1);
    assert_eq!(outcome.failures[0].status, Status::Refused);
}

#[test]
fn a_run_that_traps_after_the_set_was_kept_hands_the_embedder_nothing() {
    let bytes = set(&[("a.txt", b"1")]).encode();
    let guest = wasm(&format!(
        r#"(module {IMPORT}
  (memory (export "memory") 2)
  {set}
  (func (export "go")
    (drop (call $write_files (i32.const 0) (i32.const {len}) (i32.const 8192) (i32.const 64) (i32.const 8704)))
    unreachable))"#,
        set = data(0, &bytes),
        len = bytes.len()
    ));

    let ran = run(&guest, writing(), AlwaysWritable, &AcceptsAnything);

    assert!(
        matches!(ran, Err(HostError::Guest(_))),
        "a trap returns no outcome, so no file is applied: {ran:?}"
    );
}

/// A guest that stays loaded: one command, which writes `set` when it runs.
fn commander(set: &Transaction) -> Vec<u8> {
    let command = CommandSpec {
        id: "paint".into(),
        label: "Paint".into(),
        shortcut: None,
    }
    .encode();
    let bytes = set.encode();
    wasm(&format!(
        r#"(module
  (import "openrage" "register_command" (func $register_command (param i32 i32) (result i32)))
  {IMPORT}
  (memory (export "memory") 2)
  {command_data}
  {set_data}
  (func (export "register")
    (drop (call $register_command (i32.const 0) (i32.const {command_len}))))
  (func (export "draw_panel") (param i32))
  (func (export "run_command") (param i32)
    (drop (call $write_files (i32.const 1024) (i32.const {len}) (i32.const 8192) (i32.const 512) (i32.const 8704)))))"#,
        command_data = data(0, &command),
        command_len = command.len(),
        set_data = data(1024, &bytes),
        len = bytes.len()
    ))
}

fn session_grants() -> Grants {
    writing().granting(Capability::AddCommands)
}

#[test]
fn a_guest_that_stays_loaded_writes_its_files_together_from_a_command() {
    let three = set(&[("a.txt", b"1"), ("b.txt", b"2"), ("c.txt", b"3")]);
    let mut session = Session::open(&commander(&three), session_grants(), Limits::default())
        .expect("it registers");

    let outcome = session
        .run_command(0, Call::new(AlwaysWritable, AcceptsAnything))
        .expect("the command runs");
    assert_eq!(outcome.transaction, Some(three));

    let refused = session
        .run_command(0, Call::new(AlwaysWritable, WouldBeInvalid("nope".into())))
        .expect("the command runs");
    assert_eq!(refused.transaction, None, "the check refused the whole set");
    assert_eq!(refused.failures[0].status, Status::Refused);

    let read_only = session
        .run_command(
            0,
            Call::new(NeverWritable("read only".into()), AcceptsAnything),
        )
        .expect("the command runs");
    assert_eq!(read_only.transaction, None);
    assert_eq!(read_only.failures[0].status, Status::NotWritable);

    let again = session
        .run_command(0, Call::new(AlwaysWritable, AcceptsAnything))
        .expect("the command runs again");
    assert!(
        again.transaction.is_some(),
        "what one call kept is not left over for the next"
    );
}
