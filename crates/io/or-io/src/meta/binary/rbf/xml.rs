//! RBF to XML and back, as the element tree RBF already is.
//!
//! ⚠ **RBF is binary XML, not a schema instance**, so this is a transcription
//! and never asks the schema library anything. A struct is an element, the
//! first `attribute_count` values under it are its attributes, every other
//! typed value is a `<name value="…"/>` child, and a `0xFD` payload is its
//! text. Typing the same XML against the schema instead — what the PSO and RBM
//! legs do — turns `<tracks content="char_array">33</tracks>` into a two-item
//! array and could not bring 206 of 206 staged RBF documents back.
//!
//! RBF stores five value types and XML text names which one it holds
//! without help: `true`/`false`, a run of digits, a number with a `.`, an
//! exponent, `NaN` or `inf`, three `x y z` attributes, and anything else is a
//! string. A string attribute that happens to read as a number comes back as
//! that number, which moves bytes once and then holds.

use std::collections::BTreeMap;
use std::fmt::Write;

use quick_xml::events::attributes::Attributes;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::meta::error::{MetaError, Result};
use crate::meta::instance::parser::{decode_attr, decode_utf8, local_name, resolve_general_ref};
use crate::meta::instance::value::{MetaInstance, Value};
use crate::meta::instance::writer::{escape_attr, escape_text, format_float};

use super::layout::{RbfLayout, RbfStructShape};

const CONTENT_FIELD: &str = "Content";
const CONTENT_KIND: &str = "content";
/// The processing instruction that stands for GTA VI's marker after the magic: a root attribute
/// would count toward the root's attributes.
const MARK_TARGET: &[u8] = b"rbf";
const MARK: &str = r#"marker="FB FF 01 00""#;

/// The XML an RBF document encodes.
pub fn to_xml(root: &MetaInstance, layout: &RbfLayout) -> Result<String> {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    if layout.wide_values {
        let _ = writeln!(out, "<?rbf {MARK}?>");
    }
    write_element(&mut out, root, layout, &mut BTreeMap::new(), 0)?;
    Ok(out)
}

/// The shape of the next struct named `name`: in a body with GTA VI's marker its own
/// occurrence's, counted in stream order as the writer counts them, else its tag's.
fn occurrence_shape(
    layout: &RbfLayout,
    seen: &mut BTreeMap<String, usize>,
    name: &str,
) -> RbfStructShape {
    if layout.wide_values {
        let k = seen.entry(name.to_string()).or_default();
        let own = (layout.occurrences.get(name)).and_then(|s| s.get(*k).copied());
        *k += 1;
        if let Some(own) = own {
            return own;
        }
    }
    layout.shape_for(name)
}

/// The RBF value tree and layout an XML document encodes.
pub fn from_xml(xml: &[u8]) -> Result<(MetaInstance, RbfLayout)> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut layout = RbfLayout::default();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(MetaError::Other(
                    "rbf xml: no root element found".to_string(),
                ))
            }
            Event::PI(e) if e.target() == MARK_TARGET => {
                if e.content().trim_ascii() != MARK.as_bytes() {
                    return Err(MetaError::Other(format!(
                        "rbf xml: <?rbf{}?> is not the one mark RBF states, <?rbf {MARK}?>",
                        String::from_utf8_lossy(e.content())
                    )));
                }
                layout.wide_values = true;
            }
            Event::Start(e) => {
                let tag = local_name(e.name());
                let root = read_element(&mut reader, &tag, e.attributes(), &mut layout)?;
                return finish(root, layout);
            }
            Event::Empty(e) => {
                let tag = local_name(e.name());
                let root = element_head(&tag, e.attributes(), &mut layout)?;
                return finish(root, layout);
            }
            _ => {}
        }
        buf.clear();
    }
}

