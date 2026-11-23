//! What a `#[record]` declaration is written in, and what it generates against:
//! one Rust struct that is read zero-copy, edited as a record and describes
//! itself to the generic tree and the canonical writer.

mod describe;
mod file;
mod owned;
mod prim;
mod record;
pub mod refusals;
mod view;

use std::ffi::CStr;

pub use or_io_macros::record;

pub use crate::desc::Image;
pub use crate::error::OwnedError;
pub use describe::{describe, intern, root, Reach};
pub use file::{kind_name, File, HasOrigin, Origin};
#[cfg(feature = "gta5")]
pub use file::{Peek, Resource};
pub use owned::{
    alias_copy, pointer_in, record_value, records_value, run_from_child, run_to_child,
    run_to_trailing, share, shares, stamp_run, Back, Carry, Codec, Cx, Entered, Heap, KeyWord,
    Keyed, Kids, Member, NullIsEmpty, Owned, OwnedOrKeyed, Raw, Ref, RefPath, Repair, Rx, Shared,
    Site, SiteMut, Stash,
};
pub use prim::{
    Array, Bool8, Count, Hash32, MemberPtr, Open, PhysPtr, Ptr, Str, Trailing, Vec2, Vec3, Vec4,
};
pub use record::{Aligned, Doc, Edit, RawRecord, RecordMut, RunMut, Saved};
pub use view::{Access, RawView, Run, View, ViewError};

use crate::desc::{
    AddressSpace, CountSource, FieldRepr, IntWidth, PackEnum, PackType, Pointee, Scalar,
};

pub type Result<T, E = ViewError> = std::result::Result<T, E>;

/// Every bit pattern of `size_of::<Self>()` bytes is a valid `Self`, and
/// `Self` holds no padding, no reference and nothing with a destructor.
///
/// # Safety
///
/// Implemented by `#[record]` after proving, at compile time, that every
/// field is `Plain` and that the fields cover every byte of the declared
/// size. A hand-written impl must hold the same two facts, because views and
/// records cast bytes to `&Self` on the strength of it.
pub unsafe trait Plain: Copy + Send + Sync + 'static {
    fn zeroed() -> Self {
        // SAFETY: every bit pattern is a valid `Self`, zero included.
        unsafe { std::mem::zeroed() }
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: `Self` has no padding, so all of its bytes are initialised.
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }

    /// `bytes` read as a `Self` wherever they sit; `None` if there are too few.
    fn read_from(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..std::mem::size_of::<Self>())?;
        // SAFETY: the length is checked, and every bit pattern is a valid `Self`.
        Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<Self>()) })
    }
}

macro_rules! plain_scalars {
    ($($t:ty => $s:expr),* $(,)?) => {$(
        // SAFETY: a primitive number is valid for every bit pattern.
        unsafe impl Plain for $t {}
        impl Field for $t {
            fn repr(_: Option<CountSource>) -> FieldRepr {
                FieldRepr::Scalar($s)
            }
        }
        impl Element for $t {
            fn elem() -> Elem {
                Elem::Scalar($s)
            }
        }
        impl Param for $t {
            fn param_name() -> Option<String> {
                Some(stringify!($t).into())
            }
        }
    )*};
}

plain_scalars! {
    u8 => Scalar::Int(IntWidth::U8),
    u16 => Scalar::Int(IntWidth::U16),
    u32 => Scalar::Int(IntWidth::U32),
    u64 => Scalar::Int(IntWidth::U64),
    i8 => Scalar::Int(IntWidth::I8),
    i16 => Scalar::Int(IntWidth::I16),
    i32 => Scalar::Int(IntWidth::I32),
    i64 => Scalar::Int(IntWidth::I64),
    f32 => Scalar::F32,
    f64 => Scalar::F64,
}

// SAFETY: an array of `Plain` has no padding between its elements.
unsafe impl<T: Plain, const N: usize> Plain for [T; N] {}

/// A declared record: plain bytes that describe themselves.
pub trait Record: Plain + Described + Param + Field + Element + PointsTo {
    /// What a view of this record derefs to: its accessors, generated.
    type Access<'a>: Access<'a, Self>;
    /// What a record opened for editing derefs to: its child accessors, generated.
    type Edit<'r>: Edit<'r, Self>;
}

/// What one declared type says about itself to the generic machinery.
pub trait Described: 'static {
    /// The layout proof `#[record]` writes, evaluated wherever the type is used.
    #[doc(hidden)]
    const __LAYOUT: () = ();

    /// The description's name: the struct's, with any parameters spelled out.
    fn name() -> String;
    /// The one target this declaration belongs to, or `None` for every target.
    fn target() -> Option<&'static str>;
    fn describe() -> PackType;
    /// Every type and enum this one's fields name.
    fn reach(reach: &mut Reach);

    /// For the raw half of an owned declaration, each field the owned struct
    /// shows and whether its type holds an address; `None` for a raw declaration.
    #[doc(hidden)]
    const __OWNED_PUBLIC: Option<&'static [(&'static str, bool)]> = None;

    /// The fields declared as a [`Hash32`], which the description reads as plain integers.
    #[doc(hidden)]
    const __HASH_FIELDS: &'static [&'static str] = &[];
}

