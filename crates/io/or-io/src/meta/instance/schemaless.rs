//! Metadata XML read with no schema, as the tree its own elements state: an
//! element with children is an `Item` array when every child is an `Item`, a
//! pointer when it names its `type` and a struct otherwise, and a leaf is text,
//! a `value` or `x y [z [w]]`.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::meta::error::{MetaError, Result};

use super::parser::{decode_attr, decode_utf8, local_name, resolve_general_ref};
use super::value::{MetaInstance, Value};

/// One element as the file spells it.
struct Element {
    tag: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Element>,
}

/// `bytes` as the tree its elements state, refused where that dialect could not have written it.
pub fn parse_schemaless(bytes: &[u8]) -> Result<MetaInstance> {
    let root = read_tree(bytes)?;
    if !root.attrs.is_empty() {
        return Err(refused(&root, "carries attributes"));
    }
    if !root.text.trim().is_empty() {
        return Err(refused(&root, "holds text"));
    }
    instance_of(root.tag.clone(), root)
}

fn read_tree(bytes: &[u8]) -> Result<Element> {
    let mut reader = Reader::from_reader(bytes);
    let mut open: Vec<Element> = Vec::new();
    let mut buf = Vec::new();
    loop {
        let closed = match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                open.push(element(&e)?);
                None
            }
            Event::Empty(e) => Some(element(&e)?),
            Event::End(_) => open.pop(),
            Event::Text(t) => {
                if let Some(el) = open.last_mut() {
                    el.text.push_str(&decode_utf8(t.as_ref()));
                }
                None
            }
            Event::CData(t) => {
                if let Some(el) = open.last_mut() {
                    el.text.push_str(&decode_utf8(t.as_ref()));
                }
                None
            }
            Event::GeneralRef(r) => {
                if let Some(el) = open.last_mut() {
                    el.text.push_str(&resolve_general_ref(&r));
                }
                None
            }
            Event::Eof => {
                return Err(MetaError::Other(match open.last() {
                    Some(el) => format!("the XML document ends inside <{}>", el.tag),
                    None => "no root element found in XML document".into(),
                }))
            }
            _ => None,
        };
        if let Some(el) = closed {
            match open.last_mut() {
                Some(parent) => parent.children.push(el),
                None => return Ok(el),
            }
        }
        buf.clear();
    }
}

fn element(start: &BytesStart) -> Result<Element> {
    let mut attrs = Vec::new();
    for a in start.attributes() {
        let a = a?;
        attrs.push((local_name(a.key), decode_attr(&a.value)));
    }
    Ok(Element {
        tag: local_name(start.name()),
        attrs,
        text: String::new(),
        children: Vec::new(),
    })
}

fn refused(el: &Element, why: &str) -> MetaError {
    MetaError::Other(format!(
        "<{}> {why}, which a document read without a schema cannot hold",
        el.tag
    ))
}

fn instance_of(type_name: String, el: Element) -> Result<MetaInstance> {
    let mut inst = MetaInstance::new(type_name);
    for child in el.children {
        let name = child.tag.clone();
        inst.fields.push((name, value_of(child)?));
    }
    Ok(inst)
}

fn attr_names(el: &Element) -> Vec<&str> {
    let mut names: Vec<&str> = el.attrs.iter().map(|(k, _)| k.as_str()).collect();
    names.sort_unstable();
    names
}

fn value_of(el: Element) -> Result<Value> {
    if el.children.is_empty() {
        return leaf(&el);
    }
    if !el.text.trim().is_empty() {
        return Err(refused(&el, "mixes text with elements"));
    }
    match attr_names(&el).as_slice() {
        [] if el.children.iter().all(|c| c.tag == "Item") => Ok(Value::Array(
            el.children
                .into_iter()
                .map(value_of)
                .collect::<Result<_>>()?,
        )),
        [] => Ok(Value::Struct(Box::new(instance_of(el.tag.clone(), el)?))),
        ["type"] => {
            let type_name = el.attrs[0].1.clone();
            Ok(Value::Pointer(Some(Box::new(instance_of(type_name, el)?))))
        }
        _ => Err(refused(&el, "carries attributes besides `type`")),
    }
}

fn leaf(el: &Element) -> Result<Value> {
    let text = el.text.trim();
    let names = attr_names(el);
    if names.is_empty() {
        return Ok(Value::String(text.to_string()));
    }
    // The writer's spelling of bytes: their decimal values, space-separated.
    if names == ["content"] && el.attrs[0].1 == "char_array" {
        return (text.split_ascii_whitespace())
            .map(|n| {
                n.parse::<u8>()
                    .map_err(|_| refused(el, "holds a char_array entry that is not a byte"))
            })
            .collect::<Result<_>>()
            .map(Value::ByteArray);
    }
    if !text.is_empty() {
        return Err(refused(el, "holds both text and attributes"));
    }
    let attr = |name: &str| {
        el.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map_or("", |(_, v)| v.as_str())
    };
    let component = |name: &str| {
        attr(name)
            .trim()
            .parse::<f32>()
            .map_err(|_| refused(el, &format!("holds `{name}` that is not a number")))
    };
    match names.as_slice() {
        ["value"] => scalar(attr("value").trim())
            .ok_or_else(|| refused(el, "holds a value that is not a number or a bool")),
        ["x", "y"] => Ok(Value::Vec2([component("x")?, component("y")?])),
        ["x", "y", "z"] => Ok(Value::Vec3([
            component("x")?,
            component("y")?,
            component("z")?,
        ])),
        ["w", "x", "y", "z"] => Ok(Value::Vec4([
            component("x")?,
            component("y")?,
            component("z")?,
            component("w")?,
        ])),
        _ => Err(refused(el, "carries attributes no value holds")),
    }
}

/// ⚠ The narrowest reading wins, so `1` stays an integer and `1.0` a float
/// across a save.
fn scalar(text: &str) -> Option<Value> {
    match text {
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        _ => text
            .parse::<i64>()
            .map(Value::Int)
            .or_else(|_| text.parse::<u64>().map(Value::UInt))
            .ok()
            .or_else(|| {
                text.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(Value::Float)
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_keeps_its_kind_and_a_word_is_no_number() {
        assert_eq!(scalar("1"), Some(Value::Int(1)));
        assert_eq!(scalar("1.000000"), Some(Value::Float(1.0)));
        assert_eq!(scalar("18446744073709551615"), Some(Value::UInt(u64::MAX)));
        assert_eq!(scalar("false"), Some(Value::Bool(false)));
        assert_eq!(scalar("inf"), None);
        assert_eq!(scalar("CONTENTS_MAP"), None);
    }
}
