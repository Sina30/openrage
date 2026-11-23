//! A binary metadata file cut short is refused with an error, and one with a
//! byte wrong reads or is refused, never with a panic.
//!
//! Each read runs under `catch_unwind`, so one panic fails the sweep naming
//! the prefix length or the corrupted offset rather than aborting the binary.
//! ⚠ A stack overflow or a failed allocation still aborts it: both are what
//! a struct that contains itself and a capacity taken from a corrupt count
//! did before the readers refused them.
//!
//! ⚠ An RBM is an RSC7, and a byte changed in its compressed stream almost
//! always stops at inflate. Its corruptions are made in the decompressed
//! heap and resealed, which is what reaches the metadata reader, and every
//! reseal deflates the heap again, so it takes four values rather than all.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Once;

use or_io::container::resource::{Compression, ResourceEnvelope, Rsc7};
use or_io::meta::binary::parse_meta_binary_full;
use or_io::meta::canonical;
use or_io::meta::instance::HashedString;
use or_io::meta::{
    build_library, detect_binary_format, write_meta_binary, BinaryFormat, Diagnostics,
    MetaInstance, SchemaLibrary, Value,
};

/// Every name here is one the default hash index carries, so the reader
/// resolves it and the synthetic file decodes to the tree it was written from.
const PSC: &str = r#"<?xml version="1.0"?>
<ParserSchema>
  <structdef type="CEntityDef">
    <string name="archetypeName" type="atHashString"/>
    <u32 name="flags"/>
    <Vector3 name="position"/>
    <Vector4 name="rotation"/>
    <float name="scaleXY"/>
    <s32 name="parentIndex"/>
  </structdef>
  <structdef type="CMloInstanceDef" base="CEntityDef">
    <u32 name="groupId"/>
  </structdef>
  <structdef type="CBlockDesc">
    <string name="name" type="atString"/>
    <array name="exportedBy" type="atFixedArray" size="4"><u8/></array>
  </structdef>
  <structdef type="CMapData">
    <string name="name" type="atHashString"/>
    <u32 name="contentFlags"/>
    <array name="entities" type="atArray"><pointer type="CEntityDef" policy="owner"/></array>
    <struct name="block" type="CBlockDesc"/>
  </structdef>
</ParserSchema>"#;

const RBF_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CCreatureMetaData>
  <pedCompExpressions>
    <Item>
      <pedCompID value="6"/>
      <name>a string</name>
      <tracks content="char_array">33 1</tracks>
      <ids content="short_array">28462 7</ids>
      <offset x="0.0" y="0.5" z="1.0"/>
      <enabled value="true"/>
      <scale value="0.25"/>
    </Item>
    <Item>
      <pedCompID value="7"/>
    </Item>
  </pedCompExpressions>
</CCreatureMetaData>
"#;

/// The byte values each offset of an RBM heap is overwritten with in turn; a
/// PSO or RBF offset takes every value.
const RBM_CORRUPTIONS: [u8; 4] = [0x00, 0x7F, 0x80, 0xFF];

fn entity(archetype: &str) -> MetaInstance {
    let mut e = MetaInstance::new("CEntityDef");
    e.set("archetypeName", Value::Hash(HashedString::new(archetype)));
    e.set("flags", Value::UInt(32));
    e.set("position", Value::Vec3([1.0, -2.5, 3.25]));
    e.set("rotation", Value::Vec4([0.0, 0.0, 0.5, 1.0]));
    e.set("scaleXY", Value::Float(1.0));
    e.set("parentIndex", Value::Int(-1));
    e
}

fn synthetic(format: BinaryFormat) -> Vec<u8> {
    if format == BinaryFormat::Rbf {
        let empty = SchemaLibrary::default();
        return canonical::import(&empty, RBF_XML.as_bytes(), format, None).expect("imports");
    }
    let library = build_library([("inline.psc", PSC.as_bytes())]).expect("the schema parses");
    let mut interior = entity("v_int_1");
    interior.type_name = "CMloInstanceDef".into();
    interior.set("groupId", Value::UInt(7));
    let mut block = MetaInstance::new("CBlockDesc");
    block.set("name", Value::String("a block".into()));
    block.set(
        "exportedBy",
        Value::Array((1..=4).map(|b| Value::UInt(b * 10)).collect()),
    );
    let mut root = MetaInstance::new("CMapData");
    root.set("name", Value::Hash(HashedString::new("cs1_02")));
    root.set("contentFlags", Value::UInt(65));
    root.set(
        "entities",
        Value::Array(vec![
            Value::Pointer(Some(Box::new(entity("prop_bin_01a")))),
            Value::Pointer(Some(Box::new(interior))),
        ]),
    );
    root.set("block", Value::Struct(Box::new(block)));
    let layout = canonical::layout_for(&library, &root, format, None).expect("lays out");
    write_meta_binary(format, Some(&layout), &root).expect("encodes")
}

thread_local! {
    static SWEEPING: Cell<bool> = const { Cell::new(false) };
    static LOCATION: Cell<Option<String>> = const { Cell::new(None) };
}

