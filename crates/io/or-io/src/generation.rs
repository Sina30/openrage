//! The asset target a RAGE resource's layout belongs to.
//!
//! Code says [`Generation::Gen8`] / [`Generation::Gen9`] (`G8` / `G9` as a type
//! or field suffix); anything a user reads says "Legacy" / "Enhanced", through
//! [`Generation::edition_label`] rather than `Debug`-formatting the value.
//!
//! ⚠ A generation is not an edition: an Enhanced install ships a *mix* of
//! Gen8- and Gen9-layout resources, so "this file is Gen9" and "this install is
//! Enhanced" are independent facts. `Game`, in `or-title`, names the edition.
//!
//! An open newtype over [`TargetId`], so a title can declare a target of its
//! own; `Gen8` and `Gen9` are associated constants and still work as patterns.
//!
//! # Example
//!
//! Declare a `const` and, if it wants a user-facing name, register a label:
//!
//! ```
//! use or_io::generation::{self, Generation, TargetDescriptor};
//!
//! const RDR2_GEN8: Generation = Generation::from_id("rdr2-gen8");
//!
//! generation::register(&TargetDescriptor {
//!     id: "rdr2-gen8",
//!     label: "Red Dead Redemption 2",
//! });
//!
//! assert_eq!(RDR2_GEN8.slug(), "rdr2-gen8");
//! assert_eq!(RDR2_GEN8.edition_label(), "Red Dead Redemption 2");
//! ```
//!
//! Registration is **optional**, because [`Generation::from_id`] is a
//! `const fn` and so cannot consult a registry: an unregistered target labels
//! itself with its own id, never with "Legacy".

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub type TargetId = &'static str;

/// Which layout a resource is in, taken from its own RSC7 version and never
/// from the install it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Generation(TargetId);

#[allow(non_upper_case_globals)]
impl Generation {
    pub const Gen8: Generation = Generation("gen8");
    pub const Gen9: Generation = Generation("gen9");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetDescriptor {
    pub id: TargetId,
    pub label: &'static str,
}

static BUILTIN: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: "gen8",
        label: "Legacy",
    },
    TargetDescriptor {
        id: "gen9",
        label: "Enhanced",
    },
];

fn registered() -> &'static RwLock<Vec<TargetDescriptor>> {
    static REGISTERED: OnceLock<RwLock<Vec<TargetDescriptor>>> = OnceLock::new();
    REGISTERED.get_or_init(|| RwLock::new(Vec::new()))
}

pub fn register(descriptor: &TargetDescriptor) -> bool {
    if by_id(descriptor.id).is_some() {
        return false;
    }
    let mut list = registered().write().expect("target registry poisoned");
    if list.iter().any(|d| d.id == descriptor.id) {
        return false;
    }
    list.push(*descriptor);
    true
}

pub fn by_id(id: &str) -> Option<TargetDescriptor> {
    BUILTIN.iter().copied().find(|d| d.id == id).or_else(|| {
        registered()
            .read()
            .expect("target registry poisoned")
            .iter()
            .copied()
            .find(|d| d.id == id)
    })
}

pub fn all() -> Vec<Generation> {
    let mut out: Vec<Generation> = BUILTIN.iter().map(|d| Generation(d.id)).collect();
    out.extend(
        registered()
            .read()
            .expect("target registry poisoned")
            .iter()
            .map(|d| Generation(d.id)),
    );
    out
}

impl Generation {
    pub const fn from_id(id: TargetId) -> Self {
        Generation(id)
    }

    pub const fn slug(self) -> TargetId {
        self.0
    }

    pub fn edition_label(self) -> &'static str {
        by_id(self.0).map_or(self.0, |d| d.label)
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "gen8" | "g8" | "8" | "legacy" => return Some(Self::Gen8),
            "gen9" | "g9" | "9" | "enhanced" => return Some(Self::Gen9),
            _ => {}
        }
        all()
            .into_iter()
            .find(|t| t.slug().eq_ignore_ascii_case(s) || t.edition_label().eq_ignore_ascii_case(s))
    }

    pub fn is_gen9(self) -> bool {
        self == Self::Gen9
    }
}

impl Default for Generation {
    fn default() -> Self {
        Self::Gen8
    }
}

/// The edition a person reads, Legacy or Enhanced; the slug a file states is [`Generation::slug`].
impl std::fmt::Display for Generation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.edition_label())
    }
}

