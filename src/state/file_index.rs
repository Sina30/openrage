//! Every file of the active install and of each install, folder and archive
//! Files shows, nested archives included: what Go to File and the search of
//! every archive read.
//!
//! ⚠ Walked off the UI thread, and what a first walk has found is usable while
//! it runs. A walk again reopens only the archives on disk whose size or time
//! changed, so it is cheap enough to follow every tree edit and save.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::SystemTime;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::hash::string_hash;
use or_title::install::crawl::{self, ControlFlow, LOOSE};

use super::document::DocumentId;
use super::jobs::{file_name, JobKind, Outcome, RunningJob, Then};
use super::memory::hash_table_bytes;
use super::vfs_session::{self, Mounted, DOCUMENT_MOUNT};
use super::{AppState, DocPayload, DocumentKind};

/// How many paths a first walk hands over at a time.
const BATCH: usize = 4096;

/// What an archive on disk was when it was walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// One root's files.
// ponytail: one string per path, repeating each archive chain; intern the chains if the memory matters.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    /// Every file's path below the root; an archive on disk is followed by what it holds.
    pub files: Vec<Box<str>>,
    /// Each archive on disk the walk opened, by its path below the root.
    stamps: HashMap<Box<str>, Stamp>,
    /// What the walk could not open or read.
    pub warnings: Vec<String>,
}

impl Listing {
    /// Its paths and the stamps of the archives it opened, each string at its capacity.
    fn held_bytes(&self) -> usize {
        let paths: usize = self.files.iter().map(|path| path.len()).sum();
        let stamped: usize = self.stamps.keys().map(|path| path.len()).sum();
        let warnings: usize = self.warnings.iter().map(String::capacity).sum();
        self.files.capacity() * std::mem::size_of::<Box<str>>()
            + paths
            + hash_table_bytes::<(Box<str>, Stamp)>(self.stamps.capacity())
            + stamped
            + self.warnings.capacity() * std::mem::size_of::<String>()
            + warnings
    }
}

/// A root's document as a tree edit or a save leaves it: its edits, whether it
/// is unsaved, its unsaved archives and its rows.
type Seen = (u64, bool, usize, usize);

/// An install, a folder or an archive, and its files.
pub struct Root {
    pub path: PathBuf,
    /// The document Files shows it as, if one is open.
    pub doc: Option<DocumentId>,
    /// An archive opened as a document, walked through the archive it holds.
    archive: bool,
    pub listing: Arc<Listing>,
    /// Bumped each time `listing` is replaced rather than added to.
    pub generation: u64,
    walked: bool,
    walk: Option<Walk>,
    seen: Option<Seen>,
    /// Asked to walk again while a walk was running.
    stale: bool,
}

