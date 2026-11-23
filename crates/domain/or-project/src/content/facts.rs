//! What the disk says of the files a module names, gathered once so that every rule and the plan are pure functions of it.
//!
//! A kind of content names its files and asks [`inspect_paths`]; what comes back is the same for every
//! kind, so a file that a person edited is read again and one that did not change is not.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// How much of a file is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Depth {
    /// Its presence and size.
    Stat,
    /// Its bytes, for the digest the diff of two builds compares.
    Digest,
    /// Its bytes, and what a measurer counts in them, for the rules that hold a value to a limit.
    Measure,
    /// Its bytes, what a measurer counts in them, and whether the reader of its format opens it.
    Open,
}

/// What a measurer counted in a file, as JSON that a kind of content reads back as a type of its own; `Null` for a file it counts nothing in.
pub type Measured = serde_json::Value;

/// Counts what a kind of content holds the file called `name` to, or `None` for a file it counts nothing in.
pub type Measurer = fn(&str, &[u8]) -> Option<Measured>;

/// What is known of one file a pack names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    /// The name leaves the module's folder, or is no name a host can create.
    Escapes,
    Missing,
    /// A folder where a file was named.
    Folder,
    File {
        bytes: u64,
        /// Hex SHA-256 of the content, from [`Depth::Digest`] on.
        digest: Option<String>,
        /// The reader that opened it, or the reason it refused; from [`Depth::Open`].
        read: Option<Result<String, String>>,
        /// What a measurer counted in it, from [`Depth::Measure`] on, where one was given.
        measured: Option<Measured>,
    },
}

impl Fact {
    /// A file of `bytes` that was looked at no deeper than its size and, where `digest` is given, its content.
    pub fn file(bytes: u64, digest: Option<String>) -> Self {
        Self::File {
            bytes,
            digest,
            read: None,
            measured: None,
        }
    }
}

/// The files a pack names, by the path it names them with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    files: BTreeMap<String, Fact>,
}

impl Facts {
    pub fn get(&self, path: &str) -> Option<&Fact> {
        self.files.get(path)
    }

    pub fn insert(&mut self, path: &str, fact: Fact) {
        self.files.insert(path.to_string(), fact);
    }

    /// The facts of `files`, as if every one of them were there with the size and digest given.
    pub fn from_files<'a>(files: impl IntoIterator<Item = (&'a str, u64, &'a str)>) -> Self {
        let mut facts = Self::default();
        for (path, bytes, digest) in files {
            facts.insert(path, Fact::file(bytes, Some(digest.to_string())));
        }
        facts
    }
}

/// Whether `path` stays inside the folder it is relative to, with `/` between names that every host can create.
pub fn stays_inside(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && path.split('/').all(crate::paths::is_component)
}

/// The reader that opens the bytes of the file called `name`, or why none does: the first field of
/// a fact's `read`.
pub type Reader = fn(&str, &[u8]) -> Result<String, String>;

/// The files `paths` name, looked at in `folder` as deeply as `depth` says, and opened by the readers of the paged formats.
///
/// ⚠ A folder that holds many of them is listed once: on Windows asking for a file opens it, which
/// costs about 150 µs here, so a pack of 12,000 files took two seconds to find that nothing had changed,
/// and a listing hands over the size and time of every entry as it reads them.
pub fn inspect_paths<'a>(
    folder: &Path,
    paths: impl IntoIterator<Item = &'a str>,
    depth: Depth,
) -> Facts {
    gather(folder, paths, depth, paged, None)
}

/// [`inspect_paths`] with `reader` opening the files, for a kind of content whose files are not all paged formats.
pub fn inspect_paths_with<'a>(
    folder: &Path,
    paths: impl IntoIterator<Item = &'a str>,
    depth: Depth,
    reader: Reader,
) -> Facts {
    gather(folder, paths, depth, reader, None)
}

/// [`inspect_paths`], and from [`Depth::Measure`] on `measurer` counts what each file holds as its bytes are read.
pub fn inspect_paths_measured<'a>(
    folder: &Path,
    paths: impl IntoIterator<Item = &'a str>,
    depth: Depth,
    measurer: Measurer,
) -> Facts {
    gather(folder, paths, depth, paged, Some(measurer))
}

