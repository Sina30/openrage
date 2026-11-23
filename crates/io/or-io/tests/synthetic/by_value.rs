//! Pointers a file aims at a target another pointer owns elsewhere in the
//! tree, held by value, on invented owned records: a record's pointer, an
//! element of a run of pointers, a string, a whole record and a whole run each
//! read as a copy of their owner, save as the alias while the copy still equals
//! it, and apart once an edit makes them differ.

use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Owned, Raw, Record, View};
use or_io::desc::{self, Pack, Value};

#[decl::record(size = 0x08)]
pub struct Bone {
    pub tag: u32,
    pub length: f32,
}

#[decl::record(size = 0x10)]
pub struct Rig {
    pub bones: Vec<Bone>,
}

/// A record owning a rig and a name, as a drawable owns its skeleton.
#[decl::record(size = 0x10)]
pub struct Body {
    pub rig: Option<Box<Rig>>,
    pub name: String,
}

/// A common body whose rig, name and self its parts, a run of rigs, a label and a cloth alias, as a fragment's do.
#[decl::record(size = 0x38)]
pub struct Figure {
    pub common: Option<Box<Body>>,
    pub parts: Vec<Option<Box<Body>>>,
    #[len(field = "rig_count")]
    pub rigs: Vec<Option<Box<Rig>>>,
    pub label: String,
    pub cloth: Option<Box<Body>>,
    #[pad_after(4)]
    rig_count: u32,
}

#[decl::record(size = 0x18)]
pub struct Plate {
    pub mass: f32,
    pub tag: u32,
    pub name: [u8; 16],
}

/// A table of names each pointing into a plate's own name, as a physics level's group names do.
#[decl::record(size = 0x18)]
pub struct Rack {
    #[len(field = "count")]
    pub names: Vec<String>,
    #[len(field = "count")]
    pub plates: Vec<Option<Box<Plate>>>,
    #[pad_after(4)]
    count: u32,
}

#[decl::record(size = 0x08)]
pub struct Knot {
    pub tag: u32,
    pub weight: f32,
}

/// A palette and a counted run of knots another strand may share, as a modded ped's geometries share a matrix palette.
#[decl::record(size = 0x20)]
pub struct Strand {
    #[len(field = "palette_count")]
    pub palette: Vec<u16>,
    pub knots: Vec<Knot>,
    #[pad_after(6)]
    palette_count: u16,
}

#[decl::record(size = 0x10)]
pub struct Braid {
    pub strands: Vec<Option<Box<Strand>>>,
}

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[
        decl::root::<Raw<Figure>>,
        decl::root::<Raw<Rack>>,
        decl::root::<Raw<Braid>>,
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

    /// The eight bytes at `address` plus `at`.
    fn word(&self, address: u64, at: usize) -> u64 {
        let base = (address - FIXED_VIRTUAL_BASE) as usize + at;
        u64::from_le_bytes(self.bytes.bytes()[base..base + 8].try_into().unwrap())
    }
}

fn build(tree: &Value) -> Result<Saved, String> {
    let built = or_io::write::build(&pack(), tree).map_err(|e| e.to_string())?;
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
    let tree = value
        .to_element(&Cx::new(pack().plans().clone()))
        .map_err(|e| e.to_string())?;
    build(&tree)
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
    build(doc.value()).unwrap()
}

fn kids(v: &mut Value) -> &mut desc::Record {
    match v {
        Value::Struct(record) => record,
        Value::Ptr {
            target: Some(t), ..
        } => kids(t),
        other => panic!("a struct, not {other:?}"),
    }
}

fn elements(v: &mut Value) -> &mut Vec<Value> {
    match v {
        Value::Array { elements, .. } => elements,
        Value::Ptr {
            target: Some(t), ..
        } => elements(t),
        other => panic!("a run, not {other:?}"),
    }
}

/// The live pointer `v` made the owner of its target at `address`.
fn own_at(v: &mut Value, address: u64) {
    match v {
        Value::Ptr {
            source,
            target: Some(_),
        } => *source = address,
        other => panic!("a live pointer, not {other:?}"),
    }
}

