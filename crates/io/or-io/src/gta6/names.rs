//! A vocabulary of names harvested from GTA VI bodies. An archive's table is encrypted, and its
//! bodies spell their own and each other's names (`name.#dr`, `name.#ft`, ...) beside plain ones;
//! each is hashed both ways, so a 32-bit or 64-bit word read elsewhere is shown by its name.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::hash::{name_hash32, name_hash64};
use crate::container::rpf::{
    Rpf9BodyKind, Rpf9Content, Rpf9Role, Rpf9Scan, Rpf9ScanFind, Rpf9ZlibRun,
};

/// The bytes of a body a harvest reads by default, from its start: what each worker of a harvest
/// holds of one body at most.
pub const HARVEST_PREFIX: usize = 16 << 20;
/// The bytes of an archive one scan of a harvest covers, and the step its progress moves by.
const HARVEST_RANGE: u64 = 64 << 20;
/// The longest name kept, without its `.#xx` ending.
pub const MAX_NAME: usize = 96;
/// The shortest plain name kept: a shorter one is as likely chance as a name.
const MIN_PLAIN: usize = 3;
/// The bytes a body is judged text by: one with no NUL among them is.
const TEXT_PROBE: usize = 4096;
/// The longest line of a kept file read as one: a kind or an offset, a space and a name.
const MAX_LINE: u64 = 160;
const VOCABULARY_HEAD: &str = "openrage gta6 names 1";
const LABELS_HEAD: &str = "openrage gta6 labels 1";

/// A name a body spells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Harvested<'a> {
    /// The name without its `.#xx` ending, as written.
    pub name: &'a str,
    /// The two characters after `.#`, lower-cased; `None` for a plain name.
    pub kind: Option<[u8; 2]>,
}

fn printable(b: u8) -> bool {
    (0x20..0x7F).contains(&b)
}

fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'@')
}

fn kind_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// A name's stem and the two characters after `.#`, lower-cased, where it ends in them.
pub fn split_kind(name: &str) -> (&str, Option<[u8; 2]>) {
    match name.as_bytes() {
        [stem @ .., b'.', b'#', a, b] if kind_byte(*a) && kind_byte(*b) => (
            &name[..stem.len()],
            Some([a.to_ascii_lowercase(), b.to_ascii_lowercase()]),
        ),
        _ => (name, None),
    }
}

/// Each name the first `prefix` bytes of `body` spell, handed to `each` in file order.
// A `name.#xx` is taken wherever it is printed. A plain name is taken from text, and from a
// binary body only out of a NUL-ended string that is all names and path separators: chance
// bytes spell short printable runs, and a vocabulary of them names nothing.
pub fn harvest<'a>(body: &'a [u8], prefix: usize, each: &mut impl FnMut(Harvested<'a>)) {
    let body = &body[..body.len().min(prefix)];
    let text = !body[..body.len().min(TEXT_PROBE)].contains(&0);
    let mut at = 0;
    while at < body.len() {
        if !printable(body[at]) {
            at += 1;
            continue;
        }
        let end = (body[at..].iter())
            .position(|b| !printable(*b))
            .map_or(body.len(), |n| at + n);
        let run = &body[at..end];
        let path_shaped = || (run.iter()).all(|b| name_byte(*b) || b"/\\:.#".contains(b));
        let plain = text || (body.get(end) == Some(&0) && path_shaped());
        tokens(run, plain, each);
        at = end;
    }
}

/// The names of one printable run; the plain ones only where `plain`.
fn tokens<'a>(run: &'a [u8], plain: bool, each: &mut impl FnMut(Harvested<'a>)) {
    let mut at = 0;
    while at < run.len() {
        if !name_byte(run[at]) {
            at += 1;
            continue;
        }
        let end = (run[at..].iter())
            .position(|b| !name_byte(*b))
            .map_or(run.len(), |n| at + n);
        let token = &run[at..end];
        let kind = match &run[end..] {
            [b'.', b'#', a, b, rest @ ..]
                if kind_byte(*a)
                    && kind_byte(*b)
                    && rest.first().is_none_or(|c| !name_byte(*c)) =>
            {
                Some([a.to_ascii_lowercase(), b.to_ascii_lowercase()])
            }
            _ => None,
        };
        let kept = match kind {
            Some(_) => true,
            None => plain && token.len() >= MIN_PLAIN && token.iter().any(u8::is_ascii_alphabetic),
        };
        if let (true, true, Ok(name)) = (kept, token.len() <= MAX_NAME, std::str::from_utf8(token))
        {
            each(Harvested { name, kind });
        }
        at = end + if kind.is_some() { 4 } else { 0 };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    /// Lower-cased, as both hashes fold it.
    text: Box<str>,
    kind: Option<[u8; 2]>,
}

/// How many names a vocabulary holds, and of what kinds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub names: usize,
    /// The names a body spelled with a `.#xx` ending, by that ending.
    pub kinds: BTreeMap<[u8; 2], usize>,
    pub plain: usize,
    /// The distinct 32-bit hashes of the names.
    pub hashes32: usize,
    /// The 32-bit hashes two names or more share, which name nothing alone.
    pub shared32: usize,
}

