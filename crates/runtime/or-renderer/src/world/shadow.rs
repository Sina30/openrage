//! The sun's shadow over a world: four cascades about what the camera sees, each cast into a tile of the game's map and read by the game's own pass.

use std::collections::BTreeMap;

use glam::{Mat4, Vec3};

use crate::fly::slice_sphere;
use crate::game_shaders::deferred::Sun;
use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::shadow::{Fit, TILE};
use crate::viewport::OrbitCamera;

/// How many cascades the game's pass reads, each a tile of its map.
pub(crate) const CASCADES: usize = 4;

/// How far ahead of the camera the sun's shadow of a world is cast, in metres.
pub const SHADOW_FAR: f32 = 300.0;

/// How far ahead of the camera each cascade ends, in metres: this renderer's own choice, each about three and a half times the one before.
const ENDS: [f32; CASCADES] = [8.0, 30.0, 100.0, SHADOW_FAR];

/// The part of the reach, at its far end, over which the shadow fades to none.
const FADE: f32 = 0.1;

/// The variable the receiving pass reads the axes, the fade and each cascade's tile from, and where the scales and the offsets of the tiles start in it, a register of four a cascade.
const SHARED: &str = "gCSMShaderVars_shared";
const SCALES: usize = 16;
const OFFSETS: usize = 32;

/// The cascades of one frame, the nearest first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cascades {
    fits: [Fit; CASCADES],
    /// How far ahead of the camera the last ends.
    far: f32,
}

impl Cascades {
    /// The cascades about what `camera` sees in a frame of `aspect`, cast by the sun of `engine`; none where it does not shine.
    pub(crate) fn about(
        engine: &EngineConstants,
        camera: &OrbitCamera,
        aspect: f32,
    ) -> Option<Self> {
        let sun = Sun::of(engine);
        if !sun.shines() {
            return None;
        }
        let eye = camera.eye();
        let forward = (camera.target - eye).normalize_or(Vec3::Y);
        let far = SHADOW_FAR.min(camera.far);
        let mut near = camera.near.min(far);
        let fits = ENDS.map(|end| {
            let end = end.min(far).max(near);
            let (centre, radius) = slice_sphere(eye, forward, camera.fov_y, aspect, near, end);
            near = end;
            snapped(sun.travel, centre, radius)
        });
        Some(Self { fits, far })
    }

    /// The fit of the nearest cascade, which a scene's own surfaces are cast into.
    pub(crate) fn nearest(&self) -> Fit {
        self.fits[0]
    }

    /// The world to the clip space of each tile, which the instances are drawn through into it.
    pub(crate) fn views(&self) -> [Mat4; CASCADES] {
        self.fits.map(|fit| {
            let caster = fit.caster();
            caster.projection * caster.view
        })
    }

    /// The variables the receiving pass reads of the shadow for a viewer at `eye`: what one fit gives, with each cascade's own tile and a fade to no shadow at the end of the reach.
    pub(crate) fn values(&self, eye: Vec3) -> BTreeMap<&'static str, Vec<f32>> {
        let tiles = self.fits.map(|fit| tile_of(&fit, eye));
        let mut values = self.fits[0].values(eye);
        let Some(shared) = values.get_mut(SHARED) else {
            return values;
        };
        for (cascade, tile) in tiles.iter().enumerate() {
            let Some((scale, offset)) = tile else {
                continue;
            };
            for (at, register) in [(SCALES, scale), (OFFSETS, offset)] {
                let from = at + cascade * 4;
                if let Some(place) = shared.get_mut(from..from + 4) {
                    place.copy_from_slice(register);
                }
            }
        }
        // The shadow is whole where a distance ahead times the first of these plus the second is one or more, and gone where it is nought
        let over = (self.far * FADE).max(1e-3);
        for (at, word) in [(3, -1.0 / over), (7, self.far / over)] {
            if let Some(place) = shared.get_mut(at) {
                *place = word;
            }
        }
        values
    }
}

/// The scale and the offset a fit gives the tile of every cascade, which are its own tile's.
fn tile_of(fit: &Fit, eye: Vec3) -> Option<([f32; 4], [f32; 4])> {
    let values = fit.values(eye);
    let shared = values.get(SHARED)?;
    let register = |at: usize| <[f32; 4]>::try_from(shared.get(at..at + 4)?).ok();
    Some((register(SCALES)?, register(OFFSETS)?))
}

/// The fit about a sphere a little larger, its centre moved to a whole texel of its tile: the texels then stay where they are in the world as the camera moves, and a shadow's edge does not crawl.
fn snapped(travel: Vec3, centre: Vec3, radius: f32) -> Fit {
    // Room for the move, of half a texel at most each way
    let radius = radius * (1.0 + 2.0 / TILE as f32);
    let texel = 2.0 * radius / TILE as f32;
    // The first two rows of a caster's view are the axes of its tile
    let view = Fit::of(travel, centre, radius).caster().view;
    let moved = [0, 1].into_iter().fold(centre, |centre, row| {
        let axis = view.row(row).truncate();
        let along = centre.dot(axis);
        centre + axis * ((along / texel).round() * texel - along)
    });
    Fit::of(travel, moved, radius)
}