impl Serialize for Generation {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for Generation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Generation::from_slug(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown asset target `{raw}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtins_keep_their_names_and_their_labels() {
        assert_eq!(Generation::Gen8.slug(), "gen8");
        assert_eq!(Generation::Gen9.slug(), "gen9");
        assert_eq!(Generation::Gen8.edition_label(), "Legacy");
        assert_eq!(Generation::Gen9.edition_label(), "Enhanced");
        assert_eq!(Generation::default(), Generation::Gen8);
        assert!(Generation::Gen9.is_gen9());
        assert!(!Generation::Gen8.is_gen9());
    }

    #[test]
    fn a_generation_is_printed_as_its_edition_and_its_slug_is_asked_for_by_name() {
        assert_eq!(Generation::Gen8.to_string(), "Legacy");
        assert_eq!(format!("{}", Generation::Gen9), "Enhanced");
        assert_eq!(Generation::Gen9.slug(), "gen9");
    }

    #[test]
    fn slug_roundtrips_and_accepts_edition_spellings() {
        for g in all() {
            assert_eq!(Generation::from_slug(g.slug()), Some(g));
        }
        assert_eq!(Generation::from_slug("Legacy"), Some(Generation::Gen8));
        assert_eq!(Generation::from_slug("ENHANCED"), Some(Generation::Gen9));
        assert_eq!(Generation::from_slug("G9"), Some(Generation::Gen9));
        assert_eq!(Generation::from_slug("nothing-declares-this"), None);
    }

    #[test]
    fn an_unregistered_target_labels_itself_rather_than_borrowing_legacy() {
        const FOREIGN: Generation = Generation::from_id("some-other-title-v1");
        assert_eq!(FOREIGN.slug(), "some-other-title-v1");
        assert_eq!(FOREIGN.edition_label(), "some-other-title-v1");
        assert_ne!(FOREIGN.edition_label(), "Legacy");
        assert_ne!(FOREIGN, Generation::Gen8);
        assert!(!FOREIGN.is_gen9());
    }

    #[test]
    fn a_registered_target_gets_its_own_user_facing_name() {
        const T: Generation = Generation::from_id("test-target-labelled");
        assert!(register(&TargetDescriptor {
            id: "test-target-labelled",
            label: "Testopolis Deluxe",
        }));
        assert_eq!(T.edition_label(), "Testopolis Deluxe");
        assert_eq!(Generation::from_slug("test-target-labelled"), Some(T));
        assert_eq!(Generation::from_slug("Testopolis Deluxe"), Some(T));
        assert!(all().contains(&T));
    }

    #[test]
    fn registration_is_idempotent_and_cannot_rename_a_builtin() {
        assert!(!register(&TargetDescriptor {
            id: "gen8",
            label: "Something Else",
        }));
        assert_eq!(Generation::Gen8.edition_label(), "Legacy");

        assert!(register(&TargetDescriptor {
            id: "test-target-idempotent",
            label: "First",
        }));
        assert!(!register(&TargetDescriptor {
            id: "test-target-idempotent",
            label: "Second",
        }));
        assert_eq!(
            Generation::from_id("test-target-idempotent").edition_label(),
            "First"
        );
    }

    #[test]
    fn serialization_uses_the_slug_and_still_reads_the_old_variant_names() {
        assert_eq!(
            serde_json::to_string(&Generation::Gen9).expect("serialize"),
            "\"gen9\""
        );
        assert_eq!(
            serde_json::from_str::<Generation>("\"Gen8\"").expect("the old spelling"),
            Generation::Gen8
        );
        assert_eq!(
            serde_json::from_str::<Generation>("\"gen9\"").expect("the new spelling"),
            Generation::Gen9
        );
        assert!(
            serde_json::from_str::<Generation>("\"gen17\"").is_err(),
            "an unknown target must not deserialize to Gen8: reading a foreign \
             layout as GTA V's is the failure this type exists to prevent"
        );
    }

    #[test]
    fn the_constants_are_still_patterns() {
        let describe = |g: Generation| match g {
            Generation::Gen8 => "eight",
            Generation::Gen9 => "nine",
            _ => "other",
        };
        assert_eq!(describe(Generation::Gen8), "eight");
        assert_eq!(describe(Generation::Gen9), "nine");
        assert_eq!(describe(Generation::from_id("elsewhere")), "other");
    }
}
