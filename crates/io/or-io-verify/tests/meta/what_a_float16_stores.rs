//! What a `Float16` member is worth, on a schema this file declares.
//!
//! ⚠ `pso::reader` and `instance::parser` must agree that a `Float16` is a
//! number: while the reader decoded its sixteen stored bits as `Value::UInt`
//! and XML imported it as `Value::Float`, no value survived a round trip.
//! Across the 30 `Float16` members the staged corpus ships, 9,060 values, the
//! half reading lands inside the bounds the PSC declares **9,030** times
//! against the integer reading's **1,676**; the 30 that miss are the half of a
//! value *equal* to the declared minimum, quantised just under it.
//!
//! ⚠ `OWNER`, `HALF` and `WHOLE` may be spelled only from
//! `public_names::KNOWN`: with no snapshot staged that is the whole of the
//! hash index [`slot`] and the writer look a field up by, so a name only a
//! snapshot knows passes on one machine and fails every run that has none.
//!
//! Nothing here gates on `has_default_library`: the library is declared inline,
//! so this runs on CI with neither a snapshot nor a corpus.

use crate::support;

use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::pso::schema::PsoDataType;
use or_io::meta::binary::public_names;
use or_io::meta::binary::synth::{name_hash, pso_layout_for_instance};
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::instance::{parse_meta, write_meta};
use or_io::meta::{
    BinaryFormat, Diagnostics, Field, FieldHint, FieldType, MetaInstance, SchemaLibrary, StructDef,
    Value,
};
use or_io::{f32_to_half, half_to_f32};

/// Every value the suite puts through the encoder, including the ones a
/// sixteen-bit float is awkward about.
fn cases() -> Vec<f64> {
    vec![
        0.0,
        -0.0,
        0.1,
        0.000_1,
        0.25,
        0.5,
        1.0,
        -1.0,
        1.5,
        2.5,
        // The bound `CProcObjInfo`'s rotation members declare.
        std::f64::consts::TAU,
        100.0,
        65504.0,
        // Smallest subnormal half, and one above it.
        f64::from(half_to_f32(0x0001)),
        f64::from(half_to_f32(0x0002)),
    ]
}

/// A name the hash index can give back, which is what the writer looks a
/// field up under.
fn slot(name: &'static str) -> &'static str {
    assert!(
        public_names::KNOWN.contains(&name),
        "'{name}' is not in public_names::KNOWN, so a snapshot is the only thing \
         that could resolve it and this suite would pass here and fail on CI"
    );
    let back = support::hash_index().name_or_literal(name_hash(name));
    assert_eq!(
        back, name,
        "'{name}' is not in the hash index, so the writer would look for '{back}' instead"
    );
    name
}

fn field(name: &'static str, ty: FieldType) -> Field {
    Field {
        name: slot(name).to_string(),
        ty,
        init: None,
        min: None,
        max: None,
        step: None,
        ui_key: false,
        hide_widgets: false,
        description: None,
        hint: FieldHint::None,
    }
}

const OWNER: &str = "CExtensionDefProcObject";
const HALF: &str = "spacing";
const WHOLE: &str = "Radius";

/// One `Float16` beside one ordinary `Float`, so a difference between them is
/// a difference the storage width made.
fn library() -> SchemaLibrary {
    let mut lib = SchemaLibrary::new();
    lib.insert_struct(StructDef {
        name: slot(OWNER).to_string(),
        base: None,
        abstract_: false,
        simple: true,
        fields: vec![
            field(HALF, FieldType::Float16),
            field(WHOLE, FieldType::Float),
        ],
        source_path: String::new(),
    });
    lib
}

fn document(value: f64) -> MetaInstance {
    let mut inst = MetaInstance::new(OWNER);
    inst.set(HALF, Value::Float(value));
    inst.set(WHOLE, Value::Float(value));
    inst
}

fn encode(lib: &SchemaLibrary, inst: &MetaInstance) -> Vec<u8> {
    let layout = MetaLayout {
        format: BinaryFormat::Pso,
        container: ContainerLayout::Pso(
            pso_layout_for_instance(lib, inst).expect("synthesise a layout"),
        ),
    };
    write_meta_binary(BinaryFormat::Pso, Some(&layout), inst).expect("encode")
}

fn decode(bytes: &[u8]) -> MetaInstance {
    let mut diags = Diagnostics::lenient();
    let (inst, _) = parse_meta_binary_full(support::schema(), bytes, &mut diags).expect("re-read");
    assert!(diags.is_clean(), "gaps re-reading: {:?}", diags.gaps());
    inst
}

fn float(inst: &MetaInstance, name: &str) -> f64 {
    match inst.get(name) {
        Some(Value::Float(v)) => *v,
        other => panic!("{name} came back as {other:?} rather than a float"),
    }
}

/// The value the member is worth, never the bit pattern it is kept in.
#[test]
fn a_float16_reads_back_as_the_number_a_half_can_hold() {
    let lib = library();
    let mut exercised = 0usize;

    for value in cases() {
        let back = decode(&encode(&lib, &document(value)));
        // Computed from the codec, not transcribed: what a half can hold of
        // this value is what has to come back.
        let expected = f64::from(half_to_f32(f32_to_half(value as f32)));
        assert_eq!(
            float(&back, HALF).to_bits(),
            expected.to_bits(),
            "{value} came back as {} rather than {expected}",
            float(&back, HALF)
        );
        assert_eq!(
            float(&back, WHOLE).to_bits(),
            f64::from(value as f32).to_bits(),
            "the f32 beside it moved, so this measures the encoder rather than the width"
        );
        exercised += 1;
    }

    // ⚠ The count is asserted rather than printed. A loop that silently
    // stopped exercising anything would otherwise report the absence of
    // failures as a pass.
    assert_eq!(
        exercised,
        cases().len(),
        "{exercised} of {} cases reached the assertion",
        cases().len()
    );
    assert!(exercised >= 15, "too few cases to tell two readings apart");
}

/// ⚠ **`0.1` is the discriminator.** A store that kept the member's bits, or
/// widened it to an `f32`, cannot produce this number from this input.
#[test]
fn the_quantisation_is_a_half_rather_than_a_float_or_an_integer() {
    let lib = library();
    let back = decode(&encode(&lib, &document(0.1)));

    let half = float(&back, HALF);
    assert_eq!(half.to_bits(), f64::from(half_to_f32(0x2E66)).to_bits());
    assert_ne!(half, 0.1, "a half cannot hold 0.1 exactly");
    assert_ne!(
        half,
        f64::from(0.1f32),
        "this is what an f32 would have given"
    );
    assert!(
        (half - 0.1).abs() < 1e-4,
        "and it is still 0.1 to four places, not its bit pattern {half}"
    );
}

/// A document holding one member and nothing else, so the bytes a value
/// change moves are that member's and no one else's.
fn alone(ty: FieldType, value: f64) -> (SchemaLibrary, MetaInstance) {
    let mut lib = SchemaLibrary::new();
    lib.insert_struct(StructDef {
        name: slot(OWNER).to_string(),
        base: None,
        abstract_: false,
        simple: true,
        fields: vec![field(HALF, ty)],
        source_path: String::new(),
    });
    let mut inst = MetaInstance::new(OWNER);
    inst.set(HALF, Value::Float(value));
    (lib, inst)
}

fn bytes_that_moved(ty: FieldType, from: f64, to: f64) -> (Vec<u8>, Vec<u8>, Vec<usize>) {
    let (lib, low) = alone(ty.clone(), from);
    let (_, high) = alone(ty, to);
    let a = encode(&lib, &low);
    let b = encode(&lib, &high);
    assert_eq!(a.len(), b.len(), "the value moved an allocation");
    let at = a
        .iter()
        .zip(&b)
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| i)
        .collect();
    (a, b, at)
}

