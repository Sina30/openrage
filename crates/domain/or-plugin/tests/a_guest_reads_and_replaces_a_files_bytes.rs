//! A guest is handed a file's bytes and hands bytes back, through the grants
//! and the embedder's checks, and `or-io` linked into a guest parses them.
//!
//! ⚠ The sample guest is built with its `bytes` feature by a nested `cargo
//! build` for wasm32-unknown-unknown into this target's scratch directory, so
//! that target must be installed and `or-io` must build for it; a failure to
//! build fails the suite rather than skipping it.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use or_io::container::resource::{Compression, ResourceEnvelope, Rsc7};
use or_io::decl::{File as _, Gen8};
use or_io::desc::Pack;
use or_io::gta5::{GtaDrawable, PgDictionary};
use or_plugin::{
    AcceptsAnything, AlwaysWritable, Because, Capability, Grants, HostError, Limits, NeverWritable,
    Outcome, Refused, Runtime, SaveCheck, Status, Subject,
};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const WASM32: &str = "wasm32-unknown-unknown";

/// A capability as a manifest names it, by its stable key.
fn capability(key: &str) -> Capability {
    Capability::ALL
        .into_iter()
        .find(|c| c.key() == key)
        .unwrap_or_else(|| {
            panic!("no capability is keyed `{key}`: the host cannot hand a guest a file's bytes")
        })
}

fn reading() -> Grants {
    Grants::default().granting(capability("cap.read_file_bytes"))
}

fn reading_and_writing() -> Grants {
    reading().granting(capability("cap.write_file_bytes"))
}

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

