//! The canonical form on documents this file declares, schema included, so a
//! clean checkout with no snapshot and no corpus still exercises it.
//!
//! ⚠ **Every struct and field name here is one
//! [`or_io::meta::binary::public_names`] carries.** The readers name a hash
//! through the default index whatever library they are handed, so a name the
//! index cannot give back reads as `0xHHHHHHHH`, exports under that tag, and
//! is dropped on import as a member the schema does not declare. The types
//! are this file's own and say nothing about what the game declares.

use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::canonical::{self, converge, converge_with, NotCanonical};
use or_io::meta::instance::{parse_meta, HashedString};
use or_io::meta::{build_library, BinaryFormat, Diagnostics, MetaInstance, SchemaLibrary, Value};

/// `CBlockDesc::exportedBy` is sized symbolically, the way 83 members of the
/// real schema are, so its length has to come from the document.
const PSC: &str = r#"<?xml version="1.0"?>
<ParserSchema>
  <structdef type="CEntityDef">
    <string name="archetypeName" type="atHashString"/>
    <u32 name="flags"/>
    <Vector3 name="position"/>
    <Vector4 name="rotation"/>
    <float name="scaleXY"/>
    <s32 name="parentIndex"/>
    <Color32 name="tintValue"/>
  </structdef>
  <structdef type="CMloInstanceDef" base="CEntityDef">
    <u32 name="groupId"/>
  </structdef>
  <structdef type="CBlockDesc">
    <string name="name" type="atString"/>
    <array name="exportedBy" type="atFixedArray" size="EXPORTER_NAME_LENGTH"><u8/></array>
  </structdef>
  <structdef type="OccludeModel">
    <u32 name="dataSize"/>
    <array name="verts" type="atArray"><u8/></array>
  </structdef>
  <structdef type="CMapData">
    <string name="name" type="atHashString"/>
    <u32 name="contentFlags"/>
    <array name="entities" type="atArray"><pointer type="CEntityDef" policy="owner"/></array>
    <struct name="block" type="CBlockDesc"/>
  </structdef>
</ParserSchema>"#;

fn library() -> SchemaLibrary {
    build_library([("inline.psc", PSC.as_bytes())]).expect("the inline schema parses")
}

fn entity(archetype: &str, tint: u32) -> MetaInstance {
    let mut e = MetaInstance::new("CEntityDef");
    e.set("archetypeName", Value::Hash(HashedString::new(archetype)));
    e.set("flags", Value::UInt(32));
    e.set("position", Value::Vec3([1.0, -2.5, 3.25]));
    e.set(
        "rotation",
        Value::Vec4([0.0, 0.0, 0.707_106_77, 0.707_106_77]),
    );
    e.set("scaleXY", Value::Float(1.0));
    e.set("parentIndex", Value::Int(-1));
    e.set("tintValue", Value::UInt(u64::from(tint)));
    e
}

fn document() -> MetaInstance {
    let mut interior = entity("v_int_1", 0);
    interior.type_name = "CMloInstanceDef".into();
    interior.set("groupId", Value::UInt(7));

    let mut block = MetaInstance::new("CBlockDesc");
    block.set("name", Value::String("a block".into()));
    block.set(
        "exportedBy",
        Value::Array((1..=3).map(|b| Value::UInt(b * 10)).collect()),
    );

    let mut root = MetaInstance::new("CMapData");
    root.set("name", Value::Hash(HashedString::new("cs1_02")));
    root.set("contentFlags", Value::UInt(65));
    root.set(
        "entities",
        Value::Array(vec![
            Value::Pointer(Some(Box::new(entity("prop_bin_01a", 0xFF80_4020)))),
            Value::Pointer(Some(Box::new(interior))),
        ]),
    );
    root.set("block", Value::Struct(Box::new(block)));
    root
}

fn encode(library: &SchemaLibrary, format: BinaryFormat) -> Vec<u8> {
    let root = document();
    let layout = canonical::layout_for(library, &root, format, None).expect("lays out");
    write_meta_binary(format, Some(&layout), &root).expect("encodes")
}

fn read(bytes: &[u8]) -> MetaInstance {
    let mut diags = Diagnostics::lenient();
    parse_meta_binary_full(&SchemaLibrary::default(), bytes, &mut diags)
        .expect("reads")
        .0
}

fn entities(root: &MetaInstance) -> &[Value] {
    match root.get("entities") {
        Some(Value::Array(items)) => items,
        other => panic!("entities is {other:?}"),
    }
}

