//! The shop metadata of a ped read as the elements it states, for a plan held to a shipped file and for the fields an entry has.
//!
//! The element names are what the game reads, so they are compared as text and never taken from the
//! generator: a document is a header of elements and the entries of its two sections, each entry a
//! list of the elements it has, in the order the file has them. An element with children of its own
//! (`forcedComponents`) is one field of its entry, with no text.

#![allow(dead_code)]

use std::collections::BTreeSet;

/// An element: its name, and the text between its tags or the `value` it states when it has no content.
pub type Field = (String, String);

#[derive(Debug, Default)]
pub struct Shop {
    /// The elements of the document outside its two sections: `pedName`, `dlcName` and the rest.
    pub header: Vec<Field>,
    pub components: Vec<Vec<Field>>,
    pub props: Vec<Vec<Field>>,
}

impl Shop {
    /// The value of the header element `name`.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.header
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The names of the elements the entries have.
pub fn names(entries: &[Vec<Field>]) -> BTreeSet<&str> {
    entries.iter().flatten().map(|(n, _)| n.as_str()).collect()
}

/// The entries with only the elements named in `keep`, in a fixed order.
pub fn only(entries: &[Vec<Field>], keep: &BTreeSet<&str>) -> Vec<Vec<Field>> {
    let mut kept: Vec<Vec<Field>> = entries
        .iter()
        .map(|e| {
            e.iter()
                .filter(|(n, _)| keep.contains(n.as_str()))
                .cloned()
                .collect()
        })
        .collect();
    kept.sort();
    kept
}

fn value_of(attributes: &str) -> String {
    attributes
        .split_once("value=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map_or(String::new(), |(value, _)| value.to_string())
}

pub fn read(text: &str) -> Shop {
    struct Open {
        name: String,
        text: String,
        has_children: bool,
    }
    let mut shop = Shop::default();
    let mut open: Vec<Open> = Vec::new();
    let mut entry: Vec<Field> = Vec::new();
    // The document is `[ShopPedApparel]`, a section is `[.., section]`, an entry `[.., Item]`, and its elements one level below.
    let field = |shop: &mut Shop, entry: &mut Vec<Field>, depth: usize, field: Field| match depth {
        1 if !matches!(field.0.as_str(), "pedComponents" | "pedProps") => shop.header.push(field),
        3 => entry.push(field),
        _ => {}
    };
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        // A comment can hold whole entries that are switched off, and `>` inside it ends nothing.
        if let Some(comment) = rest.strip_prefix("!--") {
            rest = comment.split_once("-->").expect("a comment is closed").1;
            continue;
        }
        let end = rest.find('>').expect("a tag is closed");
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let following = rest.split('<').next().unwrap_or("").trim().to_string();
        if tag.starts_with('?') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            let closed = open.pop().expect("tags are balanced");
            assert_eq!(closed.name, name, "tags are balanced");
            if name == "Item" && open.len() == 2 {
                let finished = std::mem::take(&mut entry);
                match open[1].name.as_str() {
                    "pedComponents" => shop.components.push(finished),
                    "pedProps" => shop.props.push(finished),
                    other => panic!("an entry inside {other}"),
                }
            } else {
                let text = if closed.has_children {
                    String::new()
                } else {
                    closed.text
                };
                field(&mut shop, &mut entry, open.len(), (closed.name, text));
            }
            continue;
        }
        if let Some(parent) = open.last_mut() {
            parent.has_children = true;
        }
        match tag.strip_suffix('/') {
            Some(empty) => {
                let (name, attributes) = empty.split_once(' ').unwrap_or((empty, ""));
                let depth = open.len();
                field(
                    &mut shop,
                    &mut entry,
                    depth,
                    (name.to_string(), value_of(attributes)),
                );
            }
            None => open.push(Open {
                name: tag.to_string(),
                text: following,
                has_children: false,
            }),
        }
    }
    shop
}

/// The distinct lists of element names the entries have.
pub fn shapes(entries: &[Vec<Field>]) -> BTreeSet<Vec<&str>> {
    entries
        .iter()
        .map(|e| e.iter().map(|(n, _)| n.as_str()).collect())
        .collect()
}

/// The distinct values the entries state for the element `name`.
pub fn values<'a>(entries: &'a [Vec<Field>], name: &str) -> BTreeSet<&'a str> {
    entries
        .iter()
        .flatten()
        .filter(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// How many entries the two lists share, counting an entry listed twice twice.
pub fn common(a: &[Vec<Field>], b: &[Vec<Field>]) -> usize {
    let mut left: std::collections::BTreeMap<&Vec<Field>, usize> = Default::default();
    for entry in a {
        *left.entry(entry).or_default() += 1;
    }
    let mut shared = 0;
    for entry in b {
        if let Some(count) = left.get_mut(entry).filter(|c| **c > 0) {
            *count -= 1;
            shared += 1;
        }
    }
    shared
}
