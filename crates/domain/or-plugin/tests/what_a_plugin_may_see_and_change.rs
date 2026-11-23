//! What a plugin can read, what it can change, and the gates a change passes
//! before it is kept.
//!
//! ⚠ A reader that tolerates what the engine does not lets a file read back
//! identically here while the game misreads it, and a plugin's author has never
//! read the format. So a patch passes the grant, the application's edit mode
//! and the pack's structural refusal, then the format's own save check on
//! the patched bytes, and a refusal at any of them
//! leaves the document byte-identical.

use std::cell::Cell;

use or_io::desc::value::ScalarValue;
use or_io::desc::{
    CountSource, FieldRepr, Image, Pack, PackField, PackType, Pointee, Scalar, Value,
};
use or_plugin::{
    host, AcceptsAnything, Capability, Grants, Limits, NeverWritable, Runtime, SaveCheck, Status,
    Subject, WouldBeInvalid,
};
use or_plugin::{AlwaysWritable, Document};

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

fn field(name: &str, offset: usize, repr: FieldRepr) -> PackField {
    PackField {
        name: name.into(),
        offset,
        repr,
        space: None,
        bits: Vec::new(),
        note: None,
    }
}

/// A root with a counted run behind a pointer, a null child, and scalars.
fn pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(PackType {
        name: "Root".into(),
        size: 0x20,
        fields: vec![
            field(
                "count",
                0x00,
                FieldRepr::Scalar(Scalar::Int(or_io::desc::IntWidth::U32)),
            ),
            field("ratio", 0x04, FieldRepr::Scalar(Scalar::F32)),
            field("skinned", 0x08, FieldRepr::Scalar(Scalar::Bool)),
            field(
                "items",
                0x10,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Item".into()),
                    count: CountSource::Field("count".into()),
                    element_size: Some(8),
                }),
            ),
            field(
                "child",
                0x18,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Item".into(),
                }),
            ),
        ],
        selector: None,
    });
    pack.types.push(PackType {
        name: "Item".into(),
        size: 8,
        fields: vec![
            field(
                "id",
                0x00,
                FieldRepr::Scalar(Scalar::Int(or_io::desc::IntWidth::U32)),
            ),
            field("weight", 0x04, FieldRepr::Scalar(Scalar::F32)),
        ],
        selector: None,
    });
    pack
}

const ITEMS_AT: usize = 0x20;
const VIRTUAL_SIZE: usize = 0x40;

/// Three items at `ITEMS_AT`, a null `child`, `ratio` 2.5.
fn document() -> Vec<u8> {
    let mut bytes = vec![0u8; VIRTUAL_SIZE];
    bytes[0x00..0x04].copy_from_slice(&3u32.to_le_bytes());
    bytes[0x04..0x08].copy_from_slice(&2.5f32.to_le_bytes());
    bytes[0x08] = 1;
    bytes[0x10..0x18].copy_from_slice(&(0x5000_0000u64 + ITEMS_AT as u64).to_le_bytes());
    for index in 0..3u32 {
        let at = ITEMS_AT + index as usize * 8;
        bytes[at..at + 4].copy_from_slice(&(100 + index).to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&(index as f32).to_le_bytes());
    }
    bytes
}

/// Not one call names a compiled Rust type: the arguments are a pack, a type
/// name, some bytes and a field name.
#[test]
fn a_plugin_reads_a_described_field_by_name() {
    let pack = pack();
    let bytes = document();
    let image = Image::new(&bytes, VIRTUAL_SIZE);

    assert_eq!(
        host::field_value(&pack, image, host::Site::new("Root", 0, "ratio"))
            .expect("a described scalar"),
        Value::f32(2.5)
    );
    assert_eq!(
        host::field_value(&pack, image, host::Site::new("Root", 0, "skinned"))
            .expect("a described bool"),
        Value::boolean(true)
    );
    assert_eq!(
        host::field_name(&pack, "Root", 1).expect("a described name"),
        "ratio"
    );
    assert_eq!(host::field_count(&pack, "Root").expect("described"), 5);
    assert_eq!(host::type_size(&pack, "Item").expect("described"), 8);
}

