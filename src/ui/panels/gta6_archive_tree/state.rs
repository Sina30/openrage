//! Panel state for the GTA VI archive explorer: the build's archives and, for the ones the user has
//! opened, a lazily-mounted tree of their deciphered entries. An entry whose body is itself an RPF9
//! archive (two-thirds of a build's are) can be expanded in place, which mounts its table from the
//! parent's row; a leaf entry opens on a double-click through the app's existing scan-body views.
//!
//! The key snapshot and the recovered-names corpus are game-derived and read at runtime, so when
//! neither is present the panel degrades to hashes or to an empty list rather than failing. Both are
//! loaded once and reused, because unmasking a masked snapshot reads a large executable.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use or_io::container::rpf::{open_at, GtaViKeys, NameResolver, RPF9_MAGIC};
use or_io::gta6::names::{vocabulary_file, Vocabulary};

use crate::ui::panel_state::PanelState;

/// The build root to list archives from, and the recovered-names folder, both game-derived and read
/// at runtime from the environment the owner's GTA VI sessions already set.
pub(super) const BUILD_ENV: &str = "OPENRAGE_GTA6_BUILD";
const NAMES_ENV: &str = "OPENRAGE_GTA6_NAMES";

/// One entry as the tree shows it: its row, name64 hash, recovered path, real size, and whether its
/// body begins a nested RPF9 archive (so it can be expanded in place).
#[derive(Debug, Clone)]
pub struct EntryRow {
    pub index: u32,
    pub name64: u64,
    pub name: Option<String>,
    pub size: Option<u32>,
    /// The entry's absolute byte offset in the file, where its row states a start.
    pub absolute_start: Option<u64>,
    /// Whether the body at [`Self::absolute_start`] begins with the clear `9FPR` header.
    pub is_archive: bool,
    /// Whether the entry's body is enciphered (it then cannot be opened).
    pub cipher: bool,
}

impl EntryRow {
    /// The recovered path, or `hash_{name64:016x}` where it never resolved — the tree's fallback.
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("hash_{:016x}", self.name64),
        }
    }
}

/// One mounted archive node: its deciphered entries and, lazily, the child nodes for the
/// nested archives the user has expanded. `base` is the node's absolute header offset in the file, so
/// an entry's absolute start is `base + row.start`; the rows already carry that sum.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub key_id: u32,
    pub rows: Vec<EntryRow>,
    /// The expanded nested archives, keyed by the entry index they sit at; loaded on first expand.
    pub children: BTreeMap<u32, Node>,
    /// Which entries are expanded (an archive row toggles this; the child is in [`Self::children`]).
    pub open: BTreeSet<u32>,
}

/// The explorer's panel state: which archive is shown, the tree mounted from it, and the key snapshot
/// and names corpus held across clicks so neither is reloaded per open.
// ⚠ Not `Debug`: `GtaViKeys` carries the cipher tables and does not implement it.
#[derive(Default)]
pub struct Gta6ArchiveExplorer {
    /// The build root's archive file names, read once and cached; `None` until first listed.
    pub listed: Option<Vec<String>>,
    /// The archive file currently shown, if any (a name under the build root).
    pub selected: Option<String>,
    /// The root node of [`Self::selected`], when it opened.
    pub root: Option<Node>,
    /// Why the last open failed, shown in place of a tree.
    pub error: Option<String>,
    /// The key snapshot, loaded once (loading may read a large executable to unmask a masked one).
    keys: Option<Result<GtaViKeys, String>>,
    /// The recovered-names corpus, loaded once.
    resolver: Option<NameResolver>,
}

impl PanelState for Gta6ArchiveExplorer {
    const KEY: &'static str = "gta6_archive_tree";
}

impl Gta6ArchiveExplorer {
    /// The build root the explorer lists archives from, or `None` when the environment names none.
    pub fn build_root() -> Option<PathBuf> {
        std::env::var_os(BUILD_ENV)
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
    }

