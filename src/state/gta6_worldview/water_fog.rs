//! The medium of the GTA VI world's water by place: the install's fog atlas, read once on a worker
//! and held, found by a scan as the height map is, its tiles laid on the fallback by their content.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use or_io::container::rpf::GtaViKeys;
use or_io::gta6::ttpf::Texture;
use or_io::gta6::water::fog::FogTexels;
use or_io::gta6::water::height::{Heights, WaterGround};
use or_io::gta6::water::GridAtlas;

use super::ground::{atlas, held, textures};

/// The documents that state the grids of the fog's two textures.
const ATLAS_ENTRY: &str = "data/levels/americas/water/waterfogatlas_gridinfo.meta";
const ATLAS2_ENTRY: &str = "data/levels/americas/water/waterfogatlas_gridinfo2.meta";
/// The archive the fog's own archive is nested in, and that archive's path in it.
const ARCHIVE: &str = "textures.rpf";
const FOG: &str = "textures/waterfogatlas/waterfogatlas";
/// The names a tile's two textures have: the scattering and the extinction.
const TILE: &str = "waterfog";
const TILE2: &str = "waterfog2";
/// The fewest tiles about the camera's own, each way, the near medium spans where the grid states no stream radius.
const LEAST_AROUND: u32 = 1;
/// The longest side a tile's texture is laid at, far past any an atlas lays: a texture's size is
/// its archive's own number, and the medium about it holds nine times its texels.
const MOST_TILE_SIDE: u32 = 2048;

/// The fog atlas's two fallbacks on the grid they share: `waterfog` the medium's scattering a metre, `waterfog2` its extinction a metre; and the tiles laid on them.
pub struct WaterFog {
    pub atlas: GridAtlas,
    pub scattering: FogTexels,
    pub extinction: FogTexels,
    /// The commonest texel's extinction and scattering: the open sea's, and the water's off the grid.
    pub commonest: ([f32; 3], [f32; 3]),
    /// Each tile laid, by its column east and row south: its extinction and scattering textures, still compressed.
    tiles: HashMap<(u32, u32), (Texture, Texture)>,
}

/// The medium about a tile at the atlas's own size: its grid's west and north edges, its extent, its side in texels, and each texel's extinction and scattering, row by row from the north.
pub struct NearFog {
    pub west: f32,
    pub north: f32,
    pub size: f32,
    pub side: u32,
    pub extinction: Vec<[f32; 3]>,
    pub scattering: Vec<[f32; 3]>,
}

impl WaterFog {
    /// How many tiles about the camera's own, each way, the near medium spans: as far as the grid streams its tiles.
    pub fn around(&self) -> u32 {
        let tile = self.atlas.tile_size;
        if !(tile > 0.0 && self.atlas.stream_radius > 0.0) {
            return LEAST_AROUND;
        }
        ((self.atlas.stream_radius / tile).ceil() as u32).max(LEAST_AROUND)
    }

    /// The tile of the grid `x`, `y` lies in; `None` off the grid.
    pub fn tile_of(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        let (east, south) = (x - self.atlas.west(), self.atlas.north() - y);
        let tile = self.atlas.tile_size;
        let across = self.atlas.size.map(|s| (s / tile).round() as u32);
        let at = ((east / tile).floor(), (south / tile).floor());
        (at.0 >= 0.0 && at.1 >= 0.0 && (at.0 as u32) < across[0] && (at.1 as u32) < across[1])
            .then_some((at.0 as u32, at.1 as u32))
    }

