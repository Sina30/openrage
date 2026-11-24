//! GTA VI's water: the level's `WaterData` document, a metadata document in the table form whose
//! quads are level rectangles and cut corners of water, and the grid a water atlas is tiled by.
//!
//! ⚠ The level's height map, which [`height`] lays on the grid of [`GridAtlas`], is the ground
//! under the water and not its surface: a quad's `z` is the surface.

use crate::gta5::data::timecycle::{attr, tokens, Token};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::pso::table_form;
use crate::meta::binary::rbm::schema::RbmModel;
use crate::meta::error::MetaError;

#[path = "water_fog.rs"]
pub mod fog;
#[path = "water_height.rs"]
pub mod height;

pub const WATER_DATA: u32 = lit(b"WaterData");
pub const WATER_QUAD: u32 = lit(b"WaterQuad");
const ROOT_SIZE: usize = 0x100;
const QUAD_SIZE: usize = 0x2C;
/// Where the root holds its `WaterQuads`.
const QUADS: usize = 0xE0;
const ARRAY: u8 = 0x52;
const S16: u8 = 0x12;
const U8: u8 = 0x11;
const BOOL: u8 = 0x01;
const FLOAT: u8 = 0x21;

/// The members of a quad this reads: name, offset and type code, which a body's own table must agree with.
const QUAD: &[(u32, u32, u8)] = &[
    (lit(b"minX"), 0x00, S16),
    (lit(b"minY"), 0x02, S16),
    (lit(b"maxX"), 0x04, S16),
    (lit(b"maxY"), 0x06, S16),
    (lit(b"a1"), 0x08, U8),
    (lit(b"a2"), 0x09, U8),
    (lit(b"a3"), 0x0A, U8),
    (lit(b"a4"), 0x0B, U8),
    (lit(b"z"), 0x18, FLOAT),
    (lit(b"Type"), 0x1C, U8),
    (lit(b"oceanQuad"), 0x26, BOOL),
    (lit(b"IsInvisible"), 0x28, BOOL),
    (lit(b"HasLimitedDepth"), 0x29, BOOL),
    (lit(b"NoStencil"), 0x2A, BOOL),
];

/// Why bytes are not a water document.
#[derive(Debug, thiserror::Error)]
pub enum WaterError {
    #[error("the water document's type table and blocks do not read: {0}")]
    Model(MetaError),
    #[error("the document's root is of type {0:#010X}, not WaterData")]
    NotWater(u32),
    #[error("the water document declares {class} with member {member:#010X} elsewhere than at {offset:#X} with type {code:#04X}")]
    Layout {
        class: &'static str,
        member: u32,
        offset: u32,
        code: u8,
    },
    #[error("the water document is broken: {0}")]
    Broken(String),
    #[error("the text is not a grid atlas: {0}")]
    NotAnAtlas(String),
    #[error("the height map does not fit its grid: {0}")]
    Ground(String),
}

/// One `WaterQuad`: a level rectangle of water, in whole metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterQuad {
    pub min: [i16; 2],
    pub max: [i16; 2],
    /// The opacity at each corner, `a1` to `a4`: south-west, south-east, north-east, north-west.
    pub alpha: [u8; 4],
    /// The height of its surface.
    pub z: f32,
    /// 0 a whole rectangle; 1 to 4 the triangle of it without its north-east, south-east, south-west or north-west corner.
    pub kind: u8,
    pub ocean: bool,
    pub invisible: bool,
    pub limited_depth: bool,
    pub no_stencil: bool,
}

/// The quads of a `WaterData` document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WaterData {
    pub quads: Vec<WaterQuad>,
}

fn broken(why: impl Into<String>) -> WaterError {
    WaterError::Broken(why.into())
}

impl WaterData {
    /// The water the table-form document `bytes` holds.
    pub fn open(bytes: &[u8]) -> Result<Self, WaterError> {
        Self::read(&table_form::model_of(bytes).map_err(WaterError::Model)?)
    }

