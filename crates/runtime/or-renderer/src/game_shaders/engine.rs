//! The values the engine gives the shared constant buffers of the game's shaders, by the names its own reflection gives them.

use std::collections::BTreeMap;

use glam::{Mat4, Vec3, Vec4};
use or_io::gta5::shaders::spirv_pack::ConstantBuffer;

use super::constants::Filler;
use super::lighting::Lighting;

/// A scene as the shared constant buffers see it: the camera, and either the light of a moment of the day or one sun and a flat ambient light.
///
/// ⚠ The names are the game's and the matrices' layout is the game's (each register holds a column); without `lighting` the lighting values are this renderer's own choice of a clear day, not what the game sets.
#[derive(Debug, Clone)]
pub struct EngineConstants {
    pub world: Mat4,
    pub view: Mat4,
    pub projection: Mat4,
    /// The direction towards the sun.
    pub sun_direction: Vec3,
    pub sun_colour: Vec3,
    pub ambient: Vec3,
    /// The light of a moment of the day, which takes the place of the sun and the ambient light above.
    pub lighting: Option<Lighting>,
    /// The skin matrices of the pose, one for each bone of the palette of the scene in its order; the skeleton at rest where none is given.
    pub bones: Option<Vec<Mat4>>,
    /// The size of the picture in pixels, which a pass that reads a depth by the position of a pixel needs.
    pub screen: [f32; 2],
}

impl Default for EngineConstants {
    fn default() -> Self {
        Self {
            world: Mat4::IDENTITY,
            view: Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y),
            projection: Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0),
            sun_direction: Vec3::new(0.35, 0.8, 0.5).normalize(),
            sun_colour: Vec3::new(1.0, 0.95, 0.85),
            ambient: Vec3::new(0.25, 0.3, 0.4),
            lighting: None,
            bones: None,
            screen: [1.0, 1.0],
        }
    }
}

/// How near and how far a perspective projection sees, whether it maps the near plane to nought or to one.
pub fn near_far(projection: &Mat4) -> (f32, f32) {
    let (a, b) = (projection.z_axis.z, projection.w_axis.z);
    if a > 0.0 {
        // The nearer a surface, the greater its depth
        let far = b / a;
        (a * far / (1.0 + a), far)
    } else {
        (b / a, b / (a + 1.0))
    }
}

pub(super) fn columns(m: Mat4) -> Vec<f32> {
    m.to_cols_array().to_vec()
}

use super::mesh::BONES;

/// The bone matrices of a skeleton at rest: each is three registers, the rows of the identity.
fn identity_bones() -> Vec<f32> {
    posed_bones(&[])
}

/// The table of bone matrices with `pose` in its first places and the identity after: each matrix is three registers, the rows of its upper three by four.
fn posed_bones(pose: &[Mat4]) -> Vec<f32> {
    (0..BONES)
        .flat_map(|at| {
            let m = pose.get(at).copied().unwrap_or(Mat4::IDENTITY);
            [
                [m.x_axis.x, m.y_axis.x, m.z_axis.x, m.w_axis.x],
                [m.x_axis.y, m.y_axis.y, m.z_axis.y, m.w_axis.y],
                [m.x_axis.z, m.y_axis.z, m.z_axis.z, m.w_axis.z],
            ]
            .into_iter()
            .flatten()
        })
        .collect()
}

fn colour(c: Vec3) -> Vec<f32> {
    vec![c.x, c.y, c.z, 1.0]
}

impl EngineConstants {
    /// The light the water takes: the ambient light, and the sun by how high it stands.
    pub fn water_light(&self) -> Vec3 {
        let (colour, ambient, height) = match &self.lighting {
            Some(lighting) => (
                lighting.directional_colour.truncate(),
                lighting.natural_ambient[1].truncate(),
                -lighting.directional_light.z,
            ),
            None => (self.sun_colour, self.ambient, self.sun_direction.z),
        };
        ambient + colour * height.max(0.0)
    }

