//! Invented declarations covering every feature `#[record]` has, and a
//! resource built by hand to their declared offsets. Nothing here is a game's.

#![allow(dead_code)]

use or_io::decl::*;

mod aliases;
mod by_value;
mod codecs;
mod derived;
mod elsewhere;
mod empties;
mod generics;
mod heap;
mod maps;
mod references;
mod runs;
mod tagged_generics;
mod unions;

#[record(enum, width = u8)]
pub enum Shape {
    /// Round
    Round = 0,
    /// Square
    Square = 1,
    /// Long
    Long = 4,
    ///
    /// Flat
    ///
    /// Lies level, which the note does not say.
    Flat = 6,
}

#[record(size = 0x10)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub weight: u32,
}

#[record(size = 0x20)]
pub struct Header {
    pub magic: u32,
    pub version: u16,
    pub shape: Open<Shape>,
    pub visible: Bool8,
    pub id: Hash32,
    #[note("a word nothing names, carried")]
    reserved: u32,
    pub centre: Vec4,
}

/// A layout generic over what it holds; the parameter sits only behind pointers.
#[record(size = 0x10)]
pub struct Holder<V> {
    #[len(field = "count")]
    pub items: Vec<Option<Box<V>>>,
    count: u16,
    _pad_0x0a: [u8; 6],
}

#[record(size = 0x38)]
pub struct Channel {
    pub first: Point,
    pub second: Point,
    pub selected: Member<Point>,
    pub order: Vec<Member<Point>>,
}

pub mod gen8 {
    use or_io::decl::*;

    #[record(size = 0x10, target = "gen8")]
    pub struct Swatch {
        pub width: u16,
        pub height: u16,
        depth: u32,
        #[phys]
        #[len(bytes = "depth")]
        pub pixels: Vec<u8>,
    }
}

pub mod gen9 {
    use or_io::decl::*;

    #[record(size = 0x18, target = "gen9")]
    pub struct Swatch {
        depth: u32,
        pub width: u16,
        pub height: u16,
        pub layers: u32,
        _pad_0x0c: u32,
        #[phys]
        #[len(bytes = "depth")]
        pub pixels: Vec<u8>,
    }
}

/// What differs between the two targets; the spine points at it.
pub trait Side: TargetMarker {
    type Swatch: Owned<Raw: Record> + Default + std::fmt::Debug + Clone + PartialEq;
}
impl Side for Gen8 {
    type Swatch = gen8::Swatch;
}
impl Side for Gen9 {
    type Swatch = gen9::Swatch;
}

#[record(size = 0x10)]
pub struct Frame<T: Side> {
    pub swatch: Option<Box<T::Swatch>>,
    pub index: u32,
    _pad_0x0c: u32,
}

/// Its pointer sits at four modulo eight, so it is declared packed.
#[record(size = 0x0c, packed(4))]
pub struct Loose {
    pub tag: u32,
    pub link: Option<Box<Point>>,
}

#[record(size = 0x88)]
pub struct Catalogue {
    pub header: Header,
    pub name: String,
    #[len(field = "point_count")]
    pub points: Vec<Point>,
    point_count: u16,
    pub flags: u16,
    #[note("a word nothing names, carried")]
    reserved: u32,
    pub primary: Option<Box<Point>>,
    pub spare: Option<Box<Point>>,
    pub tags: Vec<Hash32>,
    pub held: Option<Box<Holder<Point>>>,
    pub held_channels: Option<Box<Holder<Channel>>>,
    pub channel: Option<Box<Channel>>,
    pub frame: Option<Box<Frame<Gen8>>>,
    tail_count: u32,
    _pad_0x7c: u32,
    #[note("a checksum nothing reads")]
    pub stamp: u64,
    #[trailing]
    #[len(field = "tail_count")]
    pub tail: Vec<u32>,
}

pub const VIRTUAL_BASE: u64 = 0x5000_0000;
pub const PHYSICAL_BASE: u64 = 0x6000_0000;

