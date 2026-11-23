//! `openrage-cli mesh`: a drawable out to glTF, and the census that says what
//! the exporter is allowed to assume.
//!
//! Triangle winding, primitive type and channel presence are things an
//! interchange writer has to commit to, and each is a fact about shipped files
//! rather than about the format; `mesh census` measures them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use or_interchange::extract::{channel_lanes, Extracted};
use or_interchange::gltf::{self, Container};
use or_interchange::{Destination, ExtractOptions, ImportOptions, LodSelection, SceneBuilder};
use or_io::codec::mesh::{GrcFvfChannel, VertexReader};
use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::decl::{Gen8, Gen9, Shared};
use or_io::gta5::drawable::{each_defect, DrawableTarget, MeshDefect};
use or_io::gta5::{File, GrmGeometry, GtaDrawable, GtaFragType, PgDictionary, RmcDrawable};
use or_io::Generation;
use or_title::install::crawl::{self, ControlFlow};

use super::keys::resolve_crypto;
use super::search::resolve_game_root;

const DRAWABLE: [&str; 3] = ["ydr", "ydd", "yft"];

pub fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("export") => export(&args[1..]),
        Some("import") => import(&args[1..]),
        Some("channels") => channels(&args[1..]),
        Some("graft") => graft(&args[1..]),
        Some("retuck") => retuck(&args[1..]),
        Some("underwear") => underwear(&args[1..]),
        Some("unskin") => unskin(&args[1..]),
        Some("cutout") => cutout(&args[1..]),
        Some("census") => census(&args[1..]),
        Some("-h") | Some("--help") | Some("help") | None => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("Error: unknown subcommand '{other}'");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!(
        "openrage-cli mesh — drawable geometry out to an interchange format\n\
         \n\
         USAGE:\n    \
         openrage-cli mesh export <input.ydr|.ydd|.yft> <output.glb|.gltf> [options]\n    \
         openrage-cli mesh import <input.ydr|.ydd|.yft> <mesh.glb|.gltf|.fbx> <output> [options]\n    \
         openrage-cli mesh channels <input.ydr|.ydd|.yft>\n    \
         openrage-cli mesh graft <garment.ydd> <donor.ydd> <output.ydd> [--entry N] [--donor-entry N] [--prefix NAME | --all] [--marker NAME] [--skin-uv X,Y,W,H] [--target-uv SU,SV,OU,OV] [--texture FILE[=NAME]] [--tuck [--hide-island U]]\n    \
         openrage-cli mesh census --dir DIR [--limit N]\n    \
         openrage-cli mesh census [--game DIR] [--keys FILE] [--limit N]\n\
         \n\
         EXPORT OPTIONS:\n    \
         --lods all      every LOD the drawable ships, not only the highest\n    \
         --no-textures   skip decoding the drawable's own texture dictionary\n    \
         --entry N       one drawable of a .ydd or .yft rather than all of them.\n                    \
         ⚠ An import replaces one drawable, so a mesh meant to go\n                    \
         back has to be exported this way\n\
         \n\
         The container follows the output extension: `.glb` is one self-contained\n\
         file, `.gltf` is JSON beside a `.bin` and one `.png` per texture.\n\
         \n\
         IMPORT OPTIONS:\n    \
         --entry N       which drawable of a .ydd or .yft to replace (default 0)\n    \
         --fill-missing  accept a stated default for a channel the geometry\n                    \
         declares and the mesh does not carry. Off by default: a\n                    \
         filled normal or colour is data the source never stated\n    \
         --keep-skinning carry the drawable's own influences across when the\n                    \
         mesh states none. Needed for a ped component, which is\n                    \
         skinned against a skeleton in another file, so glTF\n                    \
         cannot carry its weights. Refuses if the vertex count\n                    \
         moved, because then the old weights name other vertices\n\
         \n\
         An import keeps the geometry's own channel set, because a material's\n\
         vertex declaration names what it reads. `mesh channels` prints it.\n\
         The save runs the format's own validation on the bytes it wrote, so a\n\
         refusal leaves the input alone.\n\
         \n\
         GRAFT OPTIONS:\n    \
         --prefix NAME   the donor geometries drawn with a texture whose name starts\n                    \
         with this are moved into the garment, with their shaders and\n                    \
         skinning (default mp_fm_skin: the game's body skin), so a legs\n                    \
         garment with holes shows skin there. Refused when the garment\n                    \
         already draws with such a texture.\n\
         \n\
         CENSUS OPTIONS:\n    \
         --dir DIR       walk loose files in one directory instead of an install\n    \
         --limit N       stop after N files; 0 means every one. Default 20000\n\
         \n\
         The census keeps counters and never a per-file result, so its peak\n\
         memory is one resource's parse whatever the limit is.\n\
         \n\
         Each target is read and written as itself: an Enhanced file is never\n\
         converted through Legacy. The census's channel, winding and export-drop\n\
         figures are Legacy's; it counts Enhanced drawables for the refusals only.\n"
    );
}

/// What `mesh export` was asked for.
struct ExportArgs {
    input: PathBuf,
    output: PathBuf,
    container: Container,
    lods: LodSelection,
    textures: bool,
    only: Option<usize>,
}

/// The arguments of `mesh export` as it reads them, `None` when they ask for its usage; an `Err` is a usage error.
fn export_args(args: &[String]) -> Result<Option<ExportArgs>, String> {
    let mut positional: Vec<&str> = Vec::new();
    let mut lods = LodSelection::Highest;
    let mut textures = true;
    let mut only: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok(None),
            "--no-textures" => textures = false,
            "--entry" => {
                i += 1;
                match args.get(i).map(|v| v.parse::<usize>()) {
                    Some(Ok(n)) => only = Some(n),
                    _ => return Err("--entry needs a number".into()),
                }
            }
            "--lods" => {
                i += 1;
                match args.get(i).map(String::as_str) {
                    Some("all") => lods = LodSelection::All,
                    Some("high") | Some("highest") => lods = LodSelection::Highest,
                    _ => return Err("--lods takes `high` or `all`".into()),
                }
            }
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => positional.push(other),
        }
        i += 1;
    }

    let [input, output] = positional[..] else {
        return Err("mesh export needs an input and an output path".into());
    };
    let input = PathBuf::from(input);
    let output = PathBuf::from(output);

    let container = match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("glb") => Container::Glb,
        Some("gltf") => Container::Separate,
        _ => return Err("the output must end in .glb or .gltf".into()),
    };
    Ok(Some(ExportArgs {
        input,
        output,
        container,
        lods,
        textures,
        only,
    }))
}

/// Whether `args` are ones `mesh export` takes, read as it reads them and without exporting.
pub(crate) fn accepts_export(args: &[String]) -> Result<(), String> {
    export_args(args).map(drop)
}

/// Whether `args` are ones `mesh import` takes, read as it reads them and without importing.
pub(crate) fn accepts_import(args: &[String]) -> Result<(), String> {
    import_args(args).map(drop)
}

/// The usage when `asked` is none, the usage error when it is one, else what was asked for.
fn asked_or_exit<T>(asked: Result<Option<T>, String>) -> Result<T, ExitCode> {
    match asked {
        Ok(Some(asked)) => Ok(asked),
        Ok(None) => {
            print_usage();
            Err(ExitCode::SUCCESS)
        }
        Err(why) => {
            eprintln!("Error: {why}");
            Err(ExitCode::from(2))
        }
    }
}

