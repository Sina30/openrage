//! Stand-in resources for a pass's bind groups, and a draw of one triangle with them: what shows a pass can be drawn before anything real feeds it.

use std::collections::BTreeMap;

use wgpu::util::DeviceExt as _;

use super::bindings::{Requirement, Resource};
use super::mips;
use super::pipeline::PassPipeline;

/// The width and height of the targets a stand-in draw writes.
pub const TARGET_SIZE: u32 = 4;

/// A view of a two-dimensional texture made of `rgba`, with the levels below its first: each is the average of four of the one above, so a texture seen small shows its average.
pub fn uploaded_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    rgba: &[u8],
    srgb: bool,
) -> wgpu::TextureView {
    let levels = mips::chain(width, height, rgba, srgb);
    let mut texels = rgba.to_vec();
    for (_, _, level) in &levels {
        texels.extend_from_slice(level);
    }
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1 + levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::default(),
        &texels,
    );
    texture.create_view(&Default::default())
}

/// A view of one texel of four half floats, for an input of a pass that nothing drew.
pub fn constant_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texel: [f32; 4],
) -> wgpu::TextureView {
    row_view(device, queue, &[texel])
}

/// A view of a row of texels of four half floats, one texel high, for an input that is a curve.
pub fn row_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texels: &[[f32; 4]],
) -> wgpu::TextureView {
    let bytes: Vec<u8> = texels
        .iter()
        .flatten()
        .flat_map(|v| or_io::f32_to_half(*v).to_le_bytes())
        .collect();
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: texels.len().max(1) as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::default(),
        &bytes,
    );
    texture.create_view(&Default::default())
}

/// A uniform buffer of the size a module needs, as a pass binds it.
pub fn uniform_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    buffer(
        device,
        size,
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    )
}

/// Zeroed storage, or whatever the device clears a new buffer to, of at least `size` bytes.
fn buffer(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: size.max(16).next_multiple_of(16),
        usage,
        mapped_at_creation: false,
    })
}

/// The format a stand-in texture of a sample type has.
fn format_of(sample: wgpu::TextureSampleType) -> wgpu::TextureFormat {
    match sample {
        wgpu::TextureSampleType::Float { .. } => wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureSampleType::Sint => wgpu::TextureFormat::Rgba8Sint,
        wgpu::TextureSampleType::Uint => wgpu::TextureFormat::Rgba8Uint,
        wgpu::TextureSampleType::Depth => wgpu::TextureFormat::Depth32Float,
    }
}

/// A one-texel texture of `format` and a view of it that a binding of `dimension` takes.
fn texture_view(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    dimension: wgpu::TextureViewDimension,
    multisampled: bool,
    storage: bool,
) -> wgpu::TextureView {
    use wgpu::TextureViewDimension as V;
    let (texture_dimension, layers) = match dimension {
        V::D1 => (wgpu::TextureDimension::D1, 1),
        V::D2 => (wgpu::TextureDimension::D2, 1),
        V::D2Array => (wgpu::TextureDimension::D2, 2),
        V::Cube => (wgpu::TextureDimension::D2, 6),
        V::CubeArray => (wgpu::TextureDimension::D2, 6),
        V::D3 => (wgpu::TextureDimension::D3, 1),
    };
    let usage = if storage {
        wgpu::TextureUsages::STORAGE_BINDING
    } else if multisampled {
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT
    } else {
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST
    };
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: if multisampled { 4 } else { 1 },
            dimension: texture_dimension,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(dimension),
            ..Default::default()
        })
}

/// Real contents for a resource a pass asks for, in place of a stand-in.
pub enum Provided {
    /// The bytes of a uniform or storage buffer, which are padded with zeros to what the module needs.
    Bytes(Vec<u8>),
    /// The texels of a two-dimensional texture, as 8-bit RGBA, in the sRGB colour space when `srgb` says so, which a shader then reads as linear.
    Texture2d {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        srgb: bool,
    },
    /// A texture another pass drew, bound as it is.
    View(wgpu::TextureView),
    /// A sampler other than the game's own, such as the point sampling a pass reads a G-buffer by.
    Sampler(wgpu::Sampler),
    /// A uniform or storage buffer made elsewhere and bound as it is, which its owner writes.
    Buffer(wgpu::Buffer),
}

/// A one-texel depth texture holding the far value, so that a comparison of any depth the game takes against it passes: a pass that reads a shadow map or the scene's depth from a stand-in sees nothing in the way, where a texture holding nought would shadow everything.
fn far_depth_view(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    queue.submit([encoder.finish()]);
    view
}

/// The sampler of the game's own textures: they repeat across a surface, filter linearly between levels as well as texels, and keep detail along a slant, which a comparison against a depth does not need.
pub fn sampler_descriptor(comparison: bool) -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: if comparison { 1 } else { 8 },
        compare: comparison.then_some(wgpu::CompareFunction::Less),
        ..Default::default()
    }
}

