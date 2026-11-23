//! A minimal XML tree the four DLC documents are read through.

use quick_xml::events::Event;
use quick_xml::Reader;

use super::xml::{decode_attr, local_name, resolve_general_ref, xml_err, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub(crate) fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn child(&self, name: &str) -> Option<&Node> {
        self.children
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub(crate) fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children
            .iter()
            .filter(move |c| c.name.eq_ignore_ascii_case(name))
    }

    /// `<Item>` texts under `name`, which is how every list in these documents
    /// is spelled.
    ///
    /// ⚠ **Every `name` section, not the first.** `mplowrider`'s `content.xml`
    /// carries two `<dataFiles>` blocks, and a reader that takes the first
    /// drops the second without raising.
    pub(crate) fn item_texts(&self, name: &str) -> Vec<super::model::ListItem> {
        self.items(name)
            .filter(|c| !c.text.trim().is_empty())
            .map(|c| super::model::ListItem {
                value: c.text.trim().to_string(),
                platform: c.attr("platform").unwrap_or_default().to_string(),
            })
            .collect()
    }

    pub(crate) fn items<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children_named(name)
            .flat_map(|section| section.children_named("Item"))
    }

    pub(crate) fn text_of(&self, name: &str) -> String {
        self.child(name)
            .map(|c| c.text.trim().to_string())
            .unwrap_or_default()
    }

    /// The value a `<tag value="…" />` or `<tag>…</tag>` carries, if it carries
    /// one at all.
    ///
    /// ⚠ **`None` and `Some("")` are different facts.** A shipped
    /// `<requiredVersion />` is the element present and empty; a missing
    /// element is the element absent. A writer that cannot tell them apart
    /// invents one of them.
    pub(crate) fn scalar(&self, name: &str) -> Option<String> {
        let node = self.child(name)?;
        if let Some(v) = node.attr("value") {
            return Some(v.trim().to_string());
        }
        Some(node.text.trim().to_string())
    }

    pub(crate) fn int(&self, name: &str) -> Option<i64> {
        self.scalar(name)?.parse().ok()
    }

    /// A flag the document may simply not mention.
    pub(crate) fn flag(&self, name: &str) -> Option<bool> {
        let v = self.scalar(name)?;
        (!v.is_empty()).then(|| super::xml::parse_bool(&v))
    }
}

/// [`parse_tree`], refusing a document whose root element is none of `roots`.
///
/// ⚠ **Without this a document of another kind reads as an empty model**: every
/// lookup misses and the reader returns `Ok` with nothing in it.
pub(crate) fn parse_rooted(bytes: &[u8], what: &str, roots: &[&str]) -> Result<Node> {
    let root = parse_tree(bytes, what)?;
    if roots.iter().any(|r| root.name.eq_ignore_ascii_case(r)) {
        return Ok(root);
    }
    let wanted = roots
        .iter()
        .map(|r| format!("`{r}`"))
        .collect::<Vec<_>>()
        .join(" or ");
    if root.name.is_empty() {
        return Err(xml_err(format!(
            "{what}: no root element, expected {wanted}"
        )));
    }
    Err(xml_err(format!(
        "{what}: the root element is `{}`, not {wanted}",
        root.name
    )))
}

/// Paths of the elements and attributes under `node` that `known` does not list, each once; subtrees at `owned` paths are left to their own reader.
///
/// ⚠ A `value` attribute counts as read on any known element but an `Item`,
/// because leaves are read through [`Node::scalar`] and items never are.
pub(crate) fn unread(node: &Node, known: &[&str], owned: &[&str]) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    collect_unread(node, "", known, owned, &mut out);
    out.into_iter().collect()
}

fn collect_unread(
    node: &Node,
    prefix: &str,
    known: &[&str],
    owned: &[&str],
    out: &mut std::collections::BTreeSet<String>,
) {
    let listed = |list: &[&str], path: &str| list.iter().any(|k| k.eq_ignore_ascii_case(path));
    for (key, _) in &node.attrs {
        if key.eq_ignore_ascii_case("value") && !node.name.eq_ignore_ascii_case("Item") {
            continue;
        }
        let path = format!("{prefix}@{key}");
        if !listed(known, &path) {
            out.insert(path);
        }
    }
    for child in &node.children {
        let path = if prefix.is_empty() {
            child.name.clone()
        } else {
            format!("{prefix}/{}", child.name)
        };
        if !listed(known, &path) {
            out.insert(path);
        } else if !listed(owned, &path) {
            collect_unread(child, &path, known, owned, out);
        }
    }
}

/// ⚠ Dropping a [`Node`] recurses once per level, so a deeper document would overflow the stack.
const MAX_DEPTH: usize = 256;

pub(crate) fn parse_tree(bytes: &[u8], what: &str) -> Result<Node> {
    let mut reader = Reader::from_reader(strip_bom(bytes));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut root = Node::default();
    let mut stack: Vec<Node> = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| xml_err(format!("{what}: {e}")))?
        {
            Event::Eof => break,
            Event::Start(_) if stack.len() == MAX_DEPTH => {
                return Err(xml_err(format!(
                    "{what}: elements nest deeper than {MAX_DEPTH} levels"
                )));
            }
            Event::Start(e) => stack.push(start_node(&e)),
            Event::Empty(e) => {
                let node = start_node(&e);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = node,
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            // CDATA is literal by definition, so it is appended as it stands.
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            Event::GeneralRef(r) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&resolve_general_ref(&r));
                }
            }
            Event::End(_) => {
                let Some(node) = stack.pop() else { continue };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = node,
                }
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(root)
}

fn start_node(e: &quick_xml::events::BytesStart<'_>) -> Node {
    Node {
        name: local_name(e.name()),
        attrs: e
            .attributes()
            .flatten()
            .map(|a| (local_name(a.key), decode_attr(&a.value)))
            .collect(),
        text: String::new(),
        children: Vec::new(),
    }
}

/// ⚠ **Every shipped `setup2.xml` starts with a UTF-8 BOM** and quick-xml
/// reads it as text content of nothing, which puts `\u{feff}` at the front of
/// the first value.
fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}
