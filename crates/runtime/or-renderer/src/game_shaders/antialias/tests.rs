use glam::Mat4;

use super::super::deferred::{hdr_target, DepthViews, GBuffer, ReflectionTarget};
use super::super::test_pack::{gpu, real_pack};
use super::*;

const SIZE: u32 = 64;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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

/// A frame of white above the diagonal and black below it, with the luminance of each pixel in its alpha as the game's mapping leaves it.
fn diagonal() -> Vec<u8> {
    let mut texels = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            let level = if x > y { 255 } else { 0 };
            texels.extend([level; 4]);
        }
    }
    texels
}

/// The pixels the pass draws from `frame`, row by row.
fn antialiased(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    frame: &[u8],
) -> Result<Vec<u8>, String> {
    let source = texture(
        device,
        FORMAT,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    queue.write_texture(
        source.as_image_copy(),
        frame,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SIZE * 4),
            rows_per_image: Some(SIZE),
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    let source_view = source.create_view(&Default::default());
    let depth = texture(
        device,
        DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let gbuffer = GBuffer::new(device, SIZE, SIZE);
    let views = DepthViews::of(&depth);
    let hdr = hdr_target(device, SIZE, SIZE);
    let reflection = ReflectionTarget::new(device, 4, 1);

    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pass = AntiAliasPass::new(
        device,
        queue,
        pack,
        FORMAT,
        &Inputs {
            gbuffer: &gbuffer,
            depth: &views,
            hdr: &hdr,
            reflection: reflection.view(),
            ldr: &source_view,
        },
    )
    .map_err(|e| e.to_string())?;
    pass.write(
        queue,
        &EngineConstants {
            projection: Mat4::IDENTITY,
            ..EngineConstants::default()
        },
        &Screen {
            width: SIZE,
            height: SIZE,
            near: 0.1,
            far: 50.0,
            reflection_levels: 1,
        },
    );
    let out = texture(
        device,
        FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let out_view = out.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut draw = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("antialias"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &out_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.record(&mut draw);
    }
    queue.submit([encoder.finish()]);
    let error = pollster::block_on(validation.pop()).or(pollster::block_on(internal.pop()));
    if let Some(error) = error {
        return Err(error.to_string().replace('\n', " | "));
    }
    Ok(super::super::frame::read_back(device, queue, &out, FORMAT, SIZE).bytes)
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_hard_diagonal_edge_is_blended_and_the_flat_regions_either_side_of_it_are_not() {
    let test = "a_hard_diagonal_edge_is_blended";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let frame = diagonal();
    let drawn = antialiased(&device, &queue, &pack, &frame).expect("the pass draws");
    let red = |texels: &[u8], x: u32, y: u32| texels[((y * SIZE + x) * 4) as usize];
    // Far from the edge nothing changes, and the pass only touches the colour of the pixels beside it, never the alpha that holds the luminance
    let changed: Vec<(u32, u32)> = (0..SIZE * SIZE)
        .filter(|i| {
            drawn[*i as usize * 4..*i as usize * 4 + 3]
                != frame[*i as usize * 4..*i as usize * 4 + 3]
        })
        .map(|i| (i % SIZE, i / SIZE))
        .collect();
    assert!(changed.len() >= SIZE as usize, "{}", changed.len());
    assert!(
        changed.iter().all(|(x, y)| x.abs_diff(*y) <= 2),
        "{changed:?}"
    );
    // What it writes there lies between the white and the black it joins
    let (x, y) = changed[changed.len() / 2];
    assert!(red(&drawn, x, y) != red(&frame, x, y));
    assert_eq!(red(&drawn, SIZE - 4, 3), 255);
    assert_eq!(red(&drawn, 3, SIZE - 4), 0);
    // The screen keeps the alpha it was cleared to
    assert!(drawn.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
}
