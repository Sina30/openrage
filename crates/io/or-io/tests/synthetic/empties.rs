//! Pointers at empty values, on invented owned records: a run whose pointer a
//! file leaves live beside a stated length of zero, a string pointer live at an
//! empty string, and a slot whose null means an empty value. Each opens, saves
//! where the tree saves and reaches a fixed point.

use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Gen8, Gen9, NullIsEmpty, Owned, Raw, Record, View};
use or_io::desc::{self, Pack, Value};

use super::generics::{Edition, Page};

/// Indices behind a pointer whose length a count states, as a skeleton's child
/// and parent indices are, and a payload in the graphics half.
#[decl::record(size = 0x18)]
pub struct Pairs {
    #[len(field = "count")]
    pub indices: Vec<u16>,
    #[phys]
    #[len(bytes = "size")]
    pub pixels: Vec<u8>,
    size: u32,
    #[pad_after(2)]
    count: u16,
}

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[
        decl::root::<Raw<Pairs>>,
        decl::root::<Raw<Haunt>>,
        decl::root::<Raw<Stages>>,
        decl::root::<Raw<Tile>>,
        decl::root::<Raw<Names>>,
    ];
    Arc::new(decl::describe("synthetic", "gen8", roots).expect("describes"))
}

/// A saved image: its bytes, its virtual size and where its root is.
struct Saved {
    bytes: Aligned,
    virtual_size: usize,
    root: usize,
}

impl Saved {
    fn image(&self) -> desc::Image<'_> {
        desc::Image::new(self.bytes.bytes(), self.virtual_size)
    }
}

/// `tree` laid out by the canonical writer.
fn build(tree: &Value) -> Saved {
    let built = or_io::write::build(&pack(), tree).unwrap();
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    Saved {
        bytes: Aligned::from_bytes(&bytes),
        virtual_size,
        root: (built.root - FIXED_VIRTUAL_BASE) as usize,
    }
}

fn tree<T: Owned>(value: &T) -> Value {
    value.to_element(&Cx::new(pack().plans().clone())).unwrap()
}

fn save<T: Owned>(value: &T) -> Saved {
    build(&tree(value))
}

fn open<T: Owned>(saved: &Saved) -> Result<T, String>
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).map_err(|e| e.to_string())?;
    T::from_element(doc.value()).map_err(|e| e.to_string())
}

/// `saved` read as a tree and laid out again by the canonical writer.
fn tree_save<T: Owned>(saved: &Saved) -> Saved
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).unwrap();
    build(doc.value())
}

/// A file opened owned and saved: byte-identical to its tree's save, and saved again to a fixed point.
fn saves_as_tree<T: Owned + PartialEq + std::fmt::Debug>(file: &Saved) -> (T, Saved)
where
    Raw<T>: Record,
{
    let value: T = open(file).unwrap();
    let saved = save(&value);
    assert_eq!(saved.bytes.bytes(), tree_save::<T>(file).bytes.bytes());
    let again: T = open(&saved).unwrap();
    assert_eq!(again, value);
    assert_eq!(
        save(&again).bytes.bytes(),
        saved.bytes.bytes(),
        "a fixed point"
    );
    (value, saved)
}

fn record(tree: &mut Value) -> &mut desc::Record {
    match tree {
        Value::Struct(record) => record,
        other => panic!("a struct, not {other:?}"),
    }
}

/// A file whose pointer at `at` is live beside a length of zero.
fn pairs_file(at: usize) -> Saved {
    let mut t = tree(&Pairs::default());
    *record(&mut t).child_at_mut(at).unwrap() = Value::ptr_to(Value::Bytes(Vec::new()));
    build(&t)
}

fn indices_pointer(saved: &Saved) -> (u64, u16) {
    let p = View::<Raw<Pairs>>::at_offset(saved.image(), saved.root).unwrap();
    ({ p.indices }.raw(), p.count)
}

