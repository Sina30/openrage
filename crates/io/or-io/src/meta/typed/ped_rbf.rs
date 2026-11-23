//! Creature metadata in RBF: the tag tree a reader gives, as the tree the typed records read, and
//! back.
//!
//! RBF holds tags and no types. A member that is an array of records is a tag whose `Item`
//! children are the elements; an array of bytes is a tag whose `content` attribute names the
//! width (`char_array`, `short_array`) and whose payload is the `Content` field; and a hashed
//! string is a tag with `content="string"`. This reads each into the member the PSO and RBM
//! trees give (an `Array`, a `String`), and writes the typed record's members back into the tags
//! it came from, so a tag nothing typed touches, its attributes and its order are the file's.
//!
//! ⚠ The three kinds of member below are the creature records' own; a tag this does not name
//! passes through both ways unchanged.

use std::collections::BTreeMap;

use crate::meta::binary::rbf::layout::{RbfLayout, RbfStructShape};
use crate::meta::binary::synth::name_hash;
use crate::meta::binary::{BinaryFormat, ContainerLayout, MetaLayout};
use crate::meta::instance::{MetaInstance, SourceFormat, Value};

use super::Arrival;

const ITEM: &str = "Item";
const CONTENT: &str = "Content";
const CONTENT_KIND: &str = "content";

#[derive(Clone, Copy)]
enum Kind {
    /// A leaf the tree already holds as the typed member does.
    Scalar,
    /// A tag of `Item` children, each a record of this class.
    Items(&'static str),
    /// A tag whose payload is bytes.
    Bytes,
    /// A tag whose payload is little-endian 16-bit words.
    Words,
    /// A tag whose payload is a string.
    Text,
}

use Kind::{Bytes, Items, Scalar, Text, Words};

/// The members of the creature records, in the order the file writes them.
fn members(class: &str) -> &'static [(&'static str, Kind)] {
    match class {
        "CCreatureMetaData" => &[
            // ⚠ The order a tag is added in, which 56 of the 64 staged documents hold, the others being a tag present and empty (7) and one document that adds them in another order: the comp expressions before the prop ones.
            (
                "shaderVariableComponents",
                Items("CShaderVariableComponent"),
            ),
            ("pedCompExpressions", Items("CPedCompExpressionData")),
            ("pedPropExpressions", Items("CPedPropExpressionData")),
        ],
        "CShaderVariableComponent" => &[
            ("pedcompID", Scalar),
            ("maskID", Scalar),
            ("shaderVariableHashString", Text),
            ("tracks", Bytes),
            ("ids", Words),
            ("components", Bytes),
        ],
        "CPedPropExpressionData" => &[
            ("pedPropID", Scalar),
            ("pedPropVarIndex", Scalar),
            ("pedPropExpressionIndex", Scalar),
            ("tracks", Bytes),
            ("ids", Words),
            ("types", Bytes),
            ("components", Bytes),
        ],
        "CPedCompExpressionData" => &[
            ("pedCompID", Scalar),
            ("pedCompVarIndex", Scalar),
            ("pedCompExpressionIndex", Scalar),
            ("tracks", Bytes),
            ("ids", Words),
            ("types", Bytes),
            ("components", Bytes),
        ],
        _ => &[],
    }
}

fn kind_of(class: &str, name: &str) -> Kind {
    members(class)
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(Scalar, |(_, kind)| *kind)
}

fn payload(tag: &MetaInstance) -> Option<&Value> {
    tag.fields
        .iter()
        .find(|(n, _)| n == CONTENT)
        .map(|(_, v)| v)
}

