//! A GTA VI install mounted by identity. With a key snapshot an archive's table names its entries,
//! and a registration document or a registered archive is found by its path; with none the table
//! is ciphertext, and a document is found by scanning an archive for XML bodies and known by its
//! root element and its `id`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9Content, Rpf9Scan};
use or_io::gta6::hash::name_hash32;

use crate::dlc::{
    ContentChangeSetDef, ContentDef, ExecutionConditions, RegisteredFile, SetupContentType,
    SetupData,
};

#[path = "gta6_mount_table.rs"]
pub mod table;

use table::{entry_path, split_extension, Entry, Table, KIND_FILE};

/// What a registration document may inflate to; a longer one is passed over.
const BODY_LIMIT: u64 = 16 << 20;
/// The inflated bytes a scan keeps of each body, in which a document's root element is sought.
const HEAD: usize = 4096;
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
/// The bit of a map's flags that holds it back as script-managed.
pub const SCRIPT_MANAGED: u32 = 1;
/// The type a change set registers a map-types file under that stays loaded.
const PERMANENT_TYPES: &str = "PERMANENT_ITYP_FILE";
/// What ends the name of a registered archive of maps or map types.
const METADATA: &str = "_metadata";
/// The folder a registered archive of a mode's world-state selectors lies in.
const GAME_SHAPES: &str = "gameshapes";

/// One registration document a scan of an archive found, with where it lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub offset: u64,
    pub document: Document,
}

/// A registration document, by its root element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document {
    Setup(SetupData),
    Content(ContentDef),
    ChangeSet(ContentChangeSetDef),
}

type Reader = fn(&[u8]) -> Option<Document>;

/// The root element of each registration document, and what reads it.
const READERS: [(&str, Reader); 3] = [
    ("SSetupData", |b| {
        SetupData::parse(b).ok().map(Document::Setup)
    }),
    ("CDataFileMgr__ContentDef", |b| {
        ContentDef::parse(b).ok().map(Document::Content)
    }),
    ("CDataFileMgr__ContentChangeSetDef", |b| {
        ContentChangeSetDef::parse(b).ok().map(Document::ChangeSet)
    }),
];

impl Document {
    /// The registration document `body` is by its root element, `None` for any other body and for one that does not parse.
    pub fn parse(body: &[u8]) -> Option<Self> {
        reader_of(root_name(body)?)?(body)
    }
}

fn reader_of(root: &[u8]) -> Option<Reader> {
    (READERS.iter())
        .find(|(name, _)| root.eq_ignore_ascii_case(name.as_bytes()))
        .map(|(_, read)| *read)
}

/// The name of the root element `head` opens, past a byte order mark, a declaration and comments.
fn root_name(head: &[u8]) -> Option<&[u8]> {
    let mut rest = head.strip_prefix(BOM).unwrap_or(head);
    loop {
        rest = rest.trim_ascii_start().strip_prefix(b"<")?;
        let close: &[u8] = match rest.first()? {
            b'?' => b"?>",
            b'!' => b"-->",
            _ => break,
        };
        let end = rest.windows(close.len()).position(|w| w == close)?;
        rest = &rest[end + close.len()..];
    }
    let len = (rest.iter()).position(|c| c.is_ascii_whitespace() || matches!(c, b'>' | b'/'))?;
    Some(&rest[..len])
}

/// Every registration document the clear XML bodies of the GTA VI archive at `archive` hold; a body that is no such document, or does not parse, is passed over, and `stop` ends the scan with `Interrupted`.
pub fn read_documents(archive: &Path, stop: &dyn Fn() -> bool) -> io::Result<Vec<Found>> {
    let file = File::open(archive)?;
    let mut scan = Rpf9Scan::new(&file, 0..u64::MAX)
        .map_err(io::Error::other)?
        .keeping(HEAD);
    let mut found = Vec::new();
    while let Some(find) = scan.next() {
        if stop() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let find = find.map_err(io::Error::other)?;
        let xml = matches!(find.kind, Rpf9BodyKind::Zlib(run) if run.content == Rpf9Content::Xml);
        // The head the scan kept says which body is worth reading whole.
        if !xml || root_name(scan.kept(&find)).and_then(reader_of).is_none() {
            continue;
        }
        let offset = find.offset;
        match read_rpf9_body(&file, offset, BODY_LIMIT) {
            Ok((_, body)) => {
                found.extend(Document::parse(&body).map(|document| Found { offset, document }))
            }
            Err(e) => tracing::warn!(offset, "a registration document is not read: {e}"),
        }
    }
    Ok(found)
}

