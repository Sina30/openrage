//! An inline union on invented owned records: eight bytes whose arm a bit
//! range or a whole sibling field selects, read as the arm selected and saved
//! with the selecting bits the arm implies.

use std::sync::Arc;

use super::*;
use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Owned, Raw, Record, View};
use or_io::desc::{self, FieldRepr, Pack};

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[decl::root::<Raw<Shelf>>];
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

fn save(shelf: &Shelf) -> Result<Saved, String> {
    let pack = pack();
    let tree = shelf
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

/// A word with `name` and `place` set and the sealed bit as given.
fn word(name: u32, place: u32, sealed: bool) -> u32 {
    name | (place << 10) | (u32::from(sealed) << 31)
}

fn shelf() -> Shelf {
    Shelf {
        slots: vec![
            Slot {
                word: word(3, 0, false),
                body: Body::Folder(Folder {
                    first: 1,
                    count: 4,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Slot {
                word: word(9, 0x40, false),
                body: Body::Packed(Packed {
                    virtual_flags: 0xAB,
                    physical_flags: 0xCD,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Slot {
                word: word(12, 0x41, false),
                body: Body::Stored(Stored {
                    length: 700,
                    scheme: 2,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        notes: vec![
            Note {
                body: Body::Packed(Packed {
                    virtual_flags: 5,
                    physical_flags: 6,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Note {
                kind: 1,
                body: Body::Folder(Folder {
                    first: 7,
                    count: 8,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

#[test]
fn a_union_is_described_as_its_bytes_beside_the_bits_that_select_it() {
    let pack = pack();
    let slot = pack.type_by_name("Slot").expect("Slot is described");
    let body = slot.field_by_name("body").expect("body");
    assert_eq!(body.repr, FieldRepr::Opaque { size: 8 });
    let word = slot.field_by_name("word").expect("word");
    let ranges: Vec<(&str, u32, u32)> = word
        .bits
        .iter()
        .map(|b| (b.name.as_str(), b.low, b.width))
        .collect();
    assert_eq!(
        ranges,
        [("name", 0, 10), ("place", 10, 21), ("sealed", 31, 1)]
    );
}

#[test]
fn a_union_selects_its_arm_on_read_and_writes_it_on_save() {
    let value = shelf();
    let saved = save(&value).unwrap();

    let s = View::<Raw<Shelf>>::at_offset(saved.image(), saved.root).unwrap();
    let slots = s.slots().unwrap();
    let words: Vec<u32> = slots.as_slice().iter().map(|slot| slot.word).collect();
    assert_eq!(
        words,
        [
            word(3, 0x1F_FFFF, false),
            word(9, 0x40, true),
            word(12, 0x41, false)
        ],
        "a folder's place and a packed arm's sealed bit are what their arms imply"
    );
    let kinds: Vec<u32> = s
        .notes()
        .unwrap()
        .as_slice()
        .iter()
        .map(|n| n.kind)
        .collect();
    assert_eq!(kinds, [2, 1], "a whole field is set to its arm's value");

    let again: Shelf = open(&saved).unwrap();
    let mut expected = value;
    expected.slots[0].word = word(3, 0x1F_FFFF, false);
    expected.slots[1].word = word(9, 0x40, true);
    expected.notes[0].kind = 2;
    assert_eq!(again, expected);
    assert_eq!(
        save(&again).unwrap().bytes.bytes(),
        saved.bytes.bytes(),
        "a fixed point"
    );
}

#[test]
fn an_arm_its_selecting_bits_would_read_as_another_is_refused_on_save() {
    let mut value = shelf();
    value.slots[2].word = word(12, 0x41, true);
    let why = save(&value).map(|_| ()).unwrap_err();
    assert!(
        why.starts_with("Slot.body") && why.contains("Packed"),
        "{why}"
    );
}

#[test]
fn a_selector_that_names_no_arm_is_refused_on_open() {
    let saved = save(&shelf()).unwrap();
    let s = View::<Raw<Shelf>>::at_offset(saved.image(), saved.root).unwrap();
    let notes = ({ s.notes }.raw() - FIXED_VIRTUAL_BASE) as usize;
    let mut bytes = saved.bytes.bytes().to_vec();
    bytes[notes..notes + 4].copy_from_slice(&3u32.to_le_bytes());
    let broken = Saved {
        bytes: Aligned::from_bytes(&bytes),
        ..saved
    };
    let why = open::<Shelf>(&broken).unwrap_err();
    assert!(why.starts_with("Note.body"), "{why}");
}
