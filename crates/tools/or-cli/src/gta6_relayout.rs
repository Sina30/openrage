//! `rpf scan-relayout`: whether the loose-resource writer's rules give a scanned resource's
//! stored block map, blocks and RSC8 head back from its sections, re-derived in memory alone.

use std::fs;
use std::io::{Cursor, Read, Seek};

use or_io::container::rpf::{
    read_rpf9_body, rpf9_block_streams, write_rpf9_resource, Rpf9BodyKind, Rpf9Resource, Rpf9Scan,
    Rpf9ScanFind, Rpf9Sections, Rpf9ZlibRun,
};

/// A block whose bytes a stream cap may cut into several streams: one of more than 8 MiB.
const PARTITIONED: u64 = 8 << 20;

const USAGE: &str = "usage: rpf scan-relayout <archive.rpf> [--from <offset>] [--to <offset>] \
                     [--every <n>] [--count <n>] [--limit <bytes>]";

/// How one resource's layout compares with what the rules derive from its sections.
#[derive(Default)]
struct Tally {
    checked: u64,
    blocks: u64,
    map: u64,
    heads: u64,
    rsc8: u64,
    refused: u64,
    /// Resources with a block of more than 8 MiB, and those whose blocks' streams are the same.
    partitioned: u64,
    streams: u64,
}

pub(crate) fn cmd_scan_relayout(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let (from, args) = super::rpf::split_offset_flag(&args, "--from")?;
    let (to, args) = super::rpf::split_offset_flag(&args, "--to")?;
    let (every, args) = super::rpf::split_offset_flag(&args, "--every")?;
    let (count, args) = super::rpf::split_offset_flag(&args, "--count")?;
    let [path] = args.as_slice() else {
        return Err(USAGE.to_string());
    };
    let (every, count) = (every.unwrap_or(1).max(1), count.unwrap_or(u64::MAX));
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let range = from.unwrap_or(0)..to.unwrap_or(u64::MAX);
    let cancelled = super::jobs::on_interrupt();
    let mut tally = Tally::default();
    let mut seen = 0u64;
    println!("  relayout      each resource's sections re-derived in memory; nothing is written");
    for find in Rpf9Scan::new(&file, range).map_err(|e| e.to_string())? {
        if cancelled.is_set() {
            return Err("cancelled".to_string());
        }
        let find = find.map_err(|e| e.to_string())?;
        let whole = match find.kind {
            Rpf9BodyKind::Zlib(Rpf9ZlibRun {
                resource: Some(_), ..
            }) => find.stored_len.is_some(),
            Rpf9BodyKind::StoredRsc8 { .. } => true,
            _ => false,
        };
        if !whole {
            continue;
        }
        seen += 1;
        if !(seen - 1).is_multiple_of(every) {
            continue;
        }
        println!("{}", relayout(&file, &find, limit, &mut tally));
        if tally.checked >= count {
            break;
        }
    }
    let share =
        |n: u64, of: u64| format!("{n} of {of} ({:.2}%)", 100.0 * n as f64 / of.max(1) as f64);
    let mapped = tally.checked - tally.rsc8;
    println!(
        "summary: {} resources; blocks the same {}; block map the same {}; RSC8 head the same {}; the writer refused {}; streams of blocks past 8 MiB the same {}",
        tally.checked,
        share(tally.blocks, mapped),
        share(tally.map, mapped),
        share(tally.heads, tally.rsc8),
        tally.refused,
        share(tally.streams, tally.partitioned)
    );
    Ok(())
}

/// The inflated length of each block, virtual then physical.
fn blocks(resource: &Rpf9Resource) -> Vec<u64> {
    (resource.blocks.iter().flatten())
        .map(|b| b.inflated_len)
        .collect()
}

fn resource(find: &Rpf9ScanFind) -> Option<Rpf9Resource> {
    match find.kind {
        Rpf9BodyKind::Zlib(run) => run.resource,
        _ => None,
    }
}

