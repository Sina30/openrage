//! What it costs the CPU to lay out and upload one frame of particle sprites with a pass of the game's own `ptfx_sprite`, from a baked pack.
//!
//!     cargo run -p or-renderer --release --example sprite_frame_cost -- <PACK> [PARTICLES_PER_BATCH] [BATCHES] [TECHNIQUE]

use std::sync::Arc;
use std::time::Instant;

use glam::Vec3;
use or_renderer::game_shaders::engine::EngineConstants;
use or_renderer::game_shaders::pack::GamePack;
use or_renderer::game_shaders::pipeline::Target;
use or_renderer::particle_sprites::GameSprites;
use or_renderer::particles::{
    ParticleAlignment, ParticleBatch, ParticleBlendMode, ParticleInstance,
};

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(root) = args.first() else {
        fail("usage: sprite_frame_cost <PACK> [PARTICLES_PER_BATCH] [BATCHES] [TECHNIQUE]");
    };
    let number = |at: usize, default: usize| {
        args.get(at).map_or(default, |s| {
            s.parse().unwrap_or_else(|_| fail("a number is wanted"))
        })
    };
    let (per_batch, batch_count) = (number(1, 5_000), number(2, 4));
    let technique = args.get(3).map_or("RGBA", String::as_str);
    let pack = Arc::new(GamePack::open(root).unwrap_or_else(|e| fail(e)));

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap_or_else(|e| fail(format!("no adapter: {e}")));
    println!("device: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("sprite-frame-cost"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap_or_else(|e| fail(format!("no device: {e}")));

    let batches: Vec<ParticleBatch> = (0..batch_count)
        .map(|b| {
            let instances = (0..per_batch)
                .map(|i| {
                    let t = i as f32 / per_batch as f32;
                    ParticleInstance {
                        position: [t * 10.0 - 5.0, b as f32, (t * 40.0).sin()],
                        dimensions: [0.25; 4],
                        rotation: [t * 6.0, 0.0, 0.0, 0.0],
                        color: [1.0, 0.8, 0.6, 0.5],
                        tex_frame_a: (i % 4) as f32,
                        ..Default::default()
                    }
                })
                .collect();
            let mut batch = ParticleBatch::new(
                ParticleBlendMode::Alpha,
                ParticleAlignment::Camera,
                instances,
            )
            .with_uv_atlas(2, 2);
            batch.shader_technique = technique.into();
            batch
        })
        .collect();

    let target = Target {
        colour: wgpu::TextureFormat::Rgba8UnormSrgb,
        depth: wgpu::TextureFormat::Depth32Float,
        depth_compare: wgpu::CompareFunction::GreaterEqual,
        blend: None,
        colour_attachments: Some(1),
    };
    let mut sprites = GameSprites::new(pack, target);
    let scene = EngineConstants::default();
    let eye = Vec3::new(0.0, -10.0, 2.0);

    // The first frame builds the passes; what follows is what a frame costs
    let first = Instant::now();
    sprites.prepare(&device, &queue, &batches, &scene, eye);
    let first = first.elapsed();
    let frames = 100;
    let started = Instant::now();
    for _ in 0..frames {
        sprites.prepare(&device, &queue, &batches, &scene, eye);
        queue.submit([]);
    }
    let each = started.elapsed() / frames;
    // What turning the particles into sprites costs of that, since it is done for each of them each frame
    let started = Instant::now();
    let mut made = 0;
    for _ in 0..frames {
        for batch in &batches {
            let sprites: Vec<_> = batch
                .instances
                .iter()
                .map(|i| or_renderer::particle_sprites::sprite_of(i, batch, eye))
                .collect();
            made += sprites.len();
        }
    }
    println!(
        "turning them into sprites alone: {:.3} ms a frame ({:.0} ns a sprite, {made} made)",
        (started.elapsed() / frames).as_secs_f64() * 1e3,
        started.elapsed().as_nanos() as f64 / made as f64,
    );
    println!(
        "{batch_count} batches of {per_batch} sprites of {technique}: {} passes, first frame {:.1} ms, then {:.3} ms a frame ({:.0} ns a sprite)",
        sprites.drawn_batches(),
        first.as_secs_f64() * 1e3,
        each.as_secs_f64() * 1e3,
        each.as_nanos() as f64 / (per_batch * batch_count) as f64,
    );
}
