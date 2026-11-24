//! The medium of the GTA VI world's water by place: the install's fog atlas, read once on a worker
//! and held, found by a scan as the height map is.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use or_io::container::rpf::GtaViKeys;
use or_io::gta6::ttpf::Texture;
use or_io::gta6::water::fog::FogTexels;
use or_io::gta6::water::GridAtlas;

use super::ground::{atlas, held, textures};

/// The documents that state the grids of the fog's two textures.
const ATLAS_ENTRY: &str = "data/levels/americas/water/waterfogatlas_gridinfo.meta";
const ATLAS2_ENTRY: &str = "data/levels/americas/water/waterfogatlas_gridinfo2.meta";
/// The archive the fog's own archive is nested in, and that archive's path in it.
const ARCHIVE: &str = "textures.rpf";
const FOG: &str = "textures/waterfogatlas/waterfogatlas";

/// The fog atlas's two fallbacks on the grid they share: `waterfog` the medium's scattering a metre, `waterfog2` its extinction a metre.
pub struct WaterFog {
    pub atlas: GridAtlas,
    pub scattering: FogTexels,
    pub extinction: FogTexels,
}

type Read = Arc<OnceLock<Option<Arc<WaterFog>>>>;

static HELD: Mutex<Vec<(PathBuf, Read)>> = Mutex::new(Vec::new());

/// The fog of the water of the install at `root`, read once on a worker and held: `None` until it is read, and where the install's tables give none or it does not read.
pub fn fog(root: &Path) -> Option<Arc<WaterFog>> {
    held(&HELD, root, "the water's fog", read)
}

/// Whether the fog of the install at `root` was asked for and is still being read.
pub fn fog_reading(root: &Path) -> bool {
    super::ground::reading(&HELD, root)
}

fn read(root: &Path, keys: &GtaViKeys) -> Result<WaterFog, String> {
    let (atlas, atlas2) = (
        atlas(root, keys, ATLAS_ENTRY)?,
        atlas(root, keys, ATLAS2_ENTRY)?,
    );
    let (one, two) = (atlas.fallback.clone(), atlas2.fallback.clone());
    let take = |name: &str, texture: &Texture| {
        // ponytail: the fallbacks alone, 32 m a texel; lay the 4 m tiles on them by content as the height map's are if a canal needs its own colour.
        if name != one && name != two {
            return None;
        }
        Some((name == two, FogTexels::of(texture)?))
    };
    let read = textures(root, keys, (ARCHIVE, FOG), &take)?;
    let (mut scattering, mut extinction) = (None, None);
    for (second, texels) in read {
        match second {
            false => scattering = Some(texels),
            true => extinction = Some(texels),
        }
    }
    Ok(WaterFog {
        atlas,
        scattering: scattering.ok_or("the fog's archive holds no first fallback")?,
        extinction: extinction.ok_or("the fog's archive holds no second fallback")?,
    })
}
