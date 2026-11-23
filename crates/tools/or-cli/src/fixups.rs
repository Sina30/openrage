use or_interchange::doctor::levels::copies;
use or_interchange::doctor::palette::restore_palettes;
use or_interchange::doctor::place::{place_drawable, Frame};
use or_interchange::extract::Extracted;
use or_interchange::import::Imported;
use or_io::container::resource::{
    DatResourceFileHeader, DatResourceSizes, ResourceEnvelope, ResourceType, Rsc7, GAME_PAGE_LIMIT,
    RSC_PHYSICAL_LEAF_SIZE, RSC_VIRTUAL_LEAF_SIZE,
};
use or_io::decl::{Aligned, Ptr, Raw, Record, Repair, Run, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::drawable::{aliased_lods, Aliased, Buffers, DrawableTarget, LodSlot, RmcDrawable};
use or_io::gta5::{
    drawable, drawable_dict, fragment, particle, File, GtaDrawable, GtaFragType, PgDictionary,
    PtxFxList,
};
use or_io::Generation;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::keys;

pub const DRAWABLE_BEARING: [ResourceType; 4] = [
    ResourceType::Ydr,
    ResourceType::Ydd,
    ResourceType::Yft,
    ResourceType::Ypt,
];

pub fn is_drawable_bearing(leaf: &str) -> bool {
    let Some(extension) = leaf.rsplit('.').next() else {
        return false;
    };
    ResourceType::from_extension(extension)
        .is_some_and(|resource| DRAWABLE_BEARING.contains(&resource))
}

/// A heap that is one page larger than the game takes: it refuses the resource with `ERR_STR_FAILURE_3` and the client crashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OversizedPage {
    pub heap: &'static str,
    pub bytes: u32,
}

impl std::fmt::Display for OversizedPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a {} page of {:.1} MiB: the game refuses a page over {} MB (ERR_STR_FAILURE_3)",
            self.heap,
            f64::from(self.bytes) / f64::from(1u32 << 20),
            GAME_PAGE_LIMIT / 1_000_000,
        )
    }
}

/// Every buffer more than one geometry of a file points at, and every heap that is one page the game refuses.
#[derive(Debug, Clone, Default)]
pub struct FixupReport {
    pub aliased_blocks: Vec<Aliased>,
    pub oversized_pages: Vec<OversizedPage>,
}

impl FixupReport {
    pub fn is_clean(&self) -> bool {
        self.aliased_blocks.is_empty() && self.oversized_pages.is_empty()
    }

    pub fn describe(&self) -> String {
        let aliased = self.aliased_blocks.iter().map(|block| format!("{block}\n"));
        let pages = self.oversized_pages.iter().map(|page| format!("{page}\n"));
        aliased.chain(pages).collect()
    }

