use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::device::FileDevice;
use or_io::container::rpf::{BodyForm, RpfArchive, RpfEntry, RpfNode};

const MAX_NEST_DEPTH: u32 = 8;

/// How many nested archives one device keeps open at once.
///
/// ⚠ Unbounded, a bulk job that touches every `.rpf` in an install holds all
/// of them until the device drops.
///
/// Sixteen because the deepest chain the resolver will follow is
/// [`MAX_NEST_DEPTH`] = 8, so a cache smaller than that can evict an archive
/// still on the stack of the walk that is using it — the eviction would be
/// correct and the re-open pointless. Twice the depth keeps a whole descent
/// resident with room for a sibling.
const NESTED_CACHE_CAPACITY: usize = 16;

/// A bounded, least-recently-used map of open nested archives.
///
/// Recency is a counter rather than a clock: two opens inside one millisecond
/// are ordered here and would not be by a timestamp, and this is exactly the
/// access pattern a recursive descent has.
#[derive(Debug, Default)]
struct NestedArchives {
    entries: HashMap<String, (u64, Arc<RpfArchive>)>,
    clock: u64,
}

impl NestedArchives {
    fn get(&mut self, key: &str) -> Option<Arc<RpfArchive>> {
        self.clock += 1;
        let clock = self.clock;
        let (stamp, archive) = self.entries.get_mut(key)?;
        *stamp = clock;
        Some(Arc::clone(archive))
    }

    fn insert(&mut self, key: String, archive: Arc<RpfArchive>) {
        self.clock += 1;
        self.entries.insert(key, (self.clock, archive));
        while self.entries.len() > NESTED_CACHE_CAPACITY {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (stamp, _))| *stamp)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

type NestedCache = Arc<Mutex<NestedArchives>>;

#[derive(Clone)]
pub struct ArchiveDevice {
    archive: Arc<RpfArchive>,
    sub_root: String,
    label: String,
    nested_cache: NestedCache,
}

/// What an archive entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveEntryKind {
    Folder,
    File,
    Resource,
}

/// One entry of an archive directory, named as the archive spells it.
#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub kind: ArchiveEntryKind,
    pub entry_index: u32,
    /// The nested archive this entry opens as; `None` for any other entry, or one that failed to open.
    pub archive: Option<Arc<RpfArchive>>,
}

struct Resolved {
    archive: Arc<RpfArchive>,
    entry_index: u32,
    inside: String,
}

impl std::fmt::Debug for ArchiveDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchiveDevice")
            .field("sub_root", &self.sub_root)
            .field("entry_count", &self.archive.file_count())
            .finish()
    }
}

impl ArchiveDevice {
    pub fn new(archive: Arc<RpfArchive>) -> Self {
        Self {
            archive,
            sub_root: String::new(),
            label: String::new(),
            nested_cache: Arc::new(Mutex::new(NestedArchives::default())),
        }
    }

    pub fn with_sub_root(archive: Arc<RpfArchive>, sub_root: impl Into<String>) -> Self {
        let sub_root = normalise(&sub_root.into());
        Self {
            archive,
            sub_root,
            label: String::new(),
            nested_cache: Arc::new(Mutex::new(NestedArchives::default())),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn sub_root(&self) -> &str {
        &self.sub_root
    }

    pub fn archive(&self) -> &RpfArchive {
        &self.archive
    }

    /// How many nested archives this device is holding open.
    pub fn nested_cache_len(&self) -> usize {
        self.nested_cache.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// The entries of one directory in archive order; `None` when `relative_dir` names no directory.
    ///
    /// The path matches case-blind and descends through nested archives, as
    /// [`FileDevice::open`] does. Every nested archive listed is opened.
    pub fn entries(&self, relative_dir: &str) -> Option<Vec<ArchiveEntry>> {
        let needle = self.full_path(relative_dir);
        entries_in(&self.archive, &needle, "", &self.nested_cache, 0)
    }

    /// One entry's body in `form`, by its index in this device's own archive, so a renamed or moved entry still reads.
    pub fn read_entry(&self, entry_index: u32, form: BodyForm) -> or_io::Result<Vec<u8>> {
        self.archive.read_form(entry_index, form)
    }

    /// The nested archive entry `entry_index` of this device's own archive holds, opened once and cached.
    pub fn open_nested(&self, entry_index: u32) -> or_io::Result<Arc<RpfArchive>> {
        let node = self.archive.node_for_entry(entry_index).ok_or_else(|| {
            or_io::IoError::RpfBadEntry {
                message: format!("entry {entry_index} names no node"),
            }
        })?;
        open_nested_cached(&self.archive, entry_index, node.path(), &self.nested_cache)
    }

    fn resolve(&self, relative_path: &str) -> Option<Resolved> {
        let needle = self.full_path(relative_path);
        resolve_in(&self.archive, &needle, "", &self.nested_cache, 0)
    }

    fn full_path(&self, relative_path: &str) -> String {
        let rel = normalise(relative_path);
        if self.sub_root.is_empty() {
            return rel;
        }
        if rel.is_empty() {
            return self.sub_root.clone();
        }
        format!("{}/{}", self.sub_root, rel)
    }

    /// The file at `relative_path`, of which a resource reads no more than `limit` bytes from the archive.
    fn read(&self, relative_path: &str, limit: usize) -> Option<Vec<u8>> {
        let resolved = self.resolve(relative_path)?;
        // Resource entries (.ydd/.ydr/.ytd/…) store their RSC7 header in
        // the entry bitfield rather than in the body, so they need the
        // resource path to come back as a standalone file.
        let extracted = match resolved.archive.entries.get(resolved.entry_index as usize) {
            Some(RpfEntry::Resource(_)) => {
                resolved.archive.read_prefix(resolved.entry_index, limit)
            }
            _ => resolved
                .archive
                .read_entry(resolved.entry_index)
                .map(|mut bytes| {
                    bytes.truncate(limit);
                    bytes
                }),
        };
        match extracted {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    archive_path = relative_path,
                    sub_root = %self.sub_root,
                    "ArchiveDevice extract failed"
                );
                None
            }
        }
    }
}

impl FileDevice for ArchiveDevice {
    fn open(&self, relative_path: &str) -> Option<Vec<u8>> {
        self.read(relative_path, usize::MAX)
    }

