use glam::{Mat4, Vec3};

use super::*;
use crate::game_shaders::deferred::GBUFFER_FORMAT;
use crate::game_shaders::frame::read_back;
use crate::game_shaders::pack::GamePack;
use crate::game_shaders::stand_in::uploaded_view;
use crate::game_shaders::test_pack::{gpu, real_pack};
use crate::viewport::{OrbitCamera, ShadingMode};

const SIZE: u32 = 64;
const BACKGROUND: [f32; 4] = [0.1, 0.2, 0.3, 1.0];
const WHITE: [f32; 4] = [1.0; 4];
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// The corners of a square about `centre` that reaches `half` to each side, facing up, its texture laid once across it with u along x and v along y.
fn corners(centre: [f32; 2], half: f32) -> [MeshVertex; 4] {
    [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|c: [f32; 2]| {
        MeshVertex::new(
            [centre[0] + c[0] * half, centre[1] + c[1] * half, 0.0],
            [0.0, 0.0, 1.0],
            [c[0] * 0.5 + 0.5, c[1] * 0.5 + 0.5],
        )
    })
}

const SQUARE: [u32; 6] = [0, 1, 2, 0, 2, 3];

/// A square about the origin that reaches `half` to each side, drawn with `material`.
fn square(device: &wgpu::Device, half: f32, material: WorldMaterial) -> Arc<WorldMesh> {
    let vertices = corners([0.0, 0.0], half);
    Arc::new(WorldMesh::new(device, &vertices, &SQUARE, material))
}

/// The material of no map in `bucket`.
fn plain(device: &wgpu::Device, renderer: &ViewportRenderer, bucket: WorldBucket) -> WorldMaterial {
    renderer.world_material(device, None, None, None, bucket)
}

/// A texture of the one texel `rgba`, read as it is stored.
fn texel(device: &wgpu::Device, queue: &wgpu::Queue, rgba: [u8; 4]) -> wgpu::TextureView {
    uploaded_view(device, queue, 1, 1, &rgba, false)
}

/// A camera `distance` over the origin, looking down.
fn overhead_from(distance: f32) -> OrbitCamera {
    OrbitCamera {
        yaw: 0.0,
        pitch: 1.5,
        distance,
        ..OrbitCamera::default()
    }
}

/// A camera five metres over the origin, looking down.
fn overhead() -> OrbitCamera {
    overhead_from(5.0)
}

/// The pixel of a square frame of `SIZE` that `camera` draws `point` of the world at.
fn pixel_of(camera: &OrbitCamera, point: Vec3) -> usize {
    let clip = camera.view_proj(1.0) * point.extend(1.0);
    let (x, y) = (clip.x / clip.w * 0.5 + 0.5, 0.5 - clip.y / clip.w * 0.5);
    (y * SIZE as f32) as usize * SIZE as usize + (x * SIZE as f32) as usize
}

/// Every pixel of the frame `renderer` draws in `mode`, row by row.
fn frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &ViewportRenderer,
    camera: &OrbitCamera,
    mode: ShadingMode,
) -> Vec<[u8; 4]> {
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.render(&mut encoder, queue, camera, mode);
    queue.submit([encoder.finish()]);
    let read = read_back(
        device,
        queue,
        renderer.colour_texture(),
        renderer.colour_format,
        SIZE,
    );
    read.bytes.as_chunks::<4>().0.to_vec()
}

/// The machine's device as every test takes it, and after it one that issues several draws by one call, where the adapter makes such a device.
fn devices(test: &str) -> Vec<(wgpu::Device, wgpu::Queue)> {
    let Some(first) = gpu(test) else {
        return Vec::new();
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }));
    let wanted = wgpu::Features::INDIRECT_FIRST_INSTANCE;
    let second = adapter
        .ok()
        .filter(|adapter| adapter.features().contains(wanted))
        .and_then(|adapter| {
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("world-test-several-draws"),
                required_features: wanted,
                required_limits: adapter.limits(),
                ..Default::default()
            }))
            .ok()
        });
    [Some(first), second].into_iter().flatten().collect()
}

