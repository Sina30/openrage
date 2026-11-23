//! `.ydd`: game drawables keyed by name hash, each the drawable a `.ydr`
//! would hold — a ped's components, a vehicle's variants.

use super::drawable::{
    validate_game_drawable, Aliasing, Broken as DrawableBroken, Buffers, DrawableTarget,
    GtaDrawable,
};
use super::shared::PgDictionary;
use crate::decl::*;
use crate::Generation;

impl Resource for PgDictionary<GtaDrawable<Gen8>> {
    const EXT: &'static str = "ydd";
    const TARGET: Generation = Generation::Gen8;
}

#[cfg(feature = "gen9")]
impl Resource for PgDictionary<GtaDrawable<Gen9>> {
    const EXT: &'static str = "ydd";
    const TARGET: Generation = Generation::Gen9;
}

/// A dictionary the game would misread: its order, or one of its drawables.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error(
        "the dictionary's hashes are not ascending, so a lookup would find the wrong drawable"
    )]
    HashesUnsorted,
    #[error("the dictionary holds {hashes} hashes and {entries} drawables")]
    Lengths { hashes: usize, entries: usize },
    #[error("drawable {hash:#010x}: {why}")]
    Drawable { hash: u32, why: DrawableBroken },
    #[error("{0}")]
    Aliased(#[from] Aliasing),
    #[error("{0}")]
    View(#[from] ViewError),
}

/// The buffers of the entry at `at`.
fn visit<T: DrawableTarget>(
    buffers: &mut Buffers,
    image: Image<'_>,
    at: usize,
) -> Result<(), ViewError> {
    buffers.visit(&View::<Raw<GtaDrawable<T>>>::at_offset(image, at)?.base())
}

fn check<T: DrawableTarget>(
    image: Image<'_>,
    root: usize,
    target: Generation,
    buffers: &mut Buffers,
) -> Result<(), Broken>
where
    GtaDrawable<T>: Owned,
    Raw<GtaDrawable<T>>: Record,
{
    let dict = View::<Raw<PgDictionary<GtaDrawable<T>>>>::at_offset(image, root)?;
    let (codes, entries) = (dict.codes()?, dict.entries()?);
    if codes.len() != entries.len() {
        return Err(Broken::Lengths {
            hashes: codes.len(),
            entries: entries.len(),
        });
    }
    if !PgDictionary::<GtaDrawable<T>>::codes_are_sorted(&codes) {
        return Err(Broken::HashesUnsorted);
    }
    for (i, hash) in codes.iter().enumerate() {
        if let Some(drawable) = entries.follow(i)? {
            validate_game_drawable(image, View::offset(&drawable), target)
                .map_err(|why| Broken::Drawable { hash: *hash, why })?;
            visit::<T>(buffers, image, View::offset(&drawable))?;
        }
    }
    Ok(())
}

/// The dictionary's order, every drawable in it, and the buffers they point at.
pub fn validate(image: Image<'_>, root: usize, target: Generation) -> Result<(), Broken> {
    Ok(buffers(image, root, target)?.refuse()?)
}

/// The buffers every drawable of a dictionary points at, once each passes its checks.
pub fn buffers(image: Image<'_>, root: usize, target: Generation) -> Result<Buffers, Broken> {
    let mut buffers = Buffers::default();
    #[cfg(feature = "gen9")]
    if target == Generation::Gen9 {
        check::<Gen9>(image, root, target, &mut buffers)?;
        return Ok(buffers);
    }
    check::<Gen8>(image, root, target, &mut buffers)?;
    Ok(buffers)
}