struct Walk {
    rx: Receiver<Message>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Walk {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

enum Message {
    /// Paths a first walk found, in its order.
    Found(Vec<Box<str>>),
    /// A first walk's end: the archives it opened, and what it could not.
    Finished {
        stamps: HashMap<Box<str>, Stamp>,
        warnings: Vec<String>,
    },
    /// A walk again's whole listing.
    Replaced(Listing),
}

enum Source {
    Disk(Arc<dyn RpfCryptoProvider>),
    Archive(Mounted),
}

impl Root {
    fn new(path: PathBuf, doc: Option<DocumentId>, archive: bool, seen: Option<Seen>) -> Self {
        Self {
            path,
            doc,
            archive,
            listing: Arc::default(),
            generation: 0,
            walked: false,
            walk: None,
            seen,
            stale: false,
        }
    }

    pub fn is_walking(&self) -> bool {
        self.walk.is_some()
    }

    fn start(&mut self, source: Source) {
        let previous = self.walked.then(|| Arc::clone(&self.listing));
        let root = self.path.clone();
        self.start_with(move |tx, stop| match source {
            Source::Disk(provider) => walk_disk(&root, provider, previous, tx, stop),
            Source::Archive(mounted) => walk_archive(&mounted, tx),
        });
    }

    fn start_with(&mut self, work: impl FnOnce(&Sender<Message>, &AtomicBool) + Send + 'static) {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        std::thread::spawn(move || work(&tx, &stop));
        self.walk = Some(Walk { rx, cancel });
    }

    /// A root with nothing to walk, such as an archive never saved.
    fn settle_empty(&mut self) {
        self.listing = Arc::default();
        self.generation += 1;
        self.walked = true;
    }

    /// Take what the walk found; how many files a first walk found, once it has ended.
    fn drain(&mut self) -> Option<usize> {
        let walk = self.walk.as_ref()?;
        let finished = loop {
            match walk.rx.try_recv() {
                Ok(Message::Found(paths)) => Arc::make_mut(&mut self.listing).files.extend(paths),
                Ok(Message::Finished { stamps, warnings }) => {
                    let listing = Arc::make_mut(&mut self.listing);
                    listing.stamps = stamps;
                    listing.warnings = warnings;
                    break true;
                }
                Ok(Message::Replaced(listing)) => {
                    self.listing = Arc::new(listing);
                    self.generation += 1;
                    break true;
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    // A walk that died before its end must not read as one that looked everywhere.
                    let stopped =
                        format!("{}: the walk stopped before its end", self.path.display());
                    Arc::make_mut(&mut self.listing).warnings.push(stopped);
                    break false;
                }
            }
        };
        self.walk = None;
        let first = !std::mem::replace(&mut self.walked, true);
        (first && finished).then(|| self.listing.files.len())
    }
}

/// What the index should hold this frame: one root, and its document if one is open.
struct Wanted {
    path: PathBuf,
    doc: Option<DocumentId>,
    archive: bool,
    seen: Option<Seen>,
}

#[derive(Default)]
pub struct FileIndex {
    pub roots: Vec<Root>,
    /// Whether the roots were walked with keys, which open archives nothing else can.
    keys: bool,
    /// Walk every root on disk again, for what changed outside the tree.
    pub recheck: bool,
}

impl FileIndex {
    pub fn root_of(&self, doc: DocumentId) -> Option<&Root> {
        self.roots.iter().find(|root| root.doc == Some(doc))
    }

    /// What every root's listing holds, each path once for the root that found it.
    pub fn held_bytes(&self) -> usize {
        let roots = self.roots.capacity() * std::mem::size_of::<Root>();
        let held = |root: &Root| root.path.capacity() + root.listing.held_bytes();
        roots + self.roots.iter().map(held).sum::<usize>()
    }

    pub fn is_walking(&self) -> bool {
        self.roots.iter().any(Root::is_walking)
    }

    pub fn files(&self) -> usize {
        self.roots.iter().map(|root| root.listing.files.len()).sum()
    }

    pub fn generations(&self) -> u64 {
        self.roots.iter().map(|root| root.generation).sum()
    }

    pub fn warnings(&self) -> impl Iterator<Item = &String> {
        self.roots.iter().flat_map(|root| &root.listing.warnings)
    }

    /// A first walk in flight, as the job list draws it; a walk again is too short to list.
    pub fn job(&self) -> Option<RunningJob> {
        let mut first = self.roots.iter().filter(|r| r.is_walking() && !r.walked);
        let root = first.next()?;
        let files =
            root.listing.files.len() + first.map(|other| other.listing.files.len()).sum::<usize>();
        Some(RunningJob {
            kind: JobKind::FileIndex,
            doing: format!("Listing every file of {}", file_name(&root.path)),
            fraction: None,
            detail: format!("{files} files so far"),
            stopping: false,
        })
    }

    /// Stop every walk, keeping what it found; `false` when none runs.
    pub fn stop(&mut self) -> bool {
        let mut stopped = false;
        for root in &mut self.roots {
            if root.walk.take().is_none() {
                continue;
            }
            stopped = true;
            // A walk again leaves the last listing whole; a first walk's is part of the files.
            if !std::mem::replace(&mut root.walked, true) {
                let short = format!("{}: stopped before its end", root.path.display());
                Arc::make_mut(&mut root.listing).warnings.push(short);
            }
        }
        stopped
    }

    /// A walk of `path` that does `work` in place of reading it.
    #[cfg(test)]
    pub fn walk_with(&mut self, path: PathBuf, work: impl FnOnce(&AtomicBool) + Send + 'static) {
        let mut root = Root::new(path, None, false, None);
        root.start_with(move |_, stop| work(stop));
        self.roots.push(root);
    }

    /// Take `wanted` as the roots, and return the ones to walk now.
    fn sync(&mut self, wanted: Vec<Wanted>) -> Vec<usize> {
        let recheck = std::mem::take(&mut self.recheck);
        self.roots
            .retain(|root| wanted.iter().any(|want| want.path == root.path));
        let mut walk = Vec::new();
        for want in wanted {
            let Some(at) = self.roots.iter().position(|root| root.path == want.path) else {
                walk.push(self.roots.len());
                let root = Root::new(want.path, want.doc, want.archive, want.seen);
                self.roots.push(root);
                continue;
            };
            let root = &mut self.roots[at];
            let edited = root.seen.is_some() && want.seen.is_some() && root.seen != want.seen;
            root.doc = want.doc;
            root.seen = want.seen;
            root.stale |= edited || (recheck && !root.archive);
            if root.stale && !root.is_walking() {
                root.stale = false;
                walk.push(at);
            }
        }
        walk
    }

    /// The files `query` names: by the hash of their name first, then by their
    /// name, fuzzily, recent ones first; at most `limit`, and the recent ones
    /// alone for an empty query.
    pub fn find(
        &self,
        query: &str,
        scope: Scope,
        install: Option<&Path>,
        recent: &[Offer],
        limit: usize,
    ) -> Vec<Offer> {
        let in_scope = |root: &Path, path: &str| match scope {
            Scope::All => true,
            Scope::BaseGame | Scope::Overlay => {
                install == Some(root) && in_overlay(path) == (scope == Scope::Overlay)
            }
        };
        let letters = fuzzy_letters(query);
        if letters.is_empty() {
            return recent
                .iter()
                .filter(|offer| in_scope(&offer.root, &offer.path))
                .take(limit)
                .cloned()
                .collect();
        }
        let hashes = hashes_named(query);
        let recency: HashMap<(&Path, &str), usize> = recent
            .iter()
            .enumerate()
            .map(|(at, offer)| ((offer.root.as_path(), offer.path.as_str()), at))
            .collect();
        let mut found: Vec<(Rank, &Path)> = Vec::new();
        for root in &self.roots {
            for path in root.listing.files.iter().map(|path| &**path) {
                if !in_scope(&root.path, path) {
                    continue;
                }
                let leaf = leaf_of(path);
                let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
                let named = !hashes.is_empty()
                    && [leaf, stem]
                        .iter()
                        .any(|name| hashes.contains(&string_hash(name.as_bytes())));
                let Some(score) = fuzzy_score(&letters, leaf).or(named.then_some(0)) else {
                    continue;
                };
                let recent = recency
                    .get(&(root.path.as_path(), path))
                    .copied()
                    .unwrap_or(usize::MAX);
                found.push(((!named, recent, -score, leaf.len(), leaf, path), &root.path));
            }
        }
        if found.len() > limit {
            found.select_nth_unstable_by(limit, |a, b| a.0.cmp(&b.0));
            found.truncate(limit);
        }
        found.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        found
            .into_iter()
            .map(|((.., path), root)| Offer {
                root: root.to_path_buf(),
                path: path.to_string(),
            })
            .collect()
    }
}

/// Where a file Go to File finds sorts, ascending: a file a hash names, then
/// the recent, the best score, the shortest name, the name and the path.
type Rank<'a> = (bool, usize, i32, usize, &'a str, &'a str);

/// Which files Go to File looks through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    BaseGame,
    Overlay,
}

impl Scope {
    pub const ALL: [Scope; 3] = [Scope::All, Scope::BaseGame, Scope::Overlay];

