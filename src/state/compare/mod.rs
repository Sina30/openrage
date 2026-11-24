//! Comparing a file with another, an archive with another, or a file or
//! archive in the mods overlay with the game's own copy: both read and lined
//! up on a worker, then shown as a document of their own.
//!
//! ⚠ The game's copy is the same entry in the install's own archive, which
//! the overlay's archive stands in for, and is read from that one archive: a
//! game mount would open every archive and pack to read one entry.

mod align;
mod archive;
mod found;
mod resource;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use or_io::container::rpf::BodyForm;
use or_io::container::{NoOpCryptoProvider, RpfCryptoProvider};
use or_title::vfs::{ArchiveDevice, FileDevice};

pub use found::{By, Change, FieldChange, Found, LinePair};

use super::document::{DocumentId, RpfNodeId};
use super::extract_job::{plan, ExtractItem, TypeFilter};
use super::jobs::{JobKind, Outcome, RunningJob, Then};
use super::rpf_node::{ArchiveSource, RpfNodeBacking};
use super::{document_io, retail, AppState, DocPayload, Document, DocumentKind, RpfEditSession};

/// How many levels of parts a resource comparison reads: more than the inspector's outline, since a comparison lists a difference and cannot leave a deep part to be opened.
pub(super) const DEPTH: usize = 24;

const RUNNING: &str = tr_key!("A comparison is already under way");
const UNSAVED: &str =
    tr_key!("Save the archive's changes first: a comparison reads it as it was last saved");
const DEFRAGMENTING: &str =
    tr_key!("The archive this is read from is being defragmented: compare once that is done");
const SELECTED_DEFRAGMENTING: &str = tr_key!(
    "The archive the selected file is read from is being defragmented: compare once that is done"
);

/// Two files compared: what each side is, and how they differ.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// What heads each side's column: a file's name, or the game and the mods overlay.
    pub headings: [String; 2],
    /// Where each side is.
    pub places: [String; 2],
    pub found: Found,
}

impl Comparison {
    /// The rows Previous and Next difference step between: each change, or the first line of each run of changed lines.
    pub fn stops(&self) -> Vec<usize> {
        match &self.found {
            Found::Fields { changes, .. } => (0..changes.len()).collect(),
            Found::Lines(lines) => (0..lines.len())
                .filter(|&i| lines[i].change.is_some())
                .filter(|&i| i == 0 || lines[i - 1].change.is_none())
                .collect(),
            Found::Bytes { .. } => Vec::new(),
        }
    }

    /// The text it holds, in bytes.
    pub fn held_bytes(&self) -> usize {
        let line = |side: &Option<(usize, String)>| side.as_ref().map_or(0, |(_, text)| text.len());
        match &self.found {
            Found::Fields { changes, .. } => (changes.iter())
                .map(|c| {
                    c.record.len() + c.named.len() + c.field.len() + c.left.len() + c.right.len()
                })
                .sum(),
            Found::Lines(lines) => lines.iter().map(|l| line(&l.left) + line(&l.right)).sum(),
            Found::Bytes { .. } => 0,
        }
    }

    /// What the comparison did not read, where a limit of the outline left parts out: a difference there is not listed.
    pub fn unread(&self) -> Option<&str> {
        match &self.found {
            Found::Fields { unread, .. } => unread.as_deref(),
            _ => None,
        }
    }

    /// How they were compared and how much differs, in a sentence's words.
    pub fn summary(&self) -> String {
        let counted = |n: usize| match n {
            0 => tr!("no differences").to_string(),
            1 => tr!("1 difference").to_string(),
            n => format!("{n} differences"),
        };
        match &self.found {
            Found::Fields { by, changes, .. } => {
                tr!(
                    "by {words}, {count}",
                    words = by.words(),
                    count = counted(changes.len())
                )
            }
            Found::Lines(_) => tr!("by line, {count}", count = counted(self.stops().len())),
            Found::Bytes { same: true, .. } => {
                tr!("by their bytes, which are the same").to_string()
            }
            Found::Bytes { sizes, .. } => tr!(
                "by their bytes, which differ: {size} and {size_2}",
                size = or_job::human_bytes(sizes[0] as u64),
                size_2 = or_job::human_bytes(sizes[1] as u64)
            ),
        }
    }
}

