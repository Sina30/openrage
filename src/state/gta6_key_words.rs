//! The 32-bit name hash each texture key states where the app knows no name for it: what the key
//! records of the install's drawables state, gathered as the install's harvest reads them and kept
//! in the configuration folder, and what a name typed for a slot hashes to.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use or_io::container::rpf::{seal_rpf9_scan, unseal_rpf9_scan, Rpf9Signature};
use or_io::gta6::hash::{name_hash32, name_hash64};
use or_io::gta6::headerless::{stated_words, Entry, Resource};

/// The version of what the kept file holds: a change to it takes another.
const VERSION: u64 = 1;
/// The most a kept file inflates to, twelve bytes a key.
const MOST_BYTES: usize = 1 << 28;

// ponytail: one map for the whole app, which no view empties.
static WORDS: Mutex<BTreeMap<u64, u32>> = Mutex::new(BTreeMap::new());

fn words() -> MutexGuard<'static, BTreeMap<u64, u32>> {
    WORDS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The 32-bit name hash a drawable's key record or a typed name stated for texture `key`.
pub(crate) fn word(key: u64) -> Option<u32> {
    words().get(&key).copied()
}

/// Keep the hashes of the name `text`, where it is a name and not a `hash_` key.
pub(crate) fn typed(text: &str) {
    let text = text.trim();
    let hashed = text
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case("hash_"));
    if !text.is_empty() && !hashed {
        words().insert(name_hash64(text.as_bytes()), name_hash32(text.as_bytes()));
    }
}

/// Keep each word the key records of the drawables of `resource` state, read from
/// `virtual_section` under `signature`.
pub(super) fn learn(signature: Rpf9Signature, virtual_section: &[u8], resource: &Resource) {
    let Some(role) = signature.role() else {
        return;
    };
    let codes: Vec<Option<u32>> = match resource {
        Resource::Drawable(_) | Resource::DrawableWithBound(_) => vec![None],
        Resource::Dictionary(dict) => (dict.entries.iter())
            .filter(|(_, e)| matches!(e, Ok(Entry::Drawable(_))))
            .map(|(code, _)| Some(*code))
            .collect(),
        _ => return,
    };
    let found: Vec<(u64, u32)> = (codes.into_iter())
        .flat_map(|code| stated_words(virtual_section, role, code))
        .collect();
    words().extend(found);
}

fn kept_file() -> Option<PathBuf> {
    Some(super::prefs::config_root()?.join("gta6_key_words.kept"))
}

fn encode(words: &BTreeMap<u64, u32>) -> Vec<u8> {
    let mut plain = VERSION.to_le_bytes().to_vec();
    for (key, word) in words {
        plain.extend(key.to_le_bytes());
        plain.extend(word.to_le_bytes());
    }
    plain
}

/// What [`encode`] wrote, sealed; an error for another version and for bytes cut short or altered.
fn decode(sealed: &[u8]) -> io::Result<Vec<(u64, u32)>> {
    let plain = unseal_rpf9_scan(sealed, MOST_BYTES)?;
    let other = || {
        let why = "kept texture key words of another version, cut short or altered";
        io::Error::new(io::ErrorKind::InvalidData, why)
    };
    let body = (plain.strip_prefix(VERSION.to_le_bytes().as_slice())).ok_or_else(other)?;
    let (records, rest) = body.as_chunks::<12>();
    if !rest.is_empty() {
        return Err(other());
    }
    Ok((records.iter())
        .map(|r| {
            let key = u64::from_le_bytes(std::array::from_fn(|k| r[k]));
            (key, u32::from_le_bytes(std::array::from_fn(|k| r[8 + k])))
        })
        .collect())
}

/// Keep every word learned in the configuration folder, with what was kept before.
pub(super) fn keep() {
    let Some(file) = kept_file() else {
        return;
    };
    // The kept words may not have been read yet this session; a key's word is one value.
    read_kept();
    let plain = encode(&words());
    let kept = seal_rpf9_scan(&plain).and_then(|sealed| {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&file, sealed)
    });
    if let Err(error) = kept {
        tracing::warn!(%error, "the texture keys' name hashes were not kept");
    }
}

/// Learn the words kept in the configuration folder.
pub(super) fn read_kept() {
    let Some(file) = kept_file() else {
        return;
    };
    let Ok(sealed) = std::fs::read(&file) else {
        return;
    };
    match decode(&sealed) {
        Ok(found) => words().extend(found),
        Err(error) => tracing::info!("{}: {error}", file.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_name_states_its_word_and_a_hash_key_states_none() {
        let name = "zq_key_words_typed_only";
        let key = name_hash64(name.as_bytes());
        assert_eq!(word(key), None);
        typed(&format!("  {name} "));
        assert_eq!(word(key), Some(name_hash32(name.as_bytes())));
        let hashed = "hash_0123456789ABCDEF";
        typed(hashed);
        assert_eq!(word(name_hash64(hashed.as_bytes())), None);
    }

    #[test]
    fn kept_words_read_back_and_cut_or_other_versions_do_not() {
        let words = BTreeMap::from([(7u64, 9u32), (u64::MAX, 0xDEAD_BEEF)]);
        let plain = encode(&words);
        let sealed = |plain: &[u8]| seal_rpf9_scan(plain).expect("sealed");
        let back = decode(&sealed(&plain)).expect("read");
        assert_eq!(back.into_iter().collect::<BTreeMap<_, _>>(), words);
        assert!(decode(&sealed(&plain[..plain.len() - 1])).is_err());
        let mut other = plain.clone();
        other[0] ^= 1;
        assert!(decode(&sealed(&other)).is_err());
    }
}
