//! `openrage-cli analyse <file.ynv>` — what a resource says about its own shape.
//!
//! For formats no reader covers: an install ships `.yvr` at 7,533 files and
//! `.ywr` at 1,612.
//!
//! ⚠ **It needs no reader for the format.** A paged RSC7 resource rebases its
//! pointers, so a `u64` the envelope can resolve is a candidate pointer whatever
//! the struct holding it turns out to be.
//!
//! ⚠ **It reports a stride only where a count of two or more divided the span**,
//! and says which of three reasons it could not otherwise. A glass pane
//! declared at 0x70 measured 0x40, and only a four-pane carrier showed it; a
//! tool printing a span as a stride would repeat that at ten thousand files a
//! time. See `crates/io/or-io/tests/what_an_analyser_cannot_know.rs`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::desc::analyse::{layout_of, Confidence, Extent, Space};
use or_io::desc::Image;

pub fn print_usage() {
    eprintln!("Usage: openrage-cli analyse <file> [options]");
    eprintln!();
    eprintln!("Reports what a paged resource's own pointers say about its layout,");
    eprintln!("with no reader for the format.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --game <path>   Read from an install instead of a loose file, and");
    eprintln!("                  show each entry's own metadata beside its layout.");
    eprintln!("  --glob <pat>    Which files, with --game. e.g. `**/*.ynv`.");
    eprintln!("  --top <n>       Show at most n blocks. Default 40.");
    eprintln!("  --all           Show every block.");
    eprintln!("  --min <level>   weak | plausible | corroborated. Default plausible.");
    eprintln!("  --sites         List candidate pointers as well as blocks.");
}

pub fn run(args: &[String]) -> ExitCode {
    match run_inner(args) {
        Ok(code) => code,
        Err(e) if e.is_empty() => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn level(name: &str) -> Option<Confidence> {
    match name {
        "weak" => Some(Confidence::Weak),
        "plausible" => Some(Confidence::Plausible),
        "corroborated" => Some(Confidence::Corroborated),
        _ => None,
    }
}

fn run_inner(args: &[String]) -> Result<ExitCode, String> {
    let mut path: Option<PathBuf> = None;
    let mut game: Option<PathBuf> = None;
    let mut glob: Option<String> = None;
    let mut top = 40usize;
    let mut min = Confidence::Plausible;
    let mut show_sites = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game" => {
                game = Some(PathBuf::from(args.get(i + 1).ok_or("--game needs a path")?));
                i += 2;
            }
            "--glob" => {
                glob = Some(args.get(i + 1).ok_or("--glob needs a pattern")?.clone());
                i += 2;
            }
            "--top" => {
                top = args
                    .get(i + 1)
                    .ok_or("--top needs a number")?
                    .parse()
                    .map_err(|e| format!("--top: {e}"))?;
                i += 2;
            }
            "--all" => {
                top = usize::MAX;
                i += 1;
            }
            "--min" => {
                let name = args.get(i + 1).ok_or("--min needs a level")?;
                min = level(name).ok_or_else(|| format!("--min: unknown level `{name}`"))?;
                i += 2;
            }
            "--sites" => {
                show_sites = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_usage();
                return Err(String::new());
            }
            other if other.starts_with('-') => return Err(format!("unknown argument: {other}")),
            other => {
                path = Some(PathBuf::from(other));
                i += 1;
            }
        }
    }

    if let Some(root) = game {
        let glob = glob.ok_or("--game needs --glob to say which files")?;
        return through_an_install(&root, &glob, top, min, show_sites);
    }

    let path = path.ok_or_else(|| {
        print_usage();
        "missing <file>, or --game with --glob".to_string()
    })?;
    let bytes = std::fs::read(&path).map_err(|e| format!("read({}): {e}", path.display()))?;

    report(&path, &bytes, top, min, show_sites)
}