    pub fn label(self) -> &'static str {
        match self {
            Scope::All => "All",
            Scope::BaseGame => "Base game",
            Scope::Overlay => "Mods overlay",
        }
    }

    pub fn hover(self) -> &'static str {
        match self {
            Scope::All => "The install, and every folder and archive open in Files",
            Scope::BaseGame => "The install's own files, outside its mods folder",
            Scope::Overlay => "The install's mods folder, which the game reads first",
        }
    }
}

/// A file Go to File offers: the root it is under and its path there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub root: PathBuf,
    pub path: String,
}

impl Offer {
    pub fn leaf(&self) -> &str {
        leaf_of(&self.path)
    }

    /// The folders and archives it is in, below its root.
    pub fn place(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(place, _)| place)
    }
}

fn leaf_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn in_overlay(path: &str) -> bool {
    path.split('/')
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case(or_title::OVERLAY_DIR))
}

/// The hashes `query` spells: `0x` and hex digits, eight hex digits, or a decimal, signed or not.
fn hashes_named(query: &str) -> Vec<u32> {
    let query = query.trim();
    if let Some(hex) = query
        .strip_prefix("0x")
        .or_else(|| query.strip_prefix("0X"))
    {
        return u32::from_str_radix(hex, 16).ok().into_iter().collect();
    }
    let mut hashes = Vec::new();
    if query.len() == 8 {
        hashes.extend(u32::from_str_radix(query, 16).ok());
    }
    hashes.extend(
        query
            .parse::<u32>()
            .ok()
            .or_else(|| query.parse::<i32>().ok().map(|signed| signed as u32)),
    );
    hashes
}