/// Bytes laid out by hand at the declared offsets, the way a file holds them.
pub struct Image {
    pub bytes: Vec<u8>,
    pub virtual_size: usize,
}

pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn put(&mut self, at: usize, data: &[u8]) -> &mut Self {
        if self.0.len() < at + data.len() {
            self.0.resize(at + data.len(), 0);
        }
        self.0[at..at + data.len()].copy_from_slice(data);
        self
    }
    pub fn u16(&mut self, at: usize, v: u16) -> &mut Self {
        self.put(at, &v.to_le_bytes())
    }
    pub fn u32(&mut self, at: usize, v: u32) -> &mut Self {
        self.put(at, &v.to_le_bytes())
    }
    pub fn u64(&mut self, at: usize, v: u64) -> &mut Self {
        self.put(at, &v.to_le_bytes())
    }
    pub fn f32(&mut self, at: usize, v: f32) -> &mut Self {
        self.put(at, &v.to_le_bytes())
    }
    pub fn ptr(&mut self, at: usize, target: usize) -> &mut Self {
        self.u64(at, VIRTUAL_BASE + target as u64)
    }
    pub fn point(&mut self, at: usize, p: [f32; 3], weight: u32) -> &mut Self {
        self.f32(at, p[0])
            .f32(at + 4, p[1])
            .f32(at + 8, p[2])
            .u32(at + 12, weight)
    }
}

/// The catalogue every suite reads: every field set, one pointer null, one generic twice.
pub fn catalogue() -> Image {
    let mut w = Writer(vec![0; 0x400]);
    // header, embedded at 0x00
    w.u32(0x00, 0x4341_5431).u16(0x04, 3);
    w.put(0x06, &[4]).put(0x07, &[1]);
    w.u32(0x08, Hash32::of("catalogue").get())
        .u32(0x0c, 0xDEAD_BEEF);
    w.f32(0x10, 1.0)
        .f32(0x14, 2.0)
        .f32(0x18, 3.0)
        .f32(0x1c, f32::from_bits(0x7FC0_0001));
    // name, points
    w.ptr(0x20, 0x100)
        .ptr(0x28, 0x110)
        .u16(0x30, 3)
        .u16(0x32, 0x8001)
        .u32(0x34, 0x1234_5678);
    // primary points at a point of its own; spare is null
    w.ptr(0x38, 0x200).u64(0x40, 0);
    w.point(0x200, [3.25, 4.25, 5.25], 21);
    // tags: an Array of three hashes
    w.ptr(0x48, 0x140).u16(0x50, 3).u16(0x52, 4);
    w.ptr(0x58, 0x160)
        .ptr(0x60, 0x180)
        .ptr(0x68, 0x1a0)
        .ptr(0x70, 0x220);
    w.u32(0x78, 2)
        .u32(0x7c, 0xFFFF_FFFF)
        .u64(0x80, 0x0102_0304_0506_0708);
    // trailing run of two u32 right after the root
    w.u32(0x88, 7).u32(0x8c, 9);

    w.put(0x100, b"stock\0");
    w.point(0x110, [0.5, 1.5, 2.5], 10);
    w.point(0x120, [3.5, 4.5, 5.5], 20);
    w.point(0x130, [6.5, 7.5, 8.5], 30);
    for (i, name) in ["alpha", "beta", "gamma"].iter().enumerate() {
        w.u32(0x140 + 4 * i, Hash32::of(name).get());
    }
    // Holder<Point>: two slots, the second null; padding deliberately dirty
    w.ptr(0x160, 0x170).u16(0x168, 2).put(0x16a, &[0xAA; 6]);
    w.ptr(0x170, 0x1f0).u64(0x178, 0);
    // Holder<Channel>: one slot
    w.ptr(0x180, 0x190).u16(0x188, 1);
    w.ptr(0x190, 0x1a0);
    // Channel at 0x1a0: two points, a member pointer at the second, a table of both
    w.point(0x1a0, [1.0, 0.0, 0.0], 1);
    w.point(0x1b0, [0.0, 1.0, 0.0], 2);
    w.ptr(0x1c0, 0x1b0);
    w.ptr(0x1c8, 0x1e0).u16(0x1d0, 2).u16(0x1d2, 2);
    w.ptr(0x1e0, 0x1b0).ptr(0x1e8, 0x1a0);
    // the point Holder<Point> holds
    w.point(0x1f0, [9.0, 9.0, 9.0], 99);
    // Frame<Gen8> at 0x220, its swatch at 0x230, pixels in the graphics half
    w.ptr(0x220, 0x230).u32(0x228, 5);
    w.u16(0x230, 2)
        .u16(0x232, 2)
        .u32(0x234, 4)
        .u64(0x238, PHYSICAL_BASE);
    let virtual_size = 0x400;
    w.0.resize(virtual_size, 0);
    w.put(virtual_size, &[0x10, 0x20, 0x30, 0x40]);
    w.0.resize(virtual_size + 0x10, 0);
    Image {
        bytes: w.0,
        virtual_size,
    }
}

