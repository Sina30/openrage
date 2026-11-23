//! Pointers a file aims at what another pointer in the tree owns, on invented
//! owned records: a run a descendant owns, an element of an embedded record's
//! pointer array, a slot that owns its target or names one a keyed map owns, a
//! keyed map entry a slot reached first, read as a copy and saved as the alias,
//! and a pointer slot holding a word that is not an address. Each opens, saves,
//! reads back equal and reaches a fixed point, and a value built from scratch
//! writes every pointer it implies.

use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Owned, OwnedOrKeyed, Raw, Record, Shared, View};
use or_io::desc::{self, Pack, Value};

#[decl::record(size = 0x10)]
pub struct Strip {
    #[len(field = "count")]
    pub bytes: Vec<u8>,
    #[pad_after(4)]
    count: u32,
}

/// A run that is its strip's run again, as a geometry points at its vertex buffer's vertices.
#[decl::record(size = 0x18)]
pub struct Mesh {
    pub strip: Box<Strip>,
    #[shares(strip.bytes)]
    #[len(field = "count")]
    pub data: Shared<Vec<u8>>,
    #[pad_after(4)]
    count: u32,
}

#[decl::record(size = 0x08)]
pub struct Lod {
    pub distance: f32,
    pub flags: u32,
}

#[decl::record(size = 0x10)]
pub struct Levels {
    pub lods: Vec<Option<Box<Lod>>>,
}

/// A pointer at the first level again, as a drawable's models block is.
#[decl::record(size = 0x18)]
pub struct Model {
    pub levels: Levels,
    #[shares(levels.lods[0])]
    pub first: Shared<Option<Box<Lod>>>,
}

#[decl::record(size = 0x20)]
pub struct Group {
    pub lods: [Option<Box<Lod>>; 4],
}

/// A pointer at the first of an embedded group's levels, which a reader reaches first, as a drawable's container is.
#[decl::record(size = 0x30)]
pub struct Tower {
    pub group: Group,
    #[shares(group.lods[0])]
    pub first: Shared<Option<Box<Lod>>>,
    pub name: String,
}

#[decl::record(size = 0x08)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
}

/// Tiles by code in two runs of one length, as a dictionary holds textures.
#[decl::record(size = 0x20)]
pub struct Rack {
    pub codes: Vec<u32>,
    #[keyed_by(codes)]
    pub entries: Vec<Option<Box<Tile>>>,
}

#[decl::record(size = 0x08)]
pub struct Binding {
    pub tile: OwnedOrKeyed<Tile>,
}

/// Bindings that own a tile or point into the rack, as a material's textures do.
#[decl::record(size = 0x18)]
pub struct Sheet {
    pub rack: Option<Box<Rack>>,
    pub bindings: Vec<Binding>,
}

/// Bindings before the rack, so a binding reaches a tile first and owns it, as a
/// particle rule's spawner owns the effect rule its list's dictionary holds again.
#[decl::record(size = 0x18)]
pub struct Board {
    pub bindings: Vec<Binding>,
    pub rack: Option<Box<Rack>>,
}

/// A name slot and a pointer slot a file may fill with words that are not addresses.
#[decl::record(size = 0x18)]
pub struct Label {
    pub name: String,
    pub tile: Option<Box<Tile>>,
    pub width: u32,
    pub height: u32,
}

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[
        decl::root::<Raw<Mesh>>,
        decl::root::<Raw<Model>>,
        decl::root::<Raw<Sheet>>,
        decl::root::<Raw<Board>>,
        decl::root::<Raw<Label>>,
        decl::root::<Raw<Tower>>,
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

    /// The eight bytes at `address`, or at the root's offset `at` for an address of zero.
    fn word(&self, address: u64, at: usize) -> u64 {
        let base = match address {
            0 => self.root,
            a => (a - FIXED_VIRTUAL_BASE) as usize,
        };
        let bytes = &self.bytes.bytes()[base + at..base + at + 8];
        u64::from_le_bytes(bytes.try_into().unwrap())
    }
}

fn build(pack: &Arc<Pack>, tree: &Value) -> Result<Saved, String> {
    let built = or_io::write::build(pack, tree).map_err(|e| e.to_string())?;
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    Ok(Saved {
        bytes: Aligned::from_bytes(&bytes),
        virtual_size,
        root: (built.root - FIXED_VIRTUAL_BASE) as usize,
    })
}

