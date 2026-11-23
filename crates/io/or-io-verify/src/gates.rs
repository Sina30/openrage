//! What every declared format's corpus suite shares: opening a staged file,
//! filing a record tree by address, the recorded testimony checked through the
//! declarations by offset, pairing with the private description by address,
//! and the gates [`declared_gates!`](crate::declared_gates) writes as tests.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::read::MultiGzDecoder;
use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::decl::Aligned;
use or_io::desc::{Image, Pack, ScalarValue, Value};
use or_io::gta5::formats::{self, Format};
use or_io::Generation;
use or_test_support::ledger::{self, SkipReason};
use or_test_support::Corpus;
use rayon::prelude::*;

use crate::census;
use crate::diff;
pub use crate::testimony::FieldMoved;
use crate::testimony::{
    check_file, sha1_hex, DeclaredReading, FileRecord, Key, RecordReader, KEY_FILE,
};

pub fn format(ext: &str) -> &'static Format {
    formats::format(ext).expect("declared")
}

pub fn pack(ext: &str, target: Generation) -> Arc<Pack> {
    format(ext)
        .description(target)
        .expect("the built-in description")
}

pub fn corpus_root() -> Option<PathBuf> {
    let origin = or_test_support::shared_corpus_origin(env!("CARGO_MANIFEST_DIR"));
    origin.found().then(|| origin.path().to_path_buf())
}