/// An owned leaf: four words, a name hash among them.
#[record(size = 0x10)]
pub struct Mark {
    pub x: f32,
    pub y: f32,
    pub weight: u32,
    pub id: Hash32,
}

/// One owned run per `#[len]` kind, each counted by fields of its own.
#[record(size = 0x58)]
pub struct Tally {
    #[len(field = "entry_count")]
    pub entries: Vec<u32>,
    #[len(count = "slot_count", capacity = "slot_capacity")]
    pub slots: Vec<u16>,
    /// Masked with `0x00FF_FFF0`, the byte count; the other bits are carried.
    #[len(masked = "packed", mask = 0x00FF_FFF0)]
    pub blob: Vec<u8>,
    #[len(product = "row_count * row_width")]
    pub grid: Vec<u8>,
    #[len(sum = "head_count + tail_count")]
    pub parts: Vec<u32>,
    #[len(fixed = 4)]
    pub corners: Vec<f32>,
    #[phys]
    #[len(bytes = "byte_count")]
    pub payload: Vec<u8>,
    /// One more mark than `box_count` when it is above one: their union leads.
    #[len(prefixed_above_one = "box_count")]
    pub boxes: Vec<Mark>,
    pub entry_count: u16,
    pub slot_count: u16,
    pub slot_capacity: u16,
    pub row_count: u16,
    pub row_width: u16,
    pub head_count: u16,
    pub tail_count: u16,
    pub box_count: u16,
    pub packed: u32,
    pub byte_count: u32,
}

/// Two runs one count states: a box per shelf with their union in front above one, and a weight per shelf.
#[record(size = 0x18)]
pub struct Shelves {
    #[len(prefixed_above_one = "shelf_count")]
    pub boxes: Vec<Mark>,
    #[len(field = "shelf_count")]
    pub weights: Vec<u16>,
    #[pad_after(6)]
    pub shelf_count: u16,
}

/// A tagged base: `kind` says which layout the object behind a pointer has.
#[record(size = 0x10, tag = "kind", variant(1 => Disc))]
pub struct Figure {
    pub vft: u64,
    #[pad_after(4)]
    pub kind: u32,
}

/// The one variant [`Figure`] declares.
#[record(size = 0x18)]
pub struct Disc {
    pub base: Figure,
    pub radius: f32,
    pub rim: u32,
}

/// Every pointer an owned record holds: to a member, never null, maybe null, and to a tagged base.
#[record(size = 0x50)]
pub struct Links {
    pub first: Mark,
    pub second: Mark,
    pub selected: Member<Mark>,
    pub primary: Box<Mark>,
    pub spare: Option<Box<Mark>>,
    pub figure: Option<Box<FigureAny>>,
    pub figures: Vec<Option<Box<FigureAny>>>,
}