fn export(args: &[String]) -> ExitCode {
    let ExportArgs {
        input,
        output,
        container,
        lods,
        textures,
        only,
    } = match asked_or_exit(export_args(args)) {
        Ok(asked) => asked,
        Err(code) => return code,
    };

    let bytes = match std::fs::read(&input) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Error: cannot read {}: {e}", input.display());
            return ExitCode::from(1);
        }
    };

    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mesh")
        .to_string();
    let options = ExtractOptions { lods, textures };
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !DRAWABLE.contains(&ext.as_str()) {
        eprintln!("Error: mesh export does not read '.{ext}'");
        return ExitCode::from(2);
    }

    let scene = match export_scene(&input.display().to_string(), &bytes, &stem, only, &options) {
        Ok(exported) => exported.scene,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let files = gltf::write(&scene, &stem, container);
    let directory = output.parent().unwrap_or(Path::new("."));
    if let Err(e) = std::fs::write(&output, &files.primary) {
        eprintln!("Error: cannot write {}: {e}", output.display());
        return ExitCode::from(1);
    }
    for (name, bytes) in &files.sidecars {
        let at = directory.join(name);
        if let Err(e) = std::fs::write(&at, bytes) {
            eprintln!("Error: cannot write {}: {e}", at.display());
            return ExitCode::from(1);
        }
    }

    println!(
        "{} -> {} ({} objects, {} meshes, {} vertices, {} triangles, {} materials, {} textures)",
        input.display(),
        output.display(),
        scene.objects.len(),
        scene.objects.iter().map(|o| o.meshes.len()).sum::<usize>(),
        scene.vertex_count(),
        scene.triangle_count(),
        scene.materials.len(),
        scene.images.len(),
    );
    if !files.sidecars.is_empty() {
        println!("  beside it: {}", files.sidecars.len());
    }
    for line in or_interchange::report_lines(&scene.report) {
        println!("  note: {line}");
    }
    ExitCode::SUCCESS
}

/// A drawable file as `mesh export` exports it.
pub struct Exported {
    pub scene: or_interchange::Scene,
    /// The `--entry` number of each of the scene's objects, in order.
    pub entries: Vec<usize>,
}

/// `bytes`, a `.ydr`, `.ydd` or `.yft` called `name`, as `mesh export` exports the drawable `only` selects, the scene named `stem`; `Err` says why it is refused.
///
/// ⚠ The app's Export to glTF… calls this too, so its report and this command's are one computation.
pub fn export_scene(
    name: &str,
    bytes: &[u8],
    stem: &str,
    only: Option<usize>,
    options: &ExtractOptions,
) -> Result<Exported, String> {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let mut builder = SceneBuilder::new(stem);
    // ⚠ Each target is read as itself: converting a Gen9 file first
    // would put Gen8 vertex declarations on its data.
    let entries = match crate::target_of(Path::new(name), bytes) {
        Generation::Gen9 => push_drawables::<Gen9>(&mut builder, &ext, bytes, only, stem, options),
        _ => push_drawables::<Gen8>(&mut builder, &ext, bytes, only, stem, options),
    }
    .map_err(|e| format!("cannot read {name}: {e}"))?;
    let scene = builder.finish();
    if scene.objects.iter().all(|o| o.meshes.is_empty()) {
        return Err(format!("{name} holds no geometry to export"));
    }
    Ok(Exported { scene, entries })
}

/// Every drawable of a file that `only` selects, into `builder`, and the `--entry` number of each.
fn push_drawables<T: Extracted>(
    builder: &mut SceneBuilder,
    ext: &str,
    bytes: &[u8],
    only: Option<usize>,
    stem: &str,
    options: &ExtractOptions,
) -> Result<Vec<usize>, or_io::error::Error>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    GtaFragType<T>: File,
{
    let mut pushed = Vec::new();
    match ext {
        "ydr" => {
            let d = GtaDrawable::<T>::from_bytes(bytes)?;
            builder.push(&name_of(&d, stem), &d.base, options);
            pushed.push(0);
        }
        // ⚠ `--entry` is what makes export and import symmetric. A whole
        // dictionary's meshes cannot be imported back into one entry, so
        // editing entry 3 means exporting entry 3.
        "ydd" => {
            let dict = PgDictionary::<GtaDrawable<T>>::from_bytes(bytes)?;
            for (index, entry) in dict.entries.iter().enumerate() {
                let Some(entry) = entry.as_deref() else {
                    continue;
                };
                if only.is_some_and(|wanted| wanted != index) {
                    continue;
                }
                let hash = dict.codes.get(index).copied().unwrap_or(0);
                let fallback = format!("entry_{hash:08X}");
                builder.push(&name_of(entry, &fallback), &entry.base, options);
                pushed.push(index);
            }
        }
        _ => {
            let frag = GtaFragType::<T>::from_bytes(bytes)?;
            // ⚠ An extra drawable has no shader group and indexes the common drawable's
            let common_base = builder.material_count();
            let shares = frag.common_drawable.is_some();
            for (index, drawable) in fragment_drawables(&frag).into_iter().enumerate() {
                if only.is_some_and(|wanted| wanted != index) {
                    continue;
                }
                let fallback = format!("part_{index}");
                let name = name_of(drawable, &fallback);
                if shares && index > 0 && drawable.shader_group.is_none() {
                    builder.push_sharing(&name, drawable, common_base, options);
                } else {
                    builder.push(&name, drawable, options);
                }
                pushed.push(index);
            }
        }
    }
    Ok(pushed)
}

