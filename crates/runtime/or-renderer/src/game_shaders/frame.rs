//! A colour target and a depth target that several draws share, and the pixels read back from it.

use std::collections::BTreeMap;

use wgpu::util::DeviceExt as _;

use super::pipeline::PassPipeline;

/// What a pass wrote to its first colour target.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// The width and the height, in pixels.
    pub size: u32,
    pub format: wgpu::TextureFormat,
    /// The pixels row by row, with no padding between rows.
    pub bytes: Vec<u8>,
}

impl Rendered {
    fn pixel_bytes(&self) -> usize {
        self.format.block_copy_size(None).unwrap_or(4) as usize
    }

    /// How many pixels hold anything but zero, since the target was cleared to transparent black.
    pub fn touched(&self) -> usize {
        self.bytes
            .chunks_exact(self.pixel_bytes())
            .filter(|pixel| pixel.iter().any(|b| *b != 0))
            .count()
    }

    /// How many different pixels it holds.
    pub fn colours(&self) -> usize {
        self.bytes
            .chunks_exact(self.pixel_bytes())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }
}

/// The colour target of a frame, in the format the game's own passes write most.
pub const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A square colour target and depth target that a number of passes draw into one after another.
pub struct Frame {
    size: u32,
    colour: wgpu::Texture,
    depth: wgpu::Texture,
}

impl Frame {
    pub fn new(device: &wgpu::Device, size: u32) -> Self {
        let target = |format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
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
                usage,
                view_formats: &[],
            })
        };
        Self {
            size,
            colour: target(
                COLOUR_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            ),
            depth: target(
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// Clear the colour to transparent black and the depth to the far plane.
    pub fn clear(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let colour = self.colour.create_view(&Default::default());
        let depth = self.depth.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
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
        queue.submit([encoder.finish()]);
    }

    /// Clear the colour to transparent black and the depth to the far plane, then let `record` draw into one pass over both, and submit it.
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        record: impl FnOnce(&mut wgpu::RenderPass<'_>),
    ) {
        let colour = self.colour.create_view(&Default::default());
        let depth = self.depth.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &colour,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
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
            record(&mut pass);
        }
        queue.submit([encoder.finish()]);
    }

    /// Draw `count` vertices of `vertices`, `pass.vertex_stride` bytes each, with `pass` and `groups`, on top of what the frame holds.
    ///
    /// The frame takes the pass's first colour target when that is its own format, and the depth target when the pass has one; a target the frame has no room for is a scratch one the draw throws away.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &PassPipeline,
        groups: &BTreeMap<u32, wgpu::BindGroup>,
        vertices: &[u8],
        count: u32,
    ) {
        let scratch: Vec<Option<wgpu::TextureView>> = pass
            .colour_formats
            .iter()
            .enumerate()
            .map(|(location, format)| {
                format.and_then(|format| {
                    let mine = location == 0 && format == COLOUR_FORMAT;
                    let texture = (!mine).then(|| {
                        device.create_texture(&wgpu::TextureDescriptor {
                            label: None,
                            size: wgpu::Extent3d {
                                width: self.size,
                                height: self.size,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            view_formats: &[],
                        })
                    });
                    Some(match texture {
                        Some(texture) => texture.create_view(&Default::default()),
                        None => self.colour.create_view(&Default::default()),
                    })
                })
            })
            .collect();
        let attachments: Vec<Option<wgpu::RenderPassColorAttachment<'_>>> = scratch
            .iter()
            .map(|view| {
                view.as_ref().map(|view| wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })
            })
            .collect();
        let depth = pass
            .depth
            .then(|| self.depth.create_view(&Default::default()));
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: vertices,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &attachments,
                depth_stencil_attachment: depth.as_ref().map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
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
            for (group, bind_group) in groups {
                render.set_bind_group(*group, bind_group, &[]);
            }
            render.draw(0..count, 0..1);
        }
        queue.submit([encoder.finish()]);
    }

    /// The colour pixels, row by row.
    pub fn read_back(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Rendered {
        read_back(device, queue, &self.colour, COLOUR_FORMAT, self.size)
    }
}

/// The pixels of `texture`, a `size`-square target of `format`, row by row.
pub fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
    size: u32,
) -> Rendered {
    let pixel = format.block_copy_size(None).unwrap_or(4);
    let row = (size * pixel).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * size),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size),
            },
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("the buffer maps")
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let mapped = slice.get_mapped_range();
    let mut bytes = Vec::with_capacity((size * size * pixel) as usize);
    for y in 0..size as usize {
        let start = y * row as usize;
        bytes.extend_from_slice(&mapped[start..start + (size * pixel) as usize]);
    }
    drop(mapped);
    Rendered {
        size,
        format,
        bytes,
    }
}

#[cfg(test)]
mod tests;