#[test]
fn a_live_pointer_beside_a_length_of_zero_opens_empty_and_is_written_back() {
    let file = pairs_file(std::mem::offset_of!(Raw<Pairs>, indices));
    assert_ne!(indices_pointer(&file).0, 0, "the file's pointer is live");
    let (pairs, saved) = saves_as_tree::<Pairs>(&file);
    assert!(pairs.indices.is_empty());
    let (pointer, count) = indices_pointer(&saved);
    assert!(pointer != 0 && count == 0, "{pointer:#x} {count}");

    let mut edited = pairs;
    edited.indices = vec![3, 1];
    let saved = save(&edited);
    assert_eq!(open::<Pairs>(&saved).unwrap().indices, [3, 1]);
    assert_eq!(indices_pointer(&saved).1, 2);

    assert_eq!(indices_pointer(&save(&Pairs::default())), (0, 0));
}

const OTHER: u64 = 0x5000_1000;

#[decl::record(size = 0x08)]
pub struct Plate {
    pub a: u32,
    pub b: u32,
}

/// A plate and a run beside a count of zero that a file aims at the plate's allocation, as a
/// skeleton's per-bone runs aim at a shader's data.
#[decl::record(size = 0x18)]
pub struct Haunt {
    pub plate: Option<Box<Plate>>,
    #[len(field = "count")]
    pub marks: Vec<u32>,
    #[pad_after(4)]
    count: u32,
}

/// A file whose marks slot aims at the allocation the plate owns, beside a length of zero.
fn haunted_file() -> Saved {
    let haunt = Haunt {
        plate: Some(Box::new(Plate {
            a: 7,
            b: 8,
            ..Default::default()
        })),
        ..Default::default()
    };
    let mut t = tree(&haunt);
    let record = record(&mut t);
    match record
        .child_at_mut(std::mem::offset_of!(Raw<Haunt>, plate))
        .unwrap()
    {
        Value::Ptr { source, .. } => *source = OTHER,
        other => panic!("a live pointer, not {other:?}"),
    }
    *record
        .child_at_mut(std::mem::offset_of!(Raw<Haunt>, marks))
        .unwrap() = Value::Alias(OTHER);
    build(&t)
}

fn marks_pointer(saved: &Saved) -> (u64, u32) {
    let h = View::<Raw<Haunt>>::at_offset(saved.image(), saved.root).unwrap();
    ({ h.marks }.raw(), h.count)
}

#[test]
fn an_alias_beside_a_length_of_zero_opens_empty_and_is_written_back_live() {
    let haunt: Haunt = open(&haunted_file()).unwrap();
    assert!(haunt.marks.is_empty());
    assert_eq!(
        haunt.plate.as_deref(),
        Some(&Plate {
            a: 7,
            b: 8,
            ..Default::default()
        })
    );

    let saved = save(&haunt);
    let again: Haunt = open(&saved).unwrap();
    assert!(again.marks.is_empty());
    assert_eq!(again.plate, haunt.plate);
    let (pointer, count) = marks_pointer(&saved);
    assert!(pointer != 0 && count == 0, "{pointer:#x} {count}");
    assert_eq!(
        save(&again).bytes.bytes(),
        saved.bytes.bytes(),
        "a fixed point"
    );

    let mut edited = haunt;
    edited.marks = vec![5];
    assert_eq!(open::<Haunt>(&save(&edited)).unwrap().marks, [5]);
}

#[test]
fn a_live_payload_pointer_beside_a_length_of_zero_is_refused() {
    let file = pairs_file(std::mem::offset_of!(Raw<Pairs>, pixels));
    let why = open::<Pairs>(&file).expect_err("a payload the length does not state");
    assert!(why.contains("is live"), "{why}");
}

#[decl::record(size = 0x10)]
pub struct Stage {
    pub marks: Vec<u32>,
}

/// Slots whose null the file means as an empty stage, as a drawable's levels of detail are.
#[decl::record(size = 0x10)]
pub struct Stages {
    pub stages: [NullIsEmpty<Stage>; 2],
}

/// A file whose first stage holds marks and whose second slot is `second`.
fn stages_file(second: Value) -> Saved {
    let stages = Stages {
        stages: [
            Stage {
                marks: vec![1, 2],
                ..Default::default()
            }
            .into(),
            NullIsEmpty::default(),
        ],
        ..Default::default()
    };
    let mut t = tree(&stages);
    match record(&mut t).child_at_mut(std::mem::offset_of!(Raw<Stages>, stages)) {
        Some(Value::Array { elements, .. }) => elements[1] = second,
        other => panic!("{other:?}"),
    }
    build(&t)
}

