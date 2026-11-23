//! The merge policy: how what several contents contribute becomes one plan.
//!
//! Each function reads contributions and appends what is wrong to the plan's problems, so a
//! plan says everything that is wrong with it at once.

use std::collections::{BTreeMap, BTreeSet};

use crate::container::{Output, TOTALS};
use crate::contribution::{Contribution, Item, ManifestEntry};
use crate::plan::{Budget, NameCheck};

/// The manifest keys that describe the whole resource, with the value the build gives one that no
/// content states. Every content that states one must state the same value; any other key adds up.
const AGREE: [(&str, &str); 2] = [("fx_version", "cerulean"), ("game", "gta5")];

/// The words of `keys` that `declared` states as text, in the order of `keys`, and a problem for each that is not text.
pub(crate) fn stated<'a>(
    declared: &Output,
    keys: &[&'a str],
    problems: &mut Vec<String>,
) -> Vec<(&'a str, String)> {
    let mut found = Vec::new();
    for &key in keys {
        match declared.extra.get(key) {
            None => {}
            Some(serde_json::Value::String(text)) if !text.is_empty() => {
                found.push((key, text.clone()));
            }
            Some(serde_json::Value::String(_)) => {}
            Some(v) => problems.push(format!(
                "build `{}` has `{key}` {v}, which is not text",
                declared.id
            )),
        }
    }
    found
}

/// The text of the manifest: the keys that must agree, the words the build states, the files, then every line a content added.
///
/// `listed` is every file the manifest names, in order; `entries` are the lines the contents add,
/// each with the id of the content that adds it. The added lines are sorted and repeated ones are
/// written once, so the text does not depend on which content came first.
pub(crate) fn manifest(
    listed: &[&str],
    stated: &[(&str, String)],
    entries: &[(&str, &ManifestEntry)],
    problems: &mut Vec<String>,
) -> String {
    let lines = well_formed(entries, problems);
    let mut text = String::new();
    for (key, default) in AGREE {
        let mut stated: Option<(&str, &str)> = None;
        for &(module, line) in lines.iter().filter(|(_, line)| line.key == key) {
            let value = line.values[0].as_str();
            match stated {
                Some((first, held)) if held != value => problems.push(format!(
                    "content `{first}` states `{key} '{held}'` and content `{module}` states `{key} '{value}'`, and the resource has one"
                )),
                Some(_) => {}
                None => stated = Some((module, value)),
            }
        }
        let value = stated.map_or(default, |(_, value)| value);
        text.push_str(&format!("{key} {}\n", lua_string(value)));
    }
    for (key, value) in stated {
        text.push_str(&format!("{key} {}\n", lua_string(value)));
    }
    if !listed.is_empty() {
        text.push_str("\nfiles {\n");
        for path in listed {
            text.push_str(&format!("  {},\n", lua_string(path)));
        }
        text.push_str("}\n");
    }
    let added: BTreeSet<(&str, &[String])> = lines
        .iter()
        .filter(|(_, line)| !AGREE.iter().any(|(key, _)| *key == line.key))
        .map(|(_, line)| (line.key.as_str(), line.values.as_slice()))
        .collect();
    if !added.is_empty() {
        text.push('\n');
        for (key, values) in added {
            let values: Vec<String> = values.iter().map(|v| lua_string(v)).collect();
            text.push_str(&format!("{key} {}\n", values.join(" ")));
        }
    }
    text
}

/// The lines that can be written as `key 'value'`, and a problem for each that cannot.
fn well_formed<'a>(
    entries: &[(&'a str, &'a ManifestEntry)],
    problems: &mut Vec<String>,
) -> Vec<(&'a str, &'a ManifestEntry)> {
    let mut lines = Vec::new();
    for &(module, line) in entries {
        let key = &line.key;
        let own = AGREE.iter().any(|(own, _)| own == key);
        if !is_key(key) {
            problems.push(format!(
                "content `{module}` adds a manifest line named {key:?}, which is not a name"
            ));
        } else if line.values.is_empty() {
            problems.push(format!(
                "content `{module}` adds the manifest line `{key}` with no value"
            ));
        } else if own && line.values.len() != 1 {
            problems.push(format!(
                "`{key}` takes one value and content `{module}` gives {}",
                line.values.len()
            ));
        } else {
            lines.push((module, line));
        }
    }
    lines
}