/// The extension and bytes of a drawable-bearing input, or a usage error.
/// `mesh retuck <garment.ydd> <output.ydd> [--entry N] [--marker NAME] [--margin METRES]`: the skin of an already grafted garment pushed under its cloth again.
fn retuck(args: &[String]) -> ExitCode {
    let mut positional: Vec<&str> = Vec::new();
    let (mut entry, mut marker, mut margin) = (0usize, "lowr_normal_015".to_string(), 0.003f32);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            flag @ ("--entry" | "--marker" | "--margin") => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("Error: {flag} needs a value");
                    return ExitCode::from(2);
                };
                match flag {
                    "--marker" => marker = value.clone(),
                    "--margin" => match value.parse::<f32>() {
                        Ok(v) => margin = v,
                        Err(_) => {
                            eprintln!("Error: --margin needs a number of metres");
                            return ExitCode::from(2);
                        }
                    },
                    _ => match value.parse::<usize>() {
                        Ok(n) => entry = n,
                        Err(_) => {
                            eprintln!("Error: --entry needs a number");
                            return ExitCode::from(2);
                        }
                    },
                }
            }
            other if other.starts_with('-') => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
            other => positional.push(other),
        }
        i += 1;
    }
    let [garment, output] = positional[..] else {
        eprintln!("Error: mesh retuck needs a garment and an output path");
        return ExitCode::from(2);
    };
    let (garment, output) = (PathBuf::from(garment), PathBuf::from(output));
    let bytes = match drawable_input(&garment) {
        Ok((ext, bytes)) if ext == "ydd" => bytes,
        Ok(_) => {
            eprintln!("Error: mesh retuck reads .ydd, a ped component's dictionary");
            return ExitCode::from(2);
        }
        Err(code) => return code,
    };
    let mut doc = match PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes) {
        Ok(d) => d,
        Err(e) => return fail_to_read(&garment, e),
    };
    let held = doc.entries.len();
    let Some(drawable) = doc.entries.get_mut(entry).and_then(|e| e.as_deref_mut()) else {
        eprintln!("Error: the garment holds {held} entries and entry {entry} was asked for");
        return ExitCode::from(1);
    };
    match or_interchange::doctor::graft::retuck(&mut drawable.base, &marker, margin) {
        Ok(lines) => match doc.to_bytes() {
            Ok(out) => {
                if let Err(e) = std::fs::write(&output, &out) {
                    eprintln!("Error: cannot write {}: {e}", output.display());
                    return ExitCode::from(1);
                }
                println!(
                    "{} -> {} ({} bytes)",
                    garment.display(),
                    output.display(),
                    out.len()
                );
                for line in lines {
                    println!("  {line}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!(
                    "Error: the drawable did not save, and {} is unchanged.",
                    garment.display()
                );
                eprintln!("  {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!(
                "Error: the skin was not tucked and {} is unchanged.",
                garment.display()
            );
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

/// `mesh underwear <garment.ydd> <output.ydd> [--entry N] [--marker NAME] [--island U,V]`: the donor's underwear dropped from an already grafted garment.
fn underwear(args: &[String]) -> ExitCode {
    let mut positional: Vec<&str> = Vec::new();
    let (mut entry, mut marker, mut island) =
        (0usize, "lowr_normal_015".to_string(), [0.5f32, 0.0f32]);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            flag @ ("--entry" | "--marker" | "--island") => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("Error: {flag} needs a value");
                    return ExitCode::from(2);
                };
                match flag {
                    "--marker" => marker = value.clone(),
                    "--island" => {
                        let parts: Vec<f32> = value
                            .split(',')
                            .filter_map(|p| p.trim().parse().ok())
                            .collect();
                        match parts[..] {
                            [u] => island = [u, 0.0],
                            [u, v] => island = [u, v],
                            _ => {
                                eprintln!("Error: --island needs U or U,V");
                                return ExitCode::from(2);
                            }
                        }
                    }
                    _ => match value.parse::<usize>() {
                        Ok(n) => entry = n,
                        Err(_) => {
                            eprintln!("Error: --entry needs a number");
                            return ExitCode::from(2);
                        }
                    },
                }
            }
            other if other.starts_with('-') => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
            other => positional.push(other),
        }
        i += 1;
    }
    let [garment, output] = positional[..] else {
        eprintln!("Error: mesh underwear needs a garment and an output path");
        return ExitCode::from(2);
    };
    let (garment, output) = (PathBuf::from(garment), PathBuf::from(output));
    let bytes = match drawable_input(&garment) {
        Ok((ext, bytes)) if ext == "ydd" => bytes,
        Ok(_) => {
            eprintln!("Error: mesh underwear reads .ydd, a ped component's dictionary");
            return ExitCode::from(2);
        }
        Err(code) => return code,
    };
    let mut doc = match PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes) {
        Ok(d) => d,
        Err(e) => return fail_to_read(&garment, e),
    };
    let held = doc.entries.len();
    let Some(drawable) = doc.entries.get_mut(entry).and_then(|e| e.as_deref_mut()) else {
        eprintln!("Error: the garment holds {held} entries and entry {entry} was asked for");
        return ExitCode::from(1);
    };
    match or_interchange::doctor::graft::drop_underwear(&mut drawable.base, &marker, island) {
        Ok(lines) => match doc.to_bytes() {
            Ok(out) => {
                if let Err(e) = std::fs::write(&output, &out) {
                    eprintln!("Error: cannot write {}: {e}", output.display());
                    return ExitCode::from(1);
                }
                println!(
                    "{} -> {} ({} bytes)",
                    garment.display(),
                    output.display(),
                    out.len()
                );
                for line in lines {
                    println!("  {line}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!(
                    "Error: the drawable did not save, and {} is unchanged.",
                    garment.display()
                );
                eprintln!("  {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!(
                "Error: the underwear was not dropped and {} is unchanged.",
                garment.display()
            );
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

/// `mesh unskin <garment.ydd> <output.ydd> [--entry N] [--marker NAMES]`: the skin taken out of an already grafted garment (to graft it again with other settings); prints `bounds U0,V0,U1,V1`.
fn unskin(args: &[String]) -> ExitCode {
    let mut positional: Vec<&str> = Vec::new();
    let (mut entry, mut marker) = (0usize, "lowr_normal_015".to_string());
    let mut bounds_only = false;
    let mut island: Option<[f32; 2]> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bounds" => bounds_only = true,
            flag @ ("--entry" | "--marker" | "--island") => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("Error: {flag} needs a value");
                    return ExitCode::from(2);
                };
                if flag == "--marker" {
                    marker = value.clone();
                } else if flag == "--island" {
                    let parts: Vec<f32> = value
                        .split(',')
                        .filter_map(|p| p.trim().parse().ok())
                        .collect();
                    island = match parts[..] {
                        [u] => Some([u, 0.0]),
                        [u, v] => Some([u, v]),
                        _ => {
                            eprintln!("Error: --island needs U or U,V");
                            return ExitCode::from(2);
                        }
                    };
                } else {
                    match value.parse::<usize>() {
                        Ok(n) => entry = n,
                        Err(_) => {
                            eprintln!("Error: --entry needs a number");
                            return ExitCode::from(2);
                        }
                    }
                }
            }
            other if other.starts_with('-') => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
            other => positional.push(other),
        }
        i += 1;
    }
    let (garment, output) = match (positional[..].to_vec().as_slice(), bounds_only) {
        ([garment], true) => (*garment, ""),
        ([garment, output], false) => (*garment, *output),
        _ => {
            eprintln!("Error: mesh unskin needs a garment and an output path (or only a garment with --bounds)");
            return ExitCode::from(2);
        }
    };
    let (garment, output) = (PathBuf::from(garment), PathBuf::from(output));
    let bytes = match drawable_input(&garment) {
        Ok((ext, bytes)) if ext == "ydd" => bytes,
        Ok(_) => {
            eprintln!("Error: mesh unskin reads .ydd, a ped component's dictionary");
            return ExitCode::from(2);
        }
        Err(code) => return code,
    };
    let mut doc = match PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes) {
        Ok(d) => d,
        Err(e) => return fail_to_read(&garment, e),
    };
    let held = doc.entries.len();
    let Some(drawable) = doc.entries.get_mut(entry).and_then(|e| e.as_deref_mut()) else {
        eprintln!("Error: the garment holds {held} entries and entry {entry} was asked for");
        return ExitCode::from(1);
    };
    match or_interchange::doctor::graft::unskin(&mut drawable.base, &marker, island) {
        Ok((b, _)) if bounds_only => {
            println!("bounds {},{},{},{}", b[0], b[1], b[2], b[3]);
            if b[4] >= 0.0 {
                println!("common {},{}", b[4], b[5]);
            }
            ExitCode::SUCCESS
        }
        Ok((b, lines)) => match doc.to_bytes() {
            Ok(out) => {
                if let Err(e) = std::fs::write(&output, &out) {
                    eprintln!("Error: cannot write {}: {e}", output.display());
                    return ExitCode::from(1);
                }
                println!(
                    "{} -> {} ({} bytes)",
                    garment.display(),
                    output.display(),
                    out.len()
                );
                for line in lines {
                    println!("  {line}");
                }
                println!("bounds {},{},{},{}", b[0], b[1], b[2], b[3]);
                if b[4] >= 0.0 {
                    println!("common {},{}", b[4], b[5]);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!(
                    "Error: the drawable did not save, and {} is unchanged.",
                    garment.display()
                );
                eprintln!("  {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!(
                "Error: the skin was not taken out and {} is unchanged.",
                garment.display()
            );
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

/// `mesh cutout <garment.ydd> <stock.ydd> <output.ydd> [--entry N] [--marker NAMES]`: the garment's own shaders drawn as cutouts, as the stock material of `stock` is.
fn cutout(args: &[String]) -> ExitCode {
    let mut positional: Vec<&str> = Vec::new();
    let (mut entry, mut marker) = (0usize, "lowr_normal_015".to_string());
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            flag @ ("--entry" | "--marker") => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("Error: {flag} needs a value");
                    return ExitCode::from(2);
                };
                if flag == "--marker" {
                    marker = value.clone();
                } else {
                    match value.parse::<usize>() {
                        Ok(n) => entry = n,
                        Err(_) => {
                            eprintln!("Error: --entry needs a number");
                            return ExitCode::from(2);
                        }
                    }
                }
            }
            other if other.starts_with('-') => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
            other => positional.push(other),
        }
        i += 1;
    }
    let [garment, stock, output] = positional[..] else {
        eprintln!("Error: mesh cutout needs a garment, a stock drawable and an output path");
        return ExitCode::from(2);
    };
    let (garment, stock, output) = (
        PathBuf::from(garment),
        PathBuf::from(stock),
        PathBuf::from(output),
    );
    let read = |p: &PathBuf| match drawable_input(p) {
        Ok((ext, bytes)) if ext == "ydd" => Ok(bytes),
        Ok(_) => {
            eprintln!("Error: mesh cutout reads .ydd, a ped component's dictionary");
            Err(ExitCode::from(2))
        }
        Err(code) => Err(code),
    };
    let (garment_bytes, stock_bytes) = match (read(&garment), read(&stock)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(code), _) | (_, Err(code)) => return code,
    };
    let (mut doc, stock_doc) = match (
        PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&garment_bytes),
        PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&stock_bytes),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) => return fail_to_read(&garment, e),
        (_, Err(e)) => return fail_to_read(&stock, e),
    };
    let held = doc.entries.len();
    let Some(drawable) = doc.entries.get_mut(entry).and_then(|e| e.as_deref_mut()) else {
        eprintln!("Error: the garment holds {held} entries and entry {entry} was asked for");
        return ExitCode::from(1);
    };
    let Some(stock_drawable) = stock_doc.entries.first().and_then(|e| e.as_deref()) else {
        eprintln!("Error: the stock drawable holds no entry");
        return ExitCode::from(1);
    };
    match or_interchange::doctor::graft::cutout(&mut drawable.base, &stock_drawable.base, &marker) {
        Ok(lines) => match doc.to_bytes() {
            Ok(out) => {
                if let Err(e) = std::fs::write(&output, &out) {
                    eprintln!("Error: cannot write {}: {e}", output.display());
                    return ExitCode::from(1);
                }
                println!(
                    "{} -> {} ({} bytes)",
                    garment.display(),
                    output.display(),
                    out.len()
                );
                for line in lines {
                    println!("  {line}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!(
                    "Error: the drawable did not save, and {} is unchanged.",
                    garment.display()
                );
                eprintln!("  {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!(
                "Error: the garment was not made a cutout and {} is unchanged.",
                garment.display()
            );
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

/// `mesh graft`: the skin geometries of a donor legs drawable moved into a garment (`or_interchange::doctor::graft`).
fn graft(args: &[String]) -> ExitCode {
    let mut positional: Vec<&str> = Vec::new();
    let (mut entry, mut donor_entry) = (0usize, 0usize);
    let mut prefix = "mp_fm_skin".to_string();
    let mut marker: Option<String> = None;
    let mut tuck = false;
    let mut hide_island: Option<[f32; 2]> = None;
    let mut unskin_marker: Option<String> = None;
    let mut margin: Option<f32> = None;
    let (mut skin_uv, mut target_uv): (Option<[f32; 4]>, Option<[f32; 4]>) = (None, None);
    let mut texture_files: Vec<(PathBuf, Option<String>)> = Vec::new();
    let four = |value: &str| -> Option<[f32; 4]> {
        let parts: Vec<f32> = value
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        <[f32; 4]>::try_from(parts).ok()
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            "--all" => prefix = String::new(),
            "--tuck" => tuck = true,
            flag @ ("--entry" | "--donor-entry" | "--prefix" | "--marker" | "--skin-uv"
            | "--target-uv" | "--texture" | "--hide-island" | "--margin" | "--unskin") => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("Error: {flag} needs a value");
                    return ExitCode::from(2);
                };
                match flag {
                    "--prefix" => prefix = value.clone(),
                    "--marker" => marker = Some(value.clone()),
                    "--unskin" => unskin_marker = Some(value.clone()),
                    "--margin" => match value.parse::<f32>() {
                        Ok(v) => margin = Some(v),
                        Err(_) => {
                            eprintln!("Error: {flag} needs a number of metres");
                            return ExitCode::from(2);
                        }
                    },
                    "--hide-island" => {
                        let parts: Vec<f32> = value
                            .split(',')
                            .filter_map(|p| p.trim().parse().ok())
                            .collect();
                        match parts[..] {
                            [u] => hide_island = Some([u, 0.0]),
                            [u, v] => hide_island = Some([u, v]),
                            _ => {
                                eprintln!("Error: {flag} needs U or U,V");
                                return ExitCode::from(2);
                            }
                        }
                    }
                    "--texture" => match value.rsplit_once('=') {
                        Some((file, name)) => {
                            texture_files.push((PathBuf::from(file), Some(name.to_string())))
                        }
                        None => texture_files.push((PathBuf::from(value), None)),
                    },
                    "--skin-uv" | "--target-uv" => {
                        let Some(box4) = four(value) else {
                            eprintln!("Error: {flag} needs four numbers");
                            return ExitCode::from(2);
                        };
                        if flag == "--skin-uv" {
                            skin_uv = Some(box4);
                        } else {
                            target_uv = Some(box4);
                        }
                    }
                    _ => match value.parse::<usize>() {
                        Ok(n) if flag == "--entry" => entry = n,
                        Ok(n) => donor_entry = n,
                        Err(_) => {
                            eprintln!("Error: {flag} needs a number");
                            return ExitCode::from(2);
                        }
                    },
                }
            }
            other if other.starts_with('-') => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
            other => positional.push(other),
        }
        i += 1;
    }
    let [garment, donor, output] = positional[..] else {
        eprintln!("Error: mesh graft needs a garment, a donor and an output path");
        return ExitCode::from(2);
    };
    let (garment, donor, output) = (
        PathBuf::from(garment),
        PathBuf::from(donor),
        PathBuf::from(output),
    );
    let read = |path: &Path| match drawable_input(path) {
        Ok((ext, bytes)) if ext == "ydd" => Ok(bytes),
        Ok(_) => {
            eprintln!("Error: mesh graft reads .ydd, a ped component's dictionary");
            Err(ExitCode::from(2))
        }
        Err(code) => Err(code),
    };
    let (garment_bytes, donor_bytes) = match (read(&garment), read(&donor)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(code), _) | (_, Err(code)) => return code,
    };
    let (mut garment_doc, mut donor_doc) = match (
        PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&garment_bytes),
        PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&donor_bytes),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) => return fail_to_read(&garment, e),
        (_, Err(e)) => return fail_to_read(&donor, e),
    };
    let held = garment_doc.entries.len();
    let Some(into) = garment_doc
        .entries
        .get_mut(entry)
        .and_then(|e| e.as_deref_mut())
    else {
        eprintln!("Error: the garment holds {held} entries and entry {entry} was asked for");
        return ExitCode::from(1);
    };
    let held = donor_doc.entries.len();
    let Some(from) = donor_doc
        .entries
        .get_mut(donor_entry)
        .and_then(|e| e.as_deref_mut())
    else {
        eprintln!("Error: the donor holds {held} entries and entry {donor_entry} was asked for");
        return ExitCode::from(1);
    };
    let mut textures = Vec::new();
    for (file, rename) in &texture_files {
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("Error: cannot read {}: {e}", file.display());
                return ExitCode::from(1);
            }
        };
        match PgDictionary::<or_io::gta5::gen8::GrcTexture>::from_bytes(&bytes) {
            Ok(dictionary) => {
                for mut texture in dictionary.entries.into_iter().flatten().map(|t| *t) {
                    if let Some(name) = rename {
                        texture.name = name.clone();
                    }
                    textures.push(texture);
                }
            }
            Err(e) => return fail_to_read(file, e),
        }
    }
    let options = or_interchange::doctor::graft::GraftOptions {
        prefix: &prefix,
        marker: marker.as_deref(),
        skin_uv,
        target_uv,
        textures,
        tuck,
        hide_island,
        margin,
    };
    if let Some(marker) = &unskin_marker {
        if let Err(e) = or_interchange::doctor::graft::unskin(&mut into.base, marker, None) {
            eprintln!(
                "Error: the old skin was not taken out and {} is unchanged.",
                garment.display()
            );
            eprintln!("  {e}");
            return ExitCode::from(1);
        }
    }
    match or_interchange::doctor::graft::graft_skin(&mut into.base, &mut from.base, &options) {
        Ok(report) => match garment_doc.to_bytes() {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&output, &bytes) {
                    eprintln!("Error: cannot write {}: {e}", output.display());
                    return ExitCode::from(1);
                }
                println!(
                    "{} + {} -> {} ({} bytes)",
                    garment.display(),
                    donor.display(),
                    output.display(),
                    bytes.len()
                );
                for line in report.lines() {
                    println!("  {line}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!(
                    "Error: the grafted drawable did not save, and {} is unchanged.",
                    garment.display()
                );
                eprintln!("  {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!(
                "Error: the graft was refused and {} is unchanged.",
                garment.display()
            );
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

fn drawable_input(path: &Path) -> std::result::Result<(String, Vec<u8>), ExitCode> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Error: cannot read {}: {e}", path.display());
            return Err(ExitCode::from(1));
        }
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !DRAWABLE.contains(&ext.as_str()) {
        eprintln!("Error: mesh interchange does not read '.{ext}'");
        return Err(ExitCode::from(2));
    }
    Ok((ext, bytes))
}