fn stage_pointers(saved: &Saved) -> [u64; 2] {
    let s = View::<Raw<Stages>>::at_offset(saved.image(), saved.root).unwrap();
    { s.stages }.map(|p| p.raw())
}

#[test]
fn a_null_slot_reads_as_an_empty_value_and_saves_as_null() {
    let (stages, saved) = saves_as_tree::<Stages>(&stages_file(Value::null_ptr()));
    assert_eq!(stages.stages[0].marks, [1, 2]);
    assert!(stages.stages[1].marks.is_empty());
    let [first, second] = stage_pointers(&saved);
    assert!(first != 0 && second == 0, "{first:#x} {second:#x}");

    let mut edited = stages;
    edited.stages[1].marks.push(7);
    edited.stages[0].marks.clear();
    let saved = save(&edited);
    let [first, second] = stage_pointers(&saved);
    assert!(first == 0 && second != 0, "{first:#x} {second:#x}");
    assert_eq!(open::<Stages>(&saved).unwrap(), edited);

    assert_eq!(stage_pointers(&save(&Stages::default())), [0, 0]);
}

#[test]
fn a_live_slot_at_an_empty_value_is_written_back_live() {
    let empty = Value::ptr_to(tree(&Stage::default()));
    let (stages, saved) = saves_as_tree::<Stages>(&stages_file(empty));
    assert!(stages.stages[1].marks.is_empty());
    assert_eq!(
        stages.stages[1],
        NullIsEmpty::default(),
        "equal to a null slot"
    );
    let [_, second] = stage_pointers(&saved);
    assert_ne!(second, 0, "the file's allocation is kept");
}

#[test]
fn a_slot_over_a_record_generic_over_its_target_is_owned_for_either_target() {
    fn pointer<O: Owned>() -> bool {
        !O::LEAF
    }
    fn slot<T: Edition>() -> bool {
        pointer::<NullIsEmpty<Page<T>>>()
    }
    assert!(slot::<Gen8>() && slot::<Gen9>());
}

/// A texture-shaped record whose masked word may state zero beside its pixels.
#[decl::record(size = 0x20)]
pub struct Tile {
    #[phys]
    #[len(masked_or_mip_chain = "physical_size", mask = 0x07FF_FF80)]
    pub pixels: Vec<u8>,
    physical_size: u32,
    pub format: u32,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub mip_count: u8,
    pub cut_mip_levels: u8,
    #[pad_after(7)]
    pub layer_count: u8,
}

/// A 16x16 DXT1 tile of one level: 128 bytes, a whole multiple of the mask's lowest bit.
fn tile() -> Tile {
    Tile {
        pixels: (0..128).map(|i| i as u8).collect(),
        format: u32::from_le_bytes(*b"DXT1"),
        width: 16,
        height: 16,
        depth: 1,
        mip_count: 1,
        ..Default::default()
    }
}

fn physical_size(saved: &Saved) -> u32 {
    View::<Raw<Tile>>::at_offset(saved.image(), saved.root)
        .unwrap()
        .physical_size
}

/// `tile` saved, with its masked word then zeroed beside the live pixel pointer.
fn unstated_file(tile: &Tile) -> Saved {
    let saved = save(tile);
    let at = saved.root + std::mem::offset_of!(Raw<Tile>, physical_size);
    let mut bytes = saved.bytes.bytes().to_vec();
    bytes[at..at + 4].fill(0);
    Saved {
        bytes: Aligned::from_bytes(&bytes),
        ..saved
    }
}

#[test]
fn a_pixel_run_whose_word_states_zero_opens_at_its_implied_length() {
    let file = unstated_file(&tile());
    assert_eq!(physical_size(&file), 0);
    let (opened, saved) = saves_as_tree::<Tile>(&file);
    assert_eq!(opened.pixels, tile().pixels);
    assert_eq!(
        physical_size(&saved),
        0,
        "a word left alone saves as it was read"
    );

    let mut grown = opened;
    grown.pixels.resize(256, 0xEE);
    let saved = save(&grown);
    assert_eq!(
        physical_size(&saved),
        256,
        "a length the dimensions no longer imply is stated"
    );
    assert_eq!(open::<Tile>(&saved).unwrap(), grown);
}

