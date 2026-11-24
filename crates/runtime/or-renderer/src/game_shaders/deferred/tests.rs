use glam::{Mat4, Vec3};

use super::super::fog_volumes::FogLight;
use super::super::lighting::Lighting;
use super::super::pack::GamePack;
use super::super::synthetic::quad;
use super::super::test_pack::{gpu, real_pack};
use super::super::tone::{Tone, ToneSettings};
use super::*;

/// The projection the viewport draws with: a nearer surface has the greater depth.
fn reversed(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    Mat4::from_cols(
        glam::Vec4::new(f / aspect, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, f, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, near / (far - near), -1.0),
        glam::Vec4::new(0.0, 0.0, near * far / (far - near), 0.0),
    )
}

fn engine() -> EngineConstants {
    EngineConstants {
        view: Mat4::look_at_rh(Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, -5.0, 2.0), Vec3::Z),
        projection: reversed(60f32.to_radians(), 16.0 / 9.0, 0.5, 400.0),
        ..EngineConstants::default()
    }
}

fn screen() -> Screen {
    Screen {
        width: 1280,
        height: 720,
        near: 0.5,
        far: 400.0,
        reflection_levels: 8,
    }
}

fn three(values: &BTreeMap<&'static str, Vec<f32>>, name: &str) -> Vec3 {
    let v = &values[name];
    Vec3::new(v[0], v[1], v[2])
}

#[test]
fn the_ray_through_a_pixel_is_the_one_the_camera_sees_it_by() {
    let engine = engine();
    let values = lighting_values(&engine, &screen());
    let (tan_h, tan_v) = (
        1.0 / engine.projection.x_axis.x,
        1.0 / engine.projection.y_axis.y,
    );
    for (x, y) in [(0.0, 0.0), (0.3, -0.2), (-1.0, 1.0), (1.0, 1.0)] {
        // How the lighting shader makes it: three dot products of the pixel's place with the three rows
        let at = Vec3::new(x, y, 1.0);
        let ray = Vec3::new(
            at.dot(three(&values, "deferredPerspectiveShearParams0")),
            at.dot(three(&values, "deferredPerspectiveShearParams1")),
            at.dot(three(&values, "deferredPerspectiveShearParams2")),
        );
        let seen = engine
            .view
            .inverse()
            .transform_vector3(Vec3::new(x * tan_h, y * tan_v, -1.0));
        assert!((ray - seen).length() < 1e-5, "({x}, {y}): {ray} {seen}");
    }
}

#[test]
fn a_depth_the_viewport_writes_is_turned_back_into_the_distance_the_shader_reads_it_by() {
    let engine = engine();
    let values = lighting_values(&engine, &screen());
    let projection = &values["deferredProjectionParams"];
    assert!((projection[0] - 16.0 / 9.0 * (30f32.to_radians()).tan()).abs() < 1e-5);
    assert!((projection[1] - 30f32.to_radians().tan()).abs() < 1e-5);
    for distance in [0.6, 3.0, 40.0, 399.0] {
        let clip = engine.projection * glam::Vec4::new(0.0, 0.0, -distance, 1.0);
        let raw = clip.z / clip.w;
        // The shader takes the depth the other way about before it reads it
        let read = 1.0 - raw;
        let linear = projection[2] / (read + projection[3]);
        assert!(
            (linear - distance).abs() < distance * 1e-3,
            "{distance}: {linear}"
        );
    }
}

#[test]
fn the_screen_is_its_size_and_the_size_of_a_pixel_and_the_light_is_written_where_the_pass_reads_it()
{
    let engine = EngineConstants {
        lighting: Some(Lighting {
            directional_light: glam::Vec4::new(0.0, 0.6, -0.8, 1.0),
            directional_colour: glam::Vec4::new(3.0, 2.0, 1.0, 0.0),
            ..Lighting::default()
        }),
        ..engine()
    };
    let values = lighting_values(&engine, &screen());
    assert_eq!(values["deferredLightScreenSize"][..2], [1280.0, 720.0]);
    assert_eq!(values["gReflectionMipCount"], [8.0]);
    assert_eq!(values["globalScreenSize"][2], 1.0 / 1280.0);
    let light = &values["deferredLightParams"];
    assert_eq!(light.len(), LIGHT_PARAMS * 4);
    // The way the light travels in the second register, its colour times its intensity in the fourth
    assert_eq!(light[4..7], [0.0, 0.6, -0.8]);
    assert_eq!(light[12..15], [3.0, 2.0, 1.0]);
    let sun = Sun::of(&engine);
    assert!(sun.shines());
    assert_eq!(sun.travel, Vec3::new(0.0, 0.6, -0.8));
}