fn tree<T: Owned>(value: &T) -> Value {
    value.to_element(&Cx::new(pack().plans().clone())).unwrap()
}

fn save<T: Owned>(value: &T) -> Result<Saved, String> {
    let pack = pack();
    let tree = value
        .to_element(&Cx::new(pack.plans().clone()))
        .map_err(|e| e.to_string())?;
    build(&pack, &tree)
}

fn open<T: Owned>(saved: &Saved) -> Result<T, String>
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).map_err(|e| e.to_string())?;
    T::from_element(doc.value()).map_err(|e| e.to_string())
}

/// `value` saved, opened and saved again: equal to itself, and the second save byte-identical.
fn round_trip<T: Owned + PartialEq + std::fmt::Debug>(value: &T) -> Saved
where
    Raw<T>: Record,
{
    let first = save(value).unwrap();
    let again: T = open(&first).unwrap();
    assert_eq!(&again, value);
    let second = save(&again).unwrap();
    assert_eq!(second.bytes.bytes(), first.bytes.bytes(), "a fixed point");
    first
}

fn kids(tree: &mut Value) -> &mut desc::Record {
    match tree {
        Value::Struct(record) => record,
        other => panic!("a struct, not {other:?}"),
    }
}

/// The source the pointer `v` owns its target under, set to `address`.
fn own_at(v: &mut Value, address: u64) {
    match v {
        Value::Ptr {
            source,
            target: Some(_),
        } => *source = address,
        other => panic!("a live pointer, not {other:?}"),
    }
}

const SHARED: u64 = 0x5000_1000;

fn strip(bytes: &[u8]) -> Box<Strip> {
    Box::new(Strip {
        bytes: bytes.to_vec(),
        ..Default::default()
    })
}

/// A file whose mesh points its run at its strip's.
fn mesh_file() -> Saved {
    mesh_file_with(Value::Alias(SHARED))
}

/// A file whose strip owns its run and whose mesh's run slot holds `slot`.
fn mesh_file_with(slot: Value) -> Saved {
    let mesh = Mesh {
        strip: strip(&[1, 2, 3, 4]),
        data: Shared::Own(vec![1, 2, 3, 4]),
        ..Default::default()
    };
    let mut t = tree(&mesh);
    let record = kids(&mut t);
    let bytes = std::mem::offset_of!(Raw<Strip>, bytes);
    match record.child_at_mut(std::mem::offset_of!(Raw<Mesh>, strip)) {
        Some(Value::Ptr {
            target: Some(s), ..
        }) => own_at(kids(s).child_at_mut(bytes).unwrap(), SHARED),
        other => panic!("{other:?}"),
    }
    *record
        .child_at_mut(std::mem::offset_of!(Raw<Mesh>, data))
        .unwrap() = slot;
    build(&pack(), &t).unwrap()
}

/// Where a saved mesh's run and its strip's run are, and the count the mesh states.
fn mesh_runs(saved: &Saved) -> (u64, u64, u32) {
    let m = View::<Raw<Mesh>>::at_offset(saved.image(), saved.root).unwrap();
    let s = View::<Raw<Strip>>::at_address(saved.image(), { m.strip }.raw()).unwrap();
    ({ m.data }.raw(), { s.bytes }.raw(), m.count)
}

#[test]
fn a_run_that_is_a_descendants_run_opens_as_one() {
    let mesh: Mesh = open(&mesh_file()).unwrap();
    assert_eq!(mesh.data, Shared::Same);
    assert_eq!(mesh.strip.bytes, [1, 2, 3, 4]);
    let saved = round_trip(&mesh);
    let (data, bytes, count) = mesh_runs(&saved);
    assert!(data != 0 && data == bytes, "{data:#x} {bytes:#x}");
    assert_eq!(count, 4, "the count states the strip's run");
}

#[test]
fn a_mesh_built_from_scratch_points_at_its_strips_run() {
    let mesh = Mesh {
        strip: strip(&[9, 8, 7]),
        data: Shared::Same,
        ..Default::default()
    };
    let (data, bytes, count) = mesh_runs(&round_trip(&mesh));
    assert!(data != 0 && data == bytes, "{data:#x} {bytes:#x}");
    assert_eq!(count, 3);

    let apart = Mesh {
        data: Shared::Own(vec![9, 8, 7]),
        ..mesh
    };
    let (data, bytes, _) = mesh_runs(&round_trip(&apart));
    assert_ne!(data, bytes, "an owned run is its own");
}

