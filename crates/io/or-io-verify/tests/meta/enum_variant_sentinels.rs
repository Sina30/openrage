//! A variant's name and a variant's value must not spell the same.
//!
//! ⚠ When the reader wrote `0xHHHHHHHH` both for the *name hash* of a variant
//! it could not name and for the *value* of one the file's enum table did not
//! list, the writer inverted the first as if it were the second: a field
//! holding 3 was saved as `0xAB34CAAD`, the literal hash of `AT_FRG`, and RBM's
//! `Enum8` narrowed that to `0xAD`. 3,176 of 6,072 real fixtures failed on it.
//!
//! ⚠ **Everything here passes with no schema snapshot staged**, the only state
//! the defect appears in: a snapshot resolves the name, so a regression test
//! that needed one would go quiet the moment somebody staged it.
//!
//! The layout is built by hand rather than synthesised so the test controls the
//! name hashes: `binary::synth` keys names by the case-folding `string_hash`
//! where PSO stores the literal one.

use crate::support;

use or_io::meta::binary::enum_text;
use or_io::meta::binary::layout::{ContainerLayout, MetaLayout, PsoLayout};
use or_io::meta::binary::pso::schema::{
    PsoDataType, PsoEnumEntry, PsoEnumInfo, PsoSchemaSection, PsoStructEntry, PsoStructInfo,
};
use or_io::meta::binary::pso::sections::{PmapEntry, PmapHeaderKind, PmapSection, PsoSection};
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::instance::{parse_meta, write_meta};
use or_io::meta::{
    BinaryFormat, Diagnostics, EnumDef, EnumValue, Field, FieldHint, FieldType, MetaInstance,
    SchemaLibrary, StructDef, Value,
};

const AT_FRG: &str = "AT_FRG";
const AT_FRG_HASH: u32 = 0xAB34_CAAD;
const AT_FRG_VALUE: i32 = 3;

const UNNAMED_VALUE: i32 = 0x63;

const STRUCT_NAME: &str = "COrTestAssetBinding";
const ENUM_NAME: &str = "EOrTestAssetType";
const FIELD_NAMED: &str = "orTestAssetType";
const FIELD_UNNAMED: &str = "orTestSpareType";

fn h(name: &str) -> u32 {
    or_io::literal_string_hash(name.as_bytes())
}

fn field_key(name: &str) -> String {
    support::hash_index().name_or_literal(h(name))
}

fn test_layout() -> MetaLayout {
    let mut schema = PsoSchemaSection::default();
    schema.insert_enum(PsoEnumInfo {
        name_hash: h(ENUM_NAME),
        flags: 0,
        entries: vec![
            PsoEnumEntry {
                name_hash: h("AT_TXD"),
                value: 0,
            },
            PsoEnumEntry {
                name_hash: h(AT_FRG),
                value: AT_FRG_VALUE,
            },
        ],
    });
    schema.insert_struct(PsoStructInfo {
        name_hash: h(STRUCT_NAME),
        flags: 0,
        data_size: 8,
        major_version: 0,
        pad: 0,
        entries: vec![
            PsoStructEntry::new(h(FIELD_NAMED), PsoDataType::Enum, 0, 0, h(ENUM_NAME)),
            PsoStructEntry::new(h(FIELD_UNNAMED), PsoDataType::Enum, 4, 0, h(ENUM_NAME)),
        ],
    });

    let psin_len = 16usize;
    let mut psin = vec![0u8; psin_len];
    psin[..4].copy_from_slice(b"PSIN");
    psin[4..8].copy_from_slice(&(psin_len as u32).to_be_bytes());

    MetaLayout {
        format: BinaryFormat::Pso,
        container: ContainerLayout::Pso(PsoLayout {
            schema,
            pmap: PmapSection {
                root_id: 1,
                header_kind: PmapHeaderKind::Legacy { unk_0eh: 0 },
                entries: vec![PmapEntry {
                    name_hash: h(STRUCT_NAME),
                    offset: 8,
                    unknown: 0,
                    length: 8,
                }],
            },
            psin,
            opaque_sections: Vec::new(),
            section_order: vec![PsoSection::Psin, PsoSection::Pmap, PsoSection::Psch],
        }),
    }
}

fn instance_with(named: &str, unnamed: &str) -> MetaInstance {
    let mut inst = MetaInstance::new(field_key(STRUCT_NAME));
    inst.set(
        field_key(FIELD_NAMED),
        Value::Enum {
            type_name: ENUM_NAME.into(),
            variant: named.into(),
        },
    );
    inst.set(
        field_key(FIELD_UNNAMED),
        Value::Enum {
            type_name: ENUM_NAME.into(),
            variant: unnamed.into(),
        },
    );
    inst
}

fn stored_values(bytes: &[u8]) -> (i32, i32) {
    let at = |o: usize| i32::from_be_bytes(bytes[o..o + 4].try_into().unwrap());
    (at(8), at(12))
}

fn variant_of(inst: &MetaInstance, field: &str) -> String {
    match inst.get(&field_key(field)) {
        Some(Value::Enum { variant, .. }) => variant.clone(),
        other => panic!("expected an enum for {field}, found {other:?}"),
    }
}

