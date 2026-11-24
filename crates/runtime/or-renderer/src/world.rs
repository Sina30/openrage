//! The world a viewport draws: meshes on the device, each drawn once for every instance that places it, in the bucket of its material.

use std::ops::Range;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::game_shaders::deferred::Sun;
use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::mesh::scene::MATERIAL_DEFAULT;
use crate::game_shaders::shadow::TILE;
use crate::game_shaders::stand_in::constant_view;
use crate::viewport::{MeshVertex, OrbitCamera, ViewportRenderer};

mod pipelines;
mod shader;
pub(crate) mod shadow;

use pipelines::{ByBucket, Pipelines};
pub use shadow::SHADOW_FAR;
use shadow::{Cascades, CASCADES};

/// How and when a mesh of the world is drawn; declared in the order the buckets draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorldBucket {
    /// Every texel, into the G-buffer of the game's frame.
    #[default]
    Opaque,
    /// As opaque, but for the texels whose coverage is under a half.
    Cutout,
    /// As opaque, and giving light of its own colour.
    Emissive,
    /// Blended by its coverage onto the surface it lies on, which it is held in front of.
    Decal,
    /// Blended by its coverage over the lit frame.
    Blend,
}

impl WorldBucket {
    pub(crate) const COUNT: usize = 5;

    /// Whether it casts the sun's shadow: what blends lets the sun through, and a decal lies on what casts already.
    fn casts(self) -> bool {
        matches!(self, Self::Opaque | Self::Cutout | Self::Emissive)
    }
}

/// What a mesh of the world is drawn with: its maps on the device and its bucket; a clone shares the maps.
#[derive(Clone)]
pub struct WorldMaterial {
    maps: Arc<wgpu::BindGroup>,
    bucket: WorldBucket,
}

impl PartialEq for WorldMaterial {
    fn eq(&self, other: &Self) -> bool {
        self.bucket == other.bucket && Arc::ptr_eq(&self.maps, &other.maps)
    }
}

impl Eq for WorldMaterial {}

impl WorldMaterial {
    pub fn bucket(&self) -> WorldBucket {
        self.bucket
    }

    /// A number two materials have in common exactly when they draw with the same maps, to sort and group draws by.
    pub fn key(&self) -> usize {
        Arc::as_ptr(&self.maps) as usize
    }
}

/// One mesh of the world on the device: the vertices and indices it draws, and the material it draws them with.
pub struct WorldMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    base_vertex: i32,
    first_index: u32,
    index_count: u32,
    material: WorldMaterial,
}

impl WorldMesh {
    /// Upload a mesh of its own buffers, drawn with `material`.
    pub fn new(
        device: &wgpu::Device,
        vertices: &[MeshVertex],
        indices: &[u32],
        material: WorldMaterial,
    ) -> Self {
        let buffer = |label, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        Self::shared(
            buffer(
                "world-mesh-vertices",
                bytemuck::cast_slice(vertices),
                wgpu::BufferUsages::VERTEX,
            ),
            buffer(
                "world-mesh-indices",
                bytemuck::cast_slice(indices),
                wgpu::BufferUsages::INDEX,
            ),
            0,
            0,
            indices.len() as u32,
            material,
        )
    }

    /// A mesh that is a range of buffers it does not own: `index_count` indices from `first_index`, each added to `base_vertex`.
    pub fn shared(
        vertices: wgpu::Buffer,
        indices: wgpu::Buffer,
        base_vertex: i32,
        first_index: u32,
        index_count: u32,
        material: WorldMaterial,
    ) -> Self {
        Self {
            vertices,
            indices,
            base_vertex,
            first_index,
            index_count,
            material,
        }
    }

    /// The same mesh drawn with `material`, sharing its buffers.
    pub fn with_material(&self, material: WorldMaterial) -> Self {
        Self {
            vertices: self.vertices.clone(),
            indices: self.indices.clone(),
            material,
            ..*self
        }
    }

    pub fn material(&self) -> &WorldMaterial {
        &self.material
    }

    /// The bytes of the buffers it draws from, which a shared mesh does not own.
    pub fn bytes(&self) -> u64 {
        self.vertices.size() + self.indices.size()
    }
}

/// Where one instance of a mesh stands: the rows of its 3×4 model matrix, and a tint whose colour its surface is multiplied by and whose alpha is how much of it its fade leaves.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct WorldInstance {
    pub rows: [[f32; 4]; 3],
    pub tint: [f32; 4],
}

impl WorldInstance {
    /// The instance `matrix` places, its surface multiplied by `tint`.
    pub fn of(matrix: glam::Mat4, tint: [f32; 4]) -> Self {
        Self {
            rows: [0, 1, 2].map(|row| matrix.row(row).to_array()),
            tint,
        }
    }
}