/// What `mesh import` was asked for.
struct ImportArgs {
    input: PathBuf,
    mesh: PathBuf,
    output: PathBuf,
    entry: usize,
    options: ImportOptions,
}

/// The arguments of `mesh import` as it reads them, `None` when they ask for its usage; an `Err` is a usage error.
fn import_args(args: &[String]) -> Result<Option<ImportArgs>, String> {
    let mut positional: Vec<&str> = Vec::new();
    let mut entry = 0usize;
    let mut options = ImportOptions::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok(None),
            "--fill-missing" => options.fill_missing = true,
            "--keep-skinning" => options.keep_skinning = true,
            "--entry" => {
                i += 1;
                match args.get(i).map(|v| v.parse::<usize>()) {
                    Some(Ok(n)) => entry = n,
                    _ => return Err("--entry needs a number".into()),
                }
            }
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => positional.push(other),
        }
        i += 1;
    }

    let [input, mesh, output] = positional[..] else {
        return Err("mesh import needs a drawable, a mesh and an output path".into());
    };
    Ok(Some(ImportArgs {
        input: PathBuf::from(input),
        mesh: PathBuf::from(mesh),
        output: PathBuf::from(output),
        entry,
        options,
    }))
}

fn import(args: &[String]) -> ExitCode {
    let ImportArgs {
        input,
        mesh,
        output,
        entry,
        options,
    } = match asked_or_exit(import_args(args)) {
        Ok(asked) => asked,
        Err(code) => return code,
    };

    let original = match drawable_input(&input) {
        Ok((_, bytes)) => bytes,
        Err(code) => return code,
    };
    let source = match std::fs::read(&mesh) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Error: cannot read {}: {e}", mesh.display());
            return ExitCode::from(1);
        }
    };

    // ⚠ A binary FBX 7.1 or later is converted on the way in: its axes and unit are carried to the
    // drawable's, and what it held that a drawable has no place for is printed after the report.
    let read = if mesh
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("fbx"))
    {
        or_interchange::fbx::load(&source)
    } else {
        or_interchange::gltf::read::load(&mesh, &source)
    };
    let scene = match read {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("Error: {} is not a mesh this can read: {e}", mesh.display());
            return ExitCode::from(1);
        }
    };

    let name = input.display().to_string();
    match replace_mesh(&name, &original, entry, &scene, &options) {
        Ok((bytes, report)) => {
            if let Err(e) = std::fs::write(&output, &bytes) {
                eprintln!("Error: cannot write {}: {e}", output.display());
                return ExitCode::from(1);
            }
            println!(
                "{} + {} -> {} ({} bytes)",
                input.display(),
                mesh.display(),
                output.display(),
                bytes.len()
            );
            for line in report.lines() {
                println!("  {line}");
            }
            for held in &scene.dropped {
                println!("  not carried: {held}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            // The input is named as untouched, so the user knows their file
            // did not change.
            eprintln!(
                "Error: the import was refused and {} is unchanged.",
                input.display()
            );
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

/// `bytes`, a `.ydr`, `.ydd` or `.yft` called `name`, with drawable `entry`'s geometry replaced from `scene`, as `mesh import` writes it, and what changed; `Err` says why the import is refused.
///
/// ⚠ The app's Import glTF… calls this too, so its report and this command's are one computation.
pub fn replace_mesh(
    name: &str,
    bytes: &[u8],
    entry: usize,
    scene: &or_interchange::Scene,
    options: &ImportOptions,
) -> Result<(Vec<u8>, or_interchange::ImportReport), String> {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let destination = match ext.as_str() {
        "ydr" => Destination::Ydr,
        "ydd" => Destination::Ydd(entry),
        _ => Destination::Yft(entry),
    };
    let replaced = match crate::target_of(Path::new(name), bytes) {
        Generation::Gen9 => {
            or_interchange::replace_in_file::<Gen9>(bytes, destination, scene, options)
        }
        _ => or_interchange::replace_in_file::<Gen8>(bytes, destination, scene, options),
    };
    replaced.map_err(|e| e.to_string())
}

/// What channels each geometry declares, which is what an import must supply.
fn channels(args: &[String]) -> ExitCode {
    let [input] = args else {
        eprintln!("Error: mesh channels needs one drawable path");
        return ExitCode::from(2);
    };
    let input = PathBuf::from(input);
    let (ext, bytes) = match drawable_input(&input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let described = match crate::target_of(&input, &bytes) {
        Generation::Gen9 => channel_lines::<Gen9>(&ext, &bytes),
        _ => channel_lines::<Gen8>(&ext, &bytes),
    };
    match described {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail_to_read(&input, e),
    }
}

/// Each drawable's label, then its geometries' channel sets indented under it.
fn channel_lines<T: DrawableTarget>(
    ext: &str,
    bytes: &[u8],
) -> Result<Vec<String>, or_io::error::Error>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    GtaFragType<T>: File,
{
    let drawables: Vec<(String, RmcDrawable<T>)> = match ext {
        "ydr" => vec![(
            "the drawable".to_string(),
            GtaDrawable::<T>::from_bytes(bytes)?.base,
        )],
        "ydd" => PgDictionary::<GtaDrawable<T>>::from_bytes(bytes)?
            .entries
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| Some((format!("entry {i}"), e?.base)))
            .collect(),
        _ => {
            let frag = GtaFragType::<T>::from_bytes(bytes)?.base;
            let mut out = Vec::new();
            if let Some(common) = frag.common_drawable {
                out.push(("drawable 0 (common)".to_string(), common.base));
            }
            for (i, d) in frag.extra_drawables.into_iter().enumerate() {
                out.push((format!("drawable {}", i + 1), d.base));
            }
            out
        }
    };

    let mut lines = Vec::new();
    for (label, drawable) in &drawables {
        lines.push(format!("{label}:"));
        for line in or_interchange::import::describe_channels(drawable) {
            lines.push(format!("  {line}"));
        }
    }
    Ok(lines)
}

fn fail_to_read(path: &Path, error: impl std::fmt::Display) -> ExitCode {
    eprintln!("Error: cannot read {}: {error}", path.display());
    ExitCode::from(1)
}

fn name_of<T: DrawableTarget>(drawable: &RmcDrawable<T>, fallback: &str) -> String {
    if drawable.name.is_empty() {
        fallback.to_string()
    } else {
        drawable.name.clone()
    }
}

/// A fragment's common drawable, then its extra drawables, as `--entry` numbers them.
fn fragment_drawables<T: DrawableTarget>(fragment: &GtaFragType<T>) -> Vec<&RmcDrawable<T>> {
    fragment
        .common_drawable
        .iter()
        .chain(fragment.extra_drawables.iter())
        .map(|d| &d.base)
        .collect()
}

/// Whether a geometry's index run groups into triangles, by its own counts.
///
/// ⚠ Not `primitive_type`: `GrmPrimitiveType` was once numbered one out and
/// read every shipped geometry as `LineStrip`, and a reader that trusted it
/// exported nothing.
fn is_triangle_list(geometry: &GrmGeometry<Gen8>) -> bool {
    let indices = geometry.index_buffer.indices.len();
    let primitives = geometry.primitive_count() as usize;
    if primitives > 0 {
        return indices == primitives * 3;
    }
    indices.is_multiple_of(3)
}

/// The engine's name for a `primitive_type` byte; `None` for a value it never defined.
fn primitive_name(kind: u8) -> Option<&'static str> {
    const NAMES: [&str; 7] = [
        "Points",
        "Lines",
        "LineStrip",
        "Tris",
        "TriStrip",
        "TriFan",
        "QuadList",
    ];
    NAMES.get(usize::from(kind)).copied()
}

/// Whether a geometry skins against a palette.
fn is_skinned(geometry: &GrmGeometry<Gen8>) -> bool {
    !geometry.matrix_palette.is_empty()
}

/// What a whole install says about the things an exporter must commit to.
///
/// ⚠ **Every field is a counter or a bounded map**, so peak memory is one
/// resource's parse. A `Vec` of per-file notes is how a census turns into a
/// process holding tens of gigabytes at the end of a 300,000-file install.
#[derive(Default)]
struct Census {
    files: usize,
    files_read: usize,
    refused: usize,
    drawables: usize,
    /// Enhanced drawables walked for the coherence half alone.
    ///
    /// ⚠ The channel, winding and export-drop figures above are Legacy's:
    /// `VertexBufferLike` exposes one channel generically and those questions
    /// need five. Only the candidate refusals are generic.
    gen9_drawables: usize,
    geometries: usize,
    /// At most `PRIMITIVE_TYPES` wide — a `u8`'s whole range, so bounded.
    primitive_types: BTreeMap<u8, usize>,
    /// Geometries whose index run is exactly `primitive_count * 3`, which is
    /// what says a triangle list whatever `primitive_type` is numbered.
    index_run_is_three_per_primitive: usize,
    /// Nine fixed keys.
    channels: BTreeMap<&'static str, usize>,
    /// Triangles whose winding and whose stored normals agree on which side
    /// is the front, by the right-hand rule glTF specifies.
    winding_agree: u64,
    winding_disagree: u64,
    /// A triangle too thin for its geometric normal to have a reliable sign.
    winding_degenerate: u64,
    geometries_with_normals: usize,
    largest_vertex_run: usize,
    geometries_over_u16: usize,
    /// The limit, once the walk has stopped on it rather than run out of
    /// files. ⚠ A census that truncates silently prints a quotable figure
    /// that is wrong about the install it names.
    stopped_at: Option<usize>,
    geometries_without_positions: usize,
    geometries_not_triangles: usize,
    skinned_geometries: usize,
    drawables_skinned_without_a_skeleton: usize,
    normals_without_length: u64,
    tangents_without_length: u64,
    influences_outside_the_palette: u64,
    indices_out_of_range: u64,
    /// One counter per candidate refusal, so a check becomes an error only
    /// where shipped files agree it should. Ten fixed keys.
    defects: BTreeMap<MeshDefect, u64>,
    /// The first few, so a non-zero count can be looked at rather than only
    /// wondered about. Capped.
    defect_examples: Vec<String>,
}

/// How many defect sites the census spells.
const DEFECT_EXAMPLES: usize = 12;

impl Census {
    fn drawable(&mut self, drawable: &RmcDrawable<Gen8>) {
        self.drawables += 1;
        let has_skeleton = drawable.skeleton.is_some();
        let mut any_skinned = false;
        // ⚠ A container that shares the first level is that level, not a second.
        let container = match &drawable.container {
            Shared::Own(Some(lod)) => Some(&**lod),
            _ => None,
        };
        for lod in drawable
            .lod_group
            .lods
            .iter()
            .map(|l| &**l)
            .chain(container)
        {
            for model in &lod.models {
                for geometry in &model.geometries {
                    any_skinned |= is_skinned(geometry);
                    self.geometry(geometry);
                }
            }
        }
        if any_skinned && !has_skeleton {
            self.drawables_skinned_without_a_skeleton += 1;
        }
    }

    /// Every drawable a file holds, read where the bytes say, for the candidate refusals.
    fn coherence(&mut self, ext: &str, target: Generation, bytes: &[u8]) {
        use or_io::container::resource::{ResourceEnvelope, Rsc7};
        let Ok(opened) = Rsc7::open(bytes) else {
            return;
        };
        let aligned = or_io::decl::Aligned::from_bytes(&opened.data);
        let image = or_io::desc::Image::new(aligned.bytes(), opened.virtual_size as usize);
        match target {
            Generation::Gen9 => self.defects_in::<or_io::decl::Gen9>(ext, image),
            _ => self.defects_in::<or_io::decl::Gen8>(ext, image),
        }
    }

    fn defects_in<T: DrawableTarget>(&mut self, ext: &str, image: or_io::desc::Image<'_>) {
        use or_io::decl::{Raw, View};
        let drawables = match ext {
            "ydr" => View::<Raw<GtaDrawable<T>>>::at_offset(image, 0).map(|d| vec![d.base()]),
            "ydd" => {
                View::<Raw<PgDictionary<GtaDrawable<T>>>>::at_offset(image, 0).and_then(|dict| {
                    let entries = dict.entries()?;
                    let mut out = Vec::new();
                    for i in 0..entries.len() {
                        out.extend(entries.follow(i)?.map(|d| d.base()));
                    }
                    Ok(out)
                })
            }
            "yft" => or_io::gta5::fragment::drawables::<T>(image, 0),
            _ => Ok(Vec::new()),
        };
        let examples = &mut self.defect_examples;
        let counters = &mut self.defects;
        let mut seen = Vec::new();
        for drawable in drawables.unwrap_or_default() {
            // ⚠ A fragment's cloth points at its common drawable: one drawable, counted once.
            if seen.contains(&View::offset(&drawable)) {
                continue;
            }
            seen.push(View::offset(&drawable));
            let _ = each_defect(&drawable, &mut |found| {
                *counters.entry(found.defect).or_default() += 1;
                if examples.len() < DEFECT_EXAMPLES {
                    examples.push(found.to_string());
                }
            });
        }
    }

    fn geometry(&mut self, geometry: &GrmGeometry<Gen8>) {
        self.geometries += 1;
        *self
            .primitive_types
            .entry(geometry.primitive_type)
            .or_default() += 1;
        if is_triangle_list(geometry) {
            self.index_run_is_three_per_primitive += 1;
        }

        let Some(reader) = VertexReader::new(&*geometry.vertex_buffer) else {
            return;
        };
        let layout = *reader.layout();
        for (label, channel) in [
            ("position", GrcFvfChannel::Position),
            ("normal", GrcFvfChannel::Normal),
            ("tangent0", GrcFvfChannel::Tangent0),
            ("tex0", GrcFvfChannel::Tex0),
            ("tex1", GrcFvfChannel::Tex1),
            ("diffuse", GrcFvfChannel::Diffuse),
            ("specular", GrcFvfChannel::Specular),
            ("weight", GrcFvfChannel::Weight),
            ("binding", GrcFvfChannel::Binding),
        ] {
            if layout.has(channel) {
                *self.channels.entry(label).or_default() += 1;
            }
        }

        let count = reader.count();
        self.largest_vertex_run = self.largest_vertex_run.max(count);
        if count > usize::from(u16::MAX) {
            self.geometries_over_u16 += 1;
        }
        if !layout.has(GrcFvfChannel::Position) {
            self.geometries_without_positions += 1;
        }
        if !is_triangle_list(geometry) {
            self.geometries_not_triangles += 1;
        }
        if is_skinned(geometry) {
            self.skinned_geometries += 1;
            for vertex in 0..count {
                let weights = channel_lanes(&reader, GrcFvfChannel::Weight, vertex);
                let slots = channel_lanes(&reader, GrcFvfChannel::Binding, vertex);
                for lane in 0..4 {
                    if weights[lane] != 0
                        && geometry
                            .matrix_palette
                            .get(usize::from(slots[lane]))
                            .is_none()
                    {
                        self.influences_outside_the_palette += 1;
                    }
                }
            }
        }
        for (channel, counter) in [
            (GrcFvfChannel::Normal, true),
            (GrcFvfChannel::Tangent0, false),
        ] {
            if !layout.has(channel) {
                continue;
            }
            let without = (0..count)
                .filter(|&v| {
                    let d = reader.get(channel, v).unwrap_or_default();
                    d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 1e-18
                })
                .count() as u64;
            if counter {
                self.normals_without_length += without;
            } else {
                self.tangents_without_length += without;
            }
        }

        if !layout.has(GrcFvfChannel::Position) || !layout.has(GrcFvfChannel::Normal) {
            return;
        }
        let indices = &geometry.index_buffer.indices;
        if !is_triangle_list(geometry) {
            return;
        }
        self.geometries_with_normals += 1;

        for triangle in indices.as_chunks::<3>().0 {
            let corners: Option<Vec<[f32; 4]>> = triangle
                .iter()
                .map(|&i| {
                    if usize::from(i) < count {
                        reader.get(GrcFvfChannel::Position, usize::from(i))
                    } else {
                        None
                    }
                })
                .collect();
            let Some(corners) = corners else {
                self.indices_out_of_range += 1;
                continue;
            };
            let edge1 = sub(corners[1], corners[0]);
            let edge2 = sub(corners[2], corners[0]);
            let face = cross(edge1, edge2);
            let scale = length(face);
            if scale < 1e-12 {
                self.winding_degenerate += 1;
                continue;
            }
            let mut stored = [0.0f32; 3];
            for &i in triangle {
                let Some(normal) = reader.get(GrcFvfChannel::Normal, usize::from(i)) else {
                    continue;
                };
                for axis in 0..3 {
                    stored[axis] += normal[axis];
                }
            }
            if length(stored) < 1e-6 {
                self.winding_degenerate += 1;
                continue;
            }
            if dot(face, stored) > 0.0 {
                self.winding_agree += 1;
            } else {
                self.winding_disagree += 1;
            }
        }
    }
}

fn sub(a: [f32; 4], b: [f32; 4]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// How many files a census reads when nothing says otherwise.
///
/// ⚠ An unbounded walk of a retail install is 300,000-odd files; `--limit 0`
/// means every file, so asking for the whole install is a decision.
const DEFAULT_LIMIT: usize = 20_000;

fn census(args: &[String]) -> ExitCode {
    let mut game: Option<PathBuf> = None;
    let mut keys: Option<PathBuf> = None;
    let mut dir: Option<PathBuf> = None;
    let mut limit = DEFAULT_LIMIT;

    let mut i = 0;
    while i < args.len() {
        let key = args[i].as_str();
        match key {
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            "--game" | "--keys" | "--dir" | "--limit" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("Error: {key} needs a value");
                    return ExitCode::from(2);
                };
                match key {
                    "--game" => game = Some(PathBuf::from(value)),
                    "--keys" => keys = Some(PathBuf::from(value)),
                    "--dir" => dir = Some(PathBuf::from(value)),
                    _ => match value.parse::<usize>() {
                        Ok(0) => limit = usize::MAX,
                        Ok(n) => limit = n,
                        Err(_) => {
                            eprintln!("Error: --limit needs a number");
                            return ExitCode::from(2);
                        }
                    },
                }
            }
            other => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    if let Some(dir) = dir {
        eprintln!(
            "openrage-cli: reading loose drawables under {} ...",
            dir.display()
        );
        let census = survey_directory(&dir, limit);
        print!("{}", report(&census));
        return ExitCode::SUCCESS;
    }

    let game_root = match resolve_game_root(game) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("Error: {message}");
            return ExitCode::from(2);
        }
    };
    let resolved = resolve_crypto(&game_root, keys.as_deref());
    let mut warnings = resolved.notes;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        let _ = super::jobs::install_stop_handler(move || cancel.store(true, Ordering::Relaxed));
    }

    eprintln!(
        "openrage-cli: walking drawables under {} ...",
        game_root.display()
    );
    let census = survey(&game_root, resolved.provider, limit, &cancel, &mut warnings);
    for warning in &warnings {
        eprintln!("note: {warning}");
    }
    print!("{}", report(&census));
    ExitCode::SUCCESS
}

/// A directory of loose drawables, one file resident at a time.
///
/// Reads with `read_dir` rather than collecting a file list: the walk only
/// ever needs the entry it is on.
fn survey_directory(dir: &Path, limit: usize) -> Census {
    let mut census = Census::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("Error: cannot read {}", dir.display());
        return census;
    };
    for entry in entries.flatten() {
        if census.files >= limit {
            census.stopped_at = Some(limit);
            break;
        }
        let path = entry.path();
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };
        if !DRAWABLE.contains(&ext.as_str()) {
            continue;
        }
        census.files += 1;
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if crate::target_of(&path, &bytes) == Generation::Gen9 {
            if harvest_gen9(&mut census, &ext, &bytes) {
                census.files_read += 1;
            } else {
                census.refused += 1;
            }
            continue;
        }
        if harvest(&mut census, &ext, &bytes) {
            census.files_read += 1;
        } else {
            census.refused += 1;
        }
    }
    census
}