    /// Every value, by the name of the variable it is written to.
    pub fn values(&self) -> BTreeMap<&'static str, Vec<f32>> {
        let sun = self.sun_direction;
        let view_projection = self.projection * self.view;
        let camera = self.view.inverse().w_axis;
        let (near, far) = near_far(&self.projection);
        let [width, height] = self.screen.map(|v| v.max(1.0));
        let mut values = BTreeMap::from([
            ("gWorld", columns(self.world)),
            ("gWorldView", columns(self.view * self.world)),
            (
                "gWorldViewProj",
                columns(self.projection * self.view * self.world),
            ),
            ("gViewInverse", columns(self.view.inverse())),
            ("gViewProj", columns(view_projection)),
            ("gCloudViewProj", columns(view_projection)),
            ("gCameraPos", vec![camera.x, camera.y, camera.z, 1.0]),
            // How a pixel of the picture is a place in the depth map, and how a depth it holds is a distance: the nearer a surface the greater its depth, so the distance is one over a line of it
            (
                "gInvScreenSize",
                vec![1.0 / width, 1.0 / height, 0.5 / width, 0.5 / height],
            ),
            (
                "NearFarPlane",
                vec![near, far, 1.0 / near - 1.0 / far, 1.0 / far],
            ),
            // How a cable's width reaches the screen: the first number turns a width in the world at one unit away into pixels, and a cable thinner than a pixel fades out. This is the renderer's own choice, for a view 1,080 pixels high; the game's values are not decoded, and with the first at nought a cable has no width and draws nothing
            (
                "gCableParams",
                vec![540.0 * self.projection.y_axis.y, 1.0, 1.0, 1.0],
            ),
            // A tyre that is not being driven on keeps its shape: the effect's own default has the contact patch crushing it at the wheel's centre
            ("tyreDeformSwitchOn", vec![0.0]),
            // A wheel the vehicle places is drawn here where its drawable is, so its own matrices are the model's
            ("matWheelWorld", columns(self.world)),
            (
                "matWheelWorldViewProj",
                columns(view_projection * self.world),
            ),
            ("globalScalars", vec![1.0, 1.0, 1.0, 1.0]),
            // The timer of the plants' sway, how far the night emissives are on, whether the camera is in an interior, and the scale of a ped's rim light; with no day the emissives are on
            (
                "globalScalars2",
                vec![
                    1.0,
                    self.lighting
                        .as_ref()
                        .map_or(1.0, |lighting| lighting.night_emissive),
                    0.0,
                    1.0,
                ],
            ),
            ("globalScalars3", vec![1.0, 1.0, 1.0, 1.0]),
            // The UV animation is two rows of three numbers, an affine transform
            ("globalAnimUV0", vec![1.0, 0.0, 0.0]),
            ("globalAnimUV1", vec![0.0, 1.0, 0.0]),
            (
                "gBoneMtx",
                self.bones
                    .as_deref()
                    .map_or_else(identity_bones, posed_bones),
            ),
            // How much of a surface the fade-out leaves: a pass that clips on it discards everything at zero, and a fade of a quarter draws a quarter of the pixels
            ("globalFade", vec![1.0, 1.0, 1.0, 1.0]),
            // Below these a cut-out texel is discarded: the engine's references per kind of surface (global, ped, ped hair, foliage; foliage imposter, billboard, shadow billboard, grass), as bytes over 255. At zero a leaf's transparent edge is drawn
            (
                "gAlphaRefVec0",
                vec![90.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 90.0 / 255.0],
            ),
            (
                "gAlphaRefVec1",
                vec![88.0 / 255.0, 148.0 / 255.0, 88.0 / 255.0, 32.0 / 255.0],
            ),
            // The frame has no sample coverage target: a pass that clips by coverage with the engine's default of one sample discards a hair cut-out whole
            ("gTransparencyAASamples", vec![0.0]),
            // A vehicle is as clean as when it left the showroom: the effect's own default is the dirtiest, which the game sets down for each vehicle, and it makes glass opaque and puts mud on every sill (the dirt level, the dirt's modulation, the spec scales)
            ("dirtLevelMod", vec![0.0, 1.0, 1.0, 1.0]),
        ]);
        match &self.lighting {
            Some(lighting) => values.extend(lighting.values()),
            None => values.extend([
                // ⚠ The way the light travels, away from the sun: the game's lit passes read it so, and given the way towards the sun they lit a surface's back and left the side seen black. Its w multiplies the sun's colour, so it is one
                ("gDirectionalLight", vec![-sun.x, -sun.y, -sun.z, 1.0]),
                ("gDirectionalColour", colour(self.sun_colour)),
                ("gDirectionalAmbientColour", colour(self.ambient)),
                ("gLightNaturalAmbient0", colour(self.ambient)),
                ("gLightNaturalAmbient1", colour(self.ambient)),
                // A ped's ambient is this ramp times its vertex shade, so at zero it is black
                ("gDynamicBakesAndWetness", vec![0.5, 1.0, 0.0, 0.0]),
                // Without a moment of the day the fog is the stand-in of all one, which does not fog a lit surface
                ("globalFogParams", vec![1.0; 20]),
            ]),
        }
        // The light the water is lit by, and where in the frame a point of it refracts to: the picture behind it, as the camera sees it
        let (water_sun, water_ambient) = match &self.lighting {
            Some(lighting) => (
                lighting.directional_colour.truncate(),
                lighting.natural_ambient[1].truncate(),
            ),
            None => (self.sun_colour, self.ambient),
        };
        values.insert("gWaterDirectionalColor", colour(water_sun));
        values.insert("gWaterAmbientColor", colour(water_ambient));
        values.insert(
            "gRefractionWorldViewProj",
            columns(view_projection * self.world),
        );
        // A point read from the reflection is projected by the fourth number of its row, which is the height of its direction here and never nought
        values.insert(
            "gReflectionWorldViewProj",
            columns(Mat4::from_cols(
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 1.0),
                Vec4::new(0.0, 0.0, 1.0, 1.0),
            )),
        );
        values
    }

    /// The bytes of `layout` with every value this scene has for one of its variables written, and the rest zero.
    pub fn fill(&self, layout: &ConstantBuffer) -> Vec<u8> {
        let mut filler = Filler::new(layout);
        for (name, values) in self.values() {
            // A variable the buffer does not have belongs to another buffer; one too small for the values is left at zero
            let _ = filler.set(name, values.as_slice());
        }
        filler.bytes()
    }
}

#[cfg(test)]
mod tests;
