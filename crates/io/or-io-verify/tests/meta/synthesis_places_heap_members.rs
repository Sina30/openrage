//! Synthesis, on documents this file builds, so a clean checkout with no
//! corpus still exercises it.
//!
//! ⚠ Every name here is one [`or_io::meta::binary::public_names`] carries, and
//! [`slot`] asserts it. The PSO writer looks a field up by the name the hash
//! index gives back for its hash, so a name the index cannot name is a field
//! the writer cannot find — and the failure is "missing field", which reads
//! like a bug in the document rather than in the test.

use crate::support;

use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::pso::schema::PsoDataType;
use or_io::meta::binary::synth::{name_hash, pso_layout_for_instance};
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::instance::{parse_meta, HashedString};
use or_io::meta::{
    write_meta, ArrayKind, BinaryFormat, Diagnostics, Field, FieldHint, FieldType, MetaInstance,
    PointerPolicy, SchemaLibrary, StringKind, StructDef, Value,
};

/// A name the hash index can give back, which is what the writer looks a
/// field up under.
fn slot(name: &'static str) -> &'static str {
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

fn struct_def(name: &'static str, fields: Vec<Field>) -> StructDef {
    StructDef {
        name: slot(name).to_string(),
        base: None,
        abstract_: false,
        simple: true,
        fields,
        source_path: String::new(),
    }
}

fn heap_array(elem: FieldType) -> FieldType {
    FieldType::Array {
        elem: Box::new(elem),
        kind: ArrayKind::AtArray,
    }
}

/// `CMapData` holding one of every member kind synthesis had to learn.
fn library() -> SchemaLibrary {
    let mut lib = SchemaLibrary::new();
    lib.insert_struct(struct_def(
        "CTimeCycleModifier",
        vec![
            field("Name", FieldType::String(StringKind::AtString)),
            field("Radius", FieldType::Float),
        ],
    ));
    lib.insert_struct(struct_def(
        "CExtensionDefDoor",
        vec![field("audioHash", FieldType::U32)],
    ));
    lib.insert_struct(struct_def(
        "CExtensionDefLadder",
        vec![field("bottom", FieldType::Vector3)],
    ));
    lib.insert_struct(struct_def(
        "CMapData",
        vec![
            field("contentFlags", FieldType::U32),
            field("archetypeName", FieldType::String(StringKind::Plain)),
            field(
                "entities",
                heap_array(FieldType::Struct("CTimeCycleModifier".into())),
            ),
            field("dependencies", heap_array(FieldType::U32)),
            field(
                "extensions",
                heap_array(FieldType::Pointer {
                    base: "CExtensionDefDoor".into(),
                    policy: PointerPolicy::Owner,
                }),
            ),
            field(
                "entitySets",
                FieldType::Map {
                    key: Box::new(FieldType::String(StringKind::AtHashString)),
                    value: Box::new(FieldType::U32),
                },
            ),
        ],
    ));
    lib
}

fn modifier(name: &str, radius: f64) -> Value {
    let mut inst = MetaInstance::new("CTimeCycleModifier");
    inst.set("Name", Value::String(name.into()));
    inst.set("Radius", Value::Float(radius));
    Value::Struct(Box::new(inst))
}

fn document() -> MetaInstance {
    let mut door = MetaInstance::new("CExtensionDefDoor");
    door.set("audioHash", Value::UInt(0x1234_5678));
    let mut ladder = MetaInstance::new("CExtensionDefLadder");
    ladder.set("bottom", Value::Vec3([1.0, 2.0, 3.0]));

    let mut root = MetaInstance::new("CMapData");
    root.set("contentFlags", Value::UInt(9));
    root.set("archetypeName", Value::String("a heap string".into()));
    root.set(
        "entities",
        Value::Array(vec![modifier("first", 1.5), modifier("second", 2.5)]),
    );
    root.set(
        "dependencies",
        Value::Array(vec![Value::UInt(7), Value::UInt(8), Value::UInt(9)]),
    );
    root.set(
        "extensions",
        Value::Array(vec![
            Value::Pointer(Some(Box::new(door))),
            Value::Pointer(Some(Box::new(ladder))),
        ]),
    );
    root.set(
        "entitySets",
        Value::Map(vec![(
            Value::Hash(HashedString::new("Name")),
            Value::UInt(4),
        )]),
    );
    root
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
    let (inst, _) =
        parse_meta_binary_full(support::schema(), bytes, &mut diags).expect("re-read synthesised");
    assert!(
        diags.is_clean(),
        "gaps re-reading a synthesised document: {:?}",
        diags.gaps()
    );
    inst
}

#[test]
fn every_heap_member_survives_the_document_it_was_synthesised_for() {
    let lib = library();
    let root = document();
    let back = decode(&encode(&lib, &root));

    assert_eq!(back.get("contentFlags"), Some(&Value::UInt(9)));
    assert_eq!(
        back.get("archetypeName"),
        Some(&Value::String("a heap string".into()))
    );
    assert_eq!(
        back.get("dependencies"),
        Some(&Value::Array(vec![
            Value::UInt(7),
            Value::UInt(8),
            Value::UInt(9)
        ]))
    );

    let Some(Value::Array(entities)) = back.get("entities") else {
        panic!("entities came back as {:?}", back.get("entities"));
    };
    assert_eq!(entities.len(), 2);
    let Value::Struct(second) = &entities[1] else {
        panic!("element came back as {:?}", entities[1]);
    };
    assert_eq!(second.get("Name"), Some(&Value::String("second".into())));
    assert_eq!(second.get("Radius"), Some(&Value::Float(2.5)));

    let Some(Value::Map(pairs)) = back.get("entitySets") else {
        panic!("entitySets came back as {:?}", back.get("entitySets"));
    };
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].1, Value::UInt(4));
}

