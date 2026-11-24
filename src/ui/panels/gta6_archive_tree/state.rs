//! Panel state for the GTA VI archive explorer: the archive the user chose and the listing read
//! from it. The explorer reads an RPF9 archive's deciphered table of contents and shows its entries;
//! the key snapshot and the recovered-names corpus are game-derived and read at runtime, so when
//! neither is present the panel degrades to hashes or to an empty list rather than failing. The key
//! snapshot and the names corpus are loaded once and reused across clicks, because unmasking a masked
//! snapshot reads a large executable.

use std::path::{Path, PathBuf};

use or_io::container::rpf::{Gta6Archive, GtaViKeys, NameResolver, Rpf9Header, RPF9_HEADER_SIZE};

use crate::ui::panel_state::PanelState;

/// The build root to list archives from, and the recovered-names folder, both game-derived and read
/// at runtime from the environment the owner's GTA VI sessions already set.
pub(super) const BUILD_ENV: &str = "OPENRAGE_GTA6_BUILD";
const NAMES_ENV: &str = "OPENRAGE_GTA6_NAMES";

/// One entry as the explorer shows it: its row, its name64 hash, the recovered path when known, and
/// the descriptor's size field.
#[derive(Debug, Clone)]
pub struct EntryRow {
    pub index: u32,
    pub name64: u64,
    pub name: Option<String>,
    pub size: Option<u32>,
}

/// One index group as the explorer shows it.
#[derive(Debug, Clone)]
pub struct GroupRow {
    pub name_hash: u64,
    pub count: u32,
    pub width: u16,
    pub external: bool,
}

/// A read-ready listing of one opened archive; holds no `or-io` borrow, so it survives across frames.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub name: String,
    /// The entry count the header states.
    pub entry_count: u32,
    pub key_id: u32,
    /// The rows listed, at most [`Self::entry_count`] (the engine deduplicates columns).
    pub entries: Vec<EntryRow>,
    pub groups: Vec<GroupRow>,
}

/// The explorer's panel state: which archive is shown, the listing read from it, and the key snapshot
/// and names corpus held across clicks so neither is reloaded per open.
// ⚠ Not `Debug`: `GtaViKeys` carries the cipher tables and does not implement it.
#[derive(Default)]
pub struct Gta6ArchiveExplorer {
    /// The build root's archive file names, read once and cached (the folder is not re-walked every
    /// frame); `None` until the panel first lists them.
    pub listed: Option<Vec<String>>,
    /// The archive file currently shown, if any.
    pub selected: Option<String>,
    /// The listing of [`Self::selected`], when it opened.
    pub listing: Option<Listing>,
    /// Why the last open failed, shown in place of a listing.
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

    /// Open `name` (a file under the build root), decipher its table of contents and keep the
    /// listing; records the error instead when it cannot be opened.
    pub fn open(&mut self, name: &str) {
        self.selected = Some(name.to_string());
        self.listing = None;
        self.error = None;
        let Some(root) = Self::build_root() else {
            self.error = Some(format!("{BUILD_ENV} names no build folder"));
            return;
        };
        let bytes = match read_toc_prefix(&root.join(name)) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let listing = {
            let keys = match self.keys.get_or_insert_with(load_keys) {
                Ok(keys) => keys,
                Err(e) => {
                    self.error = Some(e.clone());
                    return;
                }
            };
            let resolver = self.resolver.get_or_insert_with(load_resolver);
            build_listing(name, &bytes, keys, resolver)
        };
        match listing {
            Ok(listing) => self.listing = Some(listing),
            Err(e) => self.error = Some(e),
        }
    }
}

/// The key snapshot the install supplies, or a message saying none is set.
fn load_keys() -> Result<GtaViKeys, String> {
    match GtaViKeys::for_install(or_title::config::config_dir().as_deref()) {
        Ok(Some(keys)) => Ok(keys),
        Ok(None) => Err("no GTA VI key snapshot (set OPENRAGE_GTA6_KEYS)".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// The recovered-names corpus from the folder [`NAMES_ENV`] names, or an empty resolver.
fn load_resolver() -> NameResolver {
    let mut resolver = NameResolver::new();
    if let Some(dir) = std::env::var_os(NAMES_ENV).map(PathBuf::from) {
        if let Ok(text) = std::fs::read_to_string(dir.join("toc-entries-resolved.tsv")) {
            resolver.add_toc_entries_tsv(&text);
        }
        if let Ok(text) = std::fs::read_to_string(dir.join("resolved-names.tsv")) {
            resolver.add_hash_name_tsv(&text);
        }
    }
    resolver
}

/// Read only the header and table of contents of the archive at `path` — a build archive can be many
/// gigabytes. The 64-byte header is parsed and validated before its length is trusted, so a non-RPF9
/// or corrupt `*.rpf` under the build root cannot drive a multi-gigabyte allocation before `open`
/// would refuse it.
fn read_toc_prefix(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut head = [0u8; RPF9_HEADER_SIZE];
    file.read_exact(&mut head)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let header = Rpf9Header::parse(&head).map_err(|e| e.to_string())?;
    let toc_end = u64::from(header.toc_end);
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();
    if toc_end > file_len {
        return Err(format!(
            "{}: its table of contents ends at {toc_end}, past the {file_len}-byte file",
            path.display()
        ));
    }
    let want = usize::try_from(toc_end).map_err(|e| e.to_string())?;
    let mut bytes = vec![0u8; want];
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.read_exact(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes)
}

/// Open `bytes`, resolve what names the corpus has, and build a read-ready listing.
fn build_listing(
    name: &str,
    bytes: &[u8],
    keys: &GtaViKeys,
    resolver: &NameResolver,
) -> Result<Listing, String> {
    let mut archive = Gta6Archive::open(bytes, keys).map_err(|e| e.to_string())?;
    archive.resolve_names(resolver);
    Ok(Listing {
        name: name.to_string(),
        entry_count: archive.toc.entry_count,
        key_id: archive.toc.key_id,
        entries: archive
            .entries
            .iter()
            .map(|e| EntryRow {
                index: e.index,
                name64: e.name64,
                name: e.name.clone(),
                size: e.size(),
            })
            .collect(),
        groups: archive
            .groups()
            .iter()
            .map(|g| GroupRow {
                name_hash: g.name_hash,
                count: g.count,
                width: g.width,
                external: g.external,
            })
            .collect(),
    })
}
