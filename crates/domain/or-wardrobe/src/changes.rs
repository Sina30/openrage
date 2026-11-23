//! What differs between two states of a garment or of a pack, as lines a person reads: the field that changed, what it was and what it is.
//!
//! The fields are the ones `wardrobe.json` states, under the names it gives them, so a line can be looked for in the file and in a
//! diff of it. A bulk edit lists them before it is made, and the review of two versions lists them after.

use serde_json::Value;

use crate::model::{Garment, Wardrobe};

/// The longest a value is spelled in a line before it is cut.
const SHOWN: usize = 60;

/// How a value is spelled in a line: a word as it is, a list with commas, and nothing as `none`.
fn spelled(value: &Value) -> String {
    let text = match value {
        Value::Null => return "none".to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) if items.iter().all(|i| !i.is_object() && !i.is_array()) => {
            let words: Vec<String> = items.iter().map(spelled).collect();
            match words.is_empty() {
                true => "none".to_string(),
                false => words.join(", "),
            }
        }
        other => other.to_string(),
    };
    match text.chars().count() > SHOWN {
        true => format!("{}…", text.chars().take(SHOWN).collect::<String>()),
        false => text,
    }
}

/// The name a list item of the key `key` is called by in a line.
fn item(key: &str, at: usize) -> String {
    match key {
        "variations" => format!("texture {}", at + 1),
        "expressions" => format!("expression {}", at + 1),
        // A saved query is called by its name, which the line carries, and no number says where it stands.
        "queries" => "saved query".to_string(),
        other => format!("{other} {}", at + 1),
    }
}

fn join(prefix: &str, key: &str) -> String {
    match (prefix.is_empty(), key.is_empty()) {
        (_, true) => prefix.to_string(),
        (true, false) => key.to_string(),
        (false, false) => format!("{prefix} {key}"),
    }
}

/// What an object of a list is called in a line: the file it names, a saved query by its name and what it asks, else its fields.
fn named(value: &Value) -> String {
    let text = |k: &str| value.get(k).and_then(Value::as_str);
    if let (Some(name), Some(query)) = (text("name"), text("query")) {
        return format!("{name} ({query})");
    }
    let file = ["texture", "model"]
        .iter()
        .find_map(|k| value.get(k).and_then(Value::as_str));
    match (file, value.as_object().is_some_and(|o| o.is_empty())) {
        (Some(file), _) => file.to_string(),
        (None, true) => "empty texture".to_string(),
        (None, false) => spelled(value),
    }
}

/// The objects of a list of them, a list a version does not state being an empty one.
fn objects(value: &Value) -> Option<&[Value]> {
    match value {
        Value::Null => Some(&[]),
        Value::Array(items) if items.iter().all(Value::is_object) => Some(items),
        _ => None,
    }
}

/// Add a line for every place `a` and `b` differ; `prefix` and `key` say where the two are.
fn walk(prefix: &str, key: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    if let (Some(x), Some(y)) = (objects(a), objects(b)) {
        for at in 0..x.len().max(y.len()) {
            let label = join(prefix, &item(key, at));
            match (x.get(at), y.get(at)) {
                (Some(p), Some(q)) => walk(&label, "", p, q, out),
                (Some(p), None) => out.push(format!("{label} removed: {}", named(p))),
                (None, Some(q)) => out.push(format!("{label} added: {}", named(q))),
                (None, None) => {}
            }
        }
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let here = join(prefix, key);
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let (p, q) = (
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                );
                walk(&here, k, p, q, out);
            }
        }
        _ if a != b => out.push(format!(
            "{}: {} -> {}",
            join(prefix, key),
            spelled(a),
            spelled(b)
        )),
        _ => {}
    }
}

fn value_of<T: serde::Serialize>(thing: &T) -> Value {
    serde_json::to_value(thing).unwrap_or(Value::Null)
}

/// The lines that say how `after` differs from `before`, a garment or a texture of one; none when they state the same.
pub fn lines_between<T: serde::Serialize>(before: &T, after: &T) -> Vec<String> {
    let mut out = Vec::new();
    walk("", "", &value_of(before), &value_of(after), &mut out);
    out
}

/// The lines that say how garment `after` differs from `before`; none when they state the same.
pub fn garment_lines(before: &Garment, after: &Garment) -> Vec<String> {
    lines_between(before, after)
}

/// The lines that say how the settings of `after` differ from those of `before`: everything the pack states but its garments.
pub fn settings_lines(before: &Wardrobe, after: &Wardrobe) -> Vec<String> {
    let strip = |w: &Wardrobe| {
        let mut value = value_of(w);
        if let Some(fields) = value.as_object_mut() {
            fields.remove("garments");
        }
        value
    };
    let mut out = Vec::new();
    walk("", "", &strip(before), &strip(after), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_field_is_one_line_and_an_unchanged_garment_none() {
        let a = Garment::new();
        assert!(garment_lines(&a, &a.clone()).is_empty());
        let mut b = a.clone();
        b.tags = vec!["wool".into(), "winter".into()];
        b.label = Some("Coat".into());
        assert_eq!(
            garment_lines(&a, &b),
            ["label: none -> Coat", "tags: none -> wool, winter"]
        );
    }

    #[test]
    fn a_texture_is_named_by_its_number_and_an_added_one_says_so() {
        let mut a = Garment::new();
        a.variations = vec![Default::default(), Default::default()];
        let mut b = a.clone();
        b.variations[1].tint = Some("whi".into());
        b.variations.push(Default::default());
        assert_eq!(
            garment_lines(&a, &b),
            [
                "texture 2 tint: none -> whi",
                "texture 3 added: empty texture"
            ]
        );
    }

    #[test]
    fn a_saved_query_added_is_one_plain_line_naming_it_and_what_it_asks() {
        let before = Wardrobe::new("demo");
        let mut after = before.clone();
        after.queries.push(crate::query::SavedQuery {
            name: "Heavy tops".into(),
            query: "slot:jbib polys>3000".into(),
        });
        assert_eq!(
            settings_lines(&before, &after),
            ["saved query added: Heavy tops (slot:jbib polys>3000)"]
        );
        assert_eq!(
            settings_lines(&after, &before),
            ["saved query removed: Heavy tops (slot:jbib polys>3000)"]
        );
        assert!(settings_lines(&before, &before.clone()).is_empty());
    }
}