/// The sample guest with `or-io` linked in, whose entry round-trips a `.ydd`.
fn sample_guest() -> &'static [u8] {
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    BUILT.get_or_init(|| {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugins-bytes");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&plugins)
            .args([
                "build",
                "--release",
                "-p",
                "or-plugin-sample",
                "--features",
                "bytes",
                "--target",
                WASM32,
            ])
            .arg("--target-dir")
            .arg(&target_dir)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "the sample guest did not build for {WASM32} with `bytes`; if the target \
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

/// A Legacy `.ydd` of one drawable whose first level-of-detail distance is `first`.
fn authored_ydd(first: f32) -> Vec<u8> {
    let mut drawable = GtaDrawable::<Gen8>::default();
    drawable.base.lod_group.lod_distances = [first, 200.0, 400.0, 800.0];
    PgDictionary::<GtaDrawable<Gen8>> {
        codes: vec![0x1234],
        entries: vec![Some(Box::new(drawable))],
        ..Default::default()
    }
    .to_bytes()
    .expect("a dictionary of one drawable saves")
}

fn first_distances(ydd: &[u8]) -> Vec<f32> {
    PgDictionary::<GtaDrawable<Gen8>>::from_bytes(ydd)
        .expect("the bytes are a Legacy .ydd")
        .entries
        .iter()
        .flatten()
        .map(|drawable| drawable.base.lod_group.lod_distances[0])
        .collect()
}

/// What a guest parsing with `or-io` needs: it builds the format's description
/// before it reads a byte, so even a drawable dictionary of one default
/// drawable spends more than `Limits::default()` allows
/// (`what_a_guest_parsing_a_ydd_spends_against_the_default_fuel`).
fn parsing() -> Limits {
    Limits {
        fuel: Some(1_000_000_000),
        ..Limits::default()
    }
}

fn no_pack() -> Pack {
    Pack::new("probe", "gen8")
}

/// What the host would show a user for a write its gate refused.
fn the_gates_sentence(why: &str) -> String {
    Refused {
        capability: Capability::WriteFileBytes,
        because: Because::EmbedderRefusesWrites(why.into()),
    }
    .to_string()
}

/// Escapes `text` for a WAT string.
fn wat_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `$same (a b len)`: whether two runs of guest memory hold the same bytes.
const SAME: &str = r#"
  (func $same (param $a i32) (param $b i32) (param $len i32) (result i32)
    (local $i i32)
    (block $out
      (loop $next
        (br_if $out (i32.ge_u (local.get $i) (local.get $len)))
        (if (i32.ne (i32.load8_u (i32.add (local.get $a) (local.get $i)))
                    (i32.load8_u (i32.add (local.get $b) (local.get $i))))
          (then (return (i32.const 0))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (i32.const 1))
"#;

const WRITE_FILE: &str = r#"(import "openrage" "write_file" (func $write_file (param i32 i32 i32 i32 i32) (result i32)))"#;
const READ_FILE: &str =
    r#"(import "openrage" "read_file" (func $read_file (param i32 i32 i32) (result i32)))"#;
const FILE_SIZE: &str =
    r#"(import "openrage" "file_size" (func $file_size (param i32) (result i32)))"#;

/// A check that notes every file it was shown and keeps none of what it refuses.
#[derive(Default)]
struct Watching {
    files: RefCell<Vec<Vec<u8>>>,
    refuses: Option<String>,
    documents: Cell<usize>,
}

impl SaveCheck for Watching {
    fn why_the_document_would_be_invalid(&self, _document: &[u8]) -> Option<String> {
        self.documents.set(self.documents.get() + 1);
        None
    }

    fn why_the_file_would_be_invalid(&self, file: &[u8]) -> Option<String> {
        self.files.borrow_mut().push(file.to_vec());
        self.refuses.clone()
    }
}

#[test]
fn a_guest_reads_a_range_of_the_files_bytes_into_its_own_memory() {
    let file = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let guest = wasm(&format!(
        r#"(module
  {FILE_SIZE}
  {READ_FILE}
  (memory (export "memory") 1)
  (func (export "go")
    (if (i32.ne (call $file_size (i32.const 0)) (i32.const 0)) (then unreachable))
    (if (i32.ne (i32.load (i32.const 0)) (i32.const 8)) (then unreachable))
    ;; bytes 2..6, and then all eight
    (if (i32.ne (call $read_file (i32.const 2) (i32.const 16) (i32.const 4)) (i32.const 0))
      (then unreachable))
    (if (i32.ne (i32.load (i32.const 16)) (i32.const 0x06050403)) (then unreachable))
    (if (i32.ne (call $read_file (i32.const 0) (i32.const 32) (i32.const 8)) (i32.const 0))
      (then unreachable))
    (if (i64.ne (i64.load (i32.const 32)) (i64.const 0x0807060504030201)) (then unreachable))))"#
    ));
    let pack = no_pack();

    let outcome = Runtime::new(Limits::default())
        .run(
            &guest,
            "go",
            Subject::describing(&pack, "Root").with_file(&file),
            reading(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("the guest read what it was handed");

    assert_eq!(outcome, Outcome::default(), "a read changes nothing");
}

#[test]
fn a_read_that_runs_past_the_file_or_the_guests_memory_is_a_bad_argument_that_writes_nothing() {
    let file = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let guest = wasm(&format!(
        r#"(module
  {READ_FILE}
  (memory (export "memory") 1)
  (data (i32.const 16) "\aa\aa\aa\aa")
  (func (export "go")
    ;; 4 bytes from offset 6 of 8
    (if (i32.ne (call $read_file (i32.const 6) (i32.const 16) (i32.const 4)) (i32.const 7))
      (then unreachable))
    ;; into the last bytes of the one page
    (if (i32.ne (call $read_file (i32.const 0) (i32.const 65532) (i32.const 8)) (i32.const 7))
      (then unreachable))
    ;; a length the guest's i32 reads as negative
    (if (i32.ne (call $read_file (i32.const 0) (i32.const 16) (i32.const -1)) (i32.const 7))
      (then unreachable))
    (if (i32.ne (i32.load (i32.const 16)) (i32.const 0xaaaaaaaa)) (then unreachable))))"#
    ));
    let pack = no_pack();

    let outcome = Runtime::new(Limits::default())
        .run(
            &guest,
            "go",
            Subject::describing(&pack, "Root").with_file(&file),
            reading(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("every read was refused and the guest saw its own bytes");

    assert_eq!(
        outcome
            .failures
            .iter()
            .map(|f| (f.function, f.status))
            .collect::<Vec<_>>(),
        vec![("read_file", Status::BadArgument); 3]
    );
    assert!(
        outcome.failures[0]
            .because
            .as_deref()
            .is_some_and(|why| why.contains("8 bytes") && why.contains("past its end")),
        "{:?}",
        outcome.failures[0].because
    );
}

#[test]
fn a_guest_holds_only_the_file_functions_its_grants_buy() {
    let pack = no_pack();
    let file = [0u8; 4];
    let importing = |import: &str| {
        wasm(&format!(
            r#"(module {import} (memory (export "memory") 1) (func (export "go")))"#
        ))
    };
    let run = |import: &str, grants: Grants| {
        Runtime::new(Limits::default()).run(
            &importing(import),
            "go",
            Subject::describing(&pack, "Root").with_file(&file),
            grants,
            AlwaysWritable,
            &AcceptsAnything,
        )
    };

    for (import, bought_by) in [
        (FILE_SIZE, Capability::ReadFileBytes),
        (READ_FILE, Capability::ReadFileBytes),
        (WRITE_FILE, Capability::WriteFileBytes),
    ] {
        let error = run(import, Grants::default()).expect_err("the default grant buys no file");
        assert!(
            matches!(&error, HostError::Unresolved { needs: Some(c), .. } if *c == bought_by),
            "{import} needs {bought_by:?}: {error:?}"
        );
    }

    let error = run(WRITE_FILE, reading()).expect_err("reading a file does not buy replacing it");
    assert!(
        matches!(
            &error,
            HostError::Unresolved {
                needs: Some(Capability::WriteFileBytes),
                ..
            }
        ),
        "{error:?}"
    );
    run(
        FILE_SIZE,
        Grants::default().granting(Capability::WriteFileBytes),
    )
    .expect_err("replacing a file does not buy reading it");
}

/// What a guest that writes `new!` and then checks what the host answered looks like.
fn writing_guest(expect_status: i32, sentence: &str, cap: i32) -> Vec<u8> {
    let sentence = wat_text(sentence);
    let len = sentence.len();
    wasm(&format!(
        r#"(module
  {WRITE_FILE}
  (memory (export "memory") 1)
  (data (i32.const 0) "new!")
  (data (i32.const 256) "{sentence}")
  (data (i32.const 512) "\ee\ee\ee\ee\ee\ee\ee\ee\ee\ee\ee\ee")
  {SAME}
  (func (export "go")
    ;; a length the host must overwrite
    (i32.store (i32.const 32) (i32.const 0xffff))
    (if (i32.ne (call $write_file (i32.const 0) (i32.const 4)
                  (i32.const 512) (i32.const {cap}) (i32.const 32))
                (i32.const {expect_status}))
      (then unreachable))
    ;; the whole sentence's length, and as much of it as fits
    (if (i32.ne (i32.load (i32.const 32)) (i32.const {len})) (then unreachable))
    (if (i32.eqz (call $same (i32.const 256) (i32.const 512) (i32.const {fits})))
      (then unreachable))))"#,
        fits = len.min(usize::try_from(cap).expect("a capacity")),
    ))
}

#[test]
fn a_write_the_gate_refuses_returns_the_gates_sentence_to_the_guest_and_asks_nothing_else() {
    let pack = no_pack();
    let file = [0u8; 8];
    let mut document = vec![0u8; 0x20];
    let sentence = the_gates_sentence("the editor is in read-only mode");
    let check = Watching::default();

    let outcome = Runtime::new(Limits::default())
        .run(
            &writing_guest(Status::NotWritable.code(), &sentence, 256),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20).with_file(&file),
            reading_and_writing(),
            NeverWritable("the editor is in read-only mode".into()),
            &check,
        )
        .expect("the guest saw the sentence the user sees");

    assert!(
        check.files.borrow().is_empty(),
        "⚠ the gate is asked before the format's check, so a refused write is never judged"
    );
    assert_eq!(outcome.replaced, None);
    assert!(!outcome.touched);
    assert_eq!(
        outcome
            .refused
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>(),
        vec![sentence.clone()],
        "and the host keeps the same sentence for the user"
    );
    assert_eq!(
        outcome.failures[0].because.as_deref(),
        Some(sentence.as_str())
    );
}

#[test]
fn a_write_the_formats_check_refuses_changes_nothing_and_says_why() {
    let pack = no_pack();
    let file = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let mut document = vec![0u8; 0x20];
    let check = Watching {
        refuses: Some("the bytes do not read back".into()),
        ..Default::default()
    };
    let sentence = "the file was not replaced: the bytes do not read back";
    let guest = {
        // Refused with `abi.refused`, and the file the guest reads next is the old one.
        let sentence = wat_text(sentence);
        let len = sentence.len();
        wasm(&format!(
            r#"(module
  {WRITE_FILE}
  {READ_FILE}
  (memory (export "memory") 1)
  (data (i32.const 0) "new!")
  (data (i32.const 256) "{sentence}")
  {SAME}
  (func (export "go")
    (if (i32.ne (call $write_file (i32.const 0) (i32.const 4)
                  (i32.const 512) (i32.const 256) (i32.const 32))
                (i32.const 6))
      (then unreachable))
    (if (i32.ne (i32.load (i32.const 32)) (i32.const {len})) (then unreachable))
    (if (i32.eqz (call $same (i32.const 256) (i32.const 512) (i32.const {len})))
      (then unreachable))
    (if (i32.ne (call $read_file (i32.const 0) (i32.const 1024) (i32.const 8)) (i32.const 0))
      (then unreachable))
    (if (i64.ne (i64.load (i32.const 1024)) (i64.const 0x0807060504030201)) (then unreachable))))"#
        ))
    };

    let outcome = Runtime::new(Limits::default())
        .run(
            &guest,
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20).with_file(&file),
            reading_and_writing(),
            AlwaysWritable,
            &check,
        )
        .expect("the guest was refused, said so, and still reads the old file");

    assert_eq!(*check.files.borrow(), vec![b"new!".to_vec()]);
    assert_eq!(outcome.replaced, None);
    assert!(!outcome.touched, "a refused write is not a change");
    assert_eq!(document, vec![0u8; 0x20]);
}

#[test]
fn a_write_the_gates_keep_is_the_file_the_guest_reads_next_and_the_embedder_is_handed() {
    let pack = no_pack();
    let file = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let mut document = vec![0u8; 0x20];
    let check = Watching::default();
    let guest = wasm(&format!(
        r#"(module
  {WRITE_FILE}
  {FILE_SIZE}
  {READ_FILE}
  (memory (export "memory") 1)
  (data (i32.const 0) "new!")
  (func (export "go")
    (i32.store (i32.const 32) (i32.const 0xffff))
    (if (i32.ne (call $write_file (i32.const 0) (i32.const 4)
                  (i32.const 512) (i32.const 256) (i32.const 32))
                (i32.const 0))
      (then unreachable))
    ;; no refusal, so no sentence
    (if (i32.ne (i32.load (i32.const 32)) (i32.const 0)) (then unreachable))
    (if (i32.ne (call $file_size (i32.const 64)) (i32.const 0)) (then unreachable))
    (if (i32.ne (i32.load (i32.const 64)) (i32.const 4)) (then unreachable))
    (if (i32.ne (call $read_file (i32.const 0) (i32.const 128) (i32.const 4)) (i32.const 0))
      (then unreachable))
    (if (i32.ne (i32.load (i32.const 128)) (i32.const 0x2177656e)) (then unreachable))))"#
    ));

    let outcome = Runtime::new(Limits::default())
        .run(
            &guest,
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20).with_file(&file),
            reading_and_writing(),
            AlwaysWritable,
            &check,
        )
        .expect("the write was kept and the guest read it back");

    assert_eq!(
        *check.files.borrow(),
        vec![b"new!".to_vec()],
        "the format's check was shown exactly the guest's bytes"
    );
    assert_eq!(outcome.replaced.as_deref(), Some(&b"new!"[..]));
    assert!(outcome.touched, "the document is changed");
    assert_eq!(outcome.failures, Vec::new());
    assert_eq!(
        document,
        vec![0u8; 0x20],
        "the host puts the bytes in place, not this crate: the embedder's document is its own"
    );
}

/// A run patches fields or replaces the file, and never both: the second would
/// land in a document the first had already made stale.
#[test]
fn a_run_patches_a_field_or_replaces_the_file_and_never_both() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(or_io::desc::PackType {
        name: "Root".into(),
        size: 0x20,
        fields: vec![or_io::desc::PackField {
            name: "ratio".into(),
            offset: 4,
            repr: or_io::desc::FieldRepr::Scalar(or_io::desc::Scalar::F32),
            space: None,
            bits: Vec::new(),
            note: None,
        }],
        selector: None,
    });
    let file = [1u8, 2, 3, 4];
    let patch = r#"(call $patch_field (i32.const 0) (i32.const 4) (i32.const 0)
                     (i32.const 8) (i32.const 5) (i32.const 16) (i32.const 5))"#;
    let write = r#"(call $write_file (i32.const 32) (i32.const 4)
                     (i32.const 512) (i32.const 64) (i32.const 64))"#;
    let guest = |first: &str, then: &str| {
        wasm(&format!(
            r#"(module
  (import "openrage" "patch_field"
    (func $patch_field (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  {WRITE_FILE}
  (memory (export "memory") 1)
  (data (i32.const 0) "Root")
  (data (i32.const 8) "ratio")
  (data (i32.const 16) "\02\00\00\20\41")
  (data (i32.const 32) "new!")
  (func (export "go")
    (if (i32.ne {first} (i32.const 0)) (then unreachable))
    (if (i32.ne {then} (i32.const 6)) (then unreachable))))"#
        ))
    };
    let grants = reading_and_writing().granting(Capability::EditOpenDocument);

    // A patch, then the file.
    let mut document = vec![0u8; 0x20];
    let outcome = Runtime::new(Limits::default())
        .run(
            &guest(patch, write),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20).with_file(&file),
            grants.clone(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("the write was refused and the guest saw it");
    assert_eq!(outcome.replaced, None);
    assert!(outcome.touched, "the patch stands");
    assert_eq!(document[4..8], 10.0f32.to_le_bytes());

    // The file, then a patch.
    let mut document = vec![0u8; 0x20];
    let outcome = Runtime::new(Limits::default())
        .run(
            &guest(write, patch),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20).with_file(&file),
            grants,
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("the patch was refused and the guest saw it");
    assert_eq!(outcome.replaced.as_deref(), Some(&b"new!"[..]));
    assert_eq!(document, vec![0u8; 0x20], "the patch landed nowhere");
}

#[test]
fn a_document_held_read_only_refuses_a_write_the_gate_would_allow() {
    let pack = no_pack();
    let file = [0u8; 8];
    let check = Watching::default();
    let because = "the plugin was opened on a document its host holds read-only";

    let outcome = Runtime::new(Limits::default())
        .run(
            &writing_guest(Status::NotWritable.code(), because, 256),
            "go",
            Subject::describing(&pack, "Root").with_file(&file),
            reading_and_writing(),
            AlwaysWritable,
            &check,
        )
        .expect("the guest was told why");

    assert!(check.files.borrow().is_empty());
    assert_eq!(outcome.replaced, None);
}

#[test]
fn a_sentence_slot_too_short_gets_the_length_and_a_prefix_and_the_status_stays_the_refusals() {
    let pack = no_pack();
    let file = [0u8; 8];
    let mut document = vec![0u8; 0x20];
    let sentence = the_gates_sentence("the editor is in read-only mode");

    Runtime::new(Limits::default())
        .run(
            // Ten bytes of room: the status is still the gate's, not `abi.buffer_too_small`.
            &writing_guest(Status::NotWritable.code(), &sentence, 10),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20).with_file(&file),
            reading_and_writing(),
            NeverWritable("the editor is in read-only mode".into()),
            &AcceptsAnything,
        )
        .expect("the length is whole and the ten bytes are its first ten");
}

#[test]
fn a_sentence_slot_outside_the_guests_memory_is_refused_before_anything_is_kept() {
    let pack = no_pack();
    let file = [0u8; 8];
    let mut document = vec![0u8; 0x20];
    let check = Watching::default();
    let guest = wasm(&format!(
        r#"(module
  {WRITE_FILE}
  (memory (export "memory") 1)
  (data (i32.const 0) "new!")
  (func (export "go")
    (if (i32.ne (call $write_file (i32.const 0) (i32.const 4)
                  (i32.const 65530) (i32.const 100) (i32.const 32))
                (i32.const 7))
      (then unreachable))))"#
    ));

    let outcome = Runtime::new(Limits::default())
        .run(
            &guest,
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20).with_file(&file),
            reading_and_writing(),
            AlwaysWritable,
            &check,
        )
        .expect("the guest was refused");

    assert!(check.files.borrow().is_empty());
    assert_eq!(outcome.replaced, None);
    assert!(!outcome.touched, "a bad slot never follows a kept write");
}