#[test]
fn without_a_moment_of_the_day_the_sun_travels_away_from_where_it_is() {
    let engine = EngineConstants {
        sun_direction: Vec3::Z,
        sun_colour: Vec3::new(1.0, 1.0, 1.0),
        lighting: None,
        ..engine()
    };
    assert_eq!(Sun::of(&engine).travel, -Vec3::Z);
    let dark = EngineConstants {
        sun_colour: Vec3::ZERO,
        ..engine
    };
    assert!(!Sun::of(&dark).shines());
}

const SIZE: u32 = 32;

fn target(
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

/// What a frame came to: the light summed at the middle of the frame, and the middle and a corner of it as the screen holds them.
#[derive(Debug)]
struct Lit {
    light: [f32; 3],
    middle: [u8; 4],
    corner: [u8; 4],
}

/// The background the frame is cleared to, as the display holds it.
const BACKGROUND: [u8; 4] = [26, 51, 77, 255];

/// A quad of the game's default effect facing up, seen from above, lit by `light` under a sky of the one colour `sky`.
fn lit_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    light: Lighting,
    sky: f32,
) -> Result<Lit, String> {
    lit_frame_of("default", device, queue, pack, light, sky)
}

/// [`lit_frame`] for a quad of `effect`.
fn lit_frame_of(
    effect: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    light: Lighting,
    sky: f32,
) -> Result<Lit, String> {
    let eye = Vec3::new(0.0, 0.0, 5.0);
    lit_frame_in(effect, eye, &[], &[], &[], device, queue, pack, light, sky)
}

/// [`lit_frame_of`] seen from `eye`, and lit by `lights` as well.
#[allow(clippy::too_many_arguments)]
fn lit_frame_in(
    effect: &str,
    eye: Vec3,
    lights: &[PointLight],
    spots: &[SpotLight],
    fog: &[FogVolume],
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    light: Lighting,
    sky: f32,
) -> Result<Lit, String> {
    let engine = EngineConstants {
        view: Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y),
        projection: reversed(60f32.to_radians(), 1.0, 0.1, 50.0),
        lighting: Some(light.clone()),
        ..EngineConstants::default()
    };
    let screen = Screen {
        width: SIZE,
        height: SIZE,
        near: 0.1,
        far: 50.0,
        reflection_levels: 1,
    };
    let display = wgpu::TextureFormat::Rgba8Unorm;
    let colour = target(
        device,
        display,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = target(
        device,
        DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let hdr = target(
        device,
        HDR_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    );
    let reflection = ReflectionTarget::new(device, 4, 1);
    reflection.write(
        queue,
        &super::super::sky::ReflectionMap {
            width: 4,
            height: 2,
            levels: vec![vec![[sky, sky, sky, 1.0]; 8]],
        },
    );
    let gbuffer = GBuffer::new(device, SIZE, SIZE);
    let views = DepthViews::of(&depth);
    let depth_view = depth.create_view(&Default::default());
    let hdr_view = hdr.create_view(&Default::default());
    let ldr_view = ldr_target(device, display, SIZE, SIZE);
    let white = super::super::mesh::Image {
        width: 1,
        height: 1,
        rgba: vec![255; 4],
        srgb: false,
    };
    let surfaces = [(
        quad(0.0),
        Material {
            effect: effect.into(),
            vectors: vec![
                (
                    "specularIntensityMult".to_string(),
                    vec![[1.0, 0.0, 0.0, 0.0]],
                ),
                (
                    "specularFalloffMult".to_string(),
                    vec![[100.0, 0.0, 0.0, 0.0]],
                ),
                ("reflectivePower".to_string(), vec![[1.0, 0.0, 0.0, 0.0]]),
            ],
            textures: [("DiffuseSampler".to_string(), white)].into(),
            ..Material::default()
        },
    )];

    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let passes = FramePasses::new(
        device,
        queue,
        pack,
        HDR_FORMAT,
        display,
        &Inputs {
            gbuffer: &gbuffer,
            depth: &views,
            hdr: &hdr_view,
            reflection: reflection.view(),
            ldr: &ldr_view,
        },
    )
    .map_err(|e| e.to_string())?;
    let scene = DeferredScene::prepare(
        device,
        queue,
        pack,
        &mut Default::default(),
        HDR_FORMAT,
        &surfaces,
    );
    assert_eq!(scene.report().0, 1, "{:?}", scene.report().1);
    let tone = Tone::new(&ToneSettings::default(), None, &light);
    scene.update(queue, &engine);
    passes.write(queue, &engine, &screen, &tone, 0.0);
    passes.write_point_lights(queue, &engine, &screen, lights);
    passes.write_spot_lights(queue, &engine, &screen, spots);
    let sky_of_fog = FogSky {
        sun: Vec3::ZERO,
        sun_height: 0.0,
        day_fog: 1.0,
        ground: Vec3::ONE,
    };
    passes.write_fog(queue, &engine, &screen, &sky_of_fog, fog);
    let colour_view = colour.create_view(&Default::default());
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
    let read_only = || {
        Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth_view,
            depth_ops: None,
            stencil_ops: None,
        })
    };
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lighting"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &hdr_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: read_only(),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        passes.record_lighting(&mut pass, &engine);
    }
    if !fog.is_empty() {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fog"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &hdr_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: read_only(),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        passes.record_fog(&mut pass);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(BACKGROUND[0]) / 255.0,
                        g: f64::from(BACKGROUND[1]) / 255.0,
                        b: f64::from(BACKGROUND[2]) / 255.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: read_only(),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        passes.record_resolve(&mut pass);
    }
    queue.submit([encoder.finish()]);
    let error = pollster::block_on(validation.pop()).or(pollster::block_on(internal.pop()));
    if let Some(error) = error {
        return Err(error.to_string().replace('\n', " | "));
    }
    let frame = |texture: &wgpu::Texture, format| {
        super::super::frame::read_back(device, queue, texture, format, SIZE)
    };
    let (summed, shown) = (frame(&hdr, HDR_FORMAT), frame(&colour, display));
    let middle = (SIZE / 2 * SIZE + SIZE / 2) as usize;
    let pixel = |at: usize| [0, 1, 2, 3].map(|i| shown.bytes[at * 4 + i]);
    let half = |i: usize| {
        or_io::half::half_to_f32(u16::from_le_bytes([
            summed.bytes[middle * 8 + i * 2],
            summed.bytes[middle * 8 + i * 2 + 1],
        ]))
    };
    Ok(Lit {
        light: [half(0), half(1), half(2)],
        middle: pixel(middle),
        corner: pixel(0),
    })
}