const STALE_WORD: u64 = 0x13a5_7b5c;

/// `value` saved as a save asks for, and the repairs it made.
fn save_repairing<T: Owned>(value: &T, repair: decl::Repair) -> (Saved, Vec<String>) {
    let pack = pack();
    let cx = Cx::new(pack.plans().clone()).repairing(repair);
    let tree = value.to_element(&cx).unwrap();
    (build(&pack, &tree).unwrap(), cx.repairs())
}

#[test]
fn a_shared_slot_holding_a_word_that_is_not_an_address_opens_as_shared_and_keeps_the_word() {
    let mesh: Mesh = open(&mesh_file_with(Value::NotAnAddress(STALE_WORD))).unwrap();
    assert_eq!(mesh.data, Shared::Same);
    assert_eq!(mesh.strip.bytes, [1, 2, 3, 4]);

    let (data, bytes, count) = mesh_runs(&round_trip(&mesh));
    assert_eq!(
        data, STALE_WORD,
        "the word is written back as the file had it"
    );
    assert!(bytes != 0 && bytes != STALE_WORD, "{bytes:#x}");
    assert_eq!(count, 4);
}

#[test]
fn a_save_that_repairs_points_a_stale_slot_at_the_run_it_shares() {
    let mesh: Mesh = open(&mesh_file_with(Value::NotAnAddress(STALE_WORD))).unwrap();

    let (saved, repairs) = save_repairing(&mesh, decl::Repair::Lossless);
    let (data, bytes, _) = mesh_runs(&saved);
    assert!(data != 0 && data == bytes, "{data:#x} {bytes:#x}");
    assert_eq!(repairs.len(), 1, "{repairs:?}");
    assert!(
        repairs[0].contains("Mesh.data") && repairs[0].contains("0x13a57b5c"),
        "{repairs:?}"
    );

    let (kept, repairs) = save_repairing(&mesh, decl::Repair::Refuse);
    assert_eq!(mesh_runs(&kept).0, STALE_WORD);
    assert!(repairs.is_empty(), "{repairs:?}");

    let sound: Mesh = open(&mesh_file()).unwrap();
    assert!(save_repairing(&sound, decl::Repair::Lossless).1.is_empty());
}

#[test]
fn a_stale_slot_given_a_run_of_its_own_drops_the_word() {
    let mut mesh: Mesh = open(&mesh_file_with(Value::NotAnAddress(STALE_WORD))).unwrap();
    mesh.data = Shared::Own(vec![9, 9, 9, 9]);
    let saved = save(&mesh).unwrap();
    let (data, bytes, _) = mesh_runs(&saved);
    assert!(
        data != STALE_WORD && data != 0 && data != bytes,
        "{data:#x}"
    );
    assert_eq!(
        open::<Mesh>(&saved).unwrap().data,
        Shared::Own(vec![9, 9, 9, 9])
    );
}

fn lod(distance: f32) -> Option<Box<Lod>> {
    Some(Box::new(Lod {
        distance,
        flags: 1,
        ..Default::default()
    }))
}

