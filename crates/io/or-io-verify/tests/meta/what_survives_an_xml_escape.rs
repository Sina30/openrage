//! Whether a value the XML writer had to escape comes back as itself.
//!
//! ⚠ Every test above `no_corpus_value_loses_an_escape` declares its own
//! schema so that it cannot skip: every other XML suite here skips when no
//! schema snapshot is staged, as in a clean checkout and on CI.

use crate::corpus;
use crate::support;

use or_io::meta::instance::{
    parse_meta, write_meta, HashedString, MetaInstance, UnknownNode, Value,
};
use or_io::meta::schema::{Field, FieldHint, FieldType, SchemaLibrary, StringKind, StructDef};

/// Values chosen so the writer must escape something. The last two are real
/// leaves from the staged corpus; `&#38;` is there because a literal that
/// looks like a character reference must survive one round trip unchanged,
/// which a reader that unescapes twice would fail.
const CASES: &[&str] = &[
    "Zapp & Roger",
    "<",
    "a<b>c",
    "tom & jerry & co",
    "&",
    "&&",
    "<>",
    "x&<>y",
    "a]]>b",
    "&#38;",
    "\"quoted\" and 'apostrophe'",
    "Dan Houser &",
    "Ñejo & Dalmata",
];

const ROOT: &str = "T";
const TEXT: &str = "Text";
const ITEMS: &str = "Items";