fn finish(root: MetaInstance, layout: RbfLayout) -> Result<(MetaInstance, RbfLayout)> {
    // A body with GTA VI's marker states a shape per struct, so its tags may differ.
    if let (Some(tag), false) = (layout.conflicts.first(), layout.wide_values) {
        return Err(MetaError::Other(format!(
            "rbf xml: <{tag}> carries different attribute counts in one document, and RBF \
             states one per tag"
        )));
    }
    Ok((root, layout))
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

// Only a body with GTA VI's marker reads a negative literal back as an integer; any other keeps
// its 32-bit word as unsigned text, as it did.
fn attribute_text(value: &Value, wide: bool) -> Option<String> {
    Some(match value {
        Value::Bool(b) => b.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Int(v) if wide => v.to_string(),
        Value::Int(v) => (*v as u32).to_string(),
        Value::Float(v) => format_float(*v),
        Value::String(s) => s.clone(),
        _ => return None,
    })
}

fn write_element(
    out: &mut String,
    inst: &MetaInstance,
    layout: &RbfLayout,
    seen: &mut BTreeMap<String, usize>,
    depth: usize,
) -> Result<()> {
    let shape = occurrence_shape(layout, seen, &inst.type_name);
    let declared = usize::try_from(shape.attribute_count).unwrap_or(0);
    let attributes = inst
        .fields
        .iter()
        .take(declared)
        .take_while(|(name, value)| {
            name != CONTENT_FIELD && attribute_text(value, layout.wide_values).is_some()
        })
        .count();
    let (attrs, rest) = inst.fields.split_at(attributes);

    indent(out, depth);
    let _ = write!(out, "<{}", inst.type_name);
    for (name, value) in attrs {
        let text = attribute_text(value, layout.wide_values).unwrap_or_default();
        let _ = write!(out, " {name}=\"{}\"", escape_attr(&text));
    }

    let content = rest
        .iter()
        .find(|(n, _)| n == CONTENT_FIELD)
        .map(|(_, v)| v);
    let children: Vec<&(String, Value)> = rest.iter().filter(|(n, _)| n != CONTENT_FIELD).collect();

    if children.is_empty() {
        match content {
            None => out.push_str("/>\n"),
            Some(value) => {
                out.push('>');
                out.push_str(&escape_text(&content_text(value)?));
                let _ = writeln!(out, "</{}>", inst.type_name);
            }
        }
        return Ok(());
    }

    out.push_str(">\n");
    for (name, value) in children {
        write_child(out, name, value, layout, seen, depth + 1)?;
    }
    if let Some(value) = content {
        indent(out, depth + 1);
        out.push_str(&escape_text(&content_text(value)?));
        out.push('\n');
    }
    indent(out, depth);
    let _ = writeln!(out, "</{}>", inst.type_name);
    Ok(())
}

fn write_child(
    out: &mut String,
    name: &str,
    value: &Value,
    layout: &RbfLayout,
    seen: &mut BTreeMap<String, usize>,
    depth: usize,
) -> Result<()> {
    match value {
        Value::Struct(inner) | Value::Pointer(Some(inner)) => {
            write_element(out, inner, layout, seen, depth)
        }
        Value::Vec3([x, y, z]) => {
            indent(out, depth);
            let _ = writeln!(
                out,
                "<{name} x=\"{}\" y=\"{}\" z=\"{}\"/>",
                format_float(f64::from(*x)),
                format_float(f64::from(*y)),
                format_float(f64::from(*z))
            );
            Ok(())
        }
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| write_child(out, name, item, layout, seen, depth)),
        Value::Pointer(None) | Value::Null => Ok(()),
        other => {
            let text = attribute_text(other, layout.wide_values).ok_or_else(|| {
                MetaError::Other(format!(
                    "rbf xml: '{name}' holds a value RBF cannot store: {other:?}"
                ))
            })?;
            indent(out, depth);
            let _ = writeln!(out, "<{name} value=\"{}\"/>", escape_attr(&text));
            Ok(())
        }
    }
}

fn content_text(value: &Value) -> Result<String> {
    Ok(match value {
        Value::String(s) => s.clone(),
        Value::ByteArray(bytes) => bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        Value::Array(items) => items
            .iter()
            .map(|v| match v {
                Value::UInt(n) => Ok(n.to_string()),
                Value::Int(n) => Ok(n.to_string()),
                other => Err(MetaError::Other(format!(
                    "rbf xml: a short_array holds {other:?}, not an integer"
                ))),
            })
            .collect::<Result<Vec<_>>>()?
            .join(" "),
        other => {
            return Err(MetaError::Other(format!(
                "rbf xml: element text holds {other:?}"
            )))
        }
    })
}

