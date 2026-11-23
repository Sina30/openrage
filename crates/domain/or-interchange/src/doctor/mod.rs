//! What the mesh doctor does to a drawable: levels of detail, a mirror, weights, the rig and the texture coordinates.
//!
//! ⚠ Every operation works on a [`crate::Scene`] or on a drawable built from one, keeps the shader and the vertex declaration
//! of the geometry it works from, and makes no material and no layout.

pub mod decimate;
pub mod graft;
pub mod levels;
pub mod mirror;
pub mod palette;
pub mod place;
pub mod prop;
pub mod rig;
pub mod uv;

use or_io::error::Result;
use or_io::gta5::RmcDrawable;

use crate::extract::{ExtractOptions, LodSelection, SceneBuilder};
use crate::import::{replace_geometry_named, ImportNames, ImportOptions, Imported};
use crate::scene::{Bone, Scene};

/// `drawable` with every mesh at every level of detail changed by `edit`, on a clone, and what `edit` said.
///
/// The scene is read with `skeleton` as the bones a skin without a skeleton of its own is bound to, and what `edit` leaves is
/// written back into the same geometries: the shaders, the vertex declarations and the levels are the drawable's own.
pub fn through_scene<T: Imported, R>(
    drawable: &RmcDrawable<T>,
    skeleton: Option<&[Bone]>,
    edit: impl FnOnce(&mut Scene) -> Result<R>,
) -> Result<(RmcDrawable<T>, R)> {
    let mut builder = SceneBuilder::new(&drawable.name);
    if let Some(bones) = skeleton {
        builder = builder.with_reference_skeleton(bones.to_vec());
    }
    builder.push(
        &drawable.name,
        drawable,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    let mut scene = builder.finish();
    let said = edit(&mut scene)?;
    let options = ImportOptions {
        fill_missing: false,
        keep_skinning: false,
    };
    let (made, _) = replace_geometry_named(drawable, &scene, &options, &ImportNames::default())?;
    Ok((made, said))
}
