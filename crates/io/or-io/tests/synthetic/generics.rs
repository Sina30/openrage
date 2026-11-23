//! Owned records generic over their target and over their element, on
//! invented types: a root that opens as the target its file holds, Legacy by
//! default and refused by name at the other target; a generic record held by
//! another over the same element; one whose element is a scalar; and one
//! pointing back at its root, as a particle effect rule does. Each reads,
//! saves, reads back equal and reaches a fixed point.

use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::Mark;
use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::container::resource::{Authoring, DatResourceFileType, ResourceEnvelope, Rsc7};
use or_io::decl::*;
use or_io::desc::{self, Pack};
use or_io::{Error, Generation};

pub mod gen8 {
    use or_io::decl::*;

    #[record(size = 0x08, target = "gen8")]
    pub struct Sheet {
        pub width: u16,
        pub height: u16,
        pub depth: u32,
    }
}

pub mod gen9 {
    use or_io::decl::*;

    #[record(size = 0x10, target = "gen9")]
    pub struct Sheet {
        pub depth: u32,
        pub width: u16,
        pub height: u16,
        #[pad_after(4)]
        pub layers: u32,
    }
}

/// What each target lays out its own way; an album points at it.
pub trait Edition: TargetMarker {
    type Sheet: Owned<Raw: Record> + Default + Debug + Clone + PartialEq;
}

impl Edition for Gen8 {
    type Sheet = gen8::Sheet;
}

impl Edition for Gen9 {
    type Sheet = gen9::Sheet;
}

/// A generic record another holds, over the same target.
#[record(size = 0x08)]
pub struct Page<T: Edition> {
    pub sheet: Option<Box<T::Sheet>>,
}

/// A root generic over its target, Legacy by default.
#[record(size = 0x30)]
pub struct Album<T: Edition = Gen8> {
    pub cover: Box<T::Sheet>,
    #[len(field = "sheet_count")]
    pub sheets: Vec<T::Sheet>,
    pub pages: Vec<Page<T>>,
    pub title: String,
    #[pad_after(4)]
    sheet_count: u32,
}

/// A run held in chunks, as a navmesh splits its pools: a generic record holding another over its element.
#[record(size = 0x18)]
pub struct Chunked<T> {
    #[len(field = "chunk_count")]
    pub chunks: Vec<Chunk<T>>,
    #[len(field = "chunk_count")]
    pub starts: Vec<u32>,
    #[pad_after(4)]
    chunk_count: u32,
}

#[record(size = 0x10)]
pub struct Chunk<T> {
    #[len(field = "count")]
    pub items: Vec<T>,
    #[pad_after(4)]
    count: u32,
}

/// One chunked run of a scalar and one of a record.
#[record(size = 0x10)]
pub struct Shelf {
    pub indices: Option<Box<Chunked<u16>>>,
    pub marks: Option<Box<Chunked<Mark>>>,
}

/// A record generic over its target that points back at its root, as a particle effect rule points at its list.
#[record(size = 0x10)]
pub struct Note<T: Edition> {
    binder: Back<Binder<T>>,
    #[pad_after(4)]
    pub number: u32,
}

#[record(size = 0x10)]
pub struct Binder<T: Edition = Gen8> {
    pub notes: Vec<Option<Box<Note<T>>>>,
}

fn pack(target: &str) -> Arc<Pack> {
    let roots: &[fn(&mut Reach)] = match target {
        "gen8" => &[
            root::<Raw<Album<Gen8>>>,
            root::<Raw<Shelf>>,
            root::<Raw<Binder<Gen8>>>,
        ],
        _ => &[root::<Raw<Album<Gen9>>>, root::<Raw<Binder<Gen9>>>],
    };
    Arc::new(describe("synthetic", target, roots).expect("describes"))
}

/// The RSC7 version an album file states for each target.
fn version_of(target: Generation) -> u32 {
    if target == Generation::Gen8 {
        1
    } else {
        2
    }
}

fn target_of(version: u32) -> Result<Generation, Error> {
    match version {
        1 => Ok(Generation::Gen8),
        2 => Ok(Generation::Gen9),
        v => Err(Error::NotAResource(format!(
            "version {v} is not an album's"
        ))),
    }
}

fn invalid(e: impl ToString) -> Error {
    Error::InvalidData {
        context: e.to_string(),
    }
}

