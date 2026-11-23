//! An open document's contents as one VFS, so the app can ask a path
//! question that crosses an archive boundary without walking archives itself,
//! and the search of every archive, which reads the file index.
//!
//! ⚠ **The device is built over the archive the session already opened**, not
//! over a path re-read from disk. `RpfEditSession` holds an
//! `Arc<RpfArchive>` per registered archive, which holds the keys that
//! opened it, and that is exactly [`ArchiveDevice`]'s constructor — so the
//! reader here is the same object the explorer is showing, which a second
//! `RpfArchive::open` would quietly break.

use std::collections::HashMap;
use std::sync::Arc;

use or_title::vfs::{is_archive_segment, ArchiveDevice, FolderDevice, GlobResult, Pattern, Vfs};

use super::document::DocumentId;
use super::memory::{hash_table_bytes, vec_bytes};
use super::navigation::Arrival;
use super::rpf_node::RpfNodeBacking;
use super::rpf_session::RpfEditSession;
use super::Selection;

/// The mount an open document's own contents answer to.
pub const DOCUMENT_MOUNT: &str = "doc:";

/// A glob's matches in one document, read from the file index as far as it has walked.
#[derive(Debug, Clone)]
pub struct DeepFind {
    pub pattern: String,
    matches: Vec<String>,
    /// The listing the matches were read from, and how much of it.
    generation: u64,
    read: usize,
    /// The index is still walking this document's files.
    pub indexing: bool,
    /// What the walk could not open or read.
    ///
    /// ⚠ **Empty is a claim, not a default**: matches read past an archive
    /// that would not open look exhaustive while missing everything in it.
    pub warnings: Vec<String>,
}

impl DeepFind {
    fn new(pattern: &str, generation: u64) -> Self {
        Self {
            pattern: pattern.to_string(),
            matches: Vec::new(),
            generation,
            read: 0,
            indexing: false,
            warnings: Vec::new(),
        }
    }

    /// The pattern, the matches and the warnings, each string at its capacity.
    fn held_bytes(&self) -> usize {
        let matches: usize = self.matches.iter().map(String::capacity).sum();
        let warnings: usize = self.warnings.iter().map(String::capacity).sum();
        self.pattern.capacity()
            + vec_bytes(&self.matches)
            + matches
            + vec_bytes(&self.warnings)
            + warnings
    }

    /// Did this find look everywhere the document holds?
    pub fn is_complete(&self) -> bool {
        !self.indexing && self.warnings.is_empty()
    }

    pub fn matches(&self) -> &[String] {
        &self.matches
    }

    /// How many files of the document it has looked at.
    pub fn read(&self) -> usize {
        self.read
    }
}

/// One document's contents behind one mount, ready to be walked.
pub struct Mounted {
    vfs: Vfs,
    /// Does this mount open the archives it lists?
    descends: bool,
}

/// Mount a document's contents.
///
/// `None` when the root is neither an opened archive nor a folder on disk —
/// an unsaved tree built in memory has nothing for a device to read.
pub fn mount_document(session: &RpfEditSession) -> Option<Mounted> {
    let root = session.node(session.root_id())?;
    let mut vfs = Vfs::new();
    let descends = match &root.backing {
        RpfNodeBacking::ArchiveDir { archive_id } => {
            let slot = session.archive(*archive_id)?;
            let device = ArchiveDevice::new(Arc::clone(&slot.archive));
            vfs.mount(DOCUMENT_MOUNT, Arc::new(device));
            true
        }
        RpfNodeBacking::DiskFolder { fs_path, .. } => {
            vfs.mount(DOCUMENT_MOUNT, Arc::new(FolderDevice::new(fs_path.clone())));
            false
        }
        _ => return None,
    };
    Some(Mounted { vfs, descends })
}

/// Run one glob over a mounted document.
///
/// Free rather than a method because it runs on a worker, and nothing on that
/// path may touch app state.
pub fn find(mounted: &Mounted, pattern: &str) -> (GlobResult, Vec<String>) {
    let result = mounted.vfs.glob(pattern);
    let undescended = if mounted.descends {
        Vec::new()
    } else {
        let mut archives: Vec<String> = mounted
            .vfs
            .glob("**/*.rpf")
            .matches
            .into_iter()
            .filter(|path| {
                path.rsplit(['/', '\\'])
                    .next()
                    .is_some_and(is_archive_segment)
            })
            .collect();
        archives.sort();
        archives
    };
    (result, undescended)
}