/// ⚠ The concrete type of a pointer's target is the one thing a field
/// declaration cannot supply, which is why the XML keeps `type=` on a
/// pointer and only on a pointer.
#[test]
fn a_pointer_reaches_the_type_the_value_had_rather_than_the_one_declared() {
    let lib = library();
    let back = decode(&encode(&lib, &document()));

    let Some(Value::Array(extensions)) = back.get("extensions") else {
        panic!("extensions came back as {:?}", back.get("extensions"));
    };
    assert_eq!(extensions.len(), 2);
    let names: Vec<&str> = extensions
        .iter()
        .map(|v| match v {
            Value::Pointer(Some(inner)) => inner.type_name.as_str(),
            other => panic!("element came back as {other:?}"),
        })
        .collect();
    assert_eq!(names, vec!["CExtensionDefDoor", "CExtensionDefLadder"]);

    let xml = write_meta(&back).expect("export");
    assert!(
        xml.contains("type=\"CExtensionDefLadder\""),
        "the concrete type left the document: {xml}"
    );
    let reimported = parse_meta(&lib, xml.as_bytes()).expect("import");
    let Some(Value::Array(items)) = reimported.get("extensions") else {
        panic!("extensions did not come back from XML");
    };
    let Value::Pointer(Some(ladder)) = &items[1] else {
        panic!("pointer did not come back from XML: {:?}", items[1]);
    };
    assert_eq!(ladder.type_name, "CExtensionDefLadder");
    assert_eq!(ladder.get("bottom"), Some(&Value::Vec3([1.0, 2.0, 3.0])));
}

#[test]
fn an_empty_array_and_an_empty_string_stay_empty() {
    let lib = library();
    let mut root = MetaInstance::new("CMapData");
    root.set("contentFlags", Value::UInt(0));
    root.set("archetypeName", Value::String(String::new()));
    root.set("entities", Value::Array(Vec::new()));
    root.set("dependencies", Value::Array(Vec::new()));
    root.set("extensions", Value::Array(Vec::new()));
    root.set("entitySets", Value::Map(Vec::new()));

    let back = decode(&encode(&lib, &root));
    assert_eq!(
        back.get("archetypeName"),
        Some(&Value::String(String::new()))
    );
    assert_eq!(back.get("entities"), Some(&Value::Array(Vec::new())));
    assert_eq!(back.get("entitySets"), Some(&Value::Map(Vec::new())));
}

