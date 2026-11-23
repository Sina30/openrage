//! Census the layouts nothing in the corpus exercises, over a whole install.
//!
//! The subjects are a drawable's `structTintShaderInfo` rows, a fragment's
//! `fragGlassWindow` panes, an environment cloth's `clothInstanceTuning`, and
//! the bound tags, `phBoundPlane` among them.
//! `whether_a_fragment_carries_glass.rs`,
//! `what_a_fragment_reaches_that_does_not_bind.rs` and the pack repository's
//! `what_shape_a_tint_row_is.rs` ask the same of a staged corpus; a type
//! absent from every shipped file is unfixturable rather than unfixtured.
//!
//! ⚠ **Every count here is paired with a reach count**: the bound census read
//! `Disc` as never occurring when its walk never entered the physics tree. A
//! subject whose reach is zero is reported as unmeasured rather than as absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::decl::{Gen8, Gen9, OpenEnum, Raw, View};
use or_io::desc::Image;
use or_io::gta5::drawable::DrawableTarget;
use or_io::gta5::{BoundType, File, GtaDrawable, GtaFragType, PgDictionary, PhBoundAny};
use or_io::Generation;

use super::keys;

/// Every bound tag the type enum names, so the report has a full denominator
/// rather than only the tags somebody expected to see.
const EVERY_TAG: [BoundType; 14] = [
    BoundType::Sphere,
    BoundType::Capsule,
    BoundType::TaperedCapsule,
    BoundType::Box,
    BoundType::Geometry,
    BoundType::GeometryCurved,
    BoundType::Grid,
    BoundType::Ribbon,
    BoundType::Bvh,
    BoundType::Surface,
    BoundType::Composite,
    BoundType::Disc,
    BoundType::Cylinder,
    BoundType::Plane,
];

#[derive(Default)]
struct Census {
    /// Where to write a copy of each file that carries one of the subjects.
    stage: Option<PathBuf>,
    staged: Vec<String>,

    files_seen: u64,
    files_read: u64,
    files_refused: u64,

    // Tint: `GtaDrawable::tint_data`, over `.ydr` and `.ydd` entries.
    drawables_examined: u64,
    drawables_with_a_shader_group: u64,
    drawables_with_a_tint_pointer: u64,
    tint_carriers: Vec<String>,

    // Glass: `FragType::glass_pane_model_infos`, and the groups that claim to be glass.
    fragments_read: u64,
    fragment_drawables: u64,
    fragments_with_physics_groups: u64,
    physics_groups_reached: u64,
    glass_panes: u64,
    glass_carrier_files: u64,
    /// ⚠ One pane in a file says what the block holds and nothing about where
    /// it ends; only two adjacent panes give `fragGlassWindow`'s stride. The
    /// declaration said 0x70, and the bytes past 0x40 differ between two
    /// editions of the same model.
    glass_most_panes_in_one_file: u64,
    groups_marked_glass: u64,
    glass_carriers: Vec<String>,

    // Cloth: `EnvironmentCloth::tuning`, read as an address out of the file's
    // own pointer rather than through the owned value.
    cloth_slots: u64,
    cloth_drawable_leads: u64,
    cloth_controller_leads: u64,
    tuning_non_null: u64,
    tuning_leads_somewhere: u64,
    tuning_carriers: Vec<String>,

    // Bounds, by tag, over every bound any walked file reaches.
    bound_tags: BTreeMap<u8, u64>,
    bounds_reached: u64,
    plane_carriers: Vec<String>,
}

impl Census {
    fn note_carrier(list: &mut Vec<String>, line: String) {
        // The first twenty are the evidence; past that the count is the point.
        if list.len() < 20 {
            list.push(line);
        }
    }