/// Every registration document among the entries `table` lists of the archive at `archive`: each deflated plain file whose bytes are clear is read where the table puts it; `stop` ends the read with `Interrupted`.
pub fn read_documents_by_table(
    archive: &Path,
    table: &Table,
    stop: &dyn Fn() -> bool,
) -> io::Result<Vec<Found>> {
    let file = File::open(archive)?;
    let mut entries: Vec<&Entry> = (table.entries().iter())
        // A movie or a sound is a plain file too, stored as it is: a document is deflated.
        .filter(|e| e.kind == KIND_FILE && !e.ciphered && e.chunks > 0 && e.stated <= BODY_LIMIT)
        .collect();
    entries.sort_unstable_by_key(|e| e.offset);
    let mut found = Vec::new();
    for entry in entries {
        if stop() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let offset = entry.offset;
        match read_rpf9_body(&file, offset, BODY_LIMIT) {
            Ok((_, body)) => {
                found.extend(Document::parse(&body).map(|document| Found { offset, document }))
            }
            Err(e) => tracing::debug!(offset, "an entry is not read as a document: {e}"),
        }
    }
    Ok(found)
}

/// The game mode a pack is part of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Base,
    Story,
    Minigames,
}

impl Mode {
    /// The bit this mode is in a set of modes: 1 the base game, 2 story, 4 the shared minigames.
    pub fn bit(self) -> u8 {
        1 << self as u8
    }

    // ponytail: a pack's mode is read off its device name, the one place a setup says what the
    // pack is for; a setup that states its mode would replace this.
    fn of(setup: &SetupData) -> Self {
        let device = setup.device_name.to_ascii_lowercase();
        if device.starts_with("story") {
            Self::Story
        } else if device.contains("minigame") {
            Self::Minigames
        } else {
            Self::Base
        }
    }
}

/// A change set a pack registers: its id, the setup groups that name it, whether a content definition includes it as a set of the level, its flags and conditions, and its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredSet {
    pub id: String,
    pub groups: Vec<String>,
    pub level: bool,
    pub flags: Vec<String>,
    /// What the set states must hold for it to run; ⚠ stated, never evaluated here.
    pub conditions: Option<ExecutionConditions>,
    pub files: Vec<RegisteredFile>,
}

impl RegisteredSet {
    /// Whether the set states that it changes the map.
    pub fn map_change(&self) -> bool {
        (self.flags.iter()).any(|flag| flag.eq_ignore_ascii_case("MapChange"))
    }
}

/// A pack: the archive its setup lies in, its setup, its mode, and the change sets its documents give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub archive: PathBuf,
    pub setup: SetupData,
    pub mode: Mode,
    pub sets: Vec<RegisteredSet>,
    /// The ids a setup group or an include names whose change set was not found, as they are compared.
    pub unread: Vec<String>,
    /// Those of `unread` whose file the archive's table lists with bytes under a cipher of their own.
    pub ciphered: Vec<String>,
}

/// The packs of an install, in the order their setups mount.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    pub packs: Vec<Pack>,
}

impl Registry {
    /// The registry of the documents found in each archive, an archive a pack where it holds a setup.
    pub fn of(archives: Vec<(PathBuf, Vec<Document>)>) -> Self {
        Self::listed((archives.into_iter()).map(|(archive, documents)| (archive, documents, None)))
    }

    /// The same, where an archive's table, if it was read, says which change set not found is ciphered.
    pub fn listed<'t>(
        archives: impl IntoIterator<Item = (PathBuf, Vec<Document>, Option<&'t Table>)>,
    ) -> Self {
        let mut packs = Vec::new();
        for (archive, documents, table) in archives {
            let (mut setups, mut contents, mut bodies) = (Vec::new(), Vec::new(), Vec::new());
            for document in documents {
                match document {
                    Document::Setup(setup) => setups.push(setup),
                    Document::Content(content) => contents.push(content),
                    Document::ChangeSet(set) => bodies.push(set),
                }
            }
            // An archive of two setups is two packs over the same documents.
            for setup in setups {
                packs.push(pack(archive.clone(), setup, &contents, &bodies, table));
            }
        }
        packs.sort_by(|a, b| a.setup.mount_cmp(&b.setup));
        Self { packs }
    }

