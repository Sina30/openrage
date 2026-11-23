//! `rpf scan-census`: every metadata row of a GTA VI archive through `rpf scan-open`, each in a
//! process of its own, tallied by what was read, by refusal and by save line; nothing is written.

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use or_io::container::rpf::{Rpf9BodyKind, Rpf9Content, Rpf9Scan};
use or_job::Step;

const USAGE: &str =
    "usage: rpf scan-census <archive.rpf> [--from <offset>] [--to <offset>] [--every <n>] [--limit <bytes>]";

/// What `scan-open` says when a reader of metadata, not the container, refuses a body.
const READER_REFUSALS: [&str; 4] = [
    "the PSO reader does not read this body",
    "the RBF reader does not read this body",
    "GTA VI's RBM reader does not read this body's two sections",
    "the metadata reader does not read this body",
];

/// The counts of one run, each message with its numbers as N.
#[derive(Default)]
struct Tally {
    /// Per row: read, refused.
    rows: BTreeMap<String, (u64, u64)>,
    /// Per refusal: how many, and the first offset that gave it.
    refusals: BTreeMap<String, (u64, u64)>,
    saves: BTreeMap<String, u64>,
    by_readers: u64,
}

pub(crate) fn cmd_scan_census(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let (from, args) = super::rpf::split_offset_flag(&args, "--from")?;
    let (to, args) = super::rpf::split_offset_flag(&args, "--to")?;
    let (every, args) = super::rpf::split_offset_flag(&args, "--every")?;
    let [path] = args.as_slice() else {
        return Err(USAGE.to_string());
    };
    let every = every.unwrap_or(1).max(1);
    let exe = std::env::current_exe().map_err(|e| format!("where this program is: {e}"))?;
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let cancel = super::jobs::on_interrupt();
    let mut rows = Vec::new();
    let mut seen = 0u64;
    let range = from.unwrap_or(0)..to.unwrap_or(u64::MAX);
    for find in Rpf9Scan::new(&file, range).map_err(|e| e.to_string())? {
        if cancel.is_set() {
            return Err("cancelled".to_string());
        }
        let find = find.map_err(|e| e.to_string())?;
        if !is_metadata_row(&find.kind) {
            continue;
        }
        seen += 1;
        if (seen - 1).is_multiple_of(every) {
            rows.push((find.offset, super::rpf::scan_row(&find)));
        }
    }
    println!(
        "  census        {} of {seen} metadata rows (every {every}), each through `rpf scan-open` in a process of its own; nothing is written",
        rows.len()
    );
    let progress = or_job::Progress::new();
    let mut ticker = super::jobs::Ticker::new(&progress);
    let mut tally = Tally::default();
    let limit = limit.to_string();
    let report = or_job::run(rows, &cancel, &progress, |(offset, row)| {
        ticker.tick();
        let at = format!("{offset:#X}");
        let out = Command::new(&exe)
            .args(["rpf", "scan-open", path.as_str(), at.as_str()])
            .args(["--limit", limit.as_str()])
            .output();
        match out {
            Ok(out) => {
                tally.add(offset, row, out.status.success(), &out.stdout, &out.stderr);
                Step::Done(0)
            }
            Err(e) => Step::Failed(format!("{offset:#X}: scan-open did not start: {e}")),
        }
    });
    ticker.finish(&report);
    tally.print();
    match (report.cancelled, report.failures.first(), tally.by_readers) {
        (true, _, _) => Err("cancelled".to_string()),
        (false, Some(_), _) => Err(report.failures.join("; ")),
        (false, None, 0) => Ok(()),
        (false, None, n) => Err(format!("{n} bodies were refused by a reader of metadata")),
    }
}

/// Whether a find is a row a reader of metadata may be handed.
fn is_metadata_row(kind: &Rpf9BodyKind) -> bool {
    matches!(kind, Rpf9BodyKind::RawDeflate { .. })
        || matches!(
            kind,
            Rpf9BodyKind::Zlib(run) if matches!(
                run.content,
                Rpf9Content::Pso
                    | Rpf9Content::Rbf
                    | Rpf9Content::Xml
                    | Rpf9Content::ResourceShaped
                    | Rpf9Content::TextTable
                    | Rpf9Content::Other
            )
        )
}