#[test]
fn a_subject_with_no_file_has_no_bytes_to_read_or_replace() {
    let pack = no_pack();
    let mut document = vec![0u8; 0x20];
    let guest = wasm(&format!(
        r#"(module
  {FILE_SIZE}
  {WRITE_FILE}
  (memory (export "memory") 1)
  (data (i32.const 0) "new!")
  (func (export "go")
    (if (i32.ne (call $file_size (i32.const 64)) (i32.const 6)) (then unreachable))
    (if (i32.ne (call $write_file (i32.const 0) (i32.const 4)
                  (i32.const 512) (i32.const 256) (i32.const 32))
                (i32.const 6))
      (then unreachable))))"#
    ));

    let outcome = Runtime::new(Limits::default())
        .run(
            &guest,
            "go",
            Subject::editing(&pack, "Root", &mut document, 0x20),
            reading_and_writing(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("both calls were refused");
    assert_eq!(outcome.replaced, None);
    assert!(outcome.failures[0]
        .because
        .as_deref()
        .is_some_and(|why| why.contains("no file")));
}

#[test]
fn the_sample_guest_reads_a_ydds_bytes_and_parses_it_with_or_io_inside_the_guest() {
    let ydd = authored_ydd(100.0);
    let pack = no_pack();
    let mut document = Vec::new();
    let check = Watching::default();

    let outcome = Runtime::new(parsing())
        .run(
            sample_guest(),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0).with_file(&ydd),
            reading_and_writing(),
            AlwaysWritable,
            &check,
        )
        .expect("the guest parsed the .ydd in WebAssembly and wrote it back");

    let written = outcome.replaced.expect("the write was kept");
    assert_eq!(
        first_distances(&written),
        vec![200.0],
        "the guest doubled the first distance, so it read the file and edited what it parsed"
    );
    assert_eq!(*check.files.borrow(), vec![written.clone()]);
    assert!(outcome.touched);
    assert_eq!(
        outcome.failures,
        Vec::new(),
        "every host call the guest made answered ok"
    );
    assert_eq!(
        first_distances(&ydd),
        vec![100.0],
        "and the file it was handed is untouched"
    );
}