/// What a literal says it is; with GTA VI's marker (`wide`), a negative 32-bit integer too.
fn typed(text: &str, wide: bool) -> Value {
    match text {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if !text.is_empty() && text.len() <= 10 && text.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(v) = text.parse::<u32>() {
            return Value::UInt(u64::from(v));
        }
    }
    let negative = text
        .strip_prefix('-')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
    if wide && negative && text.len() <= 11 {
        match text.parse::<i32>() {
            Ok(v) if v < 0 => return Value::Int(i64::from(v)),
            _ => {}
        }
    }
    let looks_float =
        text.contains(['.', 'e', 'E']) || matches!(text, "NaN" | "inf" | "-inf" | "+inf");
    if looks_float {
        if let Ok(v) = text.parse::<f64>() {
            return Value::Float(v);
        }
    }
    Value::String(text.to_string())
}

fn read_attrs(attrs: Attributes) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for a in attrs {
        let a = a.map_err(|e| MetaError::Other(format!("rbf xml: {e}")))?;
        out.push((local_name(a.key), decode_attr(&a.value)));
    }
    Ok(out)
}

/// An element with no body: a typed scalar in shorthand, or an empty struct.
fn scalar_shorthand(attrs: &[(String, String)], wide: bool) -> Option<Value> {
    match attrs {
        [(k, v)] if k == "value" => Some(typed(v, wide)),
        [(x, xv), (y, yv), (z, zv)] if x == "x" && y == "y" && z == "z" => {
            let p = |s: &str| s.parse::<f32>().ok();
            Some(Value::Vec3([p(xv)?, p(yv)?, p(zv)?]))
        }
        _ => None,
    }
}

fn element_head(tag: &str, attrs: Attributes, layout: &mut RbfLayout) -> Result<MetaInstance> {
    let attrs = read_attrs(attrs)?;
    let mut inst = MetaInstance::new(tag);
    layout.note_descriptor(tag);
    // Every measured struct of GTA VI's form states x1 and x2 as 0.
    let shape = RbfStructShape {
        x1: 0,
        x2: 0,
        attribute_count: i16::try_from(attrs.len())
            .map_err(|_| MetaError::Other(format!("rbf xml: <{tag}> has too many attributes")))?,
    };
    layout.record_shape(tag, shape);
    (layout.occurrences.entry(tag.to_string()))
        .or_default()
        .push(shape);
    for (name, text) in attrs {
        layout.note_descriptor(&name);
        inst.fields.push((name, typed(&text, layout.wide_values)));
    }
    Ok(inst)
}

fn read_element(
    reader: &mut Reader<&[u8]>,
    tag: &str,
    attrs: Attributes,
    layout: &mut RbfLayout,
) -> Result<MetaInstance> {
    let mut inst = element_head(tag, attrs, layout)?;
    let mut text = String::new();
    let mut had_children = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => {
                return Err(MetaError::Other(format!(
                    "rbf xml: <{tag}> is never closed"
                )))
            }
            Event::End(e) if local_name(e.name()) == tag => break,
            Event::Start(e) => {
                had_children = true;
                let child_tag = local_name(e.name());
                let child = read_element(reader, &child_tag, e.attributes(), layout)?;
                inst.fields
                    .push((child_tag, Value::Struct(Box::new(child))));
            }
            Event::Empty(e) => {
                had_children = true;
                let child_tag = local_name(e.name());
                let attrs = read_attrs(e.attributes())?;
                match scalar_shorthand(&attrs, layout.wide_values) {
                    Some(value) => {
                        layout.note_descriptor(&child_tag);
                        inst.fields.push((child_tag, value));
                    }
                    None => {
                        let child = element_head(&child_tag, e.attributes(), layout)?;
                        inst.fields
                            .push((child_tag, Value::Struct(Box::new(child))));
                    }
                }
            }
            Event::Text(t) => text.push_str(&decode_utf8(t.as_ref())),
            Event::CData(t) => text.push_str(&decode_utf8(t.as_ref())),
            Event::GeneralRef(r) => text.push_str(&resolve_general_ref(&r)),
            _ => {}
        }
        buf.clear();
    }

    // ⚠ Beside child elements the surrounding whitespace is indentation, so
    // only an element with no children keeps its text byte for byte.
    let body = if had_children {
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    } else {
        Some(text)
    };
    if let Some(body) = body {
        let kind = match inst.get(CONTENT_KIND) {
            Some(Value::String(k)) => k.clone(),
            _ => String::new(),
        };
        layout.content_kinds.insert(tag.to_string(), kind.clone());
        inst.fields
            .push((CONTENT_FIELD.to_string(), content_value(&kind, &body)?));
    }
    Ok(inst)
}

