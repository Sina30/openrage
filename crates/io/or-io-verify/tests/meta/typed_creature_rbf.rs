//! Creature metadata in RBF opened as the typed root: tags that carry no types read into the
//! typed members, and every edit reaches the tags as that edit and nothing else.
//!
//! ⚠ The oracle is the tag tree a reader gives and the RBF writer, which are not the adapter: a
//! typed record is checked against the tags it was read from, and a record built from nothing
//! against the bytes of the document it copies.

use crate::corpus::Corpus;

use or_io::decl::File;
use or_io::meta::{
    BinaryFormat, CCreatureMetaData, CPedCompExpressionData, CPedPropExpressionData,
    CShaderVariableComponent, MetaDocument, MetaInstance, Value,
};

struct Staged {
    name: String,
    bytes: Vec<u8>,
    tags: MetaDocument,
}

fn staged(what: &str) -> Option<Vec<Staged>> {
    let library = crate::support::schema();
    let found: Vec<_> = Corpus::of_format(BinaryFormat::Rbf)
        .into_iter()
        .filter_map(|f| {
            let tags = MetaDocument::parse(library, f.bytes.clone()).ok()?;
            (tags.instance.type_name == "CCreatureMetaData").then_some(Staged {
                name: f.name,
                bytes: f.bytes,
                tags,
            })
        })
        .collect();
    if found.is_empty() {
        or_test_support::skip_corpus(what, "no creature metadata is staged as RBF");
        return None;
    }
    Some(found)
}

fn tag<'a>(inst: &'a MetaInstance, name: &str) -> Option<&'a MetaInstance> {
    inst.fields.iter().find_map(|(n, v)| match v {
        Value::Struct(t) if n == name => Some(&**t),
        _ => None,
    })
}