#[test]
fn the_sample_guest_is_stopped_by_the_gate_with_its_sentence_before_the_formats_check() {
    let ydd = authored_ydd(100.0);
    let pack = no_pack();
    let mut document = Vec::new();
    let check = Watching::default();

    let error = Runtime::new(parsing())
        .run(
            sample_guest(),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0).with_file(&ydd),
            reading_and_writing(),
            NeverWritable("the editor is in read-only mode".into()),
            &check,
        )
        .expect_err("the sample expects its write to land");

    let HostError::StoppedAfterRefusal { refused, .. } = &error else {
        panic!("a trap after a refusal is reported with it: {error:?}");
    };
    assert_eq!(
        refused.iter().map(|r| r.capability).collect::<Vec<_>>(),
        vec![Capability::WriteFileBytes]
    );
    assert!(
        error
            .to_string()
            .contains("the editor is in read-only mode"),
        "{error}"
    );
    assert!(check.files.borrow().is_empty());
}

#[test]
fn the_sample_guest_is_refused_the_write_its_format_check_refuses() {
    let ydd = authored_ydd(100.0);
    let pack = no_pack();
    let mut document = Vec::new();
    let check = Watching {
        refuses: Some("the game would misread it".into()),
        ..Default::default()
    };

    let error = Runtime::new(parsing())
        .run(
            sample_guest(),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0).with_file(&ydd),
            reading_and_writing(),
            AlwaysWritable,
            &check,
        )
        .expect_err("a refused write ends the sample");
    assert!(
        matches!(error, HostError::Guest(_)),
        "a format refusal is no capability refusal, only a trap: {error:?}"
    );
    assert_eq!(check.files.borrow().len(), 1);
}