fn is_key(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn lua_string(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            c if c.is_control() => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// A problem for each name of one kind that the game resolves once and that two claims hold, and a count of the names the build holds.
///
/// `contributions` is every content of the project, because the game sees one namespace however
/// the builds divide it; a clash is the problem of a build only when it takes a content that
/// holds one of the claims. Names are compared without regard to case, as the game hashes them.
pub(crate) fn claims(
    contributions: &[&Contribution],
    taken: &BTreeSet<&str>,
    problems: &mut Vec<String>,
) -> NameCheck {
    let mut held: BTreeMap<(&str, String), Vec<(&str, &str)>> = BTreeMap::new();
    for contribution in contributions {
        for claim in &contribution.claims {
            held.entry((claim.kind.as_str(), claim.name.to_lowercase()))
                .or_default()
                .push((contribution.module.as_str(), claim.name.as_str()));
        }
    }
    let mut checked = NameCheck::default();
    for ((kind, _), claimants) in held {
        if !claimants.iter().any(|(module, _)| taken.contains(module)) {
            continue;
        }
        *checked.held.entry(kind.to_string()).or_insert(0) += 1;
        if claimants.len() < 2 {
            continue;
        }
        let mut who: Vec<(&str, usize)> = Vec::new();
        for &(module, _) in &claimants {
            match who.iter_mut().find(|entry| entry.0 == module) {
                Some((_, times)) => *times += 1,
                None => who.push((module, 1)),
            }
        }
        let who: Vec<String> = who
            .iter()
            .map(|(module, times)| match times {
                1 => format!("content `{module}`"),
                times => format!("content `{module}` ({times} times)"),
            })
            .collect();
        let clash = format!(
            "the {kind} `{}` is claimed by {}, and the game resolves a name once",
            claimants[0].1,
            who.join(", ")
        );
        checked.clashes.push(clash.clone());
        problems.push(clash);
    }
    checked
}

/// The figures that contents contributed, summed by name, against the ceilings `declared` sets.
pub(crate) fn budgets(
    taken: &[&Contribution],
    declared: &Output,
    problems: &mut Vec<String>,
) -> Vec<Budget> {
    let mut used: BTreeMap<&str, u64> = BTreeMap::new();
    for contribution in taken {
        for figure in &contribution.figures {
            if TOTALS.contains(&figure.name.as_str()) {
                problems.push(format!(
                    "content `{}` contributes a figure named `{}`, which is a total of the plan, and `max_{}` sets its ceiling",
                    contribution.module, figure.name, figure.name
                ));
                continue;
            }
            let sum = used.entry(&figure.name).or_insert(0);
            *sum = sum.saturating_add(figure.amount);
        }
    }
    for name in declared.limits.keys() {
        used.entry(name).or_insert(0);
    }
    let budgets: Vec<Budget> = used
        .into_iter()
        .map(|(name, used)| Budget {
            name: name.to_string(),
            used,
            ceiling: declared.limits.get(name).copied(),
        })
        .collect();
    for budget in &budgets {
        if let Some(max) = budget.ceiling.filter(|max| budget.used > *max) {
            problems.push(format!(
                "the build holds {} {}, over the {max} that build `{}` allows",
                budget.used, budget.name, declared.id
            ));
        }
    }
    budgets
}

/// The items of the contents, ordered by id, and a problem for an id that two items hold.
pub(crate) fn items(taken: &[&Contribution], problems: &mut Vec<String>) -> Vec<Item> {
    let mut items: Vec<(&str, &Item)> = taken
        .iter()
        .flat_map(|c| c.items.iter().map(move |item| (c.module.as_str(), item)))
        .collect();
    items.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    for pair in items.windows(2) {
        let [(first, a), (second, b)] = pair else {
            continue;
        };
        if a.id == b.id {
            problems.push(format!(
                "the id `{}` belongs to two items, `{}` of content `{first}` and `{}` of content `{second}`",
                a.id, a.name, b.name
            ));
        }
    }
    items.into_iter().map(|(_, item)| item.clone()).collect()
}