/// Analyse files inside an install, with the archive entry's own metadata beside
/// the layout.
///
/// The entry table holds the **RSC7 version** and the **paging words** without
/// any body being read (`EntryFacts`), and a version is the first thing that
/// splits one extension into several layouts: `.yfd` and `.ybd` span **three
/// RSC7 versions across four files each**, so analysing them as one format would
/// average three shapes into nonsense.
fn through_an_install(
    root: &Path,
    glob: &str,
    top: usize,
    min: Confidence,
    show_sites: bool,
) -> Result<ExitCode, String> {
    let game = super::keys::mount_all(root, None)?;

    let pattern = or_title::vfs::Pattern::new(glob);
    let mut matched: Vec<or_title::vfs::EntryFacts> = Vec::new();
    game.for_each_entry(|facts| {
        if pattern.matches_path(&facts.logical) {
            matched.push(facts.clone());
        }
    });
    if matched.is_empty() {
        return Err(format!("nothing matched `{glob}`"));
    }

    // ⚠ Grouped by version before anything is analysed, because one extension at
    // two versions is two layouts and averaging them is worse than analysing
    // neither.
    let mut by_version: std::collections::BTreeMap<Option<u32>, usize> =
        std::collections::BTreeMap::new();
    for facts in &matched {
        *by_version.entry(facts.resource_version()).or_default() += 1;
    }
    println!("{} entr(ies) match `{glob}`", matched.len());
    for (version, count) in &by_version {
        match version {
            Some(v) => println!("  {count:>6} at RSC7 version {v}"),
            None => println!("  {count:>6} not a resource"),
        }
    }
    if by_version.len() > 1 {
        println!(
            "  ⚠ more than one version: these are different layouts and must be \
             analysed apart"
        );
    }
    println!();

    for facts in matched.iter().take(top.min(8)) {
        println!("--- {}", facts.logical);
        println!(
            "    entry: {:?}, {} byte(s) stored, {} inflated, {}{}",
            facts.kind,
            facts.stored_size,
            facts.size,
            if facts.compressed {
                "deflated"
            } else {
                "stored"
            },
            match facts.paging {
                Some((system, graphics)) => format!(", paging {system:#010x}/{graphics:#010x}"),
                None => String::new(),
            }
        );
        let Some(bytes) = game.vfs.open(&facts.logical) else {
            println!("    (would not read)");
            continue;
        };
        if let Err(e) = report(Path::new(&facts.leaf), &bytes, top, min, show_sites) {
            println!("    {e}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn report(
    path: &Path,
    bytes: &[u8],
    top: usize,
    min: Confidence,
    show_sites: bool,
) -> Result<ExitCode, String> {
    if !Rsc7::detect(bytes) {
        return Err(format!(
            "{} is not an RSC7 resource. This reads paged resources; a \
             plain-binary format has no rebased pointers to walk.",
            path.display()
        ));
    }
    let opened = Rsc7::open(bytes).map_err(|e| format!("open: {e}"))?;
    let seal = Rsc7::peek(bytes).map_err(|e| format!("peek: {e}"))?;

    let image = Image::new(&opened.data, opened.virtual_size as usize);
    let layout = layout_of(image);
    let (known, total) = layout.strides_known();

    println!("{}", path.display());
    println!(
        "  RSC7 version {}, {} byte(s) system, {} byte(s) graphics",
        seal.version(),
        layout.virtual_size,
        layout.physical_size
    );
    println!(
        "  {} candidate pointer(s), {total} allocation(s)",
        layout.sites.len()
    );
    // ⚠ By bytes as well as by blocks: blocks differ in size by orders of
    // magnitude, so a block count alone misstates how much is explained.
    let (explained, in_blocks) = layout.explained();
    let percent = if in_blocks == 0 {
        0.0
    } else {
        explained as f64 / in_blocks as f64 * 100.0
    };
    println!(
        "  strides derived: {known} of {total} block(s), covering {explained} of \
         {in_blocks} byte(s) ({percent:.0}%)"
    );
    println!(
        "  {} byte(s) of root struct, which nothing points at",
        layout.root_bytes
    );
    println!();

    let shown: Vec<_> = layout
        .blocks
        .iter()
        .filter(|b| b.confidence >= min)
        .collect();
    println!("offset      span   refs  space     stride",);
    for block in shown.iter().take(top) {
        let stride = match &block.extent {
            Extent::Stride { bytes, from_count } => {
                format!("{bytes:#x} (count {from_count})")
            }
            // ⚠ The reason travels with the refusal. "unknown" alone invites
            // somebody to assume the span is the answer, which is the error.
            Extent::StrideUnknown { because } => format!("— {because}"),
        };
        println!(
            "{:#010x}  {:>6}  {:>4}  {:<8}  {stride}",
            block.at,
            block.span,
            block.referrers,
            match block.space {
                Space::Virtual => "system",
                Space::Physical => "graphics",
            },
        );
    }
    if shown.len() > top {
        println!("  ... and {} more", shown.len() - top);
    }
    let hidden = layout.blocks.len() - shown.len();
    if hidden > 0 {
        println!(
            "  {hidden} block(s) below `{}` confidence not shown; --min weak to see them",
            match min {
                Confidence::Weak => "weak",
                Confidence::Plausible => "plausible",
                Confidence::Corroborated => "corroborated",
            }
        );
    }

    if show_sites {
        println!();
        println!("candidate pointers:");
        for site in layout.sites.iter().take(top) {
            println!(
                "  {:#010x} -> {:#010x}  {}",
                site.at,
                site.target,
                match site.space {
                    Space::Virtual => "system",
                    Space::Physical => "graphics",
                }
            );
        }
        if layout.sites.len() > top {
            println!("  ... and {} more", layout.sites.len() - top);
        }
    }

    Ok(ExitCode::SUCCESS)
}