/// `query` as [`fuzzy_score`] reads it: lower case, without spaces.
pub fn fuzzy_letters(query: &str) -> Vec<u8> {
    query
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|b| b.to_ascii_lowercase())
        .collect()
}

/// How well `letters` match `text` in order, ignoring case: highest for a run
/// of letters and for the text's own start, then for a word's start, lower for
/// the letters skipped; `None` when they do not all appear.
///
/// ⚠ A run outweighs letters scattered over word starts, and a `.` starts no
/// word: otherwise `addr` ranks `mph3_add.rpf`, whose extension's `r` starts a
/// word, above `adder.yft` in a retail install.
// ponytail: tries each start of the first letter and then matches greedily, not every alignment.
pub fn fuzzy_score(letters: &[u8], text: &str) -> Option<i32> {
    let text = text.as_bytes();
    let Some(&first) = letters.first() else {
        return Some(0);
    };
    (0..text.len())
        .filter(|&start| text[start].to_ascii_lowercase() == first)
        .filter_map(|start| {
            let mut score = 0;
            let mut previous: Option<usize> = None;
            let mut at = start;
            for &letter in letters {
                let found = (at..text.len()).find(|&i| text[i].to_ascii_lowercase() == letter)?;
                let starts_word = found > 0 && {
                    let before = text[found - 1];
                    !before.is_ascii_alphanumeric() && before != b'.'
                };
                score += 1 + match previous {
                    Some(p) if p + 1 == found => 6,
                    _ if found == 0 => 6,
                    _ if starts_word => 4,
                    _ => 0,
                };
                if previous.is_some() {
                    score -= (found - at).min(3) as i32;
                }
                previous = Some(found);
                at = found + 1;
            }
            Some(score)
        })
        .max()
}

fn tree_path(container: &str, path: &str) -> Box<str> {
    if container == LOOSE {
        path.into()
    } else {
        format!("{container}/{path}").into()
    }
}