/// An inline array longer than the standard library's `Default` reaches.
#[record(size = 0xd0)]
pub struct Palette {
    pub colours: [u32; 52],
}

/// Two runs one count states, which a file may point at one allocation as a
/// composite points its current and last matrices, and two marks the same way.
#[record(size = 0x28)]
pub struct Frames {
    #[len(field = "count")]
    pub current: Vec<Vec4>,
    #[shares(current)]
    #[len(field = "count")]
    pub last: Shared<Vec<Vec4>>,
    pub primary: Option<Box<Mark>>,
    #[shares(primary)]
    pub spare: Shared<Option<Box<Mark>>>,
    #[pad_after(4)]
    count: u32,
}

/// A root whose books each point back at it, as a clip points at its dictionary.
#[record(size = 0x10)]
pub struct Library {
    pub books: Vec<Option<Box<Book>>>,
}

#[record(size = 0x10)]
pub struct Book {
    pub library: Back<Library>,
    pub id: u32,
    pub pages: u32,
}

/// References by key, read before the records they name, as a clip names an animation.
#[record(size = 0x20)]
pub struct Catalog {
    pub picks: Vec<Pick>,
    pub items: Vec<Option<Box<Item>>>,
}

#[record(size = 0x08, key = "hash")]
pub struct Item {
    pub hash: Hash32,
    pub weight: u32,
}

#[record(size = 0x08)]
pub struct Pick {
    pub item: Keyed<Item>,
}

/// A table of string pointers whose length another field states, as a physics level names its groups.
#[record(size = 0x10)]
pub struct Names {
    #[len(field = "count")]
    pub names: Vec<String>,
    #[pad_after(4)]
    count: u32,
}

/// A word of named bit ranges, and eight bytes two of them say the layout of.
#[record(size = 0x0c)]
pub struct Slot {
    #[bits(name, 0, 10)]
    #[bits(place, 10, 21, "in blocks of 256 bytes")]
    #[bits(sealed, 31, 1)]
    pub word: u32,
    #[select(place = 0x1F_FFFF => Folder, sealed = 1 => Packed, _ => Stored)]
    pub body: Body,
}

/// A slot's eight bytes, laid out as whichever arm its word selects.
#[record(union, size = 0x08)]
pub enum Body {
    Folder(Folder),
    Packed(Packed),
    Stored(Stored),
}

#[record(size = 0x08)]
pub struct Folder {
    pub first: u32,
    pub count: u32,
}

#[record(size = 0x08)]
pub struct Packed {
    pub virtual_flags: u32,
    pub physical_flags: u32,
}

#[record(size = 0x08)]
pub struct Stored {
    pub length: u32,
    pub scheme: u32,
}

/// The same union selected by a whole field, with no arm for a kind it does not name.
#[record(size = 0x0c)]
pub struct Note {
    pub kind: u32,
    #[select(kind = 1 => Folder, kind = 2 => Packed)]
    pub body: Body,
}

#[record(size = 0x20)]
pub struct Shelf {
    pub slots: Vec<Slot>,
    pub notes: Vec<Note>,
}

/// A chained map entry whose key keys the value it owns, as a hashed map's entry does.
#[record(size = 0x18)]
pub struct Entry {
    #[pad_after(4)]
    pub key: Hash32,
    #[keyed_by(key)]
    pub data: Option<Box<Take>>,
    pub next: Option<Box<Entry>>,
}

/// What an entry owns: no key of its own, and a count of the pointers at it.
#[record(size = 0x10)]
pub struct Take {
    pub length: f32,
    pub frames: u32,
    #[refs]
    #[pad_after(4)]
    ref_count: u32,
}

/// Plays naming takes by the key of the entry that owns each, read before that map.
#[record(size = 0x20)]
pub struct Reel {
    pub plays: Vec<Play>,
    pub takes: Vec<Option<Box<Entry>>>,
}

#[record(size = 0x08)]
pub struct Play {
    pub take: Keyed<Take>,
}
