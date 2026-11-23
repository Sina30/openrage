use std::ffi::CStr;
use std::fmt;
use std::marker::PhantomData;

use super::{Elem, Element, Field, Len, OpenEnum, Plain, PointsTo, Reach, Record};
use crate::desc::{AddressSpace, CountSource, FieldRepr, IntWidth, Pointee, Scalar};

/// A byte read as a flag: any value is valid, and anything but zero is set.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Bool8(u8);

impl Bool8 {
    pub const fn new(value: bool) -> Self {
        Self(value as u8)
    }
    pub const fn get(self) -> bool {
        self.0 != 0
    }
    pub const fn raw(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for Bool8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get())
    }
}

impl PartialEq<bool> for Bool8 {
    fn eq(&self, other: &bool) -> bool {
        self.get() == *other
    }
}

/// A 32-bit name hash, computed from the name rather than transcribed.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Hash32(u32);

impl Hash32 {
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    pub fn of(name: &str) -> Self {
        Self(crate::hash::string_hash_str(name))
    }
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Equal when the name hashes to this value.
impl PartialEq<&str> for Hash32 {
    fn eq(&self, name: &&str) -> bool {
        *self == Hash32::of(name)
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#010x}", self.0)
    }
}

/// A count the writer keeps in step with the run it counts: readable, never set by hand.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Count<W>(W);

impl<W: Copy> Count<W> {
    pub fn get(self) -> W {
        self.0
    }
}

impl<W: fmt::Debug> fmt::Debug for Count<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<W: Len> Len for Count<W> {
    fn len(&self) -> usize {
        self.0.len()
    }
}

/// A value an enum names, or a number no variant covers.
#[repr(transparent)]
pub struct Open<E: OpenEnum> {
    raw: E::Raw,
    _e: PhantomData<fn() -> E>,
}

impl<E: OpenEnum> Open<E> {
    pub fn new(value: E) -> Self {
        Self {
            raw: value.to_raw(),
            _e: PhantomData,
        }
    }
    pub fn get(self) -> E {
        E::from_raw(self.raw)
    }
    pub fn raw(self) -> E::Raw {
        self.raw
    }
}

impl<E: OpenEnum> Clone for Open<E> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<E: OpenEnum> Copy for Open<E> {}

impl<E: OpenEnum + fmt::Debug> fmt::Debug for Open<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl<E: OpenEnum> PartialEq<E> for Open<E>
where
    E: PartialEq,
{
    fn eq(&self, other: &E) -> bool {
        self.get() == *other
    }
}

impl<E: OpenEnum> Default for Open<E> {
    fn default() -> Self {
        <Self as Plain>::zeroed()
    }
}

impl<E: OpenEnum + PartialEq> PartialEq for Open<E> {
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}

macro_rules! vectors {
    ($($name:ident [$($f:ident),*] $n:literal),*) => {$(
        #[repr(C)]
        #[derive(Clone, Copy, Default, PartialEq, Debug)]
        pub struct $name { $(pub $f: f32),* }

        // SAFETY: `$n` consecutive `f32`s; no padding.
        unsafe impl Plain for $name {}

        impl Field for $name {
            fn repr(_: Option<CountSource>) -> FieldRepr {
                FieldRepr::ScalarArray { element: Scalar::F32, count: $n }
            }
        }

        impl Element for $name {
            fn elem() -> Elem {
                Elem::Other($n * 4)
            }
        }
    )*};
}

vectors!(Vec2 [x, y] 2, Vec3 [x, y, z] 3, Vec4 [x, y, z, w] 4);

macro_rules! pointer {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// The address is the file's; there is no constructor, because the
        /// canonical writer rewrites every pointer from the child it points at.
        #[repr(transparent)]
        pub struct $name<T: ?Sized> {
            raw: u64,
            _t: PhantomData<fn(&T)>,
        }

        impl<T: ?Sized> $name<T> {
            pub const fn raw(self) -> u64 {
                self.raw
            }
            pub const fn is_null(self) -> bool {
                self.raw == 0
            }
        }

        impl<T: ?Sized> Clone for $name<T> {
            fn clone(&self) -> Self {
                *self
            }
        }
        impl<T: ?Sized> Copy for $name<T> {}

        impl<T: ?Sized> PartialEq for $name<T> {
            fn eq(&self, other: &Self) -> bool {
                self.raw == other.raw
            }
        }

        impl<T: ?Sized> Default for $name<T> {
            fn default() -> Self {
                Self {
                    raw: 0,
                    _t: PhantomData,
                }
            }
        }

        impl<T: ?Sized> fmt::Debug for $name<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:#x})", stringify!($name), self.raw)
            }
        }

        // SAFETY: one `u64`.
        unsafe impl<T: ?Sized + 'static> Plain for $name<T> {}
    };
}

macro_rules! pointer_field {
    ($name:ident, $space:expr) => {
        impl<T: ?Sized + PointsTo> Field for $name<T> {
            fn repr(len: Option<CountSource>) -> FieldRepr {
                FieldRepr::Ptr(T::pointee(len))
            }
            fn space() -> Option<AddressSpace> {
                $space
            }
            fn reach(reach: &mut Reach) {
                T::reach(reach)
            }
        }
    };
}

pointer!(Ptr, "A pointer into the system half of a resource.");
pointer!(PhysPtr, "A pointer into the graphics half of a resource.");
pointer!(MemberPtr, "A pointer at a member of the record holding it.");
pointer_field!(Ptr, Some(AddressSpace::Virtual));
pointer_field!(PhysPtr, Some(AddressSpace::Physical));

