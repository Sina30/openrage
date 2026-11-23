#![recursion_limit = "256"]
//! Draws every pass of a SPIR-V pack that `or-shader-tool bake-pack` wrote, with stand-in resources, and counts what draws.
//!
//!     cargo run -p or-renderer --release --example draw_pass_check -- <PACK> [--json <FILE>]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{Outcome, Pass, SpirvPack};
use or_renderer::game_shaders::link::match_interpolation;
use or_renderer::game_shaders::pack::GamePack;
use or_renderer::game_shaders::pipeline::build_pass_pipeline;
use or_renderer::game_shaders::stand_in::{bind_groups, draw_triangle};

/// The vertex program and the pixel program of a pass, by key.
type Pair = (String, Option<String>);

/// A message with each run of digits folded to `N`, cut to its first line, so equal causes count together.
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

/// The programs of a pass when it binds a vertex stage and at most a pixel stage.
fn pair_of(pass: &Pass) -> Option<(&String, Option<&String>)> {
    let vertex = pass.stages.get(&ShaderStageKind::Vertex)?;
    let others = pass
        .stages
        .keys()
        .any(|s| !matches!(s, ShaderStageKind::Vertex | ShaderStageKind::Pixel));
    (!others).then(|| (vertex, pass.stages.get(&ShaderStageKind::Pixel)))
}

fn translated(pack: &SpirvPack, key: &str) -> bool {
    matches!(
        pack.programs.get(key).map(|p| &p.outcome),
        Some(Outcome::Translated { .. })
    )
}

fn open_device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|e| format!("no adapter: {e}"))?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("draw-pass-check"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .map_err(|e| format!("no device: {e}"))?;
    device.on_uncaptured_error(std::sync::Arc::new(|_| {}));
    Ok((device, queue, format!("{} ({:?})", info.name, info.backend)))
}

/// Link, build and draw one pair, and say why not.
fn try_pair(
    pack: &GamePack,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vertex: &str,
    pixel: Option<&str>,
) -> Result<(), String> {
    let mut vertex = pack.module(vertex).map_err(|e| fold(&e.to_string()))?;
    let pixel = pixel
        .map(|key| pack.module(key))
        .transpose()
        .map_err(|e| fold(&e.to_string()))?;
    if let Some(pixel) = &pixel {
        match_interpolation(&mut vertex, pixel);
    }
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let pass =
            build_pass_pipeline(device, &vertex, pixel.as_ref()).map_err(|e| e.to_string())?;
        let groups = bind_groups(device, queue, &pass);
        let _targets = draw_triangle(device, queue, &pass, &groups);
        Ok::<(), String>(())
    }));
    let validation = pollster::block_on(validation.pop());
    let internal = pollster::block_on(internal.pop());
    match made {
        Err(_) => Err("panicked".into()),
        Ok(Err(message)) => Err(fold(&message)),
        Ok(Ok(())) => match validation.or(internal) {
            None => Ok(()),
            Some(error) => Err(fold(
                error
                    .to_string()
                    .lines()
                    .filter(|l| {
                        !l.trim().is_empty()
                            && !l.trim_start().starts_with("In ")
                            && *l != "Validation Error"
                            && !l.contains("Caused by")
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
                    .as_str(),
            )),
        },
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: draw_pass_check <PACK> [--json <FILE>]");
        std::process::exit(1);
    };
    let json = (args.next().as_deref() == Some("--json"))
        .then(|| args.next().map(PathBuf::from))
        .flatten();
    let pack = match GamePack::open(root) {
        Ok(pack) => pack,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let (device, queue, adapter) = match open_device() {
        Ok(opened) => opened,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    std::panic::set_hook(Box::new(|_| {}));
    println!("device: {adapter}");

    // Each distinct pair whose programs were all translated, and the passes that make it
    let mut pairs: Vec<Pair> = Vec::new();
    let mut seen = HashSet::new();
    let mut passes_by_pair: HashMap<Pair, usize> = HashMap::new();
    let mut by_technique: BTreeMap<String, Vec<Pair>> = BTreeMap::new();
    let manifest = pack.manifest();
    for effect in &manifest.effects {
        for technique in &effect.techniques {
            for pass in &technique.passes {
                let Some((vertex, pixel)) = pair_of(pass) else {
                    continue;
                };
                if !translated(manifest, vertex) || pixel.is_some_and(|p| !translated(manifest, p))
                {
                    continue;
                }
                let key = (vertex.clone(), pixel.cloned());
                *passes_by_pair.entry(key.clone()).or_default() += 1;
                if seen.insert(key.clone()) {
                    pairs.push(key.clone());
                }
                by_technique
                    .entry(technique.name.clone())
                    .or_default()
                    .push(key);
            }
        }
    }

    let next = AtomicUsize::new(0);
    let results: Mutex<HashMap<Pair, Result<(), String>>> = Mutex::new(HashMap::new());
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(pair) = pairs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = try_pair(&pack, &device, &queue, &pair.0, pair.1.as_deref());
                    if let Ok(mut all) = results.lock() {
                        all.insert(pair.clone(), outcome);
                    }
                }
            });
        }
    });
    let results = results.into_inner().unwrap_or_default();

    let (mut passes, mut drawn) = (0usize, 0usize);
    let mut causes: BTreeMap<String, usize> = BTreeMap::new();
    for (pair, count) in &passes_by_pair {
        passes += count;
        match &results[pair] {
            Ok(()) => drawn += count,
            Err(cause) => *causes.entry(cause.clone()).or_default() += count,
        }
    }
    println!(
        "{drawn} of {passes} passes draw with stand-in resources, from {} distinct pairs",
        pairs.len()
    );
    let mut techniques: Vec<_> = by_technique.iter().collect();
    techniques.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    println!("\ntechniques with the most passes (passes, passes that draw):");
    for (name, keys) in techniques.iter().take(20) {
        let ok = keys.iter().filter(|k| results[*k].is_ok()).count();
        println!("  {name:<36} {:>6} {ok:>6}", keys.len());
    }
    println!("\nwhat the passes that do not draw stop at:");
    let mut sorted: Vec<_> = causes.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (cause, n) in sorted.iter().take(15) {
        println!("  {n:>6}  {cause}");
    }
    if let Some(path) = json {
        let value = serde_json::json!({
            "adapter": adapter,
            "passes": passes,
            "drawn": drawn,
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
