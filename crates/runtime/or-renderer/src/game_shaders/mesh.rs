//! Meshes drawn with the game's own passes: each surface with the pass its material's effect names, the vertex data its inputs ask for, its material's values and its textures.

use std::collections::BTreeMap;

use glam::{Mat4, Vec3};
use or_io::gta5::shaders::spirv_pack::{Effect, Pass, SignatureElement};
use thiserror::Error;

use super::engine::EngineConstants;
use super::frame::{Frame, Rendered};
use super::pack::GamePack;
use super::pipeline::Target;
use super::quad::pair_of;
use super::stand_in::Provided;

pub mod scene;

pub use scene::{MeshScene, PipelineCache};

/// Whether the material parameter `parameter` is the variable or resource `name`: spelled alike, or, where nothing names the hash a drawable stores, the `0x` hash of it.
fn is_named(parameter: &str, name: &str) -> bool {
    parameter.eq_ignore_ascii_case(name)
        || parameter
            .strip_prefix("0x")
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            == Some(or_io::hash::string_hash_str(name))
}

/// The geometry of one drawn surface, in the model's own space.
#[derive(Debug, Clone, Default)]
pub struct Surface {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    /// Texture coordinates by the number of the `TEXCOORD` semantic they feed.
    pub uvs: BTreeMap<u32, Vec<[f32; 2]>>,
    /// Vertex colours by the number of the `COLOR` semantic they feed.
    pub colours: BTreeMap<u32, Vec<[f32; 4]>>,
    pub weights: Vec<[f32; 4]>,
    /// The bone of the model's skeleton each weight is of, by the same index.
    pub joints: Vec<[u16; 4]>,
    /// Three indices a triangle.
    pub indices: Vec<u32>,
    /// Whether the surface is skinned: it then draws with a skinned pass, in its bind pose.
    pub skinned: bool,
}

/// A texture's first level as 8-bit RGBA.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Whether the texture states the sRGB colour space, which a shader reads as linear.
    pub srgb: bool,
}

/// How a surface's coverage is resolved: the draw bucket its material belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coverage {
    /// Every pixel it covers.
    #[default]
    Opaque,
    /// The pixels its texture's alpha keeps, by the pass the effect has for it.
    Cutout,
    /// Blended over what is behind it, after what is opaque.
    Blended,
}

/// What a surface draws with: the effect, the constants it sets and the textures it binds, by the names of the game's own parameters.
#[derive(Debug, Clone, Default)]
pub struct Material {
    /// The name of the effect, such as `default` or `normal_spec`.
    pub effect: String,
    /// A constant by the name of the variable it sets, one row of four a register.
    pub vectors: Vec<(String, Vec<[f32; 4]>)>,
    /// A texture by the name of the parameter that binds it.
    pub textures: BTreeMap<String, Image>,
    pub coverage: Coverage,
    /// Which pass of the technique draws it: the first by default.
    pub pass: usize,
    /// A texture a pass of its own drew, by the name of the parameter that binds it.
    pub views: BTreeMap<String, wgpu::TextureView>,
}

#[derive(Debug, Clone, Error)]
pub enum MeshError {
    #[error("{0}")]
    Pack(String),
    #[error("{0}")]
    Pipeline(String),
    #[error("the pack has no effect named {0}")]
    NoEffect(String),
    #[error("{0} has no pass this draws")]
    NoPass(String),
    #[error("{0}")]
    Device(String),
}

/// What a render of surfaces drew.
#[derive(Debug, Clone)]
pub struct MeshRender {
    pub rendered: Rendered,
    /// Surfaces drawn.
    pub drawn: usize,
    /// Surfaces left out, with the effect of each and why.
    pub skipped: Vec<(String, MeshError)>,
}

/// A viewpoint looking at a point, with an up direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    /// A camera that sees the box from `min` to `max` whole, from the front and a little above and to the side, with Z up as the game's models have it.
    pub fn framing(min: Vec3, max: Vec3) -> Self {
        let centre = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(1e-3);
        let fov_y = 40f32.to_radians();
        let distance = radius / (fov_y * 0.5).sin() * 1.1;
        let direction = Vec3::new(0.6, -1.0, 0.45).normalize();
        Self {
            eye: centre + direction * distance,
            target: centre,
            up: Vec3::Z,
            fov_y,
            near: (distance - radius).max(distance * 0.01) * 0.5,
            far: distance + radius * 2.0,
        }
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye, self.target, self.up)
    }

    pub fn projection(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, 1.0, self.near, self.far)
    }

    /// The direction towards a light that shines over the camera's shoulder: from the viewer's side, above and to the right, so what the camera sees is lit.
    pub fn light_direction(&self) -> Vec3 {
        light_towards(self.eye, self.target, self.up)
    }
}

