//! The rules a GTA VI texture's name follows from a name its drawable states. A texture key is the
//! 64-bit hash of the lower-cased name, so a rule's candidate names a key only where it hashes to
//! one; nothing here is a list of names.

use std::collections::{HashMap, HashSet};

use super::hash::name_hash64;

/// The map kinds a texture's name ends in, after `_`: albedo, albedo with alpha, normal, the
/// three data maps, emissive, tint, palette, dirt, eyes, mask and flow.
const MAP_KINDS: [&str; 13] = [
    "ab", "abal", "nm", "ma", "mb", "mc", "em", "tn", "pal", "dirt", "eyes", "mask", "flow",
];
/// The level-of-detail tokens a stem can end in, the first none.
const LODS: [&str; 9] = [
    "",
    "_lod",
    "_slod",
    "_slod1",
    "_slod2",
    "_slod3",
    "_slod4",
    "_batchlod",
    "_batchslod",
];
/// A ped component texture's map kinds.
const PED_MAPS: [&str; 7] = ["ab", "tn", "ma", "nm", "mb", "em", "mc"];
/// A ped component's variants `c0` to `c3` and texture sets `000` to `039`.
const PED_VARIANTS: u32 = 4;
const PED_SETS: u32 = 40;
/// A numbered variant runs from 0 to 99; the first 30 also take a letter `a` to `h`.
const NUMBERS: u32 = 100;
const LETTERED: u32 = 30;
/// The longest stem the rules make names from: a name the file states is bounded only by its
/// section, and every candidate copies the stem.
const MAX_STEM: usize = 128;
/// The underscores the token rules split a stem at, the first ones: each split makes over a
/// thousand names.
const MAX_SPLITS: usize = 8;

/// The rule that gives a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// The stem itself.
    Stem,
    /// The stem, `_` and a map kind.
    MapKind,
    /// The stem with its level-of-detail token swapped for another or none, then a map kind.
    LodSwap,
    /// The stem with its last number, and a letter after it, changed, then a map kind.
    Numbered,
    /// The stem's first or last tokens, then a map kind.
    Closure,
    /// The stem's first tokens, `_c<N>_<NNN>_` and a ped map kind.
    PedComponent,
}

impl Rule {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stem => "the drawable's stem",
            Self::MapKind => "stem and map kind",
            Self::LodSwap => "level of detail swapped",
            Self::Numbered => "numbered variant",
            Self::Closure => "first or last tokens",
            Self::PedComponent => "ped component",
        }
    }
}

/// A drawable's name without its `.#dr` or `.#ft` extension, lower-cased.
pub fn stem(name: &str) -> String {
    let stem = (name.rsplit_once('.'))
        .filter(|(_, extension)| extension.starts_with('#'))
        .map_or(name, |(stem, _)| stem);
    stem.to_ascii_lowercase()
}

/// Each name the rules give from `stem`, with its rule, handed to `each` as it is made.
pub fn candidates(stem: &str, each: &mut impl FnMut(Rule, &str)) {
    if stem.len() > MAX_STEM {
        return;
    }
    let mut name = String::new();
    each(Rule::Stem, stem);
    with_kinds(Rule::MapKind, stem, &MAP_KINDS, &mut name, each);

    let base = (LODS[1..].iter())
        .find_map(|lod| stem.strip_suffix(lod))
        .unwrap_or(stem);
    for lod in LODS {
        let swapped = format!("{base}{lod}");
        if swapped != stem {
            with_kinds(Rule::LodSwap, &swapped, &MAP_KINDS, &mut name, each);
        }
    }

    if let Some(end) = stem.rfind(|c: char| c.is_ascii_digit()) {
        let start = stem[..end]
            .rfind(|c: char| !c.is_ascii_digit())
            .map_or(0, |i| i + 1);
        let width = end + 1 - start;
        // A letter that ends the number's token belongs to the number.
        let rest = &stem[end + 1..];
        let rest = match rest.as_bytes() {
            [c, ..]
                if c.is_ascii_lowercase()
                    && matches!(rest.as_bytes().get(1), None | Some(b'_')) =>
            {
                &rest[1..]
            }
            _ => rest,
        };
        let count = 10u32
            .checked_pow(width as u32)
            .map_or(NUMBERS, |p| p.min(NUMBERS));
        let head = &stem[..start];
        for n in 0..count {
            let plain = format!("{head}{n:0width$}{rest}");
            with_kinds(Rule::Numbered, &plain, &MAP_KINDS, &mut name, each);
            let letters = if n < LETTERED { "abcdefgh" } else { "" };
            for letter in letters.chars() {
                let variant = format!("{head}{n:0width$}{letter}{rest}");
                with_kinds(Rule::Numbered, &variant, &MAP_KINDS, &mut name, each);
            }
        }
    }

    for (at, _) in stem.match_indices('_').take(MAX_SPLITS) {
        for part in [&stem[..at], &stem[at + 1..]] {
            if !part.is_empty() {
                with_kinds(Rule::Closure, part, &MAP_KINDS, &mut name, each);
            }
        }
        for c in 0..PED_VARIANTS {
            for set in 0..PED_SETS {
                let component = format!("{}_c{c}_{set:03}", &stem[..at]);
                with_kinds(Rule::PedComponent, &component, &PED_MAPS, &mut name, each);
            }
        }
    }
}