fn content_value(kind: &str, body: &str) -> Result<Value> {
    let numbers = || body.split_ascii_whitespace();
    Ok(match kind {
        "char_array" => Value::ByteArray(
            numbers()
                .map(|n| {
                    n.parse::<u8>().map_err(|_| {
                        MetaError::Other(format!("rbf xml: '{n}' in a char_array is not a byte"))
                    })
                })
                .collect::<Result<_>>()?,
        ),
        "short_array" => Value::Array(
            numbers()
                .map(|n| {
                    n.parse::<u16>()
                        .map(|v| Value::UInt(u64::from(v)))
                        .map_err(|_| {
                            MetaError::Other(format!(
                                "rbf xml: '{n}' in a short_array is not a u16"
                            ))
                        })
                })
                .collect::<Result<_>>()?,
        ),
        _ => Value::String(body.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::binary::rbf::{reader, writer};
    use crate::meta::diagnostics::Diagnostics;
    use crate::meta::schema::SchemaLibrary;

    const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CRoot>
  <items>
    <Item>
      <id value="6"/>
      <scale value="0.5"/>
      <on value="false"/>
      <label value="a &amp; b"/>
      <where x="1.0" y="2.0" z="3.0"/>
      <tracks content="char_array">33 7</tracks>
      <ids content="short_array">28462 1</ids>
      <rot x="0.0" y="0.0" z="0.0" w="1.0"/>
      <name>hello world </name>
    </Item>
  </items>
</CRoot>
"#;

    fn encode(xml: &str) -> Vec<u8> {
        let (root, layout) = from_xml(xml.as_bytes()).expect("parses");
        writer::write(Some(&layout), &root).expect("encodes")
    }

    fn decode(bytes: &[u8]) -> (MetaInstance, RbfLayout) {
        let mut d = Diagnostics::lenient();
        reader::parse_full(&SchemaLibrary::default(), bytes, &mut d).expect("decodes")
    }

    #[test]
    fn the_xml_an_rbf_encodes_is_the_xml_it_came_from() {
        let bytes = encode(DOCUMENT);
        let (root, layout) = decode(&bytes);
        assert_eq!(to_xml(&root, &layout).unwrap(), DOCUMENT);
    }

    #[test]
    fn a_literal_names_its_rbf_type() {
        let (root, layout) = decode(&encode(DOCUMENT));
        let Some(Value::Struct(items)) = root.get("items") else {
            panic!("{root:?}")
        };
        let Some(Value::Struct(item)) = items.get("Item") else {
            panic!("{items:?}")
        };
        assert_eq!(item.get("id"), Some(&Value::UInt(6)));
        assert_eq!(item.get("scale"), Some(&Value::Float(0.5)));
        assert_eq!(item.get("on"), Some(&Value::Bool(false)));
        assert_eq!(item.get("label"), Some(&Value::String("a & b".into())));
        assert_eq!(item.get("where"), Some(&Value::Vec3([1.0, 2.0, 3.0])));
        let Some(Value::Struct(tracks)) = item.get("tracks") else {
            panic!("{item:?}")
        };
        assert_eq!(
            tracks.get(CONTENT_FIELD),
            Some(&Value::ByteArray(vec![33, 7]))
        );
        assert_eq!(layout.shape_for("tracks").attribute_count, 1);
        assert_eq!(layout.shape_for("rot").attribute_count, 4);
        let Some(Value::Struct(name)) = item.get("name") else {
            panic!("{item:?}")
        };
        assert_eq!(
            name.get(CONTENT_FIELD),
            Some(&Value::String("hello world ".into()))
        );
    }

    #[test]
    fn one_tag_with_two_attribute_counts_is_refused() {
        let xml = r#"<CRoot><a k="1"/><a/></CRoot>"#;
        let err = from_xml(xml.as_bytes()).expect_err("RBF cannot state both");
        assert!(err.to_string().contains("<a>"), "{err}");
    }

    /// A body with GTA VI's marker: a root whose one attribute is a negative 8-byte 0x10, an
    /// 8-byte 0x40 no f32 holds, and two structs of one name whose attribute counts differ.
    fn wide_body() -> Vec<u8> {
        let mut b = b"RBF0".to_vec();
        b.extend(reader::WIDE_MARKER);
        let name = |b: &mut Vec<u8>, index: u8, ty: u8, text: &str| {
            b.extend([index, ty]);
            b.extend((text.len() as i16).to_le_bytes());
            b.extend(text.as_bytes());
        };
        name(&mut b, 0, 0x00, "CRoot");
        b.extend([0, 0, 0, 0, 1, 0]);
        name(&mut b, 1, 0x10, "below");
        b.extend((-70_000i64).to_le_bytes());
        name(&mut b, 2, 0x40, "ratio");
        b.extend(0.1f64.to_le_bytes());
        name(&mut b, 3, 0x00, "CItem");
        b.extend([0, 0, 0, 0, 0, 0]);
        name(&mut b, 4, 0x10, "k");
        b.extend(3u64.to_le_bytes());
        b.extend([0xFF, 0xFF]);
        b.extend([3, 0x00, 0, 0, 0, 0, 1, 0]);
        b.extend([4, 0x10]);
        b.extend(0xFFFF_FFF0u64.to_le_bytes());
        b.extend([0xFF, 0xFF, 0xFF, 0xFF]);
        b
    }

    #[test]
    fn a_body_with_gta_vis_marker_goes_to_xml_and_back_to_its_own_bytes() {
        let bytes = wide_body();
        let (root, layout) = decode(&bytes);
        assert!(layout.wide_values);
        let xml = to_xml(&root, &layout).expect("transcribes");
        assert!(xml.contains(&format!("<?rbf {MARK}?>")), "{xml}");
        assert!(xml.contains(r#"<CRoot below="-70000">"#), "{xml}");
        assert!(xml.contains(r#"<CItem k="4294967280"/>"#), "{xml}");
        assert!(xml.contains("<CItem>"), "{xml}");

        let (again, back) = from_xml(xml.as_bytes()).expect("parses");
        assert!(back.wide_values);
        assert_eq!(again.get("below"), Some(&Value::Int(-70_000)));
        assert_eq!(again.get("ratio"), Some(&Value::Float(0.1)));
        assert_eq!(writer::write(Some(&back), &again).expect("encodes"), bytes);

        // Without the mark the same XML is GTA V's form, whose tags state one shape each.
        let gta5 = xml.replace(&format!("<?rbf {MARK}?>\n"), "");
        let err = from_xml(gta5.as_bytes()).expect_err("refused");
        assert!(err.to_string().contains("<CItem>"), "{err}");
        let err = from_xml(xml.replace("FB FF 01 00", "FB FF 02 00").as_bytes())
            .expect_err("another marker")
            .to_string();
        assert!(err.contains("is not the one mark RBF states"), "{err}");
    }

    #[test]
    fn a_negative_integer_is_an_integer_only_with_gta_vis_marker() {
        assert_eq!(typed("-7", true), Value::Int(-7));
        assert_eq!(typed("-7", false), Value::String("-7".into()));
        assert_eq!(typed("-2147483648", true), Value::Int(-2_147_483_648));
        assert_eq!(
            typed("-2147483649", true),
            Value::String("-2147483649".into())
        );
        assert_eq!(typed("-0", true), Value::String("-0".into()));
        assert_eq!(typed("-inf", true), Value::Float(f64::NEG_INFINITY));
        assert_eq!(typed("42", true), Value::UInt(42));
    }

    #[test]
    fn a_negative_integer_prints_signed_only_with_gta_vis_marker() {
        assert_eq!(attribute_text(&Value::Int(-5), true).as_deref(), Some("-5"));
        // Without it the text reads back as the same unsigned word the writer stores.
        let text = attribute_text(&Value::Int(-5), false).expect("an integer");
        assert_eq!(text, "4294967291");
        assert_eq!(typed(&text, false), Value::UInt(4_294_967_291));
    }
}