#[test]
fn an_instance_is_the_first_three_rows_of_its_matrix_and_its_tint() {
    let matrix = Mat4::from_scale_rotation_translation(
        Vec3::splat(2.0),
        glam::Quat::from_rotation_z(0.5),
        Vec3::new(10.0, 20.0, 30.0),
    );
    let instance = WorldInstance::of(matrix, [0.1, 0.2, 0.3, 0.4]);
    let point = Vec3::new(1.0, -2.0, 3.0);
    // What the vertex stage does with the rows is what the matrix does to a point
    let placed = Vec3::from_array(
        instance
            .rows
            .map(|row| glam::Vec4::from(row).dot(point.extend(1.0))),
    );
    assert!((placed - matrix.transform_point3(point)).length() < 1e-4);
    assert_eq!(instance.tint, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(std::mem::size_of::<WorldInstance>(), 64);
}

#[test]
fn the_buckets_are_declared_in_the_order_they_draw_and_what_blends_or_lies_on_a_surface_casts_no_shadow(
) {
    use WorldBucket::{Blend, Cutout, Decal, Emissive, Opaque};
    let mut buckets = [Blend, Decal, Emissive, Cutout, Opaque];
    buckets.sort();
    assert_eq!(buckets, [Opaque, Cutout, Emissive, Decal, Blend]);
    assert_eq!(buckets.len(), WorldBucket::COUNT);
    assert_eq!(
        buckets.map(WorldBucket::casts),
        [true, true, true, false, false]
    );
    assert_eq!(WorldBucket::default(), Opaque);
}

#[test]
fn the_cascades_hold_what_the_camera_sees_each_in_its_own_tile_and_their_texels_stay_put_in_the_world(
) {
    use crate::game_shaders::engine::EngineConstants;
    use crate::game_shaders::shadow::TILE;
    let engine = EngineConstants {
        sun_direction: Vec3::new(0.4, -0.2, 0.8).normalize(),
        ..EngineConstants::default()
    };
    let camera = OrbitCamera {
        target: Vec3::new(120.0, -75.0, 20.0),
        yaw: 0.7,
        pitch: 0.3,
        distance: 10.0,
        ..OrbitCamera::default()
    };
    let aspect = 16.0 / 9.0;
    let eye = camera.eye();
    let forward = (camera.target - eye).normalize();
    let cascades = shadow::Cascades::about(&engine, &camera, aspect).expect("the sun shines");
    let values = cascades.values(eye);
    let shared = &values["gCSMShaderVars_shared"];
    assert_eq!(shared.len(), 48);
    let register = |at: usize| Vec3::new(shared[at * 4], shared[at * 4 + 1], shared[at * 4 + 2]);
    // Where a point of the world is in the tile of `cascade`, as the receiving pass works it out: its place along each axis from the eye, scaled and moved
    let read = |point: Vec3, cascade: usize| {
        let from_eye = point - eye;
        let place = from_eye.x * register(0) + from_eye.y * register(1) + from_eye.z * register(2);
        place * register(4 + cascade) + register(8 + cascade)
    };
    let views = cascades.views();
    // A point on the view's axis in each cascade's slice, and one beside it within the view
    let side = camera.view().inverse().x_axis.truncate();
    for (cascade, ahead) in [4.0f32, 20.0, 60.0, 250.0].into_iter().enumerate() {
        for point in [
            eye + forward * ahead,
            eye + forward * ahead + side * (ahead * 0.5),
        ] {
            let at = read(point, cascade);
            assert!(at.x.abs().max(at.y.abs()) < 0.49, "{cascade} {at}");
            // The pass takes the first tile a pixel lies across, whatever its depth: so its depth is one every tile holds
            for tile in 0..shadow::CASCADES {
                let deep = read(point, tile).z;
                assert!(deep > 0.0 && deep < 1.0, "{cascade} {tile} {deep}");
                assert!((deep - at.z).abs() < 0.002, "{cascade} {tile} {deep}");
            }
            // And the instances are drawn into the tile where the pass reads them: across from the left, down from the top, a little nearer the sun than it compares with
            let clip = views[cascade] * point.extend(1.0);
            let drawn = Vec3::new(clip.x * 0.5 + 0.5, (1.0 - clip.y) * 0.5, clip.z);
            assert!((clip.w - 1.0).abs() < 1e-6);
            assert!((drawn.x - (at.x + 0.5)).abs() < 1e-3, "{drawn} {at}");
            assert!((drawn.y - (at.y + 0.5)).abs() < 1e-3, "{drawn} {at}");
            assert!((0.0..0.01).contains(&(drawn.z - at.z)), "{drawn} {at}");
        }
    }
    // The shadow is whole until the last tenth of the reach, and gone at its end
    let fade = |ahead: f32| (ahead * shared[3] + shared[7]).clamp(0.0, 1.0);
    assert!(fade(SHADOW_FAR * 0.9) > 0.999);
    assert!(fade(SHADOW_FAR * 0.95) > 0.4 && fade(SHADOW_FAR * 0.95) < 0.6);
    assert!(fade(SHADOW_FAR) < 0.001);
    // No fade by anything but the distance
    assert_eq!(shared[11], 0.0);

    // A camera that moved a little and turned: a point of the world is a whole number of texels from where it was in each tile
    let moved = OrbitCamera {
        target: camera.target + Vec3::new(0.37, -0.21, 0.13),
        yaw: 0.9,
        ..camera.clone()
    };
    let after = shadow::Cascades::about(&engine, &moved, aspect).expect("the sun shines");
    let point = Vec3::new(118.0, -70.0, 12.0);
    for (cascade, (was, is)) in views.iter().zip(after.views()).enumerate() {
        let texels = |view: Mat4| (view * point.extend(1.0)).truncate() * (TILE as f32 * 0.5);
        let step = texels(is) - texels(*was);
        for across in [step.x, step.y] {
            assert!((across - across.round()).abs() < 0.05, "{cascade} {step}");
        }
    }

    // A sun that gives no light casts none
    let dark = EngineConstants {
        sun_colour: Vec3::ZERO,
        ..engine
    };
    assert!(shadow::Cascades::about(&dark, &camera, aspect).is_none());
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn each_instance_of_a_mesh_is_drawn_where_its_matrix_puts_it_in_its_tint_and_nowhere_else() {
    let test = "each_instance_of_a_mesh_is_drawn_where_its_matrix_puts_it";
    for (device, queue) in devices(test) {
        let mut renderer =
            ViewportRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, SIZE, SIZE);
        renderer.set_background(BACKGROUND);
        let camera = overhead();
        let (red_at, green_at, empty_at) = (
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, 0.5, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
        );
        let instances = [
            WorldInstance::of(Mat4::from_translation(red_at), RED),
            WorldInstance::of(Mat4::from_translation(green_at), [0.0, 1.0, 0.0, 1.0]),
        ];
        let material = plain(&device, &renderer, WorldBucket::Opaque);
        let draws = |device: &wgpu::Device, instances| {
            vec![WorldDraw {
                mesh: square(device, 0.3, material.clone()),
                instances,
            }]
        };
        assert!(!renderer.has_world());
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let nothing = frame(&device, &queue, &renderer, &camera, ShadingMode::Material);
        let background = nothing[0];
        assert!(nothing.iter().all(|pixel| *pixel == background));

        renderer.set_world(&device, &queue, &instances, draws(&device, 0..2));
        assert!(renderer.has_world());
        assert_eq!(renderer.world_instances(), 2);
        // The draws go out by one call each batch only where the device issues an indirect draw from an instance other than the first
        let several = device
            .features()
            .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);
        assert_eq!(renderer.world.indirect.is_some(), several);
        for mode in [
            ShadingMode::Solid,
            ShadingMode::Material,
            ShadingMode::Shaded,
        ] {
            let image = frame(&device, &queue, &renderer, &camera, mode);
            let (red, green) = (
                image[pixel_of(&camera, red_at)],
                image[pixel_of(&camera, green_at)],
            );
            assert!(
                red[0] > 100 && red[1] == 0 && red[2] == 0,
                "{mode:?} {red:?}"
            );
            assert!(
                green[1] > 100 && green[0] == 0 && green[2] == 0,
                "{mode:?} {green:?}"
            );
            assert_eq!(image[pixel_of(&camera, empty_at)], background, "{mode:?}");
            assert_eq!(image[pixel_of(&camera, Vec3::ZERO)], background, "{mode:?}");
            // Two squares of 0.6 seen from five metres: the instances cover little of the frame
            let covered = image.iter().filter(|pixel| **pixel != background).count();
            assert!((40..400).contains(&covered), "{mode:?} {covered}");
        }

        // A wireframe shows no surface, and a run that ends past the instances draws the ones there are
        let wires = frame(&device, &queue, &renderer, &camera, ShadingMode::Wireframe);
        assert!(wires.iter().all(|pixel| *pixel == background));
        renderer.set_world(&device, &queue, &instances[..1], draws(&device, 0..9));
        let one = frame(&device, &queue, &renderer, &camera, ShadingMode::Material);
        assert_ne!(one[pixel_of(&camera, red_at)], background);
        assert_eq!(one[pixel_of(&camera, green_at)], background);
        // A run of no instance and a mesh of no index are no draw at all
        let whole = square(&device, 0.3, material.clone());
        let (vertices, indices) = (whole.vertices.clone(), whole.indices.clone());
        let mut none = draws(&device, 1..1);
        none.push(WorldDraw {
            mesh: Arc::new(WorldMesh::shared(
                vertices,
                indices,
                0,
                0,
                0,
                material.clone(),
            )),
            instances: 0..1,
        });
        renderer.set_world(&device, &queue, &instances, none);
        assert!(!renderer.has_world());

        renderer.set_world(&device, &queue, &[], Vec::new());
        assert!(!renderer.has_world());
        let emptied = frame(&device, &queue, &renderer, &camera, ShadingMode::Material);
        assert!(emptied.iter().all(|pixel| *pixel == background));
        assert!(
            pollster::block_on(validation.pop()).is_none(),
            "the device accepted every draw"
        );
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn meshes_that_are_ranges_of_one_pair_of_buffers_draw_their_own_range_and_go_out_together_while_their_material_is_one(
) {
    let test = "meshes_that_are_ranges_of_one_pair_of_buffers";
    for (device, queue) in devices(test) {
        let mut renderer =
            ViewportRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, SIZE, SIZE);
        renderer.set_background(BACKGROUND);
        let camera = overhead();
        let places = [[-1.0, -1.0], [1.0, 0.5], [-1.0, 1.0]];
        // One page of vertices and one of indices: three squares, the indices of each counted from its own first vertex
        let vertices: Vec<MeshVertex> = places.iter().flat_map(|at| corners(*at, 0.3)).collect();
        let indices: Vec<u32> = places.iter().flat_map(|_| SQUARE).collect();
        let page = |contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents,
                usage,
            })
        };
        let vertex_page = page(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX);
        let index_page = page(bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX);
        let opaque = plain(&device, &renderer, WorldBucket::Opaque);
        let blue_map = texel(&device, &queue, [0, 0, 255, 255]);
        let mapped =
            renderer.world_material(&device, Some(&blue_map), None, None, WorldBucket::Opaque);
        // A material is its maps and its bucket: a clone is the same, and another made of the same maps is not
        assert!(opaque == plain(&device, &renderer, WorldBucket::Opaque));
        assert_eq!(opaque.key(), opaque.clone().key());
        assert!(opaque != plain(&device, &renderer, WorldBucket::Cutout));
        assert!(
            mapped
                != renderer.world_material(&device, Some(&blue_map), None, None, mapped.bucket())
        );
        assert_ne!(mapped.key(), opaque.key());
        let part = |nth: u32, material: &WorldMaterial| WorldDraw {
            mesh: Arc::new(WorldMesh::shared(
                vertex_page.clone(),
                index_page.clone(),
                nth as i32 * 4,
                nth * 6,
                6,
                material.clone(),
            )),
            instances: nth..nth + 1,
        };
        let instances =
            [RED, [0.0, 1.0, 0.0, 1.0], WHITE].map(|tint| WorldInstance::of(Mat4::IDENTITY, tint));
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        renderer.set_world(
            &device,
            &queue,
            &instances,
            vec![part(0, &opaque), part(1, &opaque), part(2, &mapped)],
        );
        // The first two are one call; the third samples other maps, which the cast of a shadow has no use for
        assert_eq!(renderer.world.batches, [0..2, 2..3]);
        assert_eq!(
            renderer.world.cast_batches,
            std::iter::once(0..3).collect::<Vec<_>>()
        );
        let image = frame(&device, &queue, &renderer, &camera, ShadingMode::Material);
        let at = |place: [f32; 2]| image[pixel_of(&camera, Vec3::new(place[0], place[1], 0.0))];
        let (red, green, blue) = (at(places[0]), at(places[1]), at(places[2]));
        assert!(red[0] > 100 && red[1] == 0 && red[2] == 0, "{red:?}");
        assert!(
            green[1] > 100 && green[0] == 0 && green[2] == 0,
            "{green:?}"
        );
        assert!(blue[2] > 100 && blue[0] == 0 && blue[1] == 0, "{blue:?}");
        let covered = image.iter().filter(|pixel| **pixel != image[0]).count();
        assert!((60..600).contains(&covered), "{covered}");

        // Handed over in another order, the draws are put in the order of their buckets, and what does not cast is no part of the cast
        let decal = plain(&device, &renderer, WorldBucket::Decal);
        let cutout = plain(&device, &renderer, WorldBucket::Cutout);
        renderer.set_world(
            &device,
            &queue,
            &instances,
            vec![part(0, &decal), part(1, &cutout), part(2, &opaque)],
        );
        let order: Vec<WorldBucket> = (renderer.world.draws.iter())
            .map(|draw| draw.mesh.material().bucket())
            .collect();
        assert_eq!(
            order,
            [WorldBucket::Opaque, WorldBucket::Cutout, WorldBucket::Decal]
        );
        assert_eq!(renderer.world.batches, [0..1, 1..2, 2..3]);
        assert_eq!(renderer.world.cast_batches, [0..1, 1..2]);
        let image = frame(&device, &queue, &renderer, &camera, ShadingMode::Material);
        let covered = image.iter().filter(|pixel| **pixel != image[0]).count();
        assert!((60..600).contains(&covered), "{covered}");
        assert!(
            pollster::block_on(validation.pop()).is_none(),
            "the device accepted every draw"
        );
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_fade_leaves_its_share_of_the_pixels_of_an_instance_and_a_cutout_only_the_texels_that_cover() {
    let test = "a_fade_leaves_its_share_of_the_pixels_of_an_instance";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let mut renderer =
        ViewportRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, SIZE, SIZE);
    renderer.set_background(BACKGROUND);
    let camera = overhead();
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let material = |bucket, alpha: u8| {
        let colour = texel(&device, &queue, [255, 255, 255, alpha]);
        renderer.world_material(&device, Some(&colour), None, None, bucket)
    };
    let (opaque, thin) = (
        material(WorldBucket::Opaque, 255),
        material(WorldBucket::Opaque, 40),
    );
    let (cut_whole, cut_thin) = (
        material(WorldBucket::Cutout, 255),
        material(WorldBucket::Cutout, 40),
    );
    let blend_thin = material(WorldBucket::Blend, 40);
    // How many pixels a square of one metre a side covers, drawn with `material` and faded to `fade`
    let mut covered = |material: WorldMaterial, fade: f32| {
        renderer.set_world(
            &device,
            &queue,
            &[WorldInstance::of(Mat4::IDENTITY, [1.0, 1.0, 1.0, fade])],
            vec![WorldDraw {
                mesh: square(&device, 0.5, material),
                instances: 0..1,
            }],
        );
        let image = frame(&device, &queue, &renderer, &camera, ShadingMode::Material);
        image.iter().filter(|pixel| **pixel != image[0]).count()
    };
    let whole = covered(opaque.clone(), 1.0);
    assert!((60..400).contains(&whole), "{whole}");
    // The fade is no alpha: the pixels it leaves are drawn whole, in the share it states
    let half = covered(opaque.clone(), 0.5);
    assert!(
        half * 5 > whole * 2 && half * 5 < whole * 3,
        "{half} of {whole}"
    );
    let quarter = covered(opaque.clone(), 0.25);
    assert!(quarter > 0 && quarter < half, "{quarter} of {whole}");
    assert_eq!(covered(opaque, 0.0), 0);
    // What is opaque draws whatever its coverage, a cutout only what covers, and both fade alike
    assert_eq!(covered(thin, 1.0), whole);
    assert_eq!(covered(cut_whole.clone(), 1.0), whole);
    assert_eq!(covered(cut_thin, 1.0), 0);
    assert_eq!(covered(cut_whole, 0.5), half);
    // What blends shows through, and fades by the same pixels
    assert_eq!(covered(blend_thin.clone(), 1.0), whole);
    assert_eq!(covered(blend_thin, 0.5), half);
    assert!(
        pollster::block_on(validation.pop()).is_none(),
        "the device accepted every draw"
    );
}