/// `base`, `_` and each of `kinds`, built in `name`.
fn with_kinds(
    rule: Rule,
    base: &str,
    kinds: &[&str],
    name: &mut String,
    each: &mut impl FnMut(Rule, &str),
) {
    for kind in kinds {
        name.clear();
        name.push_str(base);
        name.push('_');
        name.push_str(kind);
        each(rule, name);
    }
}

/// Each key of `keys` a candidate from `names` hashes to, with the first name and rule that did.
pub fn name_keys<'a>(
    names: impl IntoIterator<Item = &'a str>,
    keys: &HashSet<u64>,
) -> HashMap<u64, (String, Rule)> {
    let mut found = HashMap::new();
    if keys.is_empty() {
        return found;
    }
    for name in names {
        candidates(&stem(name), &mut |rule, candidate| {
            let key = name_hash64(candidate.as_bytes());
            if keys.contains(&key) {
                found
                    .entry(key)
                    .or_insert_with(|| (candidate.to_string(), rule));
            }
        });
    }
    found
}

/// As [`name_keys`], with each key a name `vocabulary` holds hashes to named by it: a name a
/// body spells outranks a rule's candidate.
pub fn name_keys_with<'a>(
    vocabulary: &super::names::Vocabulary,
    names: impl IntoIterator<Item = &'a str>,
    keys: &HashSet<u64>,
) -> HashMap<u64, (String, Rule)> {
    let mut found = name_keys(names, keys);
    for key in keys {
        if let Some(name) = vocabulary.name64(*key) {
            found.insert(*key, (name.to_string(), Rule::Stem));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(drawable: &str, textures: &[&str]) -> HashMap<u64, (String, Rule)> {
        let keys = textures.iter().map(|t| name_hash64(t.as_bytes())).collect();
        name_keys([drawable], &keys)
    }

    fn rule_of(found: &HashMap<u64, (String, Rule)>, texture: &str) -> Option<Rule> {
        let (name, rule) = found.get(&name_hash64(texture.as_bytes()))?;
        assert_eq!(name, texture);
        Some(*rule)
    }

    #[test]
    fn a_stem_is_the_name_without_its_extension_lower_cased() {
        assert_eq!(stem("Zq_Shed_07.#dr"), "zq_shed_07");
        assert_eq!(stem("zq_crane.#ft"), "zq_crane");
        assert_eq!(stem("zq.v2_hut"), "zq.v2_hut");
    }

    #[test]
    fn each_rule_names_a_texture_from_a_made_up_drawable_name() {
        let cases = [
            ("zq_shed_07.#dr", "zq_shed_07", Rule::Stem),
            ("zq_shed_07.#dr", "zq_shed_07_nm", Rule::MapKind),
            ("zq_shed_07.#dr", "zq_shed_07_lod_ma", Rule::LodSwap),
            ("zq_shed_07_lod.#dr", "zq_shed_07_slod2_abal", Rule::LodSwap),
            ("zq_shed_07_lod.#dr", "zq_shed_07_em", Rule::LodSwap),
            ("zq_crate_01a.#dr", "zq_crate_05c_mb", Rule::Numbered),
            ("zq_crate_01a.#dr", "zq_crate_42_mb", Rule::Numbered),
            ("zq_tower_top_part.#dr", "zq_tower_ab", Rule::Closure),
            ("zq_tower_top_part.#dr", "top_part_tn", Rule::Closure),
            (
                "jacketzq_mr1_003_long_full.#dr",
                "jacketzq_mr1_003_long_c1_012_tn",
                Rule::PedComponent,
            ),
            (
                "jacketzq_mr1_003_long_alt_00.#dr",
                "jacketzq_mr1_003_long_c0_000_ab",
                Rule::PedComponent,
            ),
        ];
        for (drawable, texture, rule) in cases {
            let found = named(drawable, &[texture]);
            assert_eq!(
                rule_of(&found, texture),
                Some(rule),
                "{drawable} -> {texture}"
            );
        }
    }

    #[test]
    fn a_name_that_fits_no_rule_stays_a_key() {
        let outside = [
            "zq_shed_07_unknownkind",
            "zq_barn_07_ab",
            "zq_crate_45c_mb",
            "zq_crate_100_mb",
            "jacketzq_mr1_003_long_c4_000_tn",
            "jacketzq_mr1_003_long_c0_040_tn",
        ];
        for drawable in [
            "zq_shed_07.#dr",
            "zq_crate_01a.#dr",
            "jacketzq_mr1_003_long_full.#dr",
        ] {
            assert!(named(drawable, &outside).is_empty(), "{drawable}");
        }
    }

    #[test]
    fn a_long_stem_or_one_of_many_tokens_makes_a_bounded_count_of_names() {
        let count = |stem: &str, rules: &[Rule]| {
            let mut n = 0usize;
            candidates(stem, &mut |rule, _| n += usize::from(rules.contains(&rule)));
            n
        };
        let every = [
            Rule::Stem,
            Rule::MapKind,
            Rule::LodSwap,
            Rule::Numbered,
            Rule::Closure,
            Rule::PedComponent,
        ];
        let tokens: Vec<String> = (0..30).map(|k| format!("t{k}")).collect();
        let stem = tokens.join("_");
        assert!(stem.len() <= MAX_STEM);
        let per_split = 2 * MAP_KINDS.len() + (PED_VARIANTS * PED_SETS) as usize * PED_MAPS.len();
        assert_eq!(
            count(&stem, &[Rule::Closure, Rule::PedComponent]),
            MAX_SPLITS * per_split
        );

        assert_eq!(count(&format!("a{}", "_".repeat(65_536)), &every), 0);
        assert_eq!(count(&"q".repeat(MAX_STEM + 1), &every), 0);
        assert_eq!(count(&"q".repeat(MAX_STEM), &[Rule::Stem]), 1);
    }

    #[test]
    fn only_a_held_key_is_named_and_the_first_rule_keeps_it() {
        let held = name_hash64(b"zq_shed_07_ab");
        let found = name_keys(["zq_shed_07.#dr", "zq_shed.#dr"], &HashSet::from([held, 7]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[&held], ("zq_shed_07_ab".to_string(), Rule::MapKind));
        assert!(name_keys(["zq_shed_07"], &HashSet::new()).is_empty());
    }

    #[test]
    fn a_name_the_vocabulary_holds_names_a_key_no_rule_reaches() {
        let mut vocabulary = crate::gta6::names::Vocabulary::default();
        vocabulary.add("zq_odd_texture_name", None);
        let (held, ruled) = (
            name_hash64(b"zq_odd_texture_name"),
            name_hash64(b"zq_shed_07_nm"),
        );
        let keys = HashSet::from([held, ruled, 7]);
        let found = name_keys_with(&vocabulary, ["zq_shed_07.#dr"], &keys);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[&held],
            ("zq_odd_texture_name".to_string(), Rule::Stem)
        );
        assert_eq!(found[&ruled], ("zq_shed_07_nm".to_string(), Rule::MapKind));
        assert_eq!(name_keys(["zq_shed_07.#dr"], &keys).len(), 1);
    }
}
