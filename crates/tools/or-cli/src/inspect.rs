use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tracing::{error, warn};

use or_io::codec::anim::ChannelWalk;
use or_io::codec::texture::TextureSource as _;
use or_io::container::paging::{read_pointer, read_u16_raw as read_u16, FIXED_VIRTUAL_BASE};
use or_io::container::resource::{ResourceEnvelope, Rsc7, Rsc7Seal};
use or_io::decl::{Gen8, Gen9, Raw, View};
use or_io::gta5::shaders::fxc::FxcArchive;
use or_io::gta5::{
    CharacterCloth, CrClipDictionary, File as _, GtaDrawable, GtaFragType, PgDictionary, PhBound,
    PhBoundAny, PhBoundComposite, PtxFxList, RmcDrawable,
};

use crate::convert::{
    active_lods, animation_count, cloth_report, entries_of, fragment_bones, has_models,
};
use crate::texture::Listing;

fn get_ypt_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "ypt")
}

fn get_ytd_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "ytd")
}

fn get_ydr_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "ydr")
}

fn get_ydd_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "ydd")
}

fn get_ybn_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "ybn")
}

fn get_yld_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "yld")
}

fn get_yft_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "yft")
}

fn get_ycd_files(dir: &Path) -> Vec<std::path::PathBuf> {
    get_files_by_ext(dir, "ycd")
}

fn get_files_by_ext(dir: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    if !dir.exists() {
        return Vec::new();
    }

    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|e| e == ext).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default()
}

fn is_ypt_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("ypt")
}

fn is_ytd_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("ytd")
}

fn is_ydr_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("ydr")
}

fn is_ydd_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("ydd")
}

fn is_yft_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("yft")
}

fn is_ybn_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("ybn")
}

fn is_yld_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("yld")
}

fn is_ycd_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("ycd")
}

fn is_fxc_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("fxc")
}

pub(crate) fn truncate_name(name: &str, max_len: usize) -> String {
    if name.chars().count() <= max_len {
        return name.to_string();
    }
    let keep: String = name.chars().take(max_len.saturating_sub(3)).collect();
    format!("{keep}...")
}

fn cmd_stats(dir: &Path) -> Result<(), String> {
    let ypt_files = get_ypt_files(dir);
    let ytd_files = get_ytd_files(dir);
    let ydr_files = get_ydr_files(dir);
    let ydd_files = get_ydd_files(dir);
    let yft_files = get_yft_files(dir);
    let ybn_files = get_ybn_files(dir);
    let yld_files = get_yld_files(dir);
    let ycd_files = get_ycd_files(dir);

    if ypt_files.is_empty()
        && ytd_files.is_empty()
        && ydr_files.is_empty()
        && ydd_files.is_empty()
        && yft_files.is_empty()
        && ybn_files.is_empty()
        && yld_files.is_empty()
        && ycd_files.is_empty()
    {
        return Err(format!("No resource files found in {}", dir.display()));
    }

    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║              EXTRACTION STATISTICS REPORT                        ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    let mut total_files = 0;
    let mut total_effects = 0;
    let mut total_emitters = 0;
    let mut total_particles = 0;
    let mut total_textures = 0;
    let mut total_drawables = 0;
    let mut total_bounds = 0;
    let mut files_with_textures = 0;
    let mut files_with_drawables = 0;

    let mut texture_formats: HashMap<String, usize> = HashMap::new();
    let mut texture_sizes: Vec<(u32, u32)> = Vec::new();

    let mut drawables_with_geometry = 0;
    let mut drawables_without_geometry = 0;

    let mut bound_types: HashMap<String, usize> = HashMap::new();

    for fixture_path in &ypt_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let list = match PtxFxList::<Gen8>::from_bytes(&data) {
            Ok(l) => l,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };
        let effects: Vec<_> = (list.effect_rule_dict.iter())
            .flat_map(|d| d.entries.iter().flatten())
            .collect();

        let effect_count = effects.len();

        let mut emitter_count_from_effects = 0;
        let mut particle_count_from_effects = 0;
        for effect in &effects {
            for em in effect.timeline.events.iter().flatten() {
                emitter_count_from_effects += 1;
                if !em.particle_rule_name.is_empty() {
                    particle_count_from_effects += 1;
                }
            }
        }

        let emitter_dict_count = list
            .emitter_rule_dict
            .as_ref()
            .map_or(0, |d| d.entries.len());
        let particle_dict_count = list
            .particle_rule_dict
            .as_ref()
            .map_or(0, |d| d.entries.len());

        let emitter_count = emitter_dict_count.max(emitter_count_from_effects);
        let particle_count = particle_dict_count.max(particle_count_from_effects);

        total_effects += effect_count;
        total_emitters += emitter_count;
        total_particles += particle_count;

        let texture_count = if let Some(dict) = list.texture_dict.as_deref() {
            files_with_textures += 1;
            let count = dict.entries.len();
            total_textures += count;

            for texture in dict.entries.iter().flatten() {
                let format_name = format!("{:?}", texture.pixel_format());
                *texture_formats.entry(format_name).or_insert(0) += 1;
                texture_sizes.push((texture.width as u32, texture.height as u32));
            }

            count
        } else {
            0
        };

        let drawable_count = if let Some(dict) = list.model_dict.as_deref() {
            files_with_drawables += 1;
            let count = dict.entries.len();
            total_drawables += count;

            for drawable in dict.entries.iter().flatten() {
                if has_models(drawable) {
                    drawables_with_geometry += 1;
                } else {
                    drawables_without_geometry += 1;
                }
            }

            count
        } else {
            0
        };

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!("│  Effects: {}", effect_count);
        println!(
            "│  Emitters: {} (dict: {}, from effects: {})",
            emitter_count, emitter_dict_count, emitter_count_from_effects
        );
        println!(
            "│  Particles: {} (dict: {}, from effects: {})",
            particle_count, particle_dict_count, particle_count_from_effects
        );
        if texture_count > 0 || drawable_count > 0 {
            println!(
                "│  Textures: {}, Drawables: {}",
                texture_count, drawable_count
            );
        }
        println!("└────────────────────────────────────────");
    }

    for fixture_path in &ytd_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let dict = match Listing::from_bytes(fixture_path, &data) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };
        let textures = dict.textures();

        files_with_textures += 1;
        let count = textures.len();
        total_textures += count;

        for texture in &textures {
            let format_name = format!("{:?}", texture.pixels.pixel_format());
            *texture_formats.entry(format_name).or_insert(0) += 1;
            texture_sizes.push((texture.width as u32, texture.height as u32));
        }

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!("│  Textures: {}", count);
        println!("└────────────────────────────────────────");
    }

    for fixture_path in &ydr_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let drawable = match GtaDrawable::<Gen8>::from_bytes(&data) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };

        files_with_drawables += 1;
        total_drawables += 1;

        if has_models(&drawable) {
            drawables_with_geometry += 1;
        } else {
            drawables_without_geometry += 1;
        }

        if let Some(bound) = drawable.bound.as_deref() {
            total_bounds += 1;
            let bt = format!("{:?}", bound_header(bound).r#type);
            *bound_types.entry(bt).or_insert(0) += 1;
        }

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!(
            "│  LODs: {}, Geometry: {}, Skeleton: {}",
            active_lods(&drawable),
            if has_models(&drawable) { "Yes" } else { "No" },
            if drawable.skeleton.is_some() {
                "Yes"
            } else {
                "No"
            },
        );
        if !drawable.lights.is_empty() {
            println!("│  Lights: {}", drawable.lights.len());
        }
        if let Some(bound) = drawable.bound.as_deref() {
            println!("│  Bound: {:?}", bound_header(bound).r#type);
        }
        println!("└────────────────────────────────────────");
    }

    for fixture_path in &ydd_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let dict = match PgDictionary::<GtaDrawable>::from_bytes(&data) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };

        let count = entries_of(&dict).count();
        if count > 0 {
            files_with_drawables += 1;
        }
        total_drawables += count;

        for (_, drawable) in entries_of(&dict) {
            if has_models(drawable) {
                drawables_with_geometry += 1;
            } else {
                drawables_without_geometry += 1;
            }
            if let Some(bound) = drawable.bound.as_deref() {
                total_bounds += 1;
                let bt = format!("{:?}", bound_header(bound).r#type);
                *bound_types.entry(bt).or_insert(0) += 1;
            }
        }

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!("│  Drawables: {}", count);
        for (hash, drawable) in entries_of(&dict) {
            println!(
                "│    {:#010X} {:<32} LODs: {}, Geometry: {}, Skeleton: {}",
                hash,
                truncate_name(&drawable.name, 32),
                active_lods(drawable),
                if has_models(drawable) { "Yes" } else { "No" },
                if drawable.skeleton.is_some() {
                    "Yes"
                } else {
                    "No"
                },
            );
        }
        println!("└────────────────────────────────────────");
    }

    for fixture_path in &yft_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let frag = match GtaFragType::<Gen8>::from_bytes(&data) {
            Ok(f) => f,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };

        let fragment_drawables = frag.drawables();
        if !fragment_drawables.is_empty() {
            files_with_drawables += 1;
        }
        total_drawables += fragment_drawables.len();
        for drawable in &fragment_drawables {
            if has_models(drawable) {
                drawables_with_geometry += 1;
            } else {
                drawables_without_geometry += 1;
            }
            if let Some(bound) = drawable.bound.as_deref() {
                total_bounds += 1;
                let bt = format!("{:?}", bound_header(bound).r#type);
                *bound_types.entry(bt).or_insert(0) += 1;
            }
        }
        if let Some(group) = frag.physics_lod_group.as_deref() {
            for lod in group.lods() {
                if let Some(bound) = lod.composite_bounds.as_deref() {
                    total_bounds += 1;
                    let bt = format!("{:?}", bound_header(bound).r#type);
                    *bound_types.entry(bt).or_insert(0) += 1;
                }
            }
        }

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!(
            "│  Name: {}, Drawables: {}, Bones: {}, Physics children: {}, Lights: {}",
            truncate_name(&frag.tune_name, 32),
            fragment_drawables.len(),
            fragment_bones(&frag),
            frag.child_count(),
            frag.lights.len(),
        );
        println!("└────────────────────────────────────────");
    }

    let mut total_cloth = 0usize;
    let mut total_cloth_vertices = 0u32;
    let mut total_cloth_edges = 0u32;
    for fixture_path in &yld_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let report = match cloth_report(&data) {
            Ok(r) => r,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };

        total_cloth += report.entry_count;
        total_cloth_vertices += report.total_vertices();
        total_cloth_edges += report.total_edges();

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!("│  Cloth entries: {}", report.entry_count);
        for entry in &report.entries {
            println!(
                "│    {:#010X} {:<24} LODs: {:?}, Verts: {}, Edges: {}, Pinned: {}",
                entry.name_hash,
                truncate_name(&entry.name, 24),
                entry.populated_lods,
                entry.vertex_count,
                entry.edge_count,
                entry.pinned_vertex_count,
            );
        }
        for warning in report.diagnostics.messages() {
            println!("│  WARN: {warning}");
        }
        println!("└────────────────────────────────────────");
    }

    let mut total_clips = 0usize;
    let mut total_animations = 0usize;
    for fixture_path in &ycd_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let dict = match CrClipDictionary::from_bytes(&data) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };
        let clips: Vec<_> = or_anim::clip::clips(&dict).collect();

        total_clips += clips.len();
        total_animations += animation_count(&dict);

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!(
            "│  Clips: {}, Animations: {}, Hash buckets: {}",
            clips.len(),
            animation_count(&dict),
            dict.clips.hash.len()
        );
        for (key, clip) in clips.iter().take(12) {
            let animation = or_anim::clip::primary(&dict, clip);
            println!(
                "│    {:#010X} {:<28} {:>6.2}s, {} frames, {} bones",
                key,
                truncate_name(&or_anim::clip::header(clip).name.data, 28),
                or_anim::clip::duration(clip),
                animation.map_or(0, |a| a.num_frames),
                animation.map_or(0, |a| or_anim::clip::bone_ids(a).len()),
            );
        }
        if clips.len() > 12 {
            println!("│    ... and {} more", clips.len() - 12);
        }
        println!("└────────────────────────────────────────");
    }

    for fixture_path in &ybn_files {
        let filename = fixture_path.file_name().unwrap().to_string_lossy();
        total_files += 1;

        let data = match fs::read(fixture_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}: Failed to read: {}", filename, e);
                continue;
            }
        };

        let bound = match PhBoundAny::from_bytes(&data) {
            Ok(b) => b,
            Err(e) => {
                warn!("{}: Failed to parse: {}", filename, e);
                continue;
            }
        };
        let kind = bound_header(&bound).r#type;

        total_bounds += 1;
        let bt = format!("{:?}", kind);
        *bound_types.entry(bt).or_insert(0) += 1;

        println!("┌─ {} ({:.1} KB)", filename, data.len() as f64 / 1024.0);
        println!("│  Bound type: {:?}", kind);
        if let PhBoundAny::PhBoundComposite(c) = &bound {
            println!("│  Children: {}", c.bounds.len());
        }
        if let Some(g) = bound_geometry(&bound) {
            println!("│  Vertices: {}", g.compressed_vertices.len());
            println!("│  Polygons: {}", g.polygons.len());
        }
        println!("└────────────────────────────────────────");
    }

    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║                      AGGREGATE SUMMARY                           ║");
    println!("╠══════════════════════════════════════════════════════════════════╣");
    println!(
        "║  Files Processed:        {:>6}                                  ║",
        total_files
    );
    println!("║  ─────────────────────────────────────────────────────────────── ║");
    println!(
        "║  Total Effects:          {:>6}                                  ║",
        total_effects
    );
    println!(
        "║  Total Emitters:         {:>6}                                  ║",
        total_emitters
    );
    println!(
        "║  Total Particles:        {:>6}                                  ║",
        total_particles
    );
    println!("║  ─────────────────────────────────────────────────────────────── ║");
    println!(
        "║  Total Textures:         {:>6}  (in {} files)                   ║",
        total_textures, files_with_textures
    );
    println!(
        "║  Total Drawables:        {:>6}  (in {} files)                   ║",
        total_drawables, files_with_drawables
    );
    println!(
        "║  Total Bounds:           {:>6}                                  ║",
        total_bounds
    );
    if total_cloth > 0 {
        println!("║  ─────────────────────────────────────────────────────────────── ║");
        println!(
            "║  Total Cloth:            {:>6}                                  ║",
            total_cloth
        );
        println!(
            "║  Cloth Vertices:         {:>6}                                  ║",
            total_cloth_vertices
        );
        println!(
            "║  Cloth Constraints:      {:>6}                                  ║",
            total_cloth_edges
        );
    }
    if total_clips > 0 {
        println!("║  ─────────────────────────────────────────────────────────────── ║");
        println!(
            "║  Total Clips:            {:>6}                                  ║",
            total_clips
        );
        println!(
            "║  Total Animations:       {:>6}                                  ║",
            total_animations
        );
    }
    println!("╚══════════════════════════════════════════════════════════════════╝");

    if !texture_formats.is_empty() {
        println!("\n┌─ TEXTURE FORMAT DISTRIBUTION ─────────────────────────────────────┐");
        let mut formats: Vec<_> = texture_formats.iter().collect();
        formats.sort_by(|a, b| b.1.cmp(a.1));
        for (format, count) in formats {
            let percentage = (*count as f64 / total_textures as f64) * 100.0;
            println!(
                "│  {:20} {:>5} ({:>5.1}%)                          │",
                format, count, percentage
            );
        }
        println!("└───────────────────────────────────────────────────────────────────┘");
    }

    if !texture_sizes.is_empty() {
        let mut size_counts: HashMap<(u32, u32), usize> = HashMap::new();
        for size in &texture_sizes {
            *size_counts.entry(*size).or_insert(0) += 1;
        }
        let mut sizes: Vec<_> = size_counts.iter().collect();
        sizes.sort_by(|a, b| b.1.cmp(a.1));

        println!("\n┌─ TOP TEXTURE SIZES ───────────────────────────────────────────────┐");
        for (i, ((w, h), count)) in sizes.iter().take(10).enumerate() {
            println!(
                "│  {}. {:>4}x{:<4}  {:>5} textures                                  │",
                i + 1,
                w,
                h,
                count
            );
        }
        println!("└───────────────────────────────────────────────────────────────────┘");
    }

    if total_drawables > 0 {
        println!("\n┌─ DRAWABLE GEOMETRY STATUS ────────────────────────────────────────┐");
        println!(
            "│  With geometry:    {:>5} ({:>5.1}%)                               │",
            drawables_with_geometry,
            (drawables_with_geometry as f64 / total_drawables as f64) * 100.0
        );
        println!(
            "│  Without geometry: {:>5} ({:>5.1}%)                               │",
            drawables_without_geometry,
            (drawables_without_geometry as f64 / total_drawables as f64) * 100.0
        );
        println!("└───────────────────────────────────────────────────────────────────┘");
    }

    if !bound_types.is_empty() {
        println!("\n┌─ BOUND TYPE DISTRIBUTION ─────────────────────────────────────────┐");
        let mut types: Vec<_> = bound_types.iter().collect();
        types.sort_by(|a, b| b.1.cmp(a.1));
        for (bt, count) in types {
            let percentage = (*count as f64 / total_bounds as f64) * 100.0;
            println!(
                "│  {:20} {:>5} ({:>5.1}%)                          │",
                bt, count, percentage
            );
        }
        println!("└───────────────────────────────────────────────────────────────────┘");
    }

    println!();
    Ok(())
}

