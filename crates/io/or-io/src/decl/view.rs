use std::any::TypeId;
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{Arc, Mutex, OnceLock};

use super::{Described, Plain, Ptr, Reach, Record, Tagged, VariantOf};
use crate::desc::{FieldRepr, Image, Unresolvable};
pub use crate::error::ViewError;

/// What `View<'a, T>` derefs to: `#[record]` generates one per record, holding
/// its accessors, and it derefs on to `T` for the fields.
///
/// A generated type rather than an `impl View<'a, T>`, because a record
/// declared outside this crate could not add inherent methods to `View`.
pub trait Access<'a, T: Record>: Copy + std::ops::Deref<Target = T> {
    fn wrap(raw: RawView<'a, T>) -> Self;
    fn raw(&self) -> &RawView<'a, T>;
}

/// A declared record read in place: its accessors, then its fields.
pub struct View<'a, T: Record> {
    access: T::Access<'a>,
}

impl<T: Record> Clone for View<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Record> Copy for View<'_, T> {}

impl<'a, T: Record> std::ops::Deref for View<'a, T> {
    type Target = T::Access<'a>;
    #[inline]
    fn deref(&self) -> &T::Access<'a> {
        &self.access
    }
}

impl<T: Record + std::fmt::Debug> std::fmt::Debug for View<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{:#x} ", View::offset(self))?;
        View::get(self).fmt(f)
    }
}

impl<'a, T: Record> View<'a, T> {
    fn from_raw(raw: RawView<'a, T>) -> Self {
        Self {
            access: T::Access::wrap(raw),
        }
    }

    /// The record at `offset` in `img`.
    pub fn at_offset(img: Image<'a>, offset: usize) -> Result<Self, ViewError> {
        RawView::at_offset(img, offset).map(Self::from_raw)
    }

    /// The record `address` points at.
    pub fn at_address(img: Image<'a>, address: u64) -> Result<Self, ViewError> {
        let offset = img
            .resolve(address)
            .map_err(|reason| ViewError::BadPointer {
                ty: T::name().into(),
                field: "<root>",
                address,
                reason: reason.to_string().into(),
            })?;
        Self::at_offset(img, offset)
    }

    /// The record itself, for the image's lifetime. An associated function,
    /// so no declared field or accessor can shadow it.
    pub fn get(this: &Self) -> &'a T {
        this.access.raw().t
    }

    pub fn offset(this: &Self) -> usize {
        this.access.raw().at
    }

    pub fn image(this: &Self) -> Image<'a> {
        this.access.raw().img
    }

    /// The same bytes as the variant `V`, when the base's tag selects it.
    pub fn variant<V: VariantOf<T>>(this: &Self) -> Result<Option<View<'a, V>>, ViewError>
    where
        T: Tagged,
    {
        if !V::TAGS.contains(&Self::get(this).tag()) {
            return Ok(None);
        }
        View::at_offset(Self::image(this), Self::offset(this)).map(Some)
    }
}

/// `bytes[at..]` as a `&T`, checked for room and for alignment.
fn cast<T: Plain + Described>(bytes: &[u8], at: usize) -> Result<&T, ViewError> {
    let () = T::__LAYOUT;
    let len = std::mem::size_of::<T>();
    if at.checked_add(len).is_none_or(|end| end > bytes.len()) {
        return Err(ViewError::OutOfBounds {
            ty: T::name(),
            offset: at,
            len,
            size: bytes.len(),
        });
    }
    let ptr = bytes.as_ptr().wrapping_add(at);
    if ptr.align_offset(std::mem::align_of::<T>()) != 0 {
        return Err(ViewError::Misaligned {
            ty: T::name(),
            offset: at,
            align: std::mem::align_of::<T>(),
        });
    }
    // SAFETY: in bounds and aligned, both checked above, and `T: Plain`
    // makes every bit pattern of those bytes a valid `T`.
    Ok(unsafe { &*ptr.cast::<T>() })
}

