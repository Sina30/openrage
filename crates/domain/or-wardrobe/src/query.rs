//! The query bar: a line of terms over a pack, and the queries a pack keeps by name, which act as smart groups.
//!
//! `tag:wool slot:jbib polys>5000 -lod:missing` selects the garments that carry the tag, sit in the slot, draw more than 5000
//! triangles at their high level of detail and do have a medium and a low one. A word with no key is a name searched for, its
//! letters in order in one of the words [`crate::search::terms`] gives. Terms are ANDed, and a `-` in front of one turns it round.
//! A saved query is its text in `wardrobe.json`, so it follows the pack through Undo, History and Git, and it is read again each
//! time it is asked, so what it selects is what the pack states now.

use std::path::Path;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::alloc::Allocation;
use crate::edit::Error;
use crate::facts::{is_source, stays_inside, Facts};
use crate::follow::{follow, Led};
use crate::measure::{source_drawables, Drawable, Measure};
use crate::model::Wardrobe;
use crate::names::{Ped, Slot};
use crate::rules::{name_of, Finding};

/// The terms a query reads, in the order a sentence about them lists them.
pub const TERMS: &str = "tag, group, ped, slot, polys, lod, is, duplicate-of and problem";

/// What the files say of one garment that a query asks of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// The triangles of its model at the high level of detail, none while the model was not counted.
    pub triangles: Option<u64>,
    /// Its model has a high level of detail and lacks a medium or a low one: the case the rule `lod-missing` names.
    pub lod_missing: bool,
    /// The index of the earlier garment that ships the same model and the same textures.
    pub duplicate_of: Option<usize>,
}

/// The triangles each of a drawable's four levels of detail draws, none for a level it has no model at.
type Levels = [Option<u64>; 4];

/// The triangles of each level of each of `drawables`.
fn levels_of(drawables: &[Drawable]) -> Vec<Levels> {
    (drawables.iter())
        .map(|d| d.lods.map(|l| l.map(|lod| lod.triangles)))
        .collect()
}

/// The counts of every garment of `wardrobe` in its order, from what `facts` says of the files and, for a model that is a drawables source of the pack in `folder`, from the glb it holds.
pub fn counts_of(wardrobe: &Wardrobe, facts: &Facts, folder: &Path) -> Vec<Counts> {
    let mut counts: Vec<Counts> = (wardrobe.garments.iter())
        .map(|g| {
            let Some(model) = g.model.as_deref() else {
                return Counts::default();
            };
            let counted = Measure::of_fact(facts.get(model))
                .filter(|m| m.detail)
                .map(|m| levels_of(&m.drawables));
            let levels = counted.or_else(|| {
                let source = match follow(folder, model) {
                    Led::Own if is_source(model) && stays_inside(model) => {
                        folder.join(model.replace('/', std::path::MAIN_SEPARATOR_STR))
                    }
                    Led::To { at, source: true } => at,
                    _ => return None,
                };
                source_drawables(&source).map(|d| levels_of(&d))
            });
            let Some(levels) = levels else {
                return Counts::default();
            };
            let high: Vec<u64> = levels.iter().filter_map(|d| d[0]).collect();
            Counts {
                triangles: (!high.is_empty()).then(|| high.iter().sum()),
                lod_missing: (levels.iter())
                    .any(|d| d[0].is_some() && (d[1].is_none() || d[2].is_none())),
                duplicate_of: None,
            }
        })
        .collect();
    for (at, earlier) in crate::rules::duplicate_pairs(wardrobe, facts) {
        counts[at].duplicate_of = Some(earlier);
    }
    counts
}

/// How a number is held to the one a term states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compare {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl Compare {
    fn holds(self, have: u64, want: u64) -> bool {
        match self {
            Self::Lt => have < want,
            Self::Le => have <= want,
            Self::Eq => have == want,
            Self::Ge => have >= want,
            Self::Gt => have > want,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    Tag(String),
    Group(String),
    Ped(Ped),
    Slot(Slot),
    Polys(Compare, u64),
    LodMissing,
    Duplicate,
    DuplicateOf(String),
    Problem(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Clause {
    not: bool,
    term: Term,
}

/// A query read from its text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    clauses: Vec<Clause>,
    words: Vec<String>,
    /// The words written with a minus: a garment any of them finds is left out.
    not_words: Vec<String>,
}

/// The pack a query is asked of, with what was read of its files and its rules.
pub struct Subject<'a> {
    pub wardrobe: &'a Wardrobe,
    pub allocation: &'a Allocation,
    pub findings: &'a [Finding],
    /// One for each garment, in the pack's order; a garment without a count is not counted, and no count term selects it.
    pub counts: &'a [Counts],
    /// Words a screen adds to the ones a name is searched for in, for the garment at an index.
    pub more_terms: Option<&'a dyn Fn(usize) -> Vec<String>>,
}

impl<'a> Subject<'a> {
    pub fn new(
        wardrobe: &'a Wardrobe,
        allocation: &'a Allocation,
        findings: &'a [Finding],
        counts: &'a [Counts],
    ) -> Self {
        Self {
            wardrobe,
            allocation,
            findings,
            counts,
            more_terms: None,
        }
    }
}

/// `text` split at the spaces outside double quotes, the quotes taken off.
fn tokens(text: &str) -> Result<Vec<String>, String> {
    let (mut all, mut word, mut quoted, mut started) = (Vec::new(), String::new(), false, false);
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    all.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err("a quote is opened and not closed".into());
    }
    if started {
        all.push(word);
    }
    Ok(all)
}