/// The sampler an effect declares for one of its textures, from the pairs of state and value its file holds (address, the filters, how much detail is kept along a slant), over the game's default for a texture: what the file does not state stays as the default.
///
/// ⚠ The states are the engine's own numbers: `0 1 2` the address in `u`, `v` and `w` (`1` repeat, `2` mirror, `3` clamp), `4 5 6` the filter between levels, of a texture that shrinks and of one that grows (`1` point, `2` linear, `3` anisotropic), and `9` how many times the detail is kept along a slant. A clamp to a border and a bias of the level are not a sampler's here and are left out.
pub fn declared_sampler(declared: &[u32]) -> wgpu::SamplerDescriptor<'static> {
    let address = |value: u32| match value {
        2 => wgpu::AddressMode::MirrorRepeat,
        3..=5 => wgpu::AddressMode::ClampToEdge,
        _ => wgpu::AddressMode::Repeat,
    };
    let point = |value: u32| value <= 1;
    let mut sampler = sampler_descriptor(false);
    let mut slant = 1;
    let mut anisotropic = false;
    for pair in declared.as_chunks::<2>().0 {
        let (state, value) = (pair[0], pair[1]);
        match state {
            0 => sampler.address_mode_u = address(value),
            1 => sampler.address_mode_v = address(value),
            2 => sampler.address_mode_w = address(value),
            4 => {
                sampler.mipmap_filter = if point(value) {
                    wgpu::MipmapFilterMode::Nearest
                } else {
                    wgpu::MipmapFilterMode::Linear
                }
            }
            5 | 6 => {
                let filter = if point(value) {
                    wgpu::FilterMode::Nearest
                } else {
                    wgpu::FilterMode::Linear
                };
                if state == 5 {
                    sampler.min_filter = filter;
                    anisotropic = value == 3;
                } else {
                    sampler.mag_filter = filter;
                }
            }
            9 => slant = value.clamp(1, 16),
            _ => {}
        }
    }
    // The device keeps detail along a slant only where every filter is linear
    let linear = sampler.mag_filter == wgpu::FilterMode::Linear
        && sampler.min_filter == wgpu::FilterMode::Linear
        && sampler.mipmap_filter == wgpu::MipmapFilterMode::Linear;
    sampler.anisotropy_clamp = if linear && anisotropic {
        slant.max(2) as u16
    } else {
        1
    };
    sampler
}

/// A bind group for each group of `pass`, holding a stand-in for every resource it asks for.
pub fn bind_groups(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &PassPipeline,
) -> BTreeMap<u32, wgpu::BindGroup> {
    bind_groups_with(device, queue, pass, &mut |_| None)
}

/// A bind group for each group of `pass`, holding what `provide` gives for each resource it asks for, and a stand-in for the rest.
///
/// What `provide` returns that does not suit the resource, such as texels for a buffer, is ignored.
pub fn bind_groups_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &PassPipeline,
    provide: &mut dyn FnMut(&Requirement) -> Option<Provided>,
) -> BTreeMap<u32, wgpu::BindGroup> {
    bind_resources_with(device, queue, pass, provide).groups
}

/// The bind groups of a pass, and the buffers in them, which a later frame writes new contents to.
pub struct Bound {
    pub groups: BTreeMap<u32, wgpu::BindGroup>,
    /// The uniform and storage buffers, by group and binding.
    pub buffers: BTreeMap<(u32, u32), wgpu::Buffer>,
}

