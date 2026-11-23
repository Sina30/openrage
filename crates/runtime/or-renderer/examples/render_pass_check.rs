#![recursion_limit = "256"]
//! Renders a quad with every pass of a SPIR-V pack that `or-shader-tool bake-pack` wrote, with the engine's default constants, and counts which passes draw anything.
//!
//!     cargo run -p or-renderer --release --example render_pass_check -- <PACK> [--json <FILE>]

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use or_io::gta5::shaders::spirv_pack::Outcome;
use or_renderer::game_shaders::engine::EngineConstants;
use or_renderer::game_shaders::pack::GamePack;
use or_renderer::game_shaders::quad::{pair_of, render_pair};

const SIZE: u32 = 32;

/// The vertex program and the pixel program of a pass, by key.
type Pair = (String, Option<String>);

/// A message with each run of digits folded to `N` and cut to its first line, so equal causes count together.
fn fold(text: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for c in text.lines().next().unwrap_or("").chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('N');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out.chars().take(150).collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: render_pass_check <PACK> [--json <FILE>]");
        std::process::exit(1);
    };
    let json = (args.next().as_deref() == Some("--json"))
        .then(|| args.next().map(PathBuf::from))
        .flatten();
    let pack = GamePack::open(root).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap_or_else(|e| {
        eprintln!("no adapter: {e}");
        std::process::exit(1);
    });
    let name = adapter.get_info().name;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("render-pass-check"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap_or_else(|e| {
        eprintln!("no device: {e}");
        std::process::exit(1);
    });
    device.on_uncaptured_error(std::sync::Arc::new(|_| {}));
    std::panic::set_hook(Box::new(|_| {}));
    println!("device: {name}");

    // Each distinct pair whose programs all translated, and the passes of each technique that make one
    let manifest = pack.manifest();
    let translated = |key: &String| {
        matches!(
            manifest.programs.get(key).map(|p| &p.outcome),
            Some(Outcome::Translated { .. })
        )
    };
    let mut pairs: Vec<Pair> = Vec::new();
    let mut seen = HashSet::new();
    let mut by_technique: BTreeMap<String, Vec<Pair>> = BTreeMap::new();
    for effect in &manifest.effects {
        for technique in &effect.techniques {
            for pass in &technique.passes {
                let Some((vertex, pixel)) = pair_of(pass) else {
                    continue;
                };
                if !translated(vertex) || pixel.is_some_and(|p| !translated(p)) {
                    continue;
                }
                let pair = (vertex.clone(), pixel.cloned());
                if seen.insert(pair.clone()) {
                    pairs.push(pair.clone());
                }
                by_technique
                    .entry(technique.name.clone())
                    .or_default()
                    .push(pair);
            }
        }
    }

    let scene = EngineConstants::default();
    let next = AtomicUsize::new(0);
    let results: Mutex<BTreeMap<Pair, Result<usize, String>>> = Mutex::new(BTreeMap::new());
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(pair) = pairs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        render_pair(
                            &device,
                            &queue,
                            &pack,
                            &pair.0,
                            pair.1.as_deref(),
                            &scene,
                            SIZE,
                        )
                    }))
                    .unwrap_or_else(|_| {
                        Err(or_renderer::game_shaders::quad::QuadError::Device(
                            "panicked".into(),
                        ))
                    })
                    .map(|rendered| rendered.touched())
                    .map_err(|e| fold(&e.to_string()));
                    if let Ok(mut all) = results.lock() {
                        all.insert(pair.clone(), outcome);
                    }
                }
            });
        }
    });
    let results = results.into_inner().unwrap_or_default();

    let all_passes: usize = by_technique.values().map(Vec::len).sum();
    let count = |keep: &dyn Fn(&Result<usize, String>) -> bool| -> usize {
        by_technique
            .values()
            .flatten()
            .filter(|pair| keep(&results[*pair]))
            .count()
    };
    let drew = count(&|r| matches!(r, Ok(n) if *n > 0));
    let nothing = count(&|r| matches!(r, Ok(0)));
    let failed = count(&|r| r.is_err());
    println!(
        "of {all_passes} passes whose programs all translated, {drew} draw something onto the quad, {nothing} draw nothing and {failed} did not render, from {} distinct pairs",
        pairs.len()
    );
    let mut causes: BTreeMap<String, usize> = BTreeMap::new();
    for pair in by_technique.values().flatten() {
        if let Err(cause) = &results[pair] {
            *causes.entry(cause.clone()).or_default() += 1;
        }
    }
    let mut techniques: Vec<_> = by_technique.iter().collect();
    techniques.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    println!("\ntechniques with the most passes (passes, passes that draw something):");
    for (name, keys) in techniques.iter().take(20) {
        let ok = keys
            .iter()
            .filter(|k| matches!(&results[*k], Ok(n) if *n > 0))
            .count();
        println!("  {name:<36} {:>6} {ok:>6}", keys.len());
    }
    println!("\nwhat the passes that did not render stop at:");
    let mut sorted: Vec<_> = causes.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (cause, n) in sorted.iter().take(12) {
        println!("  {n:>6}  {cause}");
    }
    if let Some(path) = json {
        let value = serde_json::json!({
            "adapter": name,
            "passes": all_passes,
            "drew": drew,
            "drew_nothing": nothing,
            "not_rendered": failed,
            "distinct": pairs.len(),
            "causes": causes,
        });
        if let Err(e) = std::fs::write(
            &path,
            serde_json::to_string_pretty(&value).unwrap_or_default(),
        ) {
            eprintln!("{}: {e}", path.display());
            std::process::exit(1);
        }
    }
}