fn cmd_textures(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    println!("\n=== ALL TEXTURES IN {} ===\n", path.display());

    let mut all_textures: Vec<(String, u16, u16, String, usize)> = Vec::new();

    if is_ypt_file(path) {
        let failed = |e: or_io::Error| format!("Failed to parse YPT: {}", e);
        let found = if crate::target_of(path, &data).is_gen9() {
            PtxFxList::<Gen9>::from_bytes(&data)
                .map_err(failed)?
                .texture_dict
                .map(|dict| {
                    (dict.entries.iter().flatten())
                        .map(|t| {
                            (
                                t.name.clone(),
                                t.width,
                                t.height,
                                t.pixel_format(),
                                t.pixels.len(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
        } else {
            PtxFxList::<Gen8>::from_bytes(&data)
                .map_err(failed)?
                .texture_dict
                .map(|dict| {
                    (dict.entries.iter().flatten())
                        .map(|t| {
                            let bytes = t.backing_store.len();
                            (t.name.clone(), t.width, t.height, t.pixel_format(), bytes)
                        })
                        .collect::<Vec<_>>()
                })
        };

        if let Some(textures) = found {
            for (name, width, height, format, bytes) in textures {
                all_textures.push((name, width, height, format!("{:?}", format), bytes));
            }
        } else {
            println!("No texture dictionary found in YPT file.");
            return Ok(());
        }
    } else if is_ytd_file(path) {
        let dict = Listing::from_bytes(path, &data)
            .map_err(|e| format!("Failed to parse textures: {}", e))?;

        for texture in dict.textures() {
            all_textures.push((
                texture.name.to_string(),
                texture.width,
                texture.height,
                format!("{:?}", texture.pixels.pixel_format()),
                texture.pixels.payload().len(),
            ));
        }
    } else {
        return Err("Unsupported file type. Use .ypt or .ytd files.".to_string());
    }

    if all_textures.is_empty() {
        println!("No textures found.");
        return Ok(());
    }

    all_textures.sort_by(|a, b| a.0.cmp(&b.0));

    println!(
        "{:<40} {:>6} {:>6} {:>12} {:>12}",
        "Texture Name", "Width", "Height", "Format", "Data Size"
    );
    println!("{:─<82}", "");

    for (name, w, h, format, data_size) in &all_textures {
        let display_name = if name.len() > 38 { &name[..38] } else { name };
        println!(
            "{:<40} {:>6} {:>6} {:>12} {:>12}",
            display_name,
            w,
            h,
            format,
            if *data_size > 0 {
                format!("{} B", data_size)
            } else {
                "N/A".to_string()
            }
        );
    }

    println!("\nTotal: {} textures\n", all_textures.len());
    Ok(())
}

/// A Legacy drawable's name, whether it holds a model, which levels do (as H,M,L,V), and how many models.
fn drawable_info(drawable: &RmcDrawable<Gen8>) -> (String, bool, String, u32) {
    let levels = (["H", "M", "L", "V"].iter().zip(&drawable.lod_group.lods))
        .filter(|(_, lod)| !lod.models.is_empty())
        .map(|(abbr, _)| *abbr)
        .collect::<Vec<_>>()
        .join(",");
    let model_count: u32 = (drawable.lod_group.lods.iter())
        .map(|lod| lod.models.len() as u32)
        .sum();
    (
        drawable.name.clone(),
        has_models(drawable),
        if levels.is_empty() {
            "none".to_string()
        } else {
            levels
        },
        model_count,
    )
}

fn cmd_drawables(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    println!("\n=== ALL DRAWABLES IN {} ===\n", path.display());

    let mut all_drawables: Vec<(String, bool, String, u32)> = Vec::new();

    if is_ypt_file(path) {
        let list = PtxFxList::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse YPT: {}", e))?;

        if let Some(dict) = list.model_dict.as_deref() {
            // A model's name, where its drawable carries none, is the one a particle rule draws it by.
            let rule_names: HashMap<u32, &str> = (list.particle_rule_dict.iter())
                .flat_map(|d| d.entries.iter().flatten())
                .flat_map(|rule| rule.drawables.iter())
                .map(|d| (d.hash_name.get(), d.name.as_str()))
                .collect();
            for (hash, drawable) in dict.codes.iter().zip(&dict.entries) {
                let Some(drawable) = drawable else { continue };
                let mut info = drawable_info(drawable);
                if info.0.is_empty() {
                    info.0 = match rule_names.get(hash) {
                        Some(name) if !name.is_empty() => name.to_string(),
                        _ => format!("model #{:08X}", hash),
                    };
                }
                all_drawables.push(info);
            }
        } else {
            println!("No drawable dictionary found in YPT file.");
            return Ok(());
        }
    } else if is_ydr_file(path) {
        let d = GtaDrawable::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse drawable: {}", e))?;
        all_drawables.push(drawable_info(&d));
    } else if is_ydd_file(path) {
        let dict = PgDictionary::<GtaDrawable>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse drawable dictionary: {}", e))?;
        for (hash, d) in entries_of(&dict) {
            let mut info = drawable_info(d);
            if info.0.is_empty() {
                info.0 = format!("drawable #{hash:08X}");
            }
            all_drawables.push(info);
        }
    } else if is_yft_file(path) {
        let frag = GtaFragType::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse fragment: {}", e))?;
        for (i, d) in frag.drawables().into_iter().enumerate() {
            let mut info = drawable_info(d);
            if info.0.is_empty() {
                info.0 = format!("frag drawable #{i}");
            }
            all_drawables.push(info);
        }
    } else {
        return Err("Unsupported file type. Use .ypt, .ydr, .ydd or .yft files.".to_string());
    }

    if all_drawables.is_empty() {
        println!("No drawables found.");
        return Ok(());
    }

    all_drawables.sort_by(|a, b| a.0.cmp(&b.0));

    println!(
        "{:<45} {:>10} {:>8} {:>8}",
        "Drawable Name", "Geometry", "LODs", "Models"
    );
    println!("{:─<75}", "");

    for (name, has_geo, lods, model_count) in &all_drawables {
        let display_name = if name.len() > 43 { &name[..43] } else { name };
        let geo_str = if *has_geo { "Yes" } else { "No" };
        println!(
            "{:<45} {:>10} {:>8} {:>8}",
            display_name, geo_str, lods, model_count
        );
    }

    println!("\nTotal: {} drawables\n", all_drawables.len());
    Ok(())
}

/// The header every bound starts with.
pub(crate) fn bound_header(bound: &PhBoundAny) -> &PhBound {
    match bound {
        PhBoundAny::PhBoundSphere(b) => &b.base,
        PhBoundAny::PhBoundCapsule(b) => &b.base,
        PhBoundAny::PhBoundBox(b) => &b.base,
        PhBoundAny::PhBoundGeometry(b) => &b.base,
        PhBoundAny::PhBoundBVH(b) => &b.base.base,
        PhBoundAny::PhBoundComposite(b) => &b.base,
        PhBoundAny::PhBoundDisc(b) => &b.base,
        PhBoundAny::PhBoundCylinder(b) => &b.base,
        PhBoundAny::PhBoundPlane(b) => &b.base,
        PhBoundAny::Base(b) => b,
    }
}

/// A bound's type, extent and polygon counts, and a composite's children, indented.
fn print_owned_bound(bound: &PhBoundAny, indent: usize) {
    let pad = "  ".repeat(indent);
    if let PhBoundAny::PhBoundComposite(c) = bound {
        print_owned_composite(c, indent);
        return;
    }
    print_owned_header(bound_header(bound), &pad);
    let geometry = match bound {
        PhBoundAny::PhBoundGeometry(g) => Some(g),
        PhBoundAny::PhBoundBVH(b) => Some(&b.base),
        _ => None,
    };
    if let Some(g) = geometry {
        println!("{}Vertices: {}", pad, g.compressed_vertices.len());
        println!("{}Polygons: {}", pad, g.polygons.len());
    }
}

fn print_owned_header(header: &PhBound, pad: &str) {
    println!("{}Type: {:?}", pad, header.r#type);
    println!("{}Radius: {:.4}", pad, header.radius_around_centroid);
    let min = header.bounding_box_min();
    let max = &header.bounding_box_max_xyz_margin_w;
    println!(
        "{}BBox min: ({:.4}, {:.4}, {:.4})",
        pad, min[0], min[1], min[2]
    );
    println!(
        "{}BBox max: ({:.4}, {:.4}, {:.4})",
        pad, max.x, max.y, max.z
    );
}

fn print_owned_composite(composite: &PhBoundComposite, indent: usize) {
    let pad = "  ".repeat(indent);
    print_owned_header(&composite.base, &pad);
    println!("{}Children: {}", pad, composite.bounds.len());
    for (i, slot) in composite.bounds.iter().enumerate() {
        match slot {
            Some(child) => {
                println!("{}  [{}]", pad, i);
                print_owned_bound(child, indent + 2);
            }
            None => println!("{}  [{}] (empty slot)", pad, i),
        }
    }
}

fn cmd_bounds(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    println!("\n=== BOUNDS IN {} ===\n", path.display());

    if is_ybn_file(path) {
        let bound =
            PhBoundAny::from_bytes(&data).map_err(|e| format!("Failed to parse YBN: {}", e))?;
        print_owned_bound(&bound, 0);
    } else if is_ydr_file(path) {
        let drawable = GtaDrawable::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse drawable: {}", e))?;
        if let Some(bound) = drawable.bound.as_deref() {
            print_owned_bound(bound, 0);
        } else {
            println!("No collision bound in this drawable.");
        }
    } else if is_ydd_file(path) {
        let dict = PgDictionary::<GtaDrawable>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse drawable dictionary: {}", e))?;
        let mut found = 0usize;
        for (hash, drawable) in entries_of(&dict) {
            let Some(bound) = drawable.bound.as_deref() else {
                continue;
            };
            found += 1;
            let label = if drawable.name.is_empty() {
                format!("#{hash:08X}")
            } else {
                drawable.name.clone()
            };
            println!("[{label}]");
            print_owned_bound(bound, 1);
        }
        if found == 0 {
            println!(
                "No collision bounds across {} drawable(s) in this dictionary.",
                entries_of(&dict).count()
            );
        }
    } else if is_yld_file(path) {
        let dict = PgDictionary::<CharacterCloth>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse YLD: {}", e))?;
        let mut found = 0usize;
        for (hash, cloth) in dict.codes.iter().zip(dict.entries.iter()) {
            let Some(cloth) = cloth else { continue };
            let name = cloth
                .cloth_controller
                .as_ref()
                .and_then(|c| std::str::from_utf8(c.base.name()).ok())
                .unwrap_or("");
            let label = if name.is_empty() {
                format!("#{hash:08X}")
            } else {
                name.to_string()
            };
            if let Some(ref bound) = cloth.bound_composite {
                found += 1;
                println!("[{label}] m_BoundComposite");
                print_owned_composite(bound, 1);
            }
            if let Some(ref controller) = cloth.cloth_controller {
                for (lod, verlet) in controller.base.cloth().into_iter().enumerate() {
                    let Some(verlet) = verlet else { continue };
                    if let Some(ref bound) = verlet.custom_bound {
                        found += 1;
                        println!("[{label}] LOD {lod} m_CustomBound");
                        print_owned_bound(bound, 1);
                    }
                    if let Some(ref bound) = verlet.virtual_bound {
                        found += 1;
                        println!("[{label}] LOD {lod} m_VirtualBound");
                        print_owned_bound(bound, 1);
                    }
                }
            }
        }
        if found == 0 {
            println!(
                "No collision bounds across {} cloth entry/entries in this dictionary.",
                dict.entries.len()
            );
        }
    } else if is_yft_file(path) {
        let frag = GtaFragType::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse YFT: {}", e))?;
        let mut found = 0usize;
        for (i, drawable) in frag.drawables().into_iter().enumerate() {
            let Some(bound) = drawable.bound.as_deref() else {
                continue;
            };
            found += 1;
            let label = if drawable.name.is_empty() {
                format!("drawable #{i}")
            } else {
                drawable.name.clone()
            };
            println!("[{label}]");
            print_owned_bound(bound, 1);
        }
        if let Some(group) = frag.physics_lod_group.as_deref() {
            let slots = [&group.high_lod, &group.medium_lod, &group.low_lod];
            for (lod_i, lod) in slots.into_iter().enumerate() {
                let Some(lod) = lod.as_deref() else { continue };
                if let Some(bound) = lod.composite_bounds.as_deref() {
                    found += 1;
                    println!("[physics LOD {lod_i}] m_CompositeBounds");
                    print_owned_bound(bound, 1);
                }
            }
        }
        if found == 0 {
            println!("No collision bounds in this fragment.");
        }
    } else {
        return Err("Unsupported file type. Use .ybn, .ydr, .ydd, .yft or .yld files.".to_string());
    }

    println!();
    Ok(())
}

fn cmd_skeleton(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    println!("\n=== SKELETON IN {} ===\n", path.display());

    let skeleton = if is_yft_file(path) {
        GtaFragType::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse YFT: {}", e))?
            .base
            .common_drawable
            .and_then(|d| d.base.skeleton)
    } else if is_ydr_file(path) {
        GtaDrawable::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse drawable: {}", e))?
            .base
            .skeleton
    } else {
        return Err("Unsupported file type. Use a .yft or .ydr file.".to_string());
    };

    let Some(skel) = skeleton else {
        println!("No skeleton in this file.");
        return Ok(());
    };

    println!("Bones: {}", skel.bones.len());
    println!("Bone-tag slots: {}", skel.bone_tag_slot_count);
    println!();
    println!(
        "{:<5} {:<32} {:>7} {:>7} {:>10}",
        "Idx", "Name", "Parent", "BoneId", "Dofs"
    );
    println!("{:─<70}", "");
    for bone in &skel.bones {
        println!(
            "{:<5} {:<32} {:>7} {:>7} {:>10}",
            bone.index,
            truncate_name(&bone.name, 32),
            bone.parent_index,
            bone.bone_id,
            format!("{:#06X}", bone.dofs),
        );
    }
    println!();
    Ok(())
}

fn cmd_anim(args: &[String]) -> Result<(), String> {
    let clip_path = Path::new(&args[0]);
    let skeleton_path = Path::new(&args[1]);

    let mut clip_name: Option<String> = None;
    let mut at: Option<f32> = None;
    let mut limit = 24usize;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--clip" => {
                clip_name = args.get(index + 1).cloned();
                index += 2;
            }
            "--at" => {
                at = args
                    .get(index + 1)
                    .and_then(|v| v.parse::<f32>().ok())
                    .or_else(|| {
                        eprintln!("Warning: --at needs a number in seconds; ignoring");
                        None
                    });
                index += 2;
            }
            "--limit" => {
                limit = args
                    .get(index + 1)
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(limit);
                index += 2;
            }
            other => {
                return Err(format!("Unknown option {other}"));
            }
        }
    }

    if !is_ycd_file(clip_path) {
        return Err("First argument must be a .ycd file".to_string());
    }
    let clip_data = fs::read(clip_path).map_err(|e| format!("Failed to read clip: {e}"))?;
    let dict = <or_io::gta5::CrClipDictionary as or_io::gta5::File>::from_bytes(&clip_data)
        .map_err(|e| format!("Failed to parse YCD: {e}"))?;

    let skeleton_data = load_skeleton_for_anim(skeleton_path)?;
    let skeleton = or_anim::Skeleton::build(&skeleton_data);

    println!("\n=== ANIMATION BINDING ===\n");
    println!("Clip file:      {}", clip_path.display());
    println!("Skeleton file:  {}", skeleton_path.display());
    println!(
        "Skeleton:       {} bones, signature {:#010X}{}",
        skeleton.bone_count(),
        skeleton.signature,
        if skeleton.file_order_was_topological() {
            ""
        } else {
            " (bones were NOT stored parent-before-child)"
        }
    );
    if let Some(deviation) = skeleton.inverse_bind_deviation() {
        println!("FK deviation:   {deviation:.9} vs m_InverseTransforms");
    }
    println!();

    let clip_label = |clip: &or_io::gta5::CrClipAny| or_anim::clip::header(clip).name.data.clone();
    let clip_count = or_anim::clip::clips(&dict).count();
    let selected: Vec<(u32, &or_io::gta5::CrClipAny)> = match clip_name.as_deref() {
        Some(name) => {
            let found = or_anim::clip::clips(&dict)
                .find(|(_, clip)| clip_label(clip).eq_ignore_ascii_case(name));
            match found {
                Some(pair) => vec![pair],
                None => return Err(format!("No clip named '{name}' in this dictionary")),
            }
        }
        None => or_anim::clip::clips(&dict).take(limit).collect(),
    };

    for (key, clip) in &selected {
        let player = or_anim::ClipPlayer::bind(&skeleton, &dict, clip);
        let report = player.report();
        let name = match clip_label(clip) {
            label if label.is_empty() => format!("(unnamed 0x{key:08X})"),
            label => label,
        };
        println!("{name}");
        println!("  {}", report.summary());
        println!(
            "  coverage {:.0}%{}",
            report.match_ratio() * 100.0,
            if report.looks_compatible() {
                ""
            } else {
                "  <-- probably the wrong skeleton"
            }
        );
        if !report.unresolved_tags.is_empty() {
            let shown: Vec<String> = report
                .unresolved_tags
                .iter()
                .take(12)
                .map(|tag| format!("{tag:#06X}"))
                .collect();
            println!(
                "  unresolved: {}{}",
                shown.join(" "),
                if report.unresolved_tags.len() > shown.len() {
                    format!(" … +{}", report.unresolved_tags.len() - shown.len())
                } else {
                    String::new()
                }
            );
        }

        if let (Some(time), true) = (at, clip_name.is_some()) {
            dump_pose(&mut player.clone(), &skeleton, time, limit);
        }
        println!();
    }

    if clip_name.is_none() && clip_count > selected.len() {
        println!(
            "({} more clips; use --clip <name> or --limit <n>)",
            clip_count - selected.len()
        );
    }
    Ok(())
}

fn dump_pose(
    player: &mut or_anim::ClipPlayer,
    skeleton: &or_anim::Skeleton,
    time: f32,
    limit: usize,
) {
    let pose = player.evaluate(skeleton, time);
    let world = pose.world_matrices(skeleton);

    let mut rows: Vec<(f32, u16)> = world
        .iter()
        .enumerate()
        .map(|(index, matrix)| {
            let moved =
                (matrix.w_axis.truncate() - skeleton.bind_world[index].w_axis.truncate()).length();
            (moved, index as u16)
        })
        .collect();
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    println!("  pose at t = {time:.3}s (most-displaced bones first)");
    println!(
        "    {:<28} {:>9} {:>9} {:>9} {:>9}",
        "bone", "x", "y", "z", "moved"
    );
    for (moved, bone) in rows.iter().take(limit) {
        let matrix = world[*bone as usize];
        let position = matrix.w_axis.truncate();
        println!(
            "    {:<28} {:>9.4} {:>9.4} {:>9.4} {:>9.4}",
            truncate_name(
                skeleton.bone(*bone).map(|b| b.name.as_str()).unwrap_or("?"),
                28
            ),
            position.x,
            position.y,
            position.z,
            moved
        );
    }
    if !player.last_errors().is_empty() {
        for error in player.last_errors() {
            println!("    decode problem: {error}");
        }
    }
}

fn load_skeleton_for_anim(path: &Path) -> Result<or_io::gta5::CrSkeletonData, String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read skeleton: {e}"))?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let skeleton = match extension.as_str() {
        "yft" => GtaFragType::<Gen8>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse YFT: {e}"))?
            .base
            .common_drawable
            .and_then(|d| d.base.skeleton),
        "ydr" => {
            GtaDrawable::<Gen8>::from_bytes(&data)
                .map_err(|e| format!("Failed to parse YDR: {e}"))?
                .base
                .skeleton
        }
        "ydd" => PgDictionary::<GtaDrawable>::from_bytes(&data)
            .map_err(|e| format!("Failed to parse YDD: {e}"))?
            .entries
            .into_iter()
            .flatten()
            .find_map(|entry| entry.base.skeleton),
        other => {
            return Err(format!(
                "Cannot read a skeleton from a .{other} — use a .yft, .ydr or .ydd"
            ))
        }
    };

    skeleton
        .filter(|s| !s.bones.is_empty())
        .map(|s| *s)
        .ok_or_else(|| format!("{} carries no skeleton", path.display()))
}

/// Each clip's reference count, by key, as the file states it.
///
/// ⚠ Read from the file: an owned clip keeps its reference count hidden, and a save derives it.
fn clip_ref_counts(data: &[u8]) -> Result<HashMap<u32, u32>, String> {
    let (pages, virtual_size) = crate::pages(data)?;
    let image = or_io::desc::Image::new(pages.bytes(), virtual_size);
    let e = |e: or_io::decl::ViewError| e.to_string();
    let dict = View::<Raw<CrClipDictionary>>::at_offset(image, 0).map_err(e)?;
    let mut out = HashMap::new();
    for (_, entry) in or_io::gta5::clip_dict::entries(&dict.clips()).map_err(e)? {
        if let Some(clip) = entry.data().map_err(e)? {
            out.insert(entry.key, clip.ref_count);
        }
    }
    Ok(out)
}

fn cmd_clips(path: &Path) -> Result<(), String> {
    if !is_ycd_file(path) {
        return Err("Unsupported file type. Use a .ycd file.".to_string());
    }

    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let dict =
        CrClipDictionary::from_bytes(&data).map_err(|e| format!("Failed to parse YCD: {}", e))?;
    let ref_counts = clip_ref_counts(&data).map_err(|e| format!("Failed to parse YCD: {}", e))?;
    let clips: Vec<_> = or_anim::clip::clips(&dict).collect();

    println!("\n=== CLIPS IN {} ===\n", path.display());
    println!("Clips:          {}", clips.len());
    println!("Animations:     {}", animation_count(&dict));
    println!("Hash buckets:   {}", dict.clips.hash.len());
    println!("Base-name keys: {}", dict.base_name_keys.get());
    println!("Owns anim dict: {}", dict.anim_dictionary_owner.get());

    let mut encoding_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut track_counts: BTreeMap<u8, usize> = BTreeMap::new();
    let mut compact = 0usize;
    let mut undecodable: Vec<String> = Vec::new();

    for (key, clip) in clips {
        let header = or_anim::clip::header(clip);
        let name = &header.name.data;
        println!();
        println!("[{:#010X}] {}", key, name);
        println!(
            "  type: {}, duration: {:.3}s, looped: {}, ref_count: {}",
            header.r#type.raw(),
            or_anim::clip::duration(clip),
            or_anim::clip::is_looped(clip),
            ref_counts.get(&key).copied().unwrap_or(0)
        );
        if let Some(tags) = &header.tags {
            println!(
                "  tags: {} (block tags: {})",
                tags.tags.len(),
                tags.has_block_tags.get()
            );
            for tag in tags.tags.iter().flatten().take(6) {
                println!(
                    "    {:#010X} phase {:.3}..{:.3}, {} attributes",
                    tag.base.name.key,
                    tag.start,
                    tag.end,
                    tag.base.attributes.len()
                );
            }
        }
        if let Some(properties) = &header.properties {
            println!("  properties: {}", properties.properties.entries().count());
        }

        let Some(animation) = or_anim::clip::primary(&dict, clip) else {
            println!("  animation: (none)");
            continue;
        };

        println!(
            "  animation: {} frames, {:.3}s, {} per chunk, {} blocks, {} DOFs",
            animation.num_frames,
            animation.duration,
            animation.frames_per_chunk,
            animation.blocks.len(),
            animation.dofs.len()
        );
        if !animation.name.is_empty() {
            println!("  animation name: {}", animation.name);
        }
        if or_anim::clip::is_compact(animation) {
            compact += 1;
            println!("  NOTE: compact blocks - cannot be decoded or sampled");
            continue;
        }

        for dof in &animation.dofs {
            *track_counts.entry(dof.track).or_insert(0) += 1;
        }

        for (index, block) in animation.blocks.iter().enumerate() {
            let Some(block) = block else { continue };
            match ChannelWalk::new(&**block) {
                Ok(walk) => match walk.channels() {
                    Ok(channels) => {
                        for channel in channels {
                            *encoding_counts
                                .entry(channel.format.engine_name())
                                .or_insert(0) += 1;
                        }
                    }
                    Err(e) => undecodable.push(format!("{} block {index}: {e}", name)),
                },
                Err(e) => undecodable.push(format!("{} block {index}: {e}", name)),
            }
        }
    }

    if !track_counts.is_empty() {
        println!();
        println!("Track distribution:");
        for (track_id, count) in &track_counts {
            println!(
                "  {:>3} {:<20} {:>6}",
                track_id,
                or_anim::clip::track_name(*track_id),
                count
            );
        }
    }

    if !encoding_counts.is_empty() {
        println!();
        println!("Channel encodings (crBlockStream::eFormatType):");
        for (name, count) in &encoding_counts {
            println!("  {:<16} {:>6}", name, count);
        }
    }

    if compact > 0 {
        println!();
        println!("{compact} clip(s) use compact (compressed) blocks. They read and write verbatim");
        println!("but need crAnimCache expansion to decode, which is not implemented.");
    }

    if !undecodable.is_empty() {
        println!();
        println!("Blocks that failed to decode:");
        for message in undecodable.iter().take(20) {
            println!("  - {message}");
        }
    }

    println!();
    Ok(())
}

/// A simulated level's vertex and edge counts and its collision capacity, as the file states them.
#[derive(Clone, Copy)]
struct StatedLevel {
    vertices: i32,
    edges: i32,
    collision_capacity: usize,
}

/// Each cloth's four levels, as the file states them, in dictionary order.
///
/// ⚠ Read from the file: an owned level writes its counts from its runs and
/// keeps its collision capacity hidden.
fn stated_levels(data: &[u8]) -> Result<Vec<[Option<StatedLevel>; 4]>, String> {
    let (pages, virtual_size) = crate::pages(data)?;
    let image = or_io::desc::Image::new(pages.bytes(), virtual_size);
    let e = |e: or_io::decl::ViewError| e.to_string();
    let dict = View::<Raw<PgDictionary<CharacterCloth>>>::at_offset(image, 0).map_err(e)?;
    let entries = dict.entries().map_err(e)?;
    let mut out = Vec::new();
    for i in 0..entries.len() {
        let Some(cloth) = entries.follow(i).map_err(e)? else {
            continue;
        };
        let mut levels = [None; 4];
        if let Some(controller) = cloth.cloth_controller().map_err(e)? {
            let base = controller.base();
            let slots = [
                base.cloth_0().map_err(e)?,
                base.cloth_1().map_err(e)?,
                base.cloth_2().map_err(e)?,
                base.cloth_3().map_err(e)?,
            ];
            for (slot, level) in levels.iter_mut().zip(slots) {
                *slot = level.map(|l| StatedLevel {
                    vertices: { l.num_vertices },
                    edges: { l.num_edges },
                    collision_capacity: { l.collision_inst }.capacity(),
                });
            }
        }
        out.push(levels);
    }
    Ok(out)
}

fn cmd_cloth(path: &Path) -> Result<(), String> {
    if !is_yld_file(path) {
        return Err("Unsupported file type. Use a .yld file.".to_string());
    }

    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let dict = PgDictionary::<CharacterCloth>::from_bytes(&data)
        .map_err(|e| format!("Failed to parse YLD: {}", e))?;
    let stated = stated_levels(&data).map_err(|e| format!("Failed to parse YLD: {}", e))?;

    println!("\n=== CLOTH IN {} ===\n", path.display());
    println!("Entries: {}", dict.entries.len());
    if dict.codes.len() > 1 {
        println!(
            "Hash table ascending: {}",
            dict.codes.windows(2).all(|w| w[0] <= w[1])
        );
    }

    let set_bits =
        |set: &or_io::gta5::AtBitSet| -> u32 { set.bits.iter().map(|w| w.count_ones()).sum() };
    let name = |c: &or_io::gta5::CharacterClothController| {
        std::str::from_utf8(c.base.name()).unwrap_or("").to_string()
    };
    let entries = (dict.codes.iter().zip(&dict.entries))
        .filter_map(|(hash, cloth)| Some((hash, cloth.as_deref()?)));
    for ((hash, cloth), stated) in entries.zip(stated) {
        let controller = cloth.cloth_controller.as_deref();
        println!();
        println!(
            "[{:#010X}] {}",
            hash,
            controller.map(name).unwrap_or_default()
        );
        println!("  is_active:       {}", cloth.is_active.get());
        println!("  negate_force:    {}", cloth.negate_force.get());
        println!("  bone IDs:        {:?}", cloth.bone_id);
        println!("  bone indices:    {} entries", cloth.bone_index.len());
        println!("  poses:           {}", cloth.poses.len());
        match &cloth.bound_composite {
            Some(b) => println!("  bound composite: {:?}", b.base.r#type),
            None => println!("  bound composite: (none)"),
        }

        let Some(controller) = controller else {
            println!("  controller:      (none)");
            continue;
        };

        println!("  controller:");
        println!("    name:                  {}", name(controller));
        println!("    flags:                 {:#X}", controller.base.flags);
        println!(
            "    normal/uv offset:      {} / {}",
            controller.base.normal_offset, controller.base.uv_offset
        );
        println!("    lod:                   {}", controller.base.lod);
        println!(
            "    winding flipped:       {}",
            controller.base.indices_winding_order.get()
        );
        println!(
            "    pinning radius scale:  {}",
            controller.pinning_radius_scale
        );
        println!(
            "    pin radius threshold:  {}",
            controller.pin_radius_set_threshold
        );
        println!("    wind scale:            {}", controller.wind_scale);
        println!("    force pin:             {}", controller.force_pin);
        println!(
            "    package index:         {} (new: {})",
            controller.package_index, controller.new_package_index
        );
        println!(
            "    tri indices:           {}",
            controller.tri_indices.len()
        );
        println!(
            "    original positions:    {}",
            controller.original_pos.len()
        );
        println!("    bone index map:        {:?}", controller.bone_index_map);
        println!("    bone ID map:           {:?}", controller.bone_id_map);
        println!(
            "    bindings:              {}",
            controller.binding_info.len()
        );

        match controller.base.bridge.as_deref() {
            Some(bridge) => {
                println!("    bridge:");
                println!("      mesh verts per LOD:  {:?}", bridge.mesh_verts);
                let lods = 0..or_io::gta5::cloth_dict::LOD_COUNT;
                for lod in lods.clone() {
                    let len = bridge.pin_radius(lod).map_or(0, Vec::len);
                    if len > 0 {
                        println!("      pin radius[{lod}]:      {len} values");
                    }
                }
                for lod in lods.clone() {
                    let len = bridge.vertex_weight(lod).map_or(0, Vec::len);
                    if len > 0 {
                        println!("      vertex weight[{lod}]:   {len} values");
                    }
                }
                for lod in lods {
                    let len = bridge.inflation_scale(lod).map_or(0, Vec::len);
                    if len > 0 {
                        println!("      inflation scale[{lod}]: {len} values");
                    }
                }
                let display_maps = [
                    &bridge.cloth_display_map_0,
                    &bridge.cloth_display_map_1,
                    &bridge.cloth_display_map_2,
                    &bridge.cloth_display_map_3,
                ];
                for (lod, map) in display_maps.iter().enumerate() {
                    if !map.is_empty() {
                        println!("      display map[{lod}]:     {} values", map.len());
                    }
                }
                println!(
                    "      pinnable list:       {} bits, {} set",
                    bridge.pinnable_list.bit_size,
                    set_bits(&bridge.pinnable_list)
                );
            }
            None => println!("    bridge:                (none)"),
        }

        match controller.base.morph_controller.as_deref() {
            Some(morph) => println!(
                "    morph controller:      {} of 4 LOD maps (layout unverified)",
                [
                    &morph.map_data_0,
                    &morph.map_data_1,
                    &morph.map_data_2,
                    &morph.map_data_3,
                ]
                .iter()
                .filter(|m| m.is_some())
                .count()
            ),
            None => println!("    morph controller:      (none)"),
        }

        let levels = controller.base.cloth().into_iter().zip(stated);
        for (lod, (verlet, stated)) in levels.enumerate() {
            let (Some(verlet), Some(stated)) = (verlet, stated) else {
                println!("    m_Cloth[{lod}]:            (null)");
                continue;
            };
            println!("    m_Cloth[{lod}]:");
            println!("      vertices:            {}", stated.vertices);
            println!("      pinned vertices:     {}", verlet.num_pinned_verts);
            println!("      constraints:         {}", stated.edges);
            println!(
                "      custom constraints:  {}",
                verlet.custom_edge_data.len()
            );
            println!("      iterations:          {}", verlet.iterations);
            println!("      flags:               {:#X}", verlet.flags);
            println!("      cloth weight:        {}", verlet.cloth_weight);
            println!(
                "      locked edges:        {} front, {} back",
                verlet.num_locked_edges_front, verlet.num_locked_edges_back
            );
            println!(
                "      switch distances:    up {} / down {}",
                verlet.cloth_data.switch_distance_up, verlet.cloth_data.switch_distance_down
            );
            println!(
                "      positions/normals:   {} / {}",
                verlet.cloth_data.vertex_positions.len(),
                verlet.cloth_data.vertex_initial_normals.len()
            );
            println!(
                "      pinned/prev arrays:  {} / {}",
                verlet.cloth_data.vertex_pinned_positions.len(),
                verlet.cloth_data.vertex_prev_positions.len()
            );
            println!(
                "      dynamic pin list:    {} bits, {} set",
                verlet.dynamic_pin_list.bit_size,
                set_bits(&verlet.dynamic_pin_list)
            );
            println!(
                "      collision inst:      {} of {} capacity",
                verlet.collision_inst.len(),
                stated.collision_capacity
            );
            println!(
                "      custom/virtual bound: {} / {}",
                verlet.custom_bound.is_some(),
                verlet.virtual_bound.is_some()
            );
        }
    }

    println!();
    Ok(())
}

fn cmd_pointers(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let filename = path.file_name().unwrap().to_string_lossy();

    println!("\n=== RAW POINTER DIAGNOSTIC: {} ===\n", filename);

    let seal = Rsc7::peek(&data).map_err(|e| format!("Failed to parse RSC7 header: {}", e))?;

    println!("┌─ {} ({} bytes compressed)", filename, data.len());
    for (label, value) in seal.describe() {
        println!("│  {label}: {value}");
    }

    if is_ypt_file(path) {
        cmd_pointers_ypt(&data, &seal)?;
    } else if is_ytd_file(path) {
        cmd_pointers_ytd(path, &data)?;
    } else if is_ydr_file(path) {
        cmd_pointers_ydr(&data)?;
    } else if is_ydd_file(path) {
        cmd_pointers_ydd(&data)?;
    } else if is_yft_file(path) {
        cmd_pointers_yft(&data)?;
    } else if is_ybn_file(path) {
        cmd_pointers_ybn(&data)?;
    } else if is_yld_file(path) {
        cmd_pointers_yld(&data)?;
    } else {
        return Err(
            "Unsupported file type. Use .ypt, .ytd, .ydr, .ydd, .yft, .ybn or .yld files."
                .to_string(),
        );
    }

    println!("└────────────────────────────────────────\n");
    Ok(())
}

fn cmd_pointers_ypt(data: &[u8], _seal: &Rsc7Seal) -> Result<(), String> {
    let list =
        PtxFxList::<Gen8>::from_bytes(data).map_err(|e| format!("Failed to parse: {}", e))?;
    let effects: Vec<_> = (list.effect_rule_dict.iter())
        .flat_map(|d| d.entries.iter().flatten())
        .collect();

    println!("│  Effects parsed: {}", effects.len());

    let mut total_emitters = 0;
    let mut total_particles = 0;
    for effect in &effects {
        for em in effect.timeline.events.iter().flatten() {
            total_emitters += 1;
            if !em.particle_rule_name.is_empty() {
                total_particles += 1;
            }
        }
    }
    println!("│  Emitters in effects: {}", total_emitters);
    println!("│  Particles in effects: {}", total_particles);

    if let Some(tex_dict) = list.texture_dict.as_deref() {
        println!("│  Texture dictionary: {} textures", tex_dict.entries.len());
        let textures = tex_dict.codes.iter().zip(&tex_dict.entries);
        for (hash, tex) in textures
            .filter_map(|(h, t)| Some((h, t.as_deref()?)))
            .take(5)
        {
            println!(
                "│    - {} ({:#X}): {}x{} {:?}",
                tex.name,
                hash,
                tex.width,
                tex.height,
                tex.pixel_format()
            );
        }
        if tex_dict.entries.len() > 5 {
            println!("│    ... and {} more", tex_dict.entries.len() - 5);
        }
    } else {
        println!("│  Texture dictionary: None");
    }

    if let Some(draw_dict) = list.model_dict.as_deref() {
        let rule_names: HashMap<u32, &str> = (list.particle_rule_dict.iter())
            .flat_map(|d| d.entries.iter().flatten())
            .flat_map(|rule| rule.drawables.iter())
            .map(|d| (d.hash_name.get(), d.name.as_str()))
            .collect();
        println!(
            "│  Drawable dictionary: {} drawables",
            draw_dict.entries.len()
        );
        let drawables = draw_dict.codes.iter().zip(&draw_dict.entries);
        for (hash, draw) in drawables
            .filter_map(|(h, d)| Some((h, d.as_deref()?)))
            .take(5)
        {
            println!(
                "│    - {} ({:#X}): has_geometry={}",
                rule_names.get(hash).copied().unwrap_or(""),
                hash,
                has_models(draw)
            );
        }
        if draw_dict.entries.len() > 5 {
            println!("│    ... and {} more", draw_dict.entries.len() - 5);
        }
    } else {
        println!("│  Drawable dictionary: None");
    }

    println!("│  ");
    println!("│  Sample effect rules:");
    for (i, effect) in effects.iter().take(3).enumerate() {
        let emitters: Vec<_> = effect.timeline.events.iter().flatten().collect();
        println!(
            "│    {}. \"{}\" - {} emitters",
            i + 1,
            effect.name,
            emitters.len()
        );
        for (j, em) in emitters.iter().take(2).enumerate() {
            let particle_info = if !em.particle_rule_name.is_empty() {
                format!("particle: \"{}\"", em.particle_rule_name)
            } else {
                "no particle".to_string()
            };
            println!(
                "│       Emitter {}: \"{}\" - {}",
                j + 1,
                em.emitter_rule_name,
                particle_info
            );
        }
    }

    Ok(())
}

fn cmd_pointers_ytd(path: &Path, data: &[u8]) -> Result<(), String> {
    let resource = Rsc7::open(data).map_err(|e| format!("Failed to decompress: {}", e))?;
    let decompressed = &resource.data;

    println!("│  Decompressed: {} bytes", decompressed.len());

    let entries_ptr = read_pointer(decompressed, 0x30).unwrap_or(0);
    let entries_count = read_u16(decompressed, 0x38).unwrap_or(0);
    let codes_ptr = read_pointer(decompressed, 0x20).unwrap_or(0);
    let codes_count = read_u16(decompressed, 0x28).unwrap_or(0);

    println!("│  Dictionary header:");
    println!(
        "│    Codes ptr: {:#018X} (count: {})",
        codes_ptr, codes_count
    );
    println!(
        "│    Entries ptr: {:#018X} (count: {})",
        entries_ptr, entries_count
    );

    let dict =
        Listing::from_bytes(path, data).map_err(|e| format!("Failed to parse textures: {}", e))?;
    let codes = match &dict {
        Listing::Legacy(d) => &d.codes,
        Listing::Enhanced(d) => &d.codes,
    };
    let textures = dict.textures();

    println!("│  ");
    println!("│  Parsed textures:");
    for (hash, tex) in codes.iter().zip(&textures).take(5) {
        println!(
            "│    - {} ({:#X}): {}x{} {:?}, {} bytes",
            tex.name,
            hash,
            tex.width,
            tex.height,
            tex.pixels.pixel_format(),
            tex.pixels.payload().len()
        );
    }
    if textures.len() > 5 {
        println!("│    ... and {} more", textures.len() - 5);
    }

    Ok(())
}

fn cmd_pointers_ydr(data: &[u8]) -> Result<(), String> {
    let drawable =
        GtaDrawable::<Gen8>::from_bytes(data).map_err(|e| format!("Failed to parse: {}", e))?;

    println!("│  Name: \"{}\"", drawable.name);
    println!("│  Geometry: {}", has_models(&drawable));
    println!("│  Shader group: {}", drawable.shader_group.is_some());
    println!("│  Skeleton: {}", drawable.skeleton.is_some());
    println!("│  Active LODs: {}", active_lods(&drawable));
    println!(
        "│  Bounds radius: {:.4}",
        drawable.lod_group.bounds_sphere.center_and_radius.w
    );
    if !drawable.lights.is_empty() {
        println!("│  Lights: {}", drawable.lights.len());
    }
    if let Some(bound) = drawable.bound.as_deref() {
        println!("│  Bound type: {:?}", bound_header(bound).r#type);
        if let PhBoundAny::PhBoundComposite(c) = bound {
            println!("│  Bound children: {}", c.bounds.len());
        }
    }

    Ok(())
}

/// A dictionary header's vft and parent words, printed as the diagnostic states them.
fn print_dictionary_words(pages: &[u8]) {
    let vft = read_pointer(pages, 0x00).unwrap_or(0);
    if vft != 0 {
        println!("│  Dictionary VFT: {:#X}", vft);
    }
    match read_pointer(pages, 0x10).unwrap_or(0) {
        0 => println!("│  Parent dictionary: (none)"),
        p => println!("│  Parent dictionary: {:#X}", p),
    }
}

/// Where a viewed record sits, as the file addresses it.
fn address_of<R: or_io::decl::Record>(view: &View<'_, R>) -> u64 {
    FIXED_VIRTUAL_BASE + View::offset(view) as u64
}

fn cmd_pointers_ydd(data: &[u8]) -> Result<(), String> {
    let dict = PgDictionary::<GtaDrawable>::from_bytes(data)
        .map_err(|e| format!("Failed to parse: {}", e))?;
    let (pages, virtual_size) = crate::pages(data)?;
    let image = or_io::desc::Image::new(pages.bytes(), virtual_size);
    let e = |e: or_io::decl::ViewError| format!("Failed to parse: {e}");
    let view = View::<Raw<PgDictionary<GtaDrawable>>>::at_offset(image, 0).map_err(e)?;
    let entries = view.entries().map_err(e)?;

    println!("│  Drawables: {}", entries_of(&dict).count());
    print_dictionary_words(pages.bytes());

    for (i, (hash, d)) in dict.codes.iter().zip(&dict.entries).enumerate() {
        let at = entries.follow(i).map_err(e)?.map_or(0, |v| address_of(&v));
        let Some(d) = d else { continue };
        println!("│    {:#010X} \"{}\" @ {:#X}", hash, d.name, at);
        println!(
            "│      geometry: {}, shaders: {}, skeleton: {}, LODs: {}",
            has_models(d),
            d.shader_group.is_some(),
            d.skeleton.is_some(),
            active_lods(d)
        );
        if !d.lights.is_empty() {
            println!("│      lights: {}", d.lights.len());
        }
        if let Some(bound) = d.bound.as_deref() {
            println!("│      bound: {:?}", bound_header(bound).r#type);
        }
    }

    Ok(())
}

fn cmd_pointers_yft(data: &[u8]) -> Result<(), String> {
    let frag =
        GtaFragType::<Gen8>::from_bytes(data).map_err(|e| format!("Failed to parse: {}", e))?;
    let (pages, virtual_size) = crate::pages(data)?;
    let image = or_io::desc::Image::new(pages.bytes(), virtual_size);
    let e = |e: or_io::decl::ViewError| format!("Failed to parse: {e}");
    let common = View::<Raw<GtaFragType>>::at_offset(image, 0)
        .map_err(e)?
        .base()
        .common_drawable()
        .map_err(e)?
        .map(|v| address_of(&v));

    println!("│  Name: \"{}\"", frag.tune_name);
    println!("│  Drawables: {}", frag.drawables().len());
    println!("│  Extra drawables: {}", frag.extra_drawables.len());
    println!("│  Env cloth: {}", frag.env_cloth.len());
    println!("│  Lights: {}", frag.lights.len());
    println!("│  Glass windows: {}", frag.glass_pane_model_infos.len());
    println!("│  Has physics: {}", frag.physics_lod_group.is_some());
    println!("│  Physics children: {}", frag.child_count());
    let skeleton = (frag.common_drawable.as_deref()).and_then(|d| d.skeleton.as_deref());
    if let Some(skel) = skeleton {
        println!("│  Bones: {}", skel.bones.len());
        println!("│  Bone-tag slots: {}", skel.bone_tag_slot_count);
    } else {
        println!("│  Bones: 0");
    }
    if let Some(d) = frag.common_drawable.as_deref() {
        println!(
            "│  Common drawable: \"{}\" @ {:#X} geometry={}",
            d.name,
            common.unwrap_or(0),
            has_models(d)
        );
    }

    Ok(())
}

fn cmd_pointers_yld(data: &[u8]) -> Result<(), String> {
    let dict = PgDictionary::<CharacterCloth>::from_bytes(data)
        .map_err(|e| format!("Failed to parse: {}", e))?;
    let (pages, virtual_size) = crate::pages(data)?;
    let image = or_io::desc::Image::new(pages.bytes(), virtual_size);
    let e = |e: or_io::decl::ViewError| format!("Failed to parse: {e}");
    let view = View::<Raw<PgDictionary<CharacterCloth>>>::at_offset(image, 0).map_err(e)?;
    let entries = view.entries().map_err(e)?;

    println!("│  Cloth entries: {}", dict.entries.len());
    print_dictionary_words(pages.bytes());

    for (i, hash) in dict.codes.iter().enumerate() {
        let Some(cloth) = entries.follow(i).map_err(e)? else {
            continue;
        };
        let owned = dict.entries.get(i).and_then(|c| c.as_deref());
        let name = (owned.and_then(|c| c.cloth_controller.as_deref()))
            .and_then(|c| std::str::from_utf8(c.base.name()).ok())
            .unwrap_or("");
        println!(
            "│    {:#010X} \"{}\" @ {:#X}",
            hash,
            name,
            address_of(&cloth)
        );
        if let Some(bound) = owned.and_then(|c| c.bound_composite.as_deref()) {
            println!("│      m_BoundComposite: {:?}", bound.base.r#type);
        }
        let Some(controller) = cloth.cloth_controller().map_err(e)? else {
            println!("│      (no controller)");
            continue;
        };
        println!("│      controller @ {:#X}", address_of(&controller));
        let base = controller.base();
        if let Some(bridge) = base.bridge().map_err(e)? {
            println!(
                "│        bridge @ {:#X}, mesh verts {:?}",
                address_of(&bridge),
                { bridge.mesh_verts }
            );
        }
        if let Some(morph) = base.morph_controller().map_err(e)? {
            let maps = [
                morph.map_data_0().map_err(e)?.is_some(),
                morph.map_data_1().map_err(e)?.is_some(),
                morph.map_data_2().map_err(e)?.is_some(),
                morph.map_data_3().map_err(e)?.is_some(),
            ];
            println!(
                "│        morph @ {:#X}, {} map(s)",
                address_of(&morph),
                maps.iter().filter(|m| **m).count()
            );
        }
        let levels = [
            base.cloth_0().map_err(e)?,
            base.cloth_1().map_err(e)?,
            base.cloth_2().map_err(e)?,
            base.cloth_3().map_err(e)?,
        ];
        for (lod, verlet) in levels.into_iter().enumerate() {
            match verlet {
                Some(v) => println!(
                    "│        m_Cloth[{lod}] @ {:#X}: {} verts, {} edges, {} custom, pin list {} bits",
                    address_of(&v),
                    { v.num_vertices },
                    { v.num_edges },
                    { v.custom_edge_data }.count(),
                    { v.dynamic_pin_list.bit_size },
                ),
                None => println!("│        m_Cloth[{lod}]: (null)"),
            }
        }
    }

    Ok(())
}

/// A polygonal bound's geometry, which a BVH wraps.
fn bound_geometry(bound: &PhBoundAny) -> Option<&or_io::gta5::PhBoundGeometry> {
    match bound {
        PhBoundAny::PhBoundGeometry(g) => Some(g),
        PhBoundAny::PhBoundBVH(b) => Some(&b.base),
        _ => None,
    }
}

fn cmd_pointers_ybn(data: &[u8]) -> Result<(), String> {
    let bound = PhBoundAny::from_bytes(data).map_err(|e| format!("Failed to parse: {}", e))?;
    let header = bound_header(&bound);

    println!("│  Bound type: {:?}", header.r#type);
    println!("│  Radius: {:.4}", header.radius_around_centroid);
    let (min, max) = (
        header.bounding_box_min(),
        &header.bounding_box_max_xyz_margin_w,
    );
    println!("│  BBox min: ({:.4}, {:.4}, {:.4})", min[0], min[1], min[2]);
    println!("│  BBox max: ({:.4}, {:.4}, {:.4})", max.x, max.y, max.z);
    if let Some(g) = bound_geometry(&bound) {
        println!("│  Vertices: {}", g.compressed_vertices.len());
        println!("│  Polygons: {}", g.polygons.len());
    }
    if let PhBoundAny::PhBoundComposite(composite) = &bound {
        println!("│  Children: {}", composite.bounds.len());
        for (i, slot) in composite.bounds.iter().take(10).enumerate() {
            match slot {
                Some(child) => println!("│    [{}] {:?}", i, bound_header(child).r#type),
                None => println!("│    [{}] (empty slot)", i),
            }
        }
        if composite.bounds.len() > 10 {
            println!("│    ... and {} more", composite.bounds.len() - 10);
        }
    }

    Ok(())
}

fn cmd_diff(original: &Path, converted: &Path) -> Result<(), String> {
    let orig_bytes = fs::read(original).map_err(|e| format!("Failed to read original: {}", e))?;
    let conv_bytes = fs::read(converted).map_err(|e| format!("Failed to read converted: {}", e))?;

    println!(
        "\n{}\nBinary Diff Analysis: {} vs {}\n{}",
        "=".repeat(80),
        original.display(),
        converted.display(),
        "=".repeat(80)
    );

    let orig_resource =
        Rsc7::open(&orig_bytes).map_err(|e| format!("Failed to decompress original: {}", e))?;
    let conv_resource =
        Rsc7::open(&conv_bytes).map_err(|e| format!("Failed to decompress converted: {}", e))?;

    let orig_virtual_size = orig_resource.virtual_size as usize;
    let orig_physical_size = orig_resource.data.len() - orig_virtual_size;
    let conv_virtual_size = conv_resource.virtual_size as usize;
    let conv_physical_size = conv_resource.data.len() - conv_virtual_size;

    println!("Original file:");
    println!("  Compressed size:   {} bytes", orig_bytes.len());
    println!("  Decompressed size: {} bytes", orig_resource.data.len());
    println!("  Virtual size:      {} bytes", orig_virtual_size);
    println!("  Physical size:     {} bytes", orig_physical_size);
    for (label, value) in orig_resource.seal.describe() {
        println!("  {label:<17} {value}");
    }

    println!("\nConverted file:");
    println!("  Compressed size:   {} bytes", conv_bytes.len());
    println!("  Decompressed size: {} bytes", conv_resource.data.len());
    println!("  Virtual size:      {} bytes", conv_virtual_size);
    println!("  Physical size:     {} bytes", conv_physical_size);
    for (label, value) in conv_resource.seal.describe() {
        println!("  {label:<17} {value}");
    }

    println!("\n--- VIRTUAL SECTION COMPARISON ---");
    let orig_virtual = &orig_resource.data[..orig_virtual_size];
    let conv_virtual = &conv_resource.data[..conv_virtual_size];

    if orig_virtual_size != conv_virtual_size {
        println!(
            "WARNING: Virtual section sizes differ: {} vs {} bytes",
            orig_virtual_size, conv_virtual_size
        );
    }

    let compare_len = orig_virtual_size.min(conv_virtual_size);
    if let Some(diff_offset) =
        find_first_diff(&orig_virtual[..compare_len], &conv_virtual[..compare_len])
    {
        println!(
            "FIRST DIFFERENCE at virtual offset {:#X} ({} bytes)",
            diff_offset, diff_offset
        );
        hex_dump_region(orig_virtual, diff_offset, 32, "Original");
        hex_dump_region(conv_virtual, diff_offset, 32, "Converted");
    } else if orig_virtual_size != conv_virtual_size {
        println!(
            "Virtual content matches for first {} bytes, but sizes differ",
            compare_len
        );
    } else {
        println!(
            "Virtual sections are IDENTICAL ({} bytes)",
            orig_virtual_size
        );
    }

    println!("\n--- PHYSICAL SECTION COMPARISON ---");
    let orig_physical = &orig_resource.data[orig_virtual_size..];
    let conv_physical = &conv_resource.data[conv_virtual_size..];

    if orig_physical.len() != conv_physical.len() {
        println!(
            "Physical sections have different sizes: {} vs {}",
            orig_physical.len(),
            conv_physical.len()
        );
    }

    let compare_len = orig_physical.len().min(conv_physical.len());
    if compare_len > 0 {
        if let Some(diff_offset) =
            find_first_diff(&orig_physical[..compare_len], &conv_physical[..compare_len])
        {
            println!("FIRST DIFFERENCE at physical offset {:#X}", diff_offset);
            println!(
                "  (absolute offset: {:#X})",
                orig_virtual_size + diff_offset
            );
            hex_dump_region(orig_physical, diff_offset, 32, "Original");
            hex_dump_region(conv_physical, diff_offset, 32, "Converted");
        } else if orig_physical.len() == conv_physical.len() {
            println!(
                "Physical sections are IDENTICAL ({} bytes)",
                orig_physical.len()
            );
        } else {
            println!("Physical content matches for first {} bytes", compare_len);
        }
    }

    let total_match = orig_virtual == conv_virtual && orig_physical == conv_physical;

    println!("\n--- SUMMARY ---");
    if total_match {
        println!("SUCCESS: Decompressed data is BINARY IDENTICAL");
        println!("Any compressed file differences are due to DEFLATE non-determinism");
    } else {
        println!("FAILURE: Decompressed data differs - serialization needs investigation");
    }

    println!();
    Ok(())
}

fn find_first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    let min_len = a.len().min(b.len());
    for i in 0..min_len {
        if a[i] != b[i] {
            return Some(i);
        }
    }
    if a.len() != b.len() {
        return Some(min_len);
    }
    None
}

fn hex_dump_region(data: &[u8], center: usize, context: usize, label: &str) {
    let start = center.saturating_sub(context);
    let end = (center + context).min(data.len());

    println!("  {} (around offset {:#X}):", label, center);
    for offset in (start..end).step_by(16) {
        let line_end = (offset + 16).min(end);
        let bytes = &data[offset..line_end];

        print!("    {:06X}: ", offset);
        for (i, byte) in bytes.iter().enumerate() {
            if offset + i == center {
                print!("[{:02X}]", byte);
            } else {
                print!(" {:02X} ", byte);
            }
        }

        for _ in bytes.len()..16 {
            print!("    ");
        }

        print!(" | ");
        for byte in bytes {
            let c = if *byte >= 0x20 && *byte < 0x7F {
                *byte as char
            } else {
                '.'
            };
            print!("{}", c);
        }
        println!();
    }
}

fn cmd_layout(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let filename = path.file_name().unwrap().to_string_lossy();

    println!(
        "\n{}\nMemory Layout Analysis: {}\n{}",
        "=".repeat(80),
        filename,
        "=".repeat(80)
    );

    let resource = Rsc7::open(&data).map_err(|e| format!("Failed to decompress: {}", e))?;

    let virtual_size = resource.virtual_size as usize;
    let virtual_data = &resource.data[..virtual_size];
    let physical_size = resource.data.len() - virtual_size;

    println!(
        "Virtual section size: {} bytes ({:#X})",
        virtual_data.len(),
        virtual_data.len()
    );
    println!(
        "Physical section size: {} bytes ({:#X})",
        physical_size, physical_size
    );
    for (label, value) in resource.seal.describe() {
        println!("Header {label}: {value}");
    }

    if is_ytd_file(path) {
        cmd_layout_ytd(virtual_data)?;
    } else if is_ypt_file(path) {
        cmd_layout_ypt(virtual_data)?;
    } else if is_ydr_file(path) {
        cmd_layout_ydr(virtual_data)?;
    } else if is_ydd_file(path) {
        cmd_layout_ydd(virtual_data)?;
    } else if is_yft_file(path) {
        cmd_layout_yft(virtual_data)?;
    } else if is_ybn_file(path) {
        cmd_layout_ybn(virtual_data)?;
    } else if is_yld_file(path) {
        cmd_layout_yld(virtual_data)?;
    }

    Ok(())
}

fn cmd_layout_yld(virtual_data: &[u8]) -> Result<(), String> {
    if virtual_data.len() < 0x40 {
        return Err("Virtual data too small for dictionary header".to_string());
    }

    println!("\n--- Cloth Dictionary Header (0x00-0x3F) ---");
    let vft = u64::from_le_bytes(virtual_data[0x00..0x08].try_into().unwrap());
    let first_node = u64::from_le_bytes(virtual_data[0x08..0x10].try_into().unwrap());
    let parent = u64::from_le_bytes(virtual_data[0x10..0x18].try_into().unwrap());
    let ref_count = u32::from_le_bytes(virtual_data[0x18..0x1C].try_into().unwrap());
    let codes_ptr = u64::from_le_bytes(virtual_data[0x20..0x28].try_into().unwrap());
    let codes_count = u16::from_le_bytes(virtual_data[0x28..0x2A].try_into().unwrap());
    let entries_ptr = u64::from_le_bytes(virtual_data[0x30..0x38].try_into().unwrap());
    let entries_count = u16::from_le_bytes(virtual_data[0x38..0x3A].try_into().unwrap());

    println!("  vft:           {:#018X}", vft);
    println!("  first_node:    {:#018X}", first_node);
    println!("  parent:        {:#018X}", parent);
    println!("  ref_count:     {}", ref_count);
    println!(
        "  codes:         ptr={:#018X} count={}",
        codes_ptr, codes_count
    );
    println!(
        "  entries:       ptr={:#018X} count={}",
        entries_ptr, entries_count
    );

    let base = FIXED_VIRTUAL_BASE;
    let to_offset = |addr: u64| -> Option<usize> {
        if addr < base {
            return None;
        }
        let off = (addr - base) as usize;
        (off < virtual_data.len()).then_some(off)
    };

    let Some(entries_off) = to_offset(entries_ptr) else {
        println!("  (entry table pointer is out of range)");
        return Ok(());
    };

    println!("\n--- characterCloth entries ---");
    let mut addrs: Vec<u64> = Vec::new();
    for i in 0..entries_count as usize {
        let at = entries_off + i * 8;
        if at + 8 > virtual_data.len() {
            break;
        }
        let addr = u64::from_le_bytes(virtual_data[at..at + 8].try_into().unwrap());
        addrs.push(addr);
        let hash = to_offset(codes_ptr)
            .and_then(|c| virtual_data.get(c + i * 4..c + i * 4 + 4))
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        match hash {
            Some(h) => println!("  [{i}] {h:#010X} -> {addr:#018X}"),
            None => println!("  [{i}] (hash unavailable) -> {addr:#018X}"),
        }
    }

    let mut sorted = addrs.clone();
    sorted.sort_unstable();
    let stride = sorted.windows(2).map(|w| w[1] - w[0]).min();
    if let Some(stride) = stride {
        println!(
            "  minimum gap between adjacent entries: {stride:#X} \
             (characterCloth is 0xD0)"
        );
    }

    Ok(())
}

fn cmd_layout_ydd(virtual_data: &[u8]) -> Result<(), String> {
    if virtual_data.len() < 0x40 {
        return Err("Virtual data too small for dictionary header".to_string());
    }

    println!("\n--- Drawable Dictionary Header (0x00-0x3F) ---");
    let vft = u64::from_le_bytes(virtual_data[0x00..0x08].try_into().unwrap());
    let first_node = u64::from_le_bytes(virtual_data[0x08..0x10].try_into().unwrap());
    let parent = u64::from_le_bytes(virtual_data[0x10..0x18].try_into().unwrap());
    let ref_count = u32::from_le_bytes(virtual_data[0x18..0x1C].try_into().unwrap());
    let codes_ptr = u64::from_le_bytes(virtual_data[0x20..0x28].try_into().unwrap());
    let codes_count = u16::from_le_bytes(virtual_data[0x28..0x2A].try_into().unwrap());
    let codes_capacity = u16::from_le_bytes(virtual_data[0x2A..0x2C].try_into().unwrap());
    let entries_ptr = u64::from_le_bytes(virtual_data[0x30..0x38].try_into().unwrap());
    let entries_count = u16::from_le_bytes(virtual_data[0x38..0x3A].try_into().unwrap());
    let entries_capacity = u16::from_le_bytes(virtual_data[0x3A..0x3C].try_into().unwrap());

    println!("  vft:           {:#018X}", vft);
    println!("  first_node:    {:#018X}", first_node);
    println!("  parent:        {:#018X}", parent);
    println!("  ref_count:     {}", ref_count);
    println!(
        "  codes:         ptr={:#018X} count={} capacity={}",
        codes_ptr, codes_count, codes_capacity
    );
    println!(
        "  entries:       ptr={:#018X} count={} capacity={}",
        entries_ptr, entries_count, entries_capacity
    );

    let base = FIXED_VIRTUAL_BASE;
    let to_offset = |addr: u64| -> Option<usize> {
        if addr < base {
            return None;
        }
        let off = (addr - base) as usize;
        (off < virtual_data.len()).then_some(off)
    };

    let Some(table_off) = to_offset(entries_ptr) else {
        println!("\n  (entry table pointer is not in the virtual heap)");
        return Ok(());
    };

    let mut addrs = Vec::new();
    for i in 0..entries_count as usize {
        let at = table_off + i * 8;
        if at + 8 > virtual_data.len() {
            break;
        }
        addrs.push(u64::from_le_bytes(
            virtual_data[at..at + 8].try_into().unwrap(),
        ));
    }

    println!("\n--- Entries ---");
    let hashes_off = to_offset(codes_ptr);
    for (i, addr) in addrs.iter().enumerate() {
        let hash = hashes_off.and_then(|h| {
            let at = h + i * 4;
            (at + 4 <= virtual_data.len())
                .then(|| u32::from_le_bytes(virtual_data[at..at + 4].try_into().unwrap()))
        });
        match hash {
            Some(h) => println!("  [{i:3}] {h:#010X} -> {addr:#018X}"),
            None => println!("  [{i:3}] (no hash)  -> {addr:#018X}"),
        }
    }

    if addrs.len() > 1 {
        let mut sorted = addrs.clone();
        sorted.sort_unstable();
        let strides: Vec<u64> = sorted.windows(2).map(|w| w[1] - w[0]).collect();
        let min = strides.iter().copied().min().unwrap_or(0);
        println!("\n--- Entry stride ---");
        println!("  minimum gap: {min:#X} ({min} bytes)");
        println!(
            "  implies:     {}",
            match min {
                0xD0 => "gtaDrawable (0xD0) — lights, tint and bound present",
                0xB0 => "rmcDrawable (0xB0) — bare drawable",
                _ => "inconclusive (entries are not adjacent in this file)",
            }
        );
    }

    Ok(())
}

fn cmd_layout_ytd(virtual_data: &[u8]) -> Result<(), String> {
    if virtual_data.len() < 0x40 {
        return Err("Virtual data too small for dictionary header".to_string());
    }

    println!("\n--- Dictionary Header (0x00-0x3F) ---");

    let vft = u64::from_le_bytes(virtual_data[0x00..0x08].try_into().unwrap());
    let first_node = u64::from_le_bytes(virtual_data[0x08..0x10].try_into().unwrap());
    let parent = u64::from_le_bytes(virtual_data[0x10..0x18].try_into().unwrap());
    let entry_count = u16::from_le_bytes(virtual_data[0x18..0x1A].try_into().unwrap());

    let codes_ptr = u64::from_le_bytes(virtual_data[0x20..0x28].try_into().unwrap());
    let codes_count = u16::from_le_bytes(virtual_data[0x28..0x2A].try_into().unwrap());
    let codes_capacity = u16::from_le_bytes(virtual_data[0x2A..0x2C].try_into().unwrap());

    let entries_ptr = u64::from_le_bytes(virtual_data[0x30..0x38].try_into().unwrap());
    let entries_count = u16::from_le_bytes(virtual_data[0x38..0x3A].try_into().unwrap());
    let entries_capacity = u16::from_le_bytes(virtual_data[0x3A..0x3C].try_into().unwrap());

    println!("  vft:           {:#018X}", vft);
    println!("  first_node:    {:#018X}", first_node);
    println!("  parent:        {:#018X}", parent);
    println!("  entry_count:   {}", entry_count);
    println!(
        "  codes.data:    {:#018X} (offset: {:#X})",
        codes_ptr,
        codes_ptr.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  codes.count:   {} (capacity: {})",
        codes_count, codes_capacity
    );
    println!(
        "  entries.data:  {:#018X} (offset: {:#X})",
        entries_ptr,
        entries_ptr.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  entries.count: {} (capacity: {})",
        entries_count, entries_capacity
    );

    let mut regions: Vec<(usize, usize, &str)> = vec![(0x00, 0x40, "pgDictionary header")];

    if entries_ptr >= FIXED_VIRTUAL_BASE && entries_count > 0 {
        let entries_offset = (entries_ptr - FIXED_VIRTUAL_BASE) as usize;
        for i in 0..entries_count as usize {
            let ptr_offset = entries_offset + i * 8;
            if ptr_offset + 8 <= virtual_data.len() {
                let tex_ptr = u64::from_le_bytes(
                    virtual_data[ptr_offset..ptr_offset + 8].try_into().unwrap(),
                );
                let tex_offset = (tex_ptr - FIXED_VIRTUAL_BASE) as usize;
                if tex_offset + 0x90 <= virtual_data.len() {
                    regions.push((tex_offset, tex_offset + 0x90, "grcTexturePC"));
                }
            }
        }

        let entries_size = entries_count as usize * 8;
        regions.push((
            entries_offset,
            entries_offset + entries_size,
            "entries array",
        ));
    }

    if codes_ptr >= FIXED_VIRTUAL_BASE && codes_count > 0 {
        let codes_offset = (codes_ptr - FIXED_VIRTUAL_BASE) as usize;
        let codes_size = codes_count as usize * 4;
        regions.push((codes_offset, codes_offset + codes_size, "codes array"));
    }

    regions.sort_by_key(|r| r.0);

    println!("\n  Offset Range         Size      Description");
    println!("  -------------------- --------- ------------------");
    let mut last_end = 0usize;
    for (start, end, desc) in &regions {
        if *start > last_end {
            let gap = start - last_end;
            println!(
                "  {:#06X} - {:#06X}      {:>5}     [GAP/PADDING]",
                last_end, start, gap
            );
        }
        println!(
            "  {:#06X} - {:#06X}      {:>5}     {}",
            start,
            end,
            end - start,
            desc
        );
        last_end = *end;
    }

    if last_end < virtual_data.len() {
        println!(
            "  {:#06X} - {:#06X}      {:>5}     [TRAILING PADDING]",
            last_end,
            virtual_data.len(),
            virtual_data.len() - last_end
        );
    }

    println!();
    Ok(())
}

fn cmd_layout_ypt(virtual_data: &[u8]) -> Result<(), String> {
    if virtual_data.len() < 0x60 {
        return Err("Virtual data too small for ptxFxList header".to_string());
    }

    println!("\n--- ptxFxList Header (0x00-0x60) ---");

    let vft = read_pointer(virtual_data, 0x00).unwrap_or(0);
    let texture_dict = read_pointer(virtual_data, 0x20).unwrap_or(0);
    let model_dict = read_pointer(virtual_data, 0x30).unwrap_or(0);
    let particle_dict = read_pointer(virtual_data, 0x40).unwrap_or(0);
    let emitter_dict = read_pointer(virtual_data, 0x48).unwrap_or(0);
    let effect_dict = read_pointer(virtual_data, 0x50).unwrap_or(0);

    println!("  VFT:            {:#018X}", vft);
    println!(
        "  TextureDict:    {:#018X} (offset: {:#X})",
        texture_dict,
        texture_dict.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  ModelDict:      {:#018X} (offset: {:#X})",
        model_dict,
        model_dict.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  ParticleDict:   {:#018X} (offset: {:#X})",
        particle_dict,
        particle_dict.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  EmitterDict:    {:#018X} (offset: {:#X})",
        emitter_dict,
        emitter_dict.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  EffectDict:     {:#018X} (offset: {:#X})",
        effect_dict,
        effect_dict.saturating_sub(FIXED_VIRTUAL_BASE)
    );

    println!();
    Ok(())
}

fn cmd_layout_yft(virtual_data: &[u8]) -> Result<(), String> {
    if virtual_data.len() < 0x130 {
        return Err("Virtual data too small for gtaFragType header".to_string());
    }

    println!("\n--- gtaFragType Header (0x00-0x130) ---");

    let vft = read_pointer(virtual_data, 0x00).unwrap_or(0);
    let common = read_pointer(virtual_data, 0x30).unwrap_or(0);
    let extras = read_pointer(virtual_data, 0x38).unwrap_or(0);
    let name = read_pointer(virtual_data, 0x58).unwrap_or(0);
    let shared_matrix = read_pointer(virtual_data, 0xA8).unwrap_or(0);
    let glass = read_pointer(virtual_data, 0xE0).unwrap_or(0);
    let physics = read_pointer(virtual_data, 0xF0).unwrap_or(0);
    let lights = read_pointer(virtual_data, 0x110).unwrap_or(0);
    let lights_count = read_u16(virtual_data, 0x118).unwrap_or(0);

    let sphere = &virtual_data[0x20..0x30];
    let cx = f32::from_le_bytes(sphere[0..4].try_into().unwrap());
    let cy = f32::from_le_bytes(sphere[4..8].try_into().unwrap());
    let cz = f32::from_le_bytes(sphere[8..12].try_into().unwrap());
    let cr = f32::from_le_bytes(sphere[12..16].try_into().unwrap());

    println!("  VFT:               {:#018X}", vft);
    println!(
        "  BoundingSphere:    ({:.4}, {:.4}, {:.4}), r={:.4}",
        cx, cy, cz, cr
    );
    println!(
        "  CommonDrawable:    {:#018X} (offset: {:#X})",
        common,
        common.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  ExtraDrawables:    {:#018X} (offset: {:#X})",
        extras,
        extras.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  TuneName:          {:#018X} (offset: {:#X})",
        name,
        name.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  SharedMatrixSet:   {:#018X} (offset: {:#X})",
        shared_matrix,
        shared_matrix.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  GlassWindows:      {:#018X} (offset: {:#X})",
        glass,
        glass.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  PhysicsLodGroup:   {:#018X} (offset: {:#X})",
        physics,
        physics.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  Lights:            {:#018X} (count: {})",
        lights, lights_count
    );

    println!();
    Ok(())
}

fn cmd_layout_ydr(virtual_data: &[u8]) -> Result<(), String> {
    if virtual_data.len() < 0xD0 {
        return Err("Virtual data too small for gtaDrawable header".to_string());
    }

    println!("\n--- gtaDrawable Header (0x00-0xD0) ---");

    let vft = read_pointer(virtual_data, 0x00).unwrap_or(0);
    let shader_group = read_pointer(virtual_data, 0x10).unwrap_or(0);
    let skeleton = read_pointer(virtual_data, 0x18).unwrap_or(0);

    println!("  VFT:            {:#018X}", vft);
    println!(
        "  ShaderGroup:    {:#018X} (offset: {:#X})",
        shader_group,
        shader_group.saturating_sub(FIXED_VIRTUAL_BASE)
    );
    println!(
        "  Skeleton:       {:#018X} (offset: {:#X})",
        skeleton,
        skeleton.saturating_sub(FIXED_VIRTUAL_BASE)
    );

    let bounds_center = &virtual_data[0x20..0x30];
    let cx = f32::from_le_bytes(bounds_center[0..4].try_into().unwrap());
    let cy = f32::from_le_bytes(bounds_center[4..8].try_into().unwrap());
    let cz = f32::from_le_bytes(bounds_center[8..12].try_into().unwrap());
    let cr = f32::from_le_bytes(bounds_center[12..16].try_into().unwrap());
    println!(
        "  BoundsCenter:   ({:.4}, {:.4}, {:.4}), r={:.4}",
        cx, cy, cz, cr
    );

    for i in 0..4u64 {
        let lod_ptr = read_pointer(virtual_data, (0x50 + i * 0x10) as usize).unwrap_or(0);
        let lod_count = read_u16(virtual_data, (0x58 + i * 0x10) as usize).unwrap_or(0);
        let label = match i {
            0 => "High",
            1 => "Medium",
            2 => "Low",
            _ => "VeryLow",
        };
        if lod_ptr != 0 || lod_count != 0 {
            println!(
                "  LOD {} ptr:   {:#018X} (count: {})",
                label, lod_ptr, lod_count
            );
        }
    }

    let lights_ptr = read_pointer(virtual_data, 0xB0).unwrap_or(0);
    let lights_count = read_u16(virtual_data, 0xB8).unwrap_or(0);
    let bound_ptr = read_pointer(virtual_data, 0xC8).unwrap_or(0);

    println!(
        "  Lights:         {:#018X} (count: {})",
        lights_ptr, lights_count
    );
    println!(
        "  Bound:          {:#018X} (offset: {:#X})",
        bound_ptr,
        bound_ptr.saturating_sub(FIXED_VIRTUAL_BASE)
    );

    println!();
    Ok(())
}

fn cmd_layout_ybn(virtual_data: &[u8]) -> Result<(), String> {
    if virtual_data.len() < 0x70 {
        return Err("Virtual data too small for phBound header".to_string());
    }

    println!("\n--- phBound Header (0x00-0x70) ---");

    let vft = read_pointer(virtual_data, 0x00).unwrap_or(0);
    let bound_type = virtual_data[0x10];
    let radius = f32::from_le_bytes(virtual_data[0x18..0x1C].try_into().unwrap());

    println!("  VFT:            {:#018X}", vft);
    println!("  Type:           {} ({:#04X})", bound_type, bound_type);
    println!("  Radius:         {:.4}", radius);

    let bb_max = &virtual_data[0x20..0x30];
    let bb_min = &virtual_data[0x30..0x40];
    let max_x = f32::from_le_bytes(bb_max[0..4].try_into().unwrap());
    let max_y = f32::from_le_bytes(bb_max[4..8].try_into().unwrap());
    let max_z = f32::from_le_bytes(bb_max[8..12].try_into().unwrap());
    let min_x = f32::from_le_bytes(bb_min[0..4].try_into().unwrap());
    let min_y = f32::from_le_bytes(bb_min[4..8].try_into().unwrap());
    let min_z = f32::from_le_bytes(bb_min[8..12].try_into().unwrap());

    println!(
        "  BBox max:       ({:.4}, {:.4}, {:.4})",
        max_x, max_y, max_z
    );
    println!(
        "  BBox min:       ({:.4}, {:.4}, {:.4})",
        min_x, min_y, min_z
    );

    if bound_type == 10 && virtual_data.len() >= 0xB0 {
        let bounds_ptr = read_pointer(virtual_data, 0x70).unwrap_or(0);
        let num_bounds = read_u16(virtual_data, 0x98).unwrap_or(0);
        println!(
            "  Bounds ptr:     {:#018X} (count: {})",
            bounds_ptr, num_bounds
        );
    }

    println!();
    Ok(())
}

fn cmd_fxc(path: &Path) -> Result<(), String> {
    if !is_fxc_file(path) {
        return Err("Unsupported file type. Use .fxc files.".to_string());
    }

    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let filename = path.file_name().unwrap().to_string_lossy();

    let archive = FxcArchive::parse(&data).map_err(|e| format!("Failed to parse FXC: {}", e))?;

    println!(
        "\n=== FXC: {} ({:.1} KB, {} bytes) ===\n",
        filename,
        data.len() as f64 / 1024.0,
        data.len()
    );

    if archive.is_empty() {
        println!("No DXBC blobs found.\n");
        return Ok(());
    }

    println!(
        "{:>3}  {:<10}  {:>10}  {:>5}  Parts",
        "#", "Offset", "Size (B)", "Stage"
    );
    println!("{:─<72}", "");

    for (i, blob) in archive.blobs(&data).enumerate() {
        let parts = blob.parts();
        let parts_str: Vec<_> = parts.iter().map(|p| p.tag_string()).collect();
        println!(
            "{:>3}  {:#010X}  {:>10}  {:>5}  [{}]",
            i,
            blob.offset,
            blob.size,
            blob.detected_stage().short_code(),
            parts_str.join(", ")
        );
    }

    let counts = archive.stage_counts(&data);
    println!();
    println!(
        "Total: {} blobs ({} VS, {} PS, {} GS, {} HS, {} DS, {} CS, {} unknown)",
        archive.len(),
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4],
        counts[5],
        counts[6],
    );
    println!();
    Ok(())
}

fn print_usage(program: &str) {
    eprintln!("RAGE Resource Inspection Tool");
    eprintln!();
    eprintln!("Deep inspection and debugging utilities for GTA V resource files.");
    eprintln!();
    eprintln!("Usage:");
    eprintln!(
        "  {} stats <dir>              Aggregate statistics for resource files",
        program
    );
    eprintln!(
        "  {} textures <file>          List all textures with properties",
        program
    );
    eprintln!(
        "  {} drawables <file>         List all drawables with LOD info",
        program
    );
    eprintln!(
        "  {} bounds <file>            Inspect physics bounds",
        program
    );
    eprintln!(
        "  {} skeleton <file>          Dump bone hierarchy (.yft / .ydr)",
        program
    );
    eprintln!(
        "  {} cloth <file>             Inspect cloth simulation data (.yld)",
        program
    );
    eprintln!(
        "  {} clips <file>             Inspect animation clips (.ycd)",
        program
    );
    eprintln!(
        "  {} pointers <file>          Raw pointer diagnostic for debugging",
        program
    );
    eprintln!(
        "  {} diff <orig> <conv>       Binary diff analysis between files",
        program
    );
    eprintln!(
        "  {} layout <file>            Memory layout analysis",
        program
    );
    eprintln!(
        "  {} fxc <file>               List DXBC blobs in a .fxc shader container",
        program
    );
    eprintln!(
        "  {} fixups <file>            Find aliased geometry and pages over 100 MB the game would reject",
        program
    );
    eprintln!(
        "  {} fixups --batch <dir>     ... across a directory tree",
        program
    );
    eprintln!(
        "  {} fixups --game <install>  ... across a whole mounted install",
        program
    );
    eprintln!();
    eprintln!("Supported formats:");
    eprintln!("  .ypt  Particle Template");
    eprintln!("  .ytd  Texture Dictionary");
    eprintln!("  .ydr  Drawable (3D model)");
    eprintln!("  .ydd  Drawable Dictionary (several drawables per file)");
    eprintln!("  .yft  Fragment (drawable + skeleton + physics)");
    eprintln!("  .ybn  Physics Bound (collision)");
    eprintln!("  .yld  Cloth Dictionary (ped garment cloth)");
    eprintln!("  .ycd  Clip Dictionary (animation clips)");
    eprintln!("  .fxc  GTA V Compiled Shader Container");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} stats tests/fixtures/", program);
    eprintln!("  {} textures particles.ypt", program);
    eprintln!("  {} drawables model.ydr", program);
    eprintln!("  {} drawables clothing.ydd", program);
    eprintln!("  {} drawables vehicle.yft", program);
    eprintln!("  {} skeleton ped.yft", program);
    eprintln!("  {} bounds collision.ybn", program);
    eprintln!("  {} cloth clothing.yld", program);
    eprintln!("  {} anim move.ycd ped.yft --clip walk --at 0.5", program);
    eprintln!("  {} pointers particles.ypt", program);
    eprintln!("  {} diff original.ytd converted.ytd", program);
    eprintln!("  {} layout clothing.ydd", program);
    eprintln!("  {} fxc default.fxc", program);
    eprintln!("  {} fixups --batch ./corpus --json", program);
}

pub fn run(args: &[String]) -> ExitCode {
    let program = "openrage-cli inspect";

    if args.is_empty() {
        print_usage(program);
        return ExitCode::from(2);
    }

    let result = match args[0].as_str() {
        "stats" => {
            if args.len() < 2 {
                eprintln!("Error: Missing directory argument");
                eprintln!("Usage: {} stats <directory>", program);
                return ExitCode::from(2);
            }
            cmd_stats(Path::new(&args[1]))
        }
        "textures" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} textures <file>", program);
                return ExitCode::from(2);
            }
            cmd_textures(Path::new(&args[1]))
        }
        "drawables" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} drawables <file>", program);
                return ExitCode::from(2);
            }
            cmd_drawables(Path::new(&args[1]))
        }
        "bounds" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} bounds <file>", program);
                return ExitCode::from(2);
            }
            cmd_bounds(Path::new(&args[1]))
        }
        "skeleton" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} skeleton <file.yft|.ydr>", program);
                return ExitCode::from(2);
            }
            cmd_skeleton(Path::new(&args[1]))
        }
        "cloth" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} cloth <file.yld>", program);
                return ExitCode::from(2);
            }
            cmd_cloth(Path::new(&args[1]))
        }
        "clips" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} clips <file.ycd>", program);
                return ExitCode::from(2);
            }
            cmd_clips(Path::new(&args[1]))
        }
        "anim" => {
            if args.len() < 3 {
                eprintln!("Error: Missing arguments");
                eprintln!(
                    "Usage: {} anim <file.ycd> <skeleton.yft|.ydr> [--clip <name>] [--at <seconds>]",
                    program
                );
                return ExitCode::from(2);
            }
            cmd_anim(&args[1..])
        }
        "pointers" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} pointers <file>", program);
                return ExitCode::from(2);
            }
            cmd_pointers(Path::new(&args[1]))
        }
        "diff" => {
            if args.len() < 3 {
                eprintln!("Error: Missing file arguments");
                eprintln!("Usage: {} diff <original> <converted>", program);
                return ExitCode::from(2);
            }
            cmd_diff(Path::new(&args[1]), Path::new(&args[2]))
        }
        "layout" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} layout <file>", program);
                return ExitCode::from(2);
            }
            cmd_layout(Path::new(&args[1]))
        }
        "fxc" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} fxc <file>", program);
                return ExitCode::from(2);
            }
            cmd_fxc(Path::new(&args[1]))
        }
        "fixups" => {
            return match super::fixups::run(&args[1..]) {
                Ok(code) => code,
                Err(e) => {
                    error!("{}", e);
                    eprintln!("Error: {}", e);
                    ExitCode::from(1)
                }
            };
        }
        "-h" | "--help" | "help" => {
            print_usage(program);
            return ExitCode::SUCCESS;
        }
        cmd => {
            eprintln!("Error: Unknown command '{}'", cmd);
            eprintln!();
            print_usage(program);
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{}", e);
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}
