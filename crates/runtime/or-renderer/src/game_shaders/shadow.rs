//! The sun's shadow on the drawable: its surfaces drawn from the sun as depth into one tile of the game's cascade map, and the game's own pass that reads that map and the depth of the frame and multiplies what it finds into the self-shadow of the G-buffer.

use std::collections::BTreeMap;

use glam::{Mat4, Vec3, Vec4};

use super::bindings::{Requirement, Resource};
use super::deferred::{
    lighting_values, over_what_was_drawn, Inputs, Screen, Sun, DEPTH_STENCIL_FORMAT, GBUFFER_FORMAT,
};
use super::engine::EngineConstants;
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::samplers::{comparing, point};
use super::screen::{ScreenError, ScreenPass, Space};
use super::stand_in::Provided;
use super::targets::render_target;

const EFFECT: &str = "cascadeshadows_rendering";
const TECHNIQUE: &str = "ShadowRenderNoSoft";
/// The pass of the technique that filters the map by a box of 4 by 4 texels.
const BOX4X4: usize = 4;

/// The format of the map the shadow is cast into.
pub const MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// How many texels a side of the tile the shadow is drawn into has, and how many tiles the map the passes read is made of: the game stacks its cascades one above the other, and only the first is drawn here.
pub const TILE: u32 = 1024;
const TILES: u32 = 4;

/// How far below the surface that casts it, in the depth of the tile, the shadow is held, so that a surface does not shadow itself.
const BIAS: f32 = 0.0015;

/// How the shadow of a sphere of the world is cast by the sun: the axes of the shadow space, and where the sphere is in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    centre: Vec3,
    radius: f32,
    /// The axes of the shadow space: across the tile, down it, and the way the light travels.
    across: Vec3,
    down: Vec3,
    along: Vec3,
}

impl Fit {
    /// The fit for the sphere of `radius` about `centre`, lit by light that travels `travel`.
    pub fn of(travel: Vec3, centre: Vec3, radius: f32) -> Self {
        let along = travel.normalize_or(-Vec3::Z);
        let hint = if along.z.abs() > 0.99 {
            Vec3::X
        } else {
            Vec3::Z
        };
        let across = along.cross(hint).normalize();
        let down = along.cross(across).normalize();
        Self {
            centre,
            radius: radius.max(1e-3),
            across,
            down,
            along,
        }
    }

    /// The constants the surfaces are drawn with into the tile: the world to the shadow space about the sphere, and that space to the tile, which is across it from left to right, down it from the top, and in depth from the near side of the sphere to the far.
    pub fn caster(&self) -> EngineConstants {
        let rows = [self.across, self.down, self.along];
        let view = Mat4::from_cols(
            Vec4::new(rows[0].x, rows[1].x, rows[2].x, 0.0),
            Vec4::new(rows[0].y, rows[1].y, rows[2].y, 0.0),
            Vec4::new(rows[0].z, rows[1].z, rows[2].z, 0.0),
            Vec4::new(
                -rows[0].dot(self.centre),
                -rows[1].dot(self.centre),
                -rows[2].dot(self.centre),
                1.0,
            ),
        );
        let r = self.radius;
        let projection = Mat4::from_cols(
            Vec4::new(1.0 / r, 0.0, 0.0, 0.0),
            Vec4::new(0.0, -1.0 / r, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 0.5 / r, 0.0),
            Vec4::new(0.0, 0.0, 0.5, 1.0),
        );
        EngineConstants {
            world: Mat4::IDENTITY,
            view,
            projection,
            ..EngineConstants::default()
        }
    }

    #[cfg(test)]
    /// Where the point `p` of the world is in the tile the surfaces were drawn into, as the pass that receives the shadow works it out for a pixel `eye` away: across and down from the middle of the tile, in a unit, and its depth in the tile.
    pub fn lookup(&self, p: Vec3, eye: Vec3) -> Vec3 {
        let (a, b) = self.scale_and_offset(eye);
        let s = Vec3::new(
            (p - eye).dot(self.across),
            (p - eye).dot(self.down),
            (p - eye).dot(self.along),
        );
        s * a + b
    }