/// One line on the resource `find` of `file`: its sections written in memory, then read again.
fn relayout(file: &fs::File, find: &Rpf9ScanFind, limit: u64, tally: &mut Tally) -> String {
    tally.checked += 1;
    let rsc8 = matches!(find.kind, Rpf9BodyKind::StoredRsc8 { .. });
    tally.rsc8 += u64::from(rsc8);
    let at = find.offset;
    let derived = read_rpf9_body(file, at, limit)
        .map_err(|e| e.to_string())
        .and_then(|(read, bytes)| {
            let sections = Rpf9Sections::of(&read, &bytes).ok_or("no resource".to_string())?;
            let written = write_rpf9_resource(&sections).map_err(|e| e.to_string())?;
            let (again, back) =
                read_rpf9_body(Cursor::new(&written), 0, limit).map_err(|e| e.to_string())?;
            let back = Rpf9Sections::of(&again, &back).ok_or("no resource".to_string())?;
            Ok((read, bytes, sections, written, again, back))
        });
    let (read, bytes, sections, written, again, back) = match derived {
        Ok(d) => d,
        Err(e) => {
            tally.refused += 1;
            return format!("  {at:#012X}  refused: {e}");
        }
    };
    if rsc8 {
        let same = bytes.get(..16) == written.get(..16);
        tally.heads += u64::from(same);
        return format!(
            "  {at:#012X}  stored rsc8  head {}",
            if same { "the same" } else { "differs" }
        );
    }
    let (stored, derived) = (resource(&read), resource(&again));
    let counts = |r: &Option<Rpf9Resource>| r.map(|r| (r.virtual_blocks, r.physical_blocks));
    let same_blocks = counts(&stored) == counts(&derived)
        && stored.map(|r| blocks(&r)) == derived.map(|r| blocks(&r));
    let same_map = back.virtual_section == sections.virtual_section;
    tally.blocks += u64::from(same_blocks);
    tally.map += u64::from(same_map);
    let streams = match (stored, derived) {
        (Some(s), Some(d)) if same_blocks => streams_line(file, &s, &written, &d, tally),
        _ => String::new(),
    };
    let show = |r: Option<Rpf9Resource>| {
        r.map_or_else(String::new, |r| {
            format!(
                "v={} p={} {:?}",
                r.virtual_blocks,
                r.physical_blocks,
                blocks(&r)
            )
        })
    };
    format!(
        "  {at:#012X}  blocks {} (stored {}{}), block map {}{streams}",
        if same_blocks { "the same" } else { "differ" },
        show(stored),
        if same_blocks {
            String::new()
        } else {
            format!(", derived {}", show(derived))
        },
        if same_map { "the same" } else { "differs" }
    )
}

/// The streams of each block of more than 8 MiB of the resource `source` lays out as `resource`.
fn partitions<R: Read + Seek>(
    source: impl Fn() -> R,
    resource: &Rpf9Resource,
) -> Result<Vec<Vec<u64>>, String> {
    (resource.blocks.iter().flatten())
        .filter(|block| block.inflated_len > PARTITIONED)
        .map(|block| rpf9_block_streams(source(), block).map_err(|e| e.to_string()))
        .collect()
}

/// How the streams of each block of more than 8 MiB compare, stored in `file` and as the writer
/// cut them into `written`; nothing where no block is that long.
fn streams_line(
    file: &fs::File,
    stored: &Rpf9Resource,
    written: &[u8],
    derived: &Rpf9Resource,
    tally: &mut Tally,
) -> String {
    let stored = partitions(|| file, stored);
    let derived = partitions(|| Cursor::new(written), derived);
    match (stored, derived) {
        (Ok(stored), _) if stored.is_empty() => String::new(),
        (Ok(stored), Ok(derived)) => {
            tally.partitioned += 1;
            if stored == derived {
                tally.streams += 1;
                format!(", streams past 8 MiB the same {stored:?}")
            } else {
                format!(", streams past 8 MiB differ: stored {stored:?}, derived {derived:?}")
            }
        }
        (Err(e), _) | (_, Err(e)) => format!(", streams past 8 MiB not read: {e}"),
    }
}
