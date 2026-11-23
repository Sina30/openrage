use glam::Vec4;

use super::super::deferred::{
    hdr_target, ldr_target, stencil_clear, DeferredScene, DepthViews, GBuffer, ReflectionTarget,
    HDR_FORMAT,
};
use super::super::lighting::Lighting;
use super::super::mesh::{Image, Material, Surface};
use super::super::synthetic::quad;
use super::super::test_pack::{gpu, real_pack};
use super::*;

const SIZE: u32 = 128;
const NEAR: f32 = 0.1;
const FAR: f32 = 50.0;

fn sun_travelling(travel: Vec3) -> Lighting {
    Lighting {
        directional_light: travel.extend(1.0),
        directional_colour: Vec4::new(1.0, 1.0, 1.0, 0.0),
        ..Lighting::default()
    }
}

#[test]
fn what_the_receiving_pass_works_out_is_where_the_surfaces_were_drawn_to() {
    let fit = Fit::of(Vec3::new(-0.4, 0.3, -0.8), Vec3::new(3.0, -2.0, 1.5), 2.5);
    let caster = fit.caster();
    let eye = Vec3::new(-6.0, 4.0, 3.0);
    for p in [
        Vec3::new(3.0, -2.0, 1.5),
        Vec3::new(4.2, -1.0, 0.3),
        Vec3::new(1.1, -3.4, 2.9),
        Vec3::new(2.0, -2.5, 3.9),
    ] {
        let clip = caster.projection * caster.view * p.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        // Across the tile from the left, down it from the top, and in depth: how a texel of the map is found by a coordinate
        let drawn = Vec3::new(ndc.x * 0.5 + 0.5, (1.0 - ndc.y) * 0.5, ndc.z);
        let read = fit.lookup(p, eye);
        let expected = Vec3::new(read.x + 0.5, read.y + 0.5, read.z + BIAS);
        assert!(
            (drawn - expected).abs().max_element() < 1e-4,
            "{p}: drawn {drawn}, read {expected}"
        );
        // Inside the sphere, which the tile holds whole
        assert!(read.x.abs().max(read.y.abs()) < 0.5);
        assert!(drawn.z > 0.0 && drawn.z < 1.0);
    }
}

#[test]
fn a_surface_nearer_the_sun_is_nearer_in_the_map_and_the_constants_carry_the_axes_by_row() {
    let travel = Vec3::new(0.5, -0.25, -1.0).normalize();
    let fit = Fit::of(travel, Vec3::ZERO, 2.0);
    let eye = Vec3::new(1.0, 1.0, 5.0);
    let toward_sun = -travel;
    let near = fit.lookup(toward_sun, eye);
    let far = fit.lookup(-toward_sun, eye);
    assert!(near.z < far.z);
    // Three rows, each the axes' one component, so that a vector times them is its place on each axis
    let values = fit.values(eye);
    let shared = &values["gCSMShaderVars_shared"];
    assert_eq!(shared.len(), 48);
    let row = |i: usize| Vec3::new(shared[i * 4], shared[i * 4 + 1], shared[i * 4 + 2]);
    let v = Vec3::new(0.3, -0.7, 0.9);
    let place = v.x * row(0) + v.y * row(1) + v.z * row(2);
    let expected = Vec3::new(v.dot(fit.across), v.dot(fit.down), v.dot(fit.along));
    assert!((place - expected).length() < 1e-5);
    // The way the light travels is the third axis
    assert!((fit.along - travel).length() < 1e-6);
    // No fade of the shadow with distance, and nothing of the fog's
    assert_eq!((shared[3], shared[7]), (0.0, 1.0));
    assert_eq!(values["gCSMResolution"][..2], [TILE as f32, TILE as f32]);
}

#[test]
fn a_sun_that_does_not_shine_casts_none() {
    let dark = EngineConstants {
        lighting: Some(Lighting::default()),
        ..EngineConstants::default()
    };
    assert!(ShadowPass::fit(&dark, (Vec3::ZERO, 1.0)).is_none());
    let lit = EngineConstants {
        lighting: Some(sun_travelling(-Vec3::Z)),
        ..EngineConstants::default()
    };
    assert!(ShadowPass::fit(&lit, (Vec3::ZERO, 1.0)).is_some());
}

/// A floor, and a table-top over half of it that the sun, travelling across and down, throws a shadow of onto the floor beside it.
fn table() -> Vec<(Surface, Material)> {
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
    let mut floor = quad(0.0);
    let mut top = quad(1.0);
    top.positions = vec![
        [-0.5, -1.0, 1.0],
        [0.5, -1.0, 1.0],
        [0.5, 1.0, 1.0],
        [-0.5, 1.0, 1.0],
    ];
    for surface in [&mut floor, &mut top] {
        surface.indices.extend([0, 2, 1, 0, 3, 2]);
    }
    vec![(floor, material.clone()), (top, material)]
}