fn converges_as(format: BinaryFormat) {
    let library = library();
    let converged =
        converge(&library, &encode(&library, format)).unwrap_or_else(|e| panic!("{format:?}: {e}"));
    assert!(
        !converged.values_moved,
        "{format:?}: the first cycle lost a value"
    );

    // The fixed point is only worth having if the values made it into it.
    let back = read(&converged.bytes);
    let items = entities(&back);
    let Some(Value::Pointer(Some(first))) = items.first() else {
        panic!("{items:?}")
    };
    assert_eq!(first.get("tintValue"), Some(&Value::UInt(0xFF80_4020)));
    let Some(Value::Pointer(Some(second))) = items.get(1) else {
        panic!("{items:?}")
    };
    assert_eq!(second.type_name, "CMloInstanceDef", "the pointer's target");
    assert_eq!(second.get("groupId"), Some(&Value::UInt(7)));
    let Some(Value::Struct(block)) = back.get("block") else {
        panic!("{back:?}")
    };
    assert_eq!(
        block.get("exportedBy"),
        Some(&Value::Array(vec![
            Value::UInt(10),
            Value::UInt(20),
            Value::UInt(30)
        ]))
    );
}

#[test]
fn a_declared_pso_document_reaches_its_fixed_point() {
    converges_as(BinaryFormat::Pso);
}

#[test]
fn a_declared_rbm_document_reaches_its_fixed_point() {
    converges_as(BinaryFormat::Rbm);
}

const RBF_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CCreatureMetaData>
  <pedCompExpressions>
    <Item>
      <pedCompID value="6"/>
      <tracks content="char_array">33 1</tracks>
      <ids content="short_array">28462</ids>
      <fxOffsetRot x="0.0" y="0.5" z="0.0" w="1.0"/>
    </Item>
  </pedCompExpressions>
</CCreatureMetaData>
"#;

/// ⚠ RBF needs no schema at all, so it converges on a library with nothing
/// in it — and its XML is what the RBF encodes, byte for byte.
#[test]
fn an_rbf_document_reaches_its_fixed_point_with_no_schema() {
    let empty = SchemaLibrary::default();
    assert_eq!(canonical::unavailable(&empty, BinaryFormat::Rbf), None);
    let bytes =
        canonical::import(&empty, RBF_XML.as_bytes(), BinaryFormat::Rbf, None).expect("imports");
    let converged = converge(&empty, &bytes).expect("converges");
    assert!(!converged.values_moved);
    assert_eq!(converged.bytes, bytes, "a transcription moves nothing");

    let mut diags = Diagnostics::lenient();
    let (root, layout) = parse_meta_binary_full(&empty, &bytes, &mut diags).expect("reads");
    assert_eq!(canonical::export(&root, &layout).unwrap(), RBF_XML);
}

/// The snapshot is gitignored, so this is a clean checkout: an import with
/// nothing to take structure from says why, a document converges on its own
/// type table, and a root no table declares is refused by name.
#[test]
fn with_no_schema_the_structure_comes_from_the_documents_type_table() {
    let empty = SchemaLibrary::default();
    for format in [BinaryFormat::Pso, BinaryFormat::Rbm] {
        let why = canonical::unavailable(&empty, format).expect("refused");
        assert!(why.contains("schema snapshot"), "{why}");
        let err = canonical::import(&empty, b"<CMapData/>", format, None).expect_err("no donor");
        assert!(err.to_string().contains("schema snapshot"), "{err}");

        let converged = converge(&empty, &encode(&library(), format))
            .unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert!(!converged.values_moved, "{format:?}");

        assert_eq!(read(&converged.bytes).type_name, "CMapData");
        let mut diags = Diagnostics::lenient();
        let (_, layout) =
            parse_meta_binary_full(&empty, &converged.bytes, &mut diags).expect("reads");
        let err = canonical::import(&empty, b"<CNotAThing/>", format, Some(&layout))
            .expect_err("a root no table declares");
        assert!(err.to_string().contains("CNotAThing"), "{format:?}: {err}");
        let xml = br#"<CMapData><notAMember value="1"/></CMapData>"#;
        let err = canonical::import(&empty, xml, format, Some(&layout))
            .expect_err("a member no table declares");
        assert!(
            err.to_string().contains("CMapData.notAMember"),
            "{format:?}: {err}"
        );
    }
}

