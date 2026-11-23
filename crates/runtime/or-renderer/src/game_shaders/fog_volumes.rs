//! The fog the particles of an effect fill the air with: each volume a sphere of the game's fog volume pass, drawn over the frame the light is summed in.

use std::collections::BTreeMap;
use std::sync::Mutex;

use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

use super::deferred::{lighting_values, provide, Inputs, Screen, DEPTH_STENCIL_FORMAT};
use super::engine::{columns, EngineConstants};
use super::light_volumes::{corner_value, record_bytes, sphere};
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::screen::{vertex_inputs, ScreenError, ScreenPass, Space};

const EFFECT: &str = "vfx_fogvolume";
/// The technique for a sphere and the one for an ellipsoid, each with a pass for a camera outside the volume and one for a camera inside it.
const TECHNIQUES: [&str; 2] = ["fogvolume_default", "fogvolume_ellipsoidal"];
const VARIANTS: usize = 4;

/// How many volumes are drawn at once: each has constants of its own, which a frame writes before any is drawn.
pub const MOST: usize = 16;

/// How the density a rule states becomes the thinning per unit of distance the pass reads, as a power of ten.
const DENSITY_SCALE: f32 = -2.5;

/// How far past its range the pass takes a camera to be inside a volume, as a share of the range: the faceted mesh reaches beyond the sphere it stands for.
const INSIDE_MARGIN: f32 = 0.058 + 2.0;

/// What brightens the colour of a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FogLight {
    /// The fog of the day's brightness.
    #[default]
    DayFog,
    /// The sun's light, by how far overhead it stands.
    Sun,
    None,
}

/// A sphere of fog, scaled and turned into an ellipsoid where `scale` is not one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogVolume {
    pub position: Vec3,
    pub scale: Vec3,
    /// Euler angles about X, Y and Z, in radians.
    pub rotation: Vec3,
    /// The radius before the scale.
    pub range: f32,
    pub density: f32,
    /// How fast the fog thins towards the edge.
    pub falloff: f32,
    pub tint: Vec3,
    pub hdr_mult: f32,
    pub light: FogLight,
    /// Whether the tint tints the colour of the ground fog of the day.
    pub ground_tint: bool,
}

/// What the day gives the colour of a volume.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FogSky {
    /// The colour of the sun's light, scaled by its strength.
    pub sun: Vec3,
    /// How far overhead the sun stands, from nought to one.
    pub sun_height: f32,
    pub day_fog: f32,
    pub ground: Vec3,
}

impl FogVolume {
    /// The colour the pass is given: the tint by the volume's brightness, by what lights it and by the ground fog if it is asked for.
    pub fn colour(&self, sky: &FogSky) -> Vec3 {
        let mut colour = self.tint * self.hdr_mult;
        match self.light {
            FogLight::Sun => colour *= sky.sun * sky.sun_height.clamp(0.0, 1.0),
            FogLight::DayFog => colour *= sky.day_fog,
            FogLight::None => {}
        }
        if self.ground_tint {
            colour *= sky.ground;
        }
        colour
    }

    /// The scale turned by the rotation, which takes a point of the unit sphere to the volume.
    pub fn transform(&self) -> Mat3 {
        let turn = Quat::from_euler(
            EulerRot::ZYX,
            self.rotation.z,
            self.rotation.y,
            self.rotation.x,
        );
        Mat3::from_quat(turn) * Mat3::from_diagonal(self.scale)
    }

    /// Whether a camera at `eye` is taken to be inside the volume.
    pub fn contains(&self, eye: Vec3) -> bool {
        let reach = self.range * (1.0 + INSIDE_MARGIN);
        eye.distance_squared(self.position) < reach * reach
    }

    /// Whether the volume is a sphere.
    pub fn is_sphere(&self) -> bool {
        self.scale == Vec3::ONE
    }

    /// The pass that draws it, by the technique and then by whether the camera is inside.
    fn variant(&self, eye: Vec3) -> usize {
        2 * usize::from(!self.is_sphere()) + usize::from(self.contains(eye))
    }