impl Tally {
    fn add(&mut self, offset: u64, row: String, read: bool, stdout: &[u8], stderr: &[u8]) {
        let (stdout, stderr) = (
            String::from_utf8_lossy(stdout),
            String::from_utf8_lossy(stderr),
        );
        let counts = self.rows.entry(row).or_default();
        let mut refusal = None;
        match read {
            true => counts.0 += 1,
            false => {
                counts.1 += 1;
                refusal = (stderr.lines().rev())
                    .find_map(|l| l.strip_prefix("error: "))
                    .map(str::to_string);
            }
        }
        // A map body's typed reader can read a body the tree reader refused.
        let tree = (stdout.lines()).find_map(|l| {
            (l.trim_start().strip_prefix("tree "))
                .map(str::trim)
                .filter(|l| READER_REFUSALS.iter().any(|r| l.starts_with(r)))
        });
        if let Some(why) = refusal.as_deref().or(tree) {
            if READER_REFUSALS.iter().any(|r| why.starts_with(r)) {
                self.by_readers += 1;
            }
            let cause = self
                .refusals
                .entry(numbers_as_n(why))
                .or_insert((0, offset));
            cause.0 += 1;
        } else if !read {
            self.refusals
                .entry("refused, with no message".to_string())
                .or_insert((0, offset))
                .0 += 1;
        }
        if let Some(save) = (stdout.lines()).find_map(|l| l.trim_start().strip_prefix("save ")) {
            *self.saves.entry(numbers_as_n(save.trim())).or_default() += 1;
        }
    }

    fn print(&self) {
        for (row, (read, refused)) in &self.rows {
            println!("  row           {row}: {read} read, {refused} refused");
        }
        for (cause, (count, first)) in &self.refusals {
            println!("  refused       {count}  {cause}  (the first at {first:#X})");
        }
        for (save, count) in &self.saves {
            println!("  save          {count}  {save}");
        }
        println!(
            "  readers       {} refusals by a reader of metadata",
            self.by_readers
        );
    }
}

/// `text` with each number, decimal, `0x` hexadecimal or bare hexadecimal digits, written as N.
fn numbers_as_n(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if !c.is_ascii_digit() {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }
        let len = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        let word = &rest[..len];
        let hex = word.strip_prefix("0x").unwrap_or(word);
        match !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            true => out.push('N'),
            false => out.push_str(word),
        }
        rest = &rest[len..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_become_n_and_words_that_start_with_a_digit_stay() {
        assert_eq!(
            numbers_as_n("block 12 (0x1AE00000) differs from byte 0x1F0"),
            "block N (N) differs from byte N"
        );
        assert_eq!(numbers_as_n("has no 0DRP at 0x10"), "has no 0DRP at N");
    }

    #[test]
    fn a_reader_refusal_is_told_from_the_containers_and_saves_are_counted() {
        let mut tally = Tally::default();
        let container = b"error: the zlib chunk at 0x40 fails partway through\n";
        tally.add(0x100, "zlib other".into(), false, b"", container);
        let reader = b"error: the PSO reader does not read this body: block 3 is short\n";
        tally.add(0x200, "zlib pso".into(), false, b"", reader);
        let read = b"  save         writes back byte-identical; checksum 0x12345678 kept\n";
        tally.add(0x300, "zlib pso".into(), true, read, b"");
        let tree =
            b"  tree          GTA VI's RBM reader does not read this body's two sections: x\n";
        tally.add(0x400, "headerless map".into(), true, tree, b"");

        assert_eq!(tally.by_readers, 2);
        assert_eq!(tally.rows["zlib pso"], (1, 1));
        assert_eq!(
            tally.refusals["the zlib chunk at N fails partway through"],
            (1, 0x100)
        );
        assert_eq!(
            tally.refusals["the PSO reader does not read this body: block N is short"],
            (1, 0x200)
        );
        assert_eq!(
            tally.saves["writes back byte-identical; checksum N kept"],
            1
        );

        // A raw DEFLATE stream is a metadata row, and its reader's refusal is a reader's.
        let raw = Rpf9BodyKind::RawDeflate {
            content: Rpf9Content::Rbf,
        };
        assert!(is_metadata_row(&raw));
        let refused = b"error: the metadata reader does not read this body: short\n";
        tally.add(0x500, "raw deflate rbf".into(), false, b"", refused);
        assert_eq!(tally.by_readers, 3);
    }
}
