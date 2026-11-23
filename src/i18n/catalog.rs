//! A language: its name, how it counts, and what each English text is called in it.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use super::po::{self, Parsed};

/// Which form of a plural entry a number takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plural {
    /// One is the first form, every other number the second: English, and the header `n != 1`.
    OneOther,
    /// Nought and one are the first form: French, and the header `n > 1`.
    ZeroAndOne,
    /// Three forms, as Russian counts: a number that ends in 1 but not in 11, one that ends in 2 to 4 but not in 12 to 14, and every other.
    OneFewMany,
}

impl Plural {
    pub fn form(self, n: u64) -> usize {
        match self {
            Self::OneOther => usize::from(n != 1),
            Self::ZeroAndOne => usize::from(n > 1),
            Self::OneFewMany => match (n % 10, n % 100) {
                (1, tens) if tens != 11 => 0,
                (2..=4, tens) if !(12..=14).contains(&tens) => 1,
                _ => 2,
            },
        }
    }

    pub(super) fn from_header(rule: &str) -> Option<Self> {
        let rule = rule.replace(' ', "");
        let rule = rule.rsplit("plural=").next().unwrap_or_default();
        match rule.trim_matches(['(', ')', ';']) {
            "n!=1" => Some(Self::OneOther),
            "n>1" => Some(Self::ZeroAndOne),
            "n%10==1&&n%100!=11?0:n%10>=2&&n%10<=4&&(n%100<10||n%100>=20)?1:2" => {
                Some(Self::OneFewMany)
            }
            _ => None,
        }
    }
}

/// What a language calls each English text.
#[derive(Debug, Default)]
struct Texts {
    singular: HashMap<&'static str, &'static str>,
    plurals: HashMap<&'static str, [&'static str; 3]>,
    /// Every text given, so a text that was translated once and is asked for again is no fault.
    given: HashSet<&'static str>,
}

impl Texts {
    fn take(&mut self, entries: &[po::Entry]) {
        for entry in entries {
            let key = kept(&entry.msgid);
            match (&entry.plural, entry.forms.as_slice()) {
                (Some(_), [one, other, rest @ ..]) => {
                    // An entry of two forms has no third, and its second stands in for it.
                    let other = kept(other);
                    let forms = [kept(one), other, rest.first().map_or(other, |m| kept(m))];
                    self.given.extend(forms);
                    self.plurals.insert(key, forms);
                }
                (None, [text]) if !text.is_empty() => {
                    let text = kept(text);
                    self.given.insert(text);
                    self.singular.insert(key, text);
                }
                _ => {}
            }
        }
    }
}

