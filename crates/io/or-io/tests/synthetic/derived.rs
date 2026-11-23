//! A private field a save derives from the value, as a vertex buffer's
//! stride is from its layout, written before the count it divides.

use std::mem::offset_of;
use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Owned, Raw, View};
use or_io::desc::{self, Pack, Value};

/// How many bytes each row takes.
#[decl::record(size = 0x08)]
pub struct Width {
    #[pad_after(6)]
    pub bytes: u16,
}

/// Rows of `stride` bytes, the stride derived from `width`.
#[decl::record(size = 0x18)]
pub struct Strip {
    #[len(product = "row_count * stride")]
    pub rows: Vec<u8>,
    row_count: u32,
    #[derived(Strip::width_of)]
    #[pad_after(2)]
    stride: u16,
    pub width: Option<Box<Width>>,
}

impl Strip {
    fn width_of(&self) -> Option<u16> {
        self.width.as_deref().map(|w| w.bytes)
    }
}

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[decl::root::<Raw<Strip>>];
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

    fn stated(&self) -> (u32, u16) {
        let s = View::<Raw<Strip>>::at_offset(self.image(), self.root).unwrap();
        (s.row_count, s.stride)
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

fn tree(strip: &Strip) -> Result<Value, String> {
    strip
        .to_element(&Cx::new(pack().plans().clone()))
        .map_err(|e| e.to_string())
}

fn open(saved: &Saved) -> Strip {
    let doc = Doc::<Raw<Strip>>::read(pack(), saved.image(), saved.root).unwrap();
    Strip::from_element(doc.value()).unwrap()
}

fn strip(width: u16) -> Strip {
    Strip {
        rows: vec![0; 12],
        width: Some(Box::new(Width {
            bytes: width,
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// Twelve bytes of rows four wide, the file stating a stride of three.
fn stating_three() -> Saved {
    let mut t = tree(&strip(4)).unwrap();
    let Value::Struct(record) = &mut t else {
        panic!("a struct");
    };
    let bytes = record.bytes_mut();
    let count = offset_of!(Raw<Strip>, row_count);
    bytes[count..count + 4].copy_from_slice(&4u32.to_le_bytes());
    let stride = offset_of!(Raw<Strip>, stride);
    bytes[stride..stride + 2].copy_from_slice(&3u16.to_le_bytes());
    build(&t)
}

#[test]
fn a_value_built_from_scratch_saves_the_stride_it_derives() {
    let saved = build(&tree(&strip(4)).expect("a derived stride saves"));
    assert_eq!(saved.stated(), (3, 4));
}

#[test]
fn an_opened_value_whose_derivation_moves_is_refused() {
    let mut opened = open(&build(&tree(&strip(4)).unwrap()));
    opened.width.as_deref_mut().unwrap().bytes = 6;
    let why = tree(&opened).expect_err("the file stated 4");
    assert!(
        why.contains("Strip.stride") && why.contains("states 4"),
        "{why}"
    );
}

#[test]
fn a_stated_value_that_disagrees_saves_as_the_file_stated_it_until_the_derivation_moves() {
    let file = stating_three();
    let opened = open(&file);
    let saved = build(&tree(&opened).expect("the file's own statement saves"));
    assert_eq!(saved.stated(), (4, 3));
    assert_eq!(saved.bytes.bytes(), file.bytes.bytes());

    let mut moved = opened;
    moved.width.as_deref_mut().unwrap().bytes = 6;
    let why = tree(&moved).expect_err("the derivation moved from what the file stated");
    assert!(
        why.contains("states 3") && why.contains("derives 6"),
        "{why}"
    );
}

#[test]
fn nothing_to_derive_from_keeps_what_the_file_stated() {
    let mut opened = open(&build(&tree(&strip(4)).unwrap()));
    opened.width = None;
    let saved = build(&tree(&opened).expect("the stated stride saves"));
    assert_eq!(saved.stated(), (3, 4));
}