/// `value` laid out by the canonical writer: the image, its virtual size and where its root is.
fn build<T: Owned>(target: &str, value: &T) -> Result<(Vec<u8>, usize, usize), Error> {
    let pack = pack(target);
    let tree = value.to_element(&Cx::new(pack.plans().clone()))?;
    let built = or_io::write::build(&pack, &tree).map_err(invalid)?;
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    Ok((
        bytes,
        virtual_size,
        (built.root - FIXED_VIRTUAL_BASE) as usize,
    ))
}

fn read<T: Owned>(target: &str, bytes: &[u8], virtual_size: usize, root: usize) -> Result<T, Error>
where
    Raw<T>: Record,
{
    let aligned = Aligned::from_bytes(bytes);
    let image = desc::Image::new(aligned.bytes(), virtual_size);
    let doc = Doc::<Raw<T>>::read(pack(target), image, root).map_err(invalid)?;
    T::from_element(doc.value())
}

/// An album as an RSC7 file whose version names its target, as a declared root is.
impl<T: Edition> File for Album<T> {
    const EXTENSION: &'static str = "album";

    fn from_bytes(file: &[u8]) -> Result<Self, Error> {
        let opened = Rsc7::open(file).map_err(|e| Error::NotAResource(e.to_string()))?;
        let found = target_of(opened.version)?;
        if Some(found) != Generation::from_slug(T::ID) {
            return Err(Error::WrongTarget {
                ext: Self::EXTENSION,
                type_name: <Raw<Self> as Described>::name().into(),
                found: found.edition_label(),
            });
        }
        read(T::ID, &opened.data, opened.virtual_size as usize, 0)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let (mut bytes, virtual_size, root) = build(T::ID, self)?;
        if root != 0 {
            return Err(invalid(format!("the root landed at {root:#x}")));
        }
        let physical = bytes.split_off(virtual_size);
        let target = Generation::from_slug(T::ID).expect("a target");
        let spec = Authoring::new(
            DatResourceFileType::Custom(version_of(target)),
            flate2::Compression::default(),
        );
        Rsc7::author(spec, bytes, physical).map_err(invalid)
    }
}

/// The Legacy default's own `open`: `Album::open` names no target, and an Enhanced file is refused.
impl Album {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        <Self as File>::open(path)
    }
}

fn dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("generic_owned");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn legacy_album() -> Album<Gen8> {
    let sheet = |w| gen8::Sheet {
        width: w,
        height: 2,
        depth: 1,
        ..Default::default()
    };
    Album {
        cover: Box::new(sheet(4)),
        sheets: vec![sheet(8), sheet(16)],
        pages: vec![
            Page {
                sheet: Some(Box::new(sheet(32))),
                ..Default::default()
            },
            Page::default(),
        ],
        title: "legacy".into(),
        ..Default::default()
    }
}

fn enhanced_album() -> Album<Gen9> {
    let sheet = |w| gen9::Sheet {
        depth: 1,
        width: w,
        height: 2,
        layers: 3,
        ..Default::default()
    };
    Album {
        cover: Box::new(sheet(4)),
        sheets: vec![sheet(8)],
        pages: vec![Page {
            sheet: Some(Box::new(sheet(64))),
            ..Default::default()
        }],
        title: "enhanced".into(),
        ..Default::default()
    }
}

#[test]
fn a_target_generic_root_opens_and_saves_as_either_target() {
    let legacy = dir().join("legacy.album");
    let enhanced = dir().join("enhanced.album");
    legacy_album().save_as(&legacy).unwrap();
    enhanced_album().save_as(&enhanced).unwrap();

    // Nothing but the call names the type: the inherent `open` is the Legacy default's.
    let mut opened = Album::open(&legacy).unwrap();
    assert_eq!(opened.cover.width, 4);
    assert_eq!(opened, legacy_album());
    opened.sheets.push(opened.sheets[0].clone());
    opened.save().unwrap();
    let again = Album::open(&legacy).unwrap();
    assert_eq!((again.sheets.len(), again.pages.len()), (3, 2));
    assert_eq!(again.to_bytes().unwrap(), again.to_bytes().unwrap());
    let first = std::fs::read(&legacy).unwrap();
    again.save().unwrap();
    assert_eq!(std::fs::read(&legacy).unwrap(), first, "a fixed point");

    let mut opened = Album::<Gen9>::open(&enhanced).unwrap();
    assert_eq!(opened, enhanced_album());
    opened.cover.layers = 5;
    opened.save().unwrap();
    let again = Album::<Gen9>::open(&enhanced).unwrap();
    assert_eq!(again.cover.layers, 5);
    let first = std::fs::read(&enhanced).unwrap();
    again.save().unwrap();
    assert_eq!(std::fs::read(&enhanced).unwrap(), first, "a fixed point");
}

