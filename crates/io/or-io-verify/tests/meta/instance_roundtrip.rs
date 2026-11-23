use crate::support;

use or_io::meta::instance::{parse_meta, write_meta};
use or_io::meta::Value;

const AMBIENT_MODEL_SET_XML: &str = include_str!("fixtures/ambient_model_set.xml");

macro_rules! require_schema {
    () => {
        match support::schema_or_skip("schema-named instance round-trip") {
            Some(lib) => lib,
            None => return,
        }
    };
}

#[test]
fn ambient_model_set_roundtrips_structurally() {
    let lib = require_schema!();
    let inst = parse_meta(lib, AMBIENT_MODEL_SET_XML.as_bytes()).expect("parse 1");
    let xml = write_meta(&inst).expect("write");
    let inst2 = parse_meta(lib, xml.as_bytes()).expect("parse 2");
    assert_eq!(inst.type_name, inst2.type_name);
    assert_eq!(inst.fields.len(), inst2.fields.len());
    for ((a_name, a_val), (b_name, b_val)) in inst.fields.iter().zip(&inst2.fields) {
        assert_eq!(a_name, b_name, "field name");
        assert!(values_match(a_val, b_val), "field {a_name}");
    }
}

#[test]
fn nested_model_set_array_decoded() {
    let lib = require_schema!();
    let inst = parse_meta(lib, AMBIENT_MODEL_SET_XML.as_bytes()).expect("parse");
    let models = inst.get("Models").expect("Models array present");
    let arr = match models {
        Value::Array(items) => items,
        other => panic!("expected array, got {other:?}"),
    };
    assert_eq!(arr.len(), 2);
}

#[test]
fn unknown_field_preserved_as_extra() {
    let lib = require_schema!();
    let xml = r#"<?xml version="1.0"?>
<CAmbientModelSet>
  <m_Name>example</m_Name>
  <m_TotallyMadeUp value="42"/>
</CAmbientModelSet>"#;
    let inst = parse_meta(lib, xml.as_bytes()).expect("parse");
    assert_eq!(inst.extras.len(), 1);
    assert_eq!(inst.extras[0].tag, "m_TotallyMadeUp");
}

fn values_match(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Float(x), Float(y)) => (x - y).abs() < 1e-6,
        (Hash(x), Hash(y)) => x.raw == y.raw,
        (Array(x), Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| values_match(a, b))
        }
        (Struct(x), Struct(y)) => {
            x.type_name == y.type_name
                && x.fields.len() == y.fields.len()
                && x.fields
                    .iter()
                    .zip(&y.fields)
                    .all(|((a_n, a_v), (b_n, b_v))| a_n == b_n && values_match(a_v, b_v))
        }
        _ => a == b,
    }
}