/// What a guest that reads a file whole, which is what the cost is of, looks like.
const COPY_WHOLE_FILE: &str = r#"(module
  (import "openrage" "file_size" (func $file_size (param i32) (result i32)))
  (import "openrage" "read_file" (func $read_file (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "go")
    (local $n i32)
    (if (i32.ne (call $file_size (i32.const 0)) (i32.const 0)) (then unreachable))
    (local.set $n (i32.load (i32.const 0)))
    ;; room for the file above the first page
    (if (i32.eq (memory.grow (i32.add (i32.shr_u (local.get $n) (i32.const 16)) (i32.const 1)))
                (i32.const -1))
      (then unreachable))
    (if (i32.ne (call $read_file (i32.const 0) (i32.const 65536) (local.get $n)) (i32.const 0))
      (then unreachable))))"#;

/// The same guest that asks for the room and never copies into it.
const GROW_ONLY: &str = r#"(module
  (import "openrage" "file_size" (func $file_size (param i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "go")
    (local $n i32)
    (if (i32.ne (call $file_size (i32.const 0)) (i32.const 0)) (then unreachable))
    (local.set $n (i32.load (i32.const 0)))
    (if (i32.eq (memory.grow (i32.add (i32.shr_u (local.get $n) (i32.const 16)) (i32.const 1)))
                (i32.const -1))
      (then unreachable))
    ;; touch the last page, as a copy's last byte would
    (i32.store8 (i32.add (i32.const 65535) (local.get $n)) (i32.const 1))))"#;