    /// The medium of the tiles about `(col, row)` at the atlas's own size: a laid tile's texels, else the fallback's under it; `None` where no tile about it is laid.
    pub fn near(&self, (col, row): (u32, u32)) -> Option<NearFog> {
        let around = self.around();
        let (first_col, first_row) = (col.saturating_sub(around), row.saturating_sub(around));
        let span = 2 * around + 1;
        let laid: Vec<_> = (0..span * span)
            .map(|k| (first_col + k % span, first_row + k / span))
            .map(|at| (at, self.tiles.get(&at)))
            .collect();
        let (_, (first, _)) = laid.iter().find_map(|(at, t)| t.map(|t| (at, t)))?;
        let tile_side = first.width;
        if tile_side == 0 || tile_side > MOST_TILE_SIDE {
            return None;
        }
        let side = tile_side * span;
        let fallback =
            self.extinction.width() / (self.atlas.size[0] / self.atlas.tile_size).round() as u32;
        let mut near = NearFog {
            west: self.atlas.west() + first_col as f32 * self.atlas.tile_size,
            north: self.atlas.north() - first_row as f32 * self.atlas.tile_size,
            size: span as f32 * self.atlas.tile_size,
            side,
            extinction: vec![[0.0; 3]; (side * side) as usize],
            scattering: vec![[0.0; 3]; (side * side) as usize],
        };
        for ((tile_col, tile_row), tile) in laid {
            let decoded = tile
                .and_then(|(e, s)| Some((FogTexels::of(e)?, FogTexels::of(s)?)))
                .filter(|(e, s)| {
                    e.width() == tile_side
                        && e.height() == tile_side
                        && s.width() == tile_side
                        && s.height() == tile_side
                });
            let (ox, oy) = (
                (tile_col - first_col) * tile_side,
                (tile_row - first_row) * tile_side,
            );
            for j in 0..tile_side {
                for i in 0..tile_side {
                    let (e, s) = match &decoded {
                        Some((e, s)) => {
                            let k = (j * tile_side + i) as usize;
                            (e.texels()[k], s.texels()[k])
                        }
                        // A tile not laid is the fallback's texel under it.
                        None => {
                            let fi = tile_col * fallback + i * fallback / tile_side;
                            let fj = tile_row * fallback + j * fallback / tile_side;
                            let k = (fj * self.extinction.width() + fi) as usize;
                            match (
                                self.extinction.texels().get(k),
                                self.scattering.texels().get(k),
                            ) {
                                (Some(e), Some(s)) => (*e, *s),
                                _ => (self.commonest.0, self.commonest.1),
                            }
                        }
                    };
                    let k = ((oy + j) * side + ox + i) as usize;
                    near.extinction[k] = e;
                    near.scattering[k] = s;
                }
            }
        }
        Some(near)
    }
}

/// The pair of texels `extinction` and `scattering` hold together most often.
fn commonest(extinction: &FogTexels, scattering: &FogTexels) -> ([f32; 3], [f32; 3]) {
    let mut counts = std::collections::HashMap::<[u32; 6], usize>::new();
    for (e, s) in extinction.texels().iter().zip(scattering.texels()) {
        let key = [e[0], e[1], e[2], s[0], s[1], s[2]].map(f32::to_bits);
        *counts.entry(key).or_default() += 1;
    }
    let best = (counts.into_iter().max_by_key(|(key, n)| (*n, *key)))
        .map(|(key, _)| key.map(f32::from_bits));
    best.map_or(([0.0; 3], [0.0; 3]), |k| {
        ([k[0], k[1], k[2]], [k[3], k[4], k[5]])
    })
}

/// An extinction as a step of a scale even in its logarithm, for laying tiles by their content as the height map's are.
fn level(e: [f32; 3]) -> u16 {
    let mean = (e[0] + e[1] + e[2]) / 3.0;
    ((mean.max(1e-6).ln() + 16.0) * 2000.0).clamp(0.0, 65535.0) as u16
}

/// Every texel of `texels` as its [`level`], `side` by `side` of their means.
fn levels(texels: &FogTexels, side: u32) -> Option<Heights> {
    let (w, h) = (texels.width(), texels.height());
    if side == 0 || !w.is_multiple_of(side) || !h.is_multiple_of(side) {
        return None;
    }
    let (across, down) = (w / side, h / side);
    let mut sums = vec![[0.0f32; 3]; (side * side) as usize];
    for (k, t) in texels.texels().iter().enumerate() {
        let (i, j) = (k as u32 % w, k as u32 / w);
        let sum = &mut sums[((j / down) * side + i / across) as usize];
        for c in 0..3 {
            sum[c] += t[c];
        }
    }
    let n = (across * down) as f32;
    Heights::new(
        side,
        side,
        sums.iter().map(|s| level(s.map(|v| v / n))).collect(),
    )
}