    /// The pack whose setup states the type of a level pack and whose content definitions include sets, the first by the setups' own order.
    pub fn level_pack(&self) -> Option<&Pack> {
        (self.packs.iter()).find(|pack| {
            pack.setup.content_type == SetupContentType::LevelPack
                && pack.sets.iter().any(|set| set.level)
        })
    }
}

/// A change set's id as it is compared: in lower case, with no underscore.
fn normalised(id: &str) -> String {
    (id.chars().filter(|c| *c != '_'))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The stem and the extension of the last component of a path a document writes.
fn leaf(path: &str) -> (&str, &str) {
    let name = path.rsplit(['/', '\\', ':']).next().unwrap_or(path);
    name.rsplit_once('.').unwrap_or((name, ""))
}

/// The id of the change set the file at `load_path` holds: its stem with no leading `ccs_`.
fn included_id(load_path: &str) -> String {
    let stem = leaf(load_path).0.to_ascii_lowercase();
    normalised(stem.strip_prefix("ccs_").unwrap_or(&stem))
}

fn pack(
    archive: PathBuf,
    setup: SetupData,
    contents: &[ContentDef],
    bodies: &[ContentChangeSetDef],
    table: Option<&Table>,
) -> Pack {
    let inline = || contents.iter().flat_map(|content| &content.content_sets);
    let included = || contents.iter().flat_map(|content| &content.included_sets);
    let groups = &setup.content_change_set_groups;
    let level: HashSet<String> = (inline().map(|set| normalised(&set.id)))
        .chain(included().map(|id| normalised(id)))
        .collect();
    let mut found = HashSet::new();
    let mut sets = Vec::new();
    // An inline set comes first, so the files of an id stated twice are the inline entry's own.
    for set in inline().chain(bodies) {
        let id = normalised(&set.id);
        if !found.insert(id.clone()) {
            continue;
        }
        sets.push(RegisteredSet {
            id: set.id.clone(),
            groups: (groups.iter())
                .filter(|group| {
                    group
                        .content_change_sets
                        .iter()
                        .any(|n| normalised(n) == id)
                })
                .map(|group| group.name_hash.clone())
                .collect(),
            level: level.contains(&id),
            flags: set.flags.clone(),
            conditions: set.execution_conditions.clone(),
            files: set.file_list.clone(),
        });
    }
    let by_path = (contents.iter())
        .flat_map(|content| &content.included_changesets)
        .map(|include| included_id(&include.load_path));
    let named = (groups.iter())
        .flat_map(|group| &group.content_change_sets)
        .chain(included())
        .map(|id| normalised(id))
        .chain(by_path);
    let mut unread = Vec::new();
    for id in named {
        if !found.contains(&id) && !unread.contains(&id) {
            unread.push(id);
        }
    }
    // A set included by a path the table lists under a cipher is there, and closed.
    let mut ciphered = Vec::new();
    for include in contents.iter().flat_map(|c| &c.included_changesets) {
        let id = included_id(&include.load_path);
        let closed = table
            .and_then(|table| table.find_registered(&include.load_path))
            .is_some_and(|entry| entry.ciphered);
        if closed && unread.contains(&id) && !ciphered.contains(&id) {
            ciphered.push(id);
        }
    }
    Pack {
        archive,
        mode: Mode::of(&setup),
        setup,
        sets,
        unread,
        ciphered,
    }
}

/// What the mount needs to know of one map body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapFact {
    /// Which top-level archive it lies in, an index the caller chose.
    pub archive: u32,
    /// The offset of the nested archive that holds it, `None` at the top level.
    pub container: Option<u64>,
    pub name: u32,
    /// The name of its block map, 0 for none.
    pub block: u32,
    pub flags: u32,
    /// Whether it places an entity or a list.
    pub holds: bool,
}