fn survey(
    game_root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    limit: usize,
    cancel: &AtomicBool,
    warnings: &mut Vec<String>,
) -> Census {
    let mut census = Census::default();
    let _ = crawl::crawl(game_root, provider, warnings, &mut |found| {
        if cancel.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        if census.files >= limit {
            census.stopped_at = Some(limit);
            return ControlFlow::Break(());
        }
        let Some(ext) = found
            .leaf
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
        else {
            return ControlFlow::Continue(());
        };
        if !DRAWABLE.contains(&ext.as_str()) {
            return ControlFlow::Continue(());
        }
        census.files += 1;
        let Ok(bytes) = found.read() else {
            return ControlFlow::Continue(());
        };
        let path = PathBuf::from(found.path);
        if crate::target_of(&path, &bytes) == Generation::Gen9 {
            return ControlFlow::Continue(());
        }
        if harvest(&mut census, &ext, &bytes) {
            census.files_read += 1;
        } else {
            census.refused += 1;
        }
        ControlFlow::Continue(())
    });
    census
}

/// An Enhanced drawable, for the candidate refusals only.
///
/// ⚠ Nothing from this reaches the channel, winding or export-drop figures:
/// those are Legacy facts, and a Gen8 walk must not report on a Gen9 file.
fn harvest_gen9(census: &mut Census, ext: &str, bytes: &[u8]) -> bool {
    let read = match ext {
        "ydr" => match GtaDrawable::<Gen9>::from_bytes(bytes) {
            Ok(_) => {
                census.gen9_drawables += 1;
                true
            }
            Err(_) => false,
        },
        "ydd" => match PgDictionary::<GtaDrawable<Gen9>>::from_bytes(bytes) {
            Ok(dict) => {
                census.gen9_drawables += dict.entries.iter().flatten().count();
                true
            }
            Err(_) => false,
        },
        "yft" => match GtaFragType::<Gen9>::from_bytes(bytes) {
            Ok(frag) => {
                census.gen9_drawables += frag.drawables().len();
                true
            }
            Err(_) => false,
        },
        _ => false,
    };
    if read {
        census.coherence(ext, Generation::Gen9, bytes);
    }
    read
}