/// How many files make a scan worth sharing between threads: below it the threads cost more than they read.
const PARALLEL_FROM: usize = 64;

fn gather<'a>(
    folder: &Path,
    paths: impl IntoIterator<Item = &'a str>,
    depth: Depth,
    reader: Reader,
    measurer: Option<Measurer>,
) -> Facts {
    let mut facts = Facts::default();
    let mut asked: Vec<(&str, PathBuf)> = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        if !seen.insert(path) {
            continue;
        }
        if stays_inside(path) {
            let at = folder.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
            asked.push((path, at));
        } else {
            facts.files.insert(path.to_string(), Fact::Escapes);
        }
    }
    let mut crowd: BTreeMap<&Path, usize> = BTreeMap::new();
    for (_, at) in &asked {
        if let Some(parent) = at.parent() {
            *crowd.entry(parent).or_default() += 1;
        }
    }
    let listings: BTreeMap<&Path, BTreeMap<OsString, Metadata>> = (crowd.into_iter())
        .filter(|(_, named)| *named >= LISTED_FROM)
        .map(|(parent, _)| (parent, listing(parent)))
        .collect();
    let one = |(path, at): &(&str, PathBuf)| {
        let listed = (at.parent().and_then(|parent| listings.get(parent)))
            .zip(at.file_name())
            .and_then(|(listing, name)| listing.get(name));
        look_with(at, path, depth, listed, reader, measurer)
    };
    // ⚠ Reading the bytes and measuring them is the cost of a first look, and files are independent, so the files
    // that need their bytes are shared out; the order they are looked at in changes nothing of what comes back.
    let looked: Vec<Fact> = match depth >= Depth::Digest && asked.len() >= PARALLEL_FROM {
        true => shared_out(&asked, &one),
        false => asked.iter().map(one).collect(),
    };
    for ((path, _), fact) in asked.iter().zip(looked) {
        facts.files.insert(path.to_string(), fact);
    }
    facts
}

/// `look` of every item of `asked`, in its order, by as many threads as the machine has up to eight.
fn shared_out<T: Sync>(asked: &[T], look: &(impl Fn(&T) -> Fact + Sync)) -> Vec<Fact> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut done: Vec<(usize, Fact)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = asked.get(at) else {
                            return mine;
                        };
                        mine.push((at, look(item)));
                    }
                })
            })
            .collect();
        // ⚠ A worker that panicked took its files with it: the rest are paired with the paths in order, so every later file would get another's fact.
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|e| std::panic::resume_unwind(e))
            })
            .collect()
    });
    done.sort_by_key(|(at, _)| *at);
    done.into_iter().map(|(_, fact)| fact).collect()
}

/// The metadata of the entries of `dir` by name, links left out: `fs::metadata` follows them and an entry does not.
fn listing(dir: &Path) -> BTreeMap<OsString, Metadata> {
    LISTED.with(|n| n.set(n.get() + 1));
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_type().is_ok_and(|kind| !kind.is_symlink()) {
            if let Ok(meta) = entry.metadata() {
                out.insert(entry.file_name(), meta);
            }
        }
    }
    out
}

/// How many named files a folder must hold before it is listed once instead of each file being asked for.
pub const LISTED_FROM: usize = 32;