/// Why a map does not stream at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldBack {
    NotRegistered,
    ScriptManaged,
    Empty,
}

/// What binding the level's archives to the maps gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// For each map, in the order given: `None` where it streams at rest.
    pub held: Vec<Option<HeldBack>>,
    pub report: Report,
}

/// The counts of a binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    pub named_archives: usize,
    pub bound_archives: usize,
    pub maps: usize,
    pub registered: usize,
    pub at_rest: usize,
    pub script_managed: usize,
    pub empty: usize,
    /// The top-level archive taken as the level's, by the caller's index.
    pub level_archive: Option<u32>,
}

/// Bind the archives the sets `level` names to `maps`.
// A container is bound where it holds a map named by the hash of a registered archive's stem. A
// map is tied to the level where its container is bound, or the container of its block map is.
// The top-level archive with the most tied maps is the level's, and each of its maps registered.
pub fn bind(level_files: &[&RegisteredFile], maps: &[MapFact]) -> Binding {
    let named: HashSet<u32> = (level_files.iter())
        .map(|file| leaf(&file.filename))
        .filter(|(_, extension)| extension.eq_ignore_ascii_case("rpf"))
        .map(|(stem, _)| name_hash32(stem.as_bytes()))
        .collect();
    let place = |map: &MapFact| (map.archive, map.container);
    let bound: HashSet<(u32, Option<u64>)> = (maps.iter())
        .filter(|map| named.contains(&map.name))
        .map(place)
        .collect();
    let in_bound: HashSet<u32> = (maps.iter())
        .filter(|map| bound.contains(&place(map)))
        .map(|map| map.name)
        .collect();
    let mut tied = BTreeMap::<u32, usize>::new();
    for map in maps {
        if bound.contains(&place(map)) || (map.block != 0 && in_bound.contains(&map.block)) {
            *tied.entry(map.archive).or_default() += 1;
        }
    }
    // Of two archives with as many tied maps, the lower index.
    let level_archive = (tied.iter())
        .max_by_key(|(archive, count)| (**count, Reverse(**archive)))
        .map(|(archive, _)| *archive);
    let held: Vec<Option<HeldBack>> = (maps.iter())
        .map(|map| held_back(Some(map.archive) == level_archive, map))
        .collect();
    let count = |what: Option<HeldBack>| held.iter().filter(|h| **h == what).count();
    let report = Report {
        named_archives: named.len(),
        bound_archives: bound.len(),
        maps: maps.len(),
        registered: maps.len() - count(Some(HeldBack::NotRegistered)),
        at_rest: count(None),
        script_managed: count(Some(HeldBack::ScriptManaged)),
        empty: count(Some(HeldBack::Empty)),
        level_archive,
    };
    Binding { held, report }
}

/// Why `map` does not stream once its mode is on, `registered` saying whether a pack registers it.
pub fn held_back(registered: bool, map: &MapFact) -> Option<HeldBack> {
    if !registered {
        Some(HeldBack::NotRegistered)
    } else if map.flags & SCRIPT_MANAGED != 0 {
        Some(HeldBack::ScriptManaged)
    } else if !map.holds {
        Some(HeldBack::Empty)
    } else {
        None
    }
}

/// A top-level archive whose table was read: where it is, and the index the caller gave it.
#[derive(Debug, Clone, Copy)]
pub struct Listed<'a> {
    /// The caller's index of the archive, as a [`MapFact`] states it.
    pub archive: u32,
    pub path: &'a Path,
    pub table: &'a Table,
}

/// The nested archive of the path `path` a file of `pack` registers, and the archive that lists it: an archive of the folder the pack's setup lies in before any other.
pub fn resolve<'a>(pack: &Pack, path: &str, tables: &[Listed<'a>]) -> Option<(u32, &'a Entry)> {
    let own = pack.archive.parent();
    let find = |near: bool| {
        (tables.iter())
            .filter(|listed| (listed.path.parent() == own) == near)
            .find_map(|listed| Some((listed.archive, listed.table.find_archive(path)?)))
    };
    find(true).or_else(|| find(false))
}