/// The direction towards a light over the shoulder of a viewer at `eye` looking at `target` with `up` above: back towards the viewer, raised and turned to the right.
pub fn light_towards(eye: Vec3, target: Vec3, up: Vec3) -> Vec3 {
    let back = (eye - target).normalize_or_zero();
    let right = up.cross(back).normalize_or_zero();
    (back + up.normalize_or_zero() * 0.7 + right * 0.4).normalize_or_zero()
}

/// The techniques tried for a surface, in order: for a cutout the ones that clip, then the forward ones that light themselves, then the plain, and last, for an effect that has no forward one, the deferred ones, whose first colour is the albedo.
fn techniques(skinned: bool, coverage: Coverage) -> Vec<&'static str> {
    let (cutout, forward, deferred): (&[&str], &[&str], &[&str]) = if skinned {
        (
            &[
                "lightweight0CutOut_drawskinned",
                "lightweight0CutOutTint_drawskinned",
            ],
            &[
                "lightweight0_drawskinned",
                "drawskinned",
                "unlit_drawskinned",
            ],
            &["deferred_drawskinned", "deferredalphaclip_drawskinned"],
        )
    } else {
        (
            &["lightweight0CutOut_draw", "lightweight0CutOutTint_draw"],
            &["lightweight0_draw", "draw", "unlit_draw"],
            &["deferred_draw", "deferredalphaclip_draw"],
        )
    };
    let clipping = if coverage == Coverage::Cutout {
        cutout
    } else {
        &[]
    };
    // A cutout that falls back to the deferred passes takes the one that clips first
    let mut last: Vec<&'static str> = deferred.to_vec();
    if coverage == Coverage::Cutout {
        last.reverse();
    }
    clipping
        .iter()
        .chain(forward)
        .copied()
        .chain(last)
        .collect()
}

/// Which of the game's passes a scene draws its surfaces with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    /// Every surface lit by its own pass, in one go.
    #[default]
    Forward,
    /// What is opaque, with the passes that write the G-buffer, for the lighting passes to read.
    GBuffer,
    /// What blends, with the forward passes, over a frame the lighting passes have lit.
    Blended,
    /// What is opaque, as the depth the sun's shadow is cast by: the game's depth-only passes, whatever each material's own effect.
    Shadow,
    /// The colour of the water in front of what is opaque, by the water effects' own fog pass, into the lit frame before a copy of it is taken for the water to read.
    WaterFog,
    /// The dome of the sky, by the sky effect's own pass, over the pixels nothing was drawn at.
    Sky,
    /// The disc of the sun, by the sky effect's own pass, blended over the sky by how much of it shows.
    Sun,
    /// The disc of the moon, by the sky effect's own pass, added to the sky by how much of it shows.
    Moon,
}

impl Route {
    /// Whether a surface of this coverage is drawn by this route.
    pub fn takes(self, coverage: Coverage) -> bool {
        match self {
            Route::Forward => true,
            Route::GBuffer => coverage != Coverage::Blended,
            Route::Blended | Route::WaterFog => coverage == Coverage::Blended,
            Route::Shadow => coverage != Coverage::Blended,
            Route::Sky | Route::Sun | Route::Moon => true,
        }
    }
}

/// The techniques that write the G-buffer, the one that clips first for a cutout.
fn gbuffer_techniques(skinned: bool, coverage: Coverage) -> Vec<&'static str> {
    let mut list = if skinned {
        vec!["deferred_drawskinned", "deferredalphaclip_drawskinned"]
    } else {
        vec!["deferred_draw", "deferredalphaclip_draw"]
    };
    if coverage == Coverage::Cutout {
        list.reverse();
    }
    list
}

/// The effect whose depth-only passes cast the shadows of every surface, and the technique of each kind of vertex.
pub const SHADOW_EFFECT: &str = "cascadeshadows";

fn shadow_techniques(skinned: bool) -> Vec<&'static str> {
    vec![if skinned {
        "unlit_drawskinned"
    } else {
        "unlit_draw"
    }]
}

/// The first pass of the first of [`techniques`] that `effect` has, when it is a vertex stage and at most a pixel stage.
pub fn select_pass(effect: &Effect, skinned: bool, coverage: Coverage) -> Option<&Pass> {
    select_pass_for(effect, Route::Forward, skinned, coverage)
}

/// [`select_pass`] for the techniques `route` draws with.
pub fn select_pass_for(
    effect: &Effect,
    route: Route,
    skinned: bool,
    coverage: Coverage,
) -> Option<&Pass> {
    select_pass_at(effect, route, skinned, coverage, 0)
}