/// The search of every archive, per document, so two documents each keep their own.
#[derive(Debug, Default)]
pub struct VfsSession {
    found: HashMap<DocumentId, DeepFind>,
    /// The matches selected in the list, by index.
    pub selection: Selection,
}

impl VfsSession {
    /// What is already on screen for this document and pattern, if anything.
    pub fn found_for(&self, doc_id: DocumentId, pattern: &str) -> Option<&DeepFind> {
        self.found.get(&doc_id).filter(|d| d.pattern == pattern)
    }

    pub fn clear(&mut self) {
        self.found.clear();
        self.selection = Selection::default();
    }

    /// What every document's matches hold: each path and each warning.
    pub fn held_bytes(&self) -> usize {
        let finds: usize = self.found.values().map(DeepFind::held_bytes).sum();
        hash_table_bytes::<(DocumentId, DeepFind)>(self.found.capacity()) + finds
    }
}

impl super::AppState {
    /// Read `pattern`'s matches in this document from the file index, as far as its walk has come.
    ///
    /// Called every repaint: only the files the index found since the last call are matched.
    pub fn deep_find_tick(&mut self, doc_id: DocumentId, pattern: &str) {
        self.tick_file_index();
        let root = self.file_index.root_of(doc_id);
        let generation = root.map_or(0, |root| root.generation);
        let found = self
            .deep_find
            .found
            .entry(doc_id)
            .or_insert_with(|| DeepFind::new(pattern, generation));
        let files = root.map_or(&[][..], |root| &root.listing.files[..]);
        if found.pattern != pattern || found.generation != generation || found.read > files.len() {
            *found = DeepFind::new(pattern, generation);
        }
        if found.read < files.len() {
            let glob = Pattern::new(pattern);
            let fresh = files[found.read..]
                .iter()
                .filter(|path| glob.matches_path(path));
            found
                .matches
                .extend(fresh.map(|path| format!("{DOCUMENT_MOUNT}/{path}")));
            found.read = files.len();
        }
        found.indexing = root.is_some_and(|root| root.is_walking());
        // A listing's warnings arrive whole at the end of its walk, so their count says when to copy them.
        let warnings = root.map_or(&[][..], |root| &root.listing.warnings[..]);
        if found.warnings.len() != warnings.len() {
            found.warnings = warnings.to_vec();
        }
    }

    /// Go to the matches at `rows` of this document's find, then do what `then` says.
    pub fn go_to_deep_find_matches(&mut self, doc_id: DocumentId, rows: &[usize], then: Arrival) {
        let Some(source) = self.document(doc_id).and_then(|doc| doc.path.clone()) else {
            return;
        };
        let Some(found) = self.deep_find.found.get(&doc_id) else {
            return;
        };
        let inners = rows
            .iter()
            .filter_map(|&row| found.matches().get(row))
            .map(|path| inner_path(path).to_string())
            .collect();
        self.walk_to(&source, inners, then);
    }
}