/// Walk a folder on disk and the archives in it, reopening only those that
/// changed since `previous`; a first walk streams what it finds.
fn walk_disk(
    root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    previous: Option<Arc<Listing>>,
    tx: &Sender<Message>,
    cancel: &AtomicBool,
) {
    let mut warnings = Vec::new();
    let mut stamps: HashMap<Box<str>, Stamp> = HashMap::new();
    let mut kept: HashSet<Box<str>> = HashSet::new();
    let mut found: Vec<Box<str>> = Vec::new();
    let streams = previous.is_none();
    let outcome = crawl::crawl_where(
        root,
        provider,
        &mut warnings,
        &mut |path| {
            let rel: Box<str> = crawl::display_rel(root, path).into();
            let stamp = stamp_of(path);
            let before = previous.as_ref().and_then(|p| p.stamps.get(&rel));
            let unchanged = stamp.is_some() && before == stamp.as_ref();
            if let Some(stamp) = stamp {
                stamps.insert(rel.clone(), stamp);
            }
            if unchanged {
                kept.insert(rel);
            }
            !unchanged
        },
        &mut |file| {
            if cancel.load(Ordering::Relaxed) {
                return ControlFlow::Break(());
            }
            found.push(tree_path(file.container, file.path));
            if streams && found.len() >= BATCH {
                let _ = tx.send(Message::Found(std::mem::take(&mut found)));
            }
            ControlFlow::Continue(())
        },
    );
    if outcome.is_break() {
        return;
    }
    let Some(previous) = previous else {
        let _ = tx.send(Message::Found(found));
        let _ = tx.send(Message::Finished { stamps, warnings });
        return;
    };
    // An archive not reopened keeps what it held, and what could not be read in it.
    warnings.extend(
        previous
            .warnings
            .iter()
            .filter(|warning| kept.iter().any(|archive| warning.contains(&**archive)))
            .cloned(),
    );
    let files = with_kept_archives(found, &previous, &kept);
    let _ = tx.send(Message::Replaced(Listing {
        files,
        stamps,
        warnings,
    }));
}

/// `found`, with what each archive in `kept` held in `previous` put back after it.
fn with_kept_archives(
    found: Vec<Box<str>>,
    previous: &Listing,
    kept: &HashSet<Box<str>>,
) -> Vec<Box<str>> {
    if kept.is_empty() {
        return found;
    }
    let files = &previous.files;
    let mut held: HashMap<&str, &[Box<str>]> = HashMap::new();
    let mut at = 0;
    while at < files.len() {
        let archive = &files[at];
        at += 1;
        if !kept.contains(archive) {
            continue;
        }
        let inside = format!("{archive}/");
        let end = files[at..]
            .iter()
            .position(|file| !file.starts_with(&inside))
            .map_or(files.len(), |n| at + n);
        held.insert(archive, &files[at..end]);
        at = end;
    }
    let mut out = Vec::with_capacity(found.len() + held.values().map(|h| h.len()).sum::<usize>());
    for path in found {
        let inside = held.get(&*path).copied();
        out.push(path);
        out.extend(inside.into_iter().flatten().cloned());
    }
    out
}

/// Walk an archive opened as a document, through the archive the document holds.
fn walk_archive(mounted: &Mounted, tx: &Sender<Message>) {
    let (result, undescended) = vfs_session::find(mounted, "**");
    let files = result
        .matches
        .iter()
        .map(|path| {
            path.strip_prefix(DOCUMENT_MOUNT)
                .unwrap_or(path)
                .trim_start_matches('/')
                .into()
        })
        .collect();
    let warnings = result.unwalkable.into_iter().chain(undescended).collect();
    let _ = tx.send(Message::Replaced(Listing {
        files,
        stamps: HashMap::new(),
        warnings,
    }));
}