/// Every dimension distinct, so a reader of the mip-chain siblings that takes
/// two of them in the other order reads another length; width and height
/// alone swap to the same length.
#[test]
fn a_pixel_run_whose_word_states_zero_reads_every_dimension_in_order() {
    let (format, width, height, depth, mip_count, cut_mip_levels, layer_count) =
        (u32::from_le_bytes(*b"DXT1"), 64, 32, 1, 2, 0, 3);
    let len = desc::CountSource::mip_chain_len(
        format,
        width,
        height,
        depth,
        mip_count,
        cut_mip_levels,
        layer_count,
    );
    let tile = Tile {
        pixels: (0..len).map(|i| i as u8).collect(),
        format,
        width: width as u16,
        height: height as u16,
        depth: depth as u16,
        mip_count: mip_count as u8,
        cut_mip_levels: cut_mip_levels as u8,
        layer_count: layer_count as u8,
        ..Default::default()
    };
    let file = unstated_file(&tile);
    let view = View::<Raw<Tile>>::at_offset(file.image(), file.root).unwrap();
    assert_eq!(view.pixels().unwrap().as_slice(), tile.pixels, "the view");
    let (opened, saved) = saves_as_tree::<Tile>(&file);
    assert_eq!(opened, tile, "the owned open");
    assert_eq!(physical_size(&saved), 0, "the owned save");
}

/// A texture whose header states a full mip chain over pixels that hold the first level and nothing after it.
#[test]
fn a_pixel_run_that_holds_fewer_levels_than_its_header_states_opens_at_the_levels_it_holds() {
    let format = u32::from_le_bytes(*b"DXT1");
    let first = desc::CountSource::mip_chain_len(format, 256, 256, 1, 1, 0, 0);
    let tile = Tile {
        pixels: (0..first).map(|i| i as u8).collect(),
        format,
        width: 256,
        height: 256,
        depth: 1,
        mip_count: 1,
        ..Default::default()
    };
    let saved = unstated_file(&tile);
    let at = saved.root + std::mem::offset_of!(Raw<Tile>, mip_count);
    let mut bytes = saved.bytes.bytes().to_vec();
    bytes[at] = 9;
    let file = Saved {
        bytes: Aligned::from_bytes(&bytes),
        ..saved
    };

    let opened: Tile = open(&file).unwrap();
    assert_eq!(opened.pixels, tile.pixels);
    assert_eq!(
        opened.mip_count, 9,
        "the header is read as the file states it"
    );

    let saved = save(&opened);
    assert_eq!(
        physical_size(&saved) as usize,
        first,
        "the word states the run the save writes"
    );
    assert_eq!(open::<Tile>(&saved).unwrap(), opened);
}

/// Two names a file may point at an empty string rather than leave null, as a particle spawner's are.
#[decl::record(size = 0x10)]
pub struct Names {
    pub first: String,
    pub second: String,
}

fn name_pointers(saved: &Saved) -> (u64, u64) {
    let n = View::<Raw<Names>>::at_offset(saved.image(), saved.root).unwrap();
    ({ n.first }.raw(), { n.second }.raw())
}

#[test]
fn a_live_pointer_at_an_empty_string_opens_empty_and_is_written_back_live() {
    let mut t = tree(&Names {
        second: "b".into(),
        ..Default::default()
    });
    *record(&mut t)
        .child_at_mut(std::mem::offset_of!(Raw<Names>, first))
        .unwrap() = Value::ptr_to(Value::Bytes(vec![0]));
    let file = build(&t);
    assert_ne!(name_pointers(&file).0, 0, "the file's pointer is live");
    let (names, saved) = saves_as_tree::<Names>(&file);
    assert_eq!((names.first.as_str(), names.second.as_str()), ("", "b"));
    assert_ne!(name_pointers(&saved).0, 0, "the empty string is kept");

    let mut named = names;
    named.first = "a".into();
    assert_eq!(open::<Names>(&save(&named)).unwrap().first, "a");

    assert_eq!(name_pointers(&save(&Names::default())), (0, 0));
}
