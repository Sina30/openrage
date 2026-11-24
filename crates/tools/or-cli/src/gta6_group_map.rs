//! `rpf scan-group-map`: a scanned GTA VI PSO body's first map keyed by hashes given a pair, saved,
//! read again and saved without it, in memory; whether the save is the stored body again.

use or_io::meta::instance::HashedString;
use or_io::meta::{write_meta_binary, ErrorMode, Value};

use super::gta6_inverse::{first_difference, said, scanned, verdict, Outcome, BYTES};
use super::meta::{keeps_a_checksum, read_instance};

const USAGE: &str = "usage: rpf scan-group-map <archive.rpf> <offset> [--limit <bytes>]";

pub(crate) fn cmd_scan_group_map(args: &[String]) -> Result<(), String> {
    let (_, stored) = scanned(args, USAGE)?;
    let (instance, layout, _) = read_instance(&stored, ErrorMode::Lenient)
        .map_err(|e| format!("the metadata reader does not read the body: {e}"))?;
    let layout = layout
        .filter(keeps_a_checksum)
        .ok_or("the body is no GTA VI PSO, whose maps this writer lays out")?;
    let mut grown = instance;
    let Some(pairs) = first_map(&mut grown.fields) else {
        let none = Outcome::Refused("the body holds no map keyed by hashes".to_string());
        return verdict(said("a pair added to a map", none));
    };
    let key = fresh_key(pairs);
    let value = pairs[0].1.clone();
    pairs.push((Value::Hash(HashedString::from_hash(key)), value));
    let what =
        format!("a pair of key {key:#010X} added to the first map keyed by hashes, then removed");
    let outcome = 'check: {
        let written = match write_meta_binary(layout.format, Some(&layout), &grown) {
            Ok(written) => written,
            Err(e) => break 'check Outcome::Refused(e.to_string()),
        };
        let read = match read_instance(&written, ErrorMode::Lenient) {
            Ok(read) => read,
            Err(e) => {
                let why = format!("the save with the pair does not read: {e}");
                break 'check Outcome::Differs(why);
            }
        };
        let (mut again, Some(laid), _) = read else {
            let why = "the save with the pair reads as no binary form".to_string();
            break 'check Outcome::Differs(why);
        };
        let Some(pairs) = first_map(&mut again.fields) else {
            break 'check Outcome::Differs("the save with the pair holds no such map".to_string());
        };
        let held = |k: &Value| matches!(k, Value::Hash(h) if h.hash == key);
        let Some(at) = pairs.iter().position(|(k, _)| held(k)) else {
            break 'check Outcome::Differs("the save with the pair does not hold it".to_string());
        };
        pairs.remove(at);
        match write_meta_binary(laid.format, Some(&laid), &again) {
            Ok(back) if back == stored => Outcome::Same(BYTES),
            Ok(back) => Outcome::Differs(first_difference(&back, &stored)),
            Err(e) => Outcome::Differs(format!("the save without the pair is refused: {e}")),
        }
    };
    verdict(said(&what, outcome))
}

/// The pairs of the first map under `fields`, in field order and depth first, whose first key is
/// a hash.
fn first_map(fields: &mut [(String, Value)]) -> Option<&mut Vec<(Value, Value)>> {
    fields.iter_mut().find_map(|(_, v)| map_in(v))
}

fn map_in(v: &mut Value) -> Option<&mut Vec<(Value, Value)>> {
    match v {
        Value::Map(pairs) if matches!(pairs.first(), Some((Value::Hash(_), _))) => Some(pairs),
        Value::Map(pairs) => pairs.iter_mut().find_map(|(_, v)| map_in(v)),
        Value::Struct(s) => first_map(&mut s.fields),
        Value::Pointer(Some(s)) => first_map(&mut s.fields),
        Value::Array(items) => items.iter_mut().find_map(map_in),
        _ => None,
    }
}

/// A hash no key of `pairs` holds, the first past the first key's.
fn fresh_key(pairs: &[(Value, Value)]) -> u32 {
    let hash = |k: &Value| match k {
        Value::Hash(h) => Some(h.hash),
        _ => None,
    };
    let mut key = pairs
        .first()
        .and_then(|(k, _)| hash(k))
        .unwrap_or(0)
        .wrapping_add(1);
    // At most `pairs.len()` keys are taken, so this ends within that many more steps.
    while pairs.iter().any(|(k, _)| hash(k) == Some(key)) {
        key = key.wrapping_add(1);
    }
    key
}

#[cfg(test)]
mod tests {
    use or_io::meta::MetaInstance;

    use super::*;

    fn hashed(h: u32) -> Value {
        Value::Hash(HashedString::from_hash(h))
    }

    #[test]
    fn the_first_map_keyed_by_hashes_is_found_under_structures_and_arrays() {
        let mut inner = MetaInstance::new("Inner");
        inner.set("names", Value::Map(vec![(Value::Int(1), Value::Int(2))]));
        inner.set("groups", Value::Map(vec![(hashed(5), Value::Int(3))]));
        let mut root = MetaInstance::new("Root");
        root.set("count", Value::UInt(4));
        root.set("items", Value::Array(vec![Value::Struct(Box::new(inner))]));
        let pairs = first_map(&mut root.fields).expect("the map under the array");
        assert_eq!(pairs.len(), 1);
        assert_eq!(fresh_key(pairs), 6);
        pairs.push((hashed(6), Value::Int(0)));
        assert_eq!(fresh_key(pairs), 7);
        assert!(first_map(&mut MetaInstance::new("Empty").fields).is_none());
    }
}
