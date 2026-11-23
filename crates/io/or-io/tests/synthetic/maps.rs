//! Chained maps generic over what their entries own, and a fixed array of
//! pointers, on invented owned records: a map holds its element only through
//! its entry, as an animation map does; two maps reuse a key, which only a
//! reference by that key refuses; and a quadtree node holds its children in
//! slots, each null or owning its target. Each reads, saves, reads back equal
//! and reaches a fixed point.

use std::sync::Arc;

use super::*;
use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Hash32, Keyed, Owned, Raw, Record, View};
use or_io::desc::{self, Pack};

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[decl::root::<Raw<Quad>>, decl::root::<Raw<Deck>>];
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

fn save<T: Owned>(value: &T) -> Result<Saved, String> {
    let pack = pack();
    let tree = value
        .to_element(&Cx::new(pack.plans().clone()))
        .map_err(|e| e.to_string())?;
    let built = or_io::write::build(&pack, &tree).map_err(|e| e.to_string())?;
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    Ok(Saved {
        bytes: Aligned::from_bytes(&bytes),
        virtual_size,
        root: (built.root - FIXED_VIRTUAL_BASE) as usize,
    })
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

/// A node of a tree split in four: a slot per child, null where there is none.
#[record(size = 0x38)]
pub struct Quad {
    pub children: [Option<Box<Quad>>; 4],
    pub marks: [Option<Box<Mark>>; 2],
    pub depth: u32,
    pub weight: u32,
}

fn quad(depth: u32, children: [Option<Box<Quad>>; 4]) -> Option<Box<Quad>> {
    Some(Box::new(Quad {
        children,
        depth,
        weight: depth * 10,
        ..Default::default()
    }))
}

fn tree() -> Quad {
    let leaf = |d| quad(d, Default::default());
    let mut root = *quad(
        0,
        [leaf(1), None, quad(1, [None, None, None, leaf(2)]), None],
    )
    .unwrap();
    root.marks[1] = Some(Box::new(Mark {
        x: 1.5,
        weight: 7,
        ..Default::default()
    }));
    root
}

#[test]
fn a_fixed_array_of_pointers_reads_saves_and_reaches_a_fixed_point() {
    let saved = round_trip(&tree());
    let q = View::<Raw<Quad>>::at_offset(saved.image(), saved.root).unwrap();
    let slots: Vec<u64> = { q.children }.iter().map(|p| p.raw()).collect();
    assert!(
        slots[0] != 0 && slots[1] == 0 && slots[2] != 0 && slots[3] == 0,
        "{slots:x?}"
    );
    let marks: Vec<u64> = { q.marks }.iter().map(|p| p.raw()).collect();
    assert!(marks[0] == 0 && marks[1] != 0, "{marks:x?}");

    let mut edited: Quad = open(&saved).unwrap();
    edited.children[1] = quad(1, Default::default());
    edited.children[2] = None;
    let reopened: Quad = open(&round_trip(&edited)).unwrap();
    assert_eq!(reopened.children[1].as_ref().unwrap().depth, 1);
    assert!(reopened.children[2].is_none());
}

/// A chained map whose element sits only inside its entry.
#[record(size = 0x10)]
pub struct Table<T> {
    #[len(field = "slots")]
    pub buckets: Vec<Option<Box<TableEntry<T>>>>,
    #[pad_after(6)]
    slots: u16,
}

/// One entry of a [`Table`], keying what it owns, and the next in its bucket.
#[record(size = 0x18)]
pub struct TableEntry<T> {
    #[pad_after(4)]
    pub key: u32,
    #[keyed_by(key)]
    pub data: Option<Box<T>>,
    pub next: Option<Box<TableEntry<T>>>,
}

/// Plays naming takes by key on either side of two tables of takes, which reuse a key.
#[record(size = 0x38)]
pub struct Deck {
    pub plays: Vec<Play>,
    pub takes: Table<Take>,
    pub spares: Option<Box<Table<Take>>>,
    pub later: Vec<Play>,
}

fn take_table(names: &[&str]) -> Table<Take> {
    let mut chain = None;
    for (i, name) in names.iter().enumerate().rev() {
        chain = Some(Box::new(TableEntry {
            key: Hash32::of(name).get(),
            data: Some(Box::new(Take {
                length: i as f32,
                frames: 10 * i as u32,
                ..Default::default()
            })),
            next: chain,
            ..Default::default()
        }));
    }
    Table {
        buckets: vec![chain, None],
        ..Default::default()
    }
}

fn play(name: &str) -> Play {
    Play {
        take: Keyed::to(Hash32::of(name).get()),
        ..Default::default()
    }
}

fn deck() -> Deck {
    Deck {
        plays: vec![play("a"), play("c")],
        takes: take_table(&["a", "b", "c"]),
        spares: Some(Box::new(take_table(&["b", "d"]))),
        later: vec![play("d"), play("a")],
        ..Default::default()
    }
}

/// Each take a saved deck's tables hold: its key and the reference count it holds.
fn counts(saved: &Saved) -> Vec<(u32, u32)> {
    fn walk(table: &View<'_, Raw<Table<Take>>>, out: &mut Vec<(u32, u32)>) {
        let buckets = table.buckets().unwrap();
        for b in 0..buckets.len() {
            let mut next = buckets.follow(b).unwrap();
            while let Some(entry) = next {
                out.push((entry.key, entry.data().unwrap().unwrap().ref_count));
                next = entry.next().unwrap();
            }
        }
    }
    let deck = View::<Raw<Deck>>::at_offset(saved.image(), saved.root).unwrap();
    let mut out = Vec::new();
    walk(&deck.takes(), &mut out);
    walk(&deck.spares().unwrap().unwrap(), &mut out);
    out
}

#[test]
fn two_maps_reusing_a_key_save_and_a_reference_counts_its_target_in_either() {
    let saved = round_trip(&deck());
    let k = |name: &str| Hash32::of(name).get();
    assert_eq!(
        counts(&saved),
        [
            (k("a"), 3),
            (k("b"), 1),
            (k("c"), 2),
            (k("b"), 1),
            (k("d"), 2)
        ],
        "each take counts the plays naming it and the entry that owns it"
    );
}

#[test]
fn a_reference_by_a_key_two_maps_hold_is_refused_before_or_after_them() {
    for early in [true, false] {
        let mut ambiguous = deck();
        let plays = if early {
            &mut ambiguous.plays
        } else {
            &mut ambiguous.later
        };
        plays.push(play("b"));
        let Err(why) = save(&ambiguous) else {
            panic!("a key two entries hold names neither")
        };
        assert!(why.contains("two of them hold the key"), "{why}");
    }
}