/// A texture's top level alone.
fn top(texture: &Texture) -> Texture {
    // In `usize`: the sides are the archive's own numbers.
    let blocks = |side: u32| side.div_ceil(4) as usize;
    let bytes = blocks(texture.width) * blocks(texture.height) * 16;
    Texture {
        pixels: texture.pixels[..bytes.min(texture.pixels.len())].to_vec(),
        levels: 1,
        ..texture.clone()
    }
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

/// What a texture of the fog's archive is: a fallback, decoded, or one of a tile's two, kept as it is with its extinction's levels.
enum Taken {
    Fallback(bool, FogTexels),
    Tile(u64, bool, Texture, Option<FogTexels>),
}

/// A tile's scattering, and its extinction with that decoded, as they are found.
type TilePair = (Option<Texture>, Option<(Texture, FogTexels)>);

fn read(root: &Path, keys: &GtaViKeys) -> Result<WaterFog, String> {
    let (atlas, atlas2) = (
        atlas(root, keys, ATLAS_ENTRY)?,
        atlas(root, keys, ATLAS2_ENTRY)?,
    );
    let (one, two) = (atlas.fallback.clone(), atlas2.fallback.clone());
    let take = |offset: u64, name: &str, texture: &Texture| {
        if name == one || name == two {
            return Some(Taken::Fallback(name == two, FogTexels::of(texture)?));
        }
        // ⚠ The extinction alone is decoded here, to lay the tile; both stay compressed until the camera comes near.
        match name {
            TILE => Some(Taken::Tile(offset, false, top(texture), None)),
            TILE2 => Some(Taken::Tile(
                offset,
                true,
                top(texture),
                Some(FogTexels::of(texture)?),
            )),
            _ => None,
        }
    };
    let read = textures(root, keys, (ARCHIVE, FOG), &take)?;
    let (mut scattering, mut extinction) = (None, None);
    let mut pairs: HashMap<u64, TilePair> = HashMap::new();
    for taken in read {
        match taken {
            Taken::Fallback(false, texels) => scattering = Some(texels),
            Taken::Fallback(true, texels) => extinction = Some(texels),
            Taken::Tile(offset, false, texture, _) => {
                pairs.entry(offset).or_default().0 = Some(texture)
            }
            Taken::Tile(offset, true, texture, texels) => {
                pairs.entry(offset).or_default().1 = texels.map(|t| (texture, t))
            }
        }
    }
    let scattering = scattering.ok_or("the fog's archive holds no first fallback")?;
    let extinction = extinction.ok_or("the fog's archive holds no second fallback")?;
    // The tiles are laid as the height map's are: each by its extinction's levels against the fallback's.
    let fallback = Heights::new(
        extinction.width(),
        extinction.height(),
        extinction.texels().iter().map(|t| level(*t)).collect(),
    )
    .ok_or("the fog's fallback is empty")?;
    let mut ground = WaterGround::new(&atlas, fallback).map_err(|e| e.to_string())?;
    let block = extinction.width() / (atlas.size[0] / atlas.tile_size).round().max(1.0) as u32;
    let mut tiles = HashMap::new();
    // In the scan's order, so that the same archive lays its tiles the same way every time.
    let mut offsets: Vec<_> = pairs.keys().copied().collect();
    offsets.sort_unstable();
    for offset in offsets {
        let Some((Some(scatter), Some((extinct, texels)))) = pairs.remove(&offset) else {
            continue;
        };
        let Some(small) = levels(&texels, block) else {
            continue;
        };
        if let Some(at) = ground.add_tile(small) {
            tiles.insert(at, (extinct, scatter));
        }
    }
    tracing::info!(
        tiles = tiles.len(),
        stream_radius = atlas.stream_radius,
        tile_size = atlas.tile_size,
        "the water's fog tiles were laid"
    );
    Ok(WaterFog {
        commonest: commonest(&extinction, &scattering),
        atlas,
        scattering,
        extinction,
        tiles,
    })
}

#[cfg(test)]
mod tests {
    use or_io::gta6::ttpf::Format;

    use super::*;

    /// An unsigned BC6H texture of `width` by `height`, every block nought.
    fn bc6h(width: u32, height: u32) -> Texture {
        let blocks = (width.div_ceil(4) * height.div_ceil(4)) as usize;
        Texture {
            width,
            height,
            format: Format::of_dxgi(95).expect("BC6H_UF16"),
            levels: 1,
            faces: 1,
            pixels: vec![0; blocks * 16],
        }
    }

    fn fog_of(tile: Texture) -> WaterFog {
        let text = r#"<GridAtlasInfo><tileTextureNamePrefix>t-</tileTextureNamePrefix>
            <globalFallbackTextureName>f</globalFallbackTextureName><origin x="0.0" y="0.0" z="0.0" />
            <width value="1024" /><height value="1024" /><tileSize value="512" /></GridAtlasInfo>"#;
        let fallback = FogTexels::of(&bc6h(8, 8)).expect("a fallback");
        WaterFog {
            atlas: GridAtlas::parse(text).expect("an atlas"),
            scattering: fallback.clone(),
            extinction: fallback,
            commonest: ([0.0; 3], [0.0; 3]),
            tiles: HashMap::from([((0, 0), (tile.clone(), tile))]),
        }
    }

    #[test]
    fn a_tile_whose_texture_states_a_side_past_any_atlas_is_not_laid_and_never_overflows() {
        // An archive's own numbers: a side whose blocks' bytes pass what 32 bits hold, and no pixels.
        let wide = Texture {
            width: 70_000,
            height: 70_000,
            pixels: Vec::new(),
            ..bc6h(4, 4)
        };
        assert!(top(&wide).pixels.is_empty());
        assert!(fog_of(wide).near((0, 0)).is_none());
        assert_eq!(
            fog_of(bc6h(4, 4)).near((0, 0)).map(|near| near.side),
            Some(12)
        );
    }
}