/// [`select_pass_for`] taking pass number `at` of the technique, which a technique with a pass for each combination of what it draws has more than one of.
pub fn select_pass_at(
    effect: &Effect,
    route: Route,
    skinned: bool,
    coverage: Coverage,
    at: usize,
) -> Option<&Pass> {
    let mut wanted = match route {
        Route::GBuffer => gbuffer_techniques(skinned, coverage),
        Route::Forward | Route::Blended => techniques(skinned, coverage),
        Route::Shadow => shadow_techniques(skinned),
        Route::WaterFog => vec!["imposter_draw"],
        Route::Sky => vec!["sky"],
        Route::Sun => vec!["sun"],
        Route::Moon => vec!["moon"],
    };
    // Water names the passes of its quality levels after their number: the drawing one with the environment first
    if effect.name.to_ascii_lowercase().starts_with("water")
        && matches!(route, Route::Forward | Route::Blended)
    {
        wanted.extend([
            "alt4_draw",
            "alt6_draw",
            "alt7_draw",
            "alt5_draw",
            "alt0_draw",
        ]);
    }
    // What blends over a lit frame is lit by its own pass, never written to the G-buffer
    if route == Route::Blended {
        wanted.retain(|name| !name.starts_with("deferred"));
    }
    wanted.iter().find_map(|wanted| {
        let technique = effect.techniques.iter().find(|t| &t.name == wanted)?;
        let pass = technique.passes.get(at)?;
        pair_of(pass).map(|_| pass)
    })
}

/// One value of a vertex input, for the vertex `at` of a surface.
fn input_value(
    surface: &Surface,
    element: &SignatureElement,
    at: usize,
    palette: &[u16],
) -> [f32; 4] {
    let first = |v: Option<&[f32; 3]>, w: f32| v.map_or([0.0; 4], |v| [v[0], v[1], v[2], w]);
    match (element.semantic.as_str(), element.index) {
        ("POSITION", 0) => first(surface.positions.get(at), 1.0),
        ("NORMAL", 0) => surface
            .normals
            .get(at)
            .map_or([0.0, 0.0, 1.0, 0.0], |n| [n[0], n[1], n[2], 0.0]),
        ("TANGENT", 0) => surface
            .tangents
            .get(at)
            .copied()
            .unwrap_or([1.0, 0.0, 0.0, 1.0]),
        ("TEXCOORD", n) => surface
            .uvs
            .get(&n)
            .and_then(|uvs| uvs.get(at))
            .map_or([0.0; 4], |uv| [uv[0], uv[1], 0.0, 0.0]),
        ("COLOR", n) => surface
            .colours
            .get(&n)
            .and_then(|colours| colours.get(at))
            .copied()
            .unwrap_or([1.0; 4]),
        ("BLENDWEIGHT", 0) => surface.weights.get(at).map_or([1.0, 0.0, 0.0, 0.0], |w| *w),
        // The slot of each bone in the table the pose is written to, which holds the bones of the whole scene
        ("BLENDINDICES", 0) => surface.joints.get(at).map_or([0.0; 4], |joints| {
            joints.map(|bone| palette.binary_search(&bone).unwrap_or(0) as f32)
        }),
        _ => [0.0; 4],
    }
}

/// The vertices of the surface's triangles, `stride` bytes each, one for each index, with each attribute written from the input at its location.
pub fn surface_vertices(
    surface: &Surface,
    attributes: &[wgpu::VertexAttribute],
    stride: u64,
    inputs: &[SignatureElement],
    palette: &[u16],
) -> Vec<u8> {
    use rayon::prelude::*;
    let stride = stride as usize;
    // What an attribute writes is settled by the shader and the format, not by the vertex: found once, and each vertex only fills it in
    let plans: Vec<_> = attributes
        .iter()
        .map(|attribute| {
            (
                inputs
                    .iter()
                    .find(|i| i.register == attribute.shader_location),
                attribute.offset as usize,
                (attribute.format.size() / 4) as usize,
                // ⚠ A bone slot a shader declares as a float is a normalized byte, which it multiplies by 255 to find the bone: written as the slot itself it names a bone slot far off
                inputs.iter().any(|input| {
                    input.register == attribute.shader_location && input.semantic == "BLENDINDICES"
                }) && attribute.format == wgpu::VertexFormat::Float32x4,
                matches!(
                    attribute.format,
                    wgpu::VertexFormat::Uint32
                        | wgpu::VertexFormat::Uint32x2
                        | wgpu::VertexFormat::Uint32x3
                        | wgpu::VertexFormat::Uint32x4
                ),
            )
        })
        .collect();
    let mut bytes = vec![0u8; surface.indices.len() * stride];
    if stride == 0 {
        return bytes;
    }
    let fill = |(written, index): (&mut [u8], &u32)| {
        let at = *index as usize;
        for (element, offset, words, by_the_byte, integer) in &plans {
            let values = element.map_or([0.0; 4], |element| {
                input_value(surface, element, at, palette)
            });
            let values = if *by_the_byte {
                values.map(|slot| slot / 255.0)
            } else {
                values
            };
            for (i, value) in values.iter().take(*words).enumerate() {
                let place = offset + 4 * i;
                // An input the shader declares as an unsigned integer is read as the bits of one
                let bits = if *integer {
                    (*value as u32).to_le_bytes()
                } else {
                    value.to_le_bytes()
                };
                written[place..place + 4].copy_from_slice(&bits);
            }
        }
    };
    // ⚠ Each vertex is its own index's, so the vertices are independent: a model of tens of thousands of triangles is written by every core
    bytes
        .par_chunks_mut(stride)
        .zip(surface.indices.par_iter())
        .for_each(fill);
    bytes
}