/// Two documents differing in one `Float16` differ in exactly the two bytes
/// that member occupies, and those bytes are its half-precision pattern.
///
/// ⚠ `0.1` and `1.0` are chosen because their halves differ in **both**
/// bytes. `0.0` against `1.0` is `0x0000` against `0x3C00` and moves one.
#[test]
fn the_member_occupies_the_two_bytes_a_half_is_kept_in() {
    let (a, b, at) = bytes_that_moved(FieldType::Float16, 0.1, 1.0);
    assert_eq!(
        at.len(),
        2,
        "a sixteen-bit member moved {} bytes at {at:?}",
        at.len()
    );
    assert_eq!(at[1], at[0] + 1, "the two bytes are not adjacent");
    assert_eq!([a[at[0]], a[at[1]]], f32_to_half(0.1).to_be_bytes());
    assert_eq!([b[at[0]], b[at[1]]], f32_to_half(1.0).to_be_bytes());

    // The same two values through the same encoder, declared as an ordinary
    // float, move twice as many — so the width is the declaration's and not
    // the value's.
    let (_, _, wide) = bytes_that_moved(FieldType::Float, 0.1, 1.0);
    assert_eq!(wide.len(), 4, "an f32 moved {} bytes", wide.len());

    // And the type table the layout was synthesised into agrees about which
    // member kind that is, so the width is not an accident of alignment.
    let (lib, one) = alone(FieldType::Float16, 1.0);
    let layout = pso_layout_for_instance(&lib, &one).expect("synthesise");
    let entry = layout
        .schema
        .struct_for(name_hash(OWNER))
        .expect("the root is in the type table")
        .entries
        .iter()
        .find(|e| e.name_hash == name_hash(HALF))
        .expect("the member is in the type table");
    assert_eq!(entry.data_type, PsoDataType::Float16);
}

/// The XML says the number, and what comes back encodes to the same bytes.
#[test]
fn the_xml_leg_is_a_fixed_point_and_carries_a_number() {
    let lib = library();
    let mut exercised = 0usize;
    let mut discriminating = 0usize;

    for value in cases() {
        let first = encode(&lib, &document(value));
        let xml = write_meta(&decode(&first)).expect("export");
        let reimported = parse_meta(&lib, xml.as_bytes()).expect("import");
        let second = encode(&lib, &reimported);
        assert_eq!(
            first, second,
            "{value} did not survive the export/import leg"
        );

        // A bits-writing export gives the stored integer for every value here
        // that is not zero; the text has to be the number instead.
        let bits = f32_to_half(value as f32);
        if bits != 0 && bits != 0x8000 {
            assert!(
                !xml.contains(&format!(">{bits}<")),
                "the export wrote the stored bits rather than the value: {xml}"
            );
            discriminating += 1;
        }
        exercised += 1;
    }

    assert_eq!(
        exercised,
        cases().len(),
        "{exercised} of {} cases reached the assertion",
        cases().len()
    );
    assert!(exercised >= 15, "too few cases to tell two readings apart");

    // ⚠ The bits check is conditional, so it is the one that can stop running
    // while the loop still reaches every case. A zero half writes `0` either
    // way and tells the two readings apart from nothing.
    assert!(
        discriminating * 2 > exercised,
        "{discriminating} of {exercised} cases had a half a bits-writing export \
         could be caught on, which is too few to catch one"
    );
}
