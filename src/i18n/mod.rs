//! The interface languages: a catalog per language, the English text as the key, and the language in use.
//!
//! `tr!` turns an English text into the language in use where it is drawn. A language is a `.po` file, built in for nine languages and
//! read from the `languages` folder of the configuration for any other; the gates in `gates.rs` hold the catalogs to the code.
//! How to add a text is in `docs/reference/Crate_Reference.md`.

#[macro_use]
mod macros;
mod catalog;
#[cfg(test)]
mod gates;
pub mod po;
#[cfg(test)]
mod startup;
#[cfg(test)]
pub(crate) mod used;

use std::collections::HashSet;
use std::fmt::{Display, Write as _};
use std::path::Path;
#[cfg(not(test))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

pub use catalog::Catalog;

/// The catalogs built into the program, in the order of their codes: the code of each and its file.
const BUILT_IN: &[(&str, &str)] = &[
    ("de", include_str!("languages/de.po")),
    ("es", include_str!("languages/es.po")),
    ("fr", include_str!("languages/fr.po")),
    ("it", include_str!("languages/it.po")),
    ("ja", include_str!("languages/ja.po")),
    ("ko", include_str!("languages/ko.po")),
    ("pt", include_str!("languages/pt.po")),
    ("ru", include_str!("languages/ru.po")),
    ("zh", include_str!("languages/zh.po")),
];

static LANGUAGES: OnceLock<Vec<Catalog>> = OnceLock::new();
#[cfg(not(test))]
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

// A test build keeps the language in use per thread, so tests that run in one process never see each other's; a pin wins over it.
#[cfg(test)]
thread_local! {
    static PINNED: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static ACTIVE_HERE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Reads the languages once, with the `.po` files of `folder` laid over the built-in ones; a call after the first changes nothing.
pub fn init(folder: Option<&Path>) {
    LANGUAGES.get_or_init(|| catalog::load(BUILT_IN, folder));
}

/// Every language there is: English first, then the others by code.
pub fn languages() -> &'static [Catalog] {
    LANGUAGES.get_or_init(|| catalog::load(BUILT_IN, None))
}

fn index_of(code: &str) -> Option<usize> {
    languages()
        .iter()
        .position(|c| c.code.eq_ignore_ascii_case(code))
}

/// The language in use.
pub fn active() -> &'static Catalog {
    let all = languages();
    #[cfg(test)]
    let at = PINNED
        .with(std::cell::Cell::get)
        .unwrap_or_else(|| ACTIVE_HERE.with(std::cell::Cell::get));
    #[cfg(not(test))]
    let at = ACTIVE.load(Ordering::Relaxed);
    &all[at.min(all.len() - 1)]
}

/// Uses the language `code` from the next text drawn; false, and no change, when there is no such language.
pub fn set_active(code: &str) -> bool {
    let Some(at) = index_of(code) else {
        return false;
    };
    #[cfg(test)]
    ACTIVE_HERE.with(|a| a.set(at));
    #[cfg(not(test))]
    ACTIVE.store(at, Ordering::Relaxed);
    true
}

/// The code of the language to use: the person's choice when there is such a language, else the language of the system when there is, else English.
pub fn pick(preference: Option<&str>, system: Option<&str>) -> &'static str {
    [preference, system]
        .into_iter()
        .flatten()
        .find_map(index_of)
        .map_or("en", |at| languages()[at].code.as_str())
}

/// The language a person who has chosen none gets: the system's own when there is a catalog for it, else English.
pub fn system_default() -> &'static Catalog {
    let system = crate::platform::locale::system_language();
    &languages()[index_of(pick(None, system.as_deref())).unwrap_or(0)]
}

/// Uses the language [`pick`] names for `preference` and the system's own.
pub fn apply(preference: Option<&str>) {
    let system = crate::platform::locale::system_language();
    set_active(pick(preference, system.as_deref()));
}

/// Draws in `code` on this thread until the guard is dropped, whatever the language in use is: a test that reads English text, or French, says which.
#[cfg(test)]
pub fn pin(code: &str) -> Pinned {
    let at = index_of(code).unwrap_or_else(|| panic!("no language `{code}`"));
    Pinned(PINNED.with(|p| p.replace(Some(at))))
}

#[cfg(test)]
pub struct Pinned(Option<usize>);

#[cfg(test)]
impl Drop for Pinned {
    fn drop(&mut self) {
        PINNED.with(|p| p.set(self.0));
    }
}

static MISSED: Mutex<Option<HashSet<(String, &'static str)>>> = Mutex::new(None);

/// A text a language has no entry for is shown in English, and the log says so once.
fn missed(language: &Catalog, key: &'static str) {
    if language.is_english() || language.gives(key) {
        return;
    }
    let Ok(mut seen) = MISSED.lock() else {
        return;
    };
    if seen
        .get_or_insert_with(HashSet::new)
        .insert((language.code.clone(), key))
    {
        tracing::warn!(language = %language.code, text = key, "no translation; the English text is shown");
    }
}

/// `key` in the language in use, or `key` itself.
pub fn tr(key: &'static str) -> &'static str {
    let language = active();
    language.translate(key).unwrap_or_else(|| {
        missed(language, key);
        key
    })
}

/// The form of the plural entry `one` that `n` takes in the language in use, or the English form.
pub fn tr_n(one: &'static str, other: &'static str, n: u64) -> &'static str {
    let language = active();
    language.translate_n(one, n).unwrap_or_else(|| {
        missed(language, one);
        if n == 1 {
            one
        } else {
            other
        }
    })
}

/// `template` with each `{name}` replaced by the argument of that name; `{{` and `}}` are the braces themselves, and a name no argument has stays as written.
pub fn render(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        rest = &tail[1..];
        match tail.as_bytes()[0] {
            b'{' if tail.starts_with("{{") => {
                out.push('{');
                rest = &tail[2..];
            }
            b'}' if tail.starts_with("}}") => {
                out.push('}');
                rest = &tail[2..];
            }
            b'{' => match tail.find('}') {
                Some(end) => {
                    match args.iter().find(|(name, _)| *name == &tail[1..end]) {
                        Some((_, value)) => {
                            let _ = write!(out, "{value}");
                        }
                        None => out.push_str(&tail[..=end]),
                    }
                    rest = &tail[end + 1..];
                }
                None => out.push('{'),
            },
            _ => out.push('}'),
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_takes_its_arguments_by_name_and_keeps_its_braces() {
        let n = 3;
        let said = render(
            "{count} of {{name}} in {place}, {unknown}",
            &[("count", &n), ("place", &"the pack")],
        );
        assert_eq!(said, "3 of {name} in the pack, {unknown}");
    }

    #[test]
    fn the_language_a_person_chose_wins_then_the_systems_then_english() {
        assert_eq!(pick(Some("fr"), Some("en")), "fr");
        assert_eq!(
            pick(None, Some("FR")),
            "fr",
            "the system's language, in any case"
        );
        assert_eq!(
            pick(Some("zz"), Some("fr")),
            "fr",
            "a choice no language has is no choice"
        );
        assert_eq!(
            pick(None, Some("nl")),
            "en",
            "a system language with no catalog is English"
        );
        assert_eq!(pick(None, None), "en");
    }
}