/// Whether every channel of two pixels is within `tolerance` of the other's.
fn near(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= tolerance)
}

/// A viewport whose Shaded frame is the game's deferred one with no surface of its own, lit by a sun that stands towards `sun`.
fn deferred(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    sun: Vec3,
) -> ViewportRenderer {
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = ViewportRenderer::new(device, queue, format, SIZE, SIZE);
    renderer.set_background(BACKGROUND);
    renderer.set_game_sun(Some(sun));
    let report = renderer
        .prepare_deferred(device, queue, pack, &[])
        .expect("the deferred passes prepare");
    assert_eq!(report, (0, Vec::new()));
    renderer
}

/// The four targets of the G-buffer of the frame `renderer` drew last, at the pixel `at`.
fn words(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &ViewportRenderer,
    at: usize,
) -> Vec<[u8; 4]> {
    renderer
        .gbuffer_textures()
        .into_iter()
        .map(|target| {
            let read = read_back(device, queue, target, GBUFFER_FORMAT, SIZE);
            read.bytes.as_chunks::<4>().0[at]
        })
        .collect()
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_games_passes_light_an_instance_of_the_world_as_they_light_a_surface_of_the_default_effect() {
    use crate::game_shaders::lighting::Day;
    use crate::game_shaders::mesh::{Image, Material, Surface};
    use crate::game_shaders::synthetic::quad;
    let test = "the_games_passes_light_an_instance_of_the_world";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let camera = overhead();
    let middle = (SIZE / 2 * SIZE + SIZE / 2) as usize;
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    // A deferred frame of `surfaces`, of one instance of a square tinted `tint` where there is one, under `day` and its sky where there is one: the four targets of its G-buffer and the screen at the middle, and the screen at a corner
    let shaded = |surfaces: &[(Surface, Material)], tint: Option<f32>, day: Option<Day>| {
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut renderer = ViewportRenderer::new(&device, &queue, format, SIZE, SIZE);
        renderer.set_background(BACKGROUND);
        // One flat square shades nothing, and the pass that receives the shadow writes the target compared below
        renderer.set_shadows(false);
        let report = renderer
            .prepare_deferred(&device, &queue, &pack, surfaces)
            .expect("the deferred passes prepare");
        assert_eq!(report, (surfaces.len(), Vec::new()));
        assert!(renderer.has_deferred());
        if let Some(tint) = tint {
            let material = plain(&device, &renderer, WorldBucket::Opaque);
            renderer.set_world(
                &device,
                &queue,
                &[WorldInstance::of(Mat4::IDENTITY, [tint, tint, tint, 1.0])],
                vec![WorldDraw {
                    mesh: square(&device, 1.0, material),
                    instances: 0..1,
                }],
            );
        }
        if day.is_some() {
            renderer.set_day(day);
            renderer.set_sky_background(true);
            renderer.prepare_game_frame(&device, &queue);
        }
        let image = frame(&device, &queue, &renderer, &camera, ShadingMode::Shaded);
        let targets = words(&device, &queue, &renderer, middle);
        (targets, image[middle], image[0])
    };
    // The same square as a surface of the game's default effect, its texture of one grey
    let surface = |grey: u8| {
        let texture = Image {
            width: 1,
            height: 1,
            rgba: vec![grey, grey, grey, 255],
            srgb: false,
        };
        [(
            quad(0.0),
            Material {
                effect: "default".into(),
                textures: [("DiffuseSampler".to_string(), texture)].into(),
                ..Material::default()
            },
        )]
    };

    // A frame with no surface of its own and no world is the background, and its G-buffer is as it was cleared
    let (untouched, nothing, background) = shaded(&[], None, None);
    assert_eq!(untouched, [[0; 4]; 4]);
    assert!(near(nothing, background, 2), "{nothing:?} {background:?}");

    let (world, lit, corner) = shaded(&[], Some(1.0), None);
    let (game, game_lit, _) = shaded(&surface(255), None, None);
    // The albedo, the normal as half of it and a half, a matte specular the sun reaches, and the whole of the natural ambient light
    let written = [
        [255, 255, 255, 255],
        [127, 127, 255, 0],
        [0, 0, 250, 255],
        [255, 0, 0, 255],
    ];
    for (target, want) in world.iter().zip(written) {
        assert!(near(*target, want, 1), "{world:?}");
    }
    // Which is what the game's own effect writes, but for the alpha it gives a normal straight up and the artificial light it takes
    let (mut same, mut game_same) = (world.clone(), game.clone());
    for targets in [&mut same, &mut game_same] {
        targets[1][3] = 0;
        targets[3][1] = 0;
    }
    for (target, want) in same.iter().zip(game_same) {
        assert!(near(*target, want, 1), "{world:?} {game:?}");
    }
    assert!(
        lit[..3].iter().all(|c| *c > 30),
        "the world is lit: {lit:?}"
    );
    assert!(!near(lit, background, 8), "{lit:?} {background:?}");
    assert!(near(corner, background, 2), "{corner:?} {background:?}");
    assert!(near(lit, game_lit, 3), "{lit:?} {game_lit:?}");

    // The lighting squares the albedo: a tint of a quarter is stored as a half, as a texture of a half is, and lit as it
    let grey = 128.0 / 255.0;
    let (world, lit, _) = shaded(&[], Some(grey * grey), None);
    let (game, game_lit, _) = shaded(&surface(128), None, None);
    assert!(near(world[0], [128, 128, 128, 255], 1), "{world:?}");
    assert!(near(world[0], game[0], 1), "{world:?} {game:?}");
    assert!(near(lit, game_lit, 3), "{lit:?} {game_lit:?}");

    // Under a day, with its occlusion over the world and its sky round it, the frame of no surface is lit as well
    let mut keyframe = or_io::gta5::data::timecycle::Keyframe::default();
    keyframe.set("ssao_inten", 4.0);
    let day = Day {
        keyframe,
        ..Day::default()
    };
    let (_, lit, sky) = shaded(&[], Some(1.0), Some(day));
    assert!(lit[..3].iter().all(|c| *c > 30), "{lit:?}");
    assert!(!near(lit, sky, 8), "{lit:?} {sky:?}");
    assert!(
        pollster::block_on(validation.pop()).is_none(),
        "the device accepted every pass"
    );
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_maps_and_the_bucket_of_a_material_reach_the_g_buffer_in_the_words_the_lighting_reads() {
    let test = "the_maps_and_the_bucket_of_a_material_reach_the_g_buffer";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let camera = overhead();
    let middle = (SIZE / 2 * SIZE + SIZE / 2) as usize;
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut renderer = deferred(&device, &queue, &pack, Vec3::new(0.3, 0.2, 0.9).normalize());
    // The pass that receives the shadow writes a target read below
    renderer.set_shadows(false);
    let material = |maps: [Option<[u8; 4]>; 3], bucket| {
        let [colour, normal, specular] =
            maps.map(|map| map.map(|rgba| texel(&device, &queue, rgba)));
        renderer.world_material(
            &device,
            colour.as_ref(),
            normal.as_ref(),
            specular.as_ref(),
            bucket,
        )
    };
    let opaque = material([None; 3], WorldBucket::Opaque);
    let mapped = material(
        [
            Some([64, 64, 64, 255]),
            Some([204, 128, 255, 255]),
            Some([64, 0, 0, 255]),
        ],
        WorldBucket::Opaque,
    );
    let glowing = material([None; 3], WorldBucket::Emissive);
    let covering = material([Some([255; 4]), None, None], WorldBucket::Cutout);
    let clear = material([Some([255, 255, 255, 40]), None, None], WorldBucket::Cutout);
    let stain = material([Some([0, 255, 0, 128]), None, None], WorldBucket::Decal);
    let glass = material([Some([0, 0, 255, 128]), None, None], WorldBucket::Blend);
    // A frame of squares of one metre a side about the origin, each with its material and its tint, all in one plane: the G-buffer and the screen at the middle, and the screen at a corner
    let mut drawn = |squares: &[(&WorldMaterial, [f32; 4])]| {
        let instances: Vec<WorldInstance> = (squares.iter())
            .map(|(_, tint)| WorldInstance::of(Mat4::IDENTITY, *tint))
            .collect();
        let draws = (squares.iter().zip(0..))
            .map(|((material, _), at)| WorldDraw {
                mesh: square(&device, 0.5, (*material).clone()),
                instances: at..at + 1,
            })
            .collect();
        renderer.set_world(&device, &queue, &instances, draws);
        let image = frame(&device, &queue, &renderer, &camera, ShadingMode::Shaded);
        (
            words(&device, &queue, &renderer, middle),
            image[middle],
            image[0],
        )
    };

    // A colour of a quarter is stored as a half; a normal map that leans three fifths along the texture's u, which runs along x here, leans the normal so; a specular map of a quarter in red and one in alpha is stored as the roots of a quarter and of a hundred in five hundred and twelve
    let (targets, lit, background) = drawn(&[(&mapped, WHITE)]);
    let want = [
        [128, 128, 128, 255],
        [204, 128, 230, 0],
        [128, 113, 250, 255],
        [255, 0, 0, 255],
    ];
    for (target, want) in targets.iter().zip(want) {
        assert!(near(*target, want, 2), "{targets:?}");
    }
    assert!(!near(lit, background, 8), "{lit:?} {background:?}");

    // An emissive surface is an opaque one that states the light it gives, a sixteenth of the most the word holds
    let (plain_words, plain_lit, _) = drawn(&[(&opaque, [0.5, 0.5, 0.5, 1.0])]);
    let (targets, lit, _) = drawn(&[(&glowing, [0.5, 0.5, 0.5, 1.0])]);
    assert!(near(targets[3], [255, 0, 16, 255], 1), "{targets:?}");
    assert_eq!(targets[..3], plain_words[..3]);
    assert!(
        (0..3).all(|c| lit[c] >= plain_lit[c]) && lit != plain_lit,
        "{lit:?} {plain_lit:?}"
    );

    // A cutout writes where its texels cover and nowhere else
    let (targets, ..) = drawn(&[(&covering, WHITE)]);
    assert!(near(targets[0], [255; 4], 1), "{targets:?}");
    let (targets, lit, background) = drawn(&[(&clear, WHITE)]);
    assert_eq!(targets, [[0; 4]; 4]);
    assert!(near(lit, background, 2), "{lit:?} {background:?}");

    // A decal of half a green on a red surface it lies in the plane of: the colour is half of each as the target stores them, and the alpha of every target and the ambient words are the surface's
    let (targets, ..) = drawn(&[(&stain, WHITE), (&opaque, RED)]);
    assert!(near(targets[0], [127, 128, 0, 255], 2), "{targets:?}");
    assert!(near(targets[1], [127, 127, 255, 0], 1), "{targets:?}");
    assert!(near(targets[2], [0, 0, 250, 255], 1), "{targets:?}");
    assert!(near(targets[3], [255, 0, 0, 255], 1), "{targets:?}");
    // On nothing it marks no pixel, and the frame shows none of it
    let (_, lit, background) = drawn(&[(&stain, WHITE)]);
    assert!(near(lit, background, 2), "{lit:?} {background:?}");

    // What blends is no part of the G-buffer: it is drawn over the lit frame, by its coverage
    let (under, under_lit, _) = drawn(&[(&opaque, RED)]);
    let (targets, lit, _) = drawn(&[(&glass, WHITE), (&opaque, RED)]);
    assert_eq!(targets, under);
    assert!(
        lit[2] > under_lit[2] + 20 && lit[0] + 20 < under_lit[0] && lit[0] > 20,
        "{lit:?} {under_lit:?}"
    );
    assert!(
        pollster::block_on(validation.pop()).is_none(),
        "the device accepted every pass"
    );
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn an_instance_of_the_world_shades_the_one_below_it_within_the_reach_of_the_shadow_and_not_past_it()
{
    let test = "an_instance_of_the_world_shades_the_one_below_it";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = real_pack(test) else {
        return;
    };
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    // The light travels towards the negative x and down, half a unit across for each it falls
    let mut renderer = deferred(&device, &queue, &pack, Vec3::new(1.0, 0.0, 2.0).normalize());
    let material = plain(&device, &renderer, WorldBucket::Opaque);
    // How much of the sun the ground keeps where the top throws its shadow and where it does not, by the alpha of the G-buffer's self-shadow: a ground of eight units a side and, a unit over it, a top of one, all `scale` times as large, seen from five such units above, the top faded to `fade`
    let kept = |renderer: &mut ViewportRenderer, scale: f32, fade: f32| {
        let camera = overhead_from(5.0 * scale);
        let sized = Mat4::from_scale(Vec3::new(scale, scale, 1.0));
        let instances = [
            WorldInstance::of(sized, WHITE),
            WorldInstance::of(
                Mat4::from_translation(Vec3::Z * scale) * sized,
                [1.0, 1.0, 1.0, fade],
            ),
        ];
        let draws = vec![
            WorldDraw {
                mesh: square(&device, 4.0, material.clone()),
                instances: 0..1,
            },
            WorldDraw {
                mesh: square(&device, 0.5, material.clone()),
                instances: 1..2,
            },
        ];
        renderer.set_world(&device, &queue, &instances, draws);
        let _ = frame(&device, &queue, renderer, &camera, ShadingMode::Shaded);
        // The top, from x -0.5 to 0.5 a unit up, throws its shadow from -1 to 0, and hides the ground as far as -0.71 from this camera
        let at = |x: f32| {
            let pixel = pixel_of(&camera, Vec3::new(x * scale, 0.0, 0.0));
            words(&device, &queue, renderer, pixel)[2][3]
        };
        (at(-0.85), at(0.8))
    };
    let (shaded, sunlit) = kept(&mut renderer, 1.0, 1.0);
    assert!(shaded < 60, "shaded ground {shaded}");
    assert!(sunlit > 200, "sunlit ground {sunlit}");
    // A top faded to nothing casts nothing, and one faded to a half casts half a shadow
    let (shaded, _) = kept(&mut renderer, 1.0, 0.0);
    assert!(shaded > 200, "under a top that is not drawn {shaded}");
    let (half, _) = kept(&mut renderer, 1.0, 0.5);
    assert!((40..215).contains(&half), "under a top half drawn {half}");
    // With the shadow off the sun reaches everything
    renderer.set_shadows(false);
    let (shaded, sunlit) = kept(&mut renderer, 1.0, 1.0);
    assert!(shaded > 200 && sunlit > 200, "{shaded} {sunlit}");
    // Past the reach of the shadow there is none: the same scene a hundred times as large is seen from five hundred metres, further than the shadow is cast
    renderer.set_shadows(true);
    let (shaded, sunlit) = kept(&mut renderer, 100.0, 1.0);
    assert!(shaded > 200 && sunlit > 200, "{shaded} {sunlit}");
    assert!(
        pollster::block_on(validation.pop()).is_none(),
        "the device accepted every pass"
    );
}