/// ⚠ A reader hands a pointer-to-bytes member back as one run of bytes, and
/// the export writes that as text. An import that looked only for child
/// elements emptied every `OccludeModel::verts` in both retail installs.
#[test]
fn a_byte_array_exported_as_text_imports_as_its_bytes() {
    let mut model = MetaInstance::new("OccludeModel");
    model.set("dataSize", Value::UInt(3));
    model.set("verts", Value::ByteArray(vec![2, 0, 140]));
    let xml = or_io::meta::write_meta(&model).unwrap();
    assert!(xml.contains("char_array"), "{xml}");

    let back = parse_meta(&library(), xml.as_bytes()).expect("imports");
    assert_eq!(
        back.get("verts"),
        Some(&Value::Array(vec![
            Value::UInt(2),
            Value::UInt(0),
            Value::UInt(140)
        ]))
    );
}

const OCCLUDER_PSC: &str = r#"<?xml version="1.0"?>
<ParserSchema>
  <structdef type="rage::OccludeModel" name="OccludeModel">
    <Vec3V name="m_bmin"/>
    <u32 name="m_dataSize"/>
    <array name="m_verts" type="pointer" sizeVar="m_dataSize" align="16"><u8/></array>
    <u16 name="m_numTris"/>
  </structdef>
  <structdef type="CMapData">
    <string name="name" type="atHashString"/>
    <array name="occludeModels" type="atArray"><struct type="rage::OccludeModel"/></array>
  </structdef>
</ParserSchema>"#;

fn occluders(verts: &[&[u8]]) -> MetaInstance {
    let mut root = MetaInstance::new("CMapData");
    root.set("name", Value::Hash(HashedString::new("cs1_occl")));
    let models = verts
        .iter()
        .map(|bytes| {
            let mut m = MetaInstance::new("rage::OccludeModel");
            m.set("bmin", Value::Vec3([1.0, 2.0, 3.0]));
            m.set("dataSize", Value::UInt(bytes.len() as u64));
            m.set("verts", Value::ByteArray(bytes.to_vec()));
            m.set("numTris", Value::UInt(32770));
            Value::Struct(Box::new(m))
        })
        .collect();
    root.set("occludeModels", Value::Array(models));
    root
}

/// ⚠ All 670 occluder maps store `OccludeModel::verts` as a pointer counted by
/// `dataSize`, so an `atArray` layout gives it a header it does not have. Laid
/// out from the PSC's `type="pointer"` it is an `ArrayPointer64WithCount`
/// naming `dataSize`, aligned as `align="16"` says, each run a block of its own
/// exactly as long as it is.
#[test]
fn a_pointer_array_is_laid_out_the_way_shipped_occluder_maps_store_it() {
    use or_io::meta::binary::rbm::schema::PsoType;
    use or_io::meta::binary::synth::name_hash;
    use or_io::meta::binary::ContainerLayout;

    let library = build_library([("occl.psc", OCCLUDER_PSC.as_bytes())]).unwrap();
    let root = occluders(&[&[2, 0, 140, 7, 9], &[1, 2, 3]]);
    let layout = canonical::layout_for(&library, &root, BinaryFormat::Rbm, None).expect("lays out");
    let bytes = write_meta_binary(BinaryFormat::Rbm, Some(&layout), &root).expect("encodes");

    let converged = converge(&library, &bytes).expect("converges");
    assert!(!converged.values_moved, "the first cycle lost a value");
    assert!(
        converged.reshaped.is_empty(),
        "stored as another kind of value: {:?}",
        converged.reshaped
    );

    let mut diags = Diagnostics::lenient();
    let (back, layout) =
        parse_meta_binary_full(&SchemaLibrary::default(), &converged.bytes, &mut diags).unwrap();
    let ContainerLayout::Rbm(rbm) = &layout.container else {
        panic!("not RBM")
    };
    let class = rbm
        .model
        .classes
        .values()
        .find(|c| c.entries.iter().any(|e| e.name_hash == name_hash("verts")))
        .expect("a class holds verts");
    let verts = class
        .entries
        .iter()
        .find(|e| e.name_hash == name_hash("verts"))
        .unwrap();
    assert_eq!(verts.data_type, PsoType::ArrayPointer64WithCount);
    assert_eq!(
        class.entries[verts.array_count() as usize].name_hash,
        name_hash("dataSize"),
        "the count is the member the PSC's sizeVar names"
    );
    assert_eq!(verts.array_align_power(), 4, "align=\"16\" as a power");

    let runs: Vec<usize> = rbm
        .model
        .data_blocks
        .iter()
        .filter(|b| b.data_type == u32::from(PsoType::U8.to_byte()))
        .map(|b| b.data.len())
        .collect();
    assert_eq!(runs, [5, 3], "one block per run, each exactly its length");

    let Some(Value::Array(models)) = back.get("occludeModels") else {
        panic!("{back:?}")
    };
    let Some(Value::Struct(first)) = models.first() else {
        panic!("{models:?}")
    };
    assert_eq!(
        first.get("verts"),
        Some(&Value::ByteArray(vec![2, 0, 140, 7, 9]))
    );
}