/// The file Select for compare chose, and the comparison running.
#[derive(Default)]
pub struct Comparisons {
    selected: Option<(DocumentId, RpfNodeId)>,
    running: Option<Running>,
}

struct Running {
    label: String,
    receiver: Receiver<Ended>,
    /// The files on disk its sides are read out of: an archive's, at the offsets they were planned with, or the game's.
    reads: Vec<PathBuf>,
    /// Set by a cancel, which the worker looks for between the entries of an archive.
    stop: Arc<AtomicBool>,
}

/// How a comparison's worker ended.
pub(super) enum Ended {
    Compared(Comparison),
    /// The game holds no copy of the overlay's file, which this says.
    NoOriginal(String),
    Failed(String),
}

impl Comparisons {
    /// Run `work` on a worker as the one comparison, named `label` in the job list and on its tab, reading the files in `reads`.
    pub(super) fn start(
        &mut self,
        label: String,
        reads: Vec<PathBuf>,
        work: impl FnOnce(&AtomicBool) -> Ended + Send + 'static,
    ) {
        let (tx, receiver) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        std::thread::spawn(move || {
            let _ = tx.send(work(&stopped));
        });
        self.running = Some(Running {
            label,
            receiver,
            reads,
            stop,
        });
    }

    /// The comparison running, named, and the files on disk it reads.
    pub fn reads(&self) -> Option<(String, &[PathBuf])> {
        let running = self.running.as_ref()?;
        let name = tr!("the comparison of {running}", running = running.label);
        Some((name, &running.reads))
    }

    /// The comparison running, as the job list draws it.
    pub fn job(&self) -> Option<RunningJob> {
        let running = self.running.as_ref()?;
        Some(RunningJob {
            kind: JobKind::Compare,
            doing: tr!("Comparing {running}", running = running.label),
            fraction: None,
            detail: String::new(),
            stopping: false,
        })
    }

    /// Drop the comparison running, whose worker is told to stop; whether one was.
    pub fn cancel(&mut self) -> bool {
        let running = self.running.take();
        if let Some(running) = &running {
            running.stop.store(true, Ordering::Relaxed);
        }
        running.is_some()
    }
}

/// Where one side's bytes are, in a form a worker can hold.
enum Side {
    /// A row of an open install, folder or archive.
    Row(ExtractItem),
    /// The game's own copy: `inner` inside the archive at `file`, or `file` itself.
    Game {
        file: PathBuf,
        inner: Option<String>,
        crypto: Arc<dyn RpfCryptoProvider>,
    },
    /// An archive, which holds no one body: its entries are compared.
    Archive(archive::Archive),
}

impl Side {
    /// The file on disk it is read out of, if it is read out of one.
    fn file(&self) -> Option<PathBuf> {
        match self {
            Side::Row(item) => item.archive_read(),
            Side::Game { file, .. } => Some(file.clone()),
            Side::Archive(archive) => archive.file(),
        }
    }

    /// Its bytes; `None` where the game holds no such file.
    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        let (file, inner, crypto) = match self {
            Side::Row(item) => return item.read(BodyForm::Decoded).map(Some),
            Side::Archive(_) => {
                return Err(tr!("An archive is compared entry by entry").to_string())
            }
            Side::Game {
                file,
                inner,
                crypto,
            } => (file, inner, crypto),
        };
        if !file.is_file() {
            return Ok(None);
        }
        let Some(inner) = inner else {
            return (std::fs::read(file).map(Some)).map_err(|e| format!("{}: {e}", file.display()));
        };
        let archive = super::rpf_archive_open::open_archive_from_disk(file, Arc::clone(crypto))?;
        let device = ArchiveDevice::new(archive);
        if !device.exists(inner) {
            return Ok(None);
        }
        (device.open(inner).map(Some)).ok_or_else(|| {
            tr!(
                "{inner} in {file} could not be read",
                inner = inner,
                file = file.display()
            )
        })
    }
}

/// The file `bytes` hold, opened as its name says, where it is of a kind read field by field or line by line.
fn opened(name: &str, bytes: &[u8]) -> Option<DocPayload> {
    let path = Path::new(name);
    let kind = DocumentKind::from_path(path)?.refine_with_bytes(bytes);
    if kind.resource_type().is_none() && !matches!(kind, DocumentKind::Meta | DocumentKind::Text) {
        return None;
    }
    let crypto: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
    document_io::parse_payload(path, kind, bytes, crypto).ok()
}