    /// The values of the variables of the pass that are the volume's own.
    fn values(&self, sky: &FogSky) -> [(&'static str, Vec<f32>); 5] {
        let transform = self.transform();
        // The pass multiplies a point as a row, so the registers hold the rows of the matrix that takes the point
        let registers = |m: Mat3| columns(Mat4::from_mat3(m).transpose());
        let colour = self.colour(sky);
        [
            ("fogVolumeColor", colour.to_array().to_vec()),
            ("fogVolumePosition", self.position.to_array().to_vec()),
            (
                "fogVolumeParams",
                vec![
                    self.density * 10f32.powf(DENSITY_SCALE),
                    self.falloff,
                    self.range,
                    1.0,
                ],
            ),
            ("fogVolumeTransform", registers(transform)),
            ("fogVolumeInvTransform", registers(transform.inverse())),
        ]
    }
}

/// The mesh a variant of the pass draws.
struct Variant {
    sphere: wgpu::Buffer,
    corners: u32,
}

/// The fog of the particles, drawn over the lit frame.
pub struct FogVolumes {
    /// For each volume the frame has room for, one pass of each variant with constants of its own.
    slots: Vec<Vec<ScreenPass>>,
    variants: Vec<Variant>,
    /// The slot and the variant of each volume the frame was written with.
    drawn: Mutex<Vec<(usize, usize)>>,
}

impl FogVolumes {
    /// The passes, drawn into colour targets of `colour`, over what `frame` holds.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        colour: wgpu::TextureFormat,
        frame: &Inputs<'_>,
    ) -> Result<Self, ScreenError> {
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let mut first = Vec::with_capacity(VARIANTS);
        let mut variants = Vec::with_capacity(VARIANTS);
        for variant in 0..VARIANTS {
            let (technique, inside) = (TECHNIQUES[variant / 2], variant % 2);
            let no_pass = || ScreenError::NoPass {
                effect: EFFECT.into(),
                technique: technique.into(),
                pass: inside,
            };
            let inputs = vertex_inputs(pack, EFFECT, technique, inside).ok_or_else(no_pass)?;
            // From outside only the near side of the volume is drawn, where it is not behind a surface; from inside the far side, wherever it is
            let target = Target {
                colour,
                depth: DEPTH_STENCIL_FORMAT,
                depth_compare: if inside == 1 {
                    wgpu::CompareFunction::Always
                } else {
                    wgpu::CompareFunction::GreaterEqual
                },
                blend: Some(blend),
                colour_attachments: Some(1),
            };
            let draw = DrawState {
                cull_mode: Some(if inside == 1 {
                    wgpu::Face::Front
                } else {
                    wgpu::Face::Back
                }),
                depth_attachment: true,
                depth_tested: inside == 0,
                write_mask: Some(wgpu::ColorWrites::COLOR),
                unfiltered: true,
                ..DrawState::default()
            };
            let pass = ScreenPass::new(
                device,
                queue,
                pack,
                EFFECT,
                technique,
                inside,
                &target,
                &draw,
                Space::Unit,
                &mut |name, requirement| provide(device, frame, name, requirement),
            )?;
            let streams = pass.streams();
            let corners = sphere();
            let mut bytes = Vec::with_capacity(corners.len() * streams.vertex_stride as usize);
            for corner in &corners {
                bytes.extend(record_bytes(
                    &streams.vertex,
                    streams.vertex_stride,
                    &inputs,
                    &|input| corner_value(&input.semantic, *corner),
                ));
            }
            variants.push(Variant {
                sphere: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("fog-volume-sphere"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                corners: corners.len() as u32,
            });
            first.push(pass);
        }
        let mut slots = vec![first];
        for _ in 1..MOST {
            let other = slots[0]
                .iter()
                .map(|pass| {
                    pass.forked(device, queue, &mut |name, requirement| {
                        provide(device, frame, name, requirement)
                    })
                })
                .collect();
            slots.push(other);
        }
        Ok(Self {
            slots,
            variants,
            drawn: Mutex::new(Vec::new()),
        })
    }

    /// Read `frame` anew, for targets of another size.
    pub fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Inputs<'_>) {
        for pass in self.slots.iter_mut().flatten() {
            pass.bind(device, queue, &mut |name, requirement| {
                provide(device, frame, name, requirement)
            });
        }
    }

    /// Write the constants of this frame and of the first [`MOST`] of `volumes`.
    pub fn write(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        screen: &Screen,
        sky: &FogSky,
        volumes: &[FogVolume],
    ) {
        let mut drawn = self.drawn.lock().unwrap_or_else(|e| e.into_inner());
        drawn.clear();
        let volumes = &volumes[..volumes.len().min(MOST)];
        if volumes.is_empty() {
            return;
        }
        let mut shared = engine.values();
        // The pass reads no bone, and each volume's constants are written from a copy of these
        shared.remove("gBoneMtx");
        shared.extend(lighting_values(engine, screen));
        let eye = engine.view.inverse().w_axis.truncate();
        for (slot, volume) in volumes.iter().enumerate() {
            let variant = volume.variant(eye);
            let mut values: BTreeMap<&'static str, Vec<f32>> = shared.clone();
            values.extend(volume.values(sky));
            self.slots[slot][variant].write(queue, &values);
            drawn.push((slot, variant));
        }
    }

    /// Draw the volumes the frame was written with into `pass`: its colour target of the format this was built for, and the depth of the frame, which it only reads.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        let drawn = self.drawn.lock().unwrap_or_else(|e| e.into_inner());
        for &(slot, variant) in drawn.iter() {
            let mesh = &self.variants[variant];
            self.slots[slot][variant].record_mesh(pass, &mesh.sphere, mesh.corners);
        }
    }
}

#[cfg(test)]
mod tests;