    /// One line for a list: what is wrong, in few words.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.aliased_blocks.is_empty() {
            parts.push(format!("{} aliased block(s)", self.aliased_blocks.len()));
        }
        if !self.oversized_pages.is_empty() {
            parts.push(format!("{} oversized page(s)", self.oversized_pages.len()));
        }
        if parts.is_empty() {
            "clean".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// The heaps of a file whose one page is more than the game takes.
fn oversized_pages(data: &[u8]) -> Vec<OversizedPage> {
    let Ok(header) = DatResourceFileHeader::parse(data) else {
        return Vec::new();
    };
    [
        ("virtual", header.info.r#virtual, RSC_VIRTUAL_LEAF_SIZE),
        ("physical", header.info.physical, RSC_PHYSICAL_LEAF_SIZE),
    ]
    .into_iter()
    .filter_map(|(heap, flags, leaf)| {
        let bytes = DatResourceSizes::from_flags(flags).largest_page(leaf);
        (bytes > GAME_PAGE_LIMIT).then_some(OversizedPage { heap, bytes })
    })
    .collect()
}

/// A level that draws this share of the high level's vertices is taken for a copy of it.
pub const COPY_SHARE: f32 = 0.9;

/// The levels a repair removed, by the drawable's place in its file.
#[derive(Debug, Clone, Default)]
pub struct RepairOutcome {
    pub removed_lods: Vec<(usize, LodSlot)>,
    /// The counts rewritten from their runs, when asked (the sentences of the repair).
    pub counts_fixed: Vec<String>,
    /// The palettes given to skinned geometries that had none, when asked.
    pub palettes_restored: Vec<String>,
    /// The skinned geometries that have none and whose bindings name no bone to give them.
    pub palettes_left: Vec<String>,
    /// The drawables carried into another frame, when asked.
    pub placed: Vec<String>,
}

impl RepairOutcome {
    pub fn describe(&self) -> String {
        let counts = match self.counts_fixed.len() {
            0 => String::new(),
            n => format!("; {n} count(s) rewritten from their runs"),
        };
        let counts = match self.palettes_restored.len() {
            0 => counts,
            n => format!("{counts}; {n} palette(s) restored"),
        };
        let counts = match self.placed.len() {
            0 => counts,
            n => format!("{counts}; {n} drawable(s) carried into another frame"),
        };
        let counts = match self.palettes_left.len() {
            0 => counts,
            n => format!("{counts}; {n} skinned geometr(ies) left without a palette (their bindings name no bone of the ped)"),
        };
        if self.removed_lods.is_empty() {
            return format!("no levels removed{counts}");
        }
        let removed: Vec<String> = (self.removed_lods.iter())
            .map(|(drawable, lod)| format!("drawable {drawable} {lod}"))
            .collect();
        format!("removed {}{counts}", removed.join(", "))
    }
}

/// A file's decompressed bytes, and its format and target.
struct Opened {
    bytes: Aligned,
    virtual_size: usize,
    resource: ResourceType,
    target: Generation,
}

impl Opened {
    fn image(&self) -> Image<'_> {
        Image::new(self.bytes.bytes(), self.virtual_size)
    }
}

fn open(path: &Path, data: &[u8]) -> Result<Opened, String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let resource = ResourceType::from_extension(extension)
        .filter(|r| DRAWABLE_BEARING.contains(r))
        .ok_or_else(|| format!("no drawable scanner for '.{extension}'"))?;
    let target = crate::target_of_resource(resource, data);
    let opened = Rsc7::open(data).map_err(|e| e.to_string())?;
    Ok(Opened {
        bytes: Aligned::from_bytes(&opened.data),
        virtual_size: opened.virtual_size as usize,
        resource,
        target,
    })
}

pub fn scan_bytes(path: &Path, data: &[u8]) -> Result<FixupReport, String> {
    let opened = open(path, data)?;
    let (image, target) = (opened.image(), opened.target);
    let buffers: Buffers = match opened.resource {
        ResourceType::Ydr => drawable::buffers(image, 0, target).map_err(|e| e.to_string()),
        ResourceType::Ydd => drawable_dict::buffers(image, 0, target).map_err(|e| e.to_string()),
        ResourceType::Yft => fragment::buffers(image, 0, target).map_err(|e| e.to_string()),
        _ => particle::buffers(image, 0, target).map_err(|e| e.to_string()),
    }?;
    Ok(FixupReport {
        aliased_blocks: buffers.aliased(),
        oversized_pages: oversized_pages(data),
    })
}

