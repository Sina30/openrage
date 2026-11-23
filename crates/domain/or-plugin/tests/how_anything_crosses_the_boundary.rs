//! What crossing the boundary costs, and what it refuses.
//!
//! A status is not an answer: a `-1` refusal from a function whose success is a
//! field count makes "you may not ask" and "there are no fields" the same bits.
//! Every guest here checks the status it got against a named
//! [`Status`](or_plugin::Status) and traps on anything else, so the
//! distinctions are asserted from the side that has to act on them.
//!
//! ⚠ A host-side allocation or stack overflow happens inside a host call, where
//! WebAssembly's isolation is not in the loop; the decoder's caps bound both.

use or_io::desc::value::ScalarValue;
use or_io::desc::{
    FieldRepr, IntWidth, Pack, PackEnum, PackField, PackType, PackVariant, Pointee, Scalar, Value,
};
use or_plugin::abi::{self, tag, CodecError, Status, MAX_DEPTH, MAX_ENCODED};
use or_plugin::{
    AcceptsAnything, AlwaysWritable, Capability, Grants, Limits, Runtime, Session, Subject,
};

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

/// A root type with one field of each shape the boundary has an answer for, and
/// one it does not.
fn pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.enums.push(PackEnum {
        name: "BoundType".into(),
        width: IntWidth::U32,
        variants: vec![PackVariant {
            name: "Geometry".into(),
            value: 4,
            note: None,
        }],
    });
    pack.types.push(PackType {
        name: "Root".into(),
        size: 0x20,
        fields: vec![
            field("count", 0x00, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
            field("ratio", 0x04, FieldRepr::Scalar(Scalar::F32)),
            field("skinned", 0x08, FieldRepr::Scalar(Scalar::Bool)),
            field(
                "bound_type",
                0x0C,
                FieldRepr::Enum {
                    type_name: "BoundType".into(),
                    width: IntWidth::U32,
                },
            ),
            // Not a scalar, so the boundary refuses it rather than approximating
            // it.
            field(
                "elsewhere",
                0x10,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Root".into(),
                }),
            ),
        ],
        selector: None,
    });
    pack
}

/// `count` = 7, `ratio` = 2.5, `skinned` = 1, `bound_type` = 4.
fn document() -> Vec<u8> {
    let mut bytes = vec![0u8; 0x20];
    bytes[0x00..0x04].copy_from_slice(&7u32.to_le_bytes());
    bytes[0x04..0x08].copy_from_slice(&2.5f32.to_le_bytes());
    bytes[0x08] = 1;
    bytes[0x0C..0x10].copy_from_slice(&4u32.to_le_bytes());
    bytes
}

fn run(
    guest: &str,
    pack: &Pack,
    document: &[u8],
) -> Result<or_plugin::Outcome, or_plugin::HostError> {
    Runtime::new(Limits::default()).run(
        &wasm(guest),
        "go",
        Subject::reading(pack, "Root", document, document.len()),
        Grants::default(),
        AlwaysWritable,
        &AcceptsAnything,
    )
}