/// The staged files of `ext` whose RSC7 version is `target`'s, or every
/// declared version's with `None`, wherever each was staged.
///
/// ⚠ A file at a version the format does not declare is left out and named.
pub fn files(ext: &str, target: Option<Generation>, what: &str) -> Option<Vec<PathBuf>> {
    let Some(root) = corpus_root() else {
        ledger::record(SkipReason::Corpus, what, "no corpus is staged");
        return None;
    };
    let fmt = format(ext);
    let mut out = Vec::new();
    for path in Corpus::with_root(root, ext)
        .all_targets()
        .find_or_skip(what)?
    {
        // The header alone names the version; inflating the file here doubled
        // what every caller then inflates again.
        let mut header = [0u8; Rsc7::HEADER_LEN];
        File::open(&path)
            .and_then(|mut f| f.read_exact(&mut header))
            .expect("a corpus file reads");
        let version = Rsc7::peek(&header).expect("an RSC7 resource").version();
        match fmt.target_of_version(version) {
            Ok(t) if target.is_none_or(|want| want == t) => out.push(path),
            Ok(_) => {}
            Err(why) => println!("left out {}: {why}", name(&path)),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A file's decompressed pages, held aligned so views can be taken in place.
pub struct Opened {
    pub bytes: Aligned,
    pub virtual_size: usize,
}

impl Opened {
    pub fn of(file: &[u8]) -> Self {
        let opened = Rsc7::open(file).expect("an RSC7 resource");
        Self {
            bytes: Aligned::from_bytes(&opened.data),
            virtual_size: opened.virtual_size as usize,
        }
    }

    pub fn read(path: &Path) -> Self {
        Self::of(&std::fs::read(path).expect("a corpus file reads"))
    }

    pub fn image(&self) -> Image<'_> {
        Image::new(self.bytes.bytes(), self.virtual_size)
    }
}

pub fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every struct a tree or a walk holds, by `(offset, type)`, and every scalar
/// run by the offset of its first element.
#[derive(Default)]
pub struct Filed {
    pub structs: BTreeMap<(usize, String), Vec<u8>>,
    pub runs: BTreeMap<usize, Vec<u8>>,
}

/// File a record tree by address.
///
/// ⚠ An integer run is filed as `u32`s: the only integer runs the formats
/// declared so far hold are their dictionaries' hashes.
pub fn file_tree(image: Image<'_>, value: &Value, at: Option<usize>, out: &mut Filed) {
    match value {
        Value::Ptr {
            source,
            target: Some(target),
        } => file_tree(image, target, image.resolve(*source).ok(), out),
        Value::Struct(record) => {
            if let Some(at) = at {
                out.structs.insert(
                    (at, record.type_name().to_string()),
                    record.bytes().to_vec(),
                );
            }
            for (offset, child) in record.children() {
                file_tree(image, child, at.map(|at| at + *offset as usize), out);
            }
        }
        Value::Records(run) => {
            if let Some(at) = at {
                for (i, element) in run.iter().enumerate() {
                    out.structs.insert(
                        (at + i * run.stride(), run.type_name().to_string()),
                        element.bytes().to_vec(),
                    );
                }
            }
        }
        Value::Array { elements, .. } => {
            let scalars: Option<Vec<u8>> = elements
                .iter()
                .map(|e| match e {
                    Value::Scalar(ScalarValue::F32(f)) => Some(f.to_le_bytes().to_vec()),
                    Value::Scalar(ScalarValue::Int(i)) => Some((*i as u32).to_le_bytes().to_vec()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(|v| v.concat());
            match (scalars, at) {
                (Some(bytes), Some(at)) if !elements.is_empty() => {
                    out.runs.insert(at, bytes);
                }
                _ => {
                    let mut next = at;
                    for element in elements {
                        file_tree(image, element, next, out);
                        next = next
                            .zip(element.record())
                            .map(|(at, r)| at + r.bytes().len());
                    }
                }
            }
        }
        Value::Bytes(bytes) => {
            if let Some(at) = at {
                out.runs.insert(at, bytes.clone());
            }
        }
        _ => {}
    }
}

/// Where a run the typed API returned sits in its image.
pub fn offset_of_run<T>(image: Image<'_>, run: &[T]) -> Option<usize> {
    run.first()
        .map(|first| first as *const T as usize - image.bytes().as_ptr() as usize)
}

/// The decompressed pages and the header: what a fixed point is judged on,
/// whichever deflate implementation packed the file.
pub fn payload(file: &[u8]) -> Vec<u8> {
    let opened = Rsc7::open(file).expect("a saved file opens");
    let mut out = opened.seal.header_bytes();
    out.extend_from_slice(&opened.data);
    out
}

/// What opening files owned and saving them found.
#[derive(Default)]
pub struct OwnedSaves {
    pub files: usize,
    pub same: usize,
    /// Files both routes refuse, for the same reason.
    pub refused: Vec<String>,
    pub failures: Vec<String>,
}

impl OwnedSaves {
    fn merge(mut self, other: Self) -> Self {
        self.files += other.files;
        self.same += other.same;
        self.refused.extend(other.refused);
        self.failures.extend(other.failures);
        self
    }
}

/// Every file opened as `R`, saved, and compared with the save of its tree:
/// the owned form must write exactly what the tree writes, and reopen equal.
pub fn owned_saves_as_tree<R>(ext: &str, target: Generation, paths: &[PathBuf]) -> OwnedSaves
where
    R: or_io::decl::Resource + PartialEq + std::fmt::Debug,
    or_io::decl::Raw<R>: or_io::decl::Record,
{
    let fmt = format(ext);
    let one = |path: &PathBuf| {
        let mut out = OwnedSaves::default();
        out.files += 1;
        let file = std::fs::read(path).expect("a corpus file reads");
        let owned = match R::from_bytes(&file) {
            Ok(o) => o,
            Err(e) => {
                // Refused at open is refused: only if the tree's save refuses too.
                let tree_save = fmt
                    .read(target, Opened::of(&file).image())
                    .and_then(|t| fmt.save(target, &t));
                match tree_save {
                    Err(t) => out
                        .refused
                        .push(format!("{}: {t} (opened owned: {e})", name(path))),
                    Ok(_) => out
                        .failures
                        .push(format!("{}: opening owned: {e}", name(path))),
                }
                return out;
            }
        };
        let opened = Opened::of(&file);
        let from_tree = match fmt
            .read(target, opened.image())
            .and_then(|t| fmt.save(target, &t))
        {
            Ok(b) => b,
            Err(e) => {
                match owned.to_bytes() {
                    Err(o) if o.to_string() == e.to_string() => {
                        out.refused.push(format!("{}: {e}", name(path)))
                    }
                    other => out.failures.push(format!(
                        "{}: the tree refuses ({e}) and the owned save gives {other:?}",
                        name(path)
                    )),
                }
                return out;
            }
        };
        let from_owned = match owned.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                out.failures
                    .push(format!("{}: saving owned: {e}", name(path)));
                return out;
            }
        };
        let owned_payload = payload(&from_owned);
        if owned_payload != payload(&from_tree) {
            out.failures.push(format!(
                "{}: the owned save differs from the tree's",
                name(path)
            ));
            return out;
        }
        // The canonical form nulls runtime pointers, so the owned value is
        // held to its own save rather than to the file as shipped.
        let reopened = R::from_bytes(&from_owned);
        let resaved = reopened.as_ref().ok().map(|r| r.to_bytes());
        // ⚠ Judged on bytes, not `PartialEq`: a shipped float can be a NaN,
        // which is unequal to itself however faithfully it was carried.
        match (reopened, resaved) {
            (Ok(_), Some(Ok(bytes))) => {
                if payload(&bytes) == owned_payload {
                    out.same += 1
                } else {
                    out.failures.push(format!(
                        "{}: the owned value is not a fixed point",
                        name(path)
                    ))
                }
            }
            (Err(e), _) => out.failures.push(format!("{}: reopening: {e}", name(path))),
            (_, Some(Err(e))) => out
                .failures
                .push(format!("{}: saving again: {e}", name(path))),
            (_, None) => unreachable!(),
        }
        out
    };
    // Each file saves on its own, so the cores share the corpus; the fragments fold in file order.
    paths
        .par_iter()
        .map(one)
        .collect::<Vec<_>>()
        .into_iter()
        .fold(OwnedSaves::default(), OwnedSaves::merge)
}

/// What the fixed point found over one target's files.
#[derive(Default)]
pub struct FixedPoint {
    pub files: usize,
    pub fixed: usize,
    pub refused: Vec<String>,
    pub failures: Vec<String>,
}

impl FixedPoint {
    fn merge(mut self, other: Self) -> Self {
        self.files += other.files;
        self.fixed += other.fixed;
        self.refused.extend(other.refused);
        self.failures.extend(other.failures);
        self
    }
}

/// Every file saved, read back, compared, and saved again. A first save
/// refused for a reason in `known` is counted as refused, and any other fails.
pub fn fixed_point(
    ext: &str,
    target: Generation,
    paths: &[PathBuf],
    known: &[(&str, &str)],
) -> FixedPoint {
    let fmt = format(ext);
    let pack = pack(ext, target);
    let one = |path: &PathBuf| {
        let mut out = FixedPoint::default();
        let what = name(path);
        out.files += 1;
        let original = Opened::read(path);
        let v1 = match fmt.read(target, original.image()) {
            Ok(v) => v,
            Err(e) => {
                out.failures.push(format!("{what}: the read: {e}"));
                return out;
            }
        };
        let b1 = match fmt.save(target, &v1) {
            Ok(b) => b,
            Err(e) if known.iter().any(|(file, _)| *file == what) => {
                out.refused.push(format!("{what}: {e}"));
                return out;
            }
            Err(e) => {
                out.failures.push(format!("{what}: the first save: {e}"));
                return out;
            }
        };
        if fmt.save(target, &v1).ok().as_deref() != Some(&b1[..]) {
            out.failures
                .push(format!("{what}: two saves of one tree differ"));
        }
        let saved = Opened::of(&b1);
        let v2 = match fmt.read(target, saved.image()) {
            Ok(v) => v,
            Err(e) => {
                out.failures
                    .push(format!("{what}: the save does not read back: {e}"));
                return out;
            }
        };
        if let Err(e) = or_io::write::compare::same_content_using(
            &pack,
            &v1,
            &v2,
            &or_io::desc::value::ElementTypes::new(),
            &fmt.covered(),
        ) {
            out.failures
                .push(format!("{what}: the first save changed content: {e}"));
        }
        // `payload(&b1)` from the pages `saved` already inflated.
        let mut b1_payload = Rsc7::peek(&b1).expect("a saved file opens").header_bytes();
        b1_payload.extend_from_slice(saved.bytes.bytes());
        drop(saved);
        match fmt.save(target, &v2) {
            Ok(b2) if payload(&b2) == b1_payload => out.fixed += 1,
            Ok(_) => out
                .failures
                .push(format!("{what}: the second save is not the first")),
            Err(e) => out.failures.push(format!("{what}: the second save: {e}")),
        }
        out
    };
    paths
        .par_iter()
        .map(one)
        .collect::<Vec<_>>()
        .into_iter()
        .fold(FixedPoint::default(), FixedPoint::merge)
}

const SUFFIX: &str = ".testimony.gz";

/// One recorded stream of the corpus.
pub struct Stream {
    pub label: String,
    pub target: Generation,
    pub path: PathBuf,
}

/// The testimony directory, beside the corpus.
fn testimony_root(corpus: &Path) -> Option<PathBuf> {
    Some(corpus.parent()?.join("testimony"))
}

/// The frozen key beside the testimony, if one was written.
///
/// ⚠ A key that is there and does not parse panics rather than reads as absent.
pub fn frozen_key() -> Option<Key> {
    let path = testimony_root(&corpus_root()?)?.join(KEY_FILE);
    let file = File::open(&path).ok()?;
    Some(Key::read(BufReader::new(file)).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// Every recorded stream of `ext` under the corpus's testimony directory.
pub fn streams(ext: &str, what: &str) -> Option<(PathBuf, Vec<Stream>)> {
    let Some(corpus) = corpus_root() else {
        ledger::record(SkipReason::Corpus, what, "no corpus is staged");
        return None;
    };
    let root = testimony_root(&corpus)?;
    let mut out = Vec::new();
    for dir in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let label = dir.file_name().to_string_lossy().into_owned();
        if !label.starts_with("corpus") {
            continue;
        }
        for entry in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some((e, target)) = file.strip_suffix(SUFFIX).and_then(|s| s.split_once('.'))
            else {
                continue;
            };
            let Some(target) = Generation::from_slug(target) else {
                continue;
            };
            if e == ext {
                out.push(Stream {
                    label: format!("{label}/{file}"),
                    target,
                    path: entry.path(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    if out.is_empty() {
        ledger::record(
            SkipReason::Corpus,
            what,
            format_args!("no recorded .{ext} testimony under {}", root.display()),
        );
        return None;
    }
    Some((corpus, out))
}

pub fn open_stream(path: &Path) -> RecordReader<BufReader<MultiGzDecoder<File>>> {
    let file = File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    RecordReader::new(BufReader::new(MultiGzDecoder::new(file)))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Counts from checking every recorded field of every corpus file of one stream.
#[derive(Default, Debug)]
pub struct Tally {
    pub files: usize,
    pub fields: usize,
    pub agreed: usize,
    pub unreached: usize,
    pub undescribed: usize,
    pub unanswered: usize,
    /// Recorded instances a [`Relocated`] explains, with the fields they held.
    pub relocated: usize,
    /// Pack-witnessed fields a [`FieldMoved`] answered, each still equal to the file.
    pub corrected: usize,
    /// Recorded fields in the zero padding after a record, each still equal to the file.
    pub padding: usize,
    pub differ: Vec<String>,
    /// Recorded sites no declared field tiles, by count; printed, never committed.
    pub unanswered_sites: BTreeMap<String, usize>,
}

impl Tally {
    fn merge(&mut self, other: Self) {
        self.files += other.files;
        self.fields += other.fields;
        self.agreed += other.agreed;
        self.unreached += other.unreached;
        self.undescribed += other.undescribed;
        self.unanswered += other.unanswered;
        self.relocated += other.relocated;
        self.corrected += other.corrected;
        self.padding += other.padding;
        self.differ.extend(other.differ);
        for (site, n) in other.unanswered_sites {
            *self.unanswered_sites.entry(site).or_default() += n;
        }
    }
}

/// Recorded instances a record placed where a declaration since corrected:
/// in streams whose label starts with `record`, an instance `delta` bytes into
/// a declared `inside` is excused, and nowhere else.
pub struct Relocated {
    pub record: &'static str,
    pub inside: &'static str,
    pub delta: usize,
    pub because: &'static str,
}

/// Each corpus record of a stream: its file's bytes when they still match.
fn recorded_bytes(corpus: &Path, key: &str, sha1: &str) -> Option<Vec<u8>> {
    key.strip_prefix("corpus/")
        .and_then(|k| std::fs::read(corpus.join(k)).ok())
        .filter(|b| sha1_hex(b) == sha1)
}

/// Recorded instances a record's reader took from past the end of a run its
/// layout later corrected: in streams whose label starts with `record`, an
/// instance of exactly `bytes` recorded bytes starting `delta` bytes into a
/// declared `inside` is set aside before the check, and counted as relocated.
pub struct Overread {
    pub record: &'static str,
    pub inside: &'static str,
    pub delta: usize,
    pub bytes: usize,
    pub because: &'static str,
}

/// Every recorded field of `ext` checked through `description(target)` by offset.
pub fn check_against(
    ext: &str,
    description: &dyn Fn(Generation) -> Option<Pack>,
    relocated: &[Relocated],
    moved: &'static [FieldMoved],
    what: &str,
) -> Option<Vec<(String, Tally)>> {
    check_against_with(ext, description, relocated, &[], moved, what)
}

/// [`check_against`], setting aside what `overread` names first.
pub fn check_against_with(
    ext: &str,
    description: &dyn Fn(Generation) -> Option<Pack>,
    relocated: &[Relocated],
    overread: &[Overread],
    moved: &'static [FieldMoved],
    what: &str,
) -> Option<Vec<(String, Tally)>> {
    let key = frozen_key();
    check(
        ext,
        description,
        key.as_ref(),
        relocated,
        overread,
        moved,
        what,
        false,
    )
}

/// [`check_against`], each stream read only until a record in it disagrees:
/// a falsifier asks whether a stream notices, not how much.
pub fn caught_against(
    ext: &str,
    description: &dyn Fn(Generation) -> Option<Pack>,
    relocated: &[Relocated],
    moved: &'static [FieldMoved],
    what: &str,
) -> Option<Vec<(String, Tally)>> {
    let key = frozen_key();
    check(
        ext,
        description,
        key.as_ref(),
        relocated,
        &[],
        moved,
        what,
        true,
    )
}

/// [`check_against_with`], through `key` in place of the frozen key.
pub fn check_against_keyed(
    ext: &str,
    description: &dyn Fn(Generation) -> Option<Pack>,
    key: &Key,
    relocated: &[Relocated],
    overread: &[Overread],
    moved: &'static [FieldMoved],
    what: &str,
) -> Option<Vec<(String, Tally)>> {
    check(
        ext,
        description,
        Some(key),
        relocated,
        overread,
        moved,
        what,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn check(
    ext: &str,
    description: &dyn Fn(Generation) -> Option<Pack>,
    key: Option<&Key>,
    relocated: &[Relocated],
    overread: &[Overread],
    moved: &'static [FieldMoved],
    what: &str,
    until_caught: bool,
) -> Option<Vec<(String, Tally)>> {
    let (corpus, streams) = streams(ext, what)?;
    let fmt = format(ext);
    let absent = Key::default();
    let mut rows = Vec::new();
    for stream in streams {
        let Some(pack) = description(stream.target) else {
            continue;
        };
        let root = or_io::decl::intern(&fmt.root_name(stream.target).expect("declared"));
        let mut reader = open_stream(&stream.path);
        let header = reader.header().clone();
        let key = match key {
            Some(key) => key,
            None if header.layouts.iter().any(|l| l.pack_only) => panic!(
                "{}: its pack_only layouts are read through the frozen key, and there is no {KEY_FILE} \
                 beside the testimony; it was taken once from the version 12 pack and cannot be retaken",
                stream.label
            ),
            None => &absent,
        };
        let one = |record: FileRecord| {
            let mut tally = Tally::default();
            let Some(bytes) = recorded_bytes(&corpus, &record.key, &record.sha1) else {
                return tally;
            };
            let opened = Opened::of(&bytes);
            let image = opened.image();
            let tree = or_io::desc::value::read(&pack, or_io::desc::TypeName(root), image, 0)
                .unwrap_or_else(|e| panic!("{}: {e}", record.key));
            let rules: Vec<&Overread> = overread
                .iter()
                .filter(|o| stream.label.starts_with(o.record))
                .collect();
            let mut set_aside = 0usize;
            let record = if rules.is_empty() {
                record
            } else {
                let first = DeclaredReading::new(&header, &record, &pack, key, image, &tree, 0);
                let filed: BTreeSet<(usize, String)> = first.filed().into_iter().collect();
                let mut kept = record.clone();
                kept.instances.retain(|i| {
                    let bytes: usize = i.fields.iter().map(|(_, b)| b.len()).sum();
                    let over = rules.iter().any(|o| {
                        bytes == o.bytes
                            && i.offset
                                .checked_sub(o.delta)
                                .is_some_and(|base| filed.contains(&(base, o.inside.to_string())))
                    });
                    if over {
                        set_aside += i.fields.len();
                    }
                    !over
                });
                kept
            };
            let reading = DeclaredReading::new(&header, &record, &pack, key, image, &tree, 0)
                .with_moved(moved);
            let verdict = check_file(&record, &reading);
            // Filed only for a stream a relocation names: it is every struct
            // the file holds, as a string each.
            let rules: Vec<&Relocated> = relocated
                .iter()
                .filter(|r| stream.label.starts_with(r.record))
                .collect();
            let excused: BTreeSet<usize> = if rules.is_empty() {
                BTreeSet::new()
            } else {
                let filed: BTreeSet<(usize, String)> = reading.filed().into_iter().collect();
                reading
                    .unreached_at()
                    .into_iter()
                    .filter(|(at, _)| {
                        rules.iter().any(|r| {
                            at.checked_sub(r.delta)
                                .is_some_and(|base| filed.contains(&(base, r.inside.to_string())))
                        })
                    })
                    .map(|(at, _)| at)
                    .collect()
            };
            let excused_fields: usize = record
                .instances
                .iter()
                .filter(|i| excused.contains(&i.offset))
                .map(|i| i.fields.len())
                .sum();
            tally.files += 1;
            tally.fields += verdict.fields;
            tally.agreed += verdict.agreed;
            tally.unreached += reading.unreached() - excused_fields;
            tally.undescribed += verdict.undescribed - excused_fields;
            tally.relocated += excused_fields + set_aside;
            tally.fields += set_aside;
            tally.corrected += verdict.corrected;
            tally.padding += verdict.padding;
            tally.unanswered += verdict.unanswered;
            for (site, n) in &verdict.unanswered_sites {
                *tally.unanswered_sites.entry(site.clone()).or_default() += n;
            }
            tally.differ.extend(
                verdict
                    .discrepancies
                    .iter()
                    .take(5)
                    .map(ToString::to_string),
            );
            tally
        };
        let mut tally = Tally::default();
        // A chunk of records is checked at once on the cores, and its tallies fold in file order.
        // A misdeclared pack stops at the first record it is caught on: a later one may not read at all.
        let chunk = if until_caught {
            1
        } else {
            rayon::current_num_threads() * 2
        };
        loop {
            let mut records = Vec::with_capacity(chunk);
            while records.len() < chunk {
                let Some(record) = reader.next_file().expect("the stream parses") else {
                    break;
                };
                records.push(record);
            }
            if records.is_empty() {
                break;
            }
            for part in records.into_par_iter().map(&one).collect::<Vec<_>>() {
                tally.merge(part);
            }
            if until_caught && (!tally.differ.is_empty() || tally.unanswered > 0) {
                break;
            }
        }
        rows.push((stream.label, tally));
    }
    Some(rows)
}

/// Print the testimony table and return every failure in it.
pub fn judge(rows: &[(String, Tally)]) -> (usize, Vec<String>) {
    println!(
        "{:<34} {:>5} {:>9} {:>9} {:>9} {:>11} {:>10} {:>9} {:>9} {:>7} {:>6}",
        "stream",
        "files",
        "fields",
        "agreed",
        "unreached",
        "undescribed",
        "unanswered",
        "relocated",
        "corrected",
        "padding",
        "differ"
    );
    let mut failures = Vec::new();
    let mut total = 0;
    for (label, t) in rows {
        println!(
            "{label:<34} {:>5} {:>9} {:>9} {:>9} {:>11} {:>10} {:>9} {:>9} {:>7} {:>6}",
            t.files,
            t.fields,
            t.agreed,
            t.unreached,
            t.undescribed,
            t.unanswered,
            t.relocated,
            t.corrected,
            t.padding,
            t.differ.len()
        );
        total += t.fields;
        failures.extend(t.differ.iter().cloned());
        let mut widest: Vec<_> = t.unanswered_sites.iter().collect();
        widest.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (site, n) in widest.iter().take(6) {
            println!("    {n:>9} unanswered at a recorded {site}");
        }
        let missing = t.unreached + t.undescribed + t.unanswered;
        if missing > 0 {
            failures.push(format!(
                "{label}: {missing} recorded field(s) no declaration reaches or describes"
            ));
        }
        if t.agreed + t.relocated + t.corrected + t.padding != t.fields {
            failures.push(format!(
                "{label}: {} of {} agreed, {} relocated, {} corrected, {} padding",
                t.agreed, t.fields, t.relocated, t.corrected, t.padding
            ));
        }
    }
    (total, failures)
}

/// A description with one field moved by `by` bytes.
pub fn moved_by(pack: &Pack, type_name: &str, field: &str, by: usize) -> Pack {
    let mut pack = pack.clone();
    let ty = pack
        .types
        .iter_mut()
        .find(|t| t.name == type_name)
        .expect("declared");
    let f = ty
        .fields
        .iter_mut()
        .find(|f| f.name == field)
        .expect("declared");
    f.offset += by;
    pack
}

/// Declared and private types that sit at one address with one size, over every recorded file.
pub fn pairs(ext: &str, target: Generation, private: &Pack) -> BTreeSet<(String, String)> {
    pairs_with(ext, target, private, &[])
}

/// [`pairs`], leaving out what `overread` sets aside.
pub fn pairs_with(
    ext: &str,
    target: Generation,
    private: &Pack,
    overread: &[Overread],
) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let Some((corpus, streams)) = streams(ext, "pairing by address") else {
        return out;
    };
    let fmt = format(ext);
    let pack = pack(ext, target);
    // Only the filing is asked, which no key changes.
    let no_key = Key::default();
    for stream in streams.iter().filter(|s| s.target == target) {
        let mut reader = open_stream(&stream.path);
        let header = reader.header().clone();
        while let Some(record) = reader.next_file().expect("the stream parses") {
            let Some(bytes) = recorded_bytes(&corpus, &record.key, &record.sha1) else {
                continue;
            };
            let opened = Opened::of(&bytes);
            let tree = fmt.read(target, opened.image()).expect("reads");
            let reading =
                DeclaredReading::new(&header, &record, &pack, &no_key, opened.image(), &tree, 0);
            let filed = reading.filed();
            let set: BTreeSet<(usize, String)> = filed.iter().cloned().collect();
            let right: Vec<(usize, String)> = record
                .instances
                .iter()
                .filter(|i| {
                    let bytes: usize = i.fields.iter().map(|(_, b)| b.len()).sum();
                    !overread.iter().any(|o| {
                        stream.label.starts_with(o.record)
                            && bytes == o.bytes
                            && i.offset
                                .checked_sub(o.delta)
                                .is_some_and(|base| set.contains(&(base, o.inside.to_string())))
                    })
                })
                .map(|i| (i.offset, i.type_name.clone()))
                .collect();
            out.extend(diff::paired_by_address(&pack, &filed, private, &right));
        }
    }
    out
}

/// One format and target's corpus gates, which [`declared_gates!`](crate::declared_gates) writes as tests.
pub struct Gates {
    pub ext: &'static str,
    pub target: Generation,
    /// The `(type, field)` each falsifier declares one byte late.
    pub late: (&'static str, &'static str),
    /// Types the tree files only as part of the record embedding them.
    pub embedded: &'static [&'static str],
    /// Files whose save is refused, by file name, each with why.
    pub refusing: &'static [(&'static str, &'static str)],
    /// The fewest types the alignment census must reach.
    pub census_types: usize,
    pub relocated: &'static [Relocated],
    pub overread: &'static [Overread],
    pub moved: &'static [FieldMoved],
}

impl Gates {
    /// A format and target's gates with nothing excused, refused or required.
    pub const fn new(
        ext: &'static str,
        target: Generation,
        late: (&'static str, &'static str),
    ) -> Self {
        Self {
            ext,
            target,
            late,
            embedded: &[],
            refusing: &[],
            census_types: 0,
            relocated: &[],
            overread: &[],
            moved: &[],
        }
    }

    fn files(&self, what: &str) -> Option<Vec<PathBuf>> {
        files(self.ext, Some(self.target), what)
    }

    fn is_refusing(&self, why: &str) -> bool {
        self.refusing
            .iter()
            .any(|(file, _)| why.starts_with(&format!("{file}:")))
    }

    /// Every file's typed read, filed by address, equals the tree's, and each reaches all the other does.
    pub fn reads_typed_as_the_tree_reads(
        &self,
        typed: &dyn Fn(Image<'_>) -> Result<Filed, String>,
    ) {
        let (ext, target) = (self.ext, self.target);
        let Some(paths) = self.files(&format!("a typed .{ext} read against the tree")) else {
            return;
        };
        let (mut structs, mut runs) = (0usize, 0usize);
        let mut failures = Vec::new();
        for path in &paths {
            let opened = Opened::read(path);
            let image = opened.image();
            let tree = match format(ext).read(target, image) {
                Ok(tree) => tree,
                Err(e) => {
                    failures.push(format!("{}: {e}", name(path)));
                    continue;
                }
            };
            let mut from_tree = Filed::default();
            file_tree(image, &tree, Some(0), &mut from_tree);
            let from_view = match typed(image) {
                Ok(v) => v,
                Err(e) => {
                    failures.push(format!("{}: {e}", name(path)));
                    continue;
                }
            };
            for (key, bytes) in &from_view.structs {
                match from_tree.structs.get(key) {
                    Some(tree_bytes) if tree_bytes == bytes => structs += 1,
                    Some(_) => failures.push(format!("{}: {key:?} reads differently", name(path))),
                    None if self.embedded.contains(&key.1.as_str()) => structs += 1,
                    None => {
                        failures.push(format!("{}: the tree does not reach {key:?}", name(path)))
                    }
                }
            }
            for key in from_tree.structs.keys() {
                if !from_view.structs.contains_key(key) {
                    failures.push(format!(
                        "{}: the typed walk does not reach {key:?}",
                        name(path)
                    ));
                }
            }
            for (at, bytes) in &from_view.runs {
                match from_tree.runs.get(at) {
                    Some(tree_bytes) if tree_bytes == bytes => runs += 1,
                    _ => failures.push(format!("{}: the run at {at:#x} differs", name(path))),
                }
            }
        }
        println!(
            "{} .{ext}: {structs} struct instances and {runs} scalar runs read typed as the tree reads them",
            paths.len()
        );
        assert!(structs > 0, "nothing was compared");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Every file opens as `R` and saves as its tree saves, and the owned save is a fixed point.
    pub fn opens_owned_and_saves_as_its_tree_saves<R>(&self)
    where
        R: or_io::decl::Resource + PartialEq + std::fmt::Debug,
        or_io::decl::Raw<R>: or_io::decl::Record,
    {
        let ext = self.ext;
        let Some(paths) = self.files(&format!("the owned .{ext} against the tree")) else {
            return;
        };
        let got = owned_saves_as_tree::<R>(ext, self.target, &paths);
        println!(
            "{} .{ext}: {} opened owned, saved byte-identical to the tree's save, and reopened equal",
            got.files, got.same
        );
        for r in &got.refused {
            println!("    refused: {r}");
            assert!(self.is_refusing(r), "an unknown refusal: {r}");
        }
        assert!(got.failures.is_empty(), "{}", got.failures.join("\n"));
        assert_eq!(got.same + got.refused.len(), paths.len());
    }

    /// Every file saves, reads back to the same content, and saves again to the same bytes.
    pub fn saves_to_a_fixed_point(&self) {
        let ext = self.ext;
        let Some(paths) = self.files(&format!("the .{ext} fixed point")) else {
            return;
        };
        let got = fixed_point(ext, self.target, &paths, self.refusing);
        println!(
            "{} .{ext}: {} value-equivalent on the first save and at a fixed point on the second",
            got.files, got.fixed
        );
        for r in &got.refused {
            println!("    refused: {r}");
        }
        assert!(got.failures.is_empty(), "{}", got.failures.join("\n"));
        assert_eq!(got.fixed + got.refused.len(), paths.len());
    }

    /// No instance of any type the root reaches sits off its alignment.
    pub fn is_aligned_for_its_view(&self) {
        let ext = self.ext;
        let Some(paths) = self.files(&format!("the .{ext} alignment census")) else {
            return;
        };
        let pack = pack(ext, self.target);
        let root = format(ext).root_name(self.target).expect("declared");
        let mut total = census::Census::default();
        for path in &paths {
            total.fold(census::take(&pack, Opened::read(path).image(), &root, 0));
        }
        for (name, tally) in &total.types {
            println!(
                "{name:<24} {:>6} instances, {} misaligned, alignment {}",
                tally.instances, tally.misaligned, tally.alignment
            );
        }
        assert!(total.unreached.is_empty(), "{:?}", total.unreached);
        let misaligned: Vec<_> = total.misaligned().collect();
        assert!(misaligned.is_empty(), "{misaligned:?}");
        assert!(
            total.types.len() >= self.census_types,
            "the census reached {:?}",
            total.types.keys()
        );
    }

    fn testimony(
        &self,
        description: &dyn Fn(Generation) -> Option<Pack>,
        what: &str,
    ) -> Option<Vec<(String, Tally)>> {
        check_against_with(
            self.ext,
            description,
            self.relocated,
            self.overread,
            self.moved,
            what,
        )
    }

    /// Every recorded field of the target's streams agrees with the declarations, with reach asserted.
    pub fn testimony_holds(&self) {
        let (ext, target) = (self.ext, self.target);
        let builtin = |t: Generation| (t == target).then(|| (*pack(ext, target)).clone());
        let Some(rows) = self.testimony(
            &builtin,
            &format!("the .{ext} testimony through the declarations"),
        ) else {
            return;
        };
        let (total, mut failures) = judge(&rows);
        assert!(total > 0, "no recorded field was asked");
        failures.truncate(30);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The testimony notices [`Gates::late`] declared one byte late.
    pub fn testimony_names_a_field_one_byte_late(&self) {
        let (ext, target) = (self.ext, self.target);
        let (ty, field) = self.late;
        let late = moved_by(&pack(ext, target), ty, field, 1);
        let Some(rows) = check(
            ext,
            &|t| (t == target).then(|| late.clone()),
            frozen_key().as_ref(),
            self.relocated,
            self.overread,
            self.moved,
            &format!("a misdeclared .{ext} against the testimony"),
            true,
        ) else {
            return;
        };
        let differ: usize = rows
            .iter()
            .map(|(_, t)| t.differ.len() + t.unanswered)
            .sum();
        println!("{differ} recorded field(s) caught with `{ty}.{field}` one byte late");
        assert!(differ > 0, "the testimony did not notice a moved field");
    }
}

/// Write one format and target's corpus gates as tests, each under the name it is given.
#[macro_export]
macro_rules! declared_gates {
    (
        gates: $gates:expr,
        typed: $typed:expr,
        reads_typed: $reads_typed:ident,
        $(owned: $owned:ty => $owned_test:ident,)?
        fixed_point: $fixed_point:ident,
        census: $census:ident,
        testimony: $testimony:ident,
        one_byte_late: $one_byte_late:ident $(,)?
    ) => {
        #[test]
        fn $reads_typed() {
            $gates.reads_typed_as_the_tree_reads(&$typed)
        }
        $(
            #[test]
            fn $owned_test() {
                $gates.opens_owned_and_saves_as_its_tree_saves::<$owned>()
            }
        )?
        #[test]
        fn $fixed_point() {
            $gates.saves_to_a_fixed_point()
        }
        #[test]
        fn $census() {
            $gates.is_aligned_for_its_view()
        }
        #[test]
        fn $testimony() {
            $gates.testimony_holds()
        }
        #[test]
        fn $one_byte_late() {
            $gates.testimony_names_a_field_one_byte_late()
        }
    };
}