/// A file whose model points `first` at the first of its levels, `first` owning it.
fn model_file() -> Saved {
    let model = Model {
        levels: Levels {
            lods: vec![lod(10.0), lod(40.0)],
            ..Default::default()
        },
        first: Shared::Own(lod(10.0)),
        ..Default::default()
    };
    let mut t = tree(&model);
    let record = kids(&mut t);
    own_at(
        record
            .child_at_mut(std::mem::offset_of!(Raw<Model>, first))
            .unwrap(),
        SHARED,
    );
    let lods = std::mem::offset_of!(Raw<Model>, levels) + std::mem::offset_of!(Raw<Levels>, lods);
    match record.child_at_mut(lods) {
        Some(Value::Ptr {
            target: Some(run), ..
        }) => match run.as_mut() {
            Value::Array { elements, .. } => elements[0] = Value::Alias(SHARED),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    build(&pack(), &t).unwrap()
}

/// Where a saved model's `first` points, and where its first level is.
fn model_pointers(saved: &Saved) -> (u64, u64) {
    let m = View::<Raw<Model>>::at_offset(saved.image(), saved.root).unwrap();
    let lods = { m.levels.lods }.raw();
    let first_lod = if lods == 0 { 0 } else { saved.word(lods, 0) };
    ({ m.first }.raw(), first_lod)
}

#[test]
fn a_pointer_at_an_element_of_an_embedded_pointer_array_opens_as_one() {
    let model: Model = open(&model_file()).unwrap();
    assert_eq!(model.first, Shared::Same);
    assert_eq!(model.levels.lods, [lod(10.0), lod(40.0)]);
    let (first, lod0) = model_pointers(&round_trip(&model));
    assert!(first != 0 && first == lod0, "{first:#x} {lod0:#x}");
}

#[test]
fn a_model_built_from_scratch_points_at_its_first_level_or_nowhere() {
    let model = Model {
        levels: Levels {
            lods: vec![lod(5.0)],
            ..Default::default()
        },
        first: Shared::Same,
        ..Default::default()
    };
    let (first, lod0) = model_pointers(&round_trip(&model));
    assert!(first != 0 && first == lod0, "{first:#x} {lod0:#x}");

    let empty = Model {
        first: Shared::Same,
        ..Default::default()
    };
    let saved = save(&empty).unwrap();
    assert_eq!(model_pointers(&saved), (0, 0), "no level, so null");
    let again: Model = open(&saved).unwrap();
    assert_eq!(
        again.first,
        Shared::Own(None),
        "a null reads as its own null"
    );
    assert_eq!(save(&again).unwrap().bytes.bytes(), saved.bytes.bytes());
}

/// `saved` read as a tree and laid out again by the canonical writer.
fn tree_save<T: Owned>(saved: &Saved) -> Saved
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).unwrap();
    build(&pack(), doc.value()).unwrap()
}

/// A file whose tower points `first` at the first of its group's levels, `first` owning it.
fn tower_file() -> Saved {
    let tower = Tower {
        group: Group {
            lods: [lod(10.0), lod(40.0), None, None],
            ..Default::default()
        },
        first: Shared::Own(lod(10.0)),
        name: "tower".into(),
        ..Default::default()
    };
    let mut t = tree(&tower);
    let record = kids(&mut t);
    own_at(
        record
            .child_at_mut(std::mem::offset_of!(Raw<Tower>, first))
            .unwrap(),
        SHARED,
    );
    let lods = std::mem::offset_of!(Raw<Tower>, group) + std::mem::offset_of!(Raw<Group>, lods);
    match record.child_at_mut(lods) {
        Some(Value::Array { elements, .. }) => elements[0] = Value::Alias(SHARED),
        other => panic!("{other:?}"),
    }
    build(&pack(), &t).unwrap()
}

#[test]
fn a_pointer_read_before_the_embedded_one_it_shares_with_saves_where_the_tree_does() {
    let file = tower_file();
    let tower: Tower = open(&file).unwrap();
    assert_eq!(tower.first, Shared::Same);
    assert_eq!(
        save(&tower).unwrap().bytes.bytes(),
        tree_save::<Tower>(&file).bytes.bytes(),
        "the first level is laid out at `first`'s turn, where the tree owns it"
    );

    let scratch = Tower {
        group: Group {
            lods: [lod(5.0), None, lod(6.0), None],
            ..Default::default()
        },
        first: Shared::Same,
        name: "scratch".into(),
        ..Default::default()
    };
    let saved = save(&scratch).unwrap();
    assert_eq!(
        saved.bytes.bytes(),
        tree_save::<Tower>(&saved).bytes.bytes(),
        "a value built from scratch saves in the canonical form"
    );
    assert_eq!(open::<Tower>(&saved).unwrap(), scratch);
}

fn tile(width: u32) -> Option<Box<Tile>> {
    Some(Box::new(Tile {
        width,
        height: width * 2,
        ..Default::default()
    }))
}

fn binding(tile: OwnedOrKeyed<Tile>) -> Binding {
    Binding {
        tile,
        ..Default::default()
    }
}

fn rack() -> Option<Box<Rack>> {
    Some(Box::new(Rack {
        codes: vec![0x10, 0x20],
        entries: vec![tile(1), tile(2)],
        ..Default::default()
    }))
}