#[test]
fn an_album_opened_as_the_other_target_is_refused_by_name() {
    let legacy = dir().join("wrong_legacy.album");
    let enhanced = dir().join("wrong_enhanced.album");
    legacy_album().save_as(&legacy).unwrap();
    enhanced_album().save_as(&enhanced).unwrap();

    let why = Album::open(&enhanced).unwrap_err();
    assert!(
        matches!(
            why,
            Error::WrongTarget {
                found: "Enhanced",
                ..
            }
        ),
        "{why:?}"
    );
    let why = why.to_string();
    assert!(
        why.contains("Enhanced") && why.contains("`or_io::open`"),
        "{why}"
    );
    let why = Album::<Gen9>::open(&legacy).unwrap_err().to_string();
    assert!(
        why.contains("Legacy") && why.contains("`or_io::open`"),
        "{why}"
    );
}

#[test]
fn a_target_parameter_is_not_named_and_an_element_parameter_is() {
    assert_eq!(<Raw<Album<Gen8>> as Described>::name(), "Album");
    assert_eq!(<Raw<Album<Gen9>> as Described>::name(), "Album");
    assert_eq!(<Raw<Page<Gen9>> as Described>::name(), "Page");
    assert_eq!(<Raw<Chunked<u16>> as Described>::name(), "Chunked<u16>");
    assert_eq!(<Raw<Chunk<u16>> as Described>::name(), "Chunk<u16>");
    assert_eq!(<Raw<Chunked<Mark>> as Described>::name(), "Chunked<Mark>");
}

fn chunked<T: Owned + Clone>(items: &[T], per: usize) -> Chunked<T>
where
    Raw<T>: Plain + Element + Param,
{
    let chunks: Vec<Chunk<T>> = items
        .chunks(per)
        .map(|c| Chunk {
            items: c.to_vec(),
            ..Default::default()
        })
        .collect();
    let starts = (0..chunks.len()).map(|i| (i * per) as u32).collect();
    Chunked {
        chunks,
        starts,
        ..Default::default()
    }
}

fn shelf() -> Shelf {
    let marks: Vec<Mark> = (0..3)
        .map(|i| Mark {
            x: i as f32,
            weight: i,
            id: Hash32::of(&format!("mark{i}")),
            ..Default::default()
        })
        .collect();
    Shelf {
        indices: Some(Box::new(chunked(&[1u16, 2, 3, 4, 5], 2))),
        marks: Some(Box::new(chunked(&marks, 2))),
        ..Default::default()
    }
}

#[test]
fn a_nested_generic_record_and_a_scalar_argument_read_save_and_reach_a_fixed_point() {
    let (first, virtual_size, root) = build("gen8", &shelf()).unwrap();
    let again: Shelf = read("gen8", &first, virtual_size, root).unwrap();
    assert_eq!(again, shelf());
    let indices = again.indices.as_deref().unwrap();
    assert_eq!(indices.chunks.len(), 3);
    assert_eq!(indices.chunks[2].items, [5]);
    assert_eq!(again.marks.as_deref().unwrap().chunks[1].items[0].weight, 2);
    assert_eq!(build("gen8", &again).unwrap().0, first, "a fixed point");

    let mut edited = again;
    let indices = edited.indices.as_deref_mut().unwrap();
    indices.chunks[2].items.push(6);
    indices.chunks.push(Chunk {
        items: vec![7],
        ..Default::default()
    });
    indices.starts.push(6);
    let (bytes, virtual_size, root) = build("gen8", &edited).unwrap();
    let reopened: Shelf = read("gen8", &bytes, virtual_size, root).unwrap();
    assert_eq!(reopened, edited);
}

fn a_binder_round_trips<T: Edition>()
where
    Binder<T>: Owned + Debug + PartialEq,
    Raw<Binder<T>>: Record,
{
    let binder = Binder::<T> {
        notes: vec![
            Some(Box::new(Note {
                number: 7,
                ..Default::default()
            })),
            None,
        ],
        ..Default::default()
    };
    let (bytes, virtual_size, root) = build(T::ID, &binder).unwrap();
    let again: Binder<T> = read(T::ID, &bytes, virtual_size, root).unwrap();
    assert_eq!(again, binder);
    assert_eq!(build(T::ID, &again).unwrap().0, bytes, "a fixed point");
}

#[test]
fn a_back_pointer_in_a_record_generic_over_its_target_is_derived_for_either_target() {
    a_binder_round_trips::<Gen8>();
    a_binder_round_trips::<Gen9>();
}
