//! GTA VI's `RBF0` documents, its interface definitions among them, read as a strict tree: each
//! element with its attributes, and its child elements, values and texts in the order stored.
//! The document names its fields and carries no schema, so a value has a type and no meaning.

/// The bytes after the magic of a document whose integers and floats are 8 bytes wide.
const WIDE: [u8; 4] = [0xFB, 0xFF, 0x01, 0x00];
/// Elements open inside one another at most: far past any document measured, so a tree's
/// depth never follows a hostile document's.
pub const MAX_DEPTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UiTreeError {
    #[error("not an RBF0 document: its first four bytes are not RBF0")]
    NotRbf,
    #[error("RBF0: FB FF follows the magic with {0:02X?}, where a wide document states 01 00")]
    Marker([u8; 2]),
    #[error("RBF0: {what} at {at:#X} runs past the document's end")]
    Truncated { at: usize, what: &'static str },
    #[error("RBF0: the record at {at:#X} names descriptor {index}, where {named} are named")]
    Descriptor { at: usize, index: u8, named: usize },
    #[error("RBF0: the record at {at:#X} has the type {kind:#04X}, which no rule reads")]
    RecordType { at: usize, kind: u8 },
    #[error(
        "RBF0: the element at {at:#X} states {words:?} before its attribute count, where an element states zeros"
    )]
    ElementWords { at: usize, words: [u16; 2] },
    #[error(
        "RBF0: the element at {at:#X} declares {declared} attributes and {found} values lead it"
    )]
    Attributes {
        at: usize,
        declared: u16,
        found: u16,
    },
    #[error("RBF0: {what} at {at:#X} lies in no open element")]
    Outside { at: usize, what: &'static str },
    #[error("RBF0: the name or string at {at:#X} is not UTF-8")]
    NotUtf8 { at: usize },
    #[error(
        "RBF0: three floats at {at:#X} of a wide document, where their width was never measured"
    )]
    WideFloats { at: usize },
    #[error("RBF0: the element at {at:#X} opens more than {} deep", MAX_DEPTH)]
    Depth { at: usize },
    #[error("RBF0: {left} bytes follow the root element's close at {at:#X}")]
    Trailing { at: usize, left: usize },
}

type Result<T> = std::result::Result<T, UiTreeError>;

/// A document: its root element, and whether its integers and floats are 8 bytes or 4.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTree {
    pub wide: bool,
    pub root: Element,
}

/// An element: the values its header declares as attributes, then everything else it holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, Value)>,
    pub content: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    /// A value after the attributes, under its field's name.
    Value(String, Value),
    Text(Text),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Integer(u64),
    Bool(bool),
    Float(f64),
    /// Three 4-byte floats, which only a narrow document is read to hold.
    Floats([f32; 3]),
    String(String),
}

/// An element's text: UTF-8 before the one NUL that ends it, else the bytes as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    Utf8(String),
    Bytes(Vec<u8>),
}

/// How much a tree holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub elements: usize,
    pub attributes: usize,
    pub values: usize,
    pub texts: usize,
    /// The deepest element, the root being 1.
    pub depth: usize,
}

/// Whether `head` opens an RBF0 document.
pub fn is_rbf(head: &[u8]) -> bool {
    head.starts_with(b"RBF0")
}

struct Bytes<'a> {
    v: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8]> {
        let at = self.at;
        let bytes = (at.checked_add(len))
            .and_then(|end| self.v.get(at..end))
            .ok_or(UiTreeError::Truncated { at, what })?;
        self.at += len;
        Ok(bytes)
    }

    fn uint(&mut self, len: usize, what: &'static str) -> Result<u64> {
        let bytes = self.take(len, what)?;
        Ok(bytes.iter().rev().fold(0, |n, b| n << 8 | u64::from(*b)))
    }

    /// A `u16` length and that many bytes of UTF-8.
    fn string(&mut self, what: &'static str) -> Result<String> {
        let at = self.at;
        let len = self.uint(2, what)? as usize;
        let bytes = self.take(len, what)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| UiTreeError::NotUtf8 { at })
    }
}

/// An element still open: where it opened, the attributes it declares, and those not yet read.
struct Open {
    element: Element,
    at: usize,
    declared: u16,
    owed: u16,
}

impl Open {
    /// Refuses a record that is no value while declared attributes are still owed.
    fn attributes_read(&self) -> Result<()> {
        if self.owed == 0 {
            return Ok(());
        }
        Err(UiTreeError::Attributes {
            at: self.at,
            declared: self.declared,
            found: self.declared - self.owed,
        })
    }
}

fn text_of(bytes: &[u8]) -> Text {
    let utf8 = (bytes.split_last())
        .filter(|(nul, text)| **nul == 0 && !text.contains(&0))
        .and_then(|(_, text)| std::str::from_utf8(text).ok());
    match utf8 {
        Some(text) => Text::Utf8(text.to_string()),
        None => Text::Bytes(bytes.to_vec()),
    }
}