    /// The water of a table-form document's type table and blocks.
    pub fn read(model: &RbmModel) -> Result<Self, WaterError> {
        let root = (model.block(model.root_block_id as i32))
            .ok_or_else(|| broken("its root names no block"))?;
        if root.data_type != WATER_DATA {
            return Err(WaterError::NotWater(root.data_type));
        }
        let declares = |class: u32, name: &'static str, member: u32, offset: u32, code: u8| {
            let fits = model.class_for(class).is_some_and(|c| {
                (c.entries.iter()).any(|e| {
                    e.name_hash == member && e.offset == offset && e.data_type.to_byte() == code
                })
            });
            fits.then_some(()).ok_or(WaterError::Layout {
                class: name,
                member,
                offset,
                code,
            })
        };
        declares(
            WATER_DATA,
            "WaterData",
            lit(b"WaterQuads"),
            QUADS as u32,
            ARRAY,
        )?;
        if root.data.len() < ROOT_SIZE {
            return Err(broken("its root is shorter than a WaterData"));
        }
        let word = |at: usize| u64::from_le_bytes(std::array::from_fn(|k| root.data[at + k]));
        let (raw, counts) = (word(QUADS), word(QUADS + 8));
        let (count, capacity) = ((counts & 0xFFFF) as usize, (counts >> 16 & 0xFFFF) as usize);
        if count > capacity {
            return Err(broken(format!(
                "{count} quads are over the capacity {capacity}"
            )));
        }
        if count == 0 {
            return Ok(Self::default());
        }
        // A quad is read only where the body declares it as this reads it.
        for &(member, offset, code) in QUAD {
            declares(WATER_QUAD, "WaterQuad", member, offset, code)?;
        }
        if (raw >> 12) & 0xFF != 0 {
            return Err(broken(format!(
                "the quads' pointer {raw:#X} sets bits 12 to 19"
            )));
        }
        let block = (model.block((raw & 0xFFF) as i32))
            .filter(|b| b.data_type == WATER_QUAD)
            .ok_or_else(|| broken("the quads' pointer names no block of quads"))?;
        let at = usize::try_from(raw >> 20).unwrap_or(usize::MAX);
        let bytes = (count.checked_mul(QUAD_SIZE))
            .and_then(|len| at.checked_add(len))
            .and_then(|end| block.data.get(at..end))
            .ok_or_else(|| broken(format!("{count} quads run past their block")))?;
        let quads = (bytes.as_chunks::<QUAD_SIZE>().0.iter())
            .map(|q| {
                let s16 = |at: usize| i16::from_le_bytes([q[at], q[at + 1]]);
                WaterQuad {
                    min: [s16(0), s16(2)],
                    max: [s16(4), s16(6)],
                    alpha: [q[8], q[9], q[10], q[11]],
                    z: f32::from_le_bytes([q[0x18], q[0x19], q[0x1A], q[0x1B]]),
                    kind: q[0x1C],
                    ocean: q[0x26] != 0,
                    invisible: q[0x28] != 0,
                    limited_depth: q[0x29] != 0,
                    no_stencil: q[0x2A] != 0,
                }
            })
            .collect::<Vec<_>>();
        if let Some(bad) = quads.iter().find(|q| !q.z.is_finite()) {
            return Err(broken(format!("a quad's height is {}", bad.z)));
        }
        Ok(Self { quads })
    }
}

/// A `GridAtlasInfo`: the grid a streamed atlas of the level is tiled by, as the water's height map and its fog are.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridAtlas {
    pub client: String,
    pub tile_prefix: String,
    pub tile_suffix: String,
    pub fallback: String,
    /// The grid's centre, in metres; its rows run south from its north edge.
    pub origin: [f32; 2],
    /// The grid's extent along x and along y, in metres.
    pub size: [f32; 2],
    pub tile_size: f32,
    pub stream_radius: f32,
}

impl GridAtlas {
    /// The grid's west edge, in metres.
    pub fn west(&self) -> f32 {
        self.origin[0] - self.size[0] / 2.0
    }

    /// The grid's north edge, in metres.
    pub fn north(&self) -> f32 {
        self.origin[1] + self.size[1] / 2.0
    }

    pub fn parse(text: &str) -> Result<Self, WaterError> {
        let mut atlas = Self::default();
        let (mut rooted, mut depth, mut open) = (false, 0usize, "");
        for token in tokens(text) {
            match token {
                Token::Open { name, attrs, empty } => {
                    if !rooted && name != "GridAtlasInfo" {
                        return Err(WaterError::NotAnAtlas(format!("its root is `{name}`")));
                    }
                    rooted = true;
                    depth += 1;
                    let number = |key: &str| attr(attrs, key).and_then(|v| v.trim().parse().ok());
                    // The overrides of a sector repeat these names a level down.
                    if depth == 2 {
                        match name {
                            "origin" => {
                                atlas.origin =
                                    [number("x").unwrap_or(0.0), number("y").unwrap_or(0.0)]
                            }
                            "width" => atlas.size[0] = number("value").unwrap_or(0.0),
                            "height" => atlas.size[1] = number("value").unwrap_or(0.0),
                            "tileSize" => atlas.tile_size = number("value").unwrap_or(0.0),
                            "streamRadius" => atlas.stream_radius = number("value").unwrap_or(0.0),
                            _ => {}
                        }
                        open = name;
                    }
                    if empty {
                        depth -= 1;
                    }
                }
                Token::Close(_) => depth = depth.saturating_sub(1),
                Token::Text(text) if depth == 2 => {
                    let field = match open {
                        "clientSystemName" => &mut atlas.client,
                        "tileTextureNamePrefix" => &mut atlas.tile_prefix,
                        "tileTextureNameSuffix" => &mut atlas.tile_suffix,
                        "globalFallbackTextureName" => &mut atlas.fallback,
                        _ => continue,
                    };
                    *field = text.to_string();
                }
                Token::Text(_) => {}
            }
        }
        if !rooted || atlas.tile_size <= 0.0 {
            return Err(WaterError::NotAnAtlas("it states no tile size".into()));
        }
        Ok(atlas)
    }
}

#[cfg(test)]
#[path = "water_tests.rs"]
mod tests;