/// What each pointer of a run points at, skipping a null one.
fn present<R: Record>(run: Run<'_, Ptr<R>>) -> Result<Vec<View<'_, R>>, ViewError> {
    let mut out = Vec::new();
    for i in 0..run.len() {
        out.extend(run.follow(i)?);
    }
    Ok(out)
}

/// Each drawable a file holds, in the order its validation visits them.
fn drawables<T: DrawableTarget>(
    resource: ResourceType,
    image: Image<'_>,
) -> Result<Vec<View<'_, Raw<RmcDrawable<T>>>>, String> {
    let found = match resource {
        ResourceType::Ydr => {
            View::<Raw<GtaDrawable<T>>>::at_offset(image, 0).map(|d| vec![d.base()])
        }
        ResourceType::Ydd => View::<Raw<PgDictionary<GtaDrawable<T>>>>::at_offset(image, 0)
            .and_then(|d| present(d.entries()?))
            .map(|entries| entries.iter().map(|d| d.base()).collect()),
        ResourceType::Yft => fragment::drawables::<T>(image, 0),
        _ => View::<Raw<PtxFxList<T>>>::at_offset(image, 0).and_then(|list| {
            match list.model_dict()? {
                Some(dict) => present(dict.entries()?),
                None => Ok(Vec::new()),
            }
        }),
    };
    found.map_err(|e| e.to_string())
}

/// Each drawable a file holds, to edit, in the order [`drawables`] reads them.
fn drawables_mut<T: DrawableTarget>(file: &mut Owned<T>) -> Vec<&mut RmcDrawable<T>> {
    match file {
        Owned::Ydr(d) => vec![&mut d.base],
        Owned::Ydd(dict) => dict
            .entries
            .iter_mut()
            .flatten()
            .map(|d| &mut d.base)
            .collect(),
        Owned::Yft(frag) => frag.base.drawables_mut(),
        Owned::Ypt(list) => match list.model_dict.as_deref_mut() {
            Some(dict) => dict
                .entries
                .iter_mut()
                .flatten()
                .map(|d| &mut **d)
                .collect(),
            None => Vec::new(),
        },
    }
}

/// A drawable-bearing file, opened owned at one target.
enum Owned<T: DrawableTarget> {
    Ydr(Box<GtaDrawable<T>>),
    Ydd(Box<PgDictionary<GtaDrawable<T>>>),
    Yft(Box<GtaFragType<T>>),
    Ypt(Box<PtxFxList<T>>),
}