/// PSO has no encoding for a pointer array, so synthesis says which member
/// asked for one rather than laying out something the reader cannot read.
#[test]
fn a_pointer_array_is_refused_as_pso_by_name() {
    let library = build_library([("occl.psc", OCCLUDER_PSC.as_bytes())]).unwrap();
    let err = canonical::layout_for(&library, &occluders(&[&[1]]), BinaryFormat::Pso, None)
        .expect_err("no PSO encoding");
    assert!(err.to_string().contains("OccludeModel.verts"), "{err}");
}

/// An array whose elements are arrays, and a map keyed by attribute — the two
/// shapes the PSC parser typed `Unknown` in 18 corpus documents.
const NESTED_PSC: &str = r#"<?xml version="1.0"?>
<ParserSchema>
  <structdef type="CBlockDesc">
    <u32 name="flags"/>
  </structdef>
  <structdef type="CMapData">
    <string name="name" type="atHashString"/>
    <array name="instances" type="member" size="2">
      <array type="atArray"><float/></array>
    </array>
    <map name="dependencies" type="atBinaryMap" key="atHashString">
      <struct type="CBlockDesc"/>
    </map>
  </structdef>
</ParserSchema>"#;

fn nested(with_map: bool) -> MetaInstance {
    let floats = |v: &[f64]| Value::Array(v.iter().map(|f| Value::Float(*f)).collect());
    let mut root = MetaInstance::new("CMapData");
    root.set("name", Value::Hash(HashedString::new("cs1_nested")));
    root.set(
        "instances",
        Value::Array(vec![floats(&[1.5, -2.0]), floats(&[0.25])]),
    );
    let pairs = if with_map {
        let block = |flags: u64| {
            let mut b = MetaInstance::new("CBlockDesc");
            b.set("flags", Value::UInt(flags));
            Value::Struct(Box::new(b))
        };
        vec![
            (Value::Hash(HashedString::new("prop_bin_01a")), block(3)),
            (Value::Hash(HashedString::new("v_int_1")), block(9)),
        ]
    } else {
        Vec::new()
    };
    root.set("dependencies", Value::Map(pairs));
    root
}

fn nested_converges_as(format: BinaryFormat, with_map: bool) {
    let library = build_library([("nested.psc", NESTED_PSC.as_bytes())]).unwrap();
    let root = nested(with_map);
    let layout = canonical::layout_for(&library, &root, format, None)
        .unwrap_or_else(|e| panic!("{format:?} lays out: {e}"));
    let bytes = write_meta_binary(format, Some(&layout), &root).expect("encodes");
    let converged = converge(&library, &bytes).unwrap_or_else(|e| panic!("{format:?}: {e}"));
    assert!(
        !converged.values_moved,
        "{format:?}: the first cycle lost a value"
    );

    let back = read(&converged.bytes);
    let Some(Value::Array(runs)) = back.get("instances") else {
        panic!("{back:?}")
    };
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1], Value::Array(vec![Value::Float(0.25)]));
    let Some(Value::Map(pairs)) = back.get("dependencies") else {
        panic!("{back:?}")
    };
    assert_eq!(pairs.len(), if with_map { 2 } else { 0 });
}

#[test]
fn an_array_of_arrays_and_a_map_converge_as_pso() {
    nested_converges_as(BinaryFormat::Pso, true);
}

/// ⚠ Empty map: the RBM writer refuses a populated `atBinaryMap`, because
/// none of the 6,071 corpus documents holds one to encode against.
#[test]
fn an_array_of_arrays_and_a_map_converge_as_rbm() {
    nested_converges_as(BinaryFormat::Rbm, false);
}

