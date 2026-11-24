//! `meta info`'s lines of a GTA VI metadata document's hash values no table names that the kept
//! vocabulary does, each marked as from the text, which may be a collision.

use std::collections::BTreeSet;

use or_io::gta6::names::Vocabulary;
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::MetaLayout;
use or_io::meta::{MetaInstance, Value};

/// Prints the lines of `instance`, read with `layout`; nothing for a form that is not GTA VI's.
pub(crate) fn print_text_names(layout: &MetaLayout, instance: &MetaInstance) {
    let gta6 = match &layout.container {
        ContainerLayout::Pso(p) => p.is_table_form() || p.is_gta6(),
        ContainerLayout::Rbm(r) => r.seal.is_none(),
        _ => false,
    };
    if !gta6 {
        return;
    }
    // The vocabulary is a whole harvested file, read once; a census pays it per row, so it is read
    // only when the document holds a hash no table names.
    let mut unnamed = BTreeSet::new();
    unnamed_in(instance, &mut unnamed);
    if unnamed.is_empty() {
        return;
    }
    for line in text_name_lines(instance, super::gta6_names::vocabulary()) {
        println!("{line}");
    }
}

/// A count of the hash values no table names and how many `vocabulary` names, then a line each.
fn text_name_lines(instance: &MetaInstance, vocabulary: &Vocabulary) -> Vec<String> {
    let mut unnamed = BTreeSet::new();
    unnamed_in(instance, &mut unnamed);
    if unnamed.is_empty() {
        return Vec::new();
    }
    let named: Vec<String> = (unnamed.iter())
        .filter_map(|&hash| {
            let name = vocabulary.name32(hash)?;
            Some(format!("    hash_{hash:08X}  {name} (from the text)"))
        })
        .collect();
    let mut lines = vec![format!(
        "  text names   {} of {} hash values no table names are named by the harvested names",
        named.len(),
        unnamed.len()
    )];
    lines.extend(named);
    lines
}

fn unnamed_in(instance: &MetaInstance, into: &mut BTreeSet<u32>) {
    for (_, value) in &instance.fields {
        unnamed_value(value, into);
    }
}

fn unnamed_value(value: &Value, into: &mut BTreeSet<u32>) {
    match value {
        Value::Hash(h) if h.raw.is_empty() && h.hash != 0 => {
            into.insert(h.hash);
        }
        Value::Struct(inner) | Value::Pointer(Some(inner)) => unnamed_in(inner, into),
        Value::Array(items) => items.iter().for_each(|v| unnamed_value(v, into)),
        Value::Map(pairs) => {
            for (key, v) in pairs {
                unnamed_value(key, into);
                unnamed_value(v, into);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use or_io::gta6::hash::name_hash32;
    use or_io::meta::instance::HashedString;

    use super::*;

    #[test]
    fn a_hash_value_no_table_names_is_named_from_the_vocabulary() {
        let (held, other) = (
            name_hash32(b"zq_meta_crate"),
            name_hash32(b"zq_meta_absent"),
        );
        let mut vocabulary = Vocabulary::default();
        vocabulary.add("zq_meta_crate", None);
        let mut inner = MetaInstance::new("Inner");
        inner.fields.push((
            "list".into(),
            Value::Array(vec![Value::Hash(HashedString::from_hash(held))]),
        ));
        let mut root = MetaInstance::new("Root");
        root.fields
            .push(("named".into(), Value::Hash(HashedString::new("zq_named"))));
        root.fields
            .push(("zero".into(), Value::Hash(HashedString::from_hash(0))));
        root.fields
            .push(("other".into(), Value::Hash(HashedString::from_hash(other))));
        root.fields
            .push(("inner".into(), Value::Struct(Box::new(inner))));
        let lines = text_name_lines(&root, &vocabulary);
        assert!(lines[0].contains("1 of 2 hash values"), "{lines:?}");
        assert_eq!(
            lines[1..],
            [format!(
                "    hash_{held:08X}  zq_meta_crate (from the text)"
            )]
        );
        assert!(text_name_lines(&MetaInstance::new("Empty"), &vocabulary).is_empty());
    }
}