    /// Write a carrier out under its leaf name, so the corpus layout's
    /// same-filename-per-target rule holds without renaming anything.
    ///
    /// ⚠ **The cap is per subject**: one global cap filled with the first
    /// eight glass fragments and staged no cloth at all.
    fn stage(&mut self, kind: &str, logical: &str, data: &[u8]) {
        let Some(dir) = self.stage.clone() else {
            return;
        };
        // A fixture is an example, not a copy of the game.
        const ENOUGH_PER_SUBJECT: usize = 2;
        let leaf = logical.rsplit('/').next().unwrap_or(logical);
        let already = self.staged.iter().filter(|s| s.starts_with(kind)).count();
        if already >= ENOUGH_PER_SUBJECT || self.staged.iter().any(|s| s.ends_with(leaf)) {
            return;
        }
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        match std::fs::write(dir.join(leaf), data) {
            Ok(()) => self.staged.push(format!("{kind} {leaf}")),
            Err(e) => eprintln!("could not stage {leaf}: {e}"),
        }
    }

    /// ⚠ `tint` is the pointer the file states: the owned drawable keeps it
    /// hidden, and a live one is the thing this census looks for.
    fn drawable(&mut self, source: &str, tint: u64, shaders: usize) {
        self.drawables_examined += 1;
        if shaders > 0 {
            self.drawables_with_a_shader_group += 1;
        }
        if tint != 0 {
            self.drawables_with_a_tint_pointer += 1;
            Self::note_carrier(
                &mut self.tint_carriers,
                format!("{source}: tint at {tint:#x}, {shaders} shader(s)"),
            );
        }
    }

    fn bound(&mut self, source: &str, bound: &PhBoundAny) {
        self.bounds_reached += 1;
        let tag = crate::inspect::bound_header(bound).r#type;
        *self.bound_tags.entry(tag.raw()).or_default() += 1;
        if tag.raw() == BoundType::Plane.to_raw() {
            Self::note_carrier(&mut self.plane_carriers, source.to_string());
        }
        if let PhBoundAny::PhBoundComposite(c) = bound {
            for child in c.bounds.iter().flatten() {
                self.bound(source, child);
            }
        }
    }

    fn count(&self, tag: BoundType) -> u64 {
        self.bound_tags.get(&tag.to_raw()).copied().unwrap_or(0)
    }

    /// The tags this install never ships.
    fn never_seen(&self) -> Vec<String> {
        EVERY_TAG
            .into_iter()
            .filter(|t| self.count(*t) == 0)
            .map(|t| format!("{t:?}"))
            .collect()
    }
}

/// A file's pages, for the words the owned value keeps hidden.
fn image_of(data: &[u8]) -> Option<(or_io::decl::Aligned, usize)> {
    crate::pages(data).ok()
}

/// ⚠ A fragment's drawables are **not** counted in the tint denominator:
/// `FragDrawable` has no tint slot, and drawables that cannot carry a
/// payload would make the zero look better measured than it is.
fn read_drawables_of_fragment<T: DrawableTarget>(
    census: &mut Census,
    source: &str,
    frag: &GtaFragType<T>,
) {
    for drawable in frag.drawables() {
        census.fragment_drawables += 1;
        if let Some(b) = drawable.bound.as_deref() {
            census.bound(source, b);
        }
        for b in drawable.extra_bounds.iter().flatten() {
            census.bound(source, b);
        }
    }
    if let Some(group) = frag.physics_lod_group.as_deref() {
        for lod in group.lods() {
            if let Some(b) = lod.composite_bounds.as_deref() {
                census.bound(source, b);
            }
            for archetype in [&lod.phys_damp_undamaged, &lod.phys_damp_damaged]
                .into_iter()
                .flatten()
            {
                if let Some(b) = archetype.bound.as_deref() {
                    census.bound(source, b);
                }
            }
        }
    }
}