const BODY: u64 = 0x5000_1000;
const RIG: u64 = 0x5000_2000;
const NAME: u64 = 0x5000_3000;
const PART_RIG: u64 = 0x5000_4000;

fn rig(lengths: &[f32]) -> Option<Box<Rig>> {
    Some(Box::new(Rig {
        bones: lengths
            .iter()
            .enumerate()
            .map(|(i, &length)| Bone {
                tag: i as u32,
                length,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }))
}

fn body(lengths: &[f32], name: &str) -> Option<Box<Body>> {
    Some(Box::new(Body {
        rig: rig(lengths),
        name: name.into(),
        ..Default::default()
    }))
}

/// What the file below holds, every alias read as a copy of its owner.
fn figure() -> Figure {
    Figure {
        common: body(&[1.0, 2.0], "hull"),
        parts: vec![body(&[1.0, 2.0], "hull"), body(&[5.0], "door")],
        rigs: vec![rig(&[5.0]), rig(&[1.0, 2.0])],
        label: "hull".into(),
        cloth: body(&[1.0, 2.0], "hull"),
        ..Default::default()
    }
}

/// Each shape of alias a file below may hold.
const SHAPES: [&str; 5] = ["record", "string", "element", "label", "whole"];

/// A file whose common body owns a rig and a name that its first part, the
/// run of rigs and the label alias, and that the cloth aliases whole; the
/// first rig of the run aliases the second part's rig.
fn figure_file() -> Saved {
    figure_with(&SHAPES)
}

/// [`figure_file`] holding only the aliases `shapes` names, and owning the rest.
fn figure_with(shapes: &[&str]) -> Saved {
    let mut t = tree(&figure());
    let at = |f| match f {
        "common" => std::mem::offset_of!(Raw<Figure>, common),
        "parts" => std::mem::offset_of!(Raw<Figure>, parts),
        "rigs" => std::mem::offset_of!(Raw<Figure>, rigs),
        "label" => std::mem::offset_of!(Raw<Figure>, label),
        "cloth" => std::mem::offset_of!(Raw<Figure>, cloth),
        _ => unreachable!(),
    };
    let (rig_at, name_at) = (
        std::mem::offset_of!(Raw<Body>, rig),
        std::mem::offset_of!(Raw<Body>, name),
    );
    let root = kids(&mut t);
    let common = root.child_at_mut(at("common")).unwrap();
    own_at(common, BODY);
    own_at(kids(common).child_at_mut(rig_at).unwrap(), RIG);
    own_at(kids(common).child_at_mut(name_at).unwrap(), NAME);
    let parts = elements(root.child_at_mut(at("parts")).unwrap());
    if shapes.contains(&"record") {
        *kids(&mut parts[0]).child_at_mut(rig_at).unwrap() = Value::Alias(RIG);
    }
    if shapes.contains(&"string") {
        *kids(&mut parts[0]).child_at_mut(name_at).unwrap() = Value::Alias(NAME);
    }
    own_at(kids(&mut parts[1]).child_at_mut(rig_at).unwrap(), PART_RIG);
    if shapes.contains(&"element") {
        let rigs = elements(root.child_at_mut(at("rigs")).unwrap());
        rigs[0] = Value::Alias(PART_RIG);
        rigs[1] = Value::Alias(RIG);
    }
    if shapes.contains(&"label") {
        *root.child_at_mut(at("label")).unwrap() = Value::Alias(NAME);
    }
    if shapes.contains(&"whole") {
        *root.child_at_mut(at("cloth")).unwrap() = Value::Alias(BODY);
    }
    build(&t).unwrap()
}

#[test]
fn each_shape_alone_opens_as_its_copy_and_saves_as_the_tree_does() {
    let mut refused = Vec::new();
    for shape in SHAPES {
        let file = figure_with(&[shape]);
        let opened = match open::<Figure>(&file) {
            Ok(opened) => opened,
            Err(why) => {
                refused.push(format!("{shape}: {why}"));
                continue;
            }
        };
        assert_eq!(opened, figure(), "{shape}");
        assert_eq!(
            save(&opened).unwrap().bytes.bytes(),
            tree_save::<Figure>(&file).bytes.bytes(),
            "{shape}"
        );
    }
    assert!(refused.is_empty(), "{refused:#?}");
}

/// Where each pointer of a saved figure points.
#[derive(Debug)]
struct Aims {
    common: u64,
    common_rig: u64,
    common_name: u64,
    part_rigs: Vec<u64>,
    part_names: Vec<u64>,
    rigs: Vec<u64>,
    label: u64,
    cloth: u64,
}

fn aims(saved: &Saved) -> Aims {
    let f = View::<Raw<Figure>>::at_offset(saved.image(), saved.root).unwrap();
    let body = |address: u64| {
        if address == 0 {
            return (0, 0);
        }
        let b = View::<Raw<Body>>::at_address(saved.image(), address).unwrap();
        ({ b.rig }.raw(), { b.name }.raw())
    };
    let (common_rig, common_name) = body({ f.common }.raw());
    let parts = { f.parts };
    let (part_rigs, part_names) = (0..parts.count())
        .map(|i| body(saved.word(parts.raw(), i * 8)))
        .unzip();
    let rigs = (0..f.rig_count as usize)
        .map(|i| saved.word({ f.rigs }.raw(), i * 8))
        .collect();
    Aims {
        common: { f.common }.raw(),
        common_rig,
        common_name,
        part_rigs,
        part_names,
        rigs,
        label: { f.label }.raw(),
        cloth: { f.cloth }.raw(),
    }
}

#[test]
fn each_alias_opens_as_a_copy_of_its_owner() {
    let opened: Figure = open(&figure_file()).unwrap();
    assert_eq!(opened, figure());
}

#[test]
fn an_untouched_open_saves_as_the_tree_does() {
    let file = figure_file();
    let saved = save(&open::<Figure>(&file).unwrap()).unwrap();
    assert_eq!(
        saved.bytes.bytes(),
        tree_save::<Figure>(&file).bytes.bytes(),
        "every alias written as the alias"
    );
    let a = aims(&saved);
    assert_eq!(a.part_rigs[0], a.common_rig, "{a:x?}");
    assert_eq!(a.part_names[0], a.common_name, "{a:x?}");
    assert_eq!(a.rigs, [a.part_rigs[1], a.common_rig], "{a:x?}");
    assert_eq!((a.label, a.cloth), (a.common_name, a.common), "{a:x?}");
    let again = save(&open::<Figure>(&saved).unwrap()).unwrap();
    assert_eq!(again.bytes.bytes(), saved.bytes.bytes(), "a fixed point");
}

/// `value` saved, then opened, equal to it, and saved again byte-identical.
fn fixed_point(value: &Figure) -> Saved {
    let first = save(value).unwrap();
    let again: Figure = open(&first).unwrap();
    assert_eq!(&again, value);
    assert_eq!(
        save(&again).unwrap().bytes.bytes(),
        first.bytes.bytes(),
        "a fixed point"
    );
    first
}

#[test]
fn an_edit_that_makes_a_copy_differ_saves_it_apart() {
    let file = figure_file();

    let mut named: Figure = open(&file).unwrap();
    named.parts[0].as_mut().unwrap().name = "keel".into();
    named.label = "mast".into();
    let a = aims(&fixed_point(&named));
    assert!(
        a.part_names[0] != a.common_name && a.label != a.common_name,
        "{a:x?}"
    );
    assert_eq!(
        a.part_rigs[0], a.common_rig,
        "an unedited copy stays the alias"
    );
    assert_eq!(a.cloth, a.common, "{a:x?}");

    let mut rigged: Figure = open(&file).unwrap();
    rigged.rigs[0].as_mut().unwrap().bones[0].length = 9.0;
    rigged.cloth.as_mut().unwrap().rig = None;
    let a = aims(&fixed_point(&rigged));
    assert!(a.rigs[0] != a.part_rigs[1] && a.rigs[0] != 0, "{a:x?}");
    assert_eq!(a.rigs[1], a.common_rig, "{a:x?}");
    assert!(a.cloth != a.common && a.cloth != 0, "{a:x?}");

    let mut owner: Figure = open(&file).unwrap();
    owner.common.as_mut().unwrap().rig = rig(&[3.0]);
    let a = aims(&fixed_point(&owner));
    assert!(a.part_rigs[0] != a.common_rig, "{a:x?}");
    assert!(
        a.rigs[1] != a.common_rig && a.rigs[1] != a.part_rigs[0],
        "{a:x?}"
    );
    assert!(a.cloth != a.common, "{a:x?}");
    assert_eq!(a.part_names[0], a.common_name, "{a:x?}");

    let mut gone: Figure = open(&file).unwrap();
    gone.common = None;
    let a = aims(&fixed_point(&gone));
    assert_eq!(a.common, 0);
    assert!(
        a.part_rigs[0] != 0 && a.cloth != 0 && a.label != 0,
        "{a:x?}"
    );
}

#[test]
fn an_alias_no_pointer_owns_is_refused_at_open() {
    let mut t = tree(&figure());
    *kids(&mut t)
        .child_at_mut(std::mem::offset_of!(Raw<Figure>, label))
        .unwrap() = Value::Alias(0x5000_9000);
    let Err(why) = Figure::from_element(&t) else {
        panic!("an alias of nothing is refused")
    };
    assert!(why.to_string().contains("no pointer"), "{why}");
}

fn plate(tag: u32, name: &str) -> Option<Box<Plate>> {
    let mut bytes = [0; 16];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Some(Box::new(Plate {
        mass: tag as f32,
        tag,
        name: bytes,
        ..Default::default()
    }))
}

/// A file whose names each point into a plate's own name.
fn rack_file() -> Saved {
    let rack = Rack {
        names: vec!["bow".into(), "stern".into()],
        plates: vec![plate(1, "bow"), plate(2, "stern")],
        ..Default::default()
    };
    let saved = save(&rack).unwrap();
    let r = View::<Raw<Rack>>::at_offset(saved.image(), saved.root).unwrap();
    let (names, plates) = ({ r.names }.raw(), { r.plates }.raw());
    let mut bytes = saved.bytes.bytes().to_vec();
    for i in 0..2 {
        let into = saved.word(plates, i * 8) + std::mem::offset_of!(Raw<Plate>, name) as u64;
        let slot = (names - FIXED_VIRTUAL_BASE) as usize + i * 8;
        bytes[slot..slot + 8].copy_from_slice(&into.to_le_bytes());
    }
    Saved {
        bytes: Aligned::from_bytes(&bytes),
        ..saved
    }
}

#[test]
fn a_name_pointing_into_another_records_member_opens_and_saves_as_the_tree_does() {
    let file = rack_file();
    let rack: Rack = open(&file).unwrap();
    assert_eq!(rack.names, ["bow", "stern"]);
    assert_eq!(
        save(&rack).unwrap().bytes.bytes(),
        tree_save::<Rack>(&file).bytes.bytes()
    );
}

fn strand(palette: &[u16], weights: &[f32]) -> Option<Box<Strand>> {
    Some(Box::new(Strand {
        palette: palette.to_vec(),
        knots: weights
            .iter()
            .enumerate()
            .map(|(i, &weight)| Knot {
                tag: i as u32,
                weight,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }))
}

/// What the file below holds, the second strand's runs read as copies of the first's.
fn braid() -> Braid {
    Braid {
        strands: vec![
            strand(&[3, 1, 4], &[0.5, 2.5]),
            strand(&[3, 1, 4], &[0.5, 2.5]),
        ],
        ..Default::default()
    }
}

const PALETTE: u64 = 0x5000_5000;
const KNOTS: u64 = 0x5000_6000;

/// The tree of [`braid`] whose second strand's palette and knots alias the first strand's.
fn braid_tree() -> Value {
    let mut t = tree(&braid());
    let (palette, knots) = (
        std::mem::offset_of!(Raw<Strand>, palette),
        std::mem::offset_of!(Raw<Strand>, knots),
    );
    let strands = elements(
        kids(&mut t)
            .child_at_mut(std::mem::offset_of!(Raw<Braid>, strands))
            .unwrap(),
    );
    own_at(
        kids(&mut strands[0]).child_at_mut(palette).unwrap(),
        PALETTE,
    );
    own_at(kids(&mut strands[0]).child_at_mut(knots).unwrap(), KNOTS);
    *kids(&mut strands[1]).child_at_mut(palette).unwrap() = Value::Alias(PALETTE);
    *kids(&mut strands[1]).child_at_mut(knots).unwrap() = Value::Alias(KNOTS);
    t
}

/// Per strand of a saved braid: where its palette and its knots are, and the knots' count.
fn strand_aims(saved: &Saved) -> Vec<(u64, u64, usize)> {
    let b = View::<Raw<Braid>>::at_offset(saved.image(), saved.root).unwrap();
    let strands = { b.strands };
    (0..strands.count())
        .map(|i| {
            let address = saved.word(strands.raw(), i * 8);
            let s = View::<Raw<Strand>>::at_address(saved.image(), address).unwrap();
            let knots = { s.knots };
            ({ s.palette }.raw(), knots.raw(), knots.count())
        })
        .collect()
}

#[test]
fn a_run_another_pointer_owns_opens_as_a_copy_and_saves_as_the_alias() {
    let file = build(&braid_tree()).unwrap();
    let opened: Braid = open(&file).unwrap();
    assert_eq!(opened, braid());
    let saved = save(&opened).unwrap();
    assert_eq!(
        saved.bytes.bytes(),
        tree_save::<Braid>(&file).bytes.bytes(),
        "each shared run written as the alias"
    );
    let a = strand_aims(&saved);
    assert_eq!(a[0], a[1], "{a:x?}");
    let again = save(&open::<Braid>(&saved).unwrap()).unwrap();
    assert_eq!(again.bytes.bytes(), saved.bytes.bytes(), "a fixed point");
}

/// `value` saved, then opened, equal to it, and saved again byte-identical.
fn braid_fixed_point(value: &Braid) -> Saved {
    let first = save(value).unwrap();
    let again: Braid = open(&first).unwrap();
    assert_eq!(&again, value);
    assert_eq!(
        save(&again).unwrap().bytes.bytes(),
        first.bytes.bytes(),
        "a fixed point"
    );
    first
}

#[test]
fn an_edit_to_one_copy_of_a_shared_run_saves_it_apart() {
    let file = build(&braid_tree()).unwrap();

    let mut copy: Braid = open(&file).unwrap();
    copy.strands[1].as_mut().unwrap().palette[0] = 9;
    let a = strand_aims(&braid_fixed_point(&copy));
    assert!(a[1].0 != a[0].0 && a[1].0 != 0, "{a:x?}");
    assert_eq!(a[1].1, a[0].1, "an unedited copy stays the alias");

    let mut owner: Braid = open(&file).unwrap();
    owner.strands[0].as_mut().unwrap().knots.push(Knot {
        tag: 2,
        weight: 7.0,
        ..Default::default()
    });
    let a = strand_aims(&braid_fixed_point(&owner));
    assert_eq!(a[1].0, a[0].0, "{a:x?}");
    assert!(a[1].1 != a[0].1 && a[1].1 != 0, "{a:x?}");
    assert_eq!((a[0].2, a[1].2), (3, 2), "each states its own count");
}

#[test]
fn a_run_alias_no_pointer_owns_is_refused_at_open() {
    let mut t = braid_tree();
    let strands = elements(
        kids(&mut t)
            .child_at_mut(std::mem::offset_of!(Raw<Braid>, strands))
            .unwrap(),
    );
    *kids(&mut strands[1])
        .child_at_mut(std::mem::offset_of!(Raw<Strand>, palette))
        .unwrap() = Value::Alias(0x5000_9000);
    let Err(why) = Braid::from_element(&t) else {
        panic!("an alias of nothing is refused")
    };
    assert!(why.to_string().contains("no pointer"), "{why}");
}
