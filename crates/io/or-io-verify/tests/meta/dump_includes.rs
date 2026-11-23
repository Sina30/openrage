use crate::support;

use or_io::meta::{MetaInstance, Value};

fn arr<'a>(i: &'a MetaInstance, n: &str) -> Option<&'a Vec<Value>> {
    i.fields.iter().find_map(|(k, v)| match v {
        Value::Array(a) if k == n => Some(a),
        _ => None,
    })
}

#[test]
#[ignore]
fn dump() {
    let path = std::env::var("OR_YMT").expect("OR_YMT");
    let bytes = std::fs::read(&path).unwrap();
    let i = or_io::meta::binary::parse_meta_binary(support::schema(), &bytes).unwrap();
    println!("root {}", i.type_name);
    if let Some(inc) = arr(&i, "includedDataFiles") {
        println!("includedDataFiles ({}):", inc.len());
        for v in inc {
            println!("  {v:?}");
        }
    }
    if let Some(incx) = arr(&i, "includedXmlFiles") {
        println!("includedXmlFiles ({} blocks)", incx.len());
    }
    if let Some(df) = arr(&i, "dataFiles") {
        for item in df {
            if let Value::Struct(s) = item {
                if let Some(Value::String(fname)) = s.get("filename") {
                    let low = fname.to_ascii_lowercase();
                    if low.contains("gta5")
                        || low.contains("common.meta")
                        || low.contains("/levels")
                    {
                        println!("  DF {fname}  {:?}", s.get("fileType"));
                    }
                }
            }
        }
    }
}