/// A text's lines, or a metadata file's as its XML holds them.
fn text_of(payload: &DocPayload) -> Option<String> {
    match payload {
        DocPayload::Text(text) => Some(text.text.to_string()),
        DocPayload::Meta(meta) if !meta.source_format.is_binary() => {
            // Name comments are display-only, so a diff reads the comment-free XML.
            Some((meta.comment_free_xml()).unwrap_or_else(|| meta.text.text.to_string()))
        }
        _ => None,
    }
}

/// A texture dictionary's textures, of either edition.
fn textures_of(payload: &DocPayload) -> Option<Vec<super::textures::Texture<'_>>> {
    use super::textures::Texture;
    use std::borrow::Cow;
    match payload {
        DocPayload::Resource(or_io::Asset::TextureDict(dict)) => Some(
            (dict.entries.iter().flatten())
                .map(|t| Texture::Owned(Cow::Borrowed(&**t)))
                .collect(),
        ),
        DocPayload::Resource(or_io::Asset::TextureDictGen9(dict)) => Some(
            (dict.entries.iter().flatten())
                .map(|t| Texture::Enhanced(Cow::Borrowed(&**t)))
                .collect(),
        ),
        _ => None,
    }
}

/// `left` and `right`, each a file's name and bytes, compared the way their kind reads.
///
/// ⚠ Binary metadata, on either side, by record and field; metadata both
/// sides hold as XML is text, and reads line by line. Two resources of one
/// kind compare by their parts and fields, unless they are texture dictionaries.
fn compared(left: (&str, &[u8]), right: (&str, &[u8])) -> Found {
    let (a, b) = (opened(left.0, left.1), opened(right.0, right.1));
    if let (Some(DocPayload::Meta(a)), Some(DocPayload::Meta(b))) = (&a, &b) {
        let binary = a.source_format.is_binary() || b.source_format.is_binary();
        if let (Some(a), Some(b), true) = (&a.parsed, &b.parsed, binary) {
            let changes = found::records(&a.instance, &b.instance);
            return Found::Fields {
                by: By::RecordAndField,
                changes,
                unread: None,
            };
        }
    }
    let (a, b) = (a.as_ref(), b.as_ref());
    if let (Some(a), Some(b)) = (a.and_then(textures_of), b.and_then(textures_of)) {
        let changes = found::textures(&a, &b);
        return Found::Fields {
            by: By::Texture,
            changes,
            unread: None,
        };
    }
    if let (Some(DocPayload::Resource(x)), Some(DocPayload::Resource(y))) = (a, b) {
        // Not one kind at one target: nothing lines up part for part.
        if std::mem::discriminant(x) == std::mem::discriminant(y) {
            if let Some((changes, unread)) = resource::compare(left, right, x, y) {
                return Found::Fields {
                    by: By::PartAndField,
                    changes,
                    unread,
                };
            }
        }
    }
    if let (Some(a), Some(b)) = (a.and_then(text_of), b.and_then(text_of)) {
        return Found::Lines(found::lines(&a, &b));
    }
    Found::Bytes {
        same: left.1 == right.1,
        sizes: [left.1.len(), right.1.len()],
    }
}

/// Both sides read and compared, on a worker.
fn compare_sides(
    headings: [String; 2],
    places: [String; 2],
    names: [String; 2],
    sides: [Side; 2],
    stop: &AtomicBool,
) -> Ended {
    let sides = match sides {
        [Side::Archive(left), Side::Archive(right)] => {
            return archive::compare(headings, places, names, [left, right], stop)
        }
        sides => sides,
    };
    let [left, right] = sides;
    let right = match right.read() {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ended::Failed(tr!("{name} could not be read", name = names[1])),
        Err(why) => return Ended::Failed(why),
    };
    let left = match left.read() {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            return Ended::NoOriginal(tr!(
                "The game has no {name} of its own: the mods overlay adds it",
                name = names[1]
            ))
        }
        Err(why) => return Ended::Failed(why),
    };
    let found = compared((&names[0], &left), (&names[1], &right));
    Ended::Compared(Comparison {
        headings,
        places,
        found,
    })
}

/// Whether `row` is an archive a tree holds, which compares entry by entry.
fn is_archive(session: &RpfEditSession, row: RpfNodeId) -> bool {
    let backing = session.node(row).map(|node| &node.backing);
    matches!(backing, Some(RpfNodeBacking::Archive { .. }))
}