fn field(name: &str, ty: FieldType) -> Field {
    Field {
        name: name.to_string(),
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

fn library() -> SchemaLibrary {
    let mut lib = SchemaLibrary::new();
    lib.insert_struct(StructDef {
        name: ROOT.to_string(),
        base: None,
        abstract_: false,
        simple: true,
        fields: vec![
            field(TEXT, FieldType::String(StringKind::Plain)),
            field("Hashed", FieldType::String(StringKind::AtHashString)),
            field("Opaque", FieldType::Unknown("something".to_string())),
            field(
                ITEMS,
                FieldType::Array {
                    elem: Box::new(FieldType::String(StringKind::Plain)),
                    kind: or_io::meta::schema::ArrayKind::AtArray,
                },
            ),
        ],
        source_path: String::new(),
    });
    lib
}

fn one_field(name: &str, value: Value) -> MetaInstance {
    let mut inst = MetaInstance::new(ROOT);
    inst.set(name.to_string(), value);
    inst
}

/// Write an instance and read it back through the same library.
fn round_trip(inst: &MetaInstance) -> (String, MetaInstance) {
    let lib = library();
    let xml = write_meta(inst).expect("write");
    let back = parse_meta(&lib, xml.as_bytes()).unwrap_or_else(|e| panic!("parse {xml:?}: {e}"));
    (xml, back)
}

/// How many `&name;` references a document actually carries.
///
/// The guard against a vacuous pass: if the writer stopped escaping, every
/// assertion below would still hold and none of them would be about entity
/// references any more.
fn entity_references(xml: &str) -> usize {
    xml.match_indices('&')
        .filter(|(i, _)| {
            xml[i + 1..]
                .split(';')
                .next()
                .is_some_and(|n| !n.is_empty())
        })
        .count()
}

#[test]
fn every_escaped_value_comes_back_as_itself() {
    let mut checked = 0usize;
    let mut references = 0usize;

    for original in CASES {
        let original = original.to_string();

        // A plain string, which is what the 40 differing corpus leaves are.
        let (xml, back) = round_trip(&one_field(TEXT, Value::String(original.clone())));
        references += entity_references(&xml);
        assert_eq!(
            back.get(TEXT),
            Some(&Value::String(original.clone())),
            "plain string, from {xml:?}"
        );
        checked += 1;

        // A hashed string: the text is the payload and the hash is derived
        // from it, so a lost character changes both.
        let hashed = HashedString::new(original.clone());
        let (xml, back) = round_trip(&one_field("Hashed", Value::Hash(hashed.clone())));
        references += entity_references(&xml);
        assert_eq!(
            back.get("Hashed"),
            Some(&Value::Hash(hashed)),
            "hashed string, from {xml:?}"
        );
        checked += 1;

        // An unknown field type, which carries its text verbatim.
        let (xml, back) = round_trip(&one_field("Opaque", Value::Raw(original.clone())));
        references += entity_references(&xml);
        assert_eq!(
            back.get("Opaque"),
            Some(&Value::Raw(original.clone())),
            "raw, from {xml:?}"
        );
        checked += 1;

        // An array element, which reaches the text loop through `read_array`.
        let items = Value::Array(vec![Value::String(original.clone())]);
        let (xml, back) = round_trip(&one_field(ITEMS, items.clone()));
        references += entity_references(&xml);
        assert_eq!(back.get(ITEMS), Some(&items), "array item, from {xml:?}");
        checked += 1;

        // A node no field matches, kept as an extra. Its text and its
        // attributes take two different paths through the reader.
        let mut inst = MetaInstance::new(ROOT);
        inst.extras.push(UnknownNode {
            tag: "NotAField".to_string(),
            attrs: vec![("note".to_string(), original.clone())],
            text: original.clone(),
            children: Vec::new(),
        });
        let (xml, back) = round_trip(&inst);
        references += entity_references(&xml);
        let extra = back.extras.first().expect("extra survived");
        assert_eq!(extra.text, original, "extra text, from {xml:?}");
        assert_eq!(
            extra.attrs.first().map(|(_, v)| v.as_str()),
            Some(original.as_str()),
            "extra attribute, from {xml:?}"
        );
        checked += 2;
    }

    println!("checked {checked} value round trips carrying {references} entity references");
    assert_eq!(
        checked,
        CASES.len() * 6,
        "every case must reach every field kind"
    );
    assert!(
        references >= CASES.len(),
        "the writer emitted {references} entity references over {} cases, so these assertions \
         are no longer about escaping",
        CASES.len()
    );
}

/// A numeric character reference is a separate quick-xml event from a named
/// one and resolves through a different call, so it needs its own case. The
/// writer never emits one; hand-authored `.meta` does.
#[test]
fn a_numeric_character_reference_resolves_in_text_and_in_an_attribute() {
    let lib = library();
    let mut checked = 0usize;

    for ch in ['&', '<', '>', '"', '\'', 'Ñ', '\u{1F600}'] {
        let decimal = format!("&#{};", ch as u32);
        let hexadecimal = format!("&#x{:X};", ch as u32);
        let expected = format!("a{ch}b{ch}c");

        let doc = format!("<{ROOT}><{TEXT}>a{decimal}b{hexadecimal}c</{TEXT}></{ROOT}>");
        let inst = parse_meta(&lib, doc.as_bytes()).unwrap_or_else(|e| panic!("{doc}: {e}"));
        assert_eq!(
            inst.get(TEXT),
            Some(&Value::String(expected.clone())),
            "text, from {doc}"
        );
        checked += 1;

        let doc = format!("<{ROOT}><{TEXT} value=\"a{decimal}b{hexadecimal}c\"/></{ROOT}>");
        let inst = parse_meta(&lib, doc.as_bytes()).unwrap_or_else(|e| panic!("{doc}: {e}"));
        assert_eq!(
            inst.get(TEXT),
            Some(&Value::String(expected)),
            "attribute, from {doc}"
        );
        checked += 1;
    }

    println!("checked {checked} character references");
    assert_eq!(checked, 14, "two spellings in two places for seven scalars");
}

/// ⚠ **CDATA is the one place the escapes are not escapes.** Its content is
/// literal by definition, so a reader that unescapes it turns `&amp;` in a
/// hand-authored document into `&` and loses the distinction for good.
#[test]
fn cdata_content_is_literal_and_survives_a_section_split() {
    let lib = library();
    let mut checked = 0usize;

    for literal in ["&amp; &lt; &#38;", "a < b & c", "a]]>b"] {
        // The only way XML can carry `]]>` inside CDATA is to end the section
        // and open another, so the document is derived from the value.
        let sections = literal.replace("]]>", "]]]]><![CDATA[>");
        let doc = format!("<{ROOT}><{TEXT}><![CDATA[{sections}]]></{TEXT}></{ROOT}>");
        let inst = parse_meta(&lib, doc.as_bytes()).unwrap_or_else(|e| panic!("{doc}: {e}"));
        assert_eq!(
            inst.get(TEXT),
            Some(&Value::String(literal.to_string())),
            "from {doc}"
        );
        checked += 1;
    }

    println!("checked {checked} CDATA sections");
    assert_eq!(checked, 3);
}

/// An entity the reader cannot resolve keeps its own spelling, neither dropped
/// nor guessed at.
#[test]
fn an_unresolvable_entity_keeps_its_source_spelling() {
    let lib = library();
    let mut checked = 0usize;

    for name in ["nbsp", "eacute", "someProjectEntity"] {
        let reference = format!("&{name};");
        let expected = format!("a{reference}b");
        let doc = format!("<{ROOT}><{TEXT}>a{reference}b</{TEXT}></{ROOT}>");
        let inst = parse_meta(&lib, doc.as_bytes()).unwrap_or_else(|e| panic!("{doc}: {e}"));
        assert_eq!(inst.get(TEXT), Some(&Value::String(expected)), "from {doc}");
        checked += 1;
    }

    println!("checked {checked} unresolvable entities");
    assert_eq!(checked, 3);
}

fn text_leaves(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) | Value::Raw(s) => out.push(s.clone()),
        Value::Hash(h) => out.push(h.raw.clone()),
        Value::Enum { variant, .. } => out.push(variant.clone()),
        Value::Array(items) => items.iter().for_each(|v| text_leaves(v, out)),
        Value::Map(pairs) => pairs.iter().for_each(|(k, v)| {
            text_leaves(k, out);
            text_leaves(v, out);
        }),
        Value::Struct(inner) | Value::Pointer(Some(inner)) => {
            inner.fields.iter().for_each(|(_, v)| text_leaves(v, out))
        }
        _ => {}
    }
}

