use or_io::gta5::data::timecycle::Keyframe;

use super::super::deferred::{DepthViews, GBuffer, ReflectionTarget};
use super::super::lighting::Lighting;
use super::super::test_pack::{gpu, real_pack};
use super::super::tone::ToneSettings;
use super::*;

const SIZE: u32 = 64;

#[test]
fn the_plan_reads_the_frame_first_writes_it_last_and_fills_each_buffer_before_it_reads_it() {
    let steps = plan();
    assert_eq!(
        steps.first().map(|s| (s.0, s.1)),
        Some((Kind::Bright, Slot::Frame))
    );
    assert_eq!(
        steps.last().map(|s| (s.0, s.2)),
        Some((Kind::Add(Amount::Whole), Slot::Frame))
    );
    let mut written = vec![Slot::Frame];
    for (kind, from, to) in &steps {
        assert!(
            written.contains(from),
            "{kind:?} reads {from:?} before it holds anything"
        );
        if !matches!(kind, Kind::Add(_)) {
            // What is not added is written whole, so it is written once before it is read
            written.retain(|slot| slot != to);
        }
        written.push(*to);
    }
    // The blurs of the four sizes: one across and down at half, a quarter, and twice at an eighth and a sixteenth, and once more at half
    let across = steps.iter().filter(|s| s.0 == Kind::Across).count();
    assert_eq!(across, 1 + 1 + 2 + 2 + 1);
    // Every buffer is part of the plan
    for buffer in Slot::BUFFERS {
        assert!(
            steps.iter().any(|s| s.1 == buffer || s.2 == buffer),
            "{buffer:?}"
        );
    }
}

#[test]
fn a_buffer_is_the_share_of_the_frame_its_name_gives_and_never_less_than_a_texel() {
    assert_eq!(Slot::Half.size((1600, 900)), (800, 450));
    assert_eq!(Slot::Quarter.size((1600, 900)), (400, 225));
    assert_eq!(Slot::Eighth.size((1600, 900)), (200, 112));
    assert_eq!(Slot::SixteenthScratch.size((1600, 900)), (100, 56));
    assert_eq!(Slot::Sixteenth.size((8, 8)), (1, 1));
    assert_eq!(Slot::Frame.size((1600, 900)), (1600, 900));
}

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

/// The red of each pixel of a frame of light that is dark but for a bright square in the middle, after the bloom `intensity` says.
fn bloomed(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    intensity: f32,
) -> Result<Vec<f32>, String> {
    let hdr = texture(
        device,
        HDR_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let texels: Vec<u8> = (0..SIZE * SIZE)
        .flat_map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            let level = if (28..36).contains(&x) && (28..36).contains(&y) {
                30.0
            } else {
                0.0
            };
            [level, level, level, 1.0f32].map(|v| or_io::half::f32_to_half(v).to_le_bytes())
        })
        .flatten()
        .collect();
    queue.write_texture(
        hdr.as_image_copy(),
        &texels,
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
    let depth = texture(
        device,
        DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let views = DepthViews::of(&depth);
    let gbuffer = GBuffer::new(device, SIZE, SIZE);
    let reflection = ReflectionTarget::new(device, 4, 1);
    let ldr =
        super::super::deferred::ldr_target(device, wgpu::TextureFormat::Rgba8Unorm, SIZE, SIZE);

    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pass = BloomPass::new(
        device,
        queue,
        pack,
        &Inputs {
            gbuffer: &gbuffer,
            depth: &views,
            hdr: &hdr_view,
            reflection: reflection.view(),
            ldr: &ldr,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut moment = Keyframe::default();
    for (name, value) in [
        ("postfx_bright_pass_thresh", 0.1),
        ("postfx_bright_pass_thresh_width", 0.0),
        ("postfx_intensity_bloom", intensity),
    ] {
        moment.set(name, value);
    }
    let tone = Tone::new(
        &ToneSettings::default(),
        Some(&moment),
        &Lighting::default(),
    );
    let screen = Screen {
        width: SIZE,
        height: SIZE,
        near: 0.1,
        far: 50.0,
        reflection_levels: 1,
    };
    pass.write(queue, &screen, &tone);
    let mut encoder = device.create_command_encoder(&Default::default());
    pass.record(&mut encoder, &hdr_view, &tone);
    queue.submit([encoder.finish()]);
    let error = pollster::block_on(validation.pop()).or(pollster::block_on(internal.pop()));
    if let Some(error) = error {
        return Err(error.to_string().replace('\n', " | "));
    }
    let read = super::super::frame::read_back(device, queue, &hdr, HDR_FORMAT, SIZE);
    Ok(read
        .bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|p| or_io::half::half_to_f32(u16::from_le_bytes([p[0], p[1]])))
        .collect())
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_bright_part_of_a_frame_is_blurred_and_put_back_round_itself_and_none_is_put_back_where_the_moment_has_none(
) {
    let test = "the_bright_part_of_a_frame_is_blurred";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let at = |frame: &[f32], x: u32, y: u32| frame[(y * SIZE + x) as usize];
    let none = bloomed(&device, &queue, &pack, 0.0).expect("the passes run");
    assert_eq!(at(&none, 2, 2), 0.0);
    assert_eq!(at(&none, 38, 32), 0.0);
    assert_eq!(at(&none, 32, 32), 30.0);

    // A quarter of the intensity is put back
    let some = bloomed(&device, &queue, &pack, 4.0).expect("the passes run");
    let (near, far, middle) = (at(&some, 38, 32), at(&some, 2, 2), at(&some, 32, 32));
    assert!(far > 0.0, "the glow reaches the corner: {far}");
    assert!(
        near > far * 1.5,
        "it is stronger beside the square: {near} against {far}"
    );
    assert!(
        middle > 30.0,
        "the square keeps its light and gains: {middle}"
    );
}