/// ⚠ A level is removed only where every buffer it draws a lower level owns,
/// so no geometry is lost; a save that still aliases is refused, as any save
/// through `or-io` is. With `drop_copies` a level that draws [`COPY_SHARE`] of the high level's vertices goes too: it is
/// the high level again, and the heap it adds can be what makes a page the game refuses.
fn repair<T: DrawableTarget + Extracted + Imported>(
    opened: &Opened,
    data: &[u8],
    drop_copies: bool,
    fix_counts: bool,
    fix_palettes: bool,
    place: Option<Frame>,
) -> Result<(RepairOutcome, Vec<u8>), String>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    GtaFragType<T>: File,
    PtxFxList<T>: File,
{
    let mut plans = Vec::new();
    for view in drawables::<T>(opened.resource, opened.image())?.iter() {
        plans.push(aliased_lods(view).map_err(|e| e.to_string())?);
    }
    let e = |e: or_io::Error| e.to_string();
    let mut file = match opened.resource {
        ResourceType::Ydr => Owned::Ydr(Box::new(GtaDrawable::<T>::from_bytes(data).map_err(e)?)),
        ResourceType::Ydd => Owned::Ydd(Box::new(
            PgDictionary::<GtaDrawable<T>>::from_bytes(data).map_err(e)?,
        )),
        ResourceType::Yft => Owned::Yft(Box::new(GtaFragType::<T>::from_bytes(data).map_err(e)?)),
        _ => Owned::Ypt(Box::new(PtxFxList::<T>::from_bytes(data).map_err(e)?)),
    };
    let mut removed_lods = Vec::new();
    let mut palettes_restored = Vec::new();
    let mut palettes_left = Vec::new();
    let mut placed = Vec::new();
    for (index, (drawable, lods)) in drawables_mut(&mut file).into_iter().zip(plans).enumerate() {
        let mut lods = lods;
        if drop_copies {
            for level in copies(drawable, COPY_SHARE) {
                let slot = LodSlot::Level(level);
                if !lods.contains(&slot) {
                    lods.push(slot);
                }
            }
        }
        removed_lods.extend(lods.iter().map(|lod| (index, *lod)));
        drawable.remove_lods(&lods);
        if fix_palettes {
            let report = restore_palettes(drawable);
            palettes_restored.extend(
                (report.restored.into_iter()).map(|said| format!("drawable {index} {said}")),
            );
            palettes_left
                .extend((report.left.into_iter()).map(|said| format!("drawable {index} {said}")));
        }
        if let Some(frame) = &place {
            let (made, report) = place_drawable(&*drawable, frame).map_err(|e| e.to_string())?;
            *drawable = made;
            placed.push(format!(
                "drawable {index}: {} vertices carried, box {:?} to {:?}",
                report.vertices, report.min, report.max
            ));
        }
    }
    // ⚠ A count is rewritten from its run only when asked, and only where that loses nothing (`Repair::Lossless`): without it a model whose counts contradict
    // their runs is refused, as any save through `or-io` is.
    let (bytes, counts_fixed) = if fix_counts {
        match &file {
            Owned::Ydr(d) => d.to_bytes_repairing(Repair::Lossless),
            Owned::Ydd(d) => d.to_bytes_repairing(Repair::Lossless),
            Owned::Yft(d) => d.to_bytes_repairing(Repair::Lossless),
            Owned::Ypt(d) => d.to_bytes_repairing(Repair::Lossless),
        }
        .map_err(e)?
    } else {
        let bytes = match &file {
            Owned::Ydr(d) => d.to_bytes(),
            Owned::Ydd(d) => d.to_bytes(),
            Owned::Yft(d) => d.to_bytes(),
            Owned::Ypt(d) => d.to_bytes(),
        }
        .map_err(e)?;
        (bytes, Vec::new())
    };
    Ok((
        RepairOutcome {
            removed_lods,
            counts_fixed,
            palettes_restored,
            palettes_left,
            placed,
        },
        bytes,
    ))
}

/// A Legacy ped component `.ydd` made into a prop `.ydd`: carried into `frame`, laid out and shaded as the stock props `opaque` and `alpha` are (`doctor::prop`).
pub fn as_prop_bytes(
    data: &[u8],
    opaque: &[u8],
    alpha: &[u8],
    frame: Frame,
) -> Result<(String, Vec<u8>), String> {
    use or_io::decl::Gen8;
    let e = |e: or_io::Error| e.to_string();
    let mut file = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(data).map_err(e)?;
    let stock = |bytes: &[u8], which: &str| {
        let dict = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).map_err(e)?;
        let first = dict.entries.iter().flatten().next().map(|d| d.base.clone());
        first.ok_or_else(|| format!("the {which} stock prop holds no drawable"))
    };
    let (opaque, alpha) = (stock(opaque, "opaque")?, stock(alpha, "alpha")?);
    let mut said = Vec::new();
    for (i, entry) in file.entries.iter_mut().flatten().enumerate() {
        let (made, report) =
            or_interchange::doctor::prop::as_prop(&entry.base, &opaque, &alpha, &frame)
                .map_err(|e| e.to_string())?;
        entry.base = made;
        said.push(format!(
            "drawable {i}: {} vertices, shaders {:?}, box {:?} to {:?}{}",
            report.vertices,
            report.shaders,
            report.min,
            report.max,
            if report.filled.is_empty() {
                String::new()
            } else {
                format!(", filled {}", report.filled.join("; "))
            }
        ));
    }
    // ⚠ The stride a geometry states is the component's: a count is rewritten from its run, as `repair --fix-counts` does.
    let (bytes, _) = file.to_bytes_repairing(Repair::Lossless).map_err(e)?;
    Ok((said.join("\n"), with_page_map(bytes)?))
}

