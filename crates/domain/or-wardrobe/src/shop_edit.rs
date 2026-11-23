//! Editing the shop entry of one texture: its price, its restriction tags and its forced components, and any other element it states kept as written.
//!
//! The staged shop files state, for an entry, `cost`, `forcedComponents` (items of `nameHash`, `enumValue` and
//! `eCompType`) and `restrictionTags` (items of `tagNameHash`), and no other element besides the ids and the places
//! (`the_staged_shop_files_state_only_the_elements_the_editor_names`). Those three have typed edits. A lock hash, the
//! variant components and the outfits are elements of the engine's shop document that no staged file states, so they
//! have no fields of their own: [`Field::Raw`] keeps any element a person types or an import found, as the XML it was
//! written in, and writes it back in its place.
//!
//! ⚠ An entry no file stated is made when the first of these edits a texture that has none. It has no id, so the
//! build names it as it names any entry, and it is dropped again when the last thing it states is taken away.

use crate::edit::Error;
use crate::model::{Garment, ShopEntry};
use crate::names::Component;
use crate::shop::{compact, parse_tree, slot_of, Node};

/// The longest text of one tag or name, as one line.
pub const MAX_TEXT: usize = 128;

/// The elements of an entry that its id and its place own, and no edit here touches.
const OWNED: [&str; 7] = [
    "cost",
    "uniqueNameHash",
    "localDrawableIndex",
    "localPropIndex",
    "eCompType",
    "eAnchorPoint",
    "textureIndex",
];

pub const TAGS: &str = "restrictionTags";
pub const FORCED: &str = "forcedComponents";

/// One forced component, as the staged files state it: a component that comes with the texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forced {
    /// `nameHash`: a name the game hashes, or `0`.
    pub name_hash: String,
    /// `enumValue`.
    pub enum_value: u32,
    /// `eCompType`: the component it is for.
    pub component: Component,
}

/// What an edit of an entry changes.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    /// The price, as the whole number the file writes; none takes it away so that the build writes 0.
    Cost(Option<String>),
    /// The restriction tags, each a word the game hashes; none takes them all away.
    Tags(Vec<String>),
    /// The forced components; none takes them all away.
    Forced(Vec<Forced>),
    /// Any other element, by its name: the XML to state it with, or none to take it away.
    Raw(String, Option<String>),
}

fn element(entry: &ShopEntry, name: &str) -> Option<(usize, Node)> {
    entry
        .fields
        .iter()
        .enumerate()
        .filter_map(|(at, text)| Some((at, parse_tree(text).ok()?)))
        .find(|(_, node)| node.name == name)
}

/// The restriction tags an entry states, in order.
pub fn tags(entry: &ShopEntry) -> Vec<String> {
    let Some((_, node)) = element(entry, TAGS) else {
        return Vec::new();
    };
    (node.children.iter())
        .filter(|item| item.name == "Item")
        .filter_map(|item| item.value_of("tagNameHash"))
        .collect()
}

/// The forced components an entry states, or none where it states something of another shape (a component this does not name, an item with more elements), which stays as written and is edited as [`Field::Raw`].
pub fn forced(entry: &ShopEntry) -> Option<Vec<Forced>> {
    let Some((_, node)) = element(entry, FORCED) else {
        return Some(Vec::new());
    };
    node.children
        .iter()
        .map(|item| {
            let shaped = item.name == "Item"
                && item.children.len() == 3
                && ["nameHash", "enumValue", "eCompType"]
                    .iter()
                    .all(|n| item.children.iter().any(|c| c.name == *n));
            if !shaped {
                return None;
            }
            let component = match slot_of(&item.value_of("eCompType")?, false)? {
                crate::names::Slot::Component(c) => c,
                crate::names::Slot::Prop(_) => return None,
            };
            Some(Forced {
                name_hash: item.value_of("nameHash")?,
                enum_value: item.value_of("enumValue")?.trim().parse().ok()?,
                component,
            })
        })
        .collect()
}

/// The elements an entry states besides the ones it has typed edits for, each with its name and the XML it is kept as.
pub fn others(entry: &ShopEntry) -> Vec<(String, String)> {
    entry
        .fields
        .iter()
        .filter_map(|text| Some((parse_tree(text).ok()?.name, text.clone())))
        .filter(|(name, _)| name != TAGS && (name != FORCED || forced(entry).is_none()))
        .collect()
}

/// The whole number a price is written as, or why it is not one.
fn price(text: &str) -> Result<String, Error> {
    let text = text.trim();
    match text.parse::<i32>() {
        Ok(n) if n >= 0 && text.chars().all(|c| c.is_ascii_digit()) => Ok(n.to_string()),
        _ => Err(Error::Refused(format!(
            "a price is a whole number from 0 to {}, and `{text}` is not",
            i32::MAX
        ))),
    }
}