impl<T: Record> Field for MemberPtr<T> {
    fn repr(_: Option<CountSource>) -> FieldRepr {
        FieldRepr::Ptr(Pointee::Member {
            type_name: T::name(),
        })
    }
    fn space() -> Option<AddressSpace> {
        Some(AddressSpace::Virtual)
    }
}

/// A pointer at a NUL-terminated string.
pub type Str = Ptr<CStr>;

impl Element for Str {
    fn elem() -> Elem {
        Elem::Str
    }
    fn space() -> Option<AddressSpace> {
        Some(AddressSpace::Virtual)
    }
}

impl<T: Record> Element for Ptr<T> {
    fn elem() -> Elem {
        Elem::Ptr(T::name())
    }
    fn reach(reach: &mut Reach) {
        reach.visit::<T>()
    }
    fn space() -> Option<AddressSpace> {
        Some(AddressSpace::Virtual)
    }
}

/// A run of pointers nothing follows is a run of eight-byte words.
impl Element for Ptr<super::Opaque> {
    fn elem() -> Elem {
        Elem::Other(8)
    }
}

impl<T: Record> Element for MemberPtr<T> {
    fn elem() -> Elem {
        Elem::Member(T::name())
    }
}

/// A pointer, a count and a capacity: a run that says its own length.
#[repr(C)]
pub struct Array<T> {
    raw: u64,
    count: u16,
    capacity: u16,
    _pad: u32,
    _t: PhantomData<fn(&T)>,
}

impl<T> Array<T> {
    /// No address yet, and a count and capacity of `count`; the writer places the run.
    pub const fn counted(count: u16) -> Self {
        Self {
            raw: 0,
            count,
            capacity: count,
            _pad: 0,
            _t: PhantomData,
        }
    }

    /// No address yet, a count, and a capacity at least that count.
    pub const fn with_capacity(count: u16, capacity: u16) -> Self {
        Self {
            raw: 0,
            count,
            capacity,
            _pad: 0,
            _t: PhantomData,
        }
    }

    pub const fn raw(self) -> u64 {
        self.raw
    }
    pub const fn count(self) -> usize {
        self.count as usize
    }
    pub const fn capacity(self) -> usize {
        self.capacity as usize
    }
}

impl<T> Clone for Array<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Array<T> {}

impl<T> fmt::Debug for Array<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Array({:#x}; {}/{})",
            self.raw, self.count, self.capacity
        )
    }
}

// SAFETY: 8 + 2 + 2 + 4 bytes and a zero-sized marker; no padding.
unsafe impl<T: 'static> Plain for Array<T> {}

const _: () = assert!(std::mem::size_of::<Array<u8>>() == crate::desc::AT_ARRAY_SIZE);

impl<T: Element + Plain> Field for Array<T> {
    fn repr(_: Option<CountSource>) -> FieldRepr {
        match <[T] as PointsTo>::pointee(Some(CountSource::Inline)) {
            // A run of bytes still says its length inline; it is not a blob.
            Pointee::Bytes { .. } => FieldRepr::AtArray(Pointee::Many {
                type_name: None,
                count: CountSource::Inline,
                element_size: Some(1),
            }),
            pointee => FieldRepr::AtArray(pointee),
        }
    }
    fn space() -> Option<AddressSpace> {
        Some(AddressSpace::Virtual)
    }
    fn reach(reach: &mut Reach) {
        T::reach(reach)
    }
}

/// A run immediately after its owner, located by nothing but the owner's size.
#[repr(transparent)]
pub struct Trailing<T> {
    _t: PhantomData<fn(&T)>,
}

impl<T> Clone for Trailing<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Trailing<T> {}

impl<T> fmt::Debug for Trailing<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Trailing")
    }
}

// SAFETY: zero-sized.
unsafe impl<T: 'static> Plain for Trailing<T> {}

impl<T: Element + Plain> Field for Trailing<T> {
    fn repr(len: Option<CountSource>) -> FieldRepr {
        let count = len.unwrap_or(CountSource::Unknown);
        match T::elem() {
            Elem::Type(name) => FieldRepr::Trailing {
                type_name: Some(name),
                count,
                element_size: None,
            },
            _ => FieldRepr::Trailing {
                type_name: None,
                count,
                element_size: Some(std::mem::size_of::<T>()),
            },
        }
    }
    fn reach(reach: &mut Reach) {
        T::reach(reach)
    }
}

// SAFETY: one byte, one `u32`, and a `W` that is itself plain.
unsafe impl Plain for Bool8 {}
unsafe impl Plain for Hash32 {}
unsafe impl<W: Plain> Plain for Count<W> {}
unsafe impl<E: OpenEnum> Plain for Open<E> {}

impl Field for Bool8 {
    fn repr(_: Option<CountSource>) -> FieldRepr {
        FieldRepr::Scalar(Scalar::Bool)
    }
}

impl Element for Bool8 {
    fn elem() -> Elem {
        Elem::Scalar(Scalar::Bool)
    }
}

impl Field for Hash32 {
    fn repr(_: Option<CountSource>) -> FieldRepr {
        FieldRepr::Scalar(Scalar::Int(IntWidth::U32))
    }
}

impl Element for Hash32 {
    fn elem() -> Elem {
        Elem::Scalar(Scalar::Int(IntWidth::U32))
    }
}

impl<W: Field> Field for Count<W> {
    fn repr(len: Option<CountSource>) -> FieldRepr {
        W::repr(len)
    }
}

impl<E: OpenEnum> Field for Open<E> {
    fn repr(_: Option<CountSource>) -> FieldRepr {
        FieldRepr::Enum {
            type_name: E::name().to_string(),
            width: E::WIDTH,
        }
    }
    fn reach(reach: &mut Reach) {
        reach.visit_enum::<E>()
    }
}
