use glam::{Mat4, Vec3, Vec4};

use super::super::deferred::{
    hdr_target, ldr_target, stencil_clear, DeferredScene, DepthViews, GBuffer, ReflectionTarget,
    HDR_FORMAT,
};
use super::super::mesh::{Image, Material, Surface};
use super::super::synthetic::quad;
use super::super::test_pack::{gpu, real_pack};
use super::*;

const SIZE: u32 = 128;
const NEAR: f32 = 0.1;
const FAR: f32 = 50.0;

/// The projection the viewport draws with: a nearer surface has the greater depth.
fn reversed(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    Mat4::from_cols(
        Vec4::new(f / aspect, 0.0, 0.0, 0.0),
        Vec4::new(0.0, f, 0.0, 0.0),
        Vec4::new(0.0, 0.0, near / (far - near), -1.0),
        Vec4::new(0.0, 0.0, near * far / (far - near), 0.0),
    )
}

fn engine() -> EngineConstants {
    EngineConstants {
        view: Mat4::look_at_rh(Vec3::new(0.0, -3.0, 3.0), Vec3::new(0.0, 0.5, 0.0), Vec3::Z),
        projection: reversed(50f32.to_radians(), 1.0, NEAR, FAR),
        ..EngineConstants::default()
    }
}

fn screen() -> Screen {
    Screen {
        width: SIZE,
        height: SIZE,
        near: NEAR,
        far: FAR,
        reflection_levels: 1,
    }
}

#[test]
fn the_reach_of_the_samples_is_a_number_of_pixels_of_the_frame_and_the_depth_is_read_the_way_lighting_reads_it(
) {
    let (engine, screen) = (engine(), screen());
    let values = values(&engine, &screen, 0.5);
    assert_eq!(values["g_projParams"], values["deferredProjectionParams"]);
    assert_eq!(values["g_SSAOStrength"], [0.5]);
    // A pixel is two of the units the offsets are written in
    let pixel = 2.0 / SIZE as f32;
    let projection = &values["deferredProjectionParams"];
    assert_eq!(
        values["gNormalOffset"],
        [pixel * projection[0], 0.0, 0.0, pixel * projection[1]]
    );
    let reach = pixel * REACH;
    assert_eq!(
        values["gOffsetScale0"],
        [0.75 * reach, 0.75 * reach, -0.25 * reach, -0.25 * reach]
    );
    assert_eq!(
        values["gOffsetScale1"],
        [reach, -reach, -0.5 * reach, 0.5 * reach]
    );
}

#[test]
fn the_pass_runs_only_where_the_day_asks_for_some_occlusion() {
    assert!(!runs_at(0.0));
    assert!(!runs_at(0.0099));
    assert!(runs_at(1.0));
    // The settings of a clear day, four, make the exponent one
    assert_eq!(4.0 * STRENGTH_PER_SETTING, 1.0);
}

/// A floor of two units a side with a wall standing on its far edge, each drawn from both sides.
fn corner() -> Vec<(Surface, Material)> {
    let white = Image {
        width: 1,
        height: 1,
        rgba: vec![255; 4],
        srgb: false,
    };
    let material = Material {
        effect: "default".into(),
        textures: [("DiffuseSampler".to_string(), white)].into(),
        ..Material::default()
    };
    let mut wall = quad(0.0);
    wall.positions = vec![
        [-1.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.5],
        [-1.0, 1.0, 1.5],
    ];
    wall.normals = vec![[0.0, -1.0, 0.0]; 4];
    let mut floor = quad(0.0);
    for surface in [&mut floor, &mut wall] {
        surface.indices.extend([0, 2, 1, 0, 3, 2]);
    }
    vec![(floor, material.clone()), (wall, material)]
}