/// One file's drawables, counted and dropped.
///
/// ⚠ **Nothing survives this function but numbers.** A per-file
/// [`Scene`](or_interchange::Scene) holds every position, normal, UV and
/// weight of every LOD of every entry, and a `.ydd` of eleven ped components
/// is tens of megabytes of that.
fn harvest(census: &mut Census, ext: &str, bytes: &[u8]) -> bool {
    let read = match ext {
        "ydr" => match GtaDrawable::<Gen8>::from_bytes(bytes) {
            Ok(drawable) => {
                census.drawable(&drawable);
                true
            }
            Err(_) => false,
        },
        "ydd" => match PgDictionary::<GtaDrawable>::from_bytes(bytes) {
            Ok(dict) => {
                for entry in dict.entries.iter().flatten() {
                    census.drawable(entry);
                }
                true
            }
            Err(_) => false,
        },
        "yft" => match GtaFragType::<Gen8>::from_bytes(bytes) {
            Ok(frag) => {
                for drawable in fragment_drawables(&frag) {
                    census.drawable(drawable);
                }
                true
            }
            Err(_) => false,
        },
        _ => false,
    };
    if read {
        census.coherence(ext, Generation::Gen8, bytes);
    }
    read
}

fn report(census: &Census) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "files {} read {} refused {}   Legacy drawables {}   Enhanced drawables {}   \
         geometries {}\n\n",
        census.files,
        census.files_read,
        census.refused,
        census.drawables,
        census.gen9_drawables,
        census.geometries,
    ));

    out.push_str("primitive_type, as the raw byte and as the engine names it\n");
    for (kind, count) in &census.primitive_types {
        out.push_str(&format!(
            "  {kind:<3} read as {:<10} {count:>10}  {:>6.2}%\n",
            primitive_name(*kind).unwrap_or("(unnamed)"),
            percent(*count, census.geometries)
        ));
    }
    out.push_str(&format!(
        "  index run is primitive_count * 3: {:>10}  {:>6.2}%\n",
        census.index_run_is_three_per_primitive,
        percent(census.index_run_is_three_per_primitive, census.geometries)
    ));

    out.push_str("\nchannel presence, over geometries\n");
    for (label, count) in &census.channels {
        out.push_str(&format!(
            "  {label:<12} {count:>10}  {:>6.2}%\n",
            percent(*count, census.geometries)
        ));
    }

    let decided = census.winding_agree + census.winding_disagree;
    out.push_str(&format!(
        "\nwinding, against the stored normals, over {decided} triangles \
         in {} geometries\n  \
         right-hand rule agrees {:>12}  {:>6.2}%\n  \
         disagrees              {:>12}  {:>6.2}%\n  \
         too thin to say        {:>12}\n",
        census.geometries_with_normals,
        census.winding_agree,
        percent64(census.winding_agree, decided),
        census.winding_disagree,
        percent64(census.winding_disagree, decided),
        census.winding_degenerate,
    ));

    out.push_str(&format!(
        "\nlargest vertex run {}   geometries past 65,535 vertices {}\n",
        census.largest_vertex_run, census.geometries_over_u16,
    ));

    out.push_str(&format!(
        "\nwhat an export has to handle\n  \
         geometries with no position channel  {:>12}\n  \
         geometries that are not a tri list   {:>12}\n  \
         triangles indexing past the run      {:>12}\n  \
         skinned geometries                   {:>12}\n  \
         influences off the palette           {:>12}\n  \
         drawables skinned with no skeleton   {:>12}\n  \
         normals with no length               {:>12}\n  \
         tangents with no length              {:>12}\n",
        census.geometries_without_positions,
        census.geometries_not_triangles,
        census.indices_out_of_range,
        census.skinned_geometries,
        census.influences_outside_the_palette,
        census.drawables_skinned_without_a_skeleton,
        census.normals_without_length,
        census.tangents_without_length,
    ));
    out.push_str(
        "\ncandidate refusals, over both targets' drawables, before any becomes an error\n",
    );
    for defect in MeshDefect::ALL {
        let count = census.defects.get(&defect).copied().unwrap_or(0);
        out.push_str(&format!(
            "  {:<42} {count:>12}{}\n",
            defect.label(),
            if count == 0 { "  safe to refuse" } else { "" }
        ));
    }
    for example in &census.defect_examples {
        out.push_str(&format!("    e.g. {example}\n"));
    }

    if let Some(limit) = census.stopped_at {
        out.push_str(&format!(
            "\n⚠ TRUNCATED: the walk stopped at the {limit}-file limit, so every figure \
             above is about those {limit} files and not about the whole of what was \
             pointed at. Raise it with --limit N, or say --limit 0 for all of them.\n"
        ));
    }
    out
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        100.0 * part as f64 / whole as f64
    }
}

fn percent64(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        100.0 * part as f64 / whole as f64
    }
}