/// One hop at a time — `follow` to the run, `element_count` for how many,
/// `type_size` for the stride — and the guest indexes.
#[test]
fn a_guest_walks_a_run_one_hop_at_a_time() {
    let pack = pack();
    let bytes = document();
    let image = Image::new(&bytes, VIRTUAL_SIZE);

    let at =
        host::follow(&pack, image, host::Site::new("Root", 0, "items")).expect("the run resolves");
    assert_eq!(at, ITEMS_AT);
    let count = host::element_count(&pack, image, host::Site::new("Root", 0, "items"))
        .expect("a counted run");
    assert_eq!(count, 3, "the count comes from the sibling the pack names");
    let stride = host::type_size(&pack, "Item").expect("described");

    let ids: Vec<i128> = (0..count)
        .map(|index| {
            host::field_value(
                &pack,
                image,
                host::Site::new("Item", at + index * stride, "id"),
            )
            .expect("an element")
            .as_int()
            .expect("an integer")
        })
        .collect();
    assert_eq!(ids, vec![100, 101, 102]);
}

/// A guest that could not tell a null child from "that field is not a pointer"
/// would treat absence as a fault or a fault as absence.
#[test]
fn a_null_child_is_absent_rather_than_refused() {
    let pack = pack();
    let bytes = document();
    let image = Image::new(&bytes, VIRTUAL_SIZE);

    let denied =
        host::follow(&pack, image, host::Site::new("Root", 0, "child")).expect_err("it is null");
    assert_eq!(denied.status, Status::NullPointer);

    let not_a_pointer =
        host::follow(&pack, image, host::Site::new("Root", 0, "ratio")).expect_err("not a pointer");
    assert_eq!(not_a_pointer.status, Status::Refused);
    assert!(
        not_a_pointer.because.contains("scalar"),
        "the refusal says what it is instead: {not_a_pointer}"
    );
}

/// A check that counts how many times it was asked, and what it was shown.
#[derive(Default)]
struct Watching {
    asked: Cell<usize>,
    saw_ratio: Cell<f32>,
}

impl SaveCheck for Watching {
    fn why_the_document_would_be_invalid(&self, document: &[u8]) -> Option<String> {
        self.asked.set(self.asked.get() + 1);
        self.saw_ratio.set(f32::from_le_bytes(
            document[0x04..0x08].try_into().expect("four"),
        ));
        None
    }

    fn why_the_file_would_be_invalid(&self, _file: &[u8]) -> Option<String> {
        None
    }
}

#[test]
fn the_save_check_is_asked_about_the_patched_document() {
    let pack = pack();
    let mut bytes = document();
    let watching = Watching::default();

    host::patch_field(
        &pack,
        &mut bytes,
        VIRTUAL_SIZE,
        host::Site::new("Root", 0, "ratio"),
        ScalarValue::F32(9.75),
        &watching,
    )
    .expect("a scalar patch the check accepted");

    assert_eq!(watching.asked.get(), 1, "asked exactly once");
    assert_eq!(
        watching.saw_ratio.get(),
        9.75,
        "⚠ it saw the patched value. A check shown the document as it was could \
         not refuse anything about the edit."
    );
    assert_eq!(
        f32::from_le_bytes(bytes[0x04..0x08].try_into().unwrap()),
        9.75,
        "and the patch was kept"
    );
}

#[test]
fn a_refused_patch_leaves_the_document_byte_identical() {
    let pack = pack();
    let before = document();
    let mut bytes = before.clone();

    let denied = host::patch_field(
        &pack,
        &mut bytes,
        VIRTUAL_SIZE,
        host::Site::new("Root", 0, "ratio"),
        ScalarValue::F32(9.75),
        &WouldBeInvalid("a switch distance must not exceed the one before it".into()),
    )
    .expect_err("the check refused");

    assert_eq!(denied.status, Status::Refused);
    assert!(
        denied.because.contains("must not exceed"),
        "the format's own sentence reaches the user: {denied}"
    );
    assert_eq!(
        bytes, before,
        "⚠ every byte. The patch is applied so the check has something to judge \
         and undone when it refuses, and 'undone' has to mean identical — a \
         refusal that left four bytes changed is a write nobody consented to."
    );
}

