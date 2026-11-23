//! How an enum variant is spelled outside the file, and how it is read back.
//!
//! A variant **name** with no string in the default index (every name, in a
//! build without a schema snapshot) is `hash_HHHHHHHH`, as
//! [`HashedString::to_xml_text`](crate::meta::instance::HashedString::to_xml_text)
//! spells one, and a **value** the enum table does not list is `0xHHHHHHHH`.
//! ⚠ One spelling for both corrupted 3,176 of 6,072 real fixtures on save:
//! read as a value, `0xAB34CAAD` (the name hash of `AT_FRG`) went back into a
//! field that held `3`, and RBM's `Enum8` narrowed it to `0xAD`. Both legs go
//! through here, so the reader cannot emit a spelling the writer does not
//! accept.
//!
//! ⚠ Resolving a `hash_HHHHHHHH` needs no snapshot: the hash is the one the
//! file's own enum table stores. The snapshot is gitignored, so a round trip
//! that depended on one was green with it and corrupting without.

use crate::meta::binary::hash_index::default_hash_index;

pub fn variant_text(name_hash: u32) -> String {
    match default_hash_index().lookup(name_hash) {
        Some(name) => name.to_string(),
        None => format!("hash_{name_hash:08X}"),
    }
}

pub fn value_text(value: i32) -> String {
    format!("0x{value:08X}")
}

pub fn value_literal(variant: &str) -> Option<i32> {
    let hex = variant
        .strip_prefix("0x")
        .or_else(|| variant.strip_prefix("0X"))?;
    u32::from_str_radix(hex, 16).ok().map(|v| v as i32)
}

pub fn fits_in_bytes(value: i32, bytes: u32) -> bool {
    match bytes {
        1 => (-128..=255).contains(&value),
        2 => (-32_768..=65_535).contains(&value),
        _ => true,
    }
}

pub fn resolve_named<I>(variant: &str, entries: I) -> Option<i32>
where
    I: IntoIterator<Item = (u32, i32)>,
{
    if value_literal(variant).is_some() {
        return None;
    }
    let idx = default_hash_index();
    let target = crate::meta::binary::synth::name_hash(variant);
    entries
        .into_iter()
        .find(|(name_hash, _)| *name_hash == target || idx.lookup(*name_hash) == Some(variant))
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_hash_and_a_value_do_not_share_a_spelling() {
        let name_hash = crate::literal_string_hash(b"AT_FRG");
        assert_eq!(name_hash, 0xAB34_CAAD, "the preimage this was measured on");
        // No snapshot in CI, so the name does not resolve — and that must not
        // make it indistinguishable from the number 0xAB34CAAD.
        if default_hash_index().lookup(name_hash).is_none() {
            assert_eq!(variant_text(name_hash), "hash_AB34CAAD");
        }
        assert_eq!(value_text(0xAB34_CAADu32 as i32), "0xAB34CAAD");
        assert_ne!(variant_text(name_hash), value_text(0xAB34_CAADu32 as i32));
    }

    #[test]
    fn an_unnamed_variant_resolves_through_the_files_own_table() {
        let table = [(0xAB34_CAAD, 3), (0x1234_5678, 4)];
        assert_eq!(resolve_named("hash_AB34CAAD", table), Some(3));
        assert_eq!(resolve_named("hash_12345678", table), Some(4));
    }

    #[test]
    fn the_value_sentinel_never_matches_a_name() {
        let table = [(0x0000_0003, 99)];
        assert_eq!(resolve_named("0x00000003", table), None);
        assert_eq!(value_literal("0x00000003"), Some(3));
        assert_eq!(value_literal("0X0000002A"), Some(42));
        assert_eq!(value_literal("hash_00000003"), None);
        assert_eq!(value_literal("AT_FRG"), None);
    }

    #[test]
    fn a_source_name_resolves_by_its_literal_hash() {
        let table = [(crate::literal_string_hash(b"AT_FRG"), 3)];
        assert_eq!(resolve_named("AT_FRG", table), Some(3));
        assert_eq!(resolve_named("at_frg", table), None);
    }

    #[test]
    fn only_the_exact_placeholder_form_is_a_placeholder() {
        let table = [(crate::literal_string_hash(b"hash_table"), 7)];
        assert_eq!(resolve_named("hash_table", table), Some(7));
    }

    #[test]
    fn an_unknown_name_is_not_invented() {
        let table = [(0xAB34_CAAD, 3)];
        assert_eq!(resolve_named("hash_DEADBEEF", table), None);
        assert_eq!(resolve_named("AT_NOT_A_THING", table), None);
    }

    #[test]
    fn narrowing_admits_both_readings_and_nothing_else() {
        assert!(fits_in_bytes(3, 1));
        assert!(fits_in_bytes(255, 1), "a byte read back unsigned");
        assert!(fits_in_bytes(-1, 1), "a table value read back signed");
        assert!(!fits_in_bytes(256, 1));
        assert!(!fits_in_bytes(0xAB34_CAADu32 as i32, 1));
        assert!(fits_in_bytes(65_535, 2) && !fits_in_bytes(65_536, 2));
        assert!(
            fits_in_bytes(0xAB34_CAADu32 as i32, 4),
            "no narrowing at all"
        );
    }
}