/// The glass and cloth halves, which both need the file's own bytes as well as
/// the owned value.
fn census_fragment<T: DrawableTarget>(census: &mut Census, source: &str, data: &[u8])
where
    GtaFragType<T>: File,
{
    const GLASS: u8 = 2;

    let Ok(frag) = GtaFragType::<T>::from_bytes(data) else {
        census.files_refused += 1;
        return;
    };
    census.files_read += 1;
    census.fragments_read += 1;

    let panes = frag.glass_pane_model_infos.len() as u64;
    let groups = || {
        (frag.physics_lod_group.as_deref())
            .into_iter()
            .flat_map(|group| group.lods())
            .flat_map(|lod| lod.groups.iter())
    };
    let marked = groups().filter(|g| g.flags & GLASS != 0).count() as u64;
    let group_count = groups().count() as u64;

    if group_count > 0 {
        census.fragments_with_physics_groups += 1;
        census.physics_groups_reached += group_count;
    }
    census.glass_panes += panes;
    census.groups_marked_glass += marked;
    if panes > 0 {
        census.glass_carrier_files += 1;
        census.glass_most_panes_in_one_file = census.glass_most_panes_in_one_file.max(panes);
    }
    if panes > 0 || marked > 0 {
        Census::note_carrier(
            &mut census.glass_carriers,
            format!("{source}: {panes} pane(s), {marked} group(s) marked glass"),
        );
        census.stage("glass", source, data);
        // ⚠ **A separate slot, because one pane cannot give a stride.** 246 of
        // the 433 panes' carriers hold exactly one. A multi-pane carrier is the only fixture that settles a pane's size,
        // and the general cap would never reach one.
        if panes > 1 {
            census.stage("glassmulti", source, data);
        }
    }

    read_drawables_of_fragment(census, source, &frag);
    let tuning_before = census.tuning_leads_somewhere;
    if !frag.env_cloth.is_empty() {
        if let Some((pages, virtual_size)) = image_of(data) {
            census_cloth_tuning::<T>(census, source, Image::new(pages.bytes(), virtual_size));
        }
    }
    if census.tuning_leads_somewhere > tuning_before {
        census.stage("cloth", source, data);
    }
}

/// `EnvironmentCloth::tuning`, read out of the file's own pointer.
///
/// ⚠ **The discriminator is not `!= 0`.** A captured runtime address is
/// non-zero and means nothing; what says a slot is live is that it *resolves
/// and leads somewhere*. The drawable and controller pointers beside it are
/// the controls — without them a zero here is indistinguishable from
/// arithmetic that misses the struct.
fn census_cloth_tuning<T: DrawableTarget>(census: &mut Census, source: &str, image: Image<'_>) {
    let Ok(frag) = View::<Raw<GtaFragType<T>>>::at_offset(image, 0) else {
        return;
    };
    let Ok(cloths) = frag.base().env_cloth() else {
        return;
    };

    let leads = |addr: u64| -> bool {
        addr != 0
            && image
                .follow(addr)
                .ok()
                .and_then(|target| image.slice_at(target.base(), 16).ok())
                .is_some_and(|b| b.iter().any(|x| *x != 0))
    };

    for i in 0..cloths.len() {
        let Ok(Some(cloth)) = cloths.follow(i) else {
            continue;
        };
        census.cloth_slots += 1;
        if leads({ cloth.referenced_drawable }.raw()) {
            census.cloth_drawable_leads += 1;
        }
        if leads({ cloth.cloth_controller }.raw()) {
            census.cloth_controller_leads += 1;
        }
        let tuning = { cloth.tuning }.raw();
        if tuning != 0 {
            census.tuning_non_null += 1;
        }
        if leads(tuning) {
            census.tuning_leads_somewhere += 1;
            Census::note_carrier(
                &mut census.tuning_carriers,
                format!("{source} cloth {i}: m_Tuning at {tuning:#x}"),
            );
        }
    }
}

/// Each drawable's tint pointer, as the file states it, in the order the owned value holds them.
fn tint_pointers<T: DrawableTarget>(ext: &str, data: &[u8]) -> Vec<u64> {
    let Some((pages, virtual_size)) = image_of(data) else {
        return Vec::new();
    };
    let image = Image::new(pages.bytes(), virtual_size);
    let read = || -> Result<Vec<u64>, or_io::decl::ViewError> {
        Ok(match ext {
            "ydr" => vec![{ View::<Raw<GtaDrawable<T>>>::at_offset(image, 0)?.tint_data }.raw()],
            _ => {
                let dict = View::<Raw<PgDictionary<GtaDrawable<T>>>>::at_offset(image, 0)?;
                let entries = dict.entries()?;
                let mut out = Vec::new();
                for i in 0..entries.len() {
                    out.extend(entries.follow(i)?.map(|d| { d.tint_data }.raw()));
                }
                out
            }
        })
    };
    read().unwrap_or_default()
}

