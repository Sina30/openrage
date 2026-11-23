//! The language of a GTA VI text table. A table states none: an archive holds one nested archive
//! of text tables per language, and a change set registers those archives by name, one language
//! each. The n-th such archive of a file is the n-th the change set registers, which holds only
//! while the file holds as many as are registered; the names of the archives are encrypted.

use crate::container::rpf::{Rpf9BodyKind, Rpf9Content, Rpf9ScanFind, Rpf9ZlibRun};

/// The file type a change set registers a language's text archive under.
const TEXT_FILE_TYPE: &str = "TEXTFILE_METAFILE";

/// The languages the change set `xml` registers text archives for, in the order it states them:
/// the file name of each such item, without its folder and what follows its first `_` or `.`.
pub fn registered(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    // An item states its file's name before its type.
    let mut items = xml.split("<Item>");
    items.next();
    for item in items {
        let item = item.split("</Item>").next().unwrap_or(item);
        let field = |tag: &str| {
            let after = item.split_once(&format!("<{tag}>"))?.1;
            Some(after.split_once(&format!("</{tag}>"))?.0.trim())
        };
        if field("fileType") != Some(TEXT_FILE_TYPE) {
            continue;
        }
        let Some(file) = field("filename") else {
            continue;
        };
        let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let language = name.split(['_', '.']).next().unwrap_or(name);
        if !language.is_empty() {
            out.push(language.to_ascii_lowercase());
        }
    }
    out
}

/// What a scanned body is to a language archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scanned {
    /// A nested archive's header.
    Archive,
    TextTable,
    /// A body a language archive holds beside its tables: metadata, or its own names.
    Beside,
    Other,
}

impl Scanned {
    pub fn of(find: &Rpf9ScanFind) -> Self {
        match find.kind {
            Rpf9BodyKind::Archive(_) => Self::Archive,
            Rpf9BodyKind::Names { .. } => Self::Beside,
            Rpf9BodyKind::Zlib(Rpf9ZlibRun { content, .. })
            | Rpf9BodyKind::RawDeflate { content } => match content {
                Rpf9Content::TextTable => Self::TextTable,
                Rpf9Content::Pso => Self::Beside,
                _ => Self::Other,
            },
            _ => Self::Other,
        }
    }
}

/// The language archives of one file: the nested archives that hold text tables and no body of
/// another kind, in offset order, each from its header to the next archive's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguageArchives {
    spans: Vec<(u64, u64)>,
}

impl LanguageArchives {
    /// The language archives among `finds`, the bodies of one file in offset order.
    pub fn of(finds: impl IntoIterator<Item = (u64, Scanned)>) -> Self {
        let mut spans = Vec::new();
        // The archive being read: its header's offset, and whether it holds tables and only them.
        let mut open: Option<(u64, bool, bool)> = None;
        let mut close = |read: Option<(u64, bool, bool)>, end: u64| {
            if let Some((start, true, true)) = read {
                spans.push((start, end));
            }
        };
        for (offset, scanned) in finds {
            match scanned {
                Scanned::Archive => {
                    close(open.take(), offset);
                    open = Some((offset, false, true));
                }
                Scanned::TextTable => {
                    if let Some(read) = &mut open {
                        read.1 = true;
                    }
                }
                Scanned::Other => {
                    if let Some(read) = &mut open {
                        read.2 = false;
                    }
                }
                Scanned::Beside => {}
            }
        }
        close(open, u64::MAX);
        Self { spans }
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Which language archive, from 0, holds the body at `offset`.
    pub fn ordinal(&self, offset: u64) -> Option<usize> {
        let at = self.spans.partition_point(|span| span.0 <= offset);
        let before = at.checked_sub(1)?;
        (offset < self.spans[before].1).then_some(before)
    }

    /// The language of the table at `offset` among `registered`: `None` unless the file holds
    /// exactly as many language archives as are registered, since a file scanned in part would
    /// give its archives the wrong places.
    pub fn language<'a>(&self, offset: u64, registered: &'a [String]) -> Option<&'a str> {
        if self.spans.len() != registered.len() {
            return None;
        }
        registered.get(self.ordinal(offset)?).map(String::as_str)
    }
}

#[cfg(test)]
#[path = "text_language_tests.rs"]
mod tests;
