use crate::support;

use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::pso::schema::{
    PsoSchemaSection, PsoStructEntry, PsoStructInfo, ARRAY_INFO_NAME_HASH,
};
use or_io::meta::binary::rbf::layout::{RbfLayout, RbfStructShape};
use or_io::meta::binary::synth::{name_hash, pso_layout_for, pso_layout_for_instance};
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::{BinaryFormat, Diagnostics, MetaInstance, Value};

fn rbf_layout(shapes: &[(&str, i16)]) -> MetaLayout {
    let mut layout = RbfLayout::default();
    for (name, attrs) in shapes {
        layout.record_shape(
            name,
            RbfStructShape {
                x1: 0,
                x2: 0,
                attribute_count: *attrs,
            },
        );
    }
    MetaLayout {
        format: BinaryFormat::Rbf,
        container: ContainerLayout::Rbf(layout),
    }
}

#[test]
fn rbf_document_built_from_scratch_round_trips() {
    let mut child = MetaInstance::new("CChild");
    child.set("id", Value::UInt(7));
    child.set("ratio", Value::Float(0.25));

    let mut root = MetaInstance::new("CRoot");
    root.set("name", Value::String("hand-built".into()));
    root.set("enabled", Value::Bool(true));
    root.set("origin", Value::Vec3([1.0, -2.0, 3.5]));
    root.set("CChild", Value::Struct(Box::new(child)));

    let layout = rbf_layout(&[("CRoot", 0), ("CChild", 0)]);
    let bytes = write_meta_binary(BinaryFormat::Rbf, Some(&layout), &root).expect("write");
    assert_eq!(&bytes[..4], b"RBF0");

    let mut diags = Diagnostics::lenient();
    let (back, _) = parse_meta_binary_full(support::schema(), &bytes, &mut diags).expect("re-read");

    assert!(
        diags.is_clean(),
        "gaps re-reading a synthetic file: {:?}",
        diags.gaps()
    );
    assert_eq!(back.type_name, "CRoot");
    assert_eq!(back.get("name"), Some(&Value::String("hand-built".into())));
    assert_eq!(back.get("enabled"), Some(&Value::Bool(true)));
    assert_eq!(back.get("origin"), Some(&Value::Vec3([1.0, -2.0, 3.5])));
    let Some(Value::Struct(inner)) = back.get("CChild") else {
        panic!("child missing from {back:?}");
    };
    assert_eq!(inner.get("id"), Some(&Value::UInt(7)));
    assert_eq!(inner.get("ratio"), Some(&Value::Float(0.25)));
}

#[test]
fn rbf_needs_no_layout_at_all() {
    let mut root = MetaInstance::new("CRoot");
    root.set("v", Value::UInt(1));
    let bytes = write_meta_binary(BinaryFormat::Rbf, None, &root).expect("write");
    let mut diags = Diagnostics::lenient();
    let (back, _) = parse_meta_binary_full(support::schema(), &bytes, &mut diags).expect("re-read");
    assert_eq!(back.get("v"), Some(&Value::UInt(1)));
}

