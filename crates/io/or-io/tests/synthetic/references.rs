//! A pointer at what another pointer owns, on invented owned records: derived
//! on save when it points back at its root, held by key when its target is
//! keyed, held once when two pointers share an allocation, and a table of
//! string pointers; held by the key of the map entry that owns its target
//! where the target has none, that target counting the pointers at it. Each
//! reads, saves, reads back equal and reaches a fixed point, and a value built
//! from scratch writes every pointer it implies.

use std::sync::Arc;

use super::*;
use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Hash32, Keyed, Owned, Raw, Record, Shared, Vec4, View};
use or_io::desc::{self, Pack, Value};

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[
        decl::root::<Raw<Frames>>,
        decl::root::<Raw<Library>>,
        decl::root::<Raw<Catalog>>,
        decl::root::<Raw<Names>>,
        decl::root::<Raw<Reel>>,
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

fn v(i: u32) -> Vec4 {
    Vec4 {
        x: i as f32,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    }
}

fn mark(i: u32) -> Mark {
    Mark {
        x: i as f32,
        weight: i,
        id: Hash32::of(&format!("mark{i}")),
        ..Default::default()
    }
}

fn frames() -> Frames {
    Frames {
        current: vec![v(1), v(2)],
        last: Shared::Same,
        primary: Some(Box::new(mark(1))),
        spare: Shared::Same,
        ..Default::default()
    }
}

/// A file pointing both runs at one allocation, as every composite bound does.
#[test]
fn a_file_whose_two_runs_share_an_allocation_opens() {
    let pack = pack();
    let mut frames = Frames {
        current: vec![v(1), v(2)],
        last: Shared::Own(vec![v(1), v(2)]),
        ..Default::default()
    };
    let mut tree = frames.to_element(&Cx::new(pack.plans().clone())).unwrap();
    let Value::Struct(record) = &mut tree else {
        panic!("a struct")
    };
    let current = std::mem::offset_of!(Raw<Frames>, current);
    let last = std::mem::offset_of!(Raw<Frames>, last);
    match record.child_at_mut(current) {
        Some(Value::Ptr { source, .. }) => *source = 0x5000_1000,
        other => panic!("{other:?}"),
    }
    *record.child_at_mut(last).unwrap() = Value::Alias(0x5000_1000);
    let again: Frames = open(&build(&pack, &tree).unwrap()).unwrap();
    frames.last = Shared::Same;
    assert_eq!(again, frames);
}

#[test]
fn two_pointers_at_one_allocation_are_held_once() {
    let saved = round_trip(&frames());
    let f = View::<Raw<Frames>>::at_offset(saved.image(), saved.root).unwrap();
    let (current, last) = ({ f.current }.raw(), { f.last }.raw());
    assert!(current != 0 && current == last, "{current:#x} {last:#x}");
    let (primary, spare) = ({ f.primary }.raw(), { f.spare }.raw());
    assert!(primary != 0 && primary == spare, "{primary:#x} {spare:#x}");
    assert_eq!(
        f.last().unwrap().len(),
        2,
        "the count states the shared run"
    );

    let mut apart = frames();
    apart.last = Shared::Own(vec![v(1), v(2)]);
    let saved = round_trip(&apart);
    let f = View::<Raw<Frames>>::at_offset(saved.image(), saved.root).unwrap();
    assert_ne!(
        { f.current }.raw(),
        { f.last }.raw(),
        "an owned run is its own"
    );
}

#[test]
fn a_root_back_pointer_is_derived_on_save() {
    let library = Library {
        books: vec![
            Some(Box::new(Book {
                id: 1,
                pages: 300,
                ..Default::default()
            })),
            None,
            Some(Box::new(Book {
                id: 2,
                pages: 12,
                ..Default::default()
            })),
        ],
        ..Default::default()
    };
    let saved = round_trip(&library);
    let lib = View::<Raw<Library>>::at_offset(saved.image(), saved.root).unwrap();
    let books = lib.books().unwrap();
    for i in [0, 2] {
        let book = books.follow(i).unwrap().unwrap();
        assert_eq!(
            { book.library }.raw(),
            FIXED_VIRTUAL_BASE + saved.root as u64,
            "book {i} points at its library"
        );
    }
}

#[test]
fn a_keyed_reference_is_held_by_key_and_found_wherever_its_target_lands() {
    let item = |name: &str, weight| {
        Some(Box::new(Item {
            hash: Hash32::of(name),
            weight,
            ..Default::default()
        }))
    };
    let pick = |name: &str| Pick {
        item: Keyed::to(Hash32::of(name).get()),
        ..Default::default()
    };
    let catalog = Catalog {
        picks: vec![pick("b"), pick("a"), pick("b")],
        items: vec![item("a", 1), item("b", 2)],
        ..Default::default()
    };
    let saved = round_trip(&catalog);
    let cat = View::<Raw<Catalog>>::at_offset(saved.image(), saved.root).unwrap();
    let items = cat.items().unwrap();
    let at = |i: usize| View::offset(&items.follow(i).unwrap().unwrap());
    let picks = cat.picks().unwrap();
    let points = |p: usize| ({ picks[p].item }.raw() - FIXED_VIRTUAL_BASE) as usize;
    assert_eq!(
        (points(0), points(1), points(2)),
        (at(1), at(0), at(1)),
        "each pick points at the item its key names"
    );

    let mut dangling = catalog;
    dangling.picks.push(pick("c"));
    let Err(why) = save(&dangling) else {
        panic!("a key nothing holds is refused")
    };
    assert!(why.contains("nothing in this tree wrote"), "{why}");
}

#[test]
fn a_table_of_string_pointers_keeps_every_string() {
    let names = Names {
        names: vec!["hull".into(), String::new(), "glass".into()],
        ..Default::default()
    };
    let saved = round_trip(&names);
    let n = View::<Raw<Names>>::at_offset(saved.image(), saved.root).unwrap();
    let slots: Vec<u64> = n.names().unwrap().iter().map(|s| s.raw()).collect();
    assert_eq!(slots.len(), 3);
    assert!(
        slots[0] != 0 && slots[1] == 0 && slots[2] != 0,
        "{slots:x?}"
    );
}

fn reel() -> Reel {
    let take = |length: f32, frames| {
        Some(Box::new(Take {
            length,
            frames,
            ..Default::default()
        }))
    };
    let entry = |name: &str, data, next| {
        Some(Box::new(Entry {
            key: Hash32::of(name),
            data,
            next,
            ..Default::default()
        }))
    };
    let play = |name: &str| Play {
        take: Keyed::to(Hash32::of(name).get()),
        ..Default::default()
    };
    Reel {
        plays: ["b", "a", "b", "c", "b"].into_iter().map(play).collect(),
        takes: vec![
            entry("a", take(1.5, 10), entry("b", take(2.5, 20), None)),
            None,
            entry("c", take(3.5, 30), None),
        ],
        ..Default::default()
    }
}

/// Each entry of a saved reel's map: its key, where its take lies and the reference count the take holds.
fn targets(saved: &Saved) -> Vec<(u32, usize, u32)> {
    let reel = View::<Raw<Reel>>::at_offset(saved.image(), saved.root).unwrap();
    let buckets = reel.takes().unwrap();
    let mut out = Vec::new();
    for b in 0..buckets.len() {
        let mut next = buckets.follow(b).unwrap();
        while let Some(entry) = next {
            let take = entry.data().unwrap().expect("an entry owns its take");
            out.push((entry.key.get(), View::offset(&take), take.ref_count));
            next = entry.next().unwrap();
        }
    }
    out
}

#[test]
fn a_reference_keyed_by_its_owning_entry_reads_saves_and_counts_its_target() {
    let reel = reel();
    let saved = round_trip(&reel);
    let r = View::<Raw<Reel>>::at_offset(saved.image(), saved.root).unwrap();
    let plays = r.plays().unwrap();
    let points: Vec<usize> = (0..plays.len())
        .map(|p| {
            let at = { plays[p].take }.raw();
            assert_ne!(at, 0, "play {p} saves non-null");
            (at - FIXED_VIRTUAL_BASE) as usize
        })
        .collect();
    let found = targets(&saved);
    let at = |name: &str| {
        found
            .iter()
            .find(|(key, ..)| *key == Hash32::of(name).get())
            .map(|(_, at, _)| *at)
            .unwrap()
    };
    assert_eq!(
        points,
        [at("b"), at("a"), at("b"), at("c"), at("b")],
        "each play points at the take its entry's key names"
    );
    let counts: Vec<(u32, u32)> = found.iter().map(|(k, _, n)| (*k, *n)).collect();
    assert_eq!(
        counts,
        [
            (Hash32::of("a").get(), 2),
            (Hash32::of("b").get(), 4),
            (Hash32::of("c").get(), 2)
        ],
        "each take counts its plays and the entry that owns it"
    );

    let mut opened: Reel = open(&saved).unwrap();
    opened.plays.truncate(2);
    let resaved = round_trip(&opened);
    let counts: Vec<u32> = targets(&resaved).iter().map(|(.., n)| *n).collect();
    assert_eq!(counts, [2, 2, 1], "a save recomputes what the file held");

    let mut dangling = reel;
    dangling.plays.push(Play {
        take: Keyed::to(Hash32::of("d").get()),
        ..Default::default()
    });
    let Err(why) = save(&dangling) else {
        panic!("a key no entry holds is refused")
    };
    assert!(why.contains("nothing in this tree wrote"), "{why}");
}
