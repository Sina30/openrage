use or_io::gta5::data::timecycle::Keyframe;

use super::super::deferred::HDR_FORMAT;
use super::super::mesh::Image;
use super::super::test_pack::{gpu, real_pack};
use super::*;

const SIZE: u32 = 64;

#[test]
fn the_dome_is_a_unit_sphere_with_both_sets_of_texture_coordinates_and_every_triangle_in_range() {
    let dome = dome();
    let count = dome.positions.len();
    assert_eq!(count as u32, (AROUND + 1) * (UP + 1));
    for p in &dome.positions {
        let length = Vec3::from(*p).length();
        assert!(
            (length - 1.0).abs() < 1e-5,
            "{p:?} is {length} from the middle"
        );
    }
    // From the top to the bottom
    let heights: Vec<f32> = dome.positions.iter().map(|p| p[2]).collect();
    assert!(heights.iter().any(|z| (*z - 1.0).abs() < 1e-6));
    assert!(heights.iter().any(|z| (*z + 1.0).abs() < 1e-6));
    assert_eq!(dome.uvs.len(), 2);
    assert!(dome.uvs.values().all(|uvs| uvs.len() == count));
    assert_eq!(dome.indices.len() as u32, 6 * AROUND * UP);
    assert!(dome.indices.iter().all(|i| (*i as usize) < count));
}

fn day() -> Keyframe {
    let mut day = Keyframe::default();
    for (name, value) in [
        ("sky_azimuth_east_col_b", 1.0),
        ("sky_azimuth_east_col_g", 0.6),
        ("sky_azimuth_east_col_inten", 1.0),
        ("sky_azimuth_west_col_b", 1.0),
        ("sky_azimuth_west_col_g", 0.6),
        ("sky_azimuth_west_col_inten", 1.0),
        ("sky_azimuth_transition_col_b", 0.9),
        ("sky_azimuth_transition_col_g", 0.7),
        ("sky_azimuth_transition_col_inten", 1.0),
        ("sky_azimuth_transition_position", 0.5),
        ("sky_zenith_col_b", 0.5),
        ("sky_zenith_col_g", 0.2),
        ("sky_zenith_col_inten", 1.0),
        ("sky_zenith_transition_position", 0.8),
        ("sky_zenith_blend_start", 0.0),
        ("sky_hdr", 2.0),
        ("sky_sun_col_r", 1.0),
        ("sky_sun_col_g", 1.0),
        ("sky_sun_col_b", 1.0),
        ("sky_sun_hdr", 4.0),
    ] {
        day.set(name, value);
    }
    day
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

/// The colour of each pixel of a frame the sky is drawn over, whose stencil says the left half was drawn at and the right half was not.
fn drawn_toward(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    target: Vec3,
    toward_sun: Vec3,
    day: &Keyframe,
) -> Result<Vec<[f32; 3]>, String> {
    let sky = Sky::new(day, toward_sun, Vec3::splat(0.1));
    drawn_with(device, queue, pack, target, &sky, &SkyTextures::default())
}

/// [`drawn_toward`] for a sky as it is, with the maps of the night the install gives.
fn drawn_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    target: Vec3,
    sky: &Sky,
    textures: &SkyTextures,
) -> Result<Vec<[f32; 3]>, String> {
    let hdr = texture(
        device,
        HDR_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    );
    let hdr_view = hdr.create_view(&Default::default());
    let depth = texture(
        device,
        DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST,
    );
    // The stencil: nought where something was drawn, and the value it is cleared to where nothing was
    let stencil: Vec<u8> = (0..SIZE * SIZE)
        .map(|i| if i % SIZE < SIZE / 2 { 0 } else { 7 })
        .collect();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &depth,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::StencilOnly,
        },
        &stencil,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SIZE),
            rows_per_image: Some(SIZE),
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut pass = SkyPass::new(device, queue, pack).ok_or("the pack has no sky")?;
    pass.set_textures(textures.clone());
    let far = 100.0;
    let f = 1.0 / (60f32.to_radians() * 0.5).tan();
    let engine = EngineConstants {
        view: Mat4::look_at_rh(Vec3::ZERO, target, Vec3::Z),
        projection: Mat4::from_cols(
            glam::Vec4::new(f, 0.0, 0.0, 0.0),
            glam::Vec4::new(0.0, f, 0.0, 0.0),
            glam::Vec4::new(0.0, 0.0, 0.1 / (far - 0.1), -1.0),
            glam::Vec4::new(0.0, 0.0, 0.1 * far / (far - 0.1), 0.0),
        ),
        ..EngineConstants::default()
    };
    pass.write(queue, &engine, sky, far);
    let mut encoder = device.create_command_encoder(&Default::default());
    pass.record_noise(&mut encoder);
    {
        let depth_view = depth.create_view(&Default::default());
        let mut draw = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sky"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &hdr_view,
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
        .map(|p| {
            let half =
                |i: usize| or_io::half::half_to_f32(u16::from_le_bytes([p[i * 2], p[i * 2 + 1]]));
            [half(0), half(1), half(2)]
        })
        .collect())
}