fn ped_of(word: &str) -> Option<Ped> {
    match word.to_ascii_lowercase().as_str() {
        "male" | "m" => Some(Ped::Male),
        "female" | "f" => Some(Ped::Female),
        other => Ped::from_model(other),
    }
}

/// The term the token `key`, `op` and `value` state.
fn term_of(key: &str, op: &str, value: &str) -> Result<Term, String> {
    let needs_word = |what: &str| match value.trim().is_empty() {
        true => Err(format!("{key}{op} needs {what}")),
        false => Ok(value.trim().to_string()),
    };
    let only_colon = || match op {
        ":" => Ok(()),
        _ => Err(format!("{key} is written {key}:word, not {key}{op}word")),
    };
    match key {
        "tag" => only_colon().and(needs_word("a tag")).map(Term::Tag),
        "group" => only_colon().and(needs_word("a group")).map(Term::Group),
        "ped" => {
            only_colon()?;
            ped_of(value)
                .map(Term::Ped)
                .ok_or_else(|| format!("ped:{value} is no ped: male or female"))
        }
        "slot" => {
            only_colon()?;
            Slot::from_str(&value.to_ascii_lowercase())
                .map(Term::Slot)
                .map_err(|_| format!("slot:{value} is no slot of the game: jbib, lowr, feet, p_head and the rest"))
        }
        "polys" => {
            let how = match op {
                "<" => Compare::Lt,
                "<=" => Compare::Le,
                ">" => Compare::Gt,
                ">=" => Compare::Ge,
                _ => Compare::Eq,
            };
            value
                .trim()
                .replace(['_', ','], "")
                .parse::<u64>()
                .map(|n| Term::Polys(how, n))
                .map_err(|_| {
                    format!("polys{op}{value} needs a number of triangles, as in polys>5000")
                })
        }
        "lod" => match (only_colon(), value.to_ascii_lowercase().as_str()) {
            (Ok(()), "missing") => Ok(Term::LodMissing),
            (Err(why), _) => Err(why),
            _ => Err(format!("lod:{value} is not a term: lod:missing")),
        },
        "is" => match (only_colon(), value.to_ascii_lowercase().as_str()) {
            (Ok(()), "duplicate") => Ok(Term::Duplicate),
            (Ok(()), "problem") => Ok(Term::Problem(None)),
            (Err(why), _) => Err(why),
            _ => Err(format!(
                "is:{value} is not a term: is:duplicate or is:problem"
            )),
        },
        "duplicate-of" => only_colon()
            .and(needs_word("the name of a garment"))
            .map(Term::DuplicateOf),
        "problem" => only_colon()
            .and(needs_word("the code of a rule"))
            .map(|code| Term::Problem(Some(code))),
        other => Err(format!(
            "{other}{op} is not a term of a query; the terms are {TERMS}"
        )),
    }
}

