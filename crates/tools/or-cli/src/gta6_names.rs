//! `rpf scan-names`: the names GTA VI archives' bodies spell, harvested into a vocabulary kept in
//! the configuration folder, with the name each drawable and fragment states of itself. The
//! other GTA VI commands show a hash by the name the kept vocabulary holds for it.

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use or_io::gta6::names::{self, ArchiveHarvest, Vocabulary, HARVEST_PREFIX};
use or_io::gta6::texture_names::name_keys;
use or_io::gta6::TexturePack;

const USAGE: &str =
    "usage: rpf scan-names [<archive.rpf>...] [--prefix <bytes>] [--threads <count>] [--pack <file.ttpf>]";

/// The vocabulary kept in the configuration folder, read once; empty where none is kept.
pub(crate) fn vocabulary() -> &'static Vocabulary {
    static HELD: OnceLock<Vocabulary> = OnceLock::new();
    HELD.get_or_init(|| {
        // A test prints what its own bodies state, whatever this machine keeps.
        if cfg!(test) {
            return Vocabulary::default();
        }
        or_title::config::config_dir()
            .map(|root| Vocabulary::load(&names::vocabulary_file(&root)))
            .unwrap_or_default()
    })
}

/// ` (name)` where the kept vocabulary holds the one name the 32-bit `hash` names, else nothing.
pub(crate) fn said32(hash: u32) -> String {
    said(vocabulary().name32(hash))
}

/// ` (name)` where the kept vocabulary holds a name both `hash` and the 64-bit `word` name.
pub(crate) fn said_both(hash: u32, word: u64) -> String {
    said(vocabulary().confirmed(hash, word))
}

fn said(name: Option<&str>) -> String {
    name.map_or_else(String::new, |name| format!(" ({name})"))
}

/// Harvests every body of the archive at `path` a harvest reads, the first `prefix` bytes of
/// each, on `threads` threads, with a line a second on stderr that says how far it has got.
fn harvest_archive(
    path: &str,
    prefix: usize,
    threads: usize,
    cancelled: &or_job::Cancel,
) -> Result<ArchiveHarvest, String> {
    let (began, last) = (Instant::now(), Mutex::new(Instant::now()));
    let reached = |done: u64, len: u64| {
        let Ok(mut last) = last.lock() else {
            return;
        };
        if last.elapsed() < Duration::from_secs(1) && done < len {
            return;
        }
        *last = Instant::now();
        let secs = began.elapsed().as_secs_f64();
        let mut err = std::io::stderr().lock();
        let _ = writeln!(
            err,
            "  {path}: {} of {} MiB, {secs:.0} s",
            done >> 20,
            len.div_ceil(1 << 20)
        );
    };
    let stop = || cancelled.is_set();
    let gave = names::harvest_archive(Path::new(path), prefix, threads, &stop, &reached)
        .map_err(|e| format!("{path}: {e}"))?;
    match gave.stopped {
        true => Err("cancelled".to_string()),
        false => Ok(gave),
    }
}

/// The lines that count what `vocabulary` holds.
fn count_lines(vocabulary: &Vocabulary) -> Vec<String> {
    let counts = vocabulary.counts();
    let kinds: Vec<String> = (counts.kinds.iter())
        .map(|(kind, n)| format!("{n} .#{}{}", char::from(kind[0]), char::from(kind[1])))
        .collect();
    let resource = counts.names - counts.plain;
    vec![
        format!(
            "  names         {} held: {resource} spelled with an ending{}{}, {} plain",
            counts.names,
            if kinds.is_empty() { "" } else { ": " },
            kinds.join(", "),
            counts.plain
        ),
        format!(
            "  32-bit        {} hashes, {} of them shared by two names or more, which name nothing alone",
            counts.hashes32, counts.shared32
        ),
    ]
}