/// The red and green of the ambient scales of the G-buffer of `corner`, pixel by pixel, after the occlusion of `strength`, and the bytes of the half-size depth, with `depth_wanted` as the frame asks for it.
fn ran(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    strength: f32,
    depth_wanted: bool,
) -> Result<(Vec<[u8; 2]>, Vec<u8>), String> {
    let engine = engine();
    let screen = screen();
    let target = |format, usage| {
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
    };
    let depth = target(
        DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let depth_view = depth.create_view(&Default::default());
    let views = DepthViews::of(&depth);
    let gbuffer = GBuffer::new(device, SIZE, SIZE);
    let hdr = hdr_target(device, SIZE, SIZE);
    let reflection = ReflectionTarget::new(device, 4, 1);
    let ldr = ldr_target(device, wgpu::TextureFormat::Rgba8Unorm, SIZE, SIZE);

    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let ambient = AmbientPass::new(
        device,
        queue,
        pack,
        &Inputs {
            gbuffer: &gbuffer,
            depth: &views,
            hdr: &hdr,
            reflection: reflection.view(),
            ldr: &ldr,
        },
    )
    .map_err(|e| e.to_string())?;
    let scene = DeferredScene::prepare(
        device,
        queue,
        pack,
        &mut Default::default(),
        HDR_FORMAT,
        &corner(),
    );
    assert_eq!(scene.report().0, 2, "{:?}", scene.report().1);
    scene.update(queue, &engine);
    ambient.write(queue, &engine, &screen, strength);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gbuffer"),
            color_attachments: &gbuffer.attachments(),
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(stencil_clear()),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        scene.record_gbuffer(&mut pass);
    }
    ambient.record(&mut encoder, strength, depth_wanted);
    queue.submit([encoder.finish()]);
    let error = pollster::block_on(validation.pop()).or(pollster::block_on(internal.pop()));
    if let Some(error) = error {
        return Err(error.to_string().replace('\n', " | "));
    }
    let read = super::super::frame::read_back(
        device,
        queue,
        gbuffer.ambient_scales().texture(),
        super::super::deferred::GBUFFER_FORMAT,
        SIZE,
    );
    let scales = (read.bytes.as_chunks::<4>().0.iter())
        .map(|p| [p[0], p[1]])
        .collect();
    let depth = super::super::frame::read_back(
        device,
        queue,
        ambient.depth().texture(),
        DEPTH_FORMAT,
        SIZE / 2,
    );
    Ok((scales, depth.bytes))
}

/// The ambient scales of `corner` after the occlusion of `strength`.
fn scales(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    strength: f32,
) -> Result<Vec<[u8; 2]>, String> {
    ran(device, queue, pack, strength, false).map(|(scales, _)| scales)
}

/// The pixel `point` of the world is drawn at.
fn pixel_of(point: Vec3) -> usize {
    let engine = engine();
    let clip = engine.projection * engine.view * point.extend(1.0);
    let (x, y) = (clip.x / clip.w * 0.5 + 0.5, 0.5 - clip.y / clip.w * 0.5);
    (y * SIZE as f32) as usize * SIZE as usize + (x * SIZE as f32) as usize
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_ambient_light_is_darkened_in_the_crease_of_a_wall_and_left_alone_on_the_open_floor() {
    let test = "the_ambient_light_is_darkened_in_the_crease";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let open = pixel_of(Vec3::new(0.0, -0.8, 0.0));
    let crease = pixel_of(Vec3::new(0.0, 0.93, 0.0));
    let plain = scales(&device, &queue, &pack, 0.0).expect("the scales are drawn");
    // The exponent of a day of the strongest setting: the dip in a crease is plain beside the grain of the samples
    let darkened = scales(&device, &queue, &pack, 3.0).expect("the scales are drawn");
    // Where the day asks for none, the pass does not run and the scales are the surface's own
    assert!(plain[open][0] > 100, "{:?}", plain[open]);
    let kept = |at: usize, channel: usize| {
        f32::from(darkened[at][channel]) / f32::from(plain[at][channel].max(1))
    };
    for channel in 0..2 {
        assert!(
            kept(open, channel) > 0.9,
            "open floor, channel {channel}: {} of {}",
            darkened[open][channel],
            plain[open][channel]
        );
        assert!(
            kept(crease, channel) < 0.8,
            "crease, channel {channel}: {} of {}",
            darkened[crease][channel],
            plain[crease][channel]
        );
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_depth_water_reads_is_made_where_there_is_no_occlusion_and_holds_the_distance_along_the_view()
{
    let test = "the_depth_water_reads_is_made_where_there_is_no_occlusion";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let floor = Vec3::new(0.0, -0.8, 0.0);
    // The half-size pixel the floor point is drawn at
    let at = {
        let whole = pixel_of(floor);
        (whole / SIZE as usize / 2) * (SIZE as usize / 2) + (whole % SIZE as usize) / 2
    };
    let value = |bytes: &[u8]| {
        let pixel = &bytes[at * 2..at * 2 + 2];
        or_io::half::half_to_f32(u16::from_le_bytes([pixel[0], pixel[1]]))
    };
    // Where the day asks for no occlusion and nothing asks for the depth, the pass does not run
    let (_, idle) = ran(&device, &queue, &pack, 0.0, false).expect("the frame is drawn");
    assert_eq!(value(&idle), 0.0);
    // Asked for, it is made without the rest of the pass
    let (scales_idle, _) = ran(&device, &queue, &pack, 0.0, false).expect("the frame is drawn");
    let (scales, made) = ran(&device, &queue, &pack, 0.0, true).expect("the frame is drawn");
    assert_eq!(scales, scales_idle, "the occlusion was worked out");
    let engine = engine();
    let expected = -(engine.view * floor.extend(1.0)).z;
    assert!(
        (value(&made) - expected).abs() < 0.05 * expected,
        "{} against {expected} metres",
        value(&made)
    );
}