#[test]
fn the_same_value_tree_synthesises_to_the_same_bytes() {
    let lib = library();
    let root = document();
    assert_eq!(encode(&lib, &root), encode(&lib, &root));

    // And a tree built a second time from scratch, in case anything cached an
    // allocation from the first.
    assert_eq!(encode(&library(), &document()), encode(&lib, &root));
}

/// A document built from scratch is a fixed point of export and import, with
/// no corpus and no donor anywhere in the loop.
#[test]
fn a_hand_built_document_is_a_fixed_point_of_export_and_import() {
    let lib = library();

    let first = {
        let xml = write_meta(&document()).expect("export");
        let reimported = parse_meta(&lib, xml.as_bytes()).expect("import");
        encode(&lib, &reimported)
    };
    let second = {
        let xml = write_meta(&decode(&first)).expect("export");
        let reimported = parse_meta(&lib, xml.as_bytes()).expect("import");
        encode(&lib, &reimported)
    };
    assert_eq!(first, second, "a second pass moved the bytes");
}

/// ⚠ An array one element longer than the document had, in a container that
/// reserves room by count. Growing is re-synthesising rather than patching:
/// the layout is derived from the tree, so a tree with one more element gets
/// one more slot and every offset after it moves.
#[test]
fn an_array_can_grow_by_re_synthesising_the_layout() {
    let lib = library();
    let mut root = document();

    let Some(Value::Array(entities)) = root.get_mut("entities") else {
        panic!("the fixture lost its array");
    };
    entities.push(modifier("third", 3.5));

    let back = decode(&encode(&lib, &root));
    let Some(Value::Array(entities)) = back.get("entities") else {
        panic!("entities came back as {:?}", back.get("entities"));
    };
    assert_eq!(entities.len(), 3);
    let Value::Struct(third) = &entities[2] else {
        panic!("the grown element came back as {:?}", entities[2]);
    };
    assert_eq!(third.get("Name"), Some(&Value::String("third".into())));
    assert_eq!(third.get("Radius"), Some(&Value::Float(3.5)));
}

/// A member with no heap of its own is still described, so the count of
/// entries the writer walks is the count of fields the document has.
#[test]
fn the_type_table_describes_every_declared_member() {
    let lib = library();
    let layout = pso_layout_for_instance(&lib, &document()).expect("synthesise");
    let root = layout
        .schema
        .struct_for(name_hash("CMapData"))
        .expect("root in the type table");

    let declared: Vec<PsoDataType> = root
        .entries
        .iter()
        .filter(|e| e.name_hash != 0x100)
        .map(|e| e.data_type)
        .collect();
    assert_eq!(
        declared,
        vec![
            PsoDataType::UInt,
            PsoDataType::String,
            PsoDataType::Array,
            PsoDataType::Array,
            PsoDataType::Array,
            PsoDataType::Map,
        ]
    );

    // The pointer's target type is in the table because the value named it;
    // the declared base is never laid out on its own account.
    assert!(layout
        .schema
        .struct_for(name_hash("CExtensionDefLadder"))
        .is_some());
}

/// Two allocations of one type share a block, and a block a pointer resolves
/// through has to name that type.
#[test]
fn a_pointer_target_lands_in_a_block_that_names_its_type() {
    let lib = library();
    let layout = pso_layout_for_instance(&lib, &document()).expect("synthesise");

    let tags: Vec<u32> = layout.pmap.entries.iter().map(|e| e.name_hash).collect();
    assert_eq!(tags[0], name_hash("CMapData"), "the root block is first");
    assert!(
        tags.contains(&name_hash("CExtensionDefDoor")),
        "no block names the door's type: {tags:#X?}"
    );
    assert!(
        tags.contains(&name_hash("CExtensionDefLadder")),
        "no block names the ladder's type: {tags:#X?}"
    );

    let modifiers = layout
        .pmap
        .entries
        .iter()
        .filter(|e| e.name_hash == name_hash("CTimeCycleModifier"))
        .count();
    assert_eq!(modifiers, 1, "two instances of one type took two blocks");
}