/// The best of five runs of `guest` against `file`.
fn best_of_five(guest: &[u8], file: &[u8]) -> Duration {
    let pack = no_pack();
    (0..5)
        .map(|_| {
            let started = Instant::now();
            Runtime::new(Limits::default())
                .run(
                    guest,
                    "go",
                    Subject::describing(&pack, "Root").with_file(file),
                    reading(),
                    AlwaysWritable,
                    &AcceptsAnything,
                )
                .expect("the guest copied the file");
            started.elapsed()
        })
        .min()
        .expect("five runs")
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// R44f's measurement: the cost of a copy into a guest per call, printed with
/// `--nocapture` and recorded in the roadmap row. Its assertions are the
/// derivable half: the copy is exactly the file's size, and only a copy.
#[test]
fn what_a_files_bytes_cost_a_guest() {
    let copy = wasm(COPY_WHOLE_FILE);
    let grow = wasm(GROW_ONLY);

    println!("synthetic files, best of five runs, ms (a run builds a store and a module)");
    println!(
        "{:>10} {:>10} {:>10} {:>10}",
        "bytes", "grow only", "grow+copy", "copy"
    );
    for size in [64 << 10, 1 << 20, 8 << 20, 32 << 20] {
        let file = vec![0xA5u8; size];
        let (grown, copied) = (best_of_five(&grow, &file), best_of_five(&copy, &file));
        println!(
            "{size:>10} {:>10.2} {:>10.2} {:>10.2}",
            millis(grown),
            millis(copied),
            millis(copied.saturating_sub(grown))
        );
    }

    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let sizes_of = |ext: &str| -> Vec<(u64, std::path::PathBuf)> {
        let mut files: Vec<_> = Corpus::with_root(&root, ext)
            .target(TARGET_GEN8)
            .find()
            .into_iter()
            .filter_map(|path| Some((std::fs::metadata(&path).ok()?.len(), path)))
            .collect();
        files.sort();
        files
    };
    let (ydds, ytds) = (sizes_of("ydd"), sizes_of("ytd"));
    let (Some(typical), Some(largest_ydd), Some(large)) =
        (ydds.get(ydds.len() / 2), ydds.last(), ytds.last())
    else {
        or_test_support::skip(
            "the copy cost of a staged .ydd and .ytd",
            &root,
            "ydd and ytd",
        );
        return;
    };

    println!(
        "staged: {} .ydd, median {} bytes, largest {}; {} .ytd, largest {} bytes",
        ydds.len(),
        typical.0,
        largest_ydd.0,
        ytds.len(),
        large.0
    );
    println!(
        "{:<28} {:>10} {:>10} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "file", "on disk", "stored", "ms stored", "ms deflt", "ms grow", "ms g+c", "ms copy"
    );
    for (_, path) in [typical, largest_ydd, large] {
        let on_disk = std::fs::read(path).expect("a staged file reads");
        let opened = Rsc7::open(&on_disk).expect("a staged resource opens");
        let started = Instant::now();
        let stored =
            Rsc7::reseal(&opened.data, &opened.seal, Compression::none()).expect("it seals");
        let stored_ms = millis(started.elapsed());
        let started = Instant::now();
        let deflated = Rsc7::reseal(&opened.data, &opened.seal, Compression::default())
            .expect("it seals again");
        let deflated_ms = millis(started.elapsed());
        let (grown, copied) = (best_of_five(&grow, &stored), best_of_five(&copy, &stored));
        println!(
            "{:<28} {:>10} {:>10} {stored_ms:>9.2} {deflated_ms:>9.2} {:>9.2} {:>9.2} {:>9.2}   (deflated {} bytes)",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            on_disk.len(),
            stored.len(),
            millis(grown),
            millis(copied),
            millis(copied.saturating_sub(grown)),
            deflated.len(),
        );
    }
}

/// The instructions the sample guest spends parsing and writing back `file`,
/// found by climbing a ladder of fuel limits.
fn climb(what: &str, file: &[u8]) -> u64 {
    let pack = no_pack();
    for fuel in [
        10_000_000u64,
        30_000_000,
        100_000_000,
        300_000_000,
        1_000_000_000,
        3_000_000_000,
        10_000_000_000,
    ] {
        let mut document = Vec::new();
        let started = Instant::now();
        let ran = Runtime::new(Limits {
            fuel: Some(fuel),
            ..Limits::default()
        })
        .run(
            sample_guest(),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0).with_file(file),
            reading_and_writing(),
            AlwaysWritable,
            &AcceptsAnything,
        );
        println!(
            "{what} ({} bytes) with {fuel} fuel: {} in {:.0} ms",
            file.len(),
            match &ran {
                Ok(_) => "ran to the end".to_string(),
                Err(e) => e.to_string(),
            },
            millis(started.elapsed())
        );
        if ran.is_ok() {
            return fuel;
        }
    }
    panic!("the sample did not finish {what} in ten billion instructions");
}