/// Names by their 64-bit and 32-bit hashes.
// ponytail: every name is held in memory, a few hundred megabytes for some millions of names; an
// index on disk if a full install's plain names outgrow that.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vocabulary {
    names: HashMap<u64, Name>,
    /// The first name of each 32-bit hash, by its 64-bit word.
    by32: HashMap<u32, u64>,
    shared32: HashSet<u32>,
}

impl Vocabulary {
    /// Holds `name`, lower-cased, where it is 1 to [`MAX_NAME`] printable bytes with no space;
    /// whether it is new. A name held as plain takes the `kind` a later body spells it with.
    pub fn add(&mut self, name: &str, kind: Option<[u8; 2]>) -> bool {
        if name.is_empty() || name.len() > MAX_NAME || !name.bytes().all(|b| b.is_ascii_graphic()) {
            return false;
        }
        match self.names.entry(name_hash64(name.as_bytes())) {
            Entry::Occupied(mut held) => {
                let held = held.get_mut();
                held.kind = held.kind.or(kind);
                false
            }
            Entry::Vacant(slot) => {
                let text: Box<str> = name.to_ascii_lowercase().into();
                let (word, hash) = (*slot.key(), name_hash32(text.as_bytes()));
                slot.insert(Name { text, kind });
                match self.by32.entry(hash) {
                    Entry::Vacant(first) => {
                        first.insert(word);
                    }
                    Entry::Occupied(_) => {
                        self.shared32.insert(hash);
                    }
                }
                true
            }
        }
    }

    /// Holds each name the first `prefix` bytes of `body` spell; how many were new.
    pub fn harvest(&mut self, body: &[u8], prefix: usize) -> usize {
        let mut new = 0;
        harvest(body, prefix, &mut |found| {
            new += usize::from(self.add(found.name, found.kind));
        });
        new
    }

    /// Holds every name of `other`, as another archive's harvest.
    pub fn merge(&mut self, other: &Self) {
        for name in other.names.values() {
            self.add(&name.text, name.kind);
        }
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The name whose 64-bit hash is `word`.
    pub fn name64(&self, word: u64) -> Option<&str> {
        self.names.get(&word).map(|n| &*n.text)
    }

    /// The name whose 32-bit hash is `hash`; `None` where two held names share it.
    pub fn name32(&self, hash: u32) -> Option<&str> {
        if self.shared32.contains(&hash) {
            return None;
        }
        self.name64(*self.by32.get(&hash)?)
    }

    /// The name a 32-bit `hash` and the 64-bit `word` stated beside it both name; a zero `word`
    /// states none, and the 32-bit hash then names alone.
    pub fn confirmed(&self, hash: u32, word: u64) -> Option<&str> {
        match word {
            0 => self.name32(hash),
            _ => (self.name64(word)).filter(|name| name_hash32(name.as_bytes()) == hash),
        }
    }

    /// The names a body spelled with a `.#xx` ending, without it.
    pub fn resource_names(&self) -> impl Iterator<Item = &str> {
        (self.names.values())
            .filter(|n| n.kind.is_some())
            .map(|n| &*n.text)
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts {
            names: self.names.len(),
            hashes32: self.by32.len(),
            shared32: self.shared32.len(),
            ..Counts::default()
        };
        for name in self.names.values() {
            match name.kind {
                Some(kind) => *counts.kinds.entry(kind).or_default() += 1,
                None => counts.plain += 1,
            }
        }
        counts
    }

    /// Writes the names as text, a line each: the kind or `--`, a space and the name.
    pub fn write(&self, mut out: impl Write) -> io::Result<()> {
        writeln!(out, "{VOCABULARY_HEAD}")?;
        for name in self.names.values() {
            let kind = name.kind.unwrap_or(*b"--");
            let kind = [char::from(kind[0]), char::from(kind[1])];
            writeln!(out, "{}{} {}", kind[0], kind[1], name.text)?;
        }
        Ok(())
    }

    /// Reads what [`Self::write`] wrote; a line that is no name is passed over, and every hash
    /// is computed again from its name.
    pub fn read(mut from: impl BufRead) -> io::Result<Self> {
        let mut line = Vec::new();
        if !next_line(&mut from, &mut line)? || line != VOCABULARY_HEAD.as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a kept GTA VI vocabulary",
            ));
        }
        let mut held = Self::default();
        while next_line(&mut from, &mut line)? {
            let Some((kind, name)) =
                (std::str::from_utf8(&line).ok()).and_then(|l| l.split_once(' '))
            else {
                continue;
            };
            let kind = match kind.as_bytes() {
                b"--" => None,
                [a, b] if kind_byte(*a) && kind_byte(*b) => Some([*a, *b]),
                _ => continue,
            };
            held.add(name, kind);
        }
        Ok(held)
    }

    /// The vocabulary kept at `file`; an empty one where none is kept or it does not read.
    pub fn load(file: &Path) -> Self {
        let read = |file: &Path| Self::read(BufReader::new(fs::File::open(file)?));
        read(file).unwrap_or_default()
    }

    /// Keeps the vocabulary at `file`, whose folder is made.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        keep(file, |out| self.write(out))
    }
}