/// `row`, an archive, as one side: opened already, or a file on disk to open on the worker.
fn archive_row(session: &RpfEditSession, row: RpfNodeId) -> Option<archive::Archive> {
    match &session.node(row)?.backing {
        RpfNodeBacking::Archive {
            archive_id: Some(id),
            ..
        } => {
            let archive = Arc::clone(&session.archive(*id)?.archive);
            Some(archive::Archive::open(archive))
        }
        RpfNodeBacking::Archive {
            source: ArchiveSource::Disk(Some(file)),
            ..
        } => Some(archive::Archive::on_disk(
            file.clone(),
            Arc::clone(&session.crypto),
        )),
        RpfNodeBacking::Archive {
            source:
                ArchiveSource::Nested {
                    opened: Some(archive),
                    ..
                },
            ..
        } => Some(archive::Archive::open(Arc::clone(archive))),
        _ => None,
    }
}

/// Whether `row`, an archive, holds changes not yet saved, which a comparison reads it without.
fn is_unsaved(session: &RpfEditSession, row: RpfNodeId) -> bool {
    match session.node(row).map(|node| &node.backing) {
        Some(RpfNodeBacking::Archive {
            archive_id: Some(id),
            ..
        }) => session.archive(*id).is_some_and(|slot| slot.dirty),
        _ => false,
    }
}

/// The row `row` of `doc`, a file or an archive, with the tree holding it and where it is, as Properties says.
fn file_row(
    state: &AppState,
    doc: DocumentId,
    row: RpfNodeId,
) -> Option<(&RpfEditSession, String, String)> {
    let document = state.document(doc)?;
    let DocPayload::Rpf(rpf) = &document.payload else {
        return None;
    };
    let node =
        (rpf.session.node(row)).filter(|node| !node.is_dir() || is_archive(&rpf.session, row))?;
    let inner = rpf.session.path_of(row).unwrap_or_default();
    let place = match (&document.path, node.backing.fs_path()) {
        (_, Some(disk)) => disk.display().to_string(),
        (Some(root), None) => format!("{}\\{}", root.display(), inner.replace('/', "\\")),
        (None, None) => format!("/{inner}"),
    };
    Some((&rpf.session, node.name.clone(), place))
}

impl AppState {
    /// The name of the file Select for compare chose, while its row is open.
    pub fn selected_for_compare(&self) -> Option<String> {
        let (doc, row) = self.compare.selected?;
        file_row(self, doc, row).map(|(_, name, _)| name)
    }

    /// Remember `row` of `doc`, which Compare with selected compares another file or archive with.
    pub fn select_for_compare(&mut self, doc: DocumentId, row: RpfNodeId) {
        let chosen = file_row(self, doc, row).map(|(session, name, _)| {
            let kind = if is_archive(session, row) {
                "archive"
            } else {
                "file"
            };
            (name, kind)
        });
        if let Some((name, kind)) = chosen {
            self.compare.selected = Some((doc, row));
            self.show_info_toast(tr!(
                "{name} is selected: Compare with selected on another {kind} compares the two",
                name = name,
                kind = kind
            ));
        }
    }

    /// `row` of `doc` as one side of a comparison: its name, where it is, and its bytes, or its entries where it is an archive.
    fn side(&self, doc: DocumentId, row: RpfNodeId) -> Option<(String, String, Side)> {
        let (session, name, place) = file_row(self, doc, row)?;
        if is_archive(session, row) {
            return Some((name, place, Side::Archive(archive_row(session, row)?)));
        }
        let item = plan(session, row, &TypeFilter::All).into_iter().next()?;
        Some((name, place, Side::Row(item)))
    }