/// The tree the whole document `v` holds, refused where a record leaves the rules measured.
pub fn read(v: &[u8]) -> Result<UiTree> {
    if !is_rbf(v) {
        return Err(UiTreeError::NotRbf);
    }
    let mut r = Bytes { v, at: 4 };
    let wide = v.get(4..6) == Some(&WIDE[..2]);
    if wide {
        let version = r.take(4, "the marker after the magic")?;
        if version != WIDE {
            return Err(UiTreeError::Marker([version[2], version[3]]));
        }
    }
    let width = if wide { 8 } else { 4 };
    let mut names: Vec<String> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    loop {
        let at = r.at;
        let record = r.take(2, "a record")?;
        let (index, kind) = (record[0], record[1]);
        if (index, kind) == (0xFF, 0xFF) {
            let closed = open.pop().ok_or(UiTreeError::Outside {
                at,
                what: "a close",
            })?;
            closed.attributes_read()?;
            match open.last_mut() {
                Some(parent) => parent.element.content.push(Node::Element(closed.element)),
                None if r.at != v.len() => {
                    return Err(UiTreeError::Trailing {
                        at: r.at,
                        left: v.len() - r.at,
                    })
                }
                None => {
                    return Ok(UiTree {
                        wide,
                        root: closed.element,
                    })
                }
            }
            continue;
        }
        if (index, kind) == (0xFD, 0xFF) {
            let len = r.uint(4, "a text's length")? as usize;
            let text = text_of(r.take(len, "a text")?);
            let parent = open
                .last_mut()
                .ok_or(UiTreeError::Outside { at, what: "a text" })?;
            parent.attributes_read()?;
            parent.element.content.push(Node::Text(text));
            continue;
        }
        // A record names a descriptor already named, or the next one, whose name then follows.
        let named = names.len();
        if usize::from(index) > named {
            return Err(UiTreeError::Descriptor { at, index, named });
        }
        if usize::from(index) == named {
            names.push(r.string("a descriptor's name")?);
        }
        let name = names[usize::from(index)].clone();
        if kind == 0x00 {
            let words = [
                r.uint(2, "an element's header")? as u16,
                r.uint(2, "an element's header")? as u16,
            ];
            let declared = r.uint(2, "an element's header")? as u16;
            if words != [0, 0] {
                return Err(UiTreeError::ElementWords { at, words });
            }
            if let Some(parent) = open.last() {
                parent.attributes_read()?;
            }
            if open.len() == MAX_DEPTH {
                return Err(UiTreeError::Depth { at });
            }
            open.push(Open {
                element: Element {
                    name,
                    ..Element::default()
                },
                at,
                declared,
                owed: declared,
            });
            continue;
        }
        let value = match kind {
            0x10 => Value::Integer(r.uint(width, "an integer")?),
            0x20 => Value::Bool(true),
            0x30 => Value::Bool(false),
            0x40 if wide => Value::Float(f64::from_bits(r.uint(8, "a float")?)),
            0x40 => Value::Float(f64::from(f32::from_bits(r.uint(4, "a float")? as u32))),
            0x50 if wide => return Err(UiTreeError::WideFloats { at }),
            0x50 => {
                let mut float = || r.uint(4, "three floats").map(|w| f32::from_bits(w as u32));
                Value::Floats([float()?, float()?, float()?])
            }
            0x60 => Value::String(r.string("a string")?),
            kind => return Err(UiTreeError::RecordType { at, kind }),
        };
        let parent = open.last_mut().ok_or(UiTreeError::Outside {
            at,
            what: "a value",
        })?;
        if parent.owed > 0 {
            parent.owed -= 1;
            parent.element.attributes.push((name, value));
        } else {
            parent.element.content.push(Node::Value(name, value));
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer(n) => write!(f, "{n}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Float(x) => write!(f, "{x}"),
            Self::Floats([x, y, z]) => write!(f, "{x} {y} {z}"),
            Self::String(s) => write!(f, "{s:?}"),
        }
    }
}

impl Value {
    /// The type's name, as a listing says it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Integer(_) => "integer",
            Self::Bool(_) => "bool",
            Self::Float(_) => "float",
            Self::Floats(_) => "floats",
            Self::String(_) => "string",
        }
    }
}

impl Element {
    /// The element's texts that are UTF-8, in order.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.content.iter().filter_map(|node| match node {
            Node::Text(Text::Utf8(text)) => Some(text.as_str()),
            _ => None,
        })
    }

    /// The element's child elements, in order.
    pub fn children(&self) -> impl Iterator<Item = &Element> {
        self.content.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            _ => None,
        })
    }
}

impl UiTree {
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        // Depth is bounded by `MAX_DEPTH`, so the walk keeps its own short stack.
        let mut walk = vec![(&self.root, 1)];
        while let Some((element, depth)) = walk.pop() {
            counts.elements += 1;
            counts.attributes += element.attributes.len();
            counts.depth = counts.depth.max(depth);
            for node in &element.content {
                match node {
                    Node::Element(child) => walk.push((child, depth + 1)),
                    Node::Value(..) => counts.values += 1,
                    Node::Text(_) => counts.texts += 1,
                }
            }
        }
        counts
    }
}

#[cfg(test)]
#[path = "ui_tree_tests.rs"]
mod tests;