/// The next line of `from` without its line end, cut at [`MAX_LINE`] bytes; `false` at the end.
fn next_line(from: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<bool> {
    line.clear();
    let read = Read::take(&mut *from, MAX_LINE).read_until(b'\n', line)?;
    while line.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
        line.pop();
    }
    Ok(read > 0)
}

/// Writes `file` whole beside itself, then moves it into place.
fn keep(
    file: &Path,
    write: impl FnOnce(&mut BufWriter<fs::File>) -> io::Result<()>,
) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let beside = file.with_extension("tmp");
    let mut out = BufWriter::new(fs::File::create(&beside)?);
    write(&mut out)?;
    out.flush()?;
    drop(out);
    fs::rename(beside, file)
}

/// The names the bodies of one archive state of themselves, by the offset each starts at.
pub type Labels = BTreeMap<u64, String>;

/// The folder the names are kept in, under the configuration directory `root`.
fn folder(root: &Path) -> PathBuf {
    root.join("gta6_names")
}

/// Where the vocabulary of every archive harvested is kept, under the configuration directory `root`.
pub fn vocabulary_file(root: &Path) -> PathBuf {
    folder(root).join("vocabulary.txt")
}

/// Where the labels of `archive` are kept, under the configuration directory `root`.
pub fn labels_file(root: &Path, archive: &Path) -> PathBuf {
    let full = fs::canonicalize(archive).unwrap_or_else(|_| archive.to_path_buf());
    let word = name_hash64(full.to_string_lossy().as_bytes());
    folder(root).join(format!("{word:016x}.labels"))
}

/// Writes the labels of an archive of `archive_len` bytes as text, a line each: the offset in
/// hexadecimal, a space and the name.
pub fn write_labels(labels: &Labels, archive_len: u64, mut out: impl Write) -> io::Result<()> {
    writeln!(out, "{LABELS_HEAD} {archive_len:X}")?;
    for (offset, name) in labels {
        writeln!(out, "{offset:X} {name}")?;
    }
    Ok(())
}

/// Reads what [`write_labels`] wrote of an archive that is still `archive_len` bytes; none of an
/// archive whose length changed.
pub fn read_labels(mut from: impl BufRead, archive_len: u64) -> io::Result<Labels> {
    let mut line = Vec::new();
    let mut labels = Labels::new();
    let head = format!("{LABELS_HEAD} {archive_len:X}");
    if !next_line(&mut from, &mut line)? || line != head.as_bytes() {
        return Ok(labels);
    }
    while next_line(&mut from, &mut line)? {
        let Some((offset, name)) =
            (std::str::from_utf8(&line).ok()).and_then(|l| l.split_once(' '))
        else {
            continue;
        };
        if let (Ok(offset), true) = (u64::from_str_radix(offset, 16), stated(name)) {
            labels.insert(offset, name.to_string());
        }
    }
    Ok(labels)
}

/// Keeps the labels of `archive` under the configuration directory `root`; the file written.
pub fn save_labels(root: &Path, archive: &Path, labels: &Labels) -> io::Result<PathBuf> {
    let len = fs::metadata(archive)?.len();
    let file = labels_file(root, archive);
    keep(&file, |out| write_labels(labels, len, out))?;
    Ok(file)
}