/// The caller's indices of the archives that list a metadata archive a pack registers: the archives that hold the world's maps, map types and models.
// A registered archive of maps or map types is named after what it describes, with `_metadata`.
pub fn world_archives(packs: &[Pack], tables: &[Listed<'_>]) -> Vec<u32> {
    let mut found = Vec::new();
    for pack in packs {
        for file in pack.sets.iter().flat_map(|set| &set.files) {
            let (bare, extension) = split_extension(entry_path(&file.filename));
            let metadata = extension.eq_ignore_ascii_case("rpf")
                && bare.to_ascii_lowercase().ends_with(METADATA);
            if !metadata {
                continue;
            }
            if let Some((archive, _)) = resolve(pack, bare, tables) {
                if !found.contains(&archive) {
                    found.push(archive);
                }
            }
        }
    }
    found
}

/// The nested game-shape archives the packs register, each with its pack's mode, the caller's index of the archive that lists it, and its entry there.
// ponytail: known by the folder its registered path names, as a metadata archive is by its name.
pub fn game_shapes(packs: &[Pack], tables: &[Listed<'_>]) -> Vec<(Mode, u32, Entry)> {
    let mut found: Vec<(Mode, u32, Entry)> = Vec::new();
    for pack in packs {
        for file in pack.sets.iter().flat_map(|set| &set.files) {
            let (bare, extension) = split_extension(entry_path(&file.filename));
            let shapes = extension.eq_ignore_ascii_case("rpf")
                && (bare.split('/')).any(|part| part.eq_ignore_ascii_case(GAME_SHAPES));
            let Some((archive, entry)) = shapes.then(|| resolve(pack, bare, tables)).flatten()
            else {
                continue;
            };
            if !(found.iter()).any(|(_, a, e)| *a == archive && e.offset == entry.offset) {
                found.push((pack.mode, archive, *entry));
            }
        }
    }
    found
}

/// What binding the registered archives to the maps by the tables' names gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableBinding {
    /// For each map, in the order given: the bits of the modes whose packs register the archive it lies in.
    pub modes: Vec<u8>,
    /// The archives the packs' sets name, how many a table lists, and how many of those hold a map.
    pub named_archives: usize,
    pub listed_archives: usize,
    pub bound_archives: usize,
    /// Where the archive of each permanent map-types file lies, by the caller's index and its offset.
    pub permanent: Vec<(u32, u64)>,
    /// How many permanent map-types files the sets name.
    pub permanent_named: usize,
}

/// Bind every archive the sets of `packs` register to `maps` by its name in `tables`.
// A registered archive is the entry named by its path with no device and no extension, and a map
// is registered by the packs that register the nested archive it lies in. A permanent map-types
// file lies in the archive of its own path with `_metadata` after it.
pub fn bind_by_table(packs: &[Pack], tables: &[Listed<'_>], maps: &[MapFact]) -> TableBinding {
    let mut registered: HashMap<(u32, u64), u8> = HashMap::new();
    let mut out = TableBinding::default();
    for pack in packs {
        for file in pack.sets.iter().flat_map(|set| &set.files) {
            let (bare, extension) = split_extension(entry_path(&file.filename));
            if file.file_type.eq_ignore_ascii_case(PERMANENT_TYPES) {
                out.permanent_named += 1;
                let archive = resolve(pack, &format!("{bare}{METADATA}"), tables);
                (out.permanent).extend(archive.map(|(archive, entry)| (archive, entry.offset)));
            } else if extension.eq_ignore_ascii_case("rpf") {
                out.named_archives += 1;
                if let Some((archive, entry)) = resolve(pack, bare, tables) {
                    out.listed_archives += 1;
                    *registered.entry((archive, entry.offset)).or_default() |= pack.mode.bit();
                }
            }
        }
    }
    let mut bound = HashSet::new();
    out.modes = (maps.iter())
        .map(|map| {
            let place = map.container.map(|container| (map.archive, container));
            let modes = place.and_then(|place| registered.get(&place).copied());
            bound.extend(place.filter(|_| modes.is_some()));
            modes.unwrap_or(0)
        })
        .collect();
    out.bound_archives = bound.len();
    out
}

#[cfg(test)]
#[path = "gta6_mount_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "gta6_mount_bind_tests.rs"]
mod bind_tests;