/// One value per member, so a synthesised layout is asked to hold something
/// in every slot it declared rather than only in the scalar ones.
fn sample_for(schema: &PsoSchemaSection, entry: &PsoStructEntry, owner: &PsoStructInfo) -> Value {
    use or_io::meta::binary::pso::schema::{string_subtype, struct_subtype, PsoDataType as T};
    match entry.data_type {
        T::Bool | T::BoolV => Value::Bool(true),
        T::SByte | T::SShort | T::SInt | T::Int64 | T::PtrDiffT => Value::Int(3),
        T::UByte | T::UShort | T::UInt | T::UInt64 | T::SizeT => Value::UInt(3),
        T::Float | T::ScalarV | T::Double | T::Float16 => Value::Float(1.5),
        T::Vector2 | T::Vec2V => Value::Vec2([1.0, 2.0]),
        T::Vector3 | T::Vec3V => Value::Vec3([1.0, 2.0, 3.0]),
        T::Vector4 | T::Vec4V | T::VecBoolV => Value::Vec4([1.0, 2.0, 3.0, 4.0]),
        T::Matrix34 | T::Matrix44 | T::Mat33V | T::Mat34V | T::Mat44V => Value::Mat34([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]),
        T::String => match entry.subtype() {
            string_subtype::HASH | string_subtype::HASH_ALT => {
                Value::Hash(or_io::meta::instance::HashedString::new("sample"))
            }
            string_subtype::EMBEDDED_BUFFER => Value::String("s".into()),
            _ => Value::String("sample".into()),
        },
        T::Structure => match entry.subtype() {
            struct_subtype::EMBEDDED => match schema.struct_for(entry.referent_hash()) {
                Some(inner) => Value::Struct(Box::new(sample_instance(schema, inner))),
                None => Value::Null,
            },
            // A populated pointer needs a concrete target type, which
            // `synthesis_places_heap_members` covers on a schema it declares.
            _ => Value::Pointer(None),
        },
        T::Enum => Value::Enum {
            type_name: String::new(),
            variant: "0x00000000".into(),
        },
        T::Bitset => Value::Bitset {
            type_name: String::new(),
            bits: 0b101,
        },
        T::Array => {
            let count = entry.count().max(1) as usize;
            match entry.member_index().and_then(|i| owner.entry(i)) {
                Some(elem) => Value::Array(
                    std::iter::repeat_with(|| sample_for(schema, elem, owner))
                        .take(count)
                        .collect(),
                ),
                None => Value::Array(Vec::new()),
            }
        }
        T::Map => Value::Map(Vec::new()),
        T::Unknown(_) => Value::Null,
    }
}

/// Group a refusal by its cause, so the remainder reads as a list of missing
/// capabilities rather than as a number.
///
/// The first bucket is the sizes the PSC states as C++ constant expressions —
/// `CWeaponInfo::MAX_ATTACH_POINTS`, and one that multiplies two of them —
/// which need the headers, not a better parser. It does not mean the parser
/// drops `size=`: `parse_array_kind` did, returning `AtFixedArray(0)`, which
/// put 119 structs here until it carried the size.
fn refusal_kind(message: &str) -> &'static str {
    const KINDS: [(&str, &str); 5] = [
        (
            "fixed array that declares no size",
            "array size is not a number in the PSC",
        ),
        ("is not in the schema", "names a type the schema lacks"),
        ("contains itself by value", "recursive by value"),
        ("which synthesis does not", "member kind not laid out"),
        ("has no fixed size", "member of unknown width"),
    ];
    KINDS
        .iter()
        .find(|(needle, _)| message.contains(needle))
        .map(|(_, kind)| *kind)
        .unwrap_or("other")
}

fn encode_with(
    _lib: &or_io::meta::SchemaLibrary,
    inst: &MetaInstance,
    layout: or_io::meta::binary::layout::PsoLayout,
) -> or_io::meta::Result<Vec<u8>> {
    let meta = MetaLayout {
        format: BinaryFormat::Pso,
        container: ContainerLayout::Pso(layout),
    };
    write_meta_binary(BinaryFormat::Pso, Some(&meta), inst)
}

fn sample_instance(schema: &PsoSchemaSection, info: &PsoStructInfo) -> MetaInstance {
    let idx = support::hash_index();
    let mut inst = MetaInstance::new(idx.name_or_literal(info.name_hash));
    for entry in &info.entries {
        if entry.name_hash == ARRAY_INFO_NAME_HASH {
            continue;
        }
        inst.set(
            idx.name_or_literal(entry.name_hash),
            sample_for(schema, entry, info),
        );
    }
    inst
}