/// The lines that count the keys of the texture pack at `path` the vocabulary names: by a name it
/// holds, then by the naming rules from each name it holds with an ending.
fn pack_lines(path: &str, vocabulary: &Vocabulary) -> Result<Vec<String>, String> {
    let pack = TexturePack::open(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let keys: HashSet<u64> = pack.entries.iter().map(|e| e.key).collect();
    let (held, left): (HashSet<u64>, HashSet<u64>) =
        (keys.iter().copied()).partition(|key| vocabulary.name64(*key).is_some());
    let mut by_rule = BTreeMap::new();
    for (_, rule) in name_keys(vocabulary.resource_names(), &left).into_values() {
        *by_rule.entry(rule.label()).or_insert(0usize) += 1;
    }
    let ruled: usize = by_rule.values().sum();
    let mut lines = vec![format!(
        "  pack          {} of {} keys named: {} by a name held, {ruled} by a rule",
        held.len() + ruled,
        keys.len(),
        held.len()
    )];
    lines.extend((by_rule.iter()).map(|(rule, n)| format!("    {n} by the rule \"{rule}\"")));
    Ok(lines)
}

pub(crate) fn cmd_scan_names(args: &[String]) -> Result<(), String> {
    let (mut archives, mut prefix, mut pack) = (Vec::new(), HARVEST_PREFIX, None);
    let mut threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--prefix" => {
                let bytes = words.next().ok_or("--prefix needs a byte count")?;
                prefix = super::rpf::parse_offset(bytes)? as usize;
            }
            "--threads" => {
                let count = words.next().ok_or("--threads needs a count")?;
                threads =
                    (count.parse().ok().filter(|n| *n > 0)).ok_or("--threads needs a count")?;
            }
            // Once the most a body inflated to; a harvest holds only a body's first bytes.
            "--limit" => {
                words.next().ok_or("--limit needs a byte count")?;
            }
            "--pack" => pack = Some(words.next().ok_or("--pack needs a file")?),
            other if other.starts_with('-') => return Err(USAGE.to_string()),
            path => archives.push(path),
        }
    }
    let root = or_title::config::config_dir();
    let kept = root.as_deref().map(names::vocabulary_file);
    let mut vocabulary = kept.as_deref().map(Vocabulary::load).unwrap_or_default();
    let cancelled = super::jobs::on_interrupt();
    for path in archives {
        let before = vocabulary.len();
        let gave = harvest_archive(path, prefix, threads, &cancelled)?;
        vocabulary.merge(&gave.vocabulary);
        println!(
            "  {path}: {} bodies read, {} names new, {} bodies state their own name",
            gave.bodies,
            vocabulary.len() - before,
            gave.labels.len()
        );
        if let Some(root) = &root {
            names::save_labels(root, Path::new(path), &gave.labels)
                .map_err(|e| format!("keeping the names of {path}: {e}"))?;
        }
    }
    count_lines(&vocabulary)
        .iter()
        .for_each(|l| println!("{l}"));
    match &kept {
        Some(file) => {
            vocabulary
                .save(file)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            println!("  kept          {}", file.display());
        }
        None => println!("  kept          nowhere: no configuration directory"),
    }
    if let Some(pack) = pack {
        pack_lines(pack, &vocabulary)?
            .iter()
            .for_each(|l| println!("{l}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counts_say_each_ending_and_the_plain_names() {
        let mut vocabulary = Vocabulary::default();
        assert!(count_lines(&vocabulary)[0].contains("0 held: 0 spelled with an ending, 0 plain"));
        vocabulary.harvest(
            b"\0zq_shed_07.#dr\0zq_crane.#ft\0zq_hut.#dr\0zq_plain\0",
            1 << 10,
        );
        let lines = count_lines(&vocabulary);
        assert!(
            lines[0].contains("4 held: 3 spelled with an ending: 2 .#dr, 1 .#ft, 1 plain"),
            "{lines:?}"
        );
        assert!(lines[1].contains("4 hashes, 0 of them shared"), "{lines:?}");
    }

    #[test]
    fn a_hash_no_kept_name_names_is_said_as_before() {
        assert_eq!(said(None), "");
        assert_eq!(said(Some("zq_shed_07")), " (zq_shed_07)");
        // A test reads no kept vocabulary.
        assert!(vocabulary().is_empty());
        assert_eq!(said32(or_io::gta6::hash::name_hash32(b"zq_shed_07")), "");
    }
}