/// The labels kept of `archive` as it is now; none where none are kept or they do not read.
pub fn load_labels(root: &Path, archive: &Path) -> Labels {
    let read = || {
        let len = fs::metadata(archive)?.len();
        let file = fs::File::open(labels_file(root, archive))?;
        read_labels(BufReader::new(file), len)
    };
    read().unwrap_or_default()
}

/// Whether `name` reads as a name a body states of itself: a name the vocabulary would hold, with
/// or without a `.#xx` ending.
fn stated(name: &str) -> bool {
    let stem = split_kind(name).0;
    !stem.is_empty() && stem.len() <= MAX_NAME && name.bytes().all(|b| b.is_ascii_graphic())
}

/// Whether a harvest reads the body `find`: a resource, a stored RSC8, text or metadata. The
/// other zlib runs are vertex and index data, and audio, movies and the nested archives' own
/// names spell none.
pub fn harvests(find: &Rpf9ScanFind) -> bool {
    match find.kind {
        Rpf9BodyKind::Zlib(run) => run.resource.is_some() || run.content != Rpf9Content::Other,
        Rpf9BodyKind::StoredRsc8 { .. } => true,
        _ => false,
    }
}

/// Holds in `vocabulary` each name the first `prefix` bytes of the body `find` spell, `bytes`
/// being what a read of it returned; the name the body states of itself, where it is a drawable
/// or a fragment whose root states one.
pub fn harvest_body(
    vocabulary: &mut Vocabulary,
    find: &Rpf9ScanFind,
    bytes: &[u8],
    prefix: usize,
) -> Option<String> {
    if !harvests(find) {
        return None;
    }
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        resource: Some(resource),
        signature,
        ..
    }) = find.kind
    else {
        vocabulary.harvest(bytes, prefix);
        return None;
    };
    // The physical section is vertex and index data.
    let virtual_section = resource.sections(bytes).0;
    vocabulary.harvest(virtual_section, prefix);
    let role = signature.and_then(|s| s.role())?;
    if !matches!(role, Rpf9Role::Drawable | Rpf9Role::DrawableWithBound) {
        return None;
    }
    let name = super::headerless::stated_model_name(virtual_section).ok()?;
    if !stated(&name) {
        return None;
    }
    // Its own name is held wherever the root points, past the prefix too.
    let (stem, kind) = split_kind(&name);
    vocabulary.add(stem, kind);
    Some(name)
}

/// What a harvest of one archive gave.
#[derive(Debug, Default)]
pub struct ArchiveHarvest {
    pub vocabulary: Vocabulary,
    /// The name each drawable and fragment states of itself, by its offset.
    pub labels: Labels,
    /// The bodies read for names.
    pub bodies: usize,
    /// Whether it was stopped before the archive's end; what it read until then is given.
    pub stopped: bool,
}

/// What the scan of one range of an archive gave.
#[derive(Default)]
struct Ranged {
    index: u64,
    bodies: usize,
    labels: Vec<(u64, String)>,
    /// Where the scan ended: past the range's end where its last body is.
    end: u64,
}

/// Harvests the first `keep` inflated bytes of every body of the archive at `path` a harvest
/// reads, on up to `workers` threads; `stop` is asked between bodies, and `reached` is told the
/// archive's bytes done and its length as each range of it ends.
// Each body is inflated once, by the scan that finds it, and no worker holds more of one than
// `keep` bytes beside the scan's own window.
pub fn harvest_archive(
    path: &Path,
    keep: usize,
    workers: usize,
    stop: &(dyn Fn() -> bool + Sync),
    reached: &(dyn Fn(u64, u64) + Sync),
) -> io::Result<ArchiveHarvest> {
    harvest_ranges(path, keep, HARVEST_RANGE, workers, stop, reached)
}