fn census_at<T: DrawableTarget>(census: &mut Census, logical: &str, ext: &str, data: &[u8])
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    GtaFragType<T>: File,
{
    let shaders = |d: &GtaDrawable<T>| d.shader_group.as_deref().map_or(0, |g| g.shaders.len());
    match ext {
        "ydr" => match GtaDrawable::<T>::from_bytes(data) {
            Ok(d) => {
                census.files_read += 1;
                let tint = tint_pointers::<T>(ext, data).first().copied().unwrap_or(0);
                census.drawable(logical, tint, shaders(&d));
                if let Some(b) = d.bound.as_deref() {
                    census.bound(logical, b);
                }
            }
            Err(_) => census.files_refused += 1,
        },
        "ydd" => match PgDictionary::<GtaDrawable<T>>::from_bytes(data) {
            Ok(dict) => {
                census.files_read += 1;
                let tints = tint_pointers::<T>(ext, data);
                for (i, entry) in dict.entries.iter().flatten().enumerate() {
                    let tint = tints.get(i).copied().unwrap_or(0);
                    census.drawable(&format!("{logical}[{i}]"), tint, shaders(entry));
                    if let Some(b) = entry.bound.as_deref() {
                        census.bound(logical, b);
                    }
                }
            }
            Err(_) => census.files_refused += 1,
        },
        _ => census_fragment::<T>(census, logical, data),
    }
}

fn census_file(census: &mut Census, logical: &str, data: &[u8]) {
    let path = PathBuf::from(logical);
    let Some(ext) = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return;
    };
    census.files_seen += 1;

    match ext.as_str() {
        "ybn" => match PhBoundAny::from_bytes(data) {
            Ok(bound) => {
                census.files_read += 1;
                census.bound(logical, &bound);
            }
            Err(_) => census.files_refused += 1,
        },
        "ydr" | "ydd" | "yft" => match crate::target_of(&path, data) {
            Generation::Gen9 => census_at::<Gen9>(census, logical, &ext, data),
            _ => census_at::<Gen8>(census, logical, &ext, data),
        },
        _ => {
            census.files_seen -= 1;
        }
    }
}

fn print_report(census: &Census, label: &str) {
    println!();
    println!("=== what nothing in the corpus exercises, over {label} ===");
    println!(
        "  {} file(s) of the four kinds; {} read, {} refused",
        census.files_seen, census.files_read, census.files_refused
    );

    println!();
    println!("structTintShaderInfo  (GtaDrawable::tint_data)");
    println!(
        "  {} drawable(s) examined, {} with a shader group, {} with a tint pointer",
        census.drawables_examined,
        census.drawables_with_a_shader_group,
        census.drawables_with_a_tint_pointer
    );
    for line in &census.tint_carriers {
        println!("    {line}");
    }

    println!();
    println!("fragGlassWindow  (FragType::glass_pane_model_infos)");
    println!(
        "  {} fragment(s) read, {} fragment drawable(s); {} with physics groups, {} group(s) reached",
        census.fragments_read,
        census.fragment_drawables,
        census.fragments_with_physics_groups,
        census.physics_groups_reached
    );
    println!(
        "  {} glass pane(s) across {} carrier(s); most in one file {}; \
         {} group(s) marked glass",
        census.glass_panes,
        census.glass_carrier_files,
        census.glass_most_panes_in_one_file,
        census.groups_marked_glass
    );
    for line in &census.glass_carriers {
        println!("    {line}");
    }

    println!();
    println!("clothInstanceTuning  (EnvironmentCloth::tuning)");
    println!(
        "  {} environment-cloth slot(s); controls lead somewhere in {} (drawable) and {} (controller)",
        census.cloth_slots, census.cloth_drawable_leads, census.cloth_controller_leads
    );
    println!(
        "  m_Tuning: {} non-null, {} lead somewhere",
        census.tuning_non_null, census.tuning_leads_somewhere
    );
    for line in &census.tuning_carriers {
        println!("    {line}");
    }

    println!();
    println!("phBound tags  (every bound any walked file reaches)");
    println!("  {} bound(s) reached", census.bounds_reached);
    for tag in EVERY_TAG {
        println!("    {:<16} {}", format!("{tag:?}"), census.count(tag));
    }
    let never = census.never_seen();
    println!("  never seen: {}", never.join(", "));
    for line in &census.plane_carriers {
        println!("    plane in {line}");
    }
    if let Some(dir) = &census.stage {
        println!();
        println!(
            "staged {} carrier(s) to {}",
            census.staged.len(),
            dir.display()
        );
        for leaf in &census.staged {
            println!("    {leaf}");
        }
    }
    println!();
}