/// The guest owns the buffer, the host writes into it, and the guest decodes
/// the tag and the payload itself.
#[test]
fn a_name_crosses_in_and_a_described_value_crosses_out() {
    // "Root" at 0, "ratio" at 8; length slot at 32; the answer at 40.
    let guest = format!(
        r#"(module
             (import "openrage" "field_value"
               (func $value (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (data (i32.const 8) "ratio")
             (func (export "go")
               (if (i32.ne
                     (call $value (i32.const 0) (i32.const 4) (i32.const 0)
                                  (i32.const 8) (i32.const 5)
                                  (i32.const 40) (i32.const 64) (i32.const 32))
                     (i32.const {ok}))
                 (then unreachable))
               ;; five bytes: one tag and a little-endian f32
               (if (i32.ne (i32.load (i32.const 32)) (i32.const 5)) (then unreachable))
               (if (i32.ne (i32.load8_u (i32.const 40)) (i32.const {f32_tag})) (then unreachable))
               (if (f32.ne (f32.load (i32.const 41)) (f32.const 2.5)) (then unreachable))))"#,
        ok = Status::Ok.code(),
        f32_tag = tag::F32,
    );

    let outcome = run(&guest, &pack(), &document()).expect(
        "the guest decoded 2.5 out of the buffer it lent the host. A trap here means \
         the encoding is not what the document says it is.",
    );
    assert!(
        outcome.failures.is_empty(),
        "nothing was refused: {:?}",
        outcome.failures
    );
}

/// Without the length the guest cannot retry; with a prefix written, a guest
/// that ignored the status would decode whatever fitted and call it a value.
#[test]
fn a_buffer_too_small_reports_the_length_it_needed_and_writes_nothing() {
    let guest = format!(
        r#"(module
             (import "openrage" "field_value"
               (func $value (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (data (i32.const 8) "ratio")
             (data (i32.const 40) "\aa\aa\aa\aa\aa")
             (func (export "go")
               (if (i32.ne
                     (call $value (i32.const 0) (i32.const 4) (i32.const 0)
                                  (i32.const 8) (i32.const 5)
                                  (i32.const 40) (i32.const 1) (i32.const 32))
                     (i32.const {too_small}))
                 (then unreachable))
               (if (i32.ne (i32.load (i32.const 32)) (i32.const 5)) (then unreachable))
               ;; ⚠ and not one byte of the answer landed
               (if (i32.ne (i32.load8_u (i32.const 40)) (i32.const 0xaa)) (then unreachable))))"#,
        too_small = Status::BufferTooSmall.code(),
    );

    let outcome =
        run(&guest, &pack(), &document()).expect("a short buffer is a retry, not a fault");
    assert_eq!(
        outcome
            .failures
            .iter()
            .map(|f| (f.function, f.status))
            .collect::<Vec<_>>(),
        vec![("field_value", Status::BufferTooSmall)],
        "⚠ it is recorded even though it is not a fault, because a guest that never \
         retries looks exactly like a plugin that does nothing"
    );
}

/// [`Status::Truncated`] says the installed pack and the file disagree, not that
/// the request was bad.
#[test]
fn each_way_of_being_wrong_has_its_own_status() {
    let pack = pack();
    let document = document();

    let cases = [
        ("ratio", Status::Ok, "a described scalar", document.clone()),
        (
            "nonexistent",
            Status::NoSuchField,
            "a name the type does not have",
            document.clone(),
        ),
        (
            "elsewhere",
            Status::Refused,
            "a field that is not a scalar",
            document.clone(),
        ),
        (
            "bound_type",
            Status::Truncated,
            "a described field past the end of a short document",
            vec![0u8; 4],
        ),
    ];

    for (name, expected, what, document) in cases {
        let guest = format!(
            r#"(module
                 (import "openrage" "field_value"
                   (func $value (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "Root")
                 (data (i32.const 8) "{name}")
                 (func (export "go")
                   (if (i32.ne
                         (call $value (i32.const 0) (i32.const 4) (i32.const 0)
                                      (i32.const 8) (i32.const {len})
                                      (i32.const 40) (i32.const 64) (i32.const 32))
                         (i32.const {code}))
                     (then unreachable))))"#,
            len = name.len(),
            code = expected.code(),
        );
        run(&guest, &pack, &document).unwrap_or_else(|e| {
            panic!("{what} must answer {expected:?} ({}): {e}", expected.key())
        });
    }
}

/// The host writes into guest memory at an address the guest chose, so the
/// bounds check is the host's and the failure has to be one the guest can read.
#[test]
fn a_call_aimed_outside_the_guests_memory_is_refused_rather_than_obeyed() {
    let guest = format!(
        r#"(module
             (import "openrage" "field_count" (func $count (param i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (func (export "go")
               (if (i32.ne (call $count (i32.const 0) (i32.const 4) (i32.const 1000000))
                           (i32.const {bad}))
                 (then unreachable))
               (if (i32.ne (call $count (i32.const 0) (i32.const 4) (i32.const -4))
                           (i32.const {bad}))
                 (then unreachable))
               ;; ⚠ and a name the guest pointed outside its own memory, which is
               ;; the other direction across the same boundary
               (if (i32.ne (call $count (i32.const 1000000) (i32.const 4) (i32.const 8))
                           (i32.const {bad}))
                 (then unreachable))))"#,
        bad = Status::BadArgument.code(),
    );
    run(&guest, &pack(), &document())
        .expect("an out-of-range destination is a refusal the guest reads, not a host panic");
}

#[test]
fn a_guest_that_exports_no_memory_is_told_so_rather_than_ignored() {
    let guest = format!(
        r#"(module
             (import "openrage" "field_count" (func $count (param i32 i32 i32) (result i32)))
             (func (export "go")
               (if (i32.ne (call $count (i32.const 0) (i32.const 4) (i32.const 0))
                           (i32.const {bad}))
                 (then unreachable))))"#,
        bad = Status::BadArgument.code(),
    );
    let outcome = run(&guest, &pack(), &document())
        .expect("no memory is a refusal; the host has nowhere to put the answer");
    assert_eq!(
        outcome
            .failures
            .iter()
            .map(|f| (f.function, f.status))
            .collect::<Vec<_>>(),
        vec![("field_count", Status::BadArgument)]
    );
}

#[test]
fn a_field_name_crosses_the_other_way() {
    let guest = format!(
        r#"(module
             (import "openrage" "field_name"
               (func $name (param i32 i32 i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "Root")
             (func (export "go")
               (if (i32.ne (call $name (i32.const 0) (i32.const 4) (i32.const 1)
                                       (i32.const 40) (i32.const 64) (i32.const 32))
                           (i32.const {ok}))
                 (then unreachable))
               (if (i32.ne (i32.load (i32.const 32)) (i32.const 5)) (then unreachable))
               ;; "ratio", least significant byte first
               (if (i32.ne (i32.load (i32.const 40)) (i32.const 0x6974_6172)) (then unreachable))
               (if (i32.ne (i32.load8_u (i32.const 44)) (i32.const 0x6f)) (then unreachable))
               ;; and an index past the end is `NoSuchField`, not an empty name
               (if (i32.ne (call $name (i32.const 0) (i32.const 4) (i32.const 99)
                                       (i32.const 40) (i32.const 64) (i32.const 32))
                           (i32.const {none}))
                 (then unreachable))))"#,
        ok = Status::Ok.code(),
        none = Status::NoSuchField.code(),
    );
    run(&guest, &pack(), &document()).expect("a name crossed and an absent one was refused");
}

/// The encoder's `match` is exhaustive over `Value`, so a new variant stops
/// `or-plugin` compiling; this holds that what comes back is what went in,
/// including the two byte runs that share bytes and mean different things.
#[test]
fn every_shape_of_described_value_survives_the_round_trip() {
    let pack = shapes();
    let mut tree = or_io::desc::value::empty(&pack, "Shapes").expect("the pack describes it");
    let record = tree.record_mut().expect("a struct");
    let scalars = [
        ("i", ScalarValue::Int(-9_000_000_000)),
        ("wide", ScalarValue::Int(i128::from(u64::MAX))),
        ("f", ScalarValue::F32(2.5)),
        ("b", ScalarValue::Bool(true)),
        ("d", ScalarValue::F64(-0.125)),
        ("e", ScalarValue::Int(4)),
        ("run[0]", ScalarValue::Int(1)),
        ("run[1]", ScalarValue::Int(2)),
        ("leaf.weight", ScalarValue::F32(0.5)),
        ("leaves[1].weight", ScalarValue::F32(-7.0)),
    ];
    for (path, value) in scalars {
        record.set_scalar(path, value).expect(path);
    }
    record.bytes_mut()[0x15..0x18].copy_from_slice(&[0xAA; 3]);
    record.bytes_mut()[0x28..0x30].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

    let mut leaf = or_io::desc::value::empty(&pack, "Leaf").expect("described");
    leaf.record_mut()
        .expect("a struct")
        .set_scalar("weight", ScalarValue::F32(9.0))
        .expect("a scalar");
    let children = [
        ("ptr", Value::ptr_to(Value::int(5))),
        ("null", Value::null_ptr()),
        ("shared", Value::Alias(0x5000_1000)),
        ("stranger", Value::UndescribedPtr(0x5000_2000)),
        ("garbage", Value::NotAnAddress(0xDEAD_BEEF)),
        ("payload", Value::Bytes(vec![1, 2, 3])),
        (
            "capped",
            Value::Array {
                elements: vec![Value::int(1), Value::int(2)],
                capacity: Some(8),
            },
        ),
        (
            "uncapped",
            Value::Array {
                elements: Vec::new(),
                capacity: None,
            },
        ),
        (
            "codec",
            Value::ptr_to(Value::Blob {
                codec: "synthetic".into(),
                parts: vec![
                    ("count".into(), Value::int(2)),
                    ("rest".into(), Value::Opaque(vec![9, 9])),
                ],
            }),
        ),
        ("leaf.next", Value::ptr_to(leaf.clone())),
        ("leaves[1].next", Value::ptr_to(leaf)),
    ];
    for (path, value) in children {
        record.set_child(path, value).expect(path);
    }

    let bytes = abi::encode(&tree).expect("it fits");
    assert_eq!(
        abi::decode_with(&bytes, Some(&pack)).expect("it decodes against its pack"),
        tree
    );

    let payload = tree.child("payload").expect("there");
    let opaque = tree.get("undescribed").expect("there");
    assert_eq!(opaque, Value::Opaque(vec![1, 2, 3, 4, 5, 6, 7, 8]));
    assert_ne!(
        abi::encode(payload).expect("fits"),
        abi::encode(&Value::Opaque(vec![1, 2, 3])).expect("fits"),
        "⚠ a payload the description named and a run nothing described encode the \
         same bytes under different tags; collapsing them throws away a distinction \
         the reader made"
    );
}

#[test]
fn a_packed_run_crosses_as_a_run_of_structs() {
    let mut pack = shapes();
    pack.types.push(PackType {
        name: "Point".into(),
        size: 0x08,
        fields: vec![
            field("x", 0x00, FieldRepr::Scalar(Scalar::F32)),
            field("y", 0x04, FieldRepr::Scalar(Scalar::F32)),
        ],
        selector: None,
    });
    let bytes: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let mut run =
        or_io::desc::RecordRun::from_bytes(pack.plans(), "Point", bytes).expect("packable");
    run.set_capacity(Some(4));
    let packed = Value::Records(run.clone());
    let unpacked = Value::Array {
        elements: run.records().into_iter().map(Value::Struct).collect(),
        capacity: Some(4),
    };

    let encoded = abi::encode(&packed).expect("fits");
    assert_eq!(
        encoded,
        abi::encode(&unpacked).expect("fits"),
        "how the host holds a run is not the guest's business"
    );
    assert_eq!(
        abi::decode_with(&encoded, Some(&pack)).expect("decodes"),
        packed
    );
}

/// A struct crosses by name, and only the document's pack turns names back
/// into offsets.
#[test]
fn a_struct_is_refused_without_the_pack_that_describes_it() {
    let pack = shapes();
    let tree = or_io::desc::value::empty(&pack, "Leaf").expect("described");
    let bytes = abi::encode(&tree).expect("fits");
    assert_eq!(
        abi::decode(&bytes),
        Err(CodecError::NeedsPack {
            type_name: "Leaf".into()
        })
    );
    assert_eq!(
        abi::decode_with(&bytes, Some(&pack)).expect("with its pack it decodes"),
        tree
    );

    // A codec's value is not a pack type, so it needs no pack.
    let blob = Value::Blob {
        codec: "synthetic".into(),
        parts: vec![("n".into(), Value::int(1))],
    };
    assert_eq!(abi::decode(&abi::encode(&blob).expect("fits")), Ok(blob));
}

#[test]
fn a_struct_that_disagrees_with_its_pack_is_refused() {
    fn str(out: &mut Vec<u8>, text: &str) {
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
    }
    fn weight(out: &mut Vec<u8>, name: &str) {
        str(out, name);
        out.push(tag::F32);
        out.extend_from_slice(&1.0f32.to_le_bytes());
    }
    fn next(out: &mut Vec<u8>) {
        str(out, "next");
        out.push(tag::PTR);
        out.extend_from_slice(&0u64.to_le_bytes());
        out.push(0);
    }
    fn leaf(type_name: &str, count: u32, body: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut out = vec![tag::STRUCT];
        str(&mut out, type_name);
        out.extend_from_slice(&count.to_le_bytes());
        body(&mut out);
        out
    }

    let pack = shapes();
    let decode = |bytes: Vec<u8>| abi::decode_with(&bytes, Some(&pack));
    let named = |field: &str| ("Leaf".to_string(), field.to_string());

    assert!(decode(leaf("Leaf", 2, |o| {
        weight(o, "weight");
        next(o);
    }))
    .is_ok());

    let refused = decode(leaf("Nope", 2, |o| {
        weight(o, "weight");
        next(o);
    }))
    .expect_err("no such type");
    assert_eq!(
        refused,
        CodecError::UnknownType {
            type_name: "Nope".into()
        }
    );
    assert_eq!(refused.status(), Status::NoSuchType);

    let refused = decode(leaf("Leaf", 2, |o| {
        weight(o, "mass");
        next(o);
    }))
    .expect_err("no such field");
    let (type_name, field) = named("mass");
    assert_eq!(refused, CodecError::UnknownField { type_name, field });
    assert_eq!(refused.status(), Status::NoSuchField);

    let refused = decode(leaf("Leaf", 2, |o| {
        str(o, "weight");
        o.push(tag::INT);
        o.extend_from_slice(&1i128.to_le_bytes());
        next(o);
    }))
    .expect_err("an integer is not an f32");
    assert!(
        matches!(&refused, CodecError::WrongShape { field, .. } if field == "weight"),
        "{refused:?}"
    );

    let refused = decode(leaf("Leaf", 1, |o| weight(o, "weight"))).expect_err("one is missing");
    let (type_name, field) = named("next");
    assert_eq!(
        refused,
        CodecError::MissingField { type_name, field },
        "⚠ an absent field would otherwise cross as zero, a value nobody sent"
    );

    let refused = decode(leaf("Leaf", 2, |o| {
        weight(o, "weight");
        weight(o, "weight");
    }))
    .expect_err("one is repeated");
    let (type_name, field) = named("weight");
    assert_eq!(refused, CodecError::RepeatedField { type_name, field });

    for refused in [
        CodecError::NeedsPack {
            type_name: "Leaf".into(),
        },
        CodecError::MissingField {
            type_name: "Leaf".into(),
            field: "next".into(),
        },
    ] {
        assert!(refused.to_string().len() > 30, "{refused}");
    }
}

/// Invented types at invented offsets: one field of every shape a record holds.
fn shapes() -> Pack {
    use or_io::desc::CountSource;

    let mut pack = Pack::new("synthetic", "t0");
    pack.enums.push(PackEnum {
        name: "BoundType".into(),
        width: IntWidth::U32,
        variants: vec![PackVariant {
            name: "Geometry".into(),
            value: 4,
            note: None,
        }],
    });
    let leaf = || {
        FieldRepr::Ptr(Pointee::One {
            type_name: "Leaf".into(),
        })
    };
    let many = || {
        FieldRepr::AtArray(Pointee::Many {
            type_name: None,
            count: CountSource::Inline,
            element_size: Some(8),
        })
    };
    pack.types.push(PackType {
        name: "Leaf".into(),
        size: 0x10,
        fields: vec![
            field("weight", 0x00, FieldRepr::Scalar(Scalar::F32)),
            field("next", 0x08, leaf()),
        ],
        selector: None,
    });
    pack.types.push(PackType {
        name: "Shapes".into(),
        size: 0xC8,
        fields: vec![
            field("i", 0x00, FieldRepr::Scalar(Scalar::Int(IntWidth::I64))),
            field("wide", 0x08, FieldRepr::Scalar(Scalar::Int(IntWidth::U64))),
            field("f", 0x10, FieldRepr::Scalar(Scalar::F32)),
            field("b", 0x14, FieldRepr::Scalar(Scalar::Bool)),
            field("pad", 0x15, FieldRepr::Padding { size: 3 }),
            field("d", 0x18, FieldRepr::Scalar(Scalar::F64)),
            field(
                "e",
                0x20,
                FieldRepr::Enum {
                    type_name: "BoundType".into(),
                    width: IntWidth::U32,
                },
            ),
            field(
                "run",
                0x24,
                FieldRepr::ScalarArray {
                    element: Scalar::Int(IntWidth::U16),
                    count: 2,
                },
            ),
            field("undescribed", 0x28, FieldRepr::Opaque { size: 8 }),
            field(
                "leaf",
                0x30,
                FieldRepr::Inline {
                    type_name: "Leaf".into(),
                },
            ),
            field(
                "leaves",
                0x40,
                FieldRepr::InlineArray {
                    type_name: "Leaf".into(),
                    count: 2,
                },
            ),
            field("ptr", 0x60, leaf()),
            field("null", 0x68, leaf()),
            field("shared", 0x70, leaf()),
            field("stranger", 0x78, leaf()),
            field("garbage", 0x80, leaf()),
            field("payload", 0x88, leaf()),
            field("capped", 0x90, many()),
            field("uncapped", 0xA0, many()),
            field("codec", 0xB0, leaf()),
        ],
        selector: None,
    });
    pack
}

/// ⚠ The decoder runs on the host's stack inside a host call, so without the
/// cap a hostile guest's nesting crashes the host; WebAssembly does not prevent
/// it.
#[test]
fn a_value_nested_past_the_depth_cap_is_refused_rather_than_overflowing() {
    let deep = 10_000;
    let mut bytes = Vec::new();
    for _ in 0..deep {
        bytes.push(tag::ARRAY);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0);
    }
    bytes.push(tag::BOOL);
    bytes.push(1);

    assert_eq!(abi::decode(&bytes), Err(CodecError::TooDeep));

    // And the encoder has the same bound, so the host cannot build one either.
    let mut tree = Value::boolean(true);
    for _ in 0..(MAX_DEPTH + 2) {
        tree = Value::Array {
            elements: vec![tree],
            capacity: None,
        };
    }
    assert_eq!(abi::encode(&tree), Err(CodecError::TooDeep));
}

/// A decoder that trusted the count would reserve four billion `Value`s for
/// nine bytes — a host out-of-memory a guest can ask for. The reservation is
/// capped by what is left to read.
#[test]
fn a_declared_count_does_not_allocate_before_the_bytes_are_there() {
    for tag in [tag::ARRAY, tag::STRUCT] {
        let mut bytes = vec![tag];
        if tag == tag::STRUCT {
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        if tag == abi::tag::ARRAY {
            bytes.push(0);
        }

        let started = std::time::Instant::now();
        let refused = abi::decode(&bytes).expect_err("there are no elements to read");
        assert!(
            matches!(refused, CodecError::Truncated { .. }),
            "{refused:?}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "a count of four billion took {:?} to refuse, which means something \
             sized itself from the count",
            started.elapsed()
        );
    }
}

#[test]
fn an_answer_larger_than_the_cap_is_refused_rather_than_built() {
    let huge = Value::Bytes(vec![0u8; MAX_ENCODED + 1]);
    assert_eq!(abi::encode(&huge), Err(CodecError::TooLarge));
    assert_eq!(CodecError::TooLarge.status(), Status::TooLarge);

    let fits = Value::Bytes(vec![0u8; MAX_ENCODED - 5]);
    assert!(
        abi::encode(&fits).is_ok(),
        "one under the cap still crosses"
    );
}

#[test]
fn bytes_that_are_not_this_encoding_are_refused() {
    assert_eq!(
        abi::decode(&[]),
        Err(CodecError::Truncated { wanted: 1, had: 0 })
    );
    assert_eq!(abi::decode(&[0xEE]), Err(CodecError::UnknownTag(0xEE)));
    assert_eq!(abi::decode(&[tag::BOOL, 2]), Err(CodecError::NotABool(2)));
    assert_eq!(
        abi::decode(&[tag::BOOL, 1, 0xFF]),
        Err(CodecError::TrailingBytes(1)),
        "⚠ surplus is either a second value or a length the guest got wrong, and \
         discarding it quietly hides both"
    );

    let mut bad_utf8 = vec![tag::ENUM];
    bad_utf8.extend_from_slice(&1u32.to_le_bytes());
    bad_utf8.push(0xFF);
    bad_utf8.extend_from_slice(&0i64.to_le_bytes());
    assert_eq!(abi::decode(&bad_utf8), Err(CodecError::NotUtf8));
}

#[test]
fn every_status_has_its_own_code_and_a_sentence() {
    let mut seen = std::collections::BTreeSet::new();
    for status in Status::ALL {
        assert!(
            seen.insert(status.code()),
            "{status:?} reuses code {}",
            status.code()
        );
        assert!(status.key().starts_with("abi."), "{status:?}");
        // Only the failures: a user is never shown "the call succeeded".
        if status != Status::Ok {
            assert!(
                status.means().len() > 30,
                "{status:?} needs a sentence somebody can act on, not a label: {}",
                status.means()
            );
        }
    }
    assert_eq!(
        Status::Ok.code(),
        0,
        "zero is success, as every guest assumes"
    );
}

/// A refusal that says "update OpenRage" or "rebuild the plugin" is only actionable if the versions it
/// refused against are written where the author reads, so the document and the constants must not
/// drift apart.
#[test]
fn the_document_a_plugin_author_reads_states_the_versions_the_host_implements() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../docs/reference/Plugin_ABI.md"),
    )
    .expect("docs/reference/Plugin_ABI.md is the author-facing half of this ABI");

    assert!(
        doc.contains(&format!("ABI version {}", or_plugin::ABI_VERSION)),
        "the document must state `ABI version {}`, which is the newest the host implements",
        or_plugin::ABI_VERSION
    );
    assert!(
        doc.contains("an older plugin still loads"),
        "and it must say the policy out loud: a version number with no stated \
         policy reads as a promise"
    );
    assert!(
        doc.contains(&format!("`OLDEST_ABI` ({})", or_plugin::OLDEST_ABI))
            && doc.contains(&format!("`ABI_VERSION` ({})", or_plugin::ABI_VERSION)),
        "and the oldest version the host still runs, beside the newest"
    );
    // ⚠ A capability a newer version added is named with the version that added it, or an author
    // cannot tell what a manifest of the older one may want.
    for capability in Capability::ALL.into_iter().filter(|c| c.since() > 0) {
        let key = format!("`{}`", capability.key());
        let line = doc
            .lines()
            .find(|line| line.starts_with('|') && line.contains(&key))
            .unwrap_or_else(|| panic!("{key} is in no table of the document"));
        assert!(
            line.contains(&format!("(ABI {})", capability.since())),
            "{key} is added by version {} and its row does not say so: {line}",
            capability.since()
        );
    }

    for status in Status::ALL {
        assert!(
            doc.contains(status.key()),
            "`{}` is a status a guest can receive and the document does not name it",
            status.key()
        );
    }

    // ⚠ A function a plugin author can import is named, with the key of the
    // capability that buys it, or the document describes a smaller host.
    for name in Runtime::functions_for(&Grants::trusted_with_everything()) {
        assert!(
            doc.contains(&format!("`{name}`")),
            "`{name}` is a host function and the document does not name it"
        );
        let capability = Runtime::capability_for(name).expect("every function has a capability");
        assert!(
            doc.contains(&format!("`{}`", capability.key())),
            "`{}` buys `{name}` and the document does not name its key",
            capability.key()
        );
    }
    for name in Session::functions_for(&Grants::trusted_with_everything()) {
        assert!(
            doc.contains(&format!("`{name}`")),
            "`{name}` is a host function of a session and the document does not name it"
        );
        let capability = Session::capability_for(name).expect("every function has a capability");
        assert!(
            doc.contains(&format!("`{}`", capability.key())),
            "`{}` buys `{name}` and the document does not name its key",
            capability.key()
        );
    }
    for name in or_plugin::extension::functions_for(&Grants::trusted_with_everything()) {
        assert!(
            doc.contains(&format!("`{name}`")),
            "`{name}` is a host function of a plugin that extends a build and the document does not name it"
        );
        let capability =
            or_plugin::extension::capability_for(name).expect("every function has a capability");
        assert!(
            doc.contains(&format!("`{}`", capability.key())),
            "`{}` buys `{name}` and the document does not name its key",
            capability.key()
        );
    }
    for export in [or_plugin::extension::VALIDATE, or_plugin::extension::EMIT] {
        assert!(
            doc.contains(&format!("`{export}`")),
            "`{export}` is an export of a plugin that extends a build and the document does not name it"
        );
    }
    for export in ["register", "draw_panel", "run_command", "picked"] {
        assert!(
            doc.contains(&format!("`{export}`")),
            "`{export}` is an export a session calls and the document does not name it"
        );
    }
}