/// The breadth of synthesis over the real schema, printed and asserted against
/// the shape of the library rather than a number, because the library is not
/// committed.
#[test]
fn pso_documents_built_from_scratch_round_trip() {
    let lib = support::schema();
    if lib.structs.is_empty() {
        or_test_support::skip_corpus(
            "pso_documents_built_from_scratch_round_trip",
            "no schema snapshot is installed; see crates/io/or-io/src/meta/README.md",
        );
        return;
    }

    let mut names: Vec<&String> = lib.structs.keys().collect();
    names.sort();

    let (mut placed, mut round_tripped) = (0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();
    let mut refusals: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();

    for name in &names {
        // Sizing needs the values, so the shape is discovered with an empty
        // document and the layout is then rebuilt for the one that is written.
        let shape = match pso_layout_for(lib, name) {
            Ok(s) => s,
            Err(e) => {
                *refusals.entry(refusal_kind(&e.to_string())).or_default() += 1;
                continue;
            }
        };
        let hash = name_hash(name);
        let Some(info) = shape.schema.struct_for(hash) else {
            *refusals.entry("built under a different hash").or_default() += 1;
            continue;
        };
        if info.entries.is_empty() || info.data_size == 0 {
            *refusals.entry("declares no members").or_default() += 1;
            continue;
        }
        let inst = sample_instance(&shape.schema, info);
        let layout = match pso_layout_for_instance(lib, &inst) {
            Ok(l) => l,
            Err(e) => {
                *refusals.entry(refusal_kind(&e.to_string())).or_default() += 1;
                continue;
            }
        };
        placed += 1;

        // ⚠ The gate is a fixed point rather than equality with the sample.
        // A sample is not in canonical form — an enum reads back under the
        // name its table gives the value, not under the `0x00000000` spelling
        // written in — so comparing against it measures the sample's spelling.
        // What has to hold is that a document synthesised from what came back
        // reproduces the bytes exactly.
        let first = match encode_with(lib, &inst, layout) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: first encode: {e}"));
                continue;
            }
        };
        let mut diags = Diagnostics::lenient();
        let back = match parse_meta_binary_full(lib, &first, &mut diags) {
            Ok((b, _)) => b,
            Err(e) => {
                failures.push(format!("{name}: re-read: {e}"));
                continue;
            }
        };
        let Ok(relaid) = pso_layout_for_instance(lib, &back) else {
            failures.push(format!("{name}: what came back cannot be laid out"));
            continue;
        };
        let second = match encode_with(lib, &back, relaid) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: second encode: {e}"));
                continue;
            }
        };
        if first == second {
            round_tripped += 1;
        } else {
            failures.push(format!(
                "{name}: {} bytes then {}",
                first.len(),
                second.len()
            ));
        }
    }

    println!(
        "synthesis placed {placed} of {} structs in the installed schema; {round_tripped} \
         round-tripped",
        names.len()
    );
    for (kind, count) in &refusals {
        println!("  not placed, {kind}: {count}");
    }
    // ⚠ Every struct is in exactly one bucket. A remainder that does not add
    // up is a bucket nobody wrote, which is how a capability gap hides.
    let accounted: usize = placed + refusals.values().sum::<usize>();
    assert_eq!(
        accounted,
        names.len(),
        "{} structs reached neither the placed count nor a refusal bucket",
        names.len() - accounted
    );
    if !failures.is_empty() {
        println!(
            "  {}",
            failures
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
    assert!(
        placed > 0,
        "not one of {} structs in the installed schema could be laid out, so this run \
         asserted nothing",
        names.len()
    );
    assert!(
        failures.is_empty(),
        "{} of {placed} synthesised documents did not survive their own round trip:\n  {}",
        failures.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn synthesis_refuses_clearly() {
    let err = pso_layout_for(support::schema(), "CNoSuchStruct").expect_err("should refuse");
    let msg = err.to_string();
    assert!(msg.contains("CNoSuchStruct"), "unhelpful error: {msg}");
    assert!(msg.contains("synth:"), "error should be attributed: {msg}");
}

#[test]
fn rbm_from_scratch_is_refused_with_a_reason() {
    let inst = MetaInstance::new("CSomething");
    let err = write_meta_binary(BinaryFormat::Rbm, None, &inst).expect_err("should refuse");
    let msg = err.to_string();
    assert!(msg.contains("without a layout"), "unhelpful error: {msg}");
}