/// A file whose first binding points at the rack's second tile, and whose second owns its own.
fn sheet_file() -> Saved {
    let sheet = Sheet {
        rack: rack(),
        bindings: vec![
            binding(OwnedOrKeyed::Own(tile(2))),
            binding(OwnedOrKeyed::Own(tile(3))),
        ],
        ..Default::default()
    };
    let mut t = tree(&sheet);
    let record = kids(&mut t);
    let entries = std::mem::offset_of!(Raw<Rack>, entries);
    match record.child_at_mut(std::mem::offset_of!(Raw<Sheet>, rack)) {
        Some(Value::Ptr {
            target: Some(rack), ..
        }) => match kids(rack).child_at_mut(entries) {
            Some(Value::Ptr {
                target: Some(run), ..
            }) => match run.as_mut() {
                Value::Array { elements, .. } => own_at(&mut elements[1], SHARED),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    match record.child_at_mut(std::mem::offset_of!(Raw<Sheet>, bindings)) {
        Some(Value::Ptr {
            target: Some(run), ..
        }) => match run.as_mut() {
            Value::Array { elements, .. } => {
                *kids(&mut elements[0]).child_at_mut(0).unwrap() = Value::Alias(SHARED)
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    build(&pack(), &t).unwrap()
}

/// Where each of a saved sheet's rack tiles is, and where each binding points.
fn sheet_pointers(saved: &Saved) -> (Vec<u64>, Vec<u64>) {
    let s = View::<Raw<Sheet>>::at_offset(saved.image(), saved.root).unwrap();
    let r = View::<Raw<Rack>>::at_address(saved.image(), { s.rack }.raw()).unwrap();
    let entries = { r.entries };
    let tiles = (0..entries.count())
        .map(|i| saved.word(entries.raw(), i * 8))
        .collect();
    let bindings = { s.bindings };
    let points = (0..bindings.count())
        .map(|i| saved.word(bindings.raw(), i * 8))
        .collect();
    (tiles, points)
}

#[test]
fn a_binding_into_the_rack_is_held_by_its_code() {
    let sheet: Sheet = open(&sheet_file()).unwrap();
    let tiles: Vec<&OwnedOrKeyed<Tile>> = sheet.bindings.iter().map(|b| &b.tile).collect();
    assert_eq!(
        tiles,
        [&OwnedOrKeyed::Key(0x20), &OwnedOrKeyed::Own(tile(3))]
    );
    let (tiles, points) = sheet_pointers(&round_trip(&sheet));
    assert_eq!(
        points[0], tiles[1],
        "the first binding points into the rack"
    );
    assert!(
        points[1] != 0 && !tiles.contains(&points[1]),
        "the second owns its own"
    );
}

#[test]
fn a_sheet_built_from_scratch_points_each_code_at_its_tile() {
    let sheet = Sheet {
        rack: rack(),
        bindings: vec![
            binding(OwnedOrKeyed::Key(0x10)),
            binding(OwnedOrKeyed::Own(None)),
            binding(OwnedOrKeyed::Key(0x20)),
        ],
        ..Default::default()
    };
    let (tiles, points) = sheet_pointers(&round_trip(&sheet));
    assert_eq!(points, [tiles[0], 0, tiles[1]]);

    let mut dangling = sheet;
    dangling.bindings.push(binding(OwnedOrKeyed::Key(0x30)));
    let Err(why) = save(&dangling) else {
        panic!("a code the rack does not hold is refused")
    };
    assert!(why.contains("nothing in this tree wrote"), "{why}");
}

const NAME_WORD: u64 = 0x120;
const TILE_WORD: u64 = 0x7bc0;

/// A file whose name and tile slots hold small words that are not addresses.
fn label_file() -> Saved {
    let label = Label {
        width: 5,
        height: 6,
        ..Default::default()
    };
    let mut t = tree(&label);
    let record = kids(&mut t);
    *record
        .child_at_mut(std::mem::offset_of!(Raw<Label>, name))
        .unwrap() = Value::NotAnAddress(NAME_WORD);
    *record
        .child_at_mut(std::mem::offset_of!(Raw<Label>, tile))
        .unwrap() = Value::NotAnAddress(TILE_WORD);
    build(&pack(), &t).unwrap()
}

fn label_words(saved: &Saved) -> (u64, u64) {
    let l = View::<Raw<Label>>::at_offset(saved.image(), saved.root).unwrap();
    ({ l.name }.raw(), { l.tile }.raw())
}

#[test]
fn a_word_that_is_not_an_address_reads_empty_and_is_written_back() {
    let label: Label = open(&label_file()).unwrap();
    assert_eq!((label.name.as_str(), &label.tile), ("", &None));
    assert_eq!((label.width, label.height), (5, 6));
    let saved = round_trip(&label);
    assert_eq!(label_words(&saved), (NAME_WORD, TILE_WORD));

    let mut named = label;
    named.name = "hull".into();
    named.tile = tile(7);
    let saved = round_trip(&named);
    let (name, tile) = label_words(&saved);
    assert!(
        name != NAME_WORD && name != 0 && tile != TILE_WORD && tile != 0,
        "a slot given a value points at it: {name:#x} {tile:#x}"
    );

    let fresh = Label::default();
    assert_eq!(label_words(&round_trip(&fresh)), (0, 0));
}

/// A file whose first binding owns a tile its rack holds again as its second entry, and whose second binding names that entry by code.
fn board_file() -> Saved {
    let board = Board {
        bindings: vec![
            binding(OwnedOrKeyed::Own(tile(2))),
            binding(OwnedOrKeyed::Own(tile(3))),
        ],
        rack: rack(),
        ..Default::default()
    };
    let mut t = tree(&board);
    let record = kids(&mut t);
    match record.child_at_mut(std::mem::offset_of!(Raw<Board>, bindings)) {
        Some(Value::Ptr {
            target: Some(run), ..
        }) => match run.as_mut() {
            Value::Array { elements, .. } => {
                own_at(kids(&mut elements[0]).child_at_mut(0).unwrap(), SHARED);
                *kids(&mut elements[1]).child_at_mut(0).unwrap() = Value::Alias(SHARED);
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    let entries = std::mem::offset_of!(Raw<Rack>, entries);
    match record.child_at_mut(std::mem::offset_of!(Raw<Board>, rack)) {
        Some(Value::Ptr {
            target: Some(rack), ..
        }) => match kids(rack).child_at_mut(entries) {
            Some(Value::Ptr {
                target: Some(run), ..
            }) => match run.as_mut() {
                Value::Array { elements, .. } => elements[1] = Value::Alias(SHARED),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    build(&pack(), &t).unwrap()
}

/// Where a saved board's rack tiles are, and where each binding points.
fn board_pointers(saved: &Saved) -> (Vec<u64>, Vec<u64>) {
    let b = View::<Raw<Board>>::at_offset(saved.image(), saved.root).unwrap();
    let r = View::<Raw<Rack>>::at_address(saved.image(), { b.rack }.raw()).unwrap();
    let entries = { r.entries };
    let tiles = (0..entries.count())
        .map(|i| saved.word(entries.raw(), i * 8))
        .collect();
    let bindings = { b.bindings };
    let points = (0..bindings.count())
        .map(|i| saved.word(bindings.raw(), i * 8))
        .collect();
    (tiles, points)
}

#[test]
fn a_rack_entry_a_binding_reached_first_opens_as_a_copy_and_saves_as_the_alias() {
    let file = board_file();
    let board: Board = open(&file).unwrap();
    let tiles: Vec<&OwnedOrKeyed<Tile>> = board.bindings.iter().map(|b| &b.tile).collect();
    assert_eq!(
        tiles,
        [&OwnedOrKeyed::Own(tile(2)), &OwnedOrKeyed::Key(0x20)]
    );
    assert_eq!(board.rack.as_ref().unwrap().entries[1], tile(2));

    let saved = save(&board).unwrap();
    assert_eq!(
        saved.bytes.bytes(),
        tree_save::<Board>(&file).bytes.bytes(),
        "the entry saves as the alias the tree writes"
    );
    let (tiles, points) = board_pointers(&saved);
    assert_eq!(points, [tiles[1], tiles[1]]);
    assert_eq!(open::<Board>(&saved).unwrap(), board);
    assert_eq!(save(&board).unwrap().bytes.bytes(), saved.bytes.bytes());

    let mut apart = board;
    apart.rack.as_mut().unwrap().entries[1] = tile(5);
    let saved = save(&apart).unwrap();
    let (tiles, points) = board_pointers(&saved);
    assert!(
        points[0] != tiles[1] && points[1] == tiles[1],
        "an edited entry saves apart, and the code names it: {points:x?} {tiles:x?}"
    );
    // The binding naming it now reaches it first, so it opens owning it.
    let reopened: Board = open(&saved).unwrap();
    assert_eq!(reopened.bindings[1].tile, OwnedOrKeyed::Own(tile(5)));
    assert_eq!(reopened.rack.as_ref().unwrap().entries[1], tile(5));
    round_trip(&reopened);
}