/// `bytes[at..]` as `n` consecutive `T`s, checked the same way.
fn cast_run<'a, T: Plain>(
    bytes: &'a [u8],
    at: usize,
    n: usize,
    ty: &dyn Fn() -> String,
) -> Result<&'a [T], ViewError> {
    if n == 0 {
        return Ok(&[]);
    }
    let len = n.saturating_mul(std::mem::size_of::<T>());
    if at.checked_add(len).is_none_or(|end| end > bytes.len()) {
        return Err(ViewError::OutOfBounds {
            ty: ty(),
            offset: at,
            len,
            size: bytes.len(),
        });
    }
    let ptr = bytes.as_ptr().wrapping_add(at);
    if ptr.align_offset(std::mem::align_of::<T>()) != 0 {
        return Err(ViewError::Misaligned {
            ty: ty(),
            offset: at,
            align: std::mem::align_of::<T>(),
        });
    }
    // SAFETY: `n` elements fit and the first is aligned, so all are; `T: Plain`.
    Ok(unsafe { std::slice::from_raw_parts(ptr.cast::<T>(), n) })
}

/// The `&T`, the image and the offset a generated accessor works from.
pub struct RawView<'a, T> {
    t: &'a T,
    img: Image<'a>,
    at: usize,
}

impl<T> Clone for RawView<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for RawView<'_, T> {}

impl<'a, T: Record> RawView<'a, T> {
    fn at_offset(img: Image<'a>, offset: usize) -> Result<Self, ViewError> {
        Ok(Self {
            t: cast::<T>(img.bytes(), offset)?,
            img,
            at: offset,
        })
    }

    #[inline]
    pub fn get(&self) -> &'a T {
        self.t
    }

    fn resolve(&self, field: &'static str, address: u64) -> Result<Option<usize>, ViewError> {
        match self.img.resolve(address) {
            Ok(offset) => Ok(Some(offset)),
            Err(Unresolvable::Null) => Ok(None),
            Err(reason) => Err(ViewError::BadPointer {
                ty: T::name().into(),
                field,
                address,
                reason: reason.to_string().into(),
            }),
        }
    }

    #[doc(hidden)]
    pub fn __embed<U: Record>(&self, inner: &'a U, offset: usize) -> View<'a, U> {
        View::from_raw(RawView {
            t: inner,
            img: self.img,
            at: self.at + offset,
        })
    }

    #[doc(hidden)]
    pub fn __one<U: Record>(
        &self,
        field: &'static str,
        address: u64,
    ) -> Result<Option<View<'a, U>>, ViewError> {
        match self.resolve(field, address)? {
            None => Ok(None),
            Some(offset) => View::at_offset(self.img, offset).map(Some),
        }
    }

    #[doc(hidden)]
    pub fn __str(&self, field: &'static str, address: u64) -> Result<Option<&'a CStr>, ViewError> {
        let Some(offset) = self.resolve(field, address)? else {
            return Ok(None);
        };
        let tail = self.img.bytes().get(offset..).unwrap_or(&[]);
        CStr::from_bytes_until_nul(tail)
            .map(Some)
            .map_err(|_| ViewError::OutOfBounds {
                ty: format!("{}.{field}", T::name()),
                offset,
                len: tail.len() + 1,
                size: self.img.len(),
            })
    }

    #[doc(hidden)]
    pub fn __run<U: Plain>(
        &self,
        field: &'static str,
        address: u64,
        n: usize,
    ) -> Result<Run<'a, U>, ViewError> {
        if n == 0 {
            return Ok(Run::empty(self.img));
        }
        let Some(offset) = self.resolve(field, address)? else {
            return Err(ViewError::BadPointer {
                ty: T::name().into(),
                field,
                address,
                reason: format!("null, and the count says {n}").into(),
            });
        };
        let items = cast_run::<U>(self.img.bytes(), offset, n, &|| {
            format!("{}.{field}", T::name())
        })?;
        Ok(Run {
            items,
            img: self.img,
            first: offset,
        })
    }

    #[doc(hidden)]
    pub fn __trailing<U: Plain>(
        &self,
        field: &'static str,
        n: usize,
    ) -> Result<Run<'a, U>, ViewError> {
        let offset = self.at + std::mem::size_of::<T>();
        let items = cast_run::<U>(self.img.bytes(), offset, n, &|| {
            format!("{}.{field}", T::name())
        })?;
        Ok(Run {
            items,
            img: self.img,
            first: offset,
        })
    }

    #[doc(hidden)]
    pub fn __member<U: Record>(
        &self,
        field: &'static str,
        address: u64,
    ) -> Result<Option<View<'a, U>>, ViewError> {
        let Some(offset) = self.resolve(field, address)? else {
            return Ok(None);
        };
        let not_a_member = || ViewError::NotAMember {
            ty: T::name().into(),
            field,
            member: U::name().into(),
            address,
        };
        let inside = offset.checked_sub(self.at).ok_or_else(not_a_member)?;
        if !member_starts::<T>().contains(&(inside, U::name())) {
            return Err(not_a_member());
        }
        View::at_offset(self.img, offset).map(Some)
    }
}

