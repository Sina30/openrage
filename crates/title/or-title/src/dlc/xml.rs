//! The XML primitives the three DLC documents share, in both directions.

use quick_xml::escape::{resolve_predefined_entity, unescape};
use quick_xml::events::BytesRef;

use or_io::error::IoError;

pub type Result<T> = std::result::Result<T, IoError>;

pub(crate) fn xml_err(msg: impl Into<String>) -> IoError {
    IoError::InvalidData {
        context: msg.into(),
    }
}

pub(crate) fn local_name(name: quick_xml::name::QName<'_>) -> String {
    let full = name.as_ref();
    let local = match full.iter().rposition(|&b| b == b':') {
        Some(pos) => &full[pos + 1..],
        None => full,
    };
    String::from_utf8_lossy(local).into_owned()
}

/// ⚠ **An entity reference is a separate event, not part of the text.**
/// quick-xml 0.38 splits `&amp;` out of `Event::Text` into `Event::GeneralRef`
/// carrying only the name, so a loop that handles `Text` and never this
/// receives the surrounding characters and silently drops the entity.
///
/// An entity neither quick-xml's predefined table nor a character reference
/// resolves keeps its own source spelling, because the writer re-escapes on the
/// way out and dropping it would cost the value.
pub(crate) fn resolve_general_ref(entity: &BytesRef<'_>) -> String {
    let Ok(name) = entity.decode() else {
        return String::new();
    };
    if let Ok(Some(ch)) = entity.resolve_char_ref() {
        return ch.to_string();
    }
    match resolve_predefined_entity(&name) {
        Some(text) => text.to_string(),
        None => format!("&{name};"),
    }
}

/// ⚠ **Attribute values are the one place quick-xml leaves the escaping in**,
/// so they need the opposite treatment to text. An unresolvable entity fails
/// the whole value rather than itself, and a lenient reader keeps the source
/// text over losing the rest of the string with it.
pub(crate) fn decode_attr(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    match unescape(&raw) {
        Ok(decoded) => decoded.into_owned(),
        Err(_) => raw.into_owned(),
    }
}

pub(crate) fn parse_bool(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1")
}

/// The one spelling of a boolean this writer emits, matching what ships.
pub(crate) fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// Escape for element text and attribute values alike.
///
/// ⚠ **`'` and `"` are escaped too.** The reader resolves all five predefined
/// entities, so anything short of all five makes the pair asymmetric in the
/// direction that loses data — an attribute value holding a quote.
pub(crate) fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// An indent-aware XML sink the three writers share.
pub(crate) struct XmlOut {
    buf: String,
    depth: usize,
}

impl XmlOut {
    pub(crate) fn document(root: &str) -> Self {
        let mut out = Self {
            buf: String::new(),
            depth: 0,
        };
        out.buf
            .push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.open(root);
        out
    }

    pub(crate) fn finish(mut self, root: &str) -> String {
        self.close(root);
        self.buf
    }

    fn pad(&mut self) {
        for _ in 0..self.depth {
            self.buf.push_str("  ");
        }
    }

    pub(crate) fn open(&mut self, tag: &str) {
        self.pad();
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push_str(">\n");
        self.depth += 1;
    }

    pub(crate) fn close(&mut self, tag: &str) {
        self.depth = self.depth.saturating_sub(1);
        self.pad();
        self.buf.push_str("</");
        self.buf.push_str(tag);
        self.buf.push_str(">\n");
    }

    pub(crate) fn empty(&mut self, tag: &str) {
        self.pad();
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push_str(" />\n");
    }

    pub(crate) fn text(&mut self, tag: &str, value: &str) {
        if value.is_empty() {
            self.empty(tag);
            return;
        }
        self.pad();
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push('>');
        self.buf.push_str(&escape_xml(value));
        self.buf.push_str("</");
        self.buf.push_str(tag);
        self.buf.push_str(">\n");
    }

    pub(crate) fn value(&mut self, tag: &str, value: &str) {
        self.pad();
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push_str(" value=\"");
        self.buf.push_str(&escape_xml(value));
        self.buf.push_str("\" />\n");
    }

    pub(crate) fn bool_value(&mut self, tag: &str, value: bool) {
        self.value(tag, bool_text(value));
    }

    pub(crate) fn optional_bool(&mut self, tag: &str, value: Option<bool>) {
        if let Some(v) = value {
            self.bool_value(tag, v);
        }
    }

    pub(crate) fn open_attr(&mut self, tag: &str, key: &str, value: &str) {
        self.pad();
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push(' ');
        self.buf.push_str(key);
        self.buf.push_str("=\"");
        self.buf.push_str(&escape_xml(value));
        self.buf.push_str("\">\n");
        self.depth += 1;
    }

    /// An opening tag with each attribute of `attrs` whose value is not empty.
    pub(crate) fn open_attrs(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.pad();
        self.buf.push('<');
        self.buf.push_str(tag);
        for (key, value) in attrs.iter().filter(|(_, v)| !v.is_empty()) {
            self.buf
                .push_str(&format!(" {key}=\"{}\"", escape_xml(value)));
        }
        self.buf.push_str(">\n");
        self.depth += 1;
    }

    pub(crate) fn int_value(&mut self, tag: &str, value: i64) {
        self.value(tag, &value.to_string());
    }

    /// The one element in these documents whose payload is an attribute.
    ///
    /// ⚠ **`xpointer` is not carried.** Every shipped occurrence spells it
    /// `xpointer(/dataFiles)`, and the reader keys on `href` alone, so a model
    /// field for it would be a constant the reader never fills.
    pub(crate) fn include(&mut self, href: &str) {
        self.pad();
        self.buf.push_str("<xi:include href=\"");
        self.buf.push_str(&escape_xml(href));
        self.buf
            .push_str("\" xpointer=\"xpointer(/dataFiles)\" />\n");
    }

    /// A list of `<Item>text</Item>`, or the empty element when there are none.
    ///
    /// ⚠ **An empty list is `<tag />`, not `<tag></tag>`.** Every shipped
    /// document spells it that way and the parser treats them alike, so this is
    /// about the written file matching what a pack author will diff it against.
    pub(crate) fn item_list(&mut self, tag: &str, items: &[super::model::ListItem]) {
        if items.is_empty() {
            self.empty(tag);
            return;
        }
        self.open(tag);
        for item in items {
            if item.platform.is_empty() {
                self.text("Item", &item.value);
            } else {
                self.pad();
                self.buf.push_str("<Item platform=\"");
                self.buf.push_str(&escape_xml(&item.platform));
                self.buf.push_str("\">");
                self.buf.push_str(&escape_xml(&item.value));
                self.buf.push_str("</Item>\n");
            }
        }
        self.close(tag);
    }
}