fn viewer() -> EngineConstants {
    let f = 1.0 / (50f32.to_radians() * 0.5).tan();
    let projection = glam::Mat4::from_cols(
        Vec4::new(f, 0.0, 0.0, 0.0),
        Vec4::new(0.0, f, 0.0, 0.0),
        Vec4::new(0.0, 0.0, NEAR / (FAR - NEAR), -1.0),
        Vec4::new(0.0, 0.0, NEAR * FAR / (FAR - NEAR), 0.0),
    );
    EngineConstants {
        view: glam::Mat4::look_at_rh(Vec3::new(0.0, -0.8, 4.5), Vec3::ZERO, Vec3::Z),
        projection,
        lighting: Some(sun_travelling(Vec3::new(-1.0, 0.0, -2.0).normalize())),
        ..EngineConstants::default()
    }
}

/// How much of the sun each pixel of the G-buffer of `table` keeps, by the alpha of its self-shadow, with the sun's shadow cast where `cast` says.
fn kept(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    cast: bool,
) -> Result<Vec<u8>, String> {
    let engine = viewer();
    let screen = Screen {
        width: SIZE,
        height: SIZE,
        near: NEAR,
        far: FAR,
        reflection_levels: 1,
    };
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let views = DepthViews::of(&depth);
    let gbuffer = GBuffer::new(device, SIZE, SIZE);
    let hdr = hdr_target(device, SIZE, SIZE);
    let ldr = ldr_target(device, wgpu::TextureFormat::Rgba8Unorm, SIZE, SIZE);
    let reflection = ReflectionTarget::new(device, 4, 1);

    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shadow = ShadowPass::new(
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
        &table(),
    );
    assert_eq!(scene.report().0, 2, "{:?}", scene.report().1);
    let fit = ShadowPass::fit(&engine, scene.sphere().expect("surfaces to hold"))
        .expect("the sun shines");
    scene.update(queue, &engine);
    scene.update_shadow(queue, &fit.caster());
    shadow.write(queue, &engine, &screen, &fit);
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
    if cast {
        shadow.record_cast(&mut encoder, |pass| scene.record_shadow(pass));
        shadow.record_receive(&mut encoder, gbuffer.self_shadows(), &depth_view);
    }
    queue.submit([encoder.finish()]);
    let error = pollster::block_on(validation.pop()).or(pollster::block_on(internal.pop()));
    if let Some(error) = error {
        return Err(error.to_string().replace('\n', " | "));
    }
    let read = super::super::frame::read_back(
        device,
        queue,
        gbuffer.self_shadows().texture(),
        GBUFFER_FORMAT,
        SIZE,
    );
    Ok(read.bytes.as_chunks::<4>().0.iter().map(|p| p[3]).collect())
}

/// The pixel `point` of the world is drawn at.
fn pixel_of(point: Vec3) -> usize {
    let engine = viewer();
    let clip = engine.projection * engine.view * point.extend(1.0);
    let (x, y) = (clip.x / clip.w * 0.5 + 0.5, 0.5 - clip.y / clip.w * 0.5);
    (y * SIZE as f32) as usize * SIZE as usize + (x * SIZE as f32) as usize
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_floor_beside_a_table_top_is_in_the_shadow_the_sun_throws_of_it_and_the_floor_beyond_is_not()
{
    let test = "the_floor_beside_a_table_top_is_in_the_shadow";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    // The sun travels towards the negative x and down: the top, a unit across and a unit above the floor, throws its shadow from x -1 to 0
    let shaded = pixel_of(Vec3::new(-0.85, 0.0, 0.0));
    let sunlit = pixel_of(Vec3::new(0.8, 0.0, 0.0));
    let on_top = pixel_of(Vec3::new(0.0, 0.0, 1.0));
    let without = kept(&device, &queue, &pack, false).expect("the self-shadows are drawn");
    let with = kept(&device, &queue, &pack, true).expect("the self-shadows are drawn");
    // With no shadow the sun reaches everything the G-buffer holds
    assert!(
        without[shaded] > 200 && without[sunlit] > 200,
        "{without:?}"
    );
    assert!(
        with[shaded] < 60,
        "shaded floor {} of {}",
        with[shaded],
        without[shaded]
    );
    assert!(with[sunlit] > 200, "sunlit floor {}", with[sunlit]);
    assert!(with[on_top] > 200, "table-top {}", with[on_top]);
}