/// The tags `tags` as the tree the typed `class` reads.
pub(super) fn to_typed(tags: &MetaInstance, class: &str) -> Result<MetaInstance, String> {
    let mut typed = MetaInstance::new(class);
    for (name, value) in &tags.fields {
        let converted = match (kind_of(class, name), value) {
            (Items(element), Value::Struct(tag)) => Value::Array(
                tag.fields
                    .iter()
                    .filter(|(n, _)| n == ITEM)
                    .map(|(_, item)| match item {
                        Value::Struct(item) => {
                            to_typed(item, element).map(|t| Value::Struct(Box::new(t)))
                        }
                        other => Err(format!(
                            "an `{ITEM}` of `{name}` holds {other:?}, not a tag"
                        )),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            (Bytes, Value::Struct(tag)) => Value::Array(match payload(tag) {
                Some(Value::ByteArray(bytes)) => {
                    bytes.iter().map(|b| Value::UInt(u64::from(*b))).collect()
                }
                None => Vec::new(),
                Some(other) => return Err(format!("`{name}` holds {other:?}, not bytes")),
            }),
            (Words, Value::Struct(tag)) => Value::Array(match payload(tag) {
                Some(Value::Array(words)) => words.clone(),
                None => Vec::new(),
                Some(other) => return Err(format!("`{name}` holds {other:?}, not 16-bit words")),
            }),
            (Text, Value::Struct(tag)) => match payload(tag) {
                Some(Value::String(text)) => Value::String(text.clone()),
                None => Value::String(String::new()),
                Some(other) => return Err(format!("`{name}` holds {other:?}, not text")),
            },
            (_, other) => other.clone(),
        };
        typed.fields.push((name.clone(), converted));
    }
    // A member the file leaves out is empty, so a record can be given its first element.
    for (name, kind) in members(class) {
        if !typed.fields.iter().any(|(n, _)| n == name) {
            match kind {
                Scalar => {}
                Text => typed
                    .fields
                    .push((name.to_string(), Value::String(String::new()))),
                _ => typed
                    .fields
                    .push((name.to_string(), Value::Array(Vec::new()))),
            }
        }
    }
    Ok(typed)
}

/// The `content` word a payload of this kind states, and the payload in the form the file holds it.
fn content_of(name: &str, kind: Kind, typed: &Value) -> Result<(&'static str, Value), String> {
    Ok(match (kind, typed) {
        (Bytes, Value::Array(items)) => ("char_array", bytes(name, items)?),
        (Words, Value::Array(items)) => ("short_array", Value::Array(items.clone())),
        (Text, Value::String(s)) => ("string", Value::String(s.clone())),
        _ => return Err(format!("`{name}` has no tag to write {typed:?} into")),
    })
}

fn bytes(name: &str, items: &[Value]) -> Result<Value, String> {
    items
        .iter()
        .map(|v| match v {
            Value::UInt(n) => {
                u8::try_from(*n).map_err(|_| format!("`{name}` holds {n}, past a byte"))
            }
            other => Err(format!("`{name}` holds {other:?}, not a byte")),
        })
        .collect::<Result<_, _>>()
        .map(Value::ByteArray)
}

/// `typed` written into a copy of `template`, the tags it was read from, or into new tags where there are none.
pub(super) fn to_tags(
    typed: &MetaInstance,
    class: &str,
    template: Option<&MetaInstance>,
) -> Result<MetaInstance, String> {
    let mut tags = template.map_or_else(
        || {
            MetaInstance::new(match class {
                "CCreatureMetaData" => class,
                _ => ITEM,
            })
        },
        |t| MetaInstance {
            fields: Vec::new(),
            ..t.clone()
        },
    );
    // What the template holds, in its order, then what only the typed record holds of the members this knows the tags of.
    let mut done: Vec<&str> = Vec::new();
    let order = template
        .into_iter()
        .flat_map(|t| t.fields.iter().map(|(n, _)| n.as_str()))
        .chain(members(class).iter().map(|(n, _)| *n));
    for name in order {
        if done.contains(&name) {
            continue;
        }
        done.push(name);
        let held = template.and_then(|t| t.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v));
        // ⚠ A record built here names its members as the writer looks them up, `hash_XXXXXXXX` where no snapshot names them; a tag is written under its own name.
        let key = name_hash(name);
        let now = typed
            .fields
            .iter()
            .find(|(n, _)| name_hash(n) == key)
            .map(|(_, v)| v);
        match (kind_of(class, name), held, now) {
            // Nothing typed names it: the file's own tag.
            (_, Some(held), None) => tags.fields.push((name.to_string(), held.clone())),
            (Items(element), held, Some(Value::Array(elements))) => {
                if held.is_none() && elements.is_empty() {
                    continue;
                }
                let held_tag = match held {
                    Some(Value::Struct(tag)) => Some(&**tag),
                    _ => None,
                };
                let held_items: Vec<&MetaInstance> = held_tag
                    .into_iter()
                    .flat_map(|t| t.fields.iter())
                    .filter_map(|(n, v)| match (n.as_str(), v) {
                        (ITEM, Value::Struct(item)) => Some(&**item),
                        _ => None,
                    })
                    .collect();
                let mut tag = held_tag.map_or_else(
                    || MetaInstance::new(name),
                    |t| MetaInstance {
                        fields: t
                            .fields
                            .iter()
                            .filter(|(n, _)| n != ITEM)
                            .cloned()
                            .collect(),
                        ..t.clone()
                    },
                );
                for (i, element_value) in elements.iter().enumerate() {
                    let Value::Struct(element_tree) = element_value else {
                        return Err(format!(
                            "an element of `{name}` is {element_value:?}, not a record"
                        ));
                    };
                    let item_template = held_items.get(i).or(held_items.last()).copied();
                    let item = to_tags(element_tree, element, item_template)?;
                    tag.fields
                        .push((ITEM.to_string(), Value::Struct(Box::new(item))));
                }
                tags.fields
                    .push((name.to_string(), Value::Struct(Box::new(tag))));
            }
            (kind @ (Bytes | Words | Text), held, Some(value)) => {
                let nothing = matches!(value, Value::Array(a) if a.is_empty())
                    || matches!(value, Value::String(s) if s.is_empty());
                if held.is_none() && nothing {
                    continue;
                }
                let (text, content) = content_of(name, kind, value)?;
                let mut tag = match held {
                    Some(Value::Struct(tag)) => (**tag).clone(),
                    _ => {
                        let mut tag = MetaInstance::new(name);
                        tag.fields
                            .push((CONTENT_KIND.to_string(), Value::String(text.to_string())));
                        tag
                    }
                };
                match tag.fields.iter_mut().find(|(n, _)| n == CONTENT) {
                    Some(slot) => slot.1 = content,
                    None => tag.fields.push((CONTENT.to_string(), content)),
                }
                tags.fields
                    .push((name.to_string(), Value::Struct(Box::new(tag))));
            }
            (_, _, Some(value)) => tags.fields.push((name.to_string(), value.clone())),
            (_, None, None) => {}
        }
    }
    Ok(tags)
}

/// The layout a creature document built here is written with: the shape every staged document gives each tag.
///
/// ⚠ Only the four byte and word tags state an attribute (their `content`); every other tag states
/// none, and a tag this names nothing for is written with none.
pub(super) fn default_arrival() -> Arrival {
    let shape = |attribute_count| RbfStructShape {
        x1: 0,
        x2: 0,
        attribute_count,
    };
    let shapes: BTreeMap<String, RbfStructShape> = ["tracks", "ids", "types", "components"]
        .into_iter()
        .map(|name| (name.to_string(), shape(1)))
        .collect();
    Arrival {
        layout: Some(MetaLayout {
            format: BinaryFormat::Rbf,
            container: ContainerLayout::Rbf(RbfLayout {
                shapes,
                ..RbfLayout::default()
            }),
        }),
        format: SourceFormat::Rbf,
    }
}