/// What parsing a file with `or-io` in a guest costs in fuel, against the
/// host's default: the app runs plugins on [`Limits::default`], so the
/// figure says whether a plugin can parse in the app at all.
#[test]
fn what_a_guest_parsing_a_ydd_spends_against_the_default_fuel() {
    let spent = climb("an authored one-drawable .ydd", &authored_ydd(100.0));
    assert!(
        spent > Limits::default().fuel.expect("the default is bounded"),
        "the sample parsed an authored .ydd inside the default fuel, which is what \
         the app's limit would have to be tested against again"
    );

    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let mut ydds: Vec<_> = Corpus::with_root(&root, "ydd")
        .target(TARGET_GEN8)
        .find()
        .into_iter()
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.len(), path)))
        .collect();
    ydds.sort();
    let Some((_, path)) = ydds.get(ydds.len() / 2) else {
        or_test_support::skip(
            "the fuel a guest spends parsing a staged .ydd",
            &root,
            "ydd",
        );
        return;
    };
    climb(
        &path.display().to_string(),
        &std::fs::read(path).expect("a staged file reads"),
    );
}

#[test]
fn every_staged_ydd_comes_back_through_the_sample_guest_with_its_distances_doubled() {
    let Some(files) = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "ydd")
        .target(TARGET_GEN8)
        .limit(4)
        .find_or_skip("a staged .ydd through the sample guest")
    else {
        return;
    };
    let pack = no_pack();

    let one = |path: &std::path::PathBuf| {
        let file = std::fs::read(path).expect("a staged file reads");
        let Ok(before) = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&file) else {
            return 0usize;
        };
        let expected: Vec<f32> = before
            .entries
            .iter()
            .flatten()
            .map(|drawable| drawable.base.lod_group.lod_distances[0] * 2.0)
            .collect();

        let mut document = Vec::new();
        let outcome = Runtime::new(Limits {
            fuel: Some(10_000_000_000),
            ..Limits::default()
        })
        .run(
            sample_guest(),
            "go",
            Subject::editing(&pack, "Root", &mut document, 0).with_file(&file),
            reading_and_writing(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let written = outcome.replaced.expect("the write was kept");
        assert_eq!(first_distances(&written), expected, "{}", path.display());
        1
    };
    // Each file runs in its own guest, so one thread takes each.
    let through: usize = std::thread::scope(|scope| {
        let runs: Vec<_> = files.iter().map(|path| scope.spawn(|| one(path))).collect();
        runs.into_iter()
            .map(|run| {
                run.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .sum()
    });
    assert!(through > 0, "no staged .ydd opened owned");
}
