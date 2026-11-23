//! A bad plugin does not take the editor down, and cannot reach what it was not
//! granted.
//!
//! Guests are WAT here, so what is executed is readable beside the assertion.

use or_io::desc::{FieldRepr, IntWidth, Pack, PackField, PackType, Scalar};
use or_plugin::{
    AcceptsAnything, AlwaysWritable, Capability, Grants, HostError, Limits, NeverWritable, Runtime,
    Status, Subject,
};

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

/// A pack with one type of three fields, so `field_count` has an answer that is
/// not zero or one.
fn pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(PackType {
        name: "Root".into(),
        size: 0x10,
        fields: ["a", "b", "c"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| PackField {
                name: name.into(),
                offset: i * 4,
                repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U32)),
                space: None,
                bits: Vec::new(),
                note: None,
            })
            .collect(),
        selector: None,
    });
    pack
}

const DOCUMENT: &[u8] = &[0u8; 0x10];

fn subject(pack: &Pack) -> Subject<'_> {
    Subject::reading(pack, "Root", DOCUMENT, DOCUMENT.len())
}

/// A guest that reads the field count and drops it, with the memory the answer
/// is written into.
const READS_THE_COUNT: &str = r#"(module
     (import "openrage" "field_count" (func $count (param i32 i32 i32) (result i32)))
     (memory (export "memory") 1)
     (data (i32.const 0) "Root")
     (func (export "go") (drop (call $count (i32.const 0) (i32.const 4) (i32.const 8)))))"#;

