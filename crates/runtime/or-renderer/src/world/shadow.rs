//! The sun's shadow over a world: four cascades about what the camera sees, each cast into a tile of the game's map and read by the game's own pass.

use std::collections::BTreeMap;

use glam::{Mat4, Vec3, Vec4};

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

/// How far below the surface that casts it the shadow is held in a cascade, as a part of the width of its tile in the world, so that a surface does not shade itself.
const DEPTH_BIAS: f32 = 0.0015;

/// The variable the receiving pass reads the axes, the fade and each cascade's tile from, and where the scales and the offsets of the tiles start in it, a register of four a cascade.
const SHARED: &str = "gCSMShaderVars_shared";
const SCALES: usize = 16;
const OFFSETS: usize = 32;

/// The cascades of one frame, the nearest first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cascades {
    /// Where each tile lies across the light.
    fits: [Fit; CASCADES],
    // ⚠ One depth for all four: the receiving pass gives a pixel the first tile it lies across whatever its depth, so a tile that held only the depths of its own sphere would shade everything past them.
    /// What a point of the world is multiplied by for its depth in every tile, from nought at the nearest the sun any cascade reaches to one at the furthest.
    depth: Vec4,
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
        let spheres = ENDS.map(|end| {
            let end = end.min(far).max(near);
            let (centre, radius) = slice_sphere(eye, forward, camera.fov_y, aspect, near, end);
            near = end;
            snapped(sun.travel, centre, radius)
        });
        let fits = spheres.map(|(centre, radius)| Fit::of(sun.travel, centre, radius));
        // The third row of a caster's view is the way the light travels
        let along = fits[0].caster().view.row(2).truncate();
        let reach = |side: f32| spheres.map(|(centre, radius)| centre.dot(along) + side * radius);
        let first = reach(-1.0).into_iter().fold(f32::INFINITY, f32::min);
        let last = reach(1.0).into_iter().fold(f32::NEG_INFINITY, f32::max);
        let span = (last - first).max(1e-3);
        Some(Self {
            fits,
            depth: (along / span).extend(-first / span),
            far,
        })
    }

    /// The world to the clip space of each tile, which whatever casts is drawn through into it: across and down the tile as its own fit lays it, and the one depth of all the tiles.
    pub(crate) fn views(&self) -> [Mat4; CASCADES] {
        self.fits.map(|fit| {
            let caster = fit.caster();
            let rows = (caster.projection * caster.view).transpose();
            Mat4::from_cols(rows.x_axis, rows.y_axis, self.depth, rows.w_axis).transpose()
        })
    }

    /// The constants a scene's own surfaces are cast with, into the nearest tile.
    pub(crate) fn caster(&self) -> EngineConstants {
        let fitted = self.fits[0].caster();
        EngineConstants {
            // The view is a turn and a move, and the projection whatever takes it to the tile
            projection: self.views()[0] * fitted.view.inverse(),
            ..fitted
        }
    }

    /// The variables the receiving pass reads of the shadow for a viewer at `eye`: what one fit gives, with each cascade's own tile, the one depth of them all, and a fade to no shadow at the end of the reach.
    pub(crate) fn values(&self, eye: Vec3) -> BTreeMap<&'static str, Vec<f32>> {
        let tiles = self.fits.map(|fit| tile_of(&fit, eye));
        let mut values = self.fits[0].values(eye);
        let Some(shared) = values.get_mut(SHARED) else {
            return values;
        };
        // The pass works a place out from the eye, so the depth of the eye is part of the offset
        let deep = self.depth.truncate().length();
        let at_eye = self.depth.dot(eye.extend(1.0));
        for (cascade, tile) in tiles.iter().enumerate() {
            let Some((mut scale, mut offset)) = *tile else {
                continue;
            };
            // A tile's scale is one over its width in the world
            let bias = DEPTH_BIAS * deep / scale[0].max(f32::MIN_POSITIVE);
            (scale[2], offset[2]) = (deep, at_eye - bias);
            for (at, register) in [(SCALES, scale), (OFFSETS, offset)] {
                let from = at + cascade * 4;
                if let Some(place) = shared.get_mut(from..from + 4) {
                    place.copy_from_slice(&register);
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

/// A sphere a little larger than this one, its centre moved to a whole texel of the tile a fit about it has: the texels then stay where they are in the world as the camera moves, and a shadow's edge does not crawl.
fn snapped(travel: Vec3, centre: Vec3, radius: f32) -> (Vec3, f32) {
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
    (moved, radius)
}