/// [`bind_groups_with`], keeping the buffers it made.
pub fn bind_resources_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &PassPipeline,
    provide: &mut dyn FnMut(&Requirement) -> Option<Provided>,
) -> Bound {
    let mut groups = BTreeMap::new();
    let mut kept = BTreeMap::new();
    for (group, layout) in &pass.layouts {
        // Each resource has to outlive the entries that name it
        let mut buffers = Vec::new();
        let mut views = Vec::new();
        let mut samplers = Vec::new();
        let mut order = Vec::new();
        for requirement in &pass.requirements[group] {
            let provided = provide(requirement);
            match &requirement.resource {
                Resource::Uniform { size } | Resource::Storage { size, .. } => {
                    let usage = if matches!(requirement.resource, Resource::Uniform { .. }) {
                        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST
                    } else {
                        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST
                    };
                    let made = match &provided {
                        Some(Provided::Buffer(shared)) => shared.clone(),
                        _ => buffer(device, *size, usage),
                    };
                    if let Some(Provided::Bytes(bytes)) = &provided {
                        // A write is a whole number of words, and no longer than the buffer
                        let room = made.size() as usize;
                        let len = bytes.len().min(room) & !3;
                        queue.write_buffer(&made, 0, &bytes[..len]);
                    }
                    kept.insert((*group, requirement.binding), made.clone());
                    buffers.push(made);
                    order.push((requirement.binding, 0, buffers.len() - 1));
                }
                Resource::Texture {
                    dimension,
                    sample,
                    multisampled,
                } => {
                    let plain_2d = *dimension == wgpu::TextureViewDimension::D2
                        && matches!(sample, wgpu::TextureSampleType::Float { .. })
                        && !multisampled;
                    match &provided {
                        Some(Provided::View(view)) => views.push(view.clone()),
                        Some(Provided::Texture2d {
                            width,
                            height,
                            rgba,
                            srgb,
                        }) if plain_2d => {
                            views.push(uploaded_view(device, queue, *width, *height, rgba, *srgb));
                        }
                        _ if *sample == wgpu::TextureSampleType::Depth
                            && *dimension == wgpu::TextureViewDimension::D2
                            && !multisampled =>
                        {
                            views.push(far_depth_view(device, queue));
                        }
                        _ => views.push(texture_view(
                            device,
                            format_of(*sample),
                            *dimension,
                            *multisampled,
                            false,
                        )),
                    }
                    order.push((requirement.binding, 1, views.len() - 1));
                }
                Resource::StorageTexture {
                    dimension, format, ..
                } => {
                    views.push(texture_view(device, *format, *dimension, false, true));
                    order.push((requirement.binding, 1, views.len() - 1));
                }
                Resource::Sampler { comparison } => {
                    samplers.push(match provided {
                        Some(Provided::Sampler(sampler)) => sampler,
                        _ => device.create_sampler(&sampler_descriptor(*comparison)),
                    });
                    order.push((requirement.binding, 2, samplers.len() - 1));
                }
            }
        }
        let entries: Vec<wgpu::BindGroupEntry<'_>> = order
            .iter()
            .map(|(binding, kind, index)| wgpu::BindGroupEntry {
                binding: *binding,
                resource: match kind {
                    0 => buffers[*index].as_entire_binding(),
                    1 => wgpu::BindingResource::TextureView(&views[*index]),
                    _ => wgpu::BindingResource::Sampler(&samplers[*index]),
                },
            })
            .collect();
        groups.insert(
            *group,
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            }),
        );
    }
    Bound {
        groups,
        buffers: kept,
    }
}

/// Draw one triangle with `pass` and the bind groups `groups`, into fresh targets, and return the colour targets by location.
///
/// The vertex buffer is zeroed, so a vertex stage that reads attributes draws a degenerate triangle; the draw is what is checked, since the device validates every binding at it.
pub fn draw_triangle(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &PassPipeline,
    groups: &BTreeMap<u32, wgpu::BindGroup>,
) -> Vec<Option<wgpu::Texture>> {
    let zeros = vec![0u8; (pass.vertex_stride * 3) as usize];
    draw_vertices(device, queue, pass, groups, &zeros, 3, TARGET_SIZE)
}

/// Draw `count` vertices read from `vertices`, which are `pass.vertex_stride` bytes each, with the bind groups `groups`, into fresh `size`-square targets cleared to transparent black, and return the colour targets by location.
pub fn draw_vertices(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &PassPipeline,
    groups: &BTreeMap<u32, wgpu::BindGroup>,
    vertices: &[u8],
    count: u32,
    size: u32,
) -> Vec<Option<wgpu::Texture>> {
    draw_instances(device, queue, pass, groups, vertices, &[], count, size)
}

/// [`draw_vertices`] once for each `pass.instance_stride` bytes of `instances`, which the pipeline reads from its second vertex buffer.
pub fn draw_instances(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &PassPipeline,
    groups: &BTreeMap<u32, wgpu::BindGroup>,
    vertices: &[u8],
    instances: &[u8],
    count: u32,
    size: u32,
) -> Vec<Option<wgpu::Texture>> {
    let target = |format: wgpu::TextureFormat| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    };
    let colours: Vec<Option<wgpu::Texture>> = pass
        .colour_formats
        .iter()
        .map(|format| format.map(target))
        .collect();
    let views: Vec<Option<wgpu::TextureView>> = colours
        .iter()
        .map(|t| t.as_ref().map(|t| t.create_view(&Default::default())))
        .collect();
    let attachments: Vec<Option<wgpu::RenderPassColorAttachment<'_>>> = views
        .iter()
        .map(|view| {
            view.as_ref().map(|view| wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })
        })
        .collect();
    let depth_view = pass
        .depth
        .then(|| target(wgpu::TextureFormat::Depth32Float).create_view(&Default::default()));
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: vertices,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let instance_buffer = (pass.instance_stride > 0).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: instances,
            usage: wgpu::BufferUsages::VERTEX,
        })
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &attachments,
            depth_stencil_attachment: depth_view.as_ref().map(|view| {
                wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render.set_pipeline(&pass.pipeline);
        render.set_vertex_buffer(0, vertex_buffer.slice(..));
        let mut instance_count = 1;
        if let Some(buffer) = &instance_buffer {
            render.set_vertex_buffer(1, buffer.slice(..));
            instance_count = (instances.len() as u64 / pass.instance_stride) as u32;
        }
        for (group, bind_group) in groups {
            render.set_bind_group(*group, bind_group, &[]);
        }
        render.draw(0..count, 0..instance_count);
    }
    queue.submit([encoder.finish()]);
    colours
}

#[cfg(test)]
mod tests;