pub fn repair_bytes(
    path: &Path,
    data: &[u8],
    drop_copies: bool,
    fix_counts: bool,
    fix_palettes: bool,
    place: Option<Frame>,
) -> Result<(RepairOutcome, Vec<u8>), String> {
    let opened = open(path, data)?;
    match opened.target {
        Generation::Gen9 => {
            repair::<or_io::decl::Gen9>(&opened, data, drop_copies, fix_counts, fix_palettes, place)
        }
        _ => {
            repair::<or_io::decl::Gen8>(&opened, data, drop_copies, fix_counts, fix_palettes, place)
                .and_then(|(outcome, bytes)| Ok((outcome, with_page_map(bytes)?)))
        }
    }
}

struct Outcome {
    source: String,
    report: Option<FixupReport>,
    detail: String,
}

impl Outcome {
    fn passed(&self) -> bool {
        self.report.as_ref().is_some_and(|r| r.is_clean())
    }

    /// Readable, and something the game would reject was found.
    fn flagged(&self) -> bool {
        self.report.as_ref().is_some_and(|r| !r.is_clean())
    }
}

fn outcome_for(path: &Path, data: &[u8], source: String) -> Outcome {
    match scan_bytes(path, data) {
        Ok(report) => {
            let detail = report.summary();
            Outcome {
                source,
                report: Some(report),
                detail,
            }
        }
        Err(e) => Outcome {
            source,
            report: None,
            detail: e,
        },
    }
}

fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries =
            std::fs::read_dir(&current).map_err(|e| format!("{}: {e}", current.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let leaf = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if is_drawable_bearing(leaf) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn scan_install(
    game_root: &Path,
    keys_path: Option<&Path>,
    verbose: bool,
) -> Result<Vec<Outcome>, String> {
    let game = keys::mount_all(game_root, keys_path)?;

    for finding in game.diagnostics.messages() {
        eprintln!("warning: {finding}");
    }

    let mut targets: Vec<String> = Vec::new();
    game.for_each_file(|logical, _container, leaf| {
        if is_drawable_bearing(leaf) {
            targets.push(logical.to_string());
        }
    });
    targets.sort();
    targets.dedup();

    eprintln!("scanning {} drawable-bearing files...", targets.len());

    let mut outcomes = Vec::with_capacity(targets.len());
    for (i, logical) in targets.iter().enumerate() {
        if verbose && i > 0 && i % 5000 == 0 {
            eprintln!("  {i}/{} ...", targets.len());
        }
        let Some(data) = game.vfs.open(logical) else {
            outcomes.push(Outcome {
                source: logical.clone(),
                report: None,
                detail: "could not open through the VFS".to_string(),
            });
            continue;
        };
        outcomes.push(outcome_for(&PathBuf::from(logical), &data, logical.clone()));
    }

    Ok(outcomes)
}

fn report_json(outcomes: &[Outcome]) {
    let flagged = outcomes.iter().filter(|o| o.flagged()).count();
    let unreadable = outcomes.iter().filter(|o| o.report.is_none()).count();

    let results: Vec<serde_json::Value> = outcomes
        .iter()
        .map(|o| {
            let blocks: Vec<serde_json::Value> = o
                .report
                .as_ref()
                .map(|r| {
                    r.aliased_blocks
                        .iter()
                        .map(|b| {
                            serde_json::json!({
                                "sourceAddress": format!("{:#X}", b.source_address),
                                "kind": b.kind.to_string(),
                                "sites": b.sites.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            serde_json::json!({
                "path": o.source,
                "ok": o.passed(),
                "readable": o.report.is_some(),
                "aliasedBlocks": blocks.len(),
                "oversizedPages": o
                    .report
                    .as_ref()
                    .map(|r| r.oversized_pages.iter().map(|p| p.to_string()).collect::<Vec<_>>())
                    .unwrap_or_default(),
                "detail": o.detail,
                "blocks": blocks,
            })
        })
        .collect();

    let doc = serde_json::json!({
        "files": outcomes.len(),
        "flagged": flagged,
        "unreadable": unreadable,
        "results": results,
    });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
}

fn report_text(outcomes: &[Outcome], verbose: bool) {
    let flagged = outcomes.iter().filter(|o| o.flagged()).count();
    let unreadable = outcomes.iter().filter(|o| o.report.is_none()).count();

    for o in outcomes {
        if o.passed() && !verbose {
            continue;
        }
        let mark = if o.passed() {
            "ok  "
        } else if o.report.is_none() {
            "SKIP"
        } else {
            "FAIL"
        };
        println!("{mark} {}  {}", o.source, o.detail);
        if let Some(report) = o.report.as_ref() {
            for line in report.describe().lines() {
                println!("       {line}");
            }
        }
    }

    println!();
    println!(
        "{} file(s) scanned; {flagged} the game would reject (aliased geometry or a page over its limit); {unreadable} unreadable",
        outcomes.len()
    );
    if flagged == 0 && unreadable == 0 {
        println!("no aliased geometry buffers and no oversized pages found");
    }
}

pub fn print_usage(program: &str) {
    eprintln!("Find drawables the game's pointer-fixup walk would reject");
    eprintln!();
    eprintln!("Usage: {program} fixups <file> [--json]");
    eprintln!("       {program} fixups --batch <dir> [--json] [-v]");
    eprintln!("       {program} fixups --game <install-root> [--keys <file>] [--json] [-v]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --batch <dir>   Recursively scan .ydr/.ydd/.yft/.ypt under <dir>");
    eprintln!("  --game <path>   Mount a GTA V install and walk it through the VFS");
    eprintln!("  --keys <path>   Key cache for encrypted archives (default: auto)");
    eprintln!("  --json          Emit machine-readable output");
    eprintln!("  -v, --verbose   List clean files too, and report scan progress");
    eprintln!();
    eprintln!("A vertex or index buffer reachable from two owning sites is resolved");
    eprintln!("twice by the engine's fixup pass, which has no visited set, and the");
    eprintln!("game rejects the file. Repair with `openrage-cli convert repair`.");
    eprintln!();
    eprintln!("A heap that needs a page over 100 MB (a heap of more than 64 MiB laid out");
    eprintln!("flat is one page of 128 MiB) is refused too, with ERR_STR_FAILURE_3, and");
    eprintln!("the client crashes. A level that copies the high one is the usual cause:");
    eprintln!("`openrage-cli convert repair <in> <out> --drop-copies` removes it.");
    eprintln!();
    eprintln!("Exits 1 when anything is flagged or a file cannot be read.");
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<PathBuf> = None;
    let mut batch: Option<PathBuf> = None;
    let mut game: Option<PathBuf> = None;
    let mut keys_path: Option<PathBuf> = None;
    let mut json = false;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage("openrage-cli inspect");
                return Ok(ExitCode::SUCCESS);
            }
            "--json" => json = true,
            "-v" | "--verbose" => verbose = true,
            "--batch" => {
                i += 1;
                batch = Some(PathBuf::from(
                    args.get(i).ok_or("--batch needs a directory".to_string())?,
                ));
            }
            "--game" => {
                i += 1;
                game = Some(PathBuf::from(
                    args.get(i).ok_or("--game needs a path".to_string())?,
                ));
            }
            "--keys" => {
                i += 1;
                keys_path = Some(PathBuf::from(
                    args.get(i).ok_or("--keys needs a path".to_string())?,
                ));
            }
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => file = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    let outcomes = match (&game, &batch, &file) {
        (Some(root), _, _) => scan_install(root, keys_path.as_deref(), verbose)?,
        (None, Some(dir), _) => {
            let files = collect_files(dir)?;
            if files.is_empty() {
                return Err(format!(
                    "no .ydr/.ydd/.yft/.ypt files found under {}",
                    dir.display()
                ));
            }
            files
                .iter()
                .map(|path| {
                    let source = path.display().to_string();
                    match std::fs::read(path) {
                        Ok(data) => outcome_for(path, &data, source),
                        Err(e) => Outcome {
                            source,
                            report: None,
                            detail: e.to_string(),
                        },
                    }
                })
                .collect()
        }
        (None, None, Some(path)) => {
            let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let report = scan_bytes(path, &data)?;
            vec![Outcome {
                detail: report.summary(),
                source: path.display().to_string(),
                report: Some(report),
            }]
        }
        (None, None, None) => {
            print_usage("openrage-cli inspect");
            return Ok(ExitCode::from(2));
        }
    };

    let show_all = verbose || (game.is_none() && batch.is_none());
    if json {
        report_json(&outcomes);
    } else {
        report_text(&outcomes, show_all);
    }

    Ok(if outcomes.iter().all(Outcome::passed) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// What a Legacy resource's `pgBase.first_node` leads to: `{ null, virtual page count, physical page count }`, then the room for the game's bookkeeping.
const PAGE_MAP_HEAD: usize = 0x10;
/// ⚠ The game's `PageMap` keeps three pointers a page (FiveM's `rage::PageMap::pageInfo`), so that is the room a map is given.
const PAGE_MAP_PER_PAGE: usize = 3 * 8;

/// The pages an RSC7 flag word describes: the four tails, and the counts of the five sizes of a leaf.
fn pages_of(flags: u32) -> usize {
    [(4, 1), (5, 3), (7, 15), (11, 63), (17, 127)]
        .iter()
        .map(|(shift, mask)| ((flags >> shift) & mask) as usize)
        .sum::<usize>()
        + (24..28).filter(|bit| flags >> bit & 1 == 1).count()
}

/// A Legacy resource written with the page map the game expects at the head of its root, which the writer leaves null.
///
/// ⚠ The game wants one: 4,865 of the 4,866 ped components that load in the game carry it, and three that the writer left without
/// crashed a client with `Invalid fixup, address is neither virtual nor physical`. The writer writes `PgBase.first_node` null by
/// decision (`census::RULED`) and its gates refuse a live word there, so the map is added here, to the bytes of a repair, in the
/// zero tail of the virtual half; a file with a word there already, or without room for the map, is returned as it is.
fn with_page_map(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let e = |e: or_io::Error| e.to_string();
    let opened = Rsc7::open(&bytes).map_err(e)?;
    let (virtual_size, mut data) = (opened.virtual_size as usize, opened.data);
    if data.len() < 16 || data[8..16].iter().any(|b| *b != 0) {
        return Ok(bytes);
    }
    let flags = |at: usize| {
        u32::from_le_bytes(
            bytes[at..at + 4]
                .try_into()
                .expect("a header is sixteen bytes"),
        )
    };
    let (virtual_pages, physical_pages) = (pages_of(flags(8)), pages_of(flags(12)));
    let room =
        (PAGE_MAP_HEAD + PAGE_MAP_PER_PAGE * (virtual_pages + physical_pages)).next_multiple_of(16);
    let used = data[..virtual_size]
        .iter()
        .rposition(|b| *b != 0)
        .map_or(0, |last| last + 1);
    let at = used.next_multiple_of(16);
    let (Ok(v), Ok(p)) = (u8::try_from(virtual_pages), u8::try_from(physical_pages)) else {
        return Ok(bytes);
    };
    if at + room > virtual_size {
        return Ok(bytes);
    }
    data[at + 8] = v;
    data[at + 9] = p;
    data[8..12].copy_from_slice(&(0x5000_0000 + at as u32).to_le_bytes());
    Rsc7::reseal(
        &data,
        &opened.seal,
        or_io::container::resource::Compression::best(),
    )
    .map_err(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sixteen bytes an RSC7 file starts with.
    fn header(virtual_flags: u32, physical_flags: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [0x3743_5352u32, 165, virtual_flags, physical_flags] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn a_legacy_file_without_a_page_map_gets_one_with_the_counts_of_its_flags_and_nothing_else_changes(
    ) {
        use or_io::container::resource::{Authoring, DatResourceFileType};
        let mut virtual_data = vec![0u8; 8192];
        virtual_data[0..8].copy_from_slice(&0x1_4000_0000u64.to_le_bytes());
        virtual_data[0x40..0x48].copy_from_slice(&0x1234_5678u64.to_le_bytes());
        let physical_data = vec![7u8; 8192];
        let spec = Authoring::new(
            DatResourceFileType::Ydd,
            or_io::container::resource::Compression::default(),
        )
        .for_generation(Generation::Gen8);
        let file =
            Rsc7::author(spec, virtual_data.clone(), physical_data.clone()).expect("authored");
        let mapped = with_page_map(file.clone()).expect("mapped");
        let opened = Rsc7::open(&mapped).expect("opens");
        let first_node = u64::from_le_bytes(opened.data[8..16].try_into().unwrap());
        assert_ne!(first_node, 0, "the page map was not added");
        let at = (first_node - 0x5000_0000) as usize;
        assert_eq!(
            at, 0x50,
            "the map goes in the zero tail, after the last word the file holds"
        );
        let flags = |at: usize| u32::from_le_bytes(file[at..at + 4].try_into().unwrap());
        assert_eq!(
            (opened.data[at + 8] as usize, opened.data[at + 9] as usize),
            (pages_of(flags(8)), pages_of(flags(12)))
        );
        assert!(opened.data[at..at + 8].iter().all(|b| *b == 0));
        let mut expected = virtual_data;
        expected[8..12].copy_from_slice(&(first_node as u32).to_le_bytes());
        expected[at + 8] = opened.data[at + 8];
        expected[at + 9] = opened.data[at + 9];
        assert_eq!(
            &opened.data[..8192],
            &expected[..],
            "the file changed beyond the pointer and the counts"
        );
        assert_eq!(&opened.data[8192..], &physical_data[..]);
        assert_eq!(
            with_page_map(mapped.clone()).unwrap(),
            mapped,
            "a file with a page map is returned as it is"
        );
    }

    #[test]
    fn a_heap_laid_out_as_one_page_of_128_mib_is_flagged_and_one_of_64_mib_is_not() {
        // The flags of the hair that crashed a client with ERR_STR_FAILURE_3: 0xA000001A is one page of 128 MiB.
        let found = oversized_pages(&header(0xA000_001A, 0x5000_0089));
        assert_eq!(
            found,
            vec![OversizedPage {
                heap: "virtual",
                bytes: 128 << 20
            }]
        );
        assert!(found[0].to_string().contains("128.0 MiB"));
        let physical = oversized_pages(&header(0xA000_0080, 0x5000_001A));
        assert_eq!(physical.len(), 1);
        assert_eq!(physical[0].heap, "physical");
        // A heap of 64 MiB is the largest one chunk holds under the limit.
        let fits = DatResourceSizes::single_chunk(64 << 20, RSC_VIRTUAL_LEAF_SIZE, 0).unwrap();
        assert!(oversized_pages(&header(fits.to_flags(), 0x5000_0089)).is_empty());
        // Bytes that are no header say nothing.
        assert!(oversized_pages(&[0u8; 4]).is_empty());
    }

    #[test]
    fn a_report_with_only_an_oversized_page_is_not_clean_and_says_so() {
        let report = FixupReport {
            aliased_blocks: Vec::new(),
            oversized_pages: vec![OversizedPage {
                heap: "physical",
                bytes: 128 << 20,
            }],
        };
        assert!(!report.is_clean());
        assert_eq!(report.summary(), "1 oversized page(s)");
        assert!(report.describe().contains("ERR_STR_FAILURE_3"));
        assert_eq!(FixupReport::default().summary(), "clean");
    }
}
