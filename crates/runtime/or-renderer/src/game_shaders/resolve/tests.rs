use glam::Mat4;

use super::super::deferred::{ldr_target, DepthViews, GBuffer, ReflectionTarget, HDR_FORMAT};
use super::super::test_pack::{gpu, real_pack};
use super::super::tone::{Tone, ToneSettings};
use super::*;
use crate::game_shaders::lighting::Lighting;

const SIZE: u32 = 16;
const SCREEN: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// The alpha of the screen after the pass maps a frame of mid grey light onto it, which was cleared to an opaque black.
fn alpha_after(device: &wgpu::Device, queue: &wgpu::Queue, pack: &GamePack, to_screen: bool) -> u8 {
    let depth = texture(
        device,
        DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let depth_view = depth.create_view(&Default::default());
    let views = DepthViews::of(&depth);
    let gbuffer = GBuffer::new(device, SIZE, SIZE);
    let reflection = ReflectionTarget::new(device, 4, 1);
    let hdr = texture(
        device,
        HDR_FORMAT,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let grey: Vec<u8> = (0..SIZE * SIZE)
        .flat_map(|_| [0.01f32, 0.01, 0.01, 1.0].map(|v| or_io::half::f32_to_half(v).to_le_bytes()))
        .flatten()
        .collect();
    queue.write_texture(
        hdr.as_image_copy(),
        &grey,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SIZE * 8),
            rows_per_image: Some(SIZE),
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    let hdr_view = hdr.create_view(&Default::default());
    let ldr = ldr_target(device, SCREEN, SIZE, SIZE);
    let inputs = Inputs {
        gbuffer: &gbuffer,
        depth: &views,
        hdr: &hdr_view,
        reflection: reflection.view(),
        ldr: &ldr,
    };
    let pass =
        ResolvePass::new(device, queue, pack, SCREEN, to_screen, &inputs).expect("the pass builds");
    let engine = EngineConstants {
        projection: Mat4::IDENTITY,
        ..EngineConstants::default()
    };
    let screen = Screen {
        width: SIZE,
        height: SIZE,
        near: 0.1,
        far: 50.0,
        reflection_levels: 1,
    };
    let tone = Tone::new(&ToneSettings::default(), None, &Lighting::default());
    pass.write(queue, &engine, &screen, &tone);
    let out = texture(
        device,
        SCREEN,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let out_view = out.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    // Everything is drawn: the stencil is cleared to the material of a surface, not to nothing
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("stencil"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0),
                store: wgpu::StoreOp::Store,
            }),
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    {
        let mut draw = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &out_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: None,
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.record(&mut draw);
    }
    queue.submit([encoder.finish()]);
    let read = super::super::frame::read_back(device, queue, &out, SCREEN, SIZE);
    let middle = (SIZE / 2 * SIZE + SIZE / 2) as usize;
    assert!(read.bytes[middle * 4] > 0, "the pass drew over the middle");
    read.bytes[middle * 4 + 3]
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_pass_writes_the_luminance_in_alpha_for_the_anti_aliasing_and_leaves_the_alpha_of_the_screen_when_it_draws_to_it(
) {
    let test = "the_pass_writes_the_luminance_in_alpha";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let between = alpha_after(&device, &queue, &pack, false);
    let screen = alpha_after(&device, &queue, &pack, true);
    assert!(between > 0 && between < 255, "luminance: {between}");
    assert_eq!(screen, 255);
}