/// As [`harvest_archive`], a scan to each `range` bytes of the archive.
// ponytail: a scan that starts inside a body, as a range's does where the range before it ends in
// one, reads the body's later streams as runs of their own and may take the start of the body
// after for one more. Their bytes are the archive's, so the names stand, and a label found before
// that body's end is dropped; one worker scans the ranges in order, each from where the last
// ended, and misses none. Cut the ranges at entry starts if a harvest on several threads must
// equal that one.
fn harvest_ranges(
    path: &Path,
    keep: usize,
    range: u64,
    workers: usize,
    stop: &(dyn Fn() -> bool + Sync),
    reached: &(dyn Fn(u64, u64) + Sync),
) -> io::Result<ArchiveHarvest> {
    let len = fs::metadata(path)?.len();
    let ranges = len.div_ceil(range.max(1));
    let workers = (workers.max(1) as u64).min(ranges.max(1));
    let (next, done) = (AtomicU64::new(0), AtomicU64::new(0));
    let work = || -> io::Result<(Vocabulary, Vec<Ranged>)> {
        let file = fs::File::open(path)?;
        let (mut vocabulary, mut out) = (Vocabulary::default(), Vec::new());
        let mut carried = None;
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            if index >= ranges || stop() {
                break;
            }
            let start = index.saturating_mul(range);
            let end = start.saturating_add(range).min(len);
            let mut ranged = Ranged {
                index,
                end: start,
                ..Ranged::default()
            };
            // One worker takes the ranges in order, so each scan goes on from the last.
            let (from, carry) = match carried.filter(|_| workers == 1) {
                Some((position, carry)) => (start.max(position), Some(carry)),
                None => (start, None),
            };
            if from < end {
                let scan = Rpf9Scan::new(&file, from..end).map_err(io::Error::other)?;
                let mut scan = match carry {
                    Some(carry) => scan.carrying(carry),
                    None => scan,
                }
                .keeping(keep);
                while let Some(find) = scan.next() {
                    // A scan that fails says so once and ends: the rest of its range is not read.
                    let (Ok(find), false) = (find, stop()) else {
                        break;
                    };
                    let bytes = scan.kept(&find);
                    if bytes.is_empty() || !harvests(&find) {
                        continue;
                    }
                    ranged.bodies += 1;
                    if let Some(name) = harvest_body(&mut vocabulary, &find, bytes, keep) {
                        ranged.labels.push((find.offset, name));
                    }
                }
                ranged.end = scan.position();
                carried = Some((scan.position(), scan.carry()));
            }
            out.push(ranged);
            let step = end - start;
            reached(done.fetch_add(step, Ordering::Relaxed) + step, len);
        }
        Ok((vocabulary, out))
    };
    let gave: Vec<io::Result<(Vocabulary, Vec<Ranged>)>> = match workers {
        // No thread is started for one, as where none can be.
        1 => vec![work()],
        _ => std::thread::scope(|scope| {
            let started: Vec<_> = (0..workers).map(|_| scope.spawn(work)).collect();
            let joined = started.into_iter().map(|worker| {
                let ended = worker.join();
                ended.unwrap_or_else(|_| Err(io::Error::other("a harvest worker panicked")))
            });
            joined.collect()
        }),
    };
    let mut harvest = ArchiveHarvest {
        stopped: stop(),
        ..ArchiveHarvest::default()
    };
    let mut ranged = Vec::new();
    for gave in gave {
        let (vocabulary, out) = gave?;
        match harvest.vocabulary.is_empty() {
            true => harvest.vocabulary = vocabulary,
            false => harvest.vocabulary.merge(&vocabulary),
        }
        ranged.extend(out);
    }
    ranged.sort_by_key(|r| r.index);
    // What a scan found before the end of a body an earlier range's scan read is no body.
    let mut read_to = 0;
    for r in ranged {
        harvest.bodies += r.bodies;
        let labels = r.labels.into_iter().filter(|(at, _)| *at >= read_to);
        harvest.labels.extend(labels);
        read_to = read_to.max(r.end);
    }
    Ok(harvest)
}

#[cfg(feature = "meta")]
impl super::map::Entity {
    /// The archetype's name, where `vocabulary` holds one both of its hashes name.
    pub fn archetype_in<'a>(&self, vocabulary: &'a Vocabulary) -> Option<&'a str> {
        vocabulary.confirmed(self.archetype_name, self.archetype_word)
    }
}

#[cfg(feature = "meta")]
impl super::map::Archetype {
    /// The archetype's name, where `vocabulary` holds one both of its hashes name.
    pub fn name_in<'a>(&self, vocabulary: &'a Vocabulary) -> Option<&'a str> {
        vocabulary.confirmed(self.name, self.name_word)
    }
}

#[cfg(feature = "meta")]
impl super::map::MapData {
    /// The map's name, where `vocabulary` holds one both of its hashes name.
    pub fn name_in<'a>(&self, vocabulary: &'a Vocabulary) -> Option<&'a str> {
        vocabulary.confirmed(self.name, self.name_word)
    }
}

#[cfg(feature = "meta")]
impl super::map::links::InteriorEntity {
    /// The archetype's name, where `vocabulary` holds one both of its hashes name.
    pub fn archetype_in<'a>(&self, vocabulary: &'a Vocabulary) -> Option<&'a str> {
        vocabulary.confirmed(self.archetype_name, self.archetype_word)
    }
}

#[cfg(test)]
#[path = "names_tests.rs"]
mod tests;
