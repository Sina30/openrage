//! `.ybd`: bounds keyed by name hash, each the bound a `.ybn` would hold.

use super::bounds::{self, PhBoundAny};
use super::shared::PgDictionary;
use crate::decl::*;
use crate::Generation;

impl Resource for PgDictionary<PhBoundAny> {
    const EXT: &'static str = "ybd";
    const TARGET: Generation = Generation::Gen8;
}

/// A dictionary the game would misread: its order, or one of its bounds.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("the dictionary's codes are not ascending, so a lookup would find the wrong bound")]
    CodesUnsorted,
    #[error("the dictionary holds {codes} codes and {entries} bounds")]
    Lengths { codes: usize, entries: usize },
    #[error("bound {code:#010x}: {why}")]
    Bound { code: u32, why: bounds::Broken },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// The dictionary's order, and every bound in it.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    let dict = View::<Raw<PgDictionary<PhBoundAny>>>::at_offset(image, root)?;
    let (codes, entries) = (dict.codes()?, dict.entries()?);
    if codes.len() != entries.len() {
        return Err(Broken::Lengths {
            codes: codes.len(),
            entries: entries.len(),
        });
    }
    if !PgDictionary::<PhBoundAny>::codes_are_sorted(&codes) {
        return Err(Broken::CodesUnsorted);
    }
    for (i, code) in codes.iter().enumerate() {
        if let Some(bound) = entries.follow(i)? {
            bounds::check_bound(bound, "").map_err(|why| Broken::Bound { code: *code, why })?;
        }
    }
    Ok(())
}
