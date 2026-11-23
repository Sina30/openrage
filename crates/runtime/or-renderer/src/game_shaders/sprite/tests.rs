use super::*;
use crate::game_shaders::frame::Rendered;
use crate::game_shaders::mesh::Image;
use crate::game_shaders::synthetic::sprite_pack;
use crate::game_shaders::test_pack::{draw_into, gpu};
use glam::Mat4;
use std::sync::Arc;

const SIZE: u32 = 16;

/// The camera of the scene is nothing: a point's place on the target is its own x and y.
fn flat() -> EngineConstants {
    EngineConstants {
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        ..Default::default()
    }
}

fn kind() -> EffectKind {
    EffectKind {
        technique: "RGBA".into(),
        depth_test: true,
        ..Default::default()
    }
}

/// `sprites` of `kind` drawn into a fresh target, or why they could not be.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    kind: &EffectKind,
    sprites: &[Sprite],
) -> Result<Rendered, EffectError> {
    let target = Target::default();
    let mut pass =
        SpritePass::prepare(&mut EffectCache::new(), device, queue, pack, &target, kind)?;
    pass.update(device, queue, &flat(), sprites);
    Ok(draw_into(device, queue, &target, SIZE, &mut |render| {
        pass.record(render)
    }))
}

fn at(rendered: &Rendered, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIZE + x) * 4) as usize;
    [
        rendered.bytes[i],
        rendered.bytes[i + 1],
        rendered.bytes[i + 2],
        rendered.bytes[i + 3],
    ]
}

fn red(centre: Vec3, right: Vec3, up: Vec3) -> Sprite {
    Sprite::new(centre, right, up, [1.0, 0.0, 0.0, 1.0])
}

/// Where the vertex stage puts the corner whose parameters are `s` and `t`, from the values `sprite_value` gives it.
fn corner_of(sprite: &Sprite, s: f32, t: f32) -> Vec3 {
    let v = |index| sprite_value("TEXCOORD", index, sprite);
    let (v6, v7, v8) = (v(6), v(7), v(8));
    let a = Vec3::new(v7[0], v7[1], v7[2]);
    let w = Vec3::new(v6[3], v7[3], v8[3]);
    let c = Vec3::new(v8[0], v8[1], v8[2]);
    let d = Vec3::new(v6[0], v6[1], v6[2]);
    sprite.centre + a + w + (c - w) * s + (d - a) * t
}

#[test]
fn the_corners_the_stage_makes_of_a_sprite_are_its_centre_plus_or_minus_right_and_up() {
    let sprite = red(
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(0.5, 0.25, -0.5),
        Vec3::new(-0.1, 0.4, 0.2),
    );
    let (c, r, u) = (sprite.centre, sprite.right, sprite.up);
    assert!(corner_of(&sprite, 0.0, 0.0).abs_diff_eq(c - r - u, 1e-6));
    assert!(corner_of(&sprite, 1.0, 0.0).abs_diff_eq(c + r - u, 1e-6));
    assert!(corner_of(&sprite, 0.0, 1.0).abs_diff_eq(c - r + u, 1e-6));
    assert!(corner_of(&sprite, 1.0, 1.0).abs_diff_eq(c + r + u, 1e-6));
}

#[test]
fn a_frames_texture_coordinates_run_over_its_cell_from_the_top_left() {
    let cell = [0.25, 0.5, 0.5, 0.75];
    let mut sprite = red(Vec3::ZERO, Vec3::X, Vec3::Y);
    sprite.frame_a = cell;
    let v = sprite_value("TEXCOORD", 10, &sprite);
    // The stage: u = x + s * (y - x), v = w + t * (z - w), with t from the bottom edge
    let uv = |s: f32, t: f32| [v[0] + s * (v[1] - v[0]), v[3] + t * (v[2] - v[3])];
    assert_eq!(
        uv(0.0, 1.0),
        [0.25, 0.5],
        "the top left is the cell's corner"
    );
    assert_eq!(
        uv(1.0, 0.0),
        [0.5, 0.75],
        "the bottom right is the opposite one"
    );
}

#[test]
fn only_the_corner_is_read_once_per_vertex() {
    assert_eq!(
        corner_value("TEXCOORD", 3, [2.0, 0.0]),
        [2.0, 0.0, 0.0, 0.0]
    );
    for (semantic, index) in [
        ("TEXCOORD", 0),
        ("TEXCOORD", 6),
        ("POSITION", 0),
        ("COLOR", 0),
    ] {
        assert_eq!(corner_value(semantic, index, [2.0, 2.0]), [0.0; 4]);
    }
}