    /// Why `row` of `doc` is not something to compare, if it is not: a folder holds many files, which no comparison reads as one.
    pub fn compares(&self, doc: DocumentId, row: RpfNodeId) -> Result<(), &'static str> {
        let (session, ..) = file_row(self, doc, row)
            .ok_or(tr!("A folder is not compared: choose a file or an archive"))?;
        match is_unsaved(session, row) {
            true => Err(crate::i18n::tr(UNSAVED)),
            false => Ok(()),
        }
    }

    fn start_compare(
        &mut self,
        title: String,
        headings: [String; 2],
        places: [String; 2],
        names: [String; 2],
        sides: [Side; 2],
    ) {
        let reads: Vec<PathBuf> = sides.iter().filter_map(Side::file).collect();
        // ⚠ A defragment rewrites its archive under any offset read here. The menus are off while
        // one holds it, and compare_with_selected asks again; compare_with_original does not, so
        // this is its refusal, for a defragment begun since its menu drew.
        if let Some(held) = reads.iter().find(|path| self.defrag.holds(path)) {
            self.show_error_toast(tr!(
                "`{held}` is being defragmented; compare once that is done",
                held = held.display()
            ));
            return;
        }
        let work = move |stop: &AtomicBool| compare_sides(headings, places, names, sides, stop);
        self.compare.start(title, reads, work);
    }

    /// Whether a defragment is rewriting, or has on offer, the archive `side` is read out of.
    fn is_defragmenting(&self, side: &Side) -> bool {
        side.file().is_some_and(|file| self.defrag.holds(&file))
    }

    /// Whether `row` of `doc` is read out of an archive a defragment holds.
    fn is_held(&self, doc: DocumentId, row: RpfNodeId) -> bool {
        let side = self.side(doc, row);
        side.is_some_and(|(.., side)| self.is_defragmenting(&side))
    }

    /// Why `row` of `doc` cannot be compared with what Select for compare chose, if it cannot.
    pub fn compares_with_selected(
        &self,
        doc: DocumentId,
        row: RpfNodeId,
    ) -> Result<(), &'static str> {
        let selected = self
            .compare
            .selected
            .ok_or(tr!("Select for compare on another file first"))?;
        if selected == (doc, row) {
            return Err(tr!("This is the file selected for compare: choose another"));
        }
        file_row(self, selected.0, selected.1)
            .ok_or(tr!("The file selected for compare is no longer open"))?;
        self.compares(selected.0, selected.1)?;
        self.compares(doc, row)?;
        let archive = |doc, row| file_row(self, doc, row).map(|(s, ..)| is_archive(s, row));
        match (archive(selected.0, selected.1), archive(doc, row)) {
            (Some(true), Some(false)) => {
                return Err(tr!("An archive compares with another archive: choose one"))
            }
            (Some(false), Some(true)) => {
                return Err(tr!("A file compares with another file: choose one"))
            }
            _ => {}
        }
        if self.is_held(selected.0, selected.1) {
            return Err(crate::i18n::tr(SELECTED_DEFRAGMENTING));
        }
        if self.is_held(doc, row) {
            return Err(crate::i18n::tr(DEFRAGMENTING));
        }
        self.idle_comparing()
    }

    /// Compare the file Select for compare chose, on the left, with `row` of `doc`.
    pub fn compare_with_selected(&mut self, doc: DocumentId, row: RpfNodeId) {
        if let Err(why) = self.compares_with_selected(doc, row) {
            self.show_error_toast(why);
            return;
        }
        let Some((selected, at)) = self.compare.selected else {
            return;
        };
        let (Some(left), Some(right)) = (self.side(selected, at), self.side(doc, row)) else {
            return;
        };
        let names = [left.0, right.0];
        let title = format!("{} ↔ {}", names[0], names[1]);
        let places = [left.1, right.1];
        self.start_compare(title, names.clone(), places, names, [left.2, right.2]);
    }

    /// The game's own copy of the overlay file or archive at `row` of `doc`, and where it is.
    fn original_of(&self, doc: DocumentId, row: RpfNodeId) -> Result<(String, Side), &'static str> {
        const NOT: &str = tr_key!(
            "Only a file or an archive in the mods overlay has an original to compare with"
        );
        const NESTED: &str = tr_key!(
            "An archive inside another has no original of its own: compare the one holding it"
        );
        let (session, _, _) = file_row(self, doc, row).ok_or(crate::i18n::tr(NOT))?;
        let path = session.path_of(row).ok_or(crate::i18n::tr(NOT))?;
        let loose = session.node(row).and_then(|node| node.backing.fs_path());
        // The file on disk the row is in, and the row's path inside it.
        let (file, inner) = match (loose, session.archive_on_disk_around(row)) {
            (Some(loose), _) => (loose.to_path_buf(), None),
            (None, Some((archive, file))) => {
                let root =
                    (session.archive_root_node(archive)).and_then(|node| session.path_of(node));
                let inner = root
                    .and_then(|root| path.strip_prefix(&root))
                    .ok_or(crate::i18n::tr(NOT))?;
                (file, Some(inner.trim_start_matches('/').to_string()))
            }
            // An archive opened as a document of its own holds its rows at their paths.
            (None, None) => {
                let file = self.document(doc).and_then(|d| d.path.clone());
                (
                    file.filter(|file| file.is_file())
                        .ok_or(crate::i18n::tr(NOT))?,
                    Some(path),
                )
            }
        };
        let original = retail::install_original(&file, &self.prefs).ok_or(crate::i18n::tr(NOT))?;
        let place = match &inner {
            Some(inner) => format!("{}\\{}", original.display(), inner.replace('/', "\\")),
            None => original.display().to_string(),
        };
        let crypto = Arc::clone(&session.crypto);
        if is_archive(session, row) {
            // Only a file on disk, in the overlay, has the install's file to stand beside.
            return match inner {
                Some(_) => Err(crate::i18n::tr(NESTED)),
                None => Ok((
                    place,
                    Side::Archive(archive::Archive::on_disk(original, crypto)),
                )),
            };
        }
        Ok((
            place,
            Side::Game {
                file: original,
                inner,
                crypto,
            },
        ))
    }

    /// Why `row` of `doc` has no original to compare with, if it has none.
    pub fn compares_with_original(
        &self,
        doc: DocumentId,
        row: RpfNodeId,
    ) -> Result<(), &'static str> {
        let (_, original) = self.original_of(doc, row)?;
        self.compares(doc, row)?;
        if self.is_defragmenting(&original) || self.is_held(doc, row) {
            return Err(crate::i18n::tr(DEFRAGMENTING));
        }
        self.idle_comparing()
    }

    /// Compare the game's own copy of the overlay file at `row` of `doc`, on the left, with that file.
    pub fn compare_with_original(&mut self, doc: DocumentId, row: RpfNodeId) {
        let asked = (self.idle_comparing())
            .and_then(|()| self.compares(doc, row))
            .and_then(|()| self.original_of(doc, row));
        let (place, original) = match asked {
            Ok(original) => original,
            Err(why) => {
                self.show_error_toast(why);
                return;
            }
        };
        let Some((name, mods, side)) = self.side(doc, row) else {
            return;
        };
        let title = tr!("{name} — game ↔ mods", name = name);
        let headings = [tr!("Game").to_string(), tr!("Mods overlay").to_string()];
        let names = [name.clone(), name];
        self.start_compare(title, headings, [place, mods], names, [original, side]);
    }

    fn idle_comparing(&self) -> Result<(), &'static str> {
        match self.compare.running {
            Some(_) => Err(crate::i18n::tr(RUNNING)),
            None => Ok(()),
        }
    }

    /// Open a finished comparison as a document of its own, named for both sides.
    fn open_comparison(&mut self, title: String, comparison: Comparison) -> DocumentId {
        let id = self.alloc_doc_id();
        let mut doc = Document::new(id, None, DocPayload::Compare(comparison));
        doc.name = title;
        self.documents.push(doc);
        self.set_active_document(id);
        id
    }

    /// Where a finished comparison lands: a document of its own, or what stopped it.
    pub(super) fn poll_compare(&mut self) {
        let received = match &self.compare.running {
            Some(running) => running.receiver.try_recv(),
            None => return,
        };
        if matches!(received, Err(TryRecvError::Empty)) {
            return;
        }
        let Some(running) = self.compare.running.take() else {
            return;
        };
        let (outcome, said) = match received {
            Ok(Ended::Compared(comparison)) => {
                let said = format!("{}, {}", running.label, comparison.summary());
                let doc = self.open_comparison(running.label, comparison);
                (Outcome::Done(Then::Show(doc)), said)
            }
            Ok(Ended::NoOriginal(said)) => {
                self.show_warning_toast(said.clone());
                (Outcome::Done(Then::Nothing), said)
            }
            Ok(Ended::Failed(why)) => {
                self.show_error_toast(tr!("Could not compare: {why}", why = why));
                (Outcome::Failed, why)
            }
            Err(_) => (
                Outcome::Failed,
                tr!("It stopped on an internal error").to_string(),
            ),
        };
        self.jobs.record(JobKind::Compare, outcome, said);
    }
}