/// ⚠ **Enhanced adds a member to `CEntityDef` that the PSC corpus does not
/// declare**, and an import typed by the PSC alone dropped it from 20,376
/// world files. [`or_io::meta::shipped`] supplies it — but only to a document
/// that carries it, so a Legacy document imports exactly as before.
#[test]
fn an_enhanced_member_survives_and_a_legacy_document_is_untouched() {
    let library = library();
    for format in [BinaryFormat::Pso, BinaryFormat::Rbm] {
        let enhanced_types = or_io::meta::shipped::extend(&library, true);
        let mut root = document();
        if let Some(Value::Array(items)) = root.get_mut("entities") {
            for (i, item) in items.iter_mut().enumerate() {
                if let Value::Pointer(Some(e)) = item {
                    e.set("hash_73B8AA83", Value::UInt(1000 + i as u64));
                }
            }
        }
        let layout = canonical::layout_for(&enhanced_types, &root, format, None).unwrap();
        let bytes = write_meta_binary(format, Some(&layout), &root).unwrap();

        let converged = converge(&library, &bytes).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert!(
            !converged.values_moved,
            "{format:?}: dropped {:?}",
            converged.undeclared
        );
        let back = read(&converged.bytes);
        let Some(Value::Pointer(Some(second))) = entities(&back).get(1) else {
            panic!("{back:?}")
        };
        assert_eq!(second.type_name, "CMloInstanceDef");
        assert_eq!(second.get("hash_73B8AA83"), Some(&Value::UInt(1001)));

        // Legacy: the same import as a library with no shipped definitions.
        let legacy = document();
        let xml = or_io::meta::write_meta(&legacy).unwrap();
        let through_overlay = canonical::import(&library, xml.as_bytes(), format, None).unwrap();
        let parsed = parse_meta(&library, xml.as_bytes()).unwrap();
        let bare = canonical::layout_for(&library, &parsed, format, None).unwrap();
        assert_eq!(
            through_overlay,
            write_meta_binary(format, Some(&bare), &parsed).unwrap(),
            "{format:?}: a document carrying no Enhanced member moved"
        );
    }
}

#[test]
fn a_refusal_names_the_element_it_stopped_at() {
    let library = library();
    for format in [BinaryFormat::Pso, BinaryFormat::Rbm] {
        let mut root = document();
        let layout = canonical::layout_for(&library, &root, format, None).unwrap();
        if let Some(Value::Array(items)) = root.get_mut("entities") {
            if let Some(Value::Pointer(Some(e))) = items.get_mut(1) {
                e.set("flags", Value::String("not a number".into()));
            }
        }
        let err = write_meta_binary(format, Some(&layout), &root).expect_err("a string in a u32");
        assert!(
            err.to_string()
                .starts_with("CMapData.entities[1].flags: expected an integer"),
            "{format:?}: {err}"
        );
    }
}

/// An import that loses one entity per cycle, which is the shape of defect
/// the fixed point exists to catch. Without it red, a green run says nothing.
#[test]
fn an_import_that_keeps_losing_a_value_is_not_a_fixed_point() {
    let library = library();
    let bytes = encode(&library, BinaryFormat::Pso);
    let lossy = |xml: &[u8], format: BinaryFormat| {
        let mut root = parse_meta(&library, xml)?;
        if let Some(Value::Array(items)) = root.get_mut("entities") {
            items.pop();
        }
        let layout = canonical::layout_for(&library, &root, format, None)?;
        write_meta_binary(format, Some(&layout), &root)
    };
    let err = converge_with(&library, &bytes, lossy).expect_err("it keeps moving");
    assert!(matches!(err, NotCanonical::Moved { .. }), "{err}");
}

/// And one that loses a value once, on the first cycle only, converges —
/// which is why the result says so.
#[test]
fn a_value_lost_on_the_first_cycle_is_reported_even_though_it_converges() {
    let library = library();
    let bytes = encode(&library, BinaryFormat::Pso);
    let lose_the_tint = |xml: &[u8], format: BinaryFormat| {
        let mut root = parse_meta(&library, xml)?;
        if let Some(Value::Array(items)) = root.get_mut("entities") {
            for item in items {
                if let Value::Pointer(Some(e)) = item {
                    e.set("tintValue", Value::UInt(0));
                }
            }
        }
        let layout = canonical::layout_for(&library, &root, format, None)?;
        write_meta_binary(format, Some(&layout), &root)
    };
    let converged = converge_with(&library, &bytes, lose_the_tint).expect("converges");
    assert!(converged.values_moved, "a lost value went unreported");
    assert!(
        converged.undeclared.is_empty(),
        "and nothing undeclared can take the blame for it: {:?}",
        converged.undeclared
    );
}
