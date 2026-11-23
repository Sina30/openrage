//! The catalog file: the part of gettext's `.po` a translation needs, read by [`parse`].

use std::collections::BTreeMap;

/// One text and what it is called in the language of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The English text, which is the key.
    pub msgid: String,
    /// The English text of the other number, when the entry has plural forms.
    pub plural: Option<String>,
    /// One text for a plain entry, one per form for a plural one.
    pub forms: Vec<String>,
}

/// A parsed file: its header lines and its entries in file order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub header: BTreeMap<String, String>,
    pub entries: Vec<Entry>,
}

/// Which line of a file could not be read, and why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {what}")]
pub struct Error {
    pub line: usize,
    pub what: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Id,
    Plural,
    Str(usize),
}

#[derive(Default)]
struct Open {
    id: Option<String>,
    plural: Option<String>,
    forms: Vec<String>,
    last: Option<Field>,
    line: usize,
}

impl Open {
    fn field(&mut self, field: Field) -> Option<&mut String> {
        match field {
            Field::Id => self.id.as_mut(),
            Field::Plural => self.plural.as_mut(),
            Field::Str(i) => self.forms.get_mut(i),
        }
    }
}

/// `text` as a catalog: `msgid`, `msgid_plural`, `msgstr` and `msgstr[N]` lines of quoted strings, `#` comments, and a first entry with no `msgid` whose `msgstr` holds the header as `Name: value` lines.
pub fn parse(text: &str) -> Result<Parsed, Error> {
    let mut out = Parsed::default();
    let mut open = Open::default();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let raw = raw.trim();
        let fail = |what: &str| Error {
            line,
            what: what.to_string(),
        };
        if raw.is_empty() {
            finish(&mut out, std::mem::take(&mut open))?;
            continue;
        }
        if raw.starts_with('#') {
            continue;
        }
        if raw.starts_with('"') {
            let piece = unquote(raw).ok_or_else(|| fail("a string that is not closed"))?;
            let field = open
                .last
                .ok_or_else(|| fail("a string with nothing before it"))?;
            open.field(field)
                .ok_or_else(|| fail("a string with nothing before it"))?
                .push_str(&piece);
            continue;
        }
        let (word, rest) = raw
            .split_once(char::is_whitespace)
            .ok_or_else(|| fail("a line that is not a keyword and a string"))?;
        let value = unquote(rest.trim()).ok_or_else(|| fail("a string that is not closed"))?;
        if open.line == 0 {
            open.line = line;
        }
        let taken = |slot: &Option<String>| slot.is_some();
        match word {
            "msgid" => {
                if taken(&open.id) {
                    finish(&mut out, std::mem::take(&mut open))?;
                    open.line = line;
                }
                open.id = Some(value);
                open.last = Some(Field::Id);
            }
            "msgid_plural" => {
                if open.id.is_none() || taken(&open.plural) {
                    return Err(fail("msgid_plural needs one msgid before it"));
                }
                open.plural = Some(value);
                open.last = Some(Field::Plural);
            }
            "msgstr" => {
                if open.id.is_none() || !open.forms.is_empty() {
                    return Err(fail("msgstr needs one msgid before it"));
                }
                open.forms.push(value);
                open.last = Some(Field::Str(0));
            }
            _ => {
                let number = word
                    .strip_prefix("msgstr[")
                    .and_then(|w| w.strip_suffix(']'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .ok_or_else(|| fail("a keyword this reader does not know"))?;
                if open.plural.is_none() || number != open.forms.len() {
                    return Err(fail("msgstr[N] follows msgid_plural, in order from 0"));
                }
                open.forms.push(value);
                open.last = Some(Field::Str(number));
            }
        }
    }
    finish(&mut out, open)?;
    Ok(out)
}

fn finish(out: &mut Parsed, open: Open) -> Result<(), Error> {
    let Some(msgid) = open.id else {
        return Ok(());
    };
    let error = |what: &str| Error {
        line: open.line,
        what: what.to_string(),
    };
    if open.forms.is_empty() {
        return Err(error("an entry with no msgstr"));
    }
    if open.plural.is_some() && open.forms.len() < 2 {
        return Err(error("a plural entry needs msgstr[0] and msgstr[1]"));
    }
    if msgid.is_empty() {
        for line in open.forms[0].lines() {
            if let Some((name, value)) = line.split_once(':') {
                out.header
                    .insert(name.trim().to_string(), value.trim().to_string());
            }
        }
        return Ok(());
    }
    out.entries.push(Entry {
        msgid,
        plural: open.plural,
        forms: open.forms,
    });
    Ok(())
}

/// The text between the quotes of `quoted`, with its escapes read; `None` when it is not one quoted string.
fn unquote(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next()? {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '"' => '"',
            '\\' => '\\',
            _ => return None,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# a comment\n\
        msgid \"\"\n\
        msgstr \"\"\n\
        \"Language-Name: Fran\u{e7}ais\\n\"\n\
        \"Plural-Forms: n > 1\\n\"\n\
        \n\
        msgid \"Open\"\n\
        msgstr \"Ouvrir\"\n\
        \n\
        #, fuzzy\n\
        msgid \"{count} file\"\n\
        msgid_plural \"{count} files\"\n\
        msgstr[0] \"{count} fichier\"\n\
        msgstr[1] \"{count} fichiers\"\n\
        \n\
        msgid \"Two \"\n\
        \"lines\\nof \\\"text\\\"\"\n\
        msgstr \"Deux\"\n";

    #[test]
    fn a_file_reads_as_its_header_and_its_entries_in_order() {
        let parsed = parse(FILE).expect("a catalog");
        assert_eq!(parsed.header["Language-Name"], "Fran\u{e7}ais");
        assert_eq!(parsed.header["Plural-Forms"], "n > 1");
        let ids: Vec<&str> = parsed.entries.iter().map(|e| e.msgid.as_str()).collect();
        assert_eq!(ids, ["Open", "{count} file", "Two lines\nof \"text\""]);
        assert_eq!(parsed.entries[0].forms, ["Ouvrir"]);
        assert_eq!(parsed.entries[1].plural.as_deref(), Some("{count} files"));
        assert_eq!(
            parsed.entries[1].forms,
            ["{count} fichier", "{count} fichiers"]
        );
    }

    #[test]
    fn a_line_that_cannot_be_read_is_named_by_its_number() {
        for (text, line) in [
            ("msgid \"a\"\nmsgstr \"b", 2),
            ("msgstr \"b\"", 1),
            ("msgid \"a\"\nmsgid_plural \"b\"\nmsgstr[1] \"c\"", 3),
            ("msgid \"a\"\nmsgstr \"\\q\"", 2),
            ("nonsense", 1),
            ("\n\nmsgid \"a\"\n", 3),
        ] {
            let error = parse(text).expect_err(text);
            assert_eq!(error.line, line, "{text:?}: {error}");
        }
    }
}