    /// What the receiving pass multiplies a position by and adds to it, from the eye, to reach the tile: a unit across it, and in depth from nought to one.
    fn scale_and_offset(&self, eye: Vec3) -> (Vec3, Vec3) {
        let k = 0.5 / self.radius;
        let at = Vec3::new(
            (self.centre - eye).dot(self.across),
            (self.centre - eye).dot(self.down),
            (self.centre - eye).dot(self.along),
        );
        (
            Vec3::splat(k),
            Vec3::new(-at.x * k, -at.y * k, -(at.z - self.radius) * k - BIAS),
        )
    }

    /// The values of the variables the receiving pass reads of the shadow, by the names the game gives them, for a viewer at `eye`.
    pub fn values(&self, eye: Vec3) -> BTreeMap<&'static str, Vec<f32>> {
        let (scale, offset) = self.scale_and_offset(eye);
        let mut shared = Vec::with_capacity(48);
        // The three rows of the world to shadow rotation, each with a word of how the shadow fades: not at all
        shared.extend([
            self.across.x,
            self.down.x,
            self.along.x,
            0.0,
            self.across.y,
            self.down.y,
            self.along.y,
            1.0,
            self.across.z,
            self.down.z,
            self.along.z,
            0.0,
            // The fog's shadow
            0.0,
            0.0,
            0.0,
            0.0,
        ]);
        // Each of the four cascades is the one tile: the pass takes the first the pixel lies in
        for _ in 0..4 {
            shared.extend([scale.x, scale.y, scale.z, 0.0]);
        }
        for _ in 0..4 {
            shared.extend([offset.x, offset.y, offset.z, 0.0]);
        }
        let tile = TILE as f32;
        BTreeMap::from([
            ("gCSMShaderVars_shared", shared),
            ("gCSMDepthBias", vec![0.0; 4]),
            ("gCSMDepthSlopeBias", vec![0.0; 4]),
            ("gCSMResolution", vec![tile, tile, 1.0 / tile, 1.0 / tile]),
            ("gCSMShaderVars_deferred", vec![0.0; 4]),
            ("particleShadowsParams", vec![0.0; 4]),
            ("perspectiveShearParam", vec![0.0; 4]),
            ("shadowParams2", vec![1.0, 0.0, 0.0, 0.0]),
        ])
    }
}

/// The sphere that holds the box from `min` to `max`.
pub fn sphere_of(min: Vec3, max: Vec3) -> (Vec3, f32) {
    ((min + max) * 0.5, (max - min).length() * 0.5)
}

/// The map the shadow is cast into: the tiles of the cascades one above the other.
pub fn map_target(device: &wgpu::Device) -> wgpu::TextureView {
    render_target(
        device,
        "sun-shadow",
        (TILE, TILE * TILES),
        MAP_FORMAT,
        wgpu::TextureUsages::empty(),
    )
}

/// What the pass binds under `name`: the depth of the frame and the map of the shadow with the samplers that read them, or nothing.
fn provide(
    device: &wgpu::Device,
    inputs: &Inputs<'_>,
    map: &wgpu::TextureView,
    name: &str,
    requirement: &Requirement,
) -> Option<Provided> {
    match (&requirement.resource, name) {
        (Resource::Texture { .. }, "depthBufferSamp") => {
            Some(Provided::View(inputs.depth.depth.clone()))
        }
        (Resource::Sampler { .. }, "depthBufferSamp") => Some(Provided::Sampler(point(device))),
        (Resource::Texture { .. }, "gCSMShadowTexture") => Some(Provided::View(map.clone())),
        (Resource::Sampler { .. }, "gCSMShadowTextureSamp") => {
            Some(Provided::Sampler(comparing(device)))
        }
        // The layout of the pass takes no sampler that filters, whatever else it reads
        (Resource::Sampler { comparison: false }, _) => Some(Provided::Sampler(point(device))),
        _ => None,
    }
}

/// The game's pass that finds, for each pixel the G-buffer holds, whether the sun reaches it, and the map the surfaces are cast into.
pub struct ShadowPass {
    render: ScreenPass,
    map: wgpu::TextureView,
}

