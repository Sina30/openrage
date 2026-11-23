//! Metadata no schema declares: XML opens as the tree the file states, edits
//! and saves back as XML, and a binary `.srl` reads its frames and saves
//! byte-identical.
//!
//! ⚠ The shipped cases are install-gated: `OPENRAGE_GAME_ROOT` names the
//! installs, `;`-separated, and each install's own executable supplies the
//! keys for its `common.rpf`.

use std::path::Path;
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::RpfArchive;
use or_io::hash::string_hash;
use or_io::meta::{srl, MetaDocument, MetaInstance, SchemaLibrary, SourceFormat, Value};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// A root that is an `Item` array, scalars spelled each way the dialect has, a nested array and a typed pointer.
const DATA_FILES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>

<dataFiles>
  <Item>
    <filename>platform:/levels/gta5/minimap.rpf</filename>
    <contents>CONTENTS_MAP</contents>
    <persistent value="true"/>
    <loadCompletely value="1"/>
    <scale value="0.500000"/>
    <offset x="1.000000" y="2.000000" z="3.000000"/>
    <locked/>
  </Item>
  <Item>
    <filename>platform:/levels/gta5/navmeshes0.rpf</filename>
    <requests>
      <Item>first &amp; second</Item>
      <Item>hash_DEADBEEF</Item>
    </requests>
    <effect type="CRainEffect">
      <name>rain</name>
    </effect>
  </Item>
</dataFiles>
"#;

fn open(bytes: &[u8]) -> MetaDocument {
    MetaDocument::parse(&SchemaLibrary::default(), bytes.to_vec())
        .unwrap_or_else(|e| panic!("opens with no schema: {e}"))
}

fn item(root: &MetaInstance, index: usize) -> &MetaInstance {
    match &root.fields[index] {
        (name, Value::Struct(item)) if name == "Item" => item,
        other => panic!("field {index} is {other:?}"),
    }
}

#[test]
fn xml_whose_root_type_no_schema_declares_opens_as_the_tree_it_states() {
    let doc = open(DATA_FILES.as_bytes());
    let root = &doc.instance;
    assert_eq!(root.type_name, "dataFiles");
    assert_eq!(root.fields.len(), 2);

    let first = item(root, 0);
    for (name, value) in [
        (
            "filename",
            Value::String("platform:/levels/gta5/minimap.rpf".into()),
        ),
        ("contents", Value::String("CONTENTS_MAP".into())),
        ("persistent", Value::Bool(true)),
        ("loadCompletely", Value::Int(1)),
        ("scale", Value::Float(0.5)),
        ("offset", Value::Vec3([1.0, 2.0, 3.0])),
        ("locked", Value::String(String::new())),
    ] {
        assert_eq!(first.get(name), Some(&value), "{name}");
    }

    let second = item(root, 1);
    assert_eq!(
        second.get("requests"),
        Some(&Value::Array(vec![
            Value::String("first & second".into()),
            Value::String("hash_DEADBEEF".into()),
        ]))
    );
    let Some(Value::Pointer(Some(effect))) = second.get("effect") else {
        panic!("effect is {:?}", second.get("effect"));
    };
    assert_eq!(effect.type_name, "CRainEffect");
    assert_eq!(effect.get("name"), Some(&Value::String("rain".into())));
}

#[test]
fn an_edit_to_schemaless_xml_saves_as_xml_and_reads_back() {
    let mut doc = open(DATA_FILES.as_bytes());
    let Value::Struct(first) = &mut doc.instance.fields[0].1 else {
        panic!("the first item is not a struct");
    };
    first.set("contents", Value::String("CONTENTS_PROPS".into()));
    first.set("scale", Value::Float(2.25));

    let saved = doc.to_bytes().expect("saves");
    let text = std::str::from_utf8(&saved).expect("XML");
    assert!(
        text.contains("<contents>CONTENTS_PROPS</contents>"),
        "{text}"
    );
    assert_eq!(open(&saved).instance, doc.instance, "{text}");
}

#[test]
fn xml_the_tree_cannot_hold_is_refused_by_name() {
    let error = MetaDocument::parse(
        &SchemaLibrary::default(),
        br#"<stats><stat Name="CASH" Type="int"/></stats>"#.to_vec(),
    )
    .expect_err("an attribute the dialect has no value for is refused");
    assert!(error.to_string().contains("<stat>"), "{error}");
}

/// Every entry of the install's `common.rpf` whose path ends with one of `suffixes`.
fn shipped(root: &Path, suffixes: &[&str]) -> Vec<(String, Vec<u8>)> {
    let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let archive = root.join("common.rpf");
    let opened = RpfArchive::open_with(
        &archive,
        Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys))),
    )
    .unwrap_or_else(|e| panic!("{}: {e}", archive.display()));
    opened
        .nodes
        .iter()
        .filter(|n| !n.is_dir())
        .filter(|n| {
            let path = n.path().to_ascii_lowercase();
            suffixes.iter().any(|s| path.ends_with(s))
        })
        .map(|node| {
            let bytes = opened
                .read_entry(node.entry_index())
                .unwrap_or_else(|e| panic!("common.rpf/{}: {e}", node.path()));
            (node.path().to_string(), bytes)
        })
        .collect()
}