/// A match's path inside the document searched, without the mount it was found through.
pub fn inner_path(path: &str) -> &str {
    path.strip_prefix(DOCUMENT_MOUNT)
        .unwrap_or(path)
        .trim_start_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;
    use or_io::container::rpf::RpfBuilder;

    fn archive_of(entries: &[(&str, &[u8])]) -> RpfBuilder {
        let mut builder = RpfBuilder::default();
        for (path, bytes) in entries {
            builder
                .insert_with(path, *bytes, false)
                .unwrap_or_else(|e| panic!("add {path}: {e}"));
        }
        builder
    }

    /// `outer.rpf` → `mid.rpf` → `inner.rpf`, a `.ydr` at every level.
    fn nested_archive() -> RpfBuilder {
        let inner = archive_of(&[("deep.ydr", b"three"), ("deep.ytd", b"not-a-drawable")])
            .to_bytes()
            .expect("save");
        let mid = archive_of(&[("mid.ydr", b"two"), ("inner.rpf", &inner)])
            .to_bytes()
            .expect("save");
        archive_of(&[
            ("top.ydr", b"one"),
            ("data/nested/also.ydr", b"one-deeper"),
            ("mid.rpf", &mid),
        ])
    }

    fn nested_session() -> RpfEditSession {
        let archive = nested_archive().build().expect("the built archive opens");
        RpfEditSession::from_archive(archive).expect("session over the built archive")
    }

    /// A session opened from an archive holds nodes for its *own* entries and
    /// leaves each nested `.rpf` an unexpanded `Archive { scanned: false }`
    /// placeholder until a user clicks it. So `deep.ydr`, two boundaries
    /// down, is reachable through the VFS and **is not in the session's node
    /// tree at all** — which is what makes this fail against any glob
    /// implemented over that tree rather than over the mount.
    #[test]
    fn the_glob_reaches_a_drawable_the_session_tree_does_not_hold() {
        let session = nested_session();

        let names: Vec<String> = session
            .all_nodes()
            .into_iter()
            .filter_map(|id| session.node(id).map(|n| n.name.clone()))
            .collect();
        assert!(
            !names.iter().any(|n| n == "deep.ydr"),
            "the session expanded the nested archive by itself, so this test \
             can no longer tell a VFS walk from a tree walk: {names:?}"
        );

        let mounted = mount_document(&session).expect("an archive-rooted document mounts");
        assert!(mounted.descends, "an archive root opens what it lists");
        let (result, undescended) = find(&mounted, "**/*.ydr");

        assert!(
            result.is_complete(),
            "a mount could not be walked: {:?}",
            result.unwalkable
        );
        assert!(undescended.is_empty(), "{undescended:?}");

        let mut matches = result.matches.clone();
        matches.sort();
        assert_eq!(
            matches,
            vec![
                "doc:/data/nested/also.ydr",
                "doc:/mid.rpf/inner.rpf/deep.ydr",
                "doc:/mid.rpf/mid.ydr",
                "doc:/top.ydr",
            ],
            "the app's mount must reach every depth the library's does"
        );
    }

    /// ⚠ The anti-vacuity half: a glob returning everything it walked would
    /// pass the test above, because the wanted four are a subset.
    #[test]
    fn a_pattern_that_matches_nothing_returns_nothing() {
        let mounted = mount_document(&nested_session()).expect("mounts");
        assert!(find(&mounted, "**/*.ybn").0.matches.is_empty());
        assert!(find(&mounted, "nowhere/**/*.ydr").0.matches.is_empty());
        assert_eq!(
            find(&mounted, "*.ydr").0.matches,
            vec!["doc:/top.ydr"],
            "one star must not cross a separator or an archive boundary"
        );
    }

    /// ⚠ A folder walk lists a `.rpf` and does not open it, so a find over a
    /// game root is **not** exhaustive and has to say so.
    #[test]
    fn a_folder_root_reports_the_archives_it_did_not_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("loose.ydr"), b"loose").unwrap();
        nested_archive()
            .save(dir.path().join("packed.rpf"))
            .unwrap();

        let session =
            RpfEditSession::from_game_root(dir.path().to_path_buf(), Arc::new(NoOpCryptoProvider));
        let mounted = mount_document(&session).expect("a folder-rooted document mounts");
        assert!(
            !mounted.descends,
            "a folder device has no crypto provider, so it cannot open a .rpf"
        );
        let (result, undescended) = find(&mounted, "**/*.ydr");

        assert_eq!(
            result.matches,
            vec!["doc:/loose.ydr"],
            "the loose drawable is found and the packed ones are not"
        );
        assert_eq!(
            undescended,
            vec!["doc:/packed.rpf"],
            "an archive listed and not opened must be named, or a find over a \
             game root looks exhaustive while missing everything in it"
        );
    }

    #[test]
    fn a_session_with_no_backing_mounts_nothing() {
        assert!(mount_document(&RpfEditSession::empty()).is_none());
    }
}