#[test]
fn a_sprite_covers_the_rectangle_its_centre_and_half_extents_say_and_is_the_colour_it_was_given() {
    let Some((device, queue)) = gpu("a_sprite_covers_the_rectangle") else {
        return;
    };
    let dir = sprite_pack("RGBA", None);
    let pack = GamePack::open(dir.path()).unwrap();
    // x from -0.3 to 0.7 and y from -0.25 to 0.25 of a target that runs from -1 to 1
    let sprite = red(
        Vec3::new(0.2, 0.0, 0.0),
        Vec3::new(0.5, 0.0, 0.0),
        Vec3::new(0.0, 0.25, 0.0),
    );
    let rendered = render(&device, &queue, &pack, &kind(), &[sprite]).unwrap();
    // Columns 6 to 13 and rows 6 to 9 have their centres inside it
    assert_eq!(rendered.touched(), 8 * 4);
    assert_eq!(at(&rendered, 8, 8), [255, 0, 0, 255]);
    for (x, y) in [(4, 8), (14, 8), (8, 4), (8, 11)] {
        assert_eq!(at(&rendered, x, y), [0; 4], "{x},{y} is outside it");
    }
}

#[test]
fn a_texture_is_drawn_with_its_top_at_the_top_of_the_sprite() {
    let Some((device, queue)) = gpu("a_texture_is_drawn_with_its_top_at_the_top") else {
        return;
    };
    let dir = sprite_pack("RGBA", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let mut kind = kind();
    // A red top row over a green one
    kind.texture = Some(Arc::new(Image {
        width: 1,
        height: 2,
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        srgb: false,
    }));
    let sprite = Sprite::new(Vec3::ZERO, Vec3::X * 0.5, Vec3::Y * 0.5, [1.0; 4]);
    let rendered = render(&device, &queue, &pack, &kind, &[sprite]).unwrap();
    let top = at(&rendered, 8, 5);
    let bottom = at(&rendered, 8, 10);
    assert!(top[0] > 200 && top[1] < 60, "the top is red: {top:?}");
    assert!(
        bottom[1] > 200 && bottom[0] < 60,
        "the bottom is green: {bottom:?}"
    );
}

#[test]
fn a_kind_that_blends_mixes_the_sprite_with_what_is_behind_and_one_that_does_not_replaces_it() {
    let Some((device, queue)) = gpu("a_kind_that_blends_mixes_the_sprite") else {
        return;
    };
    let dir = sprite_pack("RGBA", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let sprite = Sprite::new(
        Vec3::ZERO,
        Vec3::X * 0.5,
        Vec3::Y * 0.5,
        [1.0, 0.0, 0.0, 0.5],
    );
    let near = |got: [u8; 4], want: [u8; 4]| got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1);
    let replaced = at(
        &render(&device, &queue, &pack, &kind(), &[sprite]).unwrap(),
        8,
        8,
    );
    assert!(near(replaced, [255, 0, 0, 128]), "{replaced:?}");
    let blended_kind = EffectKind {
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        ..kind()
    };
    let blended = at(
        &render(&device, &queue, &pack, &blended_kind, &[sprite]).unwrap(),
        8,
        8,
    );
    assert!(near(blended, [128, 0, 0, 128]), "{blended:?}");
}

#[test]
fn a_constant_the_kind_sets_reaches_the_shader_over_what_the_effect_declares() {
    let Some((device, queue)) = gpu("a_constant_the_kind_sets_reaches_the_shader") else {
        return;
    };
    let dir = sprite_pack("RGBA", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let sprite = Sprite::new(Vec3::ZERO, Vec3::X * 0.5, Vec3::Y * 0.5, [1.0; 4]);
    let declared = render(&device, &queue, &pack, &kind(), &[sprite]).unwrap();
    assert_eq!(at(&declared, 8, 8), [255; 4], "the effect declares one");
    let mut set = kind();
    set.values = vec![("gSuperAlpha".into(), vec![0.5; 4])];
    let halved = at(
        &render(&device, &queue, &pack, &set, &[sprite]).unwrap(),
        8,
        8,
    );
    assert!(halved.iter().all(|c| c.abs_diff(128) <= 1), "{halved:?}");
}

#[test]
fn a_pass_grows_to_hold_more_sprites_than_it_first_drew() {
    let Some((device, queue)) = gpu("a_pass_grows_to_hold_more_sprites") else {
        return;
    };
    let dir = sprite_pack("RGBA", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let mut pass = SpritePass::prepare(
        &mut EffectCache::new(),
        &device,
        &queue,
        &pack,
        &Target::default(),
        &kind(),
    )
    .unwrap();
    let small = |x: f32, y: f32| red(Vec3::new(x, y, 0.0), Vec3::X * 0.05, Vec3::Y * 0.05);
    pass.update(&device, &queue, &flat(), &[small(0.0, 0.0)]);
    assert_eq!(pass.drawn(), 1);
    let many: Vec<Sprite> = (0..40)
        .map(|i| small(-0.9 + 0.2 * (i % 10) as f32, -0.6 + 0.4 * (i / 10) as f32))
        .collect();
    pass.update(&device, &queue, &flat(), &many);
    assert_eq!(pass.drawn(), 40);
    pass.update(&device, &queue, &flat(), &[]);
    assert_eq!(pass.drawn(), 0);
}

#[test]
fn a_technique_the_pack_lacks_and_a_stage_that_reads_the_vertex_number_are_refused_by_name() {
    let Some((device, queue)) = gpu("a_technique_the_pack_lacks") else {
        return;
    };
    let dir = sprite_pack("RGBA", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let missing = EffectKind {
        technique: "RGBA_lit".into(),
        ..kind()
    };
    let error = render(&device, &queue, &pack, &missing, &[]).err().unwrap();
    assert!(matches!(error, EffectError::NoTechnique { technique, .. } if technique == "RGBA_lit"));

    let dir = sprite_pack("RGBA_proj", Some(("SV_VertexID", 11)));
    let projected = GamePack::open(dir.path()).unwrap();
    let proj = EffectKind {
        technique: "RGBA_proj".into(),
        ..kind()
    };
    let error = render(&device, &queue, &projected, &proj, &[])
        .err()
        .unwrap();
    assert!(matches!(error, EffectError::VertexNumber(name) if name == "RGBA_proj"));
}

/// The projection the viewport draws with: a nearer surface has the greater depth.
fn reversed(near: f32, far: f32) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, near / (far - near), -1.0),
        glam::Vec4::new(0.0, 0.0, near * far / (far - near), 0.0),
    )
}

/// How much of a white soft sprite `ahead` metres in front of a surface 20 metres from the camera is left where it covers the middle of the picture.
fn soft_alpha(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    ahead: f32,
    with_depth: bool,
) -> f32 {
    const PLANE: f32 = 20.0;
    let projection = reversed(0.1, 100.0);
    let engine = EngineConstants {
        view: Mat4::look_at_rh(Vec3::new(0.0, -10.0, 0.0), Vec3::ZERO, Vec3::Z),
        projection,
        screen: [SIZE as f32, SIZE as f32],
        ..Default::default()
    };
    // What the depth buffer holds for a surface at a distance: the nearer, the greater
    let held = |distance: f32| projection.w_axis.z / distance - projection.z_axis.z;
    let attachment = |format, usage| {
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
    let colour = attachment(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = attachment(
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let (colour_view, depth_view) = (
        colour.create_view(&Default::default()),
        depth.create_view(&Default::default()),
    );
    let target = Target {
        colour: wgpu::TextureFormat::Rgba8Unorm,
        depth: wgpu::TextureFormat::Depth32Float,
        ..Target::default()
    };
    let kind = EffectKind {
        technique: "RGBA_soft".into(),
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        depth: with_depth.then(|| depth_view.clone()),
        ..Default::default()
    };
    let mut pass =
        SpritePass::prepare(&mut EffectCache::new(), device, queue, pack, &target, &kind)
            .expect("the soft sprite pass is prepared");
    // The sprite faces the camera, four metres across, `ahead` metres in front of the surface
    let sprite = Sprite::new(
        Vec3::new(0.0, PLANE - 10.0 - ahead, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 2.0),
        [1.0; 4],
    );
    pass.update(device, queue, &engine, &[sprite]);
    let mut encoder = device.create_command_encoder(&Default::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(held(PLANE)),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    {
        let mut draw = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            // The depth is read by the sprite and tested by nothing, as the particles of the viewport read it
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
    let rendered = crate::game_shaders::frame::read_back(
        device,
        queue,
        &colour,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE,
    );
    at(&rendered, SIZE / 2, SIZE / 2)[0] as f32 / 255.0
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_soft_sprite_fades_out_where_it_meets_a_surface_and_not_where_it_is_well_in_front_of_it() {
    let test = "a_soft_sprite_fades_out";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let Some(pack) = crate::game_shaders::test_pack::real_pack(test) else {
        return;
    };
    let alpha =
        |ahead: f32, with_depth: bool| soft_alpha(&device, &queue, &pack, ahead, with_depth);
    let (far, touching) = (alpha(8.0, true), alpha(0.0, true));
    assert!(
        far > 0.7,
        "{far}: a sprite well in front of the surface is whole"
    );
    // One behind it is left to the depth test, as the fade is of the distance and not of which side it is on
    assert!(touching < 0.2, "{touching}: one on it is gone");
    // With no depth to read, the sprite is whole wherever the surface is
    assert!(alpha(0.0, false) > 0.7);
}