/// A run of the frame's instances drawn with one mesh.
pub struct WorldDraw {
    pub mesh: Arc<WorldMesh>,
    pub instances: Range<u32>,
}

/// The floats of the viewport's camera uniform, two matrices and three vectors: a cascade is seen through a uniform as wide, of which the cast reads the first matrix.
const CAMERA_FLOATS: usize = 44;

/// The bytes one draw's arguments take in the buffer the draws of a frame are issued from.
const ARGS: wgpu::BufferAddress =
    std::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>() as wgpu::BufferAddress;

/// What a viewport keeps of the world: the pipelines that draw it, what its materials share, the instances of this frame and the runs of them each mesh draws.
pub(crate) struct World {
    pipelines: Pipelines,
    materials: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// What a material samples where it names no map: white, a normal that tilts nothing, and no shine.
    stand_ins: [wgpu::TextureView; 3],
    /// The material of no map, which every such material shares.
    plain: Arc<wgpu::BindGroup>,
    /// The sun and the ambient light of the game's frame, which what blends over it is lit by.
    light: wgpu::Buffer,
    light_group: wgpu::BindGroup,
    /// The view of each cascade of the sun's shadow, bound where a pass binds its camera.
    cascades: [(wgpu::Buffer, wgpu::BindGroup); CASCADES],
    /// One buffer for every frame, made again only when a frame has more instances than it holds.
    instances: Option<wgpu::Buffer>,
    count: u32,
    /// The draws of this frame that draw something, in the order of their buckets.
    draws: Vec<WorldDraw>,
    /// The runs of `draws` one call issues, for a pass that samples the maps and for the cast of the shadow.
    batches: Vec<Range<u32>>,
    cast_batches: Vec<Range<u32>>,
    /// The arguments of each draw, where the device issues several draws by one call.
    indirect: Option<wgpu::Buffer>,
    multi: bool,
}