/// The same property over the staged corpus: radio track credits carry `&`,
/// and two dictionary entries are a bare `<` and a bare `>`.
///
/// One document's leaves are live at a time: the corpus is 6.8 million string
/// leaves.
#[test]
fn no_corpus_value_loses_an_escape() {
    let staged = corpus::Corpus::load();
    require_corpus!(staged);

    let binary_lib = support::schema();
    let mut parsed = 0usize;
    let mut leaves = 0usize;
    let mut exercised = 0usize;
    let mut lost = 0usize;
    let mut examples: Vec<String> = Vec::new();

    for fixture in staged.iter() {
        let mut diags = or_io::meta::Diagnostics::lenient();
        let Ok(inst) =
            or_io::meta::binary::parse_meta_binary_with(binary_lib, &fixture.bytes, &mut diags)
        else {
            continue;
        };
        parsed += 1;

        let mut values = Vec::new();
        inst.fields
            .iter()
            .for_each(|(_, v)| text_leaves(v, &mut values));
        for value in values {
            leaves += 1;
            if !value.contains(['&', '<', '>']) {
                continue;
            }
            exercised += 1;
            let (xml, back) = round_trip(&one_field(TEXT, Value::String(value.clone())));
            if back.get(TEXT) != Some(&Value::String(value.clone())) {
                lost += 1;
                if examples.len() < 10 {
                    examples.push(format!("{}: {value:?} from {xml:?}", fixture.name));
                }
            }
        }
    }

    println!(
        "parsed {parsed} of {} staged documents, {leaves} text leaves, {exercised} needing an escape",
        staged.len()
    );
    assert!(
        parsed > 0,
        "{} documents staged and none parsed, so nothing was checked",
        staged.len()
    );
    assert_eq!(
        lost,
        0,
        "{lost} of {exercised} corpus values lost characters through the XML round trip:\n  {}",
        examples.join("\n  ")
    );
    if exercised == 0 {
        or_test_support::skip_corpus(
            "no_corpus_value_loses_an_escape",
            format_args!(
                "none of {leaves} text leaves across {parsed} documents contained a character \
                 the writer escapes, so this asserted nothing about escaping"
            ),
        );
    }
}