/// How a type parameter shows in a generic record's name: a record by its
/// name, a target marker not at all, because a description is already per target.
pub trait Param: 'static {
    fn param_name() -> Option<String>;
}

/// A target a per-target declaration can be generic over.
// ⚠ `PartialEq` lets a record generic over its target compare equal to its
// default, which a `NullIsEmpty` slot holding one needs.
pub trait TargetMarker: Param + Send + Sync + PartialEq {
    const ID: &'static str;
}

/// The Legacy target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gen8;
/// The Enhanced target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gen9;

impl Param for Gen8 {
    fn param_name() -> Option<String> {
        None
    }
}
impl Param for Gen9 {
    fn param_name() -> Option<String> {
        None
    }
}
impl TargetMarker for Gen8 {
    const ID: &'static str = "gen8";
}
impl TargetMarker for Gen9 {
    const ID: &'static str = "gen9";
}

/// How one field describes itself.
pub trait Field {
    fn repr(len: Option<CountSource>) -> FieldRepr;
    fn space() -> Option<AddressSpace> {
        None
    }
    fn reach(_: &mut Reach) {}
}

/// What one element of a run or an inline array is.
pub enum Elem {
    Scalar(Scalar),
    Type(String),
    Ptr(String),
    Member(String),
    /// A pointer at a NUL-terminated string.
    Str,
    Other(usize),
}

pub trait Element {
    fn elem() -> Elem;
    fn reach(_: &mut Reach) {}
    /// The heap an element that is a pointer points into.
    fn space() -> Option<AddressSpace> {
        None
    }
}

/// What a pointer lands on.
pub trait PointsTo {
    fn pointee(len: Option<CountSource>) -> Pointee;
    fn reach(_: &mut Reach) {}
}

impl<T: Element + Plain> PointsTo for [T] {
    fn pointee(len: Option<CountSource>) -> Pointee {
        let count = len.unwrap_or(CountSource::Unknown);
        match T::elem() {
            Elem::Scalar(Scalar::Int(IntWidth::U8)) => Pointee::Bytes { length: count },
            Elem::Scalar(s) => Pointee::Many {
                type_name: None,
                count,
                element_size: Some(s.size()),
            },
            Elem::Type(name) => Pointee::Many {
                type_name: Some(name),
                count,
                element_size: None,
            },
            Elem::Ptr(name) => Pointee::ManyPtr {
                type_name: Some(name),
                count,
            },
            Elem::Str => Pointee::Strings { count },
            Elem::Member(name) => Pointee::Members {
                type_name: name,
                count,
            },
            Elem::Other(size) => Pointee::Many {
                type_name: None,
                count,
                element_size: Some(size),
            },
        }
    }
    fn reach(reach: &mut Reach) {
        T::reach(reach)
    }
}

impl PointsTo for CStr {
    fn pointee(_: Option<CountSource>) -> Pointee {
        Pointee::Str
    }
}

impl<T: Element + Plain, const N: usize> Field for [T; N] {
    fn repr(_: Option<CountSource>) -> FieldRepr {
        match T::elem() {
            Elem::Scalar(element) => FieldRepr::ScalarArray { element, count: N },
            Elem::Type(type_name) => FieldRepr::InlineArray {
                type_name,
                count: N,
            },
            Elem::Ptr(type_name) => FieldRepr::PtrSlots {
                pointee: Pointee::One { type_name },
                count: N,
            },
            Elem::Str => FieldRepr::PtrSlots {
                pointee: Pointee::Str,
                count: N,
            },
            Elem::Member(_) | Elem::Other(_) => FieldRepr::Opaque {
                size: std::mem::size_of::<[T; N]>(),
            },
        }
    }
    fn space() -> Option<AddressSpace> {
        T::space()
    }
    fn reach(reach: &mut Reach) {
        T::reach(reach)
    }
}

/// A closed Rust enum over a raw integer, with a variant for every value it does not name.
pub trait OpenEnum: Copy + Send + Sync + 'static {
    type Raw: Plain + Copy + Eq + std::fmt::Debug;
    const WIDTH: IntWidth;
    fn from_raw(raw: Self::Raw) -> Self;
    fn to_raw(self) -> Self::Raw;
    fn name() -> &'static str;
    fn describe() -> PackEnum;
}

/// A base whose tag field selects which record the bytes it starts are.
pub trait Tagged: Record {
    fn tag(&self) -> i64;
}

/// A record the tag values of `B` in `TAGS` select; it embeds `B` at offset zero.
pub trait VariantOf<B: Tagged>: Record {
    const TAGS: &'static [i64];
}

/// A tag field's value, however wide it is declared, and its raw bits for a
/// tag that is only part of the field.
pub trait TagValue: Copy {
    fn tag_value(self) -> i64;
    fn tag_bits(self) -> u64;
}