#[test]
fn a_named_variant_is_written_as_its_value() {
    let layout = test_layout();
    let inst = instance_with(AT_FRG, &enum_text::value_text(UNNAMED_VALUE));
    let bytes = write_meta_binary(BinaryFormat::Pso, Some(&layout), &inst).expect("write");

    let (named, unnamed) = stored_values(&bytes);
    assert_eq!(
        named, AT_FRG_VALUE,
        "AT_FRG must be stored as 3, not as {AT_FRG_HASH:#010X}"
    );
    assert_eq!(unnamed, UNNAMED_VALUE);
}

#[test]
fn the_name_placeholder_inverts_through_the_files_own_table() {
    let layout = test_layout();
    let placeholder = format!("hash_{AT_FRG_HASH:08X}");
    let inst = instance_with(&placeholder, &enum_text::value_text(UNNAMED_VALUE));
    let bytes = write_meta_binary(BinaryFormat::Pso, Some(&layout), &inst).expect("write");
    assert_eq!(stored_values(&bytes).0, AT_FRG_VALUE);
}

/// Read → write on a file the reader produced: the fixture gate in miniature.
#[test]
fn a_file_holding_a_named_variant_round_trips() {
    let layout = test_layout();
    let original = write_meta_binary(
        BinaryFormat::Pso,
        Some(&layout),
        &instance_with(AT_FRG, &enum_text::value_text(UNNAMED_VALUE)),
    )
    .expect("write");

    let mut diags = Diagnostics::lenient();
    let (back, read_layout) =
        parse_meta_binary_full(support::schema(), &original, &mut diags).expect("re-read");

    let named = variant_of(&back, FIELD_NAMED);
    let expected_named = if support::hash_index().lookup(AT_FRG_HASH).is_some() {
        AT_FRG.to_string()
    } else {
        format!("hash_{AT_FRG_HASH:08X}")
    };
    assert_eq!(named, expected_named);
    assert_eq!(
        variant_of(&back, FIELD_UNNAMED),
        enum_text::value_text(UNNAMED_VALUE),
        "a value the table does not name keeps the 0x spelling"
    );
    assert_ne!(
        named,
        enum_text::value_text(AT_FRG_HASH as i32),
        "a name hash and a value must not read back the same"
    );

    let rewritten =
        write_meta_binary(BinaryFormat::Pso, Some(&read_layout), &back).expect("re-write");
    assert_eq!(rewritten, original, "read → write changed the file");
    assert_eq!(stored_values(&rewritten), (AT_FRG_VALUE, UNNAMED_VALUE));
}

#[test]
fn the_low_byte_of_a_name_hash_is_not_the_value() {
    assert_eq!(AT_FRG_HASH as u8, 0xAD);
    assert_ne!(AT_FRG_HASH as u8 as i32, AT_FRG_VALUE);
    let table = [(AT_FRG_HASH, AT_FRG_VALUE)];
    assert_eq!(
        enum_text::resolve_named(&format!("hash_{AT_FRG_HASH:08X}"), table),
        Some(AT_FRG_VALUE)
    );
    assert_eq!(enum_text::resolve_named(AT_FRG, table), Some(AT_FRG_VALUE));
}

#[test]
fn the_xml_leg_round_trips_the_variant_rather_than_a_number() {
    let lib = xml_library();
    for spelling in [AT_FRG.to_string(), format!("hash_{AT_FRG_HASH:08X}")] {
        let mut inst = MetaInstance::new(STRUCT_NAME);
        inst.set(
            FIELD_NAMED,
            Value::Enum {
                type_name: ENUM_NAME.into(),
                variant: spelling.clone(),
            },
        );

        let xml = write_meta(&inst).expect("write xml");
        assert!(
            xml.contains(&format!(">{spelling}<")),
            "XML lost the variant: {xml}"
        );
        assert!(
            !xml.contains("0xAB34CAAD"),
            "XML spelled a name as a value: {xml}"
        );

        let back = parse_meta(&lib, xml.as_bytes()).expect("parse xml");
        assert_eq!(
            back.get(FIELD_NAMED),
            Some(&Value::Enum {
                type_name: ENUM_NAME.into(),
                variant: spelling.clone(),
            })
        );

        let Some(Value::Enum { variant, .. }) = back.get(FIELD_NAMED) else {
            unreachable!()
        };
        assert_eq!(
            enum_text::resolve_named(variant, [(AT_FRG_HASH, AT_FRG_VALUE)]),
            Some(AT_FRG_VALUE),
            "'{variant}' did not invert to the value"
        );
    }
}

fn xml_library() -> SchemaLibrary {
    let mut lib = SchemaLibrary::new();
    lib.insert_enum(EnumDef {
        name: ENUM_NAME.into(),
        values: vec![
            EnumValue {
                name: "AT_TXD".into(),
                description: None,
                value: Some(0),
            },
            EnumValue {
                name: AT_FRG.into(),
                description: None,
                value: Some(AT_FRG_VALUE as i64),
            },
        ],
        generate_bitset: false,
        source_path: "test".into(),
    });
    lib.insert_struct(StructDef {
        name: STRUCT_NAME.into(),
        base: None,
        abstract_: false,
        simple: true,
        fields: vec![Field {
            name: FIELD_NAMED.into(),
            ty: FieldType::Enum(ENUM_NAME.into()),
            init: None,
            min: None,
            max: None,
            step: None,
            ui_key: false,
            hide_widgets: false,
            description: None,
            hint: FieldHint::None,
        }],
        source_path: "test".into(),
    });
    lib
}