/// The maps of a material as the pipelines bind them: colour, normal, specular, and the one sampler.
fn bound(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    maps: [&wgpu::TextureView; 3],
) -> wgpu::BindGroup {
    let map = |binding: u32| wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(maps[binding as usize]),
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viewport-world-material-bg"),
        layout,
        entries: &[
            map(0),
            map(1),
            map(2),
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Write `bytes` to the buffer in `slot`, which is made again where there is none or it holds fewer.
fn fill(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    slot: &mut Option<wgpu::Buffer>,
    label: &'static str,
    usage: wgpu::BufferUsages,
    bytes: &[u8],
) {
    let needed = bytes.len() as wgpu::BufferAddress;
    if slot.as_ref().is_none_or(|buffer| buffer.size() < needed) {
        *slot = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: needed.next_power_of_two().max(4096),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    if let Some(buffer) = slot {
        queue.write_buffer(buffer, 0, bytes);
    }
}

/// The runs of `draws`, which are in the order of their buckets, that one call can issue: draws side by side of one bucket, the same two buffers and the same material. For the cast of the shadow (`cast`) only what casts is taken, and what casts whole needs no map, so any material joins.
fn batches(draws: &[WorldDraw], cast: bool) -> Vec<Range<u32>> {
    let mut batches: Vec<Range<u32>> = Vec::new();
    let mut before: Option<&WorldMesh> = None;
    for (at, draw) in draws.iter().enumerate() {
        let (at, mesh) = (at as u32, &*draw.mesh);
        let bucket = mesh.material.bucket;
        if cast && !bucket.casts() {
            continue;
        }
        let joins = before.is_some_and(|before| {
            before.material.bucket == bucket
                && before.vertices == mesh.vertices
                && before.indices == mesh.indices
                && ((cast && bucket != WorldBucket::Cutout) || before.material == mesh.material)
        });
        match batches.last_mut().filter(|last| joins && last.end == at) {
            Some(last) => last.end = at + 1,
            None => batches.push(at..at + 1),
        }
        before = Some(mesh);
    }
    batches
}

impl World {
    /// The pipelines and what the materials share, for a viewport whose colour target is of `colour_format`, with the layout of its camera.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_bgl: &wgpu::BindGroupLayout,
        colour_format: wgpu::TextureFormat,
    ) -> Self {
        let map = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let materials = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport-world-material-bgl"),
            entries: &[
                map(0),
                map(1),
                map(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("viewport-world-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        // Half floats, so that the normal's half is a half exactly and tilts nothing
        let stand_ins = [[1.0; 4], [0.5, 0.5, 1.0, 1.0], [0.0; 4]]
            .map(|texel| constant_view(device, queue, texel));
        let plain = Arc::new(bound(device, &materials, &sampler, stand_ins.each_ref()));

        let uniform = |label, floats: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (floats * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let group = |label, layout: &wgpu::BindGroupLayout, buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport-world-light-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let light = uniform("viewport-world-light", 16);
        let light_group = group("viewport-world-light-bg", &light_layout, &light);
        let cascades = std::array::from_fn(|_| {
            let view = uniform("viewport-world-cascade", CAMERA_FLOATS);
            let seen = group("viewport-world-cascade-bg", camera_bgl, &view);
            (view, seen)
        });
        Self {
            pipelines: Pipelines::new(device, camera_bgl, &materials, &light_layout, colour_format),
            materials,
            sampler,
            stand_ins,
            plain,
            light,
            light_group,
            cascades,
            instances: None,
            count: 0,
            draws: Vec::new(),
            batches: Vec::new(),
            cast_batches: Vec::new(),
            indirect: None,
            // An indirect draw may start at an instance other than the first only where the device says so
            multi: device
                .features()
                .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE),
        }
    }

    fn material(
        &self,
        device: &wgpu::Device,
        maps: [Option<&wgpu::TextureView>; 3],
        bucket: WorldBucket,
    ) -> WorldMaterial {
        let maps = match maps {
            [None, None, None] => Arc::clone(&self.plain),
            maps => {
                let views = std::array::from_fn(|at| maps[at].unwrap_or(&self.stand_ins[at]));
                Arc::new(bound(device, &self.materials, &self.sampler, views))
            }
        };
        WorldMaterial { maps, bucket }
    }

    fn set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[WorldInstance],
        mut draws: Vec<WorldDraw>,
    ) {
        self.count = instances.len() as u32;
        // A run past the instances of this frame would read what an earlier frame left in the buffer
        for draw in &mut draws {
            draw.instances.end = draw.instances.end.min(self.count);
        }
        draws.retain(|draw| !draw.instances.is_empty() && draw.mesh.index_count > 0);
        // The sort keeps the order the caller gave the draws of a bucket
        draws.sort_by_key(|draw| draw.mesh.material.bucket);
        self.batches = batches(&draws, false);
        self.cast_batches = batches(&draws, true);
        if !draws.is_empty() {
            fill(
                device,
                queue,
                &mut self.instances,
                "viewport-world-instances",
                wgpu::BufferUsages::VERTEX,
                bytemuck::cast_slice(instances),
            );
        }
        if self.multi && !draws.is_empty() {
            let mut arguments = Vec::with_capacity(draws.len() * ARGS as usize);
            for draw in &draws {
                let call = wgpu::util::DrawIndexedIndirectArgs {
                    index_count: draw.mesh.index_count,
                    instance_count: draw.instances.end - draw.instances.start,
                    first_index: draw.mesh.first_index,
                    base_vertex: draw.mesh.base_vertex,
                    first_instance: draw.instances.start,
                };
                arguments.extend_from_slice(call.as_bytes());
            }
            fill(
                device,
                queue,
                &mut self.indirect,
                "viewport-world-draws",
                wgpu::BufferUsages::INDIRECT,
                &arguments,
            );
        }
        self.draws = draws;
    }

    fn draws_anything(&self) -> bool {
        !self.draws.is_empty()
    }

    /// Issue `batches` into `pass`, each with the pipeline `pipelines` holds for its bucket: a bucket it holds none for is left out, and a buffer or a material that is bound already is not bound again.
    fn record(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batches: &[Range<u32>],
        pipelines: &ByBucket,
    ) {
        let mut set: Option<(&wgpu::RenderPipeline, &WorldMesh)> = None;
        for batch in batches {
            let run = &self.draws[batch.start as usize..batch.end as usize];
            let Some(mesh) = run.first().map(|draw| &*draw.mesh) else {
                continue;
            };
            let Some(pipeline) = &pipelines[mesh.material.bucket as usize] else {
                continue;
            };
            if !set.is_some_and(|(last, _)| std::ptr::eq(last, pipeline)) {
                pass.set_pipeline(pipeline);
            }
            if !set.is_some_and(|(_, last)| last.vertices == mesh.vertices) {
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            }
            if !set.is_some_and(|(_, last)| last.indices == mesh.indices) {
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            }
            let maps = &mesh.material.maps;
            if !set.is_some_and(|(_, last)| Arc::ptr_eq(&last.material.maps, maps)) {
                pass.set_bind_group(1, &**maps, &[]);
            }
            set = Some((pipeline, mesh));
            match &self.indirect {
                Some(arguments) => pass.multi_draw_indexed_indirect(
                    arguments,
                    wgpu::BufferAddress::from(batch.start) * ARGS,
                    batch.end - batch.start,
                ),
                None => {
                    for draw in run {
                        let mesh = &draw.mesh;
                        pass.draw_indexed(
                            mesh.first_index..mesh.first_index + mesh.index_count,
                            mesh.base_vertex,
                            draw.instances.clone(),
                        );
                    }
                }
            }
        }
    }

    /// Draw the buckets `pipelines` holds into `pass`, seen through `camera`.
    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &wgpu::BindGroup,
        pipelines: &ByBucket,
    ) {
        let Some(instances) = self.instances.as_ref().filter(|_| self.draws_anything()) else {
            return;
        };
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(2, &self.light_group, &[]);
        pass.set_vertex_buffer(1, instances.slice(..));
        self.record(pass, &self.batches, pipelines);
    }

    /// Draw every bucket of the world lit as the plain modes light a surface, into a pass over the viewport's colour and depth.
    pub(crate) fn draw_forward(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup) {
        self.draw(pass, camera, &self.pipelines.forward);
    }

    /// Draw what is opaque, then the decals over it, into a pass that fills the G-buffer for the game's lighting passes to light.
    pub(crate) fn draw_gbuffer(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup) {
        pass.set_stencil_reference(MATERIAL_DEFAULT);
        self.draw(pass, camera, &self.pipelines.gbuffer);
    }

    /// Draw what blends into a pass over the frame the game's passes lit, which reads the depth of the frame and writes none.
    pub(crate) fn draw_lit(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup) {
        self.draw(pass, camera, &self.pipelines.lit);
    }

    /// Write the light of the game's frame, which [`Self::draw_lit`] lights what blends by.
    pub(crate) fn write_light(&self, queue: &wgpu::Queue, engine: &EngineConstants) {
        let sun = Sun::of(engine);
        let [down, up] = match &engine.lighting {
            Some(lighting) => lighting.natural_ambient.map(|ambient| ambient.truncate()),
            None => [engine.ambient; 2],
        };
        let light = [sun.travel.normalize_or_zero(), sun.colour, down, up]
            .map(|vector| vector.extend(0.0).to_array());
        queue.write_buffer(&self.light, 0, bytemuck::cast_slice(&light));
    }

    /// The cascades the sun's shadow of this frame's world is cast into, the view of each written for the cast; none where nothing of the world casts or the sun does not shine.
    pub(crate) fn cascades(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        camera: &OrbitCamera,
        aspect: f32,
    ) -> Option<Cascades> {
        if self.cast_batches.is_empty() {
            return None;
        }
        let cascades = Cascades::about(engine, camera, aspect)?;
        for (view, (buffer, _)) in cascades.views().iter().zip(&self.cascades) {
            let mut uniform = [0.0f32; CAMERA_FLOATS];
            uniform[..16].copy_from_slice(&view.to_cols_array());
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&uniform));
        }
        Some(cascades)
    }

    /// Cast the world into the map of the sun's shadow, each cascade into its tile: `pass` is the one the game's map is cast in, and [`Self::cascades`] wrote the views this frame.
    pub(crate) fn draw_shadow(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(instances) = self.instances.as_ref().filter(|_| self.draws_anything()) else {
            return;
        };
        pass.set_vertex_buffer(1, instances.slice(..));
        for (tile, (_, view)) in self.cascades.iter().enumerate() {
            // The tiles are one above the other, the nearest cascade at the top
            let top = tile as u32 * TILE;
            pass.set_viewport(0.0, top as f32, TILE as f32, TILE as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, top, TILE, TILE);
            pass.set_bind_group(0, view, &[]);
            self.record(pass, &self.cast_batches, &self.pipelines.cast);
        }
    }
}

impl ViewportRenderer {
    /// A material of `bucket` that samples these maps; a map left out is white, the vertex normal, and matte.
    pub fn world_material(
        &self,
        device: &wgpu::Device,
        colour: Option<&wgpu::TextureView>,
        normal: Option<&wgpu::TextureView>,
        specular: Option<&wgpu::TextureView>,
        bucket: WorldBucket,
    ) -> WorldMaterial {
        self.world
            .material(device, [colour, normal, specular], bucket)
    }

    /// The world this frame draws: every instance, and the runs of them each mesh draws, in any order; an empty list draws none.
    pub fn set_world(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[WorldInstance],
        draws: Vec<WorldDraw>,
    ) {
        self.world.set(device, queue, instances, draws);
    }

    /// How many instances the world draws.
    pub fn world_instances(&self) -> u32 {
        self.world.count
    }

    /// Whether there is a world to draw.
    pub fn has_world(&self) -> bool {
        self.world.draws_anything()
    }
}

#[cfg(test)]
mod tests;