/// One language of the interface. English holds no entries of its own, because its text is the key.
#[derive(Debug)]
pub struct Catalog {
    /// The short code of the file name: `en`, `fr`.
    pub code: String,
    /// The language in its own words: `English`, `Français`.
    pub name: String,
    pub plural: Plural,
    /// What the files read at launch say, which wins over the built-in file.
    texts: Texts,
    /// The built-in file and what it says, read when a text of the language is first asked for: a launch pays for the language it draws in, not for every one.
    built_in: Option<(&'static str, OnceLock<Texts>)>,
}

/// Text that lives as long as the process, which a catalog does: each is read once, so nothing is lost by never freeing it.
fn kept(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

/// The header entry of `text` alone: what comes before its second `msgid`.
fn head(text: &str) -> &str {
    let first = text.find("msgid ").map_or(0, |at| at + 1);
    text[first..]
        .find("\nmsgid ")
        .map_or(text, |at| &text[..first + at])
}

impl Catalog {
    /// English: every text is its own key, so there is nothing to look up.
    pub fn english() -> Self {
        Self {
            code: "en".to_string(),
            name: "English".to_string(),
            plural: Plural::OneOther,
            texts: Texts::default(),
            built_in: None,
        }
    }

    /// The language `code` as `text` says it, which a later file of the same code is laid over.
    pub fn from_po(code: &str, text: &str) -> Result<Self, po::Error> {
        let mut catalog = Self {
            code: code.to_string(),
            name: code.to_string(),
            ..Self::english()
        };
        catalog.lay_over(&po::parse(text)?);
        Ok(catalog)
    }

    /// The built-in language `code`: its header now, and its entries when the first of them is asked for.
    fn built_in(code: &str, text: &'static str) -> Result<Self, po::Error> {
        let mut catalog = Self::from_po(code, head(text))?;
        catalog.built_in = Some((text, OnceLock::new()));
        Ok(catalog)
    }

    /// `parsed` over what this holds: its header and every entry it has.
    pub fn lay_over(&mut self, parsed: &Parsed) {
        if let Some(name) = parsed.header.get("Language-Name") {
            self.name = name.clone();
        }
        if let Some(rule) = parsed.header.get("Plural-Forms") {
            match Plural::from_header(rule) {
                Some(rule) => self.plural = rule,
                None => tracing::warn!(
                    language = %self.code,
                    rule = %rule,
                    "a plural rule this reader does not know; \"n != 1\" is used"
                ),
            }
        }
        self.texts.take(&parsed.entries);
    }

    /// What was laid over, then what the built-in file says.
    fn texts(&self) -> impl Iterator<Item = &Texts> {
        let built_in = self.built_in.as_ref().map(|(text, read)| {
            read.get_or_init(|| {
                let mut texts = Texts::default();
                match po::parse(text) {
                    Ok(parsed) => texts.take(&parsed.entries),
                    Err(error) => {
                        tracing::error!(language = %self.code, %error, "a built-in catalog does not read")
                    }
                }
                texts
            })
        });
        std::iter::once(&self.texts).chain(built_in)
    }

    pub fn translate(&self, key: &'static str) -> Option<&'static str> {
        self.texts().find_map(|t| t.singular.get(key).copied())
    }

    /// The form of the plural entry `one` that `n` takes.
    pub fn translate_n(&self, one: &'static str, n: u64) -> Option<&'static str> {
        self.texts()
            .find_map(|t| t.plurals.get(one))
            .map(|forms| forms[self.plural.form(n)])
    }

    /// Whether `text` is one this language gave: asking for it again returns it, which is no fault.
    pub fn gives(&self, text: &str) -> bool {
        self.texts().any(|t| t.given.contains(text))
    }

    /// Whether a text missing from this language is no fault: English is the key, so a file for it names only the words it changes.
    pub fn is_english(&self) -> bool {
        self.code == "en"
    }
}

/// The languages the program has: English, the catalogs built into it, and every `.po` file of `folder`, which lies over a built-in catalog of the same code.
pub fn load(embedded: &[(&str, &'static str)], folder: Option<&Path>) -> Vec<Catalog> {
    let mut all = vec![Catalog::english()];
    for (code, text) in embedded {
        match Catalog::built_in(code, text) {
            Ok(catalog) => all.push(catalog),
            Err(error) => {
                tracing::error!(language = %code, %error, "a built-in catalog does not read")
            }
        }
    }
    let Some(files) = folder.and_then(|f| std::fs::read_dir(f).ok()) else {
        return all;
    };
    let mut files: Vec<_> = files
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "po"))
        .collect();
    files.sort();
    for path in files {
        let Some(code) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let code = code.to_ascii_lowercase();
        let parsed = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| po::parse(&t).map_err(|e| e.to_string()))
        {
            Ok(parsed) => parsed,
            Err(error) => {
                tracing::warn!(file = %path.display(), %error, "a language file does not read; it is left out");
                continue;
            }
        };
        let at = match all.iter().position(|c| c.code == code) {
            Some(at) => at,
            None => {
                let mut fresh = Catalog::english();
                fresh.code = code.clone();
                fresh.name = code.clone();
                all.push(fresh);
                all.len() - 1
            }
        };
        all[at].lay_over(&parsed);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const FR: &str = "msgid \"\"\nmsgstr \"\"\n\"Language-Name: Fran\u{e7}ais\\n\"\n\"Plural-Forms: nplurals=2; plural=(n > 1);\\n\"\n\nmsgid \"Open\"\nmsgstr \"Ouvrir\"\n\nmsgid \"{count} file\"\nmsgid_plural \"{count} files\"\nmsgstr[0] \"{count} fichier\"\nmsgstr[1] \"{count} fichiers\"\n";

    #[test]
    fn a_plural_takes_the_form_its_language_counts_with() {
        let fr = Catalog::from_po("fr", FR).expect("a catalog");
        assert_eq!(fr.name, "Fran\u{e7}ais");
        let forms: Vec<_> = [0, 1, 2, 21]
            .iter()
            .map(|&n| fr.translate_n("{count} file", n).unwrap())
            .collect();
        assert_eq!(
            forms,
            [
                "{count} fichier",
                "{count} fichier",
                "{count} fichiers",
                "{count} fichiers"
            ]
        );
        assert_eq!(Plural::OneOther.form(0), 1, "English says 0 files");
        assert_eq!(Plural::OneOther.form(1), 0);
    }

    #[test]
    fn russian_counts_in_three_forms_and_an_entry_of_two_forms_still_answers() {
        let header = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
        let ru = format!(
            "msgid \"\"\nmsgstr \"Plural-Forms: {header}\\n\"\n\nmsgid \"{{count}} file\"\nmsgid_plural \"{{count}} files\"\nmsgstr[0] \"one\"\nmsgstr[1] \"few\"\nmsgstr[2] \"many\"\n\nmsgid \"{{count}} map\"\nmsgid_plural \"{{count}} maps\"\nmsgstr[0] \"one\"\nmsgstr[1] \"other\"\n"
        );
        let ru = Catalog::from_po("ru", &ru).expect("a catalog");
        assert_eq!(ru.plural, Plural::OneFewMany);
        for (n, form) in [
            (0, "many"),
            (1, "one"),
            (2, "few"),
            (4, "few"),
            (5, "many"),
            (11, "many"),
            (12, "many"),
            (14, "many"),
            (21, "one"),
            (22, "few"),
            (25, "many"),
            (111, "many"),
            (112, "many"),
            (121, "one"),
        ] {
            assert_eq!(ru.translate_n("{count} file", n), Some(form), "{n}");
        }
        assert_eq!(ru.translate_n("{count} map", 5), Some("other"));
    }

    #[test]
    fn a_file_of_the_folder_lies_over_the_built_in_catalog_and_a_new_code_is_a_new_language() {
        let folder = tempfile::tempdir().expect("a scratch folder");
        let over = "msgid \"Open\"\nmsgstr \"Ouvrez\"\n";
        std::fs::write(folder.path().join("fr.po"), over).unwrap();
        std::fs::write(
            folder.path().join("xx.po"),
            "msgid \"\"\nmsgstr \"Language-Name: Xish\\n\"\n\nmsgid \"Open\"\nmsgstr \"Ox\"\n",
        )
        .unwrap();
        std::fs::write(folder.path().join("broken.po"), "msgid \"a\"\n").unwrap();
        std::fs::write(folder.path().join("notes.txt"), "not a catalog").unwrap();
        let all = load(&[("fr", FR)], Some(folder.path()));
        let codes: Vec<&str> = all.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["en", "fr", "xx"], "English first, then by code");
        let fr = &all[1];
        assert_eq!(
            fr.translate("Open"),
            Some("Ouvrez"),
            "the folder's word wins"
        );
        assert!(
            fr.translate_n("{count} file", 2).is_some(),
            "what the file leaves alone stays"
        );
        assert_eq!(
            (all[2].name.as_str(), all[2].translate("Open")),
            ("Xish", Some("Ox"))
        );
    }
}