impl AppState {
    /// Index the active install and every install, folder and archive Files shows, and take what the walks found.
    pub fn tick_file_index(&mut self) {
        let keys = self.keys.has_keys();
        if self.file_index.keys != keys {
            // New keys open archives the last walks could not, and a walk again reopens only what changed.
            self.file_index = FileIndex {
                keys,
                ..FileIndex::default()
            };
        }
        let wanted = self.index_roots();
        for at in self.file_index.sync(wanted) {
            let root = &self.file_index.roots[at];
            let source = if root.archive {
                let doc = root.doc.and_then(|doc| self.document(doc));
                match doc.map(|doc| &doc.payload) {
                    Some(DocPayload::Rpf(rpf)) => {
                        vfs_session::mount_document(&rpf.session).map(Source::Archive)
                    }
                    _ => None,
                }
            } else {
                Some(Source::Disk(self.keys.provider()))
            };
            let root = &mut self.file_index.roots[at];
            match source {
                Some(source) => root.start(source),
                None => root.settle_empty(),
            }
        }
        for root in &mut self.file_index.roots {
            if let Some(files) = root.drain() {
                let said = format!("{files} files of {}", file_name(&root.path));
                self.jobs
                    .record(JobKind::FileIndex, Outcome::Done(Then::Nothing), said);
            }
        }
    }

    fn index_roots(&self) -> Vec<Wanted> {
        let mut wanted: Vec<Wanted> = Vec::new();
        for doc in self
            .documents
            .iter()
            .filter(|d| d.rpf_entry_binding.is_none())
        {
            let (Some(path), DocPayload::Rpf(rpf)) = (&doc.path, &doc.payload) else {
                continue;
            };
            let unsaved = rpf.session.dirty_archive_ids().len();
            wanted.push(Wanted {
                path: path.clone(),
                doc: Some(doc.id),
                archive: !matches!(doc.kind, DocumentKind::GameRoot | DocumentKind::Folder),
                seen: Some((
                    doc.edit_epoch(),
                    doc.dirty,
                    unsaved,
                    rpf.session.nodes.len(),
                )),
            });
        }
        if let Some(install) = self.prefs.active_game_root() {
            if !wanted.iter().any(|want| want.path == *install) && install.is_dir() {
                wanted.push(Wanted {
                    path: install.clone(),
                    doc: None,
                    archive: false,
                    seen: None,
                });
            }
        }
        wanted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_is_read_in_hex_with_or_without_its_prefix_and_in_decimal_signed_or_not() {
        let hash = string_hash(b"adder");
        assert_eq!(hashes_named(&format!("0x{hash:08x}")), [hash]);
        assert!(hashes_named(&format!("{hash:08X}")).contains(&hash));
        assert!(hashes_named(&hash.to_string()).contains(&hash));
        assert!(hashes_named(&(hash as i32).to_string()).contains(&hash));
        assert!(hashes_named("adder").is_empty());
    }

    #[test]
    fn an_archive_not_reopened_keeps_what_it_held() {
        let previous = Listing {
            files: [
                "a.rpf",
                "a.rpf/x.ydr",
                "a.rpf/n.rpf/y.ydr",
                "a.rpfx",
                "b.rpf",
                "b.rpf/old.ydr",
            ]
            .map(Box::from)
            .to_vec(),
            ..Listing::default()
        };
        let kept: HashSet<Box<str>> = HashSet::from([Box::from("a.rpf")]);
        let found: Vec<Box<str>> = ["b.rpf", "b.rpf/new.ydr", "a.rpf", "a.rpfx"]
            .map(Box::from)
            .to_vec();
        let files = with_kept_archives(found, &previous, &kept);
        assert_eq!(
            files.iter().map(|f| &**f).collect::<Vec<_>>(),
            [
                "b.rpf",
                "b.rpf/new.ydr",
                "a.rpf",
                "a.rpf/x.ydr",
                "a.rpf/n.rpf/y.ydr",
                "a.rpfx"
            ]
        );
    }
}
