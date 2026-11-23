//! Draws a sprite with a pass of the game's own `ptfx_sprite` from a baked pack into a PNG, and checks where it lands and which way up its texture goes.
//!
//!     cargo run -p or-renderer --release --example first_sprite -- <PACK> [TECHNIQUE] [OUT.png]

use glam::Vec3;
use or_renderer::game_shaders::effect_pass::{EffectCache, EffectKind};
use or_renderer::game_shaders::engine::EngineConstants;
use or_renderer::game_shaders::frame::read_back;
use or_renderer::game_shaders::mesh::Image;
use or_renderer::game_shaders::pack::GamePack;
use or_renderer::game_shaders::pipeline::Target;
use or_renderer::game_shaders::sprite::{Sprite, SpritePass};

const SIZE: u32 = 512;

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(root) = args.first() else {
        fail("usage: first_sprite <PACK> [TECHNIQUE] [OUT.png]");
    };
    let technique = args.get(1).map_or("RGBA", String::as_str);
    let pack = GamePack::open(root).unwrap_or_else(|e| fail(e));

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap_or_else(|e| fail(format!("no adapter: {e}")));
    println!("device: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("first-sprite"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap_or_else(|e| fail(format!("no device: {e}")));

    // A red top row over a green one, so that which way up the texture goes shows
    let kind = EffectKind {
        technique: technique.into(),
        depth_test: true,
        texture: Some(std::sync::Arc::new(Image {
            width: 1,
            height: 2,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            srgb: false,
        })),
        ..Default::default()
    };
    let target = Target::default();
    let mut pass = SpritePass::prepare(
        &mut EffectCache::new(),
        &device,
        &queue,
        &pack,
        &target,
        &kind,
    )
    .unwrap_or_else(|e| fail(format!("the sprite does not prepare: {e}")));
    let sprite = Sprite::new(
        Vec3::new(0.2, 0.3, 0.0),
        Vec3::new(0.5, 0.0, 0.0),
        Vec3::new(0.0, 0.25, 0.0),
        [1.0; 4],
    );
    pass.update(&device, &queue, &EngineConstants::default(), &[sprite]);

    let attachment = |format: wgpu::TextureFormat| {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    };
    let colour = attachment(target.colour);
    let depth = attachment(target.depth);
    let (colour_view, depth_view) = (
        colour.create_view(&Default::default()),
        depth.create_view(&Default::default()),
    );
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
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
        pass.record(&mut render);
    }
    queue.submit([encoder.finish()]);
    if let Some(error) = pollster::block_on(validation.pop()) {
        fail(format!("the device refused the draw: {error}"));
    }
    let rendered = read_back(&device, &queue, &colour, target.colour, SIZE);

    // The camera is at z = 3 looking at the origin with a 45 degree field of view, so a point at the origin's depth is `x / (3 * tan(22.5))` of the half-width from the middle
    let scale = 3.0 * (22.5f32).to_radians().tan();
    let column = |world: f32| (SIZE as f32 / 2.0) * (1.0 + world / scale);
    let row = |world: f32| (SIZE as f32 / 2.0) * (1.0 - world / scale);
    let (x0, x1) = (column(0.2 - 0.5), column(0.2 + 0.5));
    let (y0, y1) = (row(0.3 + 0.25), row(0.3 - 0.25));

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (u32::MAX, 0, u32::MAX, 0);
    for (i, pixel) in rendered.bytes.as_chunks::<4>().0.iter().enumerate() {
        if pixel.iter().any(|b| *b != 0) {
            let (x, y) = (i as u32 % SIZE, i as u32 / SIZE);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    println!(
        "ptfx_sprite.{technique}: {} of {} pixels touched, in {} colours",
        rendered.touched(),
        SIZE * SIZE,
        rendered.colours()
    );
    println!("landed x {min_x}..={max_x}, y {min_y}..={max_y}");
    println!("expected x {x0:.1}..{x1:.1}, y {y0:.1}..{y1:.1}");
    let close = |got: u32, want: f32| (got as f32 - want).abs() <= 2.0;
    let landed = rendered.touched() > 0
        && close(min_x, x0)
        && close(max_x + 1, x1)
        && close(min_y, y0)
        && close(max_y + 1, y1);

    // A quarter of the way down the sprite and a quarter of the way up, in its middle column
    let at = |y: f32| {
        let i = ((y as u32 * SIZE + (SIZE as f32 * 0.5 * (1.0 + 0.2 / scale)) as u32) * 4) as usize;
        [
            rendered.bytes[i],
            rendered.bytes[i + 1],
            rendered.bytes[i + 2],
        ]
    };
    let top = at(y0 + (y1 - y0) * 0.25);
    let bottom = at(y0 + (y1 - y0) * 0.75);
    println!("a quarter down {top:?}, a quarter up {bottom:?}");
    let upright = top[0] > top[1].saturating_add(60) && bottom[1] > bottom[0].saturating_add(60);

    if let Some(out) = args.get(2) {
        image::RgbaImage::from_raw(SIZE, SIZE, rendered.bytes)
            .unwrap_or_else(|| fail("the image does not fit"))
            .save(out)
            .unwrap_or_else(|e| fail(e));
    }
    if !landed {
        fail("the sprite did not land where its corners put it");
    }
    if !upright {
        fail("the texture is not drawn with its top at the top of the sprite");
    }
    println!("the sprite landed where its corners put it, with its texture the right way up");
}