/// `text` as one line of at most [`MAX_TEXT`] characters.
fn one_line(text: &str, what: &str) -> Result<String, Error> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT || text.chars().any(char::is_control) {
        return Err(Error::Refused(format!(
            "{what} is one line of at most {MAX_TEXT} characters"
        )));
    }
    Ok(text.to_string())
}

fn node(name: &str, children: Vec<Node>) -> Node {
    Node {
        name: name.to_string(),
        children,
        ..Node::default()
    }
}

fn text_node(name: &str, text: &str) -> Node {
    Node {
        name: name.to_string(),
        text: text.to_string(),
        ..Node::default()
    }
}

fn value_node(name: &str, value: u32) -> Node {
    Node {
        name: name.to_string(),
        attrs: vec![("value".to_string(), value.to_string())],
        ..Node::default()
    }
}

/// An element in the place the entry already states one of its name, else after the others; none takes it away.
fn state(entry: &mut ShopEntry, name: &str, xml: Option<String>) {
    let at = element(entry, name).map(|(at, _)| at);
    match (at, xml) {
        (Some(at), Some(xml)) => entry.fields[at] = xml,
        (None, Some(xml)) => entry.fields.push(xml),
        (Some(at), None) => {
            entry.fields.remove(at);
        }
        (None, None) => {}
    }
}

/// Whether a name can be the name of an element.
fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn change(entry: &mut ShopEntry, field: Field) -> Result<(), Error> {
    match field {
        Field::Cost(cost) => {
            entry.cost = match cost.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                Some(text) => Some(price(text)?),
                None => None,
            };
        }
        Field::Tags(tags) => {
            let tags: Vec<String> = tags
                .iter()
                .map(|t| one_line(t, "a restriction tag"))
                .collect::<Result<_, _>>()?;
            let items = (tags.iter())
                .map(|t| node("Item", vec![text_node("tagNameHash", t)]))
                .collect::<Vec<_>>();
            state(
                entry,
                TAGS,
                (!items.is_empty()).then(|| compact(&node(TAGS, items))),
            );
        }
        Field::Forced(forced_now) => {
            if self::forced(entry).is_none() {
                return Err(Error::Refused(
                    "the entry states forced components of a shape this does not name: edit them as XML".into(),
                ));
            }
            let items = forced_now
                .iter()
                .map(|f| {
                    Ok(node(
                        "Item",
                        vec![
                            text_node("nameHash", &one_line(&f.name_hash, "a name hash")?),
                            value_node("enumValue", f.enum_value),
                            text_node(
                                "eCompType",
                                &format!("PV_COMP_{}", f.component.name().to_ascii_uppercase()),
                            ),
                        ],
                    ))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            state(
                entry,
                FORCED,
                (!items.is_empty()).then(|| compact(&node(FORCED, items))),
            );
        }
        Field::Raw(name, xml) => {
            if !is_xml_name(&name) {
                return Err(Error::Refused(format!(
                    "`{name}` is not the name of an element"
                )));
            }
            if OWNED.contains(&name.as_str()) {
                return Err(Error::Refused(format!(
                    "{name} is stated by the id and the place of the entry, not edited here"
                )));
            }
            // ⚠ Forced components of a shape this does not name are kept as written and edited here; the ones it does name have their own edit.
            if name == TAGS || (name == FORCED && forced(entry).is_some()) {
                return Err(Error::Refused(format!(
                    "{name} has its own edit: the restriction tags and the forced components"
                )));
            }
            let xml = match xml.as_deref().map(str::trim).filter(|x| !x.is_empty()) {
                Some(text) => {
                    let tree = parse_tree(text).map_err(|why| {
                        Error::Refused(format!("that is not one element of XML: {why}"))
                    })?;
                    if tree.name != name {
                        return Err(Error::Refused(format!(
                            "the element is called `{}` and should be called `{name}`",
                            tree.name
                        )));
                    }
                    Some(compact(&tree))
                }
                None => None,
            };
            state(entry, &name, xml);
        }
    }
    Ok(())
}

/// Make `field` the statement of the shop entry of texture `texture` of `garment`, making the entry when no file stated one.
///
/// Nothing changes where it is refused.
pub fn apply(garment: &mut Garment, texture: usize, field: Field) -> Result<(), Error> {
    let held = garment.variations.len();
    let variation = garment.variations.get_mut(texture).ok_or_else(|| {
        Error::Refused(format!(
            "the garment has {held} texture(s), not {}",
            texture + 1
        ))
    })?;
    let mut entry = variation.shop.clone().unwrap_or_default();
    change(&mut entry, field)?;
    let blank = entry.id.is_empty()
        && entry.from.is_empty()
        && entry.cost.is_none()
        && entry.fields.is_empty();
    variation.shop = (!blank).then_some(entry);
    Ok(())
}