/// How many bones the table of bone matrices a skinned model's buffer holds has room for.
pub const BONES: usize = 255;

/// The bones of the skeleton the skinned surfaces are weighted to, once each and in order, which is the order of the table of matrices the pose is written to; a bone past the room the table has is left out, and what is weighted to it follows the first.
pub fn bone_palette(surfaces: &[(Surface, Material)]) -> Vec<u16> {
    let mut bones: Vec<u16> = surfaces
        .iter()
        .filter(|(surface, _)| surface.skinned)
        .flat_map(|(surface, _)| {
            surface
                .joints
                .iter()
                .zip(&surface.weights)
                .flat_map(|(joints, weights)| {
                    joints
                        .iter()
                        .zip(weights)
                        .filter(|(_, weight)| **weight > 0.0)
                        .map(|(bone, _)| *bone)
                })
        })
        .collect();
    bones.sort_unstable();
    bones.dedup();
    bones.truncate(BONES);
    bones
}

/// Whether an effect is one a surface is seen through: the glass of a window or a vehicle, and the blur of a rotor, which show what is behind them and not a surface of their own.
fn sees_through(effect: &str) -> bool {
    let name = effect.to_ascii_lowercase();
    name.contains("glass") || name.contains("blurredrotor")
}

/// Whether a sampler reads a palette, whose rows are the colours a surface is tinted by and which the game samples by point, never filtered.
fn is_palette(sampler: &str) -> bool {
    let name = sampler.to_ascii_lowercase();
    name.ends_with("pal") || name.contains("palette")
}

/// A neutral grey for a texture a material does not bind.
fn grey() -> Provided {
    Provided::Texture2d {
        width: 1,
        height: 1,
        rgba: vec![200, 200, 200, 255],
        srgb: false,
    }
}

/// What a texture a material does not bind is replaced by: a flat normal for a normal map, whose grey would tilt every surface the same way and light it all one colour, and a neutral grey for the rest.
fn missing(sampler: &str) -> Provided {
    if sampler.to_ascii_lowercase().contains("bump") {
        return Provided::Texture2d {
            width: 1,
            height: 1,
            rgba: vec![128, 128, 255, 255],
            srgb: false,
        };
    }
    grey()
}

/// The bounds of every position of every surface.
pub fn bounds(surfaces: &[(Surface, Material)]) -> Option<(Vec3, Vec3)> {
    let mut found: Option<(Vec3, Vec3)> = None;
    for (surface, _) in surfaces {
        for p in &surface.positions {
            let p = Vec3::from(*p);
            found = Some(match found {
                None => (p, p),
                Some((lo, hi)) => (lo.min(p), hi.max(p)),
            });
        }
    }
    found
}

/// Draw each surface with the pass its material's effect names, into a fresh frame, and read the frame back.
///
/// A surface whose effect the pack lacks, or that has no pass this draws, or that will not build, is left out and listed; one the device refuses at the draw fails the render.
pub fn render_surfaces(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    surfaces: &[(Surface, Material)],
    camera: &Camera,
    size: u32,
) -> Result<MeshRender, MeshError> {
    let engine = EngineConstants {
        view: camera.view(),
        projection: camera.projection(),
        sun_direction: camera.light_direction(),
        ..EngineConstants::default()
    };
    let frame = Frame::new(device, size);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let scene = MeshScene::prepare(device, queue, pack, &Target::default(), surfaces);
    scene.update(queue, &engine);
    frame.render(device, queue, |pass| scene.record(pass));
    let rendered = frame.read_back(device, queue);
    let validation = pollster::block_on(validation.pop());
    let internal = pollster::block_on(internal.pop());
    match validation.or(internal) {
        None => Ok(MeshRender {
            rendered,
            drawn: scene.drawn(),
            skipped: scene.skipped().to_vec(),
        }),
        Some(error) => Err(MeshError::Device(error.to_string().replace('\n', " | "))),
    }
}

#[cfg(test)]
mod tests;