#[test]
fn the_shipped_files_no_schema_declares_open_without_one() {
    let what = "images.meta and weather.xml open with no schema";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let files = shipped(
            &root,
            &["levels/gta5/images.meta", "levels/gta5/weather.xml"],
        );
        assert_eq!(files.len(), 2, "{}: {files:?}", root.display());
        for (path, bytes) in files {
            let doc = MetaDocument::parse(&SchemaLibrary::default(), bytes)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            eprintln!(
                "{path}: <{}> with {} fields",
                doc.instance.type_name,
                doc.instance.fields.len()
            );
            assert!(!doc.instance.fields.is_empty(), "{path}: no fields");
            let saved = doc.to_bytes().expect("saves");
            assert_eq!(
                open(&saved).instance,
                doc.instance,
                "{path}: the saved XML reads back differently"
            );
        }
    }
}

/// Four big-endian counted arrays: requested names, each frame's first index into them, then the same for released ones.
fn binary_srl(arrays: [&[u32]; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    for array in arrays {
        out.extend_from_slice(&(array.len() as u32).to_be_bytes());
        for w in array {
            out.extend_from_slice(&w.to_be_bytes());
        }
    }
    out
}

fn hashes(list: Option<&Value>) -> Vec<u32> {
    match list {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Hash(h) => h.hash,
                other => panic!("{other:?} is not a name"),
            })
            .collect(),
        other => panic!("{other:?} is not a list"),
    }
}

#[test]
fn a_binary_srl_reads_its_frames_and_saves_byte_identical() {
    let [a, b, c] = [b"prop_a", b"prop_b", b"prop_c"].map(|n| string_hash(n));
    let bytes = binary_srl([&[a, b, c], &[0, 2, 2], &[a], &[0, 0, 0]]);
    let doc = open(&bytes);
    assert_eq!(doc.source_format, SourceFormat::Srl);
    assert_eq!(doc.instance.type_name, srl::RECORD);
    let Some(Value::Array(frames)) = doc.instance.get("Frames") else {
        panic!("no frames: {:?}", doc.instance);
    };
    let lists: Vec<(Vec<u32>, Vec<u32>)> = frames
        .iter()
        .map(|frame| match frame {
            Value::Struct(f) => (hashes(f.get("AddList")), hashes(f.get("RemoveList"))),
            other => panic!("{other:?} is not a frame"),
        })
        .collect();
    assert_eq!(
        lists,
        vec![(vec![a, b], vec![]), (vec![], vec![]), (vec![c], vec![a])]
    );
    assert_eq!(doc.to_bytes().expect("saves"), bytes);
}

#[test]
fn a_binary_srl_edited_as_xml_saves_the_edit() {
    let a = string_hash(b"prop_a");
    let bytes = binary_srl([&[a], &[0], &[], &[0]]);
    let mut doc = open(&bytes);
    let xml = or_io::meta::write_meta(&doc.instance).expect("XML");
    assert!(xml.contains(&format!("hash_{a:08X}")), "{xml}");
    doc.instance = open(xml.replace(&format!("hash_{a:08X}"), "prop_b").as_bytes()).instance;
    assert_eq!(
        doc.to_bytes().expect("saves"),
        binary_srl([&[string_hash(b"prop_b")], &[0], &[], &[0]])
    );
}

#[test]
fn a_binary_srl_that_is_not_the_whole_file_is_refused() {
    let mut bytes = binary_srl([&[1, 2], &[0, 1], &[], &[0, 0]]);
    bytes.push(0);
    assert!(srl::read(&bytes).is_err(), "a trailing byte");
    let bytes = binary_srl([&[1, 2], &[0, 3], &[], &[0, 0]]);
    assert!(srl::read(&bytes).is_err(), "a frame past the names");
    let bytes = binary_srl([&[1, 2], &[1], &[], &[0]]);
    assert!(srl::read(&bytes).is_err(), "a name before the first frame");
}

#[test]
fn every_shipped_srl_opens_and_a_binary_one_saves_byte_identical() {
    let what = "every .srl in common.rpf opens";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let files = shipped(&root, &[".srl"]);
        assert!(!files.is_empty(), "{}: no .srl", root.display());
        let mut binary = 0;
        for (path, bytes) in &files {
            let doc = MetaDocument::parse(&SchemaLibrary::default(), bytes.clone())
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            let frames = match doc.instance.get("Frames") {
                Some(Value::Array(frames)) => frames.len(),
                other => panic!("{path}: frames are {other:?}"),
            };
            eprintln!("{path}: {} with {frames} frames", doc.source_format.label());
            if doc.source_format == SourceFormat::Srl {
                binary += 1;
                assert!(
                    doc.to_bytes().expect("saves") == *bytes,
                    "{path}: the save differs"
                );
            }
        }
        assert!(binary > 0, "{}: no binary .srl", root.display());
    }
}