thread_local! {
    /// How many folders this thread listed whole instead of asking for each file in them: a count a test reads to see that a crowded folder is listed once.
    pub static LISTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What is known of the file at `at`, whose metadata `listed` already holds when its folder was listed.
///
/// ⚠ A name a listing lacks is asked for on its own, since a host that ignores case finds it by another spelling.
pub fn look(at: &Path, named: &str, depth: Depth, listed: Option<&Metadata>) -> Fact {
    look_with(at, named, depth, listed, paged, None)
}

/// [`look`] with `reader` opening the file and `measurer` counting what it holds.
fn look_with(
    at: &Path,
    named: &str,
    depth: Depth,
    listed: Option<&Metadata>,
    reader: Reader,
    measurer: Option<Measurer>,
) -> Fact {
    let meta = match listed {
        Some(meta) => meta.clone(),
        None => match std::fs::metadata(at) {
            Ok(meta) => meta,
            Err(_) => return Fact::Missing,
        },
    };
    if meta.is_dir() {
        return Fact::Folder;
    }
    if depth == Depth::Stat {
        return Fact::file(meta.len(), None);
    }
    let measuring = measurer.is_some() && depth >= Depth::Measure;
    let stamp = meta.modified().ok().map(|at| (meta.len(), at));
    if let Some(hit) = stamp.and_then(|stamp| remembered(at, stamp, depth, measuring)) {
        return hit;
    }
    let bytes = match std::fs::read(at) {
        Ok(bytes) => {
            READ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            bytes
        }
        Err(e) => {
            return Fact::File {
                bytes: meta.len(),
                digest: None,
                read: Some(Err(format!("cannot be read: {e}"))),
                measured: None,
            }
        }
    };
    let read = (depth == Depth::Open).then(|| reader(named, &bytes));
    let measured = measurer
        .filter(|_| measuring)
        .map(|measure| measure(named, &bytes).unwrap_or(Measured::Null));
    let fact = Fact::File {
        bytes: bytes.len() as u64,
        digest: Some(digest(&bytes)),
        read,
        measured,
    };
    if let Some(stamp) = stamp {
        remember(at, stamp, &fact);
    }
    fact
}

/// What was last read of a file, with the size and time it had: a scan that runs again while a
/// project is on screen must not read every byte of a pack again to find nothing changed.
type Stamp = (u64, std::time::SystemTime);

type Memory = BTreeMap<PathBuf, (Stamp, Fact)>;

fn memory() -> &'static std::sync::Mutex<Memory> {
    static MEMORY: std::sync::OnceLock<std::sync::Mutex<Memory>> = std::sync::OnceLock::new();
    MEMORY.get_or_init(Default::default)
}

/// How many files are remembered at most. ponytail: a map cleared when full; a pack of more files than this reads them again on each scan.
const REMEMBERED: usize = 100_000;

/// How many files were read whole, by every look at a file, since this process began.
static READ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many files this process has read whole: a count a test reads to see that a second look read none.
pub fn files_read() -> usize {
    READ.load(std::sync::atomic::Ordering::Relaxed)
}

/// How many times what is remembered was written out as text since this process began.
static TOLD: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many times [`remembered_under`] ran: a count a test reads to see that a scan that read nothing wrote nothing out.
pub fn remembered_told() -> usize {
    TOLD.load(std::sync::atomic::Ordering::Relaxed)
}

fn remembered(at: &Path, stamp: Stamp, depth: Depth, measuring: bool) -> Option<Fact> {
    let memory = memory().lock().ok()?;
    let (held, fact) = memory.get(at)?;
    let Fact::File {
        bytes,
        digest,
        read,
        measured,
    } = fact
    else {
        return None;
    };
    if *held != stamp
        || digest.is_none()
        || (depth == Depth::Open && read.is_none())
        || (measuring && measured.is_none())
    {
        return None;
    }
    Some(Fact::File {
        bytes: *bytes,
        digest: digest.clone(),
        read: if depth == Depth::Open {
            read.clone()
        } else {
            None
        },
        measured: if measuring { measured.clone() } else { None },
    })
}

/// A file changed more recently than this is not remembered: a second write within one tick of the file system's clock keeps its size and its time.
const SETTLED: std::time::Duration = std::time::Duration::from_secs(2);

fn remember(at: &Path, stamp: Stamp, fact: &Fact) {
    if stamp.1.elapsed().is_ok_and(|age| age < SETTLED) {
        return;
    }
    let Ok(mut memory) = memory().lock() else {
        return;
    };
    if memory.len() >= REMEMBERED {
        memory.clear();
    }
    let mut fact = fact.clone();
    // ⚠ A reading made at a shallower depth must not replace a deeper one of the same file, and what each knows is kept.
    if let (
        Some((held, Fact::File { read, measured, .. })),
        Fact::File {
            read: r,
            measured: m,
            ..
        },
    ) = (memory.get(at), &mut fact)
    {
        if *held == stamp {
            *r = r.take().or_else(|| read.clone());
            *m = m.take().or_else(|| measured.clone());
        }
    }
    memory.insert(at.to_path_buf(), (stamp, fact));
}