/// [`drawn_toward`] with the sun high over the camera's shoulder, looking level or up.
fn drawn_over(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    looking_up: bool,
) -> Result<Vec<[f32; 3]>, String> {
    let target = if looking_up {
        Vec3::new(0.0, 1.0, 6.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    drawn_toward(
        device,
        queue,
        pack,
        target,
        Vec3::new(0.3, 0.2, 0.9),
        &day(),
    )
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_sky_is_drawn_by_the_games_pass_where_nothing_was_drawn_and_nowhere_else() {
    let test = "the_sky_is_drawn_by_the_games_pass";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let at = |frame: &[[f32; 3]], x: u32, y: u32| frame[(y * SIZE + x) as usize];
    let level = drawn_over(&device, &queue, &pack, false).expect("the sky is drawn");
    // Where something was drawn the frame is left as it was, which is black
    assert_eq!(at(&level, 8, 32), [0.0; 3]);
    // Where nothing was, the sky is: blue the most, as the colours of the day are
    let (sky, high) = (at(&level, 56, 56), at(&level, 56, 8));
    assert!(
        sky[2] > sky[1] && sky[1] > sky[0] && sky[2] > 0.0,
        "{sky:?}"
    );
    // And lighter towards the horizon, in the middle, than high above it
    let horizon = at(&level, 56, 32);
    assert!(
        horizon[2] > high[2] || horizon[1] > high[1],
        "{horizon:?} {high:?}"
    );
    // Looking up, the sky is the zenith's
    let up = drawn_over(&device, &queue, &pack, true).expect("the sky is drawn");
    assert!(
        at(&up, 56, 32)[1] < horizon[1],
        "{:?} {horizon:?}",
        at(&up, 56, 32)
    );
}

/// The day with a white disc of the sun, big enough to measure at the size of the frames of these tests.
fn day_with_a_sun_disc() -> Keyframe {
    let mut day = day();
    for (name, value) in [
        ("sky_sun_disc_col_r", 1.0),
        ("sky_sun_disc_col_g", 1.0),
        ("sky_sun_disc_col_b", 1.0),
        ("sky_sun_disc_size", 16.0),
    ] {
        day.set(name, value);
    }
    day
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_sun_is_a_disc_of_its_colour_where_it_stands_in_the_sky_and_nowhere_else() {
    let test = "the_sun_is_a_disc_of_its_colour";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let at = |frame: &[[f32; 3]], x: u32, y: u32| frame[(y * SIZE + x) as usize];
    let level = Vec3::new(0.0, 1.0, 0.0);
    let with = day_with_a_sun_disc();
    // Up to the right of the middle, clear of the horizon where the sky fades, and then behind the camera
    let sun = Vec3::new(0.15, 1.0, 0.25);
    let seen = drawn_toward(&device, &queue, &pack, level, sun, &with).expect("the sun is drawn");
    let behind =
        drawn_toward(&device, &queue, &pack, level, -sun, &with).expect("the sky is drawn");
    let bare = drawn_toward(&device, &queue, &pack, level, sun, &day()).expect("the sky is drawn");
    // The disc is its colour squared times the intensity of the sun, in the middle of it
    let disc = at(&seen, 40, 18);
    assert!(disc.iter().all(|c| (*c - 4.0).abs() < 0.2), "{disc:?}");
    // With no disc, or with the sun elsewhere, what is there is the sky, a good deal dimmer
    for frame in [&bare, &behind] {
        let sky = at(frame, 40, 18);
        assert!(sky.iter().all(|c| *c < 2.0), "{sky:?}");
    }
    // Far from it the sky is as it was, and where something was drawn the disc is not
    assert_eq!(at(&seen, 56, 56), at(&bare, 56, 56));
    assert_eq!(at(&seen, 8, 18), [0.0; 3]);
}

#[test]
fn the_pass_is_picked_by_what_is_up_as_the_game_picks_it() {
    let day = day();
    let sky = |sun: Vec3, moon: Vec3| Sky::new(&day, sun, Vec3::ZERO).placed(sun, moon);
    let up = Vec3::new(0.0, 1.0, 0.5);
    let down = -up;
    // The sun is the 8, the moon the 4 and the stars the 1, and the stars wait for a map of them
    assert_eq!(sky(up, down).variant(false, false), 8);
    assert_eq!(sky(up, down).variant(true, false), 8 | 1);
    assert_eq!(sky(down, up).variant(false, false), 4);
    assert_eq!(sky(down, up).variant(true, false), 4 | 1);
    assert_eq!(sky(down, down).variant(false, false), 0);
    // The clouds are the 2, and wait for the noise they are cut from
    assert_eq!(sky(up, down).variant(true, true), 8 | 2 | 1);
    assert_eq!(sky(down, down).variant(false, true), 2);
    assert_eq!(sky(down, down).variant(false, false), 0);
    // A moon of no intensity is not up, and a sun of no intensity either
    let mut dark = day.clone();
    dark.set("sky_moon_iten", 0.0);
    dark.set("sky_sun_hdr", 0.0);
    let dim = Sky::new(&dark, up, Vec3::ZERO).placed(up, up);
    assert_eq!(dim.variant(false, false), 0);
}

/// The night: no sun, a white moon big enough to measure, and stars at the intensity of the game.
fn night() -> Keyframe {
    let mut night = day();
    for (name, value) in [
        ("sky_sun_hdr", 0.0),
        ("sky_moon_col_r", 1.0),
        ("sky_moon_col_g", 1.0),
        ("sky_moon_col_b", 1.0),
        ("sky_moon_disc_size", 16.0),
        ("sky_moon_iten", 1.0),
        ("sky_stars_iten", 1.0),
    ] {
        night.set(name, value);
    }
    night
}

fn white() -> Image {
    Image {
        width: 4,
        height: 4,
        rgba: vec![255; 4 * 4 * 4],
        srgb: false,
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_night_has_its_stars_over_the_whole_sky_and_its_moon_only_where_the_moon_stands() {
    let test = "the_night_has_its_stars";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let at = |frame: &[[f32; 3]], x: u32, y: u32| frame[(y * SIZE + x) as usize];
    let level = Vec3::new(0.0, 1.0, 0.0);
    // The moon up to the right of the middle, clear of the horizon, and the sun well under it
    let moon = Vec3::new(0.15, 1.0, 0.25);
    let sky = Sky::new(&night(), Vec3::NEG_Z, Vec3::ZERO)
        .placed(Vec3::NEG_Z, moon)
        .with_time(13.0);
    let draw = |textures: SkyTextures| {
        drawn_with(&device, &queue, &pack, level, &sky, &textures).expect("the night is drawn")
    };
    let bare = draw(SkyTextures::default());
    let starry = draw(SkyTextures {
        stars: Some(white()),
        moon: None,
        ..SkyTextures::default()
    });
    let lit = draw(SkyTextures {
        stars: None,
        moon: Some(white()),
        ..SkyTextures::default()
    });
    // A map of white stars lights the sky wherever it is looked at
    // (The game leaves the stars out of the disc of the moon itself)
    for (x, y) in [(56, 56), (56, 8), (58, 30)] {
        let (with, without) = (at(&starry, x, y), at(&bare, x, y));
        assert!(
            with.iter().zip(without).all(|(w, wo)| *w > wo + 0.5),
            "({x}, {y}): {with:?} against {without:?}"
        );
    }
    // The moon adds its light in the middle of its disc, and nothing round it
    let (disc, sky_there) = (at(&lit, 40, 18), at(&bare, 40, 18));
    assert!(
        disc.iter().zip(sky_there).all(|(d, s)| *d > s + 0.5),
        "{disc:?} against {sky_there:?}"
    );
    assert_eq!(at(&lit, 56, 56), at(&bare, 56, 56));
    // And where something was drawn the sky is not
    assert_eq!(at(&starry, 8, 18), [0.0; 3]);
}

/// A map of noise as the game makes its own: a different value in each of the three channels at each texel, from a fixed generator.
fn noise_map() -> Image {
    let mut state = 0x2545_f491_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state >> 24) as u8
    };
    let rgba = (0..64 * 64)
        .flat_map(|_| [next(), next(), next(), 255])
        .collect();
    Image {
        width: 64,
        height: 64,
        rgba,
        srgb: false,
    }
}

fn grey(value: u8) -> Image {
    Image {
        width: 4,
        height: 4,
        rgba: [value, value, value, 255].repeat(16),
        srgb: false,
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_clouds_are_cut_from_the_noise_and_lighten_the_sky_where_they_are_and_only_there() {
    let test = "the_clouds_are_cut_from_the_noise";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let level = Vec3::new(0.0, 1.0, 0.3);
    let keyframe = white_clouds();
    let sky = Sky::new(&keyframe, Vec3::new(0.3, 0.2, 0.9), Vec3::splat(0.1));
    let made = |textures: SkyTextures| {
        drawn_with(&device, &queue, &pack, level, &sky, &textures).expect("the sky is drawn")
    };
    let clear = made(SkyTextures::default());
    let cloudy = made(SkyTextures {
        noise: Some(noise_map()),
        detail: Some(grey(128)),
        dither: Some(grey(128)),
        ..SkyTextures::default()
    });
    let right: Vec<usize> = (0..(SIZE * SIZE) as usize)
        .filter(|i| (*i as u32 % SIZE) >= SIZE / 2)
        .collect();
    let lighter = right
        .iter()
        .filter(|i| cloudy[**i].iter().sum::<f32>() > clear[**i].iter().sum::<f32>() + 0.3)
        .count();
    // Some of the sky is cloud and some of it is clear
    assert!(
        lighter > right.len() / 20 && lighter < right.len() * 19 / 20,
        "{lighter} of {} pixels are lighter with clouds",
        right.len()
    );
    // And where something was drawn there is no sky to have clouds in
    let left = |frame: &[[f32; 3]]| frame[(8 * SIZE + 8) as usize];
    assert_eq!(left(&cloudy), [0.0; 3]);
}

/// A day whose clouds are white and take the whole of what they are cut from.
fn white_clouds() -> Keyframe {
    let mut keyframe = day();
    for (name, value) in [
        ("sky_cloud_base_col_r", 1.0),
        ("sky_cloud_base_col_g", 1.0),
        ("sky_cloud_base_col_b", 1.0),
        ("sky_cloud_mid_col_r", 1.0),
        ("sky_cloud_mid_col_g", 1.0),
        ("sky_cloud_mid_col_b", 1.0),
        ("sky_cloud_shadow_col_r", 1.0),
        ("sky_cloud_shadow_col_g", 1.0),
        ("sky_cloud_shadow_col_b", 1.0),
        ("sky_small_cloud_col_r", 1.0),
        ("sky_small_cloud_col_g", 1.0),
        ("sky_small_cloud_col_b", 1.0),
        ("sky_cloud_hdr", 4.0),
        ("sky_cloud_density_mult", 4.0),
        ("sky_cloud_density_bias", 0.0),
        ("sky_stars_iten", 0.0),
    ] {
        keyframe.set(name, value);
    }
    keyframe
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_clouds_move_as_the_cycle_time_runs_and_stand_where_it_stands() {
    let test = "the_clouds_move_as_the_cycle_time_runs";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let level = Vec3::new(0.0, 1.0, 0.3);
    let sky = Sky::new(&white_clouds(), Vec3::new(0.3, 0.2, 0.9), Vec3::splat(0.1));
    let at = |time: f32| {
        let textures = SkyTextures {
            noise: Some(noise_map()),
            detail: Some(grey(128)),
            dither: Some(grey(128)),
            ..SkyTextures::default()
        };
        let sky = sky.with_time(time);
        drawn_with(&device, &queue, &pack, level, &sky, &textures).expect("the sky is drawn")
    };
    let (now, same, later) = (at(13.0), at(13.0), at(13.1));
    assert_eq!(now, same, "the same time draws the same clouds");
    let moved = now.iter().zip(&later).filter(|(a, b)| a != b).count();
    assert!(
        moved > now.len() / 20,
        "{moved} of {} pixels differ a tenth of a day on",
        now.len()
    );
}
