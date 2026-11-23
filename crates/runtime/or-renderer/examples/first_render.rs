//! Renders a quad with one pass of the game's own shaders, from a baked pack, into a PNG.
//!
//!     cargo run -p or-renderer --release --example first_render -- <PACK> <EFFECT> <TECHNIQUE> <OUT.png> [--pass N]

use or_renderer::game_shaders::engine::EngineConstants;
use or_renderer::game_shaders::pack::GamePack;
use or_renderer::game_shaders::quad::{pair_of, render_pair};

const SIZE: u32 = 512;

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, effect, technique, out, rest @ ..] = &args[..] else {
        fail("usage: first_render <PACK> <EFFECT> <TECHNIQUE> <OUT.png> [--pass N]");
    };
    let pass_index: usize = match rest {
        [flag, n] if flag == "--pass" => {
            n.parse().unwrap_or_else(|_| fail("--pass takes a number"))
        }
        _ => 0,
    };
    let pack = GamePack::open(root).unwrap_or_else(|e| fail(e));
    let pass = pack
        .manifest()
        .effects
        .iter()
        .find(|e| &e.name == effect)
        .and_then(|e| e.techniques.iter().find(|t| &t.name == technique))
        .and_then(|t| t.passes.get(pass_index))
        .unwrap_or_else(|| fail(format!("no pass {pass_index} of {effect}.{technique}")));
    let (vertex, pixel) =
        pair_of(pass).unwrap_or_else(|| fail("the pass is not a vertex and a pixel stage"));

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap_or_else(|e| fail(format!("no adapter: {e}")));
    println!("device: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("first-render"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap_or_else(|e| fail(format!("no device: {e}")));

    let rendered = render_pair(
        &device,
        &queue,
        &pack,
        vertex,
        pixel.map(String::as_str),
        &EngineConstants::default(),
        SIZE,
    )
    .unwrap_or_else(|e| fail(e));
    println!(
        "{effect}.{technique}: {} of {} pixels are not black, in {} colours",
        rendered.touched(),
        SIZE * SIZE,
        rendered.colours()
    );
    if rendered.format != wgpu::TextureFormat::Rgba8Unorm {
        fail(format!(
            "the target is {:?}, which this does not write",
            rendered.format
        ));
    }
    image::RgbaImage::from_raw(SIZE, SIZE, rendered.bytes)
        .unwrap_or_else(|| fail("the image does not fit"))
        .save(out)
        .unwrap_or_else(|e| fail(e));
}