/// Every embedded record's offset inside `T`, with its type, at any depth.
fn member_starts<T: Described>() -> Arc<Vec<(usize, String)>> {
    type Cache = Mutex<HashMap<TypeId, Arc<Vec<(usize, String)>>>>;
    static STARTS: OnceLock<Cache> = OnceLock::new();
    let cache = STARTS.get_or_init(Default::default);
    if let Some(found) = cache
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&TypeId::of::<T>())
    {
        return found.clone();
    }
    let mut reach = Reach::new("<members>");
    reach.visit::<T>();
    fn walk(reach: &Reach, type_name: &str, base: usize, out: &mut Vec<(usize, String)>) {
        let Some(ty) = reach.type_by_name(type_name) else {
            return;
        };
        for field in &ty.fields {
            let (name, count) = match &field.repr {
                FieldRepr::Inline { type_name } => (type_name, 1),
                FieldRepr::InlineArray { type_name, count } => (type_name, *count),
                _ => continue,
            };
            let size = reach.type_by_name(name).map_or(0, |t| t.size);
            for i in 0..count {
                let at = base + field.offset + i * size;
                out.push((at, name.clone()));
                walk(reach, name, at, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&reach, &T::name(), 0, &mut out);
    let out = Arc::new(out);
    cache
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(TypeId::of::<T>(), out.clone());
    out
}

/// A run of `T` read in place: a slice, and the image to follow what its elements point at.
pub struct Run<'a, T> {
    items: &'a [T],
    img: Image<'a>,
    first: usize,
}

impl<T> Clone for Run<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Run<'_, T> {}

impl<T> std::ops::Deref for Run<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.items
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Run<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.items.fmt(f)
    }
}

impl<'a, T> Run<'a, T> {
    fn empty(img: Image<'a>) -> Self {
        Self {
            items: &[],
            img,
            first: 0,
        }
    }

    pub fn as_slice(&self) -> &'a [T] {
        self.items
    }
}

impl<'a, T: Record> Run<'a, T> {
    pub fn view(&self, index: usize) -> Option<View<'a, T>> {
        self.items.get(index).map(|t| {
            View::from_raw(RawView {
                t,
                img: self.img,
                at: self.first + index * std::mem::size_of::<T>(),
            })
        })
    }

    pub fn views(&self) -> impl ExactSizeIterator<Item = View<'a, T>> + 'a {
        let (img, first) = (self.img, self.first);
        self.items.iter().enumerate().map(move |(i, t)| {
            View::from_raw(RawView {
                t,
                img,
                at: first + i * std::mem::size_of::<T>(),
            })
        })
    }
}

impl<'a, T: Record> Run<'a, Ptr<T>> {
    /// What element `index` points at; `None` for a null slot.
    pub fn follow(&self, index: usize) -> Result<Option<View<'a, T>>, ViewError> {
        let Some(slot) = self.items.get(index) else {
            return Ok(None);
        };
        match self.img.resolve(slot.raw()) {
            Ok(offset) => View::at_offset(self.img, offset).map(Some),
            Err(Unresolvable::Null) => Ok(None),
            Err(reason) => Err(ViewError::BadPointer {
                ty: format!("[{}]", T::name()).into(),
                field: "<element>",
                address: slot.raw(),
                reason: reason.to_string().into(),
            }),
        }
    }
}
