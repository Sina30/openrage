//! An archetype authored from a drawable: its names from text, its box seeded from the drawable
//! root's box, and its sphere the box's centre and half diagonal.
//!
//! ⚠ The box is a seed: an archetype's box and its model's differ by more than 1 cm in about one
//! archetype of five. `lodDist`, `guid` and the flags follow no rule from the drawable.

use crate::gta6::hash::{name_hash32, name_hash64};

/// The values of an archetype authored from a drawable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Authored {
    /// `name` and `assetName`: the 32-bit hash of the lower-cased text.
    pub name: u32,
    /// The words at 0x08 and 0x20: Murmur3 x64-128's low half of the lower-cased text.
    pub name_word: u64,
    pub bb_min: [f32; 3],
    pub bb_max: [f32; 3],
    pub bs_centre: [f32; 3],
    pub bs_radius: f32,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthorError {
    #[error("the archetype's name is empty")]
    NoName,
    #[error("the drawable's box is not finite, or its minimum passes its maximum")]
    Box,
}

/// The archetype named `text` of a drawable whose root box is `box_min..box_max`.
pub fn authored(text: &str, box_min: [f32; 3], box_max: [f32; 3]) -> Result<Authored, AuthorError> {
    let text = text.trim().to_ascii_lowercase();
    if text.is_empty() {
        return Err(AuthorError::NoName);
    }
    let finite = box_min.iter().chain(&box_max).all(|v| v.is_finite());
    if !finite || (0..3).any(|a| box_min[a] > box_max[a]) {
        return Err(AuthorError::Box);
    }
    let bs_centre = std::array::from_fn(|a| (box_min[a] + box_max[a]) * 0.5);
    let half: f32 = (0..3)
        .map(|a| ((box_max[a] - box_min[a]) * 0.5).powi(2))
        .sum();
    Ok(Authored {
        name: name_hash32(text.as_bytes()),
        name_word: name_hash64(text.as_bytes()),
        bb_min: box_min,
        bb_max: box_max,
        bs_centre,
        bs_radius: half.sqrt(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_authored_archetype_takes_the_box_its_centre_and_half_diagonal_and_names_from_text() {
        let a = authored("  Prop_Bench_01 ", [-1.0, -2.0, 0.0], [3.0, 2.0, 4.0]).expect("authored");
        assert_eq!(a.name, name_hash32(b"prop_bench_01"));
        assert_eq!(
            a.name,
            name_hash32(b"PROP_BENCH_01"),
            "the 32-bit hash folds case"
        );
        assert_eq!(a.name_word, name_hash64(b"prop_bench_01"));
        assert_eq!((a.bb_min, a.bb_max), ([-1.0, -2.0, 0.0], [3.0, 2.0, 4.0]));
        assert_eq!(a.bs_centre, [1.0, 0.0, 2.0]);
        // Half extents of 2, 2 and 2.
        assert!((a.bs_radius - 12f32.sqrt()).abs() < 1e-6, "{}", a.bs_radius);
    }

    #[test]
    fn an_empty_name_or_a_box_turned_inside_out_or_not_finite_is_refused() {
        let unit = ([0.0; 3], [1.0; 3]);
        assert_eq!(authored(" ", unit.0, unit.1), Err(AuthorError::NoName));
        assert_eq!(
            authored("a", [0.0, 2.0, 0.0], [1.0; 3]),
            Err(AuthorError::Box)
        );
        assert_eq!(
            authored("a", [f32::NAN, 0.0, 0.0], [1.0; 3]),
            Err(AuthorError::Box)
        );
        assert!(
            authored("a", [1.0; 3], [1.0; 3]).is_ok(),
            "a box of no size"
        );
    }
}