    fn open_head(&self, relative_path: &str, limit: usize) -> Option<Vec<u8>> {
        self.read(relative_path, limit)
    }

    fn exists(&self, relative_path: &str) -> bool {
        self.resolve(relative_path).is_some()
    }

    fn locate(&self, relative_path: &str) -> Option<String> {
        let resolved = self.resolve(relative_path)?;
        let inside = resolved.inside;
        if self.label.is_empty() {
            Some(inside)
        } else {
            Some(format!("{}/{inside}", self.label))
        }
    }

    fn list(&self, relative_dir: &str) -> Vec<String> {
        let needle = self.full_path(relative_dir);
        list_in(&self.archive, &needle, &self.nested_cache, 0)
    }

    fn walk(&self) -> Option<Vec<String>> {
        let mut out = Vec::new();
        walk_in(
            &self.archive,
            "",
            &self.sub_root,
            &self.nested_cache,
            0,
            &mut out,
        );
        Some(out)
    }

    fn invalidate(&self) {
        if let Ok(mut guard) = self.nested_cache.lock() {
            guard.clear();
        }
    }
}

/// Every file under this archive, and under every archive inside it.
///
/// ⚠ **A nested `.rpf` is both a file and a boundary**, so it is emitted *and*
/// descended into. A walk that only descended would lose the archive from the
/// listing, which is the one entry an explorer most needs to show; a walk that
/// only emitted would stop at the first boundary.
///
/// The `depth` cap is [`MAX_NEST_DEPTH`], the same one `resolve_in` uses — it
/// is the cycle guard as well as the depth limit, because an archive that
/// contained itself would otherwise recurse until the stack ran out.
fn walk_in(
    archive: &Arc<RpfArchive>,
    prefix: &str,
    sub_root: &str,
    cache: &NestedCache,
    depth: u32,
    out: &mut Vec<String>,
) {
    for node in &archive.nodes {
        if node.is_dir() {
            continue;
        }
        let path = normalise(node.path());
        let below = if sub_root.is_empty() {
            Some(path.as_str())
        } else if path == sub_root {
            Some("")
        } else {
            path.strip_prefix(sub_root)
                .and_then(|r| r.strip_prefix('/'))
        };
        let Some(below) = below else { continue };
        if below.is_empty() {
            continue;
        }
        let full = join_display(prefix, below);
        out.push(full.clone());

        if is_archive_name(below) && depth < MAX_NEST_DEPTH {
            if let Ok(nested) = open_nested_cached(archive, node.entry_index(), &path, cache) {
                walk_in(&nested, &full, "", cache, depth + 1, out);
            }
        }
    }
}

fn is_archive_name(path: &str) -> bool {
    super::path::is_archive_segment(path)
}

fn resolve_in(
    archive: &Arc<RpfArchive>,
    remaining: &str,
    base_display: &str,
    cache: &NestedCache,
    depth: u32,
) -> Option<Resolved> {
    for node in &archive.nodes {
        if node.is_dir() {
            continue;
        }
        if normalise(node.path()) == remaining {
            return Some(Resolved {
                archive: Arc::clone(archive),
                entry_index: node.entry_index(),
                inside: join_display(base_display, node.path()),
            });
        }
    }

    if depth >= MAX_NEST_DEPTH {
        return None;
    }

    for node in &archive.nodes {
        if node.is_dir() {
            continue;
        }
        let np = normalise(node.path());
        if !np.ends_with(".rpf") {
            continue;
        }
        let Some(rest) = remaining
            .strip_prefix(&np)
            .and_then(|r| r.strip_prefix('/'))
        else {
            continue;
        };
        let nested_display = join_display(base_display, node.path());
        let nested =
            open_nested_cached(archive, node.entry_index(), &nested_display, cache).ok()?;
        return resolve_in(&nested, rest, &nested_display, cache, depth + 1);
    }

    None
}

fn list_in(archive: &Arc<RpfArchive>, dir: &str, cache: &NestedCache, depth: u32) -> Vec<String> {
    if depth < MAX_NEST_DEPTH {
        for node in &archive.nodes {
            if node.is_dir() {
                continue;
            }
            let np = normalise(node.path());
            if !np.ends_with(".rpf") {
                continue;
            }
            let remainder = if dir == np {
                Some(String::new())
            } else {
                dir.strip_prefix(&np)
                    .and_then(|r| r.strip_prefix('/'))
                    .map(str::to_string)
            };
            if let Some(remainder) = remainder {
                let nested_display = node.path().to_string();
                if let Ok(nested) =
                    open_nested_cached(archive, node.entry_index(), &nested_display, cache)
                {
                    return list_in(&nested, &remainder, cache, depth + 1);
                }
                return Vec::new();
            }
        }
    }

    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut out = Vec::new();
    for node in &archive.nodes {
        let path = normalise(node.path());
        if path == dir {
            continue;
        }
        let Some(rest) = path.strip_prefix(&prefix) else {
            if dir.is_empty() && !path.contains('/') {
                out.push(path);
            }
            continue;
        };
        if !rest.contains('/') && !rest.is_empty() {
            out.push(rest.to_string());
        }
    }
    out
}

fn entries_in(
    archive: &Arc<RpfArchive>,
    dir: &str,
    base_display: &str,
    cache: &NestedCache,
    depth: u32,
) -> Option<Vec<ArchiveEntry>> {
    let children_of = |node: &RpfNode| {
        node.children()
            .iter()
            .filter_map(|&i| archive.nodes.get(i as usize))
            .collect::<Vec<_>>()
    };
    let mut dir_node = archive.nodes.first()?;
    let mut segments = dir.split('/').filter(|s| !s.is_empty());
    while let Some(segment) = segments.next() {
        let child = children_of(dir_node)
            .into_iter()
            .find(|n| n.name().eq_ignore_ascii_case(segment))?;
        if child.is_dir() {
            dir_node = child;
            continue;
        }
        if !is_archive_name(child.name()) || depth >= MAX_NEST_DEPTH {
            return None;
        }
        let display = join_display(base_display, child.path());
        let nested = open_nested_cached(archive, child.entry_index(), &display, cache).ok()?;
        let rest = segments.collect::<Vec<_>>().join("/");
        return entries_in(&nested, &rest, &display, cache, depth + 1);
    }

    let entries = children_of(dir_node)
        .into_iter()
        .map(|node| {
            let kind = match node {
                RpfNode::Directory { .. } => ArchiveEntryKind::Folder,
                RpfNode::BinaryFile { .. } => ArchiveEntryKind::File,
                RpfNode::ResourceFile { .. } => ArchiveEntryKind::Resource,
            };
            let nested = if kind == ArchiveEntryKind::File
                && is_archive_name(node.name())
                && depth < MAX_NEST_DEPTH
            {
                let display = join_display(base_display, node.path());
                open_nested_cached(archive, node.entry_index(), &display, cache).ok()
            } else {
                None
            };
            ArchiveEntry {
                name: node.name().to_string(),
                kind,
                entry_index: node.entry_index(),
                archive: nested,
            }
        })
        .collect();
    Some(entries)
}

fn open_nested_cached(
    parent: &Arc<RpfArchive>,
    entry_index: u32,
    nested_display: &str,
    cache: &NestedCache,
) -> or_io::Result<Arc<RpfArchive>> {
    let key = normalise(nested_display);
    if let Ok(mut guard) = cache.lock() {
        if let Some(existing) = guard.get(&key) {
            return Ok(existing);
        }
    }
    let nested = Arc::new(parent.open_nested(entry_index)?);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, Arc::clone(&nested));
    }
    Ok(nested)
}