    /// The `*.rpf` files directly under the build root, by file name, sorted.
    pub fn archives() -> Vec<String> {
        let Some(root) = Self::build_root() else {
            return Vec::new();
        };
        let mut names: Vec<String> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                (path.extension().and_then(|x| x.to_str()) == Some("rpf"))
                    .then(|| path.file_name()?.to_str().map(str::to_owned))
                    .flatten()
            })
            .collect();
        names.sort();
        names
    }

    /// The cached archive list, walking the build root once on first use.
    pub fn ensure_listed(&mut self) -> &[String] {
        self.listed.get_or_insert_with(Self::archives).as_slice()
    }

    /// The disk path of the archive currently shown, if any.
    pub fn selected_path(&self) -> Option<PathBuf> {
        Some(Self::build_root()?.join(self.selected.as_ref()?))
    }

    /// Open `name` (a file under the build root), mount its top-level table and keep the root node;
    /// records the error instead when it cannot be opened.
    pub fn open(&mut self, name: &str) {
        self.selected = Some(name.to_string());
        self.root = None;
        self.error = None;
        let Some(root) = Self::build_root() else {
            self.error = Some(format!("{BUILD_ENV} names no build folder"));
            return;
        };
        let path = root.join(name);
        match self.mount(&path, 0) {
            Ok(node) => self.root = Some(node),
            Err(e) => self.error = Some(e),
        }
    }

    /// Expand or collapse the nested archive at entry `path_indices` within the open tree, mounting
    /// its child node on first expand. `path_indices` is the chain of entry indices from the root.
    pub fn toggle(&mut self, path_indices: &[u32]) {
        // A toggle clears a stale error from an earlier failed expand, so one bad nested mount does not
        // leave the message up for good; a failure below sets it again.
        self.error = None;
        let Some(disk) = self.selected_path() else {
            return;
        };
        // Resolve the parent node and the child's absolute base before borrowing mutably for long.
        let Some((&entry_index, parent_path)) = path_indices.split_last() else {
            return;
        };
        let base = {
            let Some(parent) = self.node_at(parent_path) else {
                return;
            };
            if parent.open.contains(&entry_index) {
                // Collapsing: drop the open mark; the child node is kept so re-expanding is instant.
                None
            } else {
                parent
                    .rows
                    .iter()
                    .find(|r| r.index == entry_index && r.is_archive)
                    .and_then(|r| r.absolute_start)
            }
        };
        match base {
            // Collapse.
            None => {
                if let Some(parent) = self.node_at_mut(parent_path) {
                    parent.open.remove(&entry_index);
                }
            }
            // Expand: mount the child once, then mark the entry open.
            Some(base) => {
                let child = if self.child_exists(parent_path, entry_index) {
                    Ok(())
                } else {
                    match self.mount(&disk, base) {
                        Ok(node) => {
                            if let Some(parent) = self.node_at_mut(parent_path) {
                                parent.children.insert(entry_index, node);
                            }
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                };
                match child {
                    Ok(()) => {
                        if let Some(parent) = self.node_at_mut(parent_path) {
                            parent.open.insert(entry_index);
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    fn child_exists(&self, parent_path: &[u32], entry_index: u32) -> bool {
        self.node_at(parent_path)
            .is_some_and(|p| p.children.contains_key(&entry_index))
    }

    /// The node at `path_indices` (a chain of entry indices from the root), if mounted.
    pub fn node_at(&self, path_indices: &[u32]) -> Option<&Node> {
        let mut node = self.root.as_ref()?;
        for &index in path_indices {
            node = node.children.get(&index)?;
        }
        Some(node)
    }

    fn node_at_mut(&mut self, path_indices: &[u32]) -> Option<&mut Node> {
        let mut node = self.root.as_mut()?;
        for &index in path_indices {
            node = node.children.get_mut(&index)?;
        }
        Some(node)
    }

    /// Mount the archive whose header is at absolute `base` in `disk`, deciphering its table and
    /// reading each entry's leading magic so an archive row can be told from a leaf. `base` is 0 for
    /// the top-level archive, and the nested header's absolute offset otherwise.
    fn mount(&mut self, disk: &Path, base: u64) -> Result<Node, String> {
        let mut file = File::open(disk).map_err(|e| format!("{}: {e}", disk.display()))?;
        let keys = match self.keys.get_or_insert_with(load_keys) {
            Ok(keys) => keys,
            Err(e) => return Err(e.clone()),
        };
        let resolver = self.resolver.get_or_insert_with(load_resolver);

        let mut archive = open_at(&mut file, base, keys).map_err(|e| e.to_string())?;
        archive.resolve_names(resolver);

        let rows = archive
            .entries
            .iter()
            .map(|e| {
                let absolute_start = e.start.map(|s| base + s);
                let is_archive = absolute_start
                    .map(|off| read_magic(&mut file, off) == Some(RPF9_MAGIC))
                    .unwrap_or(false);
                EntryRow {
                    index: e.index,
                    name64: e.name64,
                    name: e.name.clone(),
                    size: e.size(),
                    absolute_start,
                    is_archive,
                    cipher: e.cipher.is_some(),
                }
            })
            .collect();
        Ok(Node {
            key_id: archive.toc.key_id,
            rows,
            children: BTreeMap::new(),
            open: Default::default(),
        })
    }
}

/// The 4-byte magic at `offset` in `file`, or `None` when it cannot be read.
fn read_magic(file: &mut File, offset: u64) -> Option<u32> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut head = [0u8; 4];
    file.read_exact(&mut head).ok()?;
    Some(u32::from_le_bytes(head))
}

/// The key snapshot the install supplies, or a message saying none is set.
fn load_keys() -> Result<GtaViKeys, String> {
    match GtaViKeys::for_install(or_title::config::config_dir().as_deref()) {
        Ok(Some(keys)) => Ok(keys),
        Ok(None) => Err("no GTA VI key snapshot (set OPENRAGE_GTA6_KEYS)".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// The recovered-names corpus from the folder [`NAMES_ENV`] names, folded together with the vocabulary
/// the app harvested into the config dir, so names found in the user's own archives resolve here too.
fn load_resolver() -> NameResolver {
    let mut resolver = match std::env::var_os(NAMES_ENV).map(PathBuf::from) {
        Some(dir) => NameResolver::load(&dir),
        None => NameResolver::new(),
    };
    if let Some(config) = or_title::config::config_dir() {
        resolver.merge_vocabulary(&Vocabulary::load(&vocabulary_file(&config)));
    }
    resolver
}