/// Forget what this process remembers of the files under `under`.
pub fn forget_under(under: &Path) {
    if let Ok(mut memory) = memory().lock() {
        memory.retain(|at, _| !at.starts_with(under));
    }
}

/// How many files this process remembers under `under`.
pub fn remembered_count(under: &Path) -> usize {
    memory().lock().map_or(0, |memory| {
        memory
            .iter()
            .filter(|(at, (_, fact))| {
                at.starts_with(under)
                    && matches!(
                        fact,
                        Fact::File {
                            digest: Some(_),
                            ..
                        }
                    )
            })
            .count()
    })
}

/// What this process remembers of the files under `under`, as text that [`remember_from`] takes back in a later session.
///
/// A pack of 12,000 files is read once and not again until a file changes, and that holds across sessions when this is kept.
pub fn remembered_under(under: &Path) -> String {
    use serde_json::json;
    TOLD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let result = |read: &Option<Result<String, String>>| match read {
        None => json!(null),
        Some(Ok(by)) => json!({ "ok": by }),
        Some(Err(why)) => json!({ "err": why }),
    };
    let files: Vec<serde_json::Value> = match memory().lock() {
        Ok(memory) => memory
            .iter()
            .filter(|(at, _)| at.starts_with(under))
            .filter_map(|(at, ((len, time), fact))| {
                let Fact::File {
                    bytes,
                    digest: Some(digest),
                    read,
                    measured,
                } = fact
                else {
                    return None;
                };
                let since = time.duration_since(std::time::UNIX_EPOCH).ok()?;
                Some(json!({
                    "path": at.to_str()?,
                    "len": len,
                    "secs": since.as_secs(),
                    "nanos": since.subsec_nanos(),
                    "bytes": bytes,
                    "digest": digest,
                    "read": result(read),
                    "measured": measured.as_ref().map(|m| json!({ "v": m })),
                }))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    json!({ "version": 1, "files": files }).to_string()
}

/// Take back what [`remembered_under`] wrote: how many files it holds. A file whose size or time is not the one it was read at is read again, so a stale entry costs nothing.
pub fn remember_from(text: &str) -> usize {
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(text)
    else {
        return 0;
    };
    if root.get("version").and_then(|v| v.as_u64()) != Some(1) {
        return 0;
    }
    let Some(files) = root.get("files").and_then(|f| f.as_array()) else {
        return 0;
    };
    let Ok(mut memory) = memory().lock() else {
        return 0;
    };
    let mut taken = 0;
    for entry in files {
        let get = |key: &str| entry.get(key);
        let (Some(path), Some(len), Some(secs), Some(nanos), Some(bytes), Some(digest)) = (
            get("path").and_then(|v| v.as_str()),
            get("len").and_then(|v| v.as_u64()),
            get("secs").and_then(|v| v.as_u64()),
            get("nanos").and_then(|v| v.as_u64()),
            get("bytes").and_then(|v| v.as_u64()),
            get("digest").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let read = match get("read") {
            Some(v) if v.get("ok").is_some() => {
                Some(Ok(v["ok"].as_str().unwrap_or("").to_string()))
            }
            Some(v) if v.get("err").is_some() => {
                Some(Err(v["err"].as_str().unwrap_or("").to_string()))
            }
            _ => None,
        };
        let measured = get("measured").and_then(|m| m.get("v")).cloned();
        let time = std::time::UNIX_EPOCH + std::time::Duration::new(secs, nanos as u32);
        let fact = Fact::File {
            bytes,
            digest: Some(digest.to_string()),
            read,
            measured,
        };
        memory.insert(PathBuf::from(path), ((len, time), fact));
        taken += 1;
    }
    taken
}

/// The reader of a paged game format that opens `bytes`, or why none does.
pub fn paged(name: &str, bytes: &[u8]) -> Result<String, String> {
    let ext = name
        .rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase();
    let Some(format) = or_io::gta5::formats::format(&ext) else {
        return Err(format!("`.{ext}` is no format with a reader here"));
    };
    format
        .open_declared(bytes)
        .map(|target| format!("{} ({})", format.ext, target.edition_label()))
        .map_err(|e| e.to_string())
}

pub fn digest(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(name: &str, bytes: &[u8]) -> Result<String, String> {
        match name.rsplit_once('.') {
            Some((_, "txt")) => Ok(format!("text of {} byte(s)", bytes.len())),
            _ => Err("no reader".to_string()),
        }
    }

    #[test]
    fn a_kind_of_content_with_files_of_its_own_gives_the_reader_that_opens_them() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("b.bin"), b"x").unwrap();

        let facts = inspect_paths_with(
            dir.path(),
            ["a.txt", "b.bin", "../c.txt"],
            Depth::Open,
            custom,
        );

        let Some(Fact::File {
            read: Some(Ok(by)),
            digest: Some(d),
            ..
        }) = facts.get("a.txt")
        else {
            panic!("the custom reader opened it: {:?}", facts.get("a.txt"));
        };
        assert_eq!(by, "text of 5 byte(s)");
        assert_eq!(d, &digest(b"hello"));
        assert!(matches!(
            facts.get("b.bin"),
            Some(Fact::File {
                read: Some(Err(_)),
                ..
            })
        ));
        assert_eq!(facts.get("../c.txt"), Some(&Fact::Escapes));

        let paged_only = inspect_paths(dir.path(), ["a.txt"], Depth::Open);
        assert!(
            matches!(paged_only.get("a.txt"), Some(Fact::File { read: Some(Err(why)), .. }) if why.contains("no format")),
            "the default reader is the paged formats' alone"
        );
    }

    /// How many times each file name was measured by this test binary, by name, so that two tests never count one another's files.
    fn measured_times() -> &'static std::sync::Mutex<std::collections::HashMap<String, usize>> {
        static TIMES: std::sync::OnceLock<
            std::sync::Mutex<std::collections::HashMap<String, usize>>,
        > = std::sync::OnceLock::new();
        TIMES.get_or_init(Default::default)
    }

    fn times(name: &str) -> usize {
        measured_times()
            .lock()
            .unwrap()
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    /// Counts the bytes of a `.len` file and says nothing of any other.
    fn length_of(name: &str, bytes: &[u8]) -> Option<Measured> {
        *measured_times()
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default() += 1;
        name.ends_with(".len")
            .then(|| serde_json::json!({ "length": bytes.len() }))
    }

    fn written(path: &Path, bytes: &[u8], when: std::time::SystemTime) {
        std::fs::write(path, bytes).unwrap();
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(when).unwrap();
    }

    fn measure(dir: &Path, names: &[&str], depth: Depth) -> Facts {
        inspect_paths_measured(dir, names.iter().copied(), depth, length_of)
    }

    fn measured_of<'a>(facts: &'a Facts, name: &str) -> Option<&'a Measured> {
        match facts.get(name) {
            Some(Fact::File { measured, .. }) => measured.as_ref(),
            other => panic!("{name}: {other:?}"),
        }
    }

    #[test]
    fn a_file_is_measured_once_from_the_bytes_that_were_read_and_not_again_while_it_is_the_same_file(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let long = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        written(&dir.path().join("once_a.len"), b"12345", long);
        written(&dir.path().join("once_b.bin"), b"xy", long);
        let names = ["once_a.len", "once_b.bin"];

        let digested = measure(dir.path(), &names, Depth::Digest);
        assert_eq!(
            measured_of(&digested, "once_a.len"),
            None,
            "Digest does not measure"
        );
        assert_eq!(times("once_a.len"), 0);

        let first = measure(dir.path(), &names, Depth::Measure);
        assert_eq!(
            measured_of(&first, "once_a.len"),
            Some(&serde_json::json!({ "length": 5 }))
        );
        assert_eq!(
            measured_of(&first, "once_b.bin"),
            Some(&Measured::Null),
            "a file the measurer counts nothing in is measured, and is nothing"
        );

        let again = measure(dir.path(), &names, Depth::Measure);
        assert_eq!(again, first);
        assert_eq!(
            (times("once_a.len"), times("once_b.bin")),
            (1, 1),
            "the second look read nothing"
        );

        // A file of another size is another file.
        written(&dir.path().join("once_a.len"), b"1234567", long);
        let changed = measure(dir.path(), &names, Depth::Measure);
        assert_eq!(
            measured_of(&changed, "once_a.len"),
            Some(&serde_json::json!({ "length": 7 }))
        );
        assert_eq!((times("once_a.len"), times("once_b.bin")), (2, 1));
    }

    #[test]
    fn what_a_session_remembered_of_a_folder_is_handed_to_the_next_which_reads_only_what_changed() {
        let dir = tempfile::tempdir().unwrap();
        let long = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for name in ["kept_a.len", "kept_b.len"] {
            written(&dir.path().join(name), name.as_bytes(), long);
        }
        let names = ["kept_a.len", "kept_b.len"];
        let before = measure(dir.path(), &names, Depth::Open);
        assert_eq!((times("kept_a.len"), times("kept_b.len")), (1, 1));
        let text = remembered_under(dir.path());

        // A new session knows nothing, and reads both.
        forget_under(dir.path());
        assert_eq!(measure(dir.path(), &names, Depth::Open), before);
        assert_eq!((times("kept_a.len"), times("kept_b.len")), (2, 2));

        // Given what the last session kept, it reads neither, and says the same of both.
        forget_under(dir.path());
        assert_eq!(remember_from(&text), 2);
        let after = measure(dir.path(), &names, Depth::Open);
        assert_eq!(
            after, before,
            "the cache keeps the digest, the reading and the measure"
        );
        assert_eq!((times("kept_a.len"), times("kept_b.len")), (2, 2));

        // A file changed between the sessions is read again, and the other is not.
        forget_under(dir.path());
        remember_from(&text);
        written(&dir.path().join("kept_a.len"), b"changed in between", long);
        let later = measure(dir.path(), &names, Depth::Open);
        assert_eq!((times("kept_a.len"), times("kept_b.len")), (3, 2));
        assert_eq!(
            measured_of(&later, "kept_a.len"),
            Some(&serde_json::json!({ "length": 18 }))
        );

        // What the text holds of another folder is not this folder's.
        let entries = |text: &str| {
            serde_json::from_str::<serde_json::Value>(text).unwrap()["files"]
                .as_array()
                .map_or(0, Vec::len)
        };
        assert_eq!(entries(&text), 2);
        assert_eq!(entries(&remembered_under(&dir.path().join("elsewhere"))), 0);
        assert_eq!(
            remember_from("not json"),
            0,
            "a cache that does not read is no cache"
        );
    }

    // ⚠ Cranelift, which builds the dev and test profiles, cannot unwind a panic: a panic there ends the process whether or not
    // the join below is looked at. So this runs only in a build that unwinds, `cargo test --release -p or-project --lib`.
    #[cfg(not(debug_assertions))]
    #[test]
    fn a_reader_that_panics_in_a_worker_ends_the_scan_and_never_gives_a_file_another_files_fact() {
        let items: Vec<u64> = (0..200).collect();
        let look = |n: &u64| match *n {
            37 => panic!("a reader of one file panicked"),
            n => Fact::file(n, None),
        };

        let outcome = std::panic::catch_unwind(|| shared_out(&items, &look));

        let Err(_) = outcome else {
            let facts = outcome.unwrap();
            let wrong = (facts.iter().zip(&items))
                .filter(|(fact, n)| **fact != Fact::file(**n, None))
                .count();
            panic!(
                "{} facts came back for 200 files, {wrong} of them another file's",
                facts.len()
            );
        };
    }
}