fn scan_install(
    census: &mut Census,
    game_root: &Path,
    keys_path: Option<&Path>,
    limit: Option<usize>,
    only: Option<&str>,
    verbose: bool,
) -> Result<String, String> {
    let game = keys::mount_all(game_root, keys_path)?;
    let label = format!(
        "{} ({})",
        game_root.display(),
        game.game.generation().edition_label()
    );

    let mut targets: Vec<String> = Vec::new();
    game.for_each_file(|logical, _container, leaf| {
        let lower = leaf.to_ascii_lowercase();
        if lower.ends_with(".ydr")
            || lower.ends_with(".ydd")
            || lower.ends_with(".yft")
            || lower.ends_with(".ybn")
        {
            targets.push(logical.to_string());
        }
    });
    targets.sort();
    targets.dedup();
    if let Some(only) = only {
        targets.retain(|t| t.to_ascii_lowercase().contains(&only.to_ascii_lowercase()));
    }
    if let Some(limit) = limit {
        targets.truncate(limit);
    }

    eprintln!("scanning {} file(s) from {label}...", targets.len());

    for (i, logical) in targets.iter().enumerate() {
        if verbose && i > 0 && i % 2000 == 0 {
            eprintln!(
                "  {i}/{}  ({} read, {} refused)",
                targets.len(),
                census.files_read,
                census.files_refused
            );
        }
        let Some(data) = game.vfs.open(logical) else {
            continue;
        };
        census_file(census, logical, &data);
    }

    Ok(label)
}

pub fn run(args: &[String]) -> ExitCode {
    let mut game: Option<PathBuf> = None;
    let mut keys_path: Option<PathBuf> = None;
    let mut limit: Option<usize> = None;
    let mut stage: Option<PathBuf> = None;
    let mut only: Option<String> = None;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game" => {
                i += 1;
                game = args.get(i).map(PathBuf::from);
            }
            "--keys" => {
                i += 1;
                keys_path = args.get(i).map(PathBuf::from);
            }
            "--limit" => {
                i += 1;
                limit = args.get(i).and_then(|v| v.parse().ok());
            }
            "--stage" => {
                i += 1;
                stage = args.get(i).map(PathBuf::from);
            }
            "--only" => {
                i += 1;
                only = args.get(i).cloned();
            }
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("Error: unexpected argument '{other}'");
                print_usage();
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let Some(game) = game else {
        print_usage();
        return ExitCode::from(2);
    };

    let mut census = Census {
        stage,
        ..Default::default()
    };
    match scan_install(
        &mut census,
        &game,
        keys_path.as_deref(),
        limit,
        only.as_deref(),
        verbose,
    ) {
        Ok(label) => {
            print_report(&census, &label);
            // A walk that read nothing reports the same zeros as a game that
            // ships none of these.
            if census.files_read == 0 {
                eprintln!("Error: nothing was read, so every zero above is about the walk");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("Census the layouts nothing in the corpus exercises");
    eprintln!();
    eprintln!("Usage: openrage-cli gaps --game <install-root> [--keys <file>] [--limit N] [-v]");
    eprintln!("Options:");
    eprintln!("  --game <path>   Mount a GTA V install and walk it through the VFS");
    eprintln!("  --keys <path>   Key cache for encrypted archives (default: auto)");
    eprintln!("  --limit <n>     Stop after n files, for a smoke run");
    eprintln!("  --stage <dir>   Copy each carrier found into <dir>, under its leaf name");
    eprintln!("  --only <substr> Walk only paths containing <substr>, to stage a named carrier");
    eprintln!("  -v, --verbose   Report progress");
    eprintln!();
    eprintln!("Reports tint payloads, glass panes, cloth tuning and bound tags, each");
    eprintln!("beside the reach of the walk that looked for it.");
}
