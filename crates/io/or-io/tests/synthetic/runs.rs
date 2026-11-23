//! Counted runs on invented owned records: a scalar run whose pointer a file
//! leaves live at an allocation it sized, and a run of pointers at members of
//! the record holding it.

use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Member, Owned, Raw, Record, View};
use or_io::desc::{self, Pack, Value};

use super::Mark;

/// A scalar run, as a cloth's `collision_inst` is.
#[decl::record(size = 0x10)]
pub struct Levels {
    pub levels: Vec<u32>,
}

/// Pointers at the record's own marks, as an effect's keyframe properties are.
#[decl::record(size = 0x20)]
pub struct Picks {
    pub first: Mark,
    pub picked: Vec<Member<Mark>>,
}

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[decl::root::<Raw<Levels>>, decl::root::<Raw<Picks>>];
    Arc::new(decl::describe("synthetic", "gen8", roots).expect("describes"))
}

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

fn open<T: Owned>(saved: &Saved) -> Result<T, String>
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).map_err(|e| e.to_string())?;
    T::from_element(doc.value()).map_err(|e| e.to_string())
}

fn tree_save<T: Owned>(saved: &Saved) -> Saved
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).unwrap();
    build(doc.value())
}

fn record(tree: &mut Value) -> &mut desc::Record {
    match tree {
        Value::Struct(record) => record,
        other => panic!("a struct, not {other:?}"),
    }
}

fn levels_header(saved: &Saved) -> (u64, usize, usize) {
    let l = View::<Raw<Levels>>::at_offset(saved.image(), saved.root).unwrap();
    let run = { l.levels };
    (run.raw(), run.count(), run.capacity())
}

#[test]
fn an_empty_scalar_run_keeps_the_allocation_the_file_sized() {
    let mut t = tree(&Levels::default());
    *record(&mut t)
        .child_at_mut(std::mem::offset_of!(Raw<Levels>, levels))
        .unwrap() = Value::ptr_to(Value::Array {
        elements: Vec::new(),
        capacity: Some(16),
    });
    let file = build(&t);
    let (pointer, count, capacity) = levels_header(&file);
    assert!(
        pointer != 0 && (count, capacity) == (0, 16),
        "the file sized it"
    );

    let levels: Levels = open(&file).unwrap();
    assert!(levels.levels.is_empty());
    let saved = build(&tree(&levels));
    let (pointer, count, capacity) = levels_header(&saved);
    assert!(pointer != 0, "the pointer stays live");
    assert_eq!((count, capacity), (0, 16), "the capacity is carried");
    assert_eq!(
        saved.bytes.bytes(),
        tree_save::<Levels>(&file).bytes.bytes(),
        "the owned save is the tree's"
    );
}

#[test]
fn a_run_of_member_pointers_opens_owned_and_saves_as_its_tree_saves() {
    let picks = Picks {
        first: Mark {
            weight: 7,
            ..Default::default()
        },
        picked: vec![Member::to("first"), Member::to("first")],
        ..Default::default()
    };
    let file = build(&tree(&picks));
    let opened: Picks = open(&file).unwrap();
    assert_eq!(opened, picks);
    let saved = build(&tree(&opened));
    assert_eq!(
        saved.bytes.bytes(),
        tree_save::<Picks>(&file).bytes.bytes(),
        "the owned save is the tree's"
    );
    let p = View::<Raw<Picks>>::at_offset(saved.image(), saved.root).unwrap();
    let first = FIXED_VIRTUAL_BASE + saved.root as u64;
    let table = saved.image().resolve({ p.picked }.raw()).unwrap();
    for slot in saved.bytes.bytes()[table..table + 16]
        .as_chunks::<8>()
        .0
        .iter()
    {
        assert_eq!(u64::from_le_bytes(*slot), first);
    }
}