macro_rules! tag_ints {
    ($($t:ty => $u:ty),*) => {$(
        impl TagValue for $t {
            fn tag_value(self) -> i64 {
                self as i64
            }
            fn tag_bits(self) -> u64 {
                self as $u as u64
            }
        }
    )*};
}
tag_ints!(u8 => u8, u16 => u16, u32 => u32, u64 => u64, i8 => u8, i16 => u16, i32 => u32, i64 => u64);

impl TagValue for f32 {
    fn tag_value(self) -> i64 {
        i64::from(self.to_bits())
    }
    fn tag_bits(self) -> u64 {
        u64::from(self.to_bits())
    }
}

impl<E: OpenEnum> TagValue for Open<E>
where
    E::Raw: TagValue,
{
    fn tag_value(self) -> i64 {
        self.raw().tag_value()
    }
    fn tag_bits(self) -> u64 {
        self.raw().tag_bits()
    }
}

/// What a pointer the description leaves unfollowed lands on.
pub enum Opaque {}

impl PointsTo for Opaque {
    fn pointee(_: Option<CountSource>) -> Pointee {
        Pointee::Opaque
    }
}

/// The declaration's reading of a length field as a count.
pub trait Len {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

macro_rules! len_ints {
    ($($t:ty),*) => {$(
        impl Len for $t {
            fn len(&self) -> usize {
                usize::try_from(*self).unwrap_or(0)
            }
        }
    )*};
}
len_ints!(u8, u16, u32, u64, i8, i16, i32, i64);

/// Machinery the generated code calls; not an API.
#[doc(hidden)]
pub mod __private {
    pub use super::owned::{
        counted_run_copied, keep_copies, keep_run_copy, keyed_run_to, lend, pad_run,
        run_to_child_by_value, run_to_child_copied, shared_len, write_by_value, Along, ByValue,
    };
    pub use crate::desc::{
        AddressSpace, BitRange, CountSource, FieldRepr, IntWidth, PackEnum, PackField,
        PackSelector, PackType, PackVariant, Pointee,
    };
    pub use std::mem::{offset_of, size_of};

    pub fn prefixed_above_one(count: usize) -> usize {
        CountSource::prefixed_above_one(count)
    }

    /// Keep an owned record's private field at `at` in its carry.
    pub fn hide<V: super::Stash>(carry: &mut super::Carry, at: usize, value: V) {
        carry.hide(at, value)
    }

    /// An owned record's private field at `at`; `None` for a value built from scratch.
    pub fn hidden<V: 'static>(carry: &super::Carry, at: usize) -> Option<&V> {
        carry.hidden(at)
    }

    /// Whether the pointer slot at `abs` holds a word that is not an address, which the carry keeps for `at`.
    pub fn keep_word(
        carry: &mut super::Carry,
        at: usize,
        kids: super::Kids<'_>,
        abs: usize,
    ) -> bool {
        match super::owned::word_at(kids, abs) {
            Some(word) => {
                carry.keep_word(at, word);
                true
            }
            None => false,
        }
    }

    /// Write back the word the carry kept for the slot at `at`, where the slot just written at `abs` is empty.
    pub fn put_word(
        carry: &super::Carry,
        at: usize,
        abs: usize,
        kids: &mut [(u32, crate::desc::Value)],
    ) {
        carry.put_word(at, abs, kids)
    }

    /// Keep for `at` whether the string slot at `abs` points live at an empty string.
    pub fn keep_empty_string(
        carry: &mut super::Carry,
        at: usize,
        kids: super::Kids<'_>,
        abs: usize,
    ) {
        carry.keep_empty_string(at, kids, abs)
    }

    /// Point the string slot just written at `abs` at an empty string again, where the carry kept that for `at` and it is still empty.
    pub fn put_empty_string(
        carry: &super::Carry,
        at: usize,
        abs: usize,
        kids: &mut [(u32, crate::desc::Value)],
    ) {
        carry.put_empty_string(at, abs, kids)
    }

    /// Whether the run at `abs` points live at nothing beside a length of zero, which the carry keeps for `at`; `zero` says the stated length is zero.
    pub fn keep_empty_run(
        carry: &mut super::Carry,
        at: usize,
        kids: super::Kids<'_>,
        abs: usize,
        zero: bool,
        rx: &super::Rx<'_>,
    ) -> bool {
        carry.keep_empty_run(at, kids, abs, zero, rx)
    }

    /// Write back the word the carry kept for the shared slot at `at`, just written at `abs`, or point the slot at what it shares where `cx` repairs.
    pub fn put_stale_word(
        carry: &super::Carry,
        what: &str,
        at: usize,
        abs: usize,
        kids: &mut [(u32, crate::desc::Value)],
        cx: &super::Cx,
    ) {
        carry.put_stale_word(what, at, abs, kids, cx)
    }

    /// Point the run of `E` just written at `abs` live at nothing again, where the carry kept that for `at` and it is still empty.
    pub fn put_empty_run<E: super::Owned>(
        carry: &super::Carry,
        at: usize,
        abs: usize,
        kids: &mut [(u32, crate::desc::Value)],
        cx: &super::Cx,
    ) -> Result<(), crate::error::Error> {
        carry.put_empty_run::<E>(at, abs, kids, cx)
    }
}