impl Query {
    /// The query `text` states, or the sentence that says which term of it cannot be read.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut query = Self::default();
        for token in tokens(text)? {
            let (not, body) = match token.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest),
                _ => (false, token.as_str()),
            };
            let at = body.find([':', '<', '>', '=']);
            let Some(at) = at else {
                match not {
                    true => query.not_words.push(body.to_string()),
                    false => query.words.push(body.to_string()),
                }
                continue;
            };
            let op_len = match body[at..].starts_with("<=") || body[at..].starts_with(">=") {
                true => 2,
                false => 1,
            };
            let (key, op, value) = (&body[..at], &body[at..at + op_len], &body[at + op_len..]);
            let term = term_of(&key.to_ascii_lowercase(), op, value)?;
            query.clauses.push(Clause { not, term });
        }
        Ok(query)
    }

    /// Whether the query selects everything: it has no term and no word.
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty() && self.words.is_empty() && self.not_words.is_empty()
    }

    /// Whether the query asks for more than part of a name: a term such as `tag:wool` or `polys>5000`, or a word turned round with a minus.
    pub fn has_terms(&self) -> bool {
        !self.clauses.is_empty() || !self.not_words.is_empty()
    }

    /// The words that name no term, which a screen may search for its own way.
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// The words that name no term and are written with a minus, which a screen leaves out the garments of its own way.
    pub fn not_words(&self) -> &[String] {
        &self.not_words
    }

    /// Whether every term but the words holds for the garment at `index`.
    pub fn terms_hold(&self, subject: &Subject, index: usize) -> bool {
        let Some(garment) = subject.wardrobe.garments.get(index) else {
            return false;
        };
        let counts = subject.counts.get(index).copied().unwrap_or_default();
        let earlier = counts
            .duplicate_of
            .and_then(|e| subject.wardrobe.garments.get(e));
        self.clauses.iter().all(|clause| {
            let holds = match &clause.term {
                Term::Tag(tag) => garment.tags.iter().any(|t| same(t, tag)),
                Term::Group(group) => garment.group.as_deref().is_some_and(|g| same(g, group)),
                Term::Ped(ped) => garment.ped == Some(*ped),
                Term::Slot(slot) => garment.slot == Some(*slot),
                Term::Polys(how, n) => counts.triangles.is_some_and(|t| how.holds(t, *n)),
                Term::LodMissing => counts.lod_missing,
                Term::Duplicate => earlier.is_some(),
                Term::DuplicateOf(word) => earlier.is_some_and(|e| {
                    [name_of(e), e.id.clone()]
                        .iter()
                        .any(|name| name.to_lowercase().contains(&word.to_lowercase()))
                }),
                Term::Problem(code) => subject
                    .findings
                    .iter()
                    .filter(|f| f.waived.is_none() && f.garment.as_deref() == Some(&garment.id))
                    .any(|f| {
                        code.as_deref()
                            .is_none_or(|c| f.code.eq_ignore_ascii_case(c))
                    }),
            };
            holds != clause.not
        })
    }

    /// Whether the garment at `index` is selected: its terms hold, each word is found in one of the words it is searched by and no word with a minus is.
    pub fn selects(&self, subject: &Subject, index: usize) -> bool {
        if !self.terms_hold(subject, index) {
            return false;
        }
        if self.words.is_empty() && self.not_words.is_empty() {
            return true;
        }
        let mut terms = crate::search::terms(subject.wardrobe, subject.allocation, index);
        if let Some(more) = subject.more_terms {
            terms.extend(more(index));
        }
        let finds = |word: &String| terms.iter().any(|term| in_order(word, term));
        self.words.iter().all(finds) && !self.not_words.iter().any(finds)
    }

    /// The indexes of the garments selected, in the pack's order.
    pub fn select(&self, subject: &Subject) -> Vec<usize> {
        (0..subject.wardrobe.garments.len())
            .filter(|&i| self.selects(subject, i))
            .collect()
    }

    /// The ids of the garments selected, in the pack's order.
    pub fn ids(&self, subject: &Subject) -> Vec<String> {
        (self.select(subject).into_iter())
            .map(|i| subject.wardrobe.garments[i].id.clone())
            .collect()
    }
}

/// Whether two words are the same word, whatever the case and the space around them.
fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Whether the letters of `word` appear in `term` in order, whatever the case.
fn in_order(word: &str, term: &str) -> bool {
    let mut letters = term.chars().flat_map(char::to_lowercase);
    (word.chars().flat_map(char::to_lowercase)).all(|w| letters.any(|c| c == w))
}

/// A query a pack keeps under a name: its text, read again each time it is asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedQuery {
    pub name: String,
    pub query: String,
}

/// The longest name a saved query has.
pub const NAME_LENGTH: usize = 64;

/// Keep `text` under `name` in the pack, in place of the query that has the name; the name is the same name in any case.
pub fn save(wardrobe: &mut Wardrobe, name: &str, text: &str) -> Result<(), Error> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_LENGTH {
        return Err(Error::Refused(format!(
            "a saved query needs a name of 1 to {NAME_LENGTH} characters"
        )));
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Query::parse(&text).map_err(Error::Refused)?;
    if text.is_empty() {
        return Err(Error::Refused(
            "an empty query selects every garment, so there is nothing to keep".into(),
        ));
    }
    let saved = SavedQuery {
        name: name.to_string(),
        query: text,
    };
    match wardrobe.queries.iter_mut().find(|q| same(&q.name, name)) {
        Some(held) => *held = saved,
        None => wardrobe.queries.push(saved),
    }
    Ok(())
}

/// Let the query called `name` go.
pub fn forget(wardrobe: &mut Wardrobe, name: &str) -> Result<(), Error> {
    let before = wardrobe.queries.len();
    wardrobe.queries.retain(|q| !same(&q.name, name));
    match wardrobe.queries.len() < before {
        true => Ok(()),
        false => Err(Error::Refused(format!("no saved query is called {name}"))),
    }
}

/// The saved query called `name`.
pub fn saved<'a>(wardrobe: &'a Wardrobe, name: &str) -> Option<&'a SavedQuery> {
    wardrobe.queries.iter().find(|q| same(&q.name, name))
}