/// Silences the panic message of a read under sweep and keeps its location.
fn install_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if SWEEPING.with(Cell::get) {
                LOCATION.with(|l| l.set(info.location().map(|l| l.to_string())));
            } else {
                default(info);
            }
        }));
    });
}

/// The tree a lenient read gives, `None` when it is refused, after a strict
/// read of the same bytes; `Err` is a panic's message and location.
fn read(bytes: &[u8]) -> Result<Option<MetaInstance>, String> {
    install_hook();
    SWEEPING.with(|s| s.set(true));
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        let library = SchemaLibrary::default();
        let _ = parse_meta_binary_full(&library, bytes, &mut Diagnostics::strict());
        parse_meta_binary_full(&library, bytes, &mut Diagnostics::lenient())
            .ok()
            .map(|(instance, _)| instance)
    }));
    SWEEPING.with(|s| s.set(false));
    outcome.map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = LOCATION.with(|l| l.take()).unwrap_or_default();
        format!("{message} at {at}")
    })
}

/// Every strict prefix of `bytes` and its single-byte corruptions, with what
/// went wrong on each.
///
/// ⚠ A prefix may read when what it drops holds none of the document: a
/// PSO's trailing `PSIG` section, or an RSC7's compressed tail of zero
/// pages. It must then read as the whole file does.
fn sweep(label: &str, bytes: &[u8]) -> Vec<String> {
    let mut failures = Vec::new();
    let whole = match read(bytes) {
        Ok(Some(whole)) => whole,
        other => panic!("{label}: the whole file does not read: {other:?}"),
    };

    for len in 0..bytes.len() {
        match read(&bytes[..len]) {
            Ok(None) => {}
            Ok(Some(tree)) if tree == whole => {}
            Ok(Some(_)) => failures.push(format!("{label}: the {len}-byte prefix reads")),
            Err(panic) => failures.push(format!("{label}: the {len}-byte prefix panics: {panic}")),
        }
    }

    let rbm = (detect_binary_format(bytes) == Some(BinaryFormat::Rbm)).then(|| {
        let resource = Rsc7::open(bytes).expect("the RBM opens as RSC7");
        (resource.data, resource.seal)
    });
    let target = rbm.as_ref().map_or(bytes, |(heap, _)| heap.as_slice());
    let values: Vec<u8> = match rbm {
        Some(_) => RBM_CORRUPTIONS.to_vec(),
        None => (0..=u8::MAX).collect(),
    };
    for offset in 0..target.len() {
        for &value in &values {
            if target[offset] == value {
                continue;
            }
            let mut corrupt = target.to_vec();
            corrupt[offset] = value;
            if let Some((_, seal)) = &rbm {
                corrupt = Rsc7::reseal(&corrupt, seal, Compression::fast()).expect("reseals");
            }
            if let Err(panic) = read(&corrupt) {
                failures.push(format!(
                    "{label}: {value:#04X} at {offset:#X} panics: {panic}"
                ));
            }
        }
    }
    failures
}

fn report(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} failure(s), the first 20:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn a_cut_or_corrupted_rbf_file_is_refused_without_a_panic() {
    report(&sweep("RBF", &synthetic(BinaryFormat::Rbf)));
}

#[test]
fn a_cut_or_corrupted_pso_file_is_refused_without_a_panic() {
    report(&sweep("PSO", &synthetic(BinaryFormat::Pso)));
}

#[test]
fn a_cut_or_corrupted_rbm_file_is_refused_without_a_panic() {
    report(&sweep("RBM", &synthetic(BinaryFormat::Rbm)));
}

/// The smallest staged file of `format` that reads.
fn smallest_staged(format: BinaryFormat) -> Option<(PathBuf, Vec<u8>)> {
    let origin = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    let mut files: Vec<(u64, PathBuf)> = ["ymt", "ymf", "ymap", "ytyp", "pso"]
        .into_iter()
        .flat_map(|ext| {
            or_test_support::Corpus::with_root(origin.path(), ext)
                .all_targets()
                .find()
        })
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.len(), path)))
        .collect();
    files.sort();
    files.into_iter().find_map(|(_, path)| {
        let bytes = std::fs::read(&path).ok()?;
        (detect_binary_format(&bytes) == Some(format) && matches!(read(&bytes), Ok(Some(_))))
            .then_some((path, bytes))
    })
}

#[test]
fn a_cut_or_corrupted_staged_file_is_refused_without_a_panic() {
    let mut failures = Vec::new();
    let mut swept = 0;
    for format in [BinaryFormat::Rbf, BinaryFormat::Pso, BinaryFormat::Rbm] {
        let Some((path, bytes)) = smallest_staged(format) else {
            continue;
        };
        swept += 1;
        failures.extend(sweep(&path.display().to_string(), &bytes));
    }
    if swept == 0 {
        or_test_support::skip_corpus(
            "a_cut_or_corrupted_staged_file_is_refused_without_a_panic",
            "no binary metadata staged",
        );
        return;
    }
    report(&failures);
}