/// Asking the format first would mean a structural edit reached the bytes for
/// as long as it took to be undone.
#[test]
fn a_structural_field_is_refused_without_the_check_being_asked() {
    let pack = pack();
    let before = document();
    let mut bytes = before.clone();
    let watching = Watching::default();

    let denied = host::patch_field(
        &pack,
        &mut bytes,
        VIRTUAL_SIZE,
        host::Site::new("Root", 0, "count"),
        ScalarValue::Int(99),
        &watching,
    )
    .expect_err("`count` is what `items` takes its length from");

    assert_eq!(denied.status, Status::Refused);
    assert!(
        denied.because.contains("structural"),
        "and it is the pack's own wording rather than a second one: {denied}"
    );
    assert_eq!(
        watching.asked.get(),
        0,
        "⚠ the format was never asked, because the description already knew"
    );
    assert_eq!(bytes, before, "and nothing was written");
}

#[test]
fn a_guest_patches_a_scalar_by_name_and_reads_it_back() {
    let pack = pack();
    let mut bytes = document();

    // "Root" at 0, "ratio" at 8, an encoded f32 at 16, scratch from 32.
    let mut encoded = or_plugin::encode(&Value::f32(9.75)).expect("it fits");
    assert_eq!(encoded.len(), 5);
    let value_bytes: String = encoded
        .drain(..)
        .map(|b| format!("\\{b:02x}"))
        .collect::<Vec<_>>()
        .join("");

    let guest = wasm(&format!(
        r#"(module
             (import "openrage" "patch_field"
               (func $patch (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
             (import "openrage" "field_value"
               (func $value (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (data (i32.const 8) "ratio")
             (data (i32.const 16) "{value_bytes}")
             (func (export "go")
               (if (i32.ne
                     (call $patch (i32.const 0) (i32.const 4) (i32.const 0)
                                  (i32.const 8) (i32.const 5)
                                  (i32.const 16) (i32.const 5))
                     (i32.const {ok}))
                 (then unreachable))
               (if (i32.ne
                     (call $value (i32.const 0) (i32.const 4) (i32.const 0)
                                  (i32.const 8) (i32.const 5)
                                  (i32.const 64) (i32.const 32) (i32.const 40))
                     (i32.const {ok}))
                 (then unreachable))
               (if (f32.ne (f32.load (i32.const 65)) (f32.const 9.75)) (then unreachable))))"#,
        ok = Status::Ok.code(),
    ));

    let outcome = {
        let subject = Subject::editing(&pack, "Root", &mut bytes, VIRTUAL_SIZE);
        Runtime::new(Limits::default())
            .run(
                &guest,
                "go",
                subject,
                Grants::default().granting(Capability::EditOpenDocument),
                AlwaysWritable,
                &AcceptsAnything,
            )
            .expect("the guest wrote a field and read back what it wrote")
    };
    assert!(outcome.touched);
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(
        f32::from_le_bytes(bytes[0x04..0x08].try_into().unwrap()),
        9.75,
        "and the document really changed"
    );
}

/// Not granted is an unresolved import; a read-only application and a document
/// the host only shares both answer `NotWritable`, told apart by the reason.
#[test]
fn each_reason_a_patch_is_refused_is_its_own_status() {
    let pack = pack();
    let value = or_plugin::encode(&Value::f32(1.0)).expect("it fits");
    let value_bytes: String = value
        .iter()
        .map(|b| format!("\\{b:02x}"))
        .collect::<Vec<_>>()
        .join("");
    let guest = wasm(&format!(
        r#"(module
             (import "openrage" "patch_field"
               (func $patch (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (data (i32.const 8) "ratio")
             (data (i32.const 16) "{value_bytes}")
             (func (export "go")
               (drop (call $patch (i32.const 0) (i32.const 4) (i32.const 0)
                                  (i32.const 8) (i32.const 5)
                                  (i32.const 16) (i32.const 5)))))"#
    ));
    let runtime = Runtime::new(Limits::default());
    let granted = Grants::default().granting(Capability::EditOpenDocument);

    let mut bytes = document();
    let refused_by_the_gate = runtime
        .run(
            &guest,
            "go",
            Subject::editing(&pack, "Root", &mut bytes, VIRTUAL_SIZE),
            granted.clone(),
            NeverWritable("the editor is in read-only mode".into()),
            &AcceptsAnything,
        )
        .expect("the guest is refused, not broken");
    assert_eq!(
        refused_by_the_gate.failures[0].status,
        Status::NotWritable,
        "the application's edit mode"
    );
    assert!(!refused_by_the_gate.touched);
    assert_eq!(bytes, document(), "and nothing was written");

    // A document the host only shares: granted, gate open, and still refused.
    let shared = document();
    let refused_by_the_document = runtime
        .run(
            &guest,
            "go",
            Subject::reading(&pack, "Root", &shared, VIRTUAL_SIZE),
            granted,
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("refused, not broken");
    assert_eq!(
        refused_by_the_document.failures[0].status,
        Status::NotWritable
    );
    assert!(refused_by_the_document.failures[0]
        .because
        .as_deref()
        .is_some_and(|why| why.contains("read-only")));

    // Not granted at all is not even an importable function.
    let error = runtime
        .run(
            &guest,
            "go",
            Subject::reading(&pack, "Root", &shared, VIRTUAL_SIZE),
            Grants::default(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect_err("the default grant does not buy a write");
    assert!(matches!(
        error,
        or_plugin::HostError::Unresolved { ref import, .. } if import == "patch_field"
    ));
}

/// `AcceptsAnything` throws the format's own validation away, so only tests may
/// use it.
#[test]
fn nothing_in_the_tree_accepts_any_document() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-plugin");

    let mut callers = Vec::new();
    for dir in ["src", "crates"] {
        walk(&root.join(dir), &mut |path| {
            if path.components().any(|c| c.as_os_str() == "tests") {
                return;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                return;
            };
            for (line_number, line) in text.lines().enumerate() {
                let trimmed = line.trim_start();
                // ⚠ Naming it is not using it: a comment explaining why it
                // exists, a re-export, and the declaration itself all say the
                // word. What this is looking for is a call site.
                if trimmed.starts_with("//")
                    || trimmed.starts_with("use ")
                    || trimmed.starts_with("pub use ")
                    || trimmed.starts_with("pub struct AcceptsAnything")
                    || trimmed.starts_with("impl SaveCheck for AcceptsAnything")
                {
                    continue;
                }
                if line.contains("AcceptsAnything") {
                    callers.push(format!("{}:{}", path.display(), line_number + 1));
                }
            }
        });
    }

    assert!(
        callers.is_empty(),
        "these use `AcceptsAnything` outside a test: {callers:?}\n\n\
         It exists for a tool with no format knowledge to apply. The application \
         has the format's own save check, and a plugin writing through this instead is \
         the corruption surface the whole gate exists to prevent."
    );
}

/// A whole-document read would build a tree proportional to the file, in the
/// host's memory, at a guest's request.
#[test]
fn the_host_reads_one_field_and_never_a_document() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in ["host.rs", "runtime.rs"] {
        let text = std::fs::read_to_string(src.join(file)).expect("a module of this crate");
        // Code only: a scan that could not tell an explanation from a call would
        // forbid a comment saying why the call is absent.
        let code: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["read_value", "or_io::desc::read", "value::read"] {
            assert!(
                !code.contains(forbidden),
                "`src/{file}` names `{forbidden}`. A whole-document read is a host \
                 allocation proportional to the file that a guest can ask for in \
                 one call; the surface is one hop at a time so that it cannot."
            );
        }
    }
}

fn walk(dir: &std::path::Path, visit: &mut impl FnMut(&std::path::Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk(&path, visit);
        } else if path.extension().is_some_and(|e| e == "rs") {
            visit(&path);
        }
    }
}

/// The `Document` split is a type, not a flag.
#[test]
fn a_document_a_host_only_shares_cannot_be_lent_for_writing() {
    let bytes = document();
    let shared = Document::ReadOnly(&bytes);
    assert_eq!(shared.bytes().len(), VIRTUAL_SIZE);
}