impl ShadowPass {
    /// The pass, reading the depth `inputs` hold and writing the self-shadow of the G-buffer.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        inputs: &Inputs<'_>,
    ) -> Result<Self, ScreenError> {
        let map = map_target(device);
        let target = Target {
            colour: GBUFFER_FORMAT,
            depth: DEPTH_STENCIL_FORMAT,
            depth_compare: wgpu::CompareFunction::Always,
            // Only the alpha is written, as what was there times what the shadow lets through
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent::REPLACE,
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::SrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            colour_attachments: Some(1),
        };
        let draw = DrawState {
            write_mask: Some(wgpu::ColorWrites::ALPHA),
            // The depth of the frame is a depth texture, which only an unfiltered binding takes
            unfiltered: true,
            ..over_what_was_drawn()
        };
        let render = ScreenPass::new(
            device,
            queue,
            pack,
            EFFECT,
            TECHNIQUE,
            BOX4X4,
            &target,
            &draw,
            Space::Clip,
            &mut |name, requirement| provide(device, inputs, &map, name, requirement),
        )?;
        Ok(Self { render, map })
    }

    /// Read `inputs` anew, for targets of another size.
    pub fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, inputs: &Inputs<'_>) {
        let map = &self.map;
        self.render.bind(device, queue, &mut |name, requirement| {
            provide(device, inputs, map, name, requirement)
        });
    }

    /// The texture of the map, for the bytes it holds.
    pub fn texture(&self) -> &wgpu::Texture {
        self.map.texture()
    }

    /// The fit of the shadow of `sphere` for the sun of `engine`, where the sun shines.
    pub fn fit(engine: &EngineConstants, sphere: (Vec3, f32)) -> Option<Fit> {
        let sun = Sun::of(engine);
        sun.shines()
            .then(|| Fit::of(sun.travel, sphere.0, sphere.1))
    }

    /// Write the constants of this frame.
    pub fn write(&self, queue: &wgpu::Queue, engine: &EngineConstants, screen: &Screen, fit: &Fit) {
        let eye = engine.view.inverse().w_axis.truncate();
        self.write_with(queue, engine, screen, fit.values(eye));
    }

    /// Write the constants of this frame, the shadow's own being `shadow`: what [`Fit::values`] gives for the viewer of `engine`, or the same variables for several cascades.
    pub fn write_with(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        screen: &Screen,
        shadow: BTreeMap<&'static str, Vec<f32>>,
    ) {
        let camera = engine.view.inverse();
        let eye = camera.w_axis.truncate();
        let mut values = engine.values();
        let lighting = lighting_values(engine, screen);
        let projection = &lighting["deferredProjectionParams"];
        // The axes of the camera, with the way a depth is read in the fourth of each, and where it stands
        let rows = [
            camera.x_axis.truncate(),
            camera.y_axis.truncate(),
            camera.z_axis.truncate(),
            eye,
        ];
        let mut view_to_world = Vec::with_capacity(16);
        for (row, w) in rows.iter().zip(projection) {
            view_to_world.extend([row.x, row.y, row.z, *w]);
        }
        values.extend(lighting);
        values.extend(shadow);
        values.insert("viewToWorldProjectionParam", view_to_world);
        self.render.write(queue, &values);
    }

    /// Draw the surfaces of the scene, from the sun, into the first tile of the map: `cast` draws them into the pass it is given.
    pub fn record_cast(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        cast: impl FnOnce(&mut wgpu::RenderPass<'_>),
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sun-shadow-cast"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.map,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(0.0, 0.0, TILE as f32, TILE as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, TILE, TILE);
        cast(&mut pass);
    }

    /// Multiply the self-shadow of the G-buffer by what the sun lets through, over what was drawn: `scales` is the target that holds it, and `depth` the depth and stencil of the frame, which are only read.
    pub fn record_receive(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        shadows: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sun-shadow-receive"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: shadows,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: None,
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_stencil_reference(super::mesh::scene::MATERIAL_CLEAR);
        self.render.record(&mut pass);
    }
}

#[cfg(test)]
mod tests;