fn join_display(base: &str, segment: &str) -> String {
    if base.is_empty() {
        segment.replace('\\', "/")
    } else {
        format!("{}/{}", base, segment.replace('\\', "/"))
    }
}

fn normalise(path: &str) -> String {
    path.replace('\\', "/")
        .trim_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_lowercases_and_normalises_slashes() {
        assert_eq!(normalise("Data\\Effects\\Foo.dat"), "data/effects/foo.dat");
        assert_eq!(normalise("/leading/and/trailing/"), "leading/and/trailing");
        assert_eq!(normalise(""), "");
    }

    fn archive_of(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut builder = or_io::container::rpf::RpfBuilder::default();
        for (path, bytes) in entries {
            builder
                .insert_with(path, bytes.clone(), false)
                .expect("add");
        }
        builder.to_bytes().expect("save")
    }

    /// `outer` holds a folder, a binary file, a resource and `mid.rpf`.
    fn listed_device() -> ArchiveDevice {
        let resource = [
            or_io::container::resource::Rsc7::header_from_entry_flags(0x10, 0x20).header_bytes(),
            vec![0u8; 32],
        ]
        .concat();
        let mid = archive_of(&[("mid.ydr", b"two".to_vec())]);
        let outer = archive_of(&[
            ("data/also.ydr", b"one-deeper".to_vec()),
            ("top.ymt", b"one".to_vec()),
            ("model.ydr", resource),
            ("mid.rpf", mid),
        ]);
        let mut archive = RpfArchive::from_bytes(outer).expect("the built archive opens");
        // The builder lowercases every name, so the mixed case is put back here.
        for node in &mut archive.nodes {
            if let RpfNode::BinaryFile { name, path, .. } = node {
                if name == "top.ymt" {
                    *name = "Top.YMT".to_string();
                    *path = "Top.YMT".to_string();
                }
            }
        }
        ArchiveDevice::new(Arc::new(archive))
    }

    #[test]
    fn a_directory_lists_names_kinds_entry_indices_and_nested_archives() {
        let device = listed_device();
        let root = device.entries("").expect("the root is a directory");

        let by_name = |name: &str| {
            root.iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} is not listed: {root:?}"))
        };
        assert_eq!(by_name("data").kind, ArchiveEntryKind::Folder);
        assert_eq!(by_name("Top.YMT").kind, ArchiveEntryKind::File);
        assert_eq!(by_name("model.ydr").kind, ArchiveEntryKind::Resource);
        assert_eq!(by_name("mid.rpf").kind, ArchiveEntryKind::File);
        assert_eq!(root.len(), 4, "{root:?}");

        for entry in &root {
            let node = device
                .archive()
                .node_for_entry(entry.entry_index)
                .expect("the entry index names a node");
            assert_eq!(node.name(), entry.name, "entry {}", entry.entry_index);
        }

        let nested = by_name("mid.rpf")
            .archive
            .as_ref()
            .expect("an archive entry carries its opened archive");
        assert!(
            nested.nodes.iter().any(|n| n.name() == "mid.ydr"),
            "the handle is the nested archive"
        );
        assert!(
            root.iter()
                .filter(|e| e.name != "mid.rpf")
                .all(|e| e.archive.is_none()),
            "only an archive entry carries a handle"
        );
    }

    #[test]
    fn an_entry_reads_by_index_and_a_nested_archive_opens_once() {
        let device = listed_device();
        let root = device.entries("").expect("the root is a directory");
        let entry = |name: &str| {
            root.iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} is not listed: {root:?}"))
        };

        let model = device
            .read_entry(entry("model.ydr").entry_index, BodyForm::Decoded)
            .expect("a resource reads");
        assert_eq!(
            &model[..4],
            b"RSC7",
            "its header comes from the entry table"
        );
        assert_eq!(
            Some(model),
            device.open("model.ydr"),
            "as a path read gives it"
        );
        assert_eq!(
            device
                .read_entry(entry("Top.YMT").entry_index, BodyForm::Decoded)
                .expect("a file reads"),
            b"one"
        );

        let mid = device
            .open_nested(entry("mid.rpf").entry_index)
            .expect("a nested archive opens");
        let listed = entry("mid.rpf").archive.as_ref().expect("listed open");
        assert!(Arc::ptr_eq(&mid, listed), "the listing's open is reused");
        assert_eq!(device.nested_cache_len(), 1);
        assert!(
            device.open_nested(entry("Top.YMT").entry_index).is_err(),
            "a file that is no archive says why"
        );
    }

    #[test]
    fn a_listing_descends_folders_and_nested_archives_case_blind() {
        let device = listed_device();
        let data = device
            .entries("DATA")
            .expect("a folder, matched case-blind");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].name, "also.ydr");

        let mid = device.entries("mid.rpf").expect("a nested archive lists");
        assert_eq!(mid.len(), 1);
        assert_eq!(mid[0].name, "mid.ydr");

        assert!(
            device.entries("top.ymt").is_none(),
            "a file is no directory"
        );
        assert!(device.entries("nowhere").is_none());
    }
}