fn items<'a>(inst: &'a MetaInstance, name: &str) -> Vec<&'a MetaInstance> {
    tag(inst, name)
        .map(|t| {
            t.fields
                .iter()
                .filter_map(|(n, v)| match v {
                    Value::Struct(i) if n == "Item" => Some(&**i),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn uint(inst: &MetaInstance, name: &str) -> Option<u64> {
    inst.fields.iter().find_map(|(n, v)| match v {
        Value::UInt(x) if n == name => Some(*x),
        _ => None,
    })
}

fn content(inst: &MetaInstance, name: &str) -> Vec<u64> {
    match tag(inst, name).and_then(|t| t.fields.iter().find(|(n, _)| n == "Content")) {
        Some((_, Value::ByteArray(b))) => b.iter().map(|b| u64::from(*b)).collect(),
        Some((_, Value::Array(w))) => w
            .iter()
            .filter_map(|v| match v {
                Value::UInt(x) => Some(*x),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn report(what: &str, total: usize, failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} of {total} {what} failed:\n{}",
        failures.len(),
        failures[..failures.len().min(5)].join("\n")
    );
}

#[test]
fn every_staged_rbf_creature_opens_typed_and_saves_byte_for_byte() {
    const WHAT: &str = "every_staged_rbf_creature_opens_typed_and_saves_byte_for_byte";
    let Some(found) = staged(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    for s in &found {
        let creature = match CCreatureMetaData::from_bytes(&s.bytes) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{}: does not open typed: {e}", s.name));
                continue;
            }
        };
        let root = &s.tags.instance;
        let counts = [
            (
                "shaderVariableComponents",
                creature.shader_variable_components.len(),
            ),
            ("pedPropExpressions", creature.ped_prop_expressions.len()),
            ("pedCompExpressions", creature.ped_comp_expressions.len()),
        ];
        for (name, typed) in counts {
            let tags = items(root, name).len();
            if tags != typed {
                failures.push(format!(
                    "{}: {name} holds {tags} tags and {typed} records",
                    s.name
                ));
            }
        }
        for (i, (record, item)) in creature
            .ped_comp_expressions
            .iter()
            .zip(items(root, "pedCompExpressions"))
            .enumerate()
        {
            let same = u64::from(record.ped_comp_id) == uint(item, "pedCompID").unwrap_or(u64::MAX)
                && u64::from(record.ped_comp_expression_index)
                    == uint(item, "pedCompExpressionIndex").unwrap_or(u64::MAX)
                && record
                    .tracks
                    .iter()
                    .map(|b| u64::from(*b))
                    .collect::<Vec<_>>()
                    == content(item, "tracks")
                && record.ids.iter().map(|w| u64::from(*w)).collect::<Vec<_>>()
                    == content(item, "ids")
                && record
                    .types
                    .iter()
                    .map(|b| u64::from(*b))
                    .collect::<Vec<_>>()
                    == content(item, "types")
                && record
                    .components
                    .iter()
                    .map(|b| u64::from(*b))
                    .collect::<Vec<_>>()
                    == content(item, "components");
            if !same {
                failures.push(format!(
                    "{}: pedCompExpressions[{i}] is not what its tags state",
                    s.name
                ));
                break;
            }
        }
        match creature.to_bytes() {
            Ok(saved) if saved == s.bytes => {}
            Ok(_) => failures.push(format!("{}: an untouched save changed bytes", s.name)),
            Err(e) => failures.push(format!("{}: does not save: {e}", s.name)),
        }
    }
    report("RBF creature documents", found.len(), &failures);
}

#[test]
fn a_grown_rbf_creature_array_reaches_the_tags_as_that_growth_and_nothing_else() {
    const WHAT: &str =
        "a_grown_rbf_creature_array_reaches_the_tags_as_that_growth_and_nothing_else";
    let Some(found) = staged(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let mut grown_any = 0usize;
    for s in &found {
        let Ok(creature) = CCreatureMetaData::from_bytes(&s.bytes) else {
            continue;
        };
        let (Some(last), true) = (
            creature.ped_comp_expressions.last().cloned(),
            tag(&s.tags.instance, "pedCompExpressions").is_some(),
        ) else {
            continue;
        };
        grown_any += 1;
        let mut grown = creature.clone();
        let mut more = last;
        more.ped_comp_var_index += 1;
        more.ids.push(7);
        more.tracks.push(9);
        more.types.push(2);
        more.components.push(1);
        grown.ped_comp_expressions.push(more);

        // The oracle: the tags with one more `Item`, edited the same way, written by the RBF writer.
        let mut expected = s.tags.clone();
        let wrapper = expected
            .instance
            .fields
            .iter_mut()
            .find_map(|(n, v)| match v {
                Value::Struct(t) if n == "pedCompExpressions" => Some(t),
                _ => None,
            })
            .expect("the tag is there");
        let mut item = match wrapper.fields.iter().rev().find(|(n, _)| n == "Item") {
            Some((_, Value::Struct(i))) => (**i).clone(),
            _ => continue,
        };
        for (n, v) in &mut item.fields {
            match (n.as_str(), v) {
                ("pedCompVarIndex", Value::UInt(x)) => *x += 1,
                ("tracks" | "types" | "components" | "ids", Value::Struct(t)) => {
                    let name = n.clone();
                    for (cn, cv) in &mut t.fields {
                        if cn != "Content" {
                            continue;
                        }
                        match (cv, name.as_str()) {
                            (Value::ByteArray(b), "tracks") => b.push(9),
                            (Value::ByteArray(b), "types") => b.push(2),
                            (Value::ByteArray(b), "components") => b.push(1),
                            (Value::Array(w), "ids") => w.push(Value::UInt(7)),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        wrapper
            .fields
            .push(("Item".into(), Value::Struct(Box::new(item))));
        let (Ok(saved), Ok(want)) = (grown.to_bytes(), expected.to_bytes()) else {
            failures.push(format!(
                "{}: the grown record or the oracle does not save",
                s.name
            ));
            continue;
        };
        if saved != want {
            failures.push(format!("{}: the grown tags are not the oracle's", s.name));
        } else if CCreatureMetaData::from_bytes(&saved).map(|c| c == grown) != Ok(true) {
            failures.push(format!(
                "{}: the grown file reopens as something else",
                s.name
            ));
        }
    }
    assert!(
        grown_any > 0,
        "no staged creature document holds pedCompExpressions"
    );
    report("grown RBF creature documents", grown_any, &failures);
}

/// What the tags of a shipped document state that a record has no member for: a tag present and
/// empty, and an order of tags other than the one a record adds them in.
fn what_a_record_cannot_state(root: &MetaInstance) -> Option<&'static str> {
    let names: Vec<&str> = root.fields.iter().map(|(n, _)| n.as_str()).collect();
    let canonical = [
        "shaderVariableComponents",
        "pedCompExpressions",
        "pedPropExpressions",
    ];
    let position = |n: &&str| canonical.iter().position(|c| c == n);
    if names.iter().any(|n| items(root, n).is_empty()) {
        return Some("a tag is present and empty");
    }
    if !names.iter().filter_map(position).is_sorted() {
        return Some("its tags are in another order");
    }
    None
}

/// A record built from nothing that holds what `source` holds.
fn rebuilt(source: &CCreatureMetaData) -> CCreatureMetaData {
    let mut built = CCreatureMetaData::default();
    for s in &source.shader_variable_components {
        let mut r = CShaderVariableComponent::default();
        r.pedcomp_id = s.pedcomp_id;
        r.mask_id = s.mask_id;
        r.shader_variable_hash_string = s.shader_variable_hash_string.clone();
        r.tracks = s.tracks.clone();
        r.ids = s.ids.clone();
        r.components = s.components.clone();
        built.shader_variable_components.push(r);
    }
    for p in &source.ped_prop_expressions {
        let mut r = CPedPropExpressionData::default();
        r.ped_prop_id = p.ped_prop_id;
        r.ped_prop_var_index = p.ped_prop_var_index;
        r.ped_prop_expression_index = p.ped_prop_expression_index;
        r.tracks = p.tracks.clone();
        r.ids = p.ids.clone();
        r.types = p.types.clone();
        r.components = p.components.clone();
        built.ped_prop_expressions.push(r);
    }
    for c in &source.ped_comp_expressions {
        let mut r = CPedCompExpressionData::default();
        r.ped_comp_id = c.ped_comp_id;
        r.ped_comp_var_index = c.ped_comp_var_index;
        r.ped_comp_expression_index = c.ped_comp_expression_index;
        r.tracks = c.tracks.clone();
        r.ids = c.ids.clone();
        r.types = c.types.clone();
        r.components = c.components.clone();
        built.ped_comp_expressions.push(r);
    }
    built
}

#[test]
fn a_creature_built_from_nothing_writes_the_bytes_a_shipped_one_holds() {
    const WHAT: &str = "a_creature_built_from_nothing_writes_the_bytes_a_shipped_one_holds";
    let Some(found) = staged(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let (mut rebuilt_any, mut excluded) = (0usize, Vec::new());
    for s in &found {
        let Ok(source) = CCreatureMetaData::from_bytes(&s.bytes) else {
            continue;
        };
        if let Some(why) = what_a_record_cannot_state(&s.tags.instance) {
            excluded.push(format!("{}: {why}", s.name));
            continue;
        }
        rebuilt_any += 1;
        match rebuilt(&source).to_bytes() {
            Ok(saved) if saved == s.bytes => {}
            Ok(saved) => failures.push(format!(
                "{}: {} bytes built, {} shipped",
                s.name,
                saved.len(),
                s.bytes.len()
            )),
            Err(e) => failures.push(format!("{}: does not save: {e}", s.name)),
        }
    }
    eprintln!(
        "{rebuilt_any} rebuilt byte for byte; left out:
{}",
        excluded.join(
            "
"
        )
    );
    assert!(
        rebuilt_any > 0,
        "no staged document states only what a record holds"
    );
    report(
        "RBF creature documents rebuilt from nothing",
        rebuilt_any,
        &failures,
    );
}

#[test]
fn an_empty_creature_built_here_is_an_rbf_root_that_reopens_empty() {
    let saved = CCreatureMetaData::default().to_bytes().expect("it saves");
    assert_eq!(&saved[..4], b"RBF0");
    let back = CCreatureMetaData::from_bytes(&saved).expect("it reopens");
    assert_eq!(back, CCreatureMetaData::default());
    assert_eq!(back.to_bytes().expect("it saves again"), saved);
}