/// A guest that tries to change `a`. The encoded value is a `u32` of 7 — tag
/// `0x01` and sixteen little-endian bytes, which is what [`or_plugin::encode`]
/// produces for an integer.
const WRITES_A_FIELD: &str = r#"(module
     (import "openrage" "patch_field"
       (func $patch (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
     (memory (export "memory") 1)
     (data (i32.const 0) "Root")
     (data (i32.const 8) "a")
     (data (i32.const 16) "\01\07\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
     (func (export "go")
       (drop (call $patch (i32.const 0) (i32.const 4) (i32.const 0)
                          (i32.const 8) (i32.const 1)
                          (i32.const 16) (i32.const 17)))))"#;

#[test]
fn a_guest_that_traps_does_not_take_the_host_with_it() {
    let pack = pack();
    let runtime = Runtime::new(Limits::default());

    // `unreachable` is what `panic!` compiles to in WASM.
    let guest = wasm(r#"(module (func (export "go") unreachable))"#);

    let outcome = runtime.run(
        &guest,
        "go",
        subject(&pack),
        Grants::default(),
        AlwaysWritable,
        &AcceptsAnything,
    );

    let error = outcome.expect_err(
        "a guest executing `unreachable` must fail. If this ever succeeds, the trap \
         is being swallowed and a broken plugin looks like a working one.",
    );
    assert!(
        matches!(error, HostError::Guest(_)),
        "a trap is a guest failure, not a host one: {error:?}"
    );
    assert!(
        error.to_string().contains("unreachable"),
        "the reason must reach the user — a plugin that 'failed' with no cause is \
         unreportable: {error}"
    );

    // The host is intact: the same runtime runs a well-behaved guest after.
    let good = wasm(READS_THE_COUNT);
    let after = runtime
        .run(
            &good,
            "go",
            subject(&pack),
            Grants::default(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("the host survived the trap and still runs guests");
    assert!(!after.touched);
}

#[test]
fn a_guest_cannot_crash_the_host_by_any_of_the_usual_means() {
    let pack = pack();
    let runtime = Runtime::new(Limits::default());

    let misbehaviours: Vec<(&str, &str)> = vec![
        (
            "an explicit trap",
            r#"(module (func (export "go") unreachable))"#,
        ),
        (
            "division by zero",
            r#"(module (func (export "go") (drop (i32.div_s (i32.const 1) (i32.const 0)))))"#,
        ),
        (
            "an out-of-bounds store",
            r#"(module (memory 1) (func (export "go") (i32.store (i32.const 1000000) (i32.const 1))))"#,
        ),
        (
            "unbounded recursion",
            r#"(module (func $f (export "go") (call $f)))"#,
        ),
    ];

    for (what, wat) in misbehaviours {
        let error = runtime
            .run(
                &wasm(wat),
                "go",
                subject(&pack),
                Grants::default(),
                AlwaysWritable,
                &AcceptsAnything,
            )
            .expect_err(&format!("{what} must fail rather than succeed"));
        assert!(
            matches!(error, HostError::Guest(_)),
            "{what} must arrive as a guest failure: {error:?}"
        );
        eprintln!("=== {what}: {error} ===");
    }

    let good = wasm(r#"(module (func (export "go")))"#);
    runtime
        .run(
            &good,
            "go",
            subject(&pack),
            Grants::none(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("four misbehaving guests later, the host still runs one");
}

/// Trap isolation does not cover a guest that hangs; fuel does.
#[test]
fn a_guest_that_spins_is_stopped_rather_than_waited_for() {
    let pack = pack();
    let runtime = Runtime::new(Limits {
        fuel: Some(100_000),
        ..Limits::default()
    });

    let spinner = wasm(r#"(module (func (export "go") (loop br 0)))"#);
    let started = std::time::Instant::now();
    let error = runtime
        .run(
            &spinner,
            "go",
            subject(&pack),
            Grants::default(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect_err("an unbounded loop must be stopped");
    let took = started.elapsed();

    eprintln!("\n=== a spinning guest was stopped in {took:?}: {error} ===");
    assert!(
        took < std::time::Duration::from_secs(5),
        "it took {took:?}, which is not a bound — a guest that freezes the editor \
         for five seconds has defeated the purpose even though it did not crash it"
    );
    assert!(
        error.to_string().contains("fuel"),
        "the reason must say it ran out of budget rather than that it failed, \
         because the two need different answers from a user: {error}"
    );
}

/// The guest fails at instantiation, enforced by WebAssembly's import resolution
/// rather than by a check inside a registered function.
#[test]
fn an_ungranted_capability_has_no_function_to_call() {
    let pack = pack();
    let runtime = Runtime::new(Limits::default());

    // A guest that wants to edit, with only the default grant.
    let editor = wasm(WRITES_A_FIELD);

    let error = runtime
        .run(
            &editor,
            "go",
            subject(&pack),
            Grants::default(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect_err("the default grant does not buy `patch_field`");

    match error {
        HostError::Unresolved { import, needs } => {
            assert_eq!(import, "patch_field");
            assert_eq!(
                needs,
                Some(Capability::EditOpenDocument),
                "⚠ the refusal must name the capability, so a host can offer the \
                 grant rather than a stack trace"
            );
        }
        other => panic!(
            "an ungranted capability must be an unresolved import, not {other:?}. If \
             it ever becomes a runtime refusal, the function is being registered \
             regardless of the grant — which is the shape this design exists to \
             avoid."
        ),
    }

    // The same guest, granted and on a document its host can lend, links and runs.
    let granted = Grants::default().granting(Capability::EditOpenDocument);
    let mut bytes = DOCUMENT.to_vec();
    let outcome = runtime
        .run(
            &editor,
            "go",
            Subject::editing(&pack, "Root", &mut bytes, DOCUMENT.len()),
            granted,
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("granted, it links");
    assert!(outcome.touched, "and it did the thing it asked to do");
    assert_eq!(
        u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        7,
        "and the change reached the document"
    );
}

/// Granted, so the import resolves, and refused at the call because the embedder
/// says read-only; the guest can tell which of the two gates refused it.
#[test]
fn a_granted_edit_is_still_refused_through_the_runtime_when_writing_is_not_permitted() {
    let pack = pack();
    let runtime = Runtime::new(Limits::default());
    // The same write, and the guest traps unless the status it reads back is
    // exactly `NotWritable`.
    let editor = wasm(&format!(
        r#"(module
             (import "openrage" "patch_field"
               (func $patch (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (data (i32.const 8) "a")
             (data (i32.const 16) "\01\07\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
             (func (export "go")
               (if (i32.ne
                     (call $patch (i32.const 0) (i32.const 4) (i32.const 0)
                                  (i32.const 8) (i32.const 1)
                                  (i32.const 16) (i32.const 17))
                     (i32.const {}))
                 (then unreachable))))"#,
        Status::NotWritable.code()
    ));
    let granted = Grants::default().granting(Capability::EditOpenDocument);
    let mut bytes = DOCUMENT.to_vec();

    let outcome = runtime
        .run(
            &editor,
            "go",
            Subject::editing(&pack, "Root", &mut bytes, DOCUMENT.len()),
            granted,
            NeverWritable("the editor is in read-only mode".into()),
            &AcceptsAnything,
        )
        .expect("the guest itself does not fail — its request is refused");

    assert!(
        !outcome.touched,
        "⚠ the document must not have been touched. The grant let the import \
         resolve; the write gate refused the call. If this is ever true, a user who \
         set the editor read-only and then installed a plugin is writable again \
         without having said so."
    );
    assert_eq!(
        outcome
            .refused
            .iter()
            .map(|r| r.capability.key())
            .collect::<Vec<_>>(),
        vec!["cap.edit_open_document"],
        "and the refusal is recorded by capability key, so a plugin that silently \
         does nothing is diagnosable"
    );
    assert_eq!(
        outcome
            .failures
            .iter()
            .map(|f| (f.function, f.status))
            .collect::<Vec<_>>(),
        vec![("patch_field", Status::NotWritable)],
        "⚠ and it is `NotWritable` rather than `NotGranted`. The two need different \
         answers from a user — grant it, against leave read-only mode — and both \
         were `-1` before the ABI had statuses."
    );
    assert_eq!(bytes, DOCUMENT, "and not a byte of the document moved");
}

#[test]
fn an_import_from_another_namespace_is_refused_rather_than_guessed_at() {
    let pack = pack();
    let runtime = Runtime::new(Limits::default());

    let sneaky = wasm(
        r#"(module
             (import "wasi_snapshot_preview1" "fd_write" (func $w (result i32)))
             (func (export "go") (drop (call $w))))"#,
    );

    let error = runtime
        .run(
            &sneaky,
            "go",
            subject(&pack),
            Grants::trusted_with_everything(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect_err("⚠ even every capability does not buy WASI");

    let said = error.to_string();
    assert!(
        said.contains("wasi_snapshot_preview1"),
        "the refusal must name what was asked for: {said}"
    );
    assert!(
        matches!(error, HostError::Unresolved { .. }),
        "and it is an unresolved import, because this host provides one namespace \
         and no WASI at all — a guest cannot open a file or a socket because there \
         is nothing to call: {error:?}"
    );
}

#[test]
fn the_reading_function_answers_from_the_pack_rather_than_a_constant() {
    let pack = pack();
    let runtime = Runtime::new(Limits::default());

    // Traps unless the call succeeded and the count it wrote is exactly three, so
    // the assertion is inside the guest.
    let checker = wasm(
        r#"(module
             (import "openrage" "field_count" (func $count (param i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (func (export "go")
               (if (i32.ne (call $count (i32.const 0) (i32.const 4) (i32.const 8)) (i32.const 0))
                 (then unreachable))
               (if (i32.ne (i32.load (i32.const 8)) (i32.const 3)) (then unreachable))))"#,
    );

    runtime
        .run(
            &checker,
            "go",
            subject(&pack),
            Grants::default(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("the guest saw three fields, which is what the pack describes");

    // A pack with a different shape gives a different answer, so the guest traps.
    let mut other = Pack::new("probe", "gen8");
    other.types.push(PackType {
        name: "Root".into(),
        size: 4,
        fields: vec![PackField {
            name: "only".into(),
            offset: 0,
            repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U32)),
            space: None,
            bits: Vec::new(),
            note: None,
        }],
        selector: None,
    });
    runtime
        .run(
            &checker,
            "go",
            subject(&other),
            Grants::default(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect_err(
            "one field is not three, so the guest trapped — the host is \
                     answering from the pack it was given",
        );
}

/// A guest cannot receive a `&GtaDrawable`, so `or-plugin` names `or-io` and
/// nothing that knows what a drawable is.
#[test]
fn the_runtime_did_not_teach_this_crate_what_a_drawable_is() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("Cargo.toml");

    let dependencies: Vec<&str> = manifest
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect();
    for forbidden in ["or-title", "or-renderer", "or-editor"] {
        assert!(
            !dependencies.iter().any(|line| line.contains(forbidden)),
            "`or-plugin` has taken a dependency on `{forbidden}`. The host's \
             vocabulary is the pack's because a WASM guest cannot take a Rust type \
             across the boundary — the reference case declares \
             `measure(drawable: &GtaDrawable)`, which cannot cross at all. Adding \
             this makes the plugin surface depend on compiled layouts again."
        );
    }
    assert!(
        dependencies
            .iter()
            .any(|line| line.trim_start().starts_with("or-io.")),
        "and it does still name the pack contract, or it has nothing to describe"
    );
}
