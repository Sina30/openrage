//! The `_hi` and `+hi` asset-pair conventions.
//!
//! RAGE content ships high-detail companions beside their base asset, named by
//! convention rather than by any reference the files carry: `prop_x.yft` has
//! `prop_x_hi.yft`, and a vehicle's `vehicle.ytd` has `vehicle+hi.ytd` holding
//! **the same texture names** at higher resolution. Nothing in either file
//! points at the other.
//!
//! ⚠ **This is a naming convention, so it cannot be verified from one name.**
//! An asset legitimately ending in `_hi` is indistinguishable from a
//! high-detail companion, and no amount of parsing separates them.

/// Whether a name is a base asset or its high-detail companion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Detail {
    Base,
    /// The `_hi` / `+hi` companion.
    High,
}

/// A RAGE asset name split into the part that identifies the asset and the
/// part that says which detail level it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetName {
    /// Lowercased, with any detail marker removed.
    pub base: String,
    pub detail: Detail,
    /// The marker as it was written, so a name can be rebuilt exactly.
    marker: Option<&'static str>,
}

/// The markers, longest first so `+hi` is not mistaken for a bare `hi`.
const MARKERS: [&str; 2] = ["+hi", "_hi"];

impl AssetName {
    /// Split a file stem — **not** a path and not a name with an extension.
    pub fn parse(stem: &str) -> Self {
        let lowered = stem.to_ascii_lowercase();
        for marker in MARKERS {
            if let Some(base) = lowered.strip_suffix(marker) {
                // ⚠ A stem that is *only* the marker has no base to pair
                // with, so it is a name in its own right.
                if !base.is_empty() {
                    return Self {
                        base: base.to_string(),
                        detail: Detail::High,
                        marker: Some(marker),
                    };
                }
            }
        }
        Self {
            base: lowered,
            detail: Detail::Base,
            marker: None,
        }
    }

    pub fn is_high_detail(&self) -> bool {
        self.detail == Detail::High
    }

    /// Whether these two name the same asset at either detail level.
    ///
    /// ⚠ True for two names at the *same* level, including a name and
    /// itself. It answers "do these belong together", not "is one the
    /// other's companion" — [`is_companion_of`](Self::is_companion_of) is
    /// that question.
    pub fn pairs_with(&self, other: &Self) -> bool {
        self.base == other.base
    }

    pub fn is_companion_of(&self, other: &Self) -> bool {
        self.base == other.base && self.detail != other.detail
    }

    /// The name of this asset's companion at the other detail level.
    ///
    /// ⚠ `_hi` for a base name, because it is the convention for the paged
    /// asset formats. A texture dictionary's companion uses `+hi`, which a
    /// caller that knows it is naming a `.ytd` asks for with
    /// [`companion_with`](Self::companion_with).
    pub fn companion(&self) -> String {
        self.companion_with("_hi")
    }

    pub fn companion_with(&self, marker: &str) -> String {
        match self.detail {
            Detail::Base => format!("{}{marker}", self.base),
            Detail::High => self.base.clone(),
        }
    }

    /// The name as parsed, marker included.
    pub fn as_written(&self) -> String {
        match self.marker {
            Some(marker) => format!("{}{marker}", self.base),
            None => self.base.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ The traps are the short names. `+hi` must not be read as a bare
    /// `hi`, a stem that is only a marker has no base, and a name that
    /// merely contains `hi` is not a companion.
    #[test]
    fn a_marker_is_only_a_marker_when_something_precedes_it() {
        for (stem, base, detail) in [
            ("vehicle", "vehicle", Detail::Base),
            ("vehicle+hi", "vehicle", Detail::High),
            ("prop_x_hi", "prop_x", Detail::High),
            ("PROP_X_HI", "prop_x", Detail::High),
            // Nothing before the marker, so it is its own name.
            ("_hi", "_hi", Detail::Base),
            ("+hi", "+hi", Detail::Base),
            // Contains the letters but is not the suffix.
            ("sushi", "sushi", Detail::Base),
            ("hi_vis_jacket", "hi_vis_jacket", Detail::Base),
            ("chihuahua", "chihuahua", Detail::Base),
        ] {
            let parsed = AssetName::parse(stem);
            assert_eq!(parsed.base, base, "{stem}");
            assert_eq!(parsed.detail, detail, "{stem}");
            assert_eq!(parsed.as_written(), stem.to_ascii_lowercase(), "{stem}");
        }
    }

    #[test]
    fn a_base_and_its_companion_pair_in_both_directions() {
        let base = AssetName::parse("vehicle");
        let hi = AssetName::parse("vehicle+hi");

        assert!(base.pairs_with(&hi) && hi.pairs_with(&base));
        assert!(base.is_companion_of(&hi) && hi.is_companion_of(&base));
        assert_eq!(base.companion_with("+hi"), "vehicle+hi");
        assert_eq!(hi.companion_with("+hi"), "vehicle");
        assert_eq!(AssetName::parse("prop_x").companion(), "prop_x_hi");

        // ⚠ A name pairs with itself, and is not its own companion.
        assert!(base.pairs_with(&base));
        assert!(!base.is_companion_of(&base));

        let unrelated = AssetName::parse("other");
        assert!(!base.pairs_with(&unrelated));
    }
}