/// A moment whose only light is a sun of `colour` travelling `towards`.
fn sunlit(towards: Vec3, colour: f32) -> Lighting {
    Lighting {
        directional_light: towards.extend(1.0),
        directional_colour: glam::Vec4::new(colour, colour, colour, 0.0),
        ..Lighting::default()
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_white_surface_is_lit_by_the_games_pass_as_the_sun_colour_times_the_cosine_of_its_angle() {
    let test = "a_white_surface_is_lit_by_the_games_pass";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let lit = |towards: Vec3, colour: f32| {
        lit_frame(&device, &queue, &pack, sunlit(towards, colour), 0.0)
            .expect("the frame is lit")
            .light[0]
    };
    // A fifth of the sun straight on, a fifth of it at 45 degrees, and none from behind
    assert!((lit(-Vec3::Z, 0.2) - 0.2).abs() < 0.01);
    assert!((lit(Vec3::new(1.0, 0.0, -1.0).normalize(), 0.2) - 0.141).abs() < 0.01);
    assert_eq!(lit(Vec3::Z, 0.2), 0.0);
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_resolve_maps_the_light_to_the_screen_where_something_was_drawn_and_leaves_the_background_be()
{
    let test = "the_resolve_maps_the_light";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let brighter = lit_frame(&device, &queue, &pack, sunlit(-Vec3::Z, 2.0), 0.0).expect("lit");
    let dimmer = lit_frame(&device, &queue, &pack, sunlit(-Vec3::Z, 0.5), 0.0).expect("lit");
    let none = lit_frame(&device, &queue, &pack, sunlit(Vec3::Z, 2.0), 0.0).expect("lit");
    // More light is more on the screen, and the curve holds even a strong sun below white
    assert!(
        brighter.middle[0] > dimmer.middle[0],
        "{brighter:?} {dimmer:?}"
    );
    assert!(dimmer.middle[0] > none.middle[0], "{dimmer:?} {none:?}");
    assert!(brighter.middle[0] < 255, "{brighter:?}");
    // Where nothing was drawn the background shows through, whatever the light
    assert_eq!(brighter.corner, BACKGROUND);
    assert_eq!(none.corner, BACKGROUND);
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_surface_reflects_the_map_of_the_sky_it_is_given() {
    let test = "a_surface_reflects_the_map_of_the_sky";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let lit = |sky| {
        lit_frame_of(
            "spec_reflect",
            &device,
            &queue,
            &pack,
            sunlit(Vec3::Z, 0.0),
            sky,
        )
        .expect("lit")
    };
    let (dark, bright) = (lit(0.0), lit(4.0));
    assert_eq!(dark.light, [0.0; 3]);
    assert!(bright.light[0] > 0.01, "{bright:?}");
}

fn lamp(at: Vec3, radius: f32) -> PointLight {
    PointLight {
        position: at,
        colour: Vec3::ONE,
        intensity: 1.0,
        radius,
        falloff: 1.0,
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_point_light_lights_the_surface_it_reaches_from_in_front_of_it_and_no_other() {
    let test = "a_point_light_lights_the_surface_it_reaches";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    // No sun and no sky: what lights the surface is the lamps
    let dark = || sunlit(Vec3::Z, 0.0);
    let lit_by = |eye: Vec3, lamps: &[PointLight]| {
        lit_frame_in(
            "default",
            eye,
            lamps,
            &[],
            &[],
            &device,
            &queue,
            &pack,
            dark(),
            0.0,
        )
        .expect("the frame is lit")
        .light[0]
    };
    let above = Vec3::new(0.0, 0.0, 5.0);
    assert_eq!(lit_by(above, &[]), 0.0);
    // The camera inside the reach of the lamp, and then outside it
    let inside = lit_by(above, &[lamp(Vec3::new(0.0, 0.0, 2.0), 10.0)]);
    let outside = lit_by(above, &[lamp(Vec3::new(0.0, 0.0, 2.0), 2.5)]);
    assert!(inside > 0.01, "{inside}");
    assert!(outside > 0.01, "{outside}");
    // The nearer a lamp, the more it lights
    let near = lit_by(above, &[lamp(Vec3::new(0.0, 0.0, 1.0), 10.0)]);
    let far = lit_by(above, &[lamp(Vec3::new(0.0, 0.0, 3.0), 10.0)]);
    assert!(near > far && far > 0.0, "{near} {far}");
    // A lamp that does not reach the surface, and one behind it, light nothing
    assert_eq!(lit_by(above, &[lamp(Vec3::new(0.0, 0.0, 2.0), 1.0)]), 0.0);
    assert_eq!(lit_by(above, &[lamp(Vec3::new(0.0, 0.0, -2.0), 10.0)]), 0.0);
    // Two lamps light it by the one and the other
    let both = lit_by(
        above,
        &[
            lamp(Vec3::new(0.0, 0.0, 1.0), 10.0),
            lamp(Vec3::new(0.0, 0.0, 3.0), 10.0),
        ],
    );
    assert!(
        (both - (near + far)).abs() < 0.01 * both,
        "{both} {near} {far}"
    );
}

fn mist(at: Vec3, range: f32) -> FogVolume {
    FogVolume {
        position: at,
        scale: Vec3::ONE,
        rotation: Vec3::ZERO,
        range,
        density: 100.0,
        falloff: 1.0,
        tint: Vec3::new(1.0, 0.0, 0.0),
        hdr_mult: 1.0,
        light: FogLight::None,
        ground_tint: false,
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_fog_volume_thickens_the_air_between_the_camera_and_the_surface_and_stops_at_the_surface() {
    let test = "a_fog_volume_thickens_the_air";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    // A sun straight down lights the surface at a fifth, and the fog is red
    let day = Lighting {
        fog_params: [glam::Vec4::ONE; 5],
        fog_colours: [glam::Vec4::ONE; 4],
        ..sunlit(-Vec3::Z, 0.2)
    };
    let middle = |eye: Vec3, fog: FogVolume| {
        let frame = lit_frame_in(
            "default",
            eye,
            &[],
            &[],
            &[fog],
            &device,
            &queue,
            &pack,
            day.clone(),
            0.0,
        )
        .expect("the frame is lit");
        // The fog is red over a surface lit at a fifth: how much of it there is is how much red the light gained
        let red = frame.light[0];
        assert!(
            frame.light[1..]
                .iter()
                .all(|c| (c - 0.2 * (1.0 - fog_of(red))).abs() < 0.01),
            "{:?}",
            frame.light
        );
        fog_of(red)
    };
    // What the pixel holds is the fog over the surface: `fog * red + (1 - fog) * 0.2`
    fn fog_of(red: f32) -> f32 {
        (red - 0.2) / 0.8
    }
    let thickness = |length: f32| 1.0 - (-100.0 * 10f32.powf(-2.5) * length).exp();
    let above = Vec3::new(0.0, 0.0, 5.0);

    // A camera in the volume sees it from the eye to where it ends: two units to the edge
    let inside = middle(above, mist(above, 2.0));
    assert!((inside - thickness(2.0)).abs() < 0.02, "{inside}");
    // The surface cuts what the eye sees of the volume: a camera two units over it, in a volume reaching a unit below
    let low = Vec3::new(0.0, 0.0, 2.0);
    let cut = middle(low, mist(Vec3::new(0.0, 0.0, 1.0), 2.0));
    assert!(
        (cut - thickness(2.0)).abs() < 0.02,
        "{cut} is not cut at the surface"
    );
    // From outside the volume the fog is as thick as the way through it, and thinner where the surface cuts it off
    let far = Vec3::new(0.0, 0.0, 30.0);
    let whole = middle(far, mist(Vec3::new(0.0, 0.0, 4.0), 2.0));
    let half = middle(far, mist(Vec3::ZERO, 2.0));
    assert!(whole > 0.3 && half < whole - 0.05, "{whole} {half}");
    // A volume wholly behind the surface, and one wholly behind the camera, fog nothing
    assert!(middle(above, mist(Vec3::new(0.0, 0.0, -4.0), 1.0)) < 0.01);
    assert!(middle(above, mist(Vec3::new(0.0, 0.0, 9.0), 1.0)) < 0.01);
    // A thicker fog takes more of the way
    let thin = middle(above, mist(above, 2.0));
    let thick = middle(
        above,
        FogVolume {
            density: 300.0,
            ..mist(above, 2.0)
        },
    );
    assert!(thick > thin + 0.2, "{thin} {thick}");
}

#[test]
fn every_value_the_frame_writes_fits_the_variable_it_names_in_the_pack() {
    let Some(pack) = real_pack("every_value_the_frame_writes_fits") else {
        return;
    };
    // A value longer than the variable is not written at all, and the variable keeps what the effect declares
    let mut values = engine().values();
    values.extend(lighting_values(&engine(), &screen()));
    let mut too_long = Vec::new();
    for buffer in pack.manifest().constant_buffers.values() {
        for variable in &buffer.variables {
            let Some(numbers) = values.get(variable.name.as_str()) else {
                continue;
            };
            let room = (variable.size / 4) as usize;
            if numbers.len() > room {
                too_long.push(format!(
                    "{} gives {} for {room}",
                    variable.name,
                    numbers.len()
                ));
            }
        }
    }
    too_long.sort();
    too_long.dedup();
    assert!(too_long.is_empty(), "{too_long:?}");
}

fn cone(at: Vec3, towards: Vec3, degrees: f32) -> SpotLight {
    SpotLight {
        position: at,
        direction: towards,
        colour: Vec3::ONE,
        intensity: 1.0,
        radius: 10.0,
        falloff: 1.0,
        outer_degrees: degrees,
        inner_degrees: 0.0,
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_spot_light_lights_the_surface_inside_its_cone_and_no_other() {
    let test = "a_spot_light_lights_the_surface_inside";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    // No sun and no sky: what lights the surface is the lamp
    let lit_by = |spot: SpotLight| {
        lit_frame_in(
            "default",
            Vec3::new(0.0, 0.0, 5.0),
            &[],
            &[spot],
            &[],
            &device,
            &queue,
            &pack,
            sunlit(Vec3::Z, 0.0),
            0.0,
        )
        .expect("the frame is lit")
        .light[0]
    };
    let above = Vec3::new(0.0, 0.0, 2.0);
    let towards_it = lit_by(cone(above, Vec3::NEG_Z, 40.0));
    assert!(towards_it > 0.01, "{towards_it}");
    // The same light turned away lights nothing, and so does a cone too narrow to reach the middle from the side
    assert_eq!(lit_by(cone(above, Vec3::Z, 40.0)), 0.0);
    let aside = Vec3::new(1.5, 0.0, 2.0);
    assert_eq!(lit_by(cone(aside, Vec3::NEG_Z, 10.0)), 0.0);
    // A wide enough cone from the same place reaches it
    assert!(lit_by(cone(aside, Vec3::NEG_Z, 60.0)) > 0.01);
    // A nearer light lights more, where the reach is short enough for the light to thin out
    let short = |height: f32| SpotLight {
        radius: 2.5,
        ..cone(Vec3::new(0.0, 0.0, height), Vec3::NEG_Z, 40.0)
    };
    let (near, far) = (lit_by(short(1.0)), lit_by(short(2.0)));
    assert!(near > far && far > 0.0, "{near} {far}");
}
