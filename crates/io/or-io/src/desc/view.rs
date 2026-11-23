use std::marker::PhantomData;

use thiserror::Error;

use crate::desc::name::{FieldName, TypeName};
use crate::desc::pack::{Pack, PackEnum, PackType};
use crate::desc::repr::{FieldRepr, IntWidth, Scalar};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BindError {
    #[error("pack `{pack}` describes no type named `{type_name}`")]
    NoType { pack: String, type_name: String },

    #[error("`{type_name}` in pack `{pack}` has no field named `{field_name}`")]
    NoField {
        pack: String,
        type_name: String,
        field_name: String,
    },

    #[error(transparent)]
    WrongRepr(Box<WrongRepr>),

    #[error(transparent)]
    Disagrees(Box<Disagreement>),

    /// The pack's description of a type contradicts itself.
    ///
    /// Checked by [`PackType::incoherences`] in place of a [`Disagreement`]
    /// against any target other than the one a declaration's `offset_of!`
    /// arrangement was written for.
    ///
    /// ⚠ **This is the weaker of the two and it is not a substitute**: a
    /// description can be perfectly coherent and still be wrong about the file;
    /// only real bytes settle that.
    #[error(transparent)]
    Incoherent(Box<Incoherence>),
}

/// The pack's description of a type does not fit together.
///
/// Two fields over the same byte, or a field running past the end of the type
/// that declares it. `problems` holds every one found rather than the first,
/// because a stale offset usually moves more than one field and reporting them
/// one build at a time is how a correction takes four attempts.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error(
    "pack `{pack}` describes `{type_name}` in a way that contradicts itself: {}",
    problems.join("; ")
)]
pub struct Incoherence {
    pub pack: String,
    pub type_name: String,
    pub problems: Vec<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error(
    "`{type_name}.{what}` is {pack_value:#x} in pack `{pack}` and {compiled_value:#x} in the \
     compiled reader; the two describe different bytes and one of them is wrong"
)]
pub struct Disagreement {
    pub pack: String,
    pub type_name: String,
    pub what: String,
    pub pack_value: usize,
    pub compiled_value: usize,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("`{type_name}.{field_name}` in pack `{pack}` is {found}, but has to be read as {expected}")]
pub struct WrongRepr {
    pub pack: String,
    pub type_name: String,
    pub field_name: String,
    pub found: String,
    pub expected: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field<T> {
    offset: usize,
    width: IntWidth,
    writable: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Field<T> {
    pub fn offset(self) -> usize {
        self.offset
    }

    pub fn is_writable(self) -> bool {
        self.writable
    }
}

pub trait FromField: Sized {
    fn accepts(repr: &FieldRepr) -> bool;
    fn expected() -> &'static str;
    fn read(bytes: &[u8], offset: usize, width: IntWidth) -> Option<Self>;
}

macro_rules! integer_field {
    ($($ty:ty),* $(,)?) => {
        $(
            impl FromField for $ty {
                fn accepts(repr: &FieldRepr) -> bool {
                    matches!(
                        repr,
                        FieldRepr::Scalar(Scalar::Int(_)) | FieldRepr::Enum { .. }
                    )
                }

                fn expected() -> &'static str {
                    stringify!($ty)
                }

                fn read(bytes: &[u8], offset: usize, width: IntWidth) -> Option<Self> {
                    let raw = width.read(bytes, offset)?;
                    <$ty>::try_from(raw).ok()
                }
            }
        )*
    };
}

integer_field!(u8, u16, u32, u64, i8, i16, i32, i64, usize);

impl FromField for f32 {
    fn accepts(repr: &FieldRepr) -> bool {
        matches!(repr, FieldRepr::Scalar(Scalar::F32))
    }

    fn expected() -> &'static str {
        "f32"
    }

    fn read(bytes: &[u8], offset: usize, _width: IntWidth) -> Option<Self> {
        let slice = bytes.get(offset..offset.checked_add(4)?)?;
        Some(f32::from_le_bytes(slice.try_into().ok()?))
    }
}

impl FromField for f64 {
    fn accepts(repr: &FieldRepr) -> bool {
        matches!(repr, FieldRepr::Scalar(Scalar::F64))
    }

    fn expected() -> &'static str {
        "f64"
    }

    fn read(bytes: &[u8], offset: usize, _width: IntWidth) -> Option<Self> {
        let slice = bytes.get(offset..offset.checked_add(8)?)?;
        Some(f64::from_le_bytes(slice.try_into().ok()?))
    }
}

impl FromField for bool {
    fn accepts(repr: &FieldRepr) -> bool {
        matches!(
            repr,
            FieldRepr::Scalar(Scalar::Bool) | FieldRepr::Scalar(Scalar::Int(IntWidth::U8))
        )
    }

    fn expected() -> &'static str {
        "bool"
    }

    fn read(bytes: &[u8], offset: usize, _width: IntWidth) -> Option<Self> {
        bytes.get(offset).map(|&b| b != 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteError {
    OutOfBounds { offset: usize, size: usize },
    DoesNotFit { width: IntWidth },
    Structural,
    NotADeclaredValue { value: i64 },
    WrongKind,
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds { offset, size } => {
                write!(f, "offset {offset:#x} is past the buffer's {size:#x} bytes")
            }
            Self::DoesNotFit { width } => {
                write!(f, "the value does not fit a {width:?} field")
            }
            Self::Structural => f.write_str(
                "this field is structural -- a count, a capacity or a tagged base's \
                 selector -- and changing it needs a rebuild rather than a patch",
            ),
            Self::NotADeclaredValue { value } => {
                write!(f, "{value} is not a value this enum declares")
            }
            Self::WrongKind => {
                f.write_str("the value is a different kind from the one this field is declared as")
            }
        }
    }
}

impl std::error::Error for WriteError {}

pub trait ToField: Sized {
    fn accepts(repr: &FieldRepr) -> bool;
    fn expected() -> &'static str;
    fn write(
        bytes: &mut [u8],
        offset: usize,
        width: IntWidth,
        value: Self,
    ) -> Result<(), WriteError>;
}

macro_rules! integer_to_field {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ToField for $ty {
                fn accepts(repr: &FieldRepr) -> bool {
                    <$ty as FromField>::accepts(repr)
                }

                fn expected() -> &'static str {
                    stringify!($ty)
                }

                fn write(
                    bytes: &mut [u8],
                    offset: usize,
                    width: IntWidth,
                    value: Self,
                ) -> Result<(), WriteError> {
                    let size = bytes.len();
                    let raw = i128::from(value);
                    if !width.holds(raw) {
                        return Err(WriteError::DoesNotFit { width });
                    }
                    if width.write(bytes, offset, raw) {
                        Ok(())
                    } else {
                        Err(WriteError::OutOfBounds { offset, size })
                    }
                }
            }
        )*
    };
}

integer_to_field!(u8, u16, u32, u64, i8, i16, i32, i64);

impl ToField for usize {
    fn accepts(repr: &FieldRepr) -> bool {
        <usize as FromField>::accepts(repr)
    }

    fn expected() -> &'static str {
        "usize"
    }

    fn write(
        bytes: &mut [u8],
        offset: usize,
        width: IntWidth,
        value: Self,
    ) -> Result<(), WriteError> {
        <u64 as ToField>::write(bytes, offset, width, value as u64)
    }
}

macro_rules! float_to_field {
    ($ty:ty, $size:literal) => {
        impl ToField for $ty {
            fn accepts(repr: &FieldRepr) -> bool {
                <$ty as FromField>::accepts(repr)
            }

            fn expected() -> &'static str {
                stringify!($ty)
            }

            fn write(
                bytes: &mut [u8],
                offset: usize,
                _width: IntWidth,
                value: Self,
            ) -> Result<(), WriteError> {
                let size = bytes.len();
                let end = offset
                    .checked_add($size)
                    .ok_or(WriteError::OutOfBounds { offset, size })?;
                let slot = bytes
                    .get_mut(offset..end)
                    .ok_or(WriteError::OutOfBounds { offset, size })?;
                slot.copy_from_slice(&value.to_le_bytes());
                Ok(())
            }
        }
    };
}

float_to_field!(f32, 4);
float_to_field!(f64, 8);

impl ToField for bool {
    fn accepts(repr: &FieldRepr) -> bool {
        <bool as FromField>::accepts(repr)
    }

    fn expected() -> &'static str {
        "bool"
    }

    fn write(
        bytes: &mut [u8],
        offset: usize,
        _width: IntWidth,
        value: Self,
    ) -> Result<(), WriteError> {
        let size = bytes.len();
        let slot = bytes
            .get_mut(offset)
            .ok_or(WriteError::OutOfBounds { offset, size })?;
        *slot = u8::from(value);
        Ok(())
    }
}

#[derive(Debug)]
pub struct View<'p> {
    pack: &'p Pack,
    pack_id: String,
    type_name: String,
    ty: &'p PackType,
    base: usize,
}

impl<'p> View<'p> {
    pub fn of(pack: &'p Pack, name: TypeName) -> Result<Self, BindError> {
        let ty = pack
            .type_by_name(name.as_str())
            .ok_or_else(|| BindError::NoType {
                pack: pack.id(),
                type_name: name.as_str().to_string(),
            })?;

        Ok(Self {
            pack,
            pack_id: pack.id(),
            type_name: ty.name.clone(),
            ty,
            base: 0,
        })
    }

    pub fn size(&self) -> usize {
        self.ty.size
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// Which target the pack this view came from describes.
    pub fn target(&self) -> &'p str {
        &self.pack.target
    }

    /// Hold the pack's description of this type to its own arithmetic.
    ///
    /// [`PackType::incoherences`] says what is checked and, more importantly,
    /// what is not. Embedded types are sized through this view's pack, so a
    /// struct stored by value is measured rather than skipped.
    pub fn require_coherent(&self) -> Result<(), BindError> {
        let problems = self
            .ty
            .incoherences(|name| self.pack.type_by_name(name).map(|t| t.size));
        if problems.is_empty() {
            return Ok(());
        }
        Err(BindError::Incoherent(Box::new(Incoherence {
            pack: self.pack_id.clone(),
            type_name: self.type_name.clone(),
            problems,
        })))
    }

    /// Resolve a field into a typed, offset-carrying handle.
    ///
    /// The whole cost of the indirection is paid here, once.
    pub fn field<T: FromField>(&self, name: FieldName) -> Result<Field<T>, BindError> {
        let field = self
            .ty
            .field_by_name(name.as_str())
            .ok_or_else(|| BindError::NoField {
                pack: self.pack_id.clone(),
                type_name: self.type_name.clone(),
                field_name: name.as_str().to_string(),
            })?;

        if !T::accepts(&field.repr) {
            return Err(BindError::WrongRepr(Box::new(WrongRepr {
                pack: self.pack_id.clone(),
                type_name: self.type_name.clone(),
                field_name: field.name.clone(),
                found: describe(&field.repr),
                expected: T::expected().to_string(),
            })));
        }

        Ok(Field {
            offset: self.base + field.offset,
            width: match &field.repr {
                FieldRepr::Scalar(Scalar::Int(w)) => *w,
                FieldRepr::Enum { width, .. } => *width,
                _ => IntWidth::U32,
            },
            writable: !self.ty.is_structural(&field.name),
            _marker: PhantomData,
        })
    }

    pub fn inner(&self, name: FieldName) -> Result<View<'p>, BindError> {
        let field = self.declared(name)?;
        let FieldRepr::Inline { type_name } = &field.repr else {
            return Err(self.wrong(field, "an embedded described type".into()));
        };
        self.descend(type_name, self.base + field.offset)
    }

    pub fn inner_element(&self, name: FieldName, index: usize) -> Result<View<'p>, BindError> {
        let field = self.declared(name)?;
        let FieldRepr::InlineArray { type_name, count } = &field.repr else {
            return Err(self.wrong(field, "a fixed inline run of a described type".into()));
        };
        if index >= *count {
            return Err(self.wrong(field, format!("a run with more than {index} element(s)")));
        }

        let element = self
            .pack
            .type_by_name(type_name)
            .ok_or_else(|| BindError::NoType {
                pack: self.pack_id.clone(),
                type_name: type_name.clone(),
            })?;

        self.descend(type_name, self.base + field.offset + index * element.size)
    }

    fn declared(&self, name: FieldName) -> Result<&'p crate::desc::pack::PackField, BindError> {
        self.ty
            .field_by_name(name.as_str())
            .ok_or_else(|| BindError::NoField {
                pack: self.pack_id.clone(),
                type_name: self.type_name.clone(),
                field_name: name.as_str().to_string(),
            })
    }

    fn wrong(&self, field: &crate::desc::pack::PackField, expected: String) -> BindError {
        BindError::WrongRepr(Box::new(WrongRepr {
            pack: self.pack_id.clone(),
            type_name: self.type_name.clone(),
            field_name: field.name.clone(),
            found: describe(&field.repr),
            expected,
        }))
    }

    fn descend(&self, type_name: &str, base: usize) -> Result<View<'p>, BindError> {
        let ty = self
            .pack
            .type_by_name(type_name)
            .ok_or_else(|| BindError::NoType {
                pack: self.pack_id.clone(),
                type_name: type_name.to_string(),
            })?;

        Ok(View {
            pack: self.pack,
            pack_id: self.pack_id.clone(),
            type_name: ty.name.clone(),
            ty,
            base,
        })
    }

    pub fn element_field<T: FromField>(
        &self,
        name: FieldName,
        index: usize,
    ) -> Result<Field<T>, BindError> {
        let field = self
            .ty
            .field_by_name(name.as_str())
            .ok_or_else(|| BindError::NoField {
                pack: self.pack_id.clone(),
                type_name: self.type_name.clone(),
                field_name: name.as_str().to_string(),
            })?;

        let wrong = |expected: String| {
            BindError::WrongRepr(Box::new(WrongRepr {
                pack: self.pack_id.clone(),
                type_name: self.type_name.clone(),
                field_name: field.name.clone(),
                found: describe(&field.repr),
                expected,
            }))
        };

        let FieldRepr::ScalarArray { element, count } = &field.repr else {
            return Err(wrong(format!("an inline run of {}", T::expected())));
        };
        if index >= *count {
            return Err(wrong(format!(
                "an inline run with more than {index} element(s)"
            )));
        }
        if !T::accepts(&FieldRepr::Scalar(*element)) {
            return Err(wrong(T::expected().to_string()));
        }

        Ok(Field {
            offset: self.base + field.offset + index * element.size(),
            width: match element {
                Scalar::Int(w) => *w,
                _ => IntWidth::U32,
            },
            writable: !self.ty.is_structural(&field.name),
            _marker: PhantomData,
        })
    }

    pub fn enum_field(&self, name: FieldName) -> Result<EnumField<'p>, BindError> {
        let field = self.declared(name)?;
        let FieldRepr::Enum { type_name, width } = &field.repr else {
            return Err(self.wrong(field, "an enum the pack declares".into()));
        };

        let declared = self
            .pack
            .enum_by_name(type_name)
            .ok_or_else(|| BindError::NoType {
                pack: self.pack_id.clone(),
                type_name: type_name.clone(),
            })?;

        Ok(EnumField {
            offset: self.base + field.offset,
            width: *width,
            writable: !self.ty.is_structural(&field.name),
            declared,
        })
    }

    pub fn element_count(&self, name: FieldName) -> Option<usize> {
        match &self.ty.field_by_name(name.as_str())?.repr {
            FieldRepr::ScalarArray { count, .. } => Some(*count),
            _ => None,
        }
    }

    pub fn field_offset(&self, name: FieldName) -> Option<usize> {
        self.ty
            .field_by_name(name.as_str())
            .map(|f| self.base + f.offset)
    }

    pub fn count_field_of(&self, name: FieldName) -> Option<&'p str> {
        self.ty.field_by_name(name.as_str())?.repr.count_field()
    }

    pub fn capacity_field_of(&self, name: FieldName) -> Option<&'p str> {
        self.ty.field_by_name(name.as_str())?.repr.capacity_field()
    }

    pub fn optional_field<T: FromField>(
        &self,
        name: FieldName,
    ) -> Result<Option<Field<T>>, BindError> {
        if self.ty.field_by_name(name.as_str()).is_none() {
            return Ok(None);
        }
        self.field(name).map(Some)
    }

    pub fn is_tagged(&self) -> bool {
        self.ty.selector.is_some()
    }

    pub fn concrete_type(&self, at: Cursor<'_>) -> Option<&'p str> {
        let selector = self.ty.selector.as_ref()?;
        let tag = selector.read_tag(self.ty, at.bytes(), at.base().checked_add(self.base)?)?;
        selector.type_for(tag)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EnumField<'p> {
    offset: usize,
    width: IntWidth,
    writable: bool,
    declared: &'p PackEnum,
}

impl<'p> EnumField<'p> {
    pub fn offset(self) -> usize {
        self.offset
    }

    pub fn is_writable(self) -> bool {
        self.writable
    }

    pub fn declared(self) -> &'p PackEnum {
        self.declared
    }

    pub fn accepts(self, value: i64) -> bool {
        self.declared.name_of(value).is_some()
    }
}

fn describe(repr: &FieldRepr) -> String {
    use crate::desc::repr::Pointee;

    let pointee = |p: &Pointee| match p {
        Pointee::One { type_name } => format!("one {type_name}"),
        Pointee::Many {
            type_name: Some(t), ..
        } => format!("many {t}"),
        Pointee::Many { .. } => "many values".into(),
        Pointee::ManyPtr {
            type_name: Some(t), ..
        } => format!("a table of pointers to {t}"),
        Pointee::ManyPtr { .. } => "a table of pointers".into(),
        Pointee::Bytes { .. } => "bytes".into(),
        Pointee::Str => "a string".into(),
        Pointee::Strings { .. } => "a table of pointers to strings".into(),
        Pointee::Opaque => "something undescribed".into(),
        Pointee::Member { type_name } => format!("a {type_name} inside its owner"),
        Pointee::Members { type_name, .. } => {
            format!("a table of pointers to {type_name}s inside its owner")
        }
    };

    match repr {
        FieldRepr::Scalar(Scalar::Int(w)) => format!("{w:?}"),
        FieldRepr::Scalar(Scalar::F32) => "f32".into(),
        FieldRepr::Scalar(Scalar::F64) => "f64".into(),
        FieldRepr::Scalar(Scalar::Bool) => "a bool".into(),
        FieldRepr::ScalarArray { element, count } => format!("[{element:?}; {count}]"),
        FieldRepr::Inline { type_name } => format!("an embedded {type_name}"),
        FieldRepr::InlineArray { type_name, count } => format!("[{type_name}; {count}]"),
        FieldRepr::Ptr(p) => format!("a pointer to {}", pointee(p)),
        FieldRepr::AtArray(p) => format!("an at-array of {}", pointee(p)),
        FieldRepr::PtrSlots { pointee: p, count } => {
            format!("{count} pointer slots to {}", pointee(p))
        }
        FieldRepr::Enum { width, .. } => format!("an enum stored as {width:?}"),
        FieldRepr::Padding { size } => format!("{size} bytes of padding"),
        FieldRepr::Opaque { size } => format!("{size} opaque bytes"),
        FieldRepr::Trailing {
            type_name: Some(t), ..
        } => format!("a trailing run of {t}"),
        FieldRepr::Trailing { element_size, .. } => format!(
            "a trailing run of {}-byte elements",
            element_size.unwrap_or(1)
        ),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Cursor<'d> {
    bytes: &'d [u8],
    base: usize,
}

impl<'d> Cursor<'d> {
    pub fn new(bytes: &'d [u8], base: usize) -> Self {
        Self { bytes, base }
    }

    pub fn get<T: FromField>(self, field: Field<T>) -> Option<T> {
        T::read(
            self.bytes,
            self.base.checked_add(field.offset)?,
            field.width,
        )
    }

    pub fn at(self, index: usize, stride: usize) -> Option<Self> {
        Some(Self {
            bytes: self.bytes,
            base: self.base.checked_add(index.checked_mul(stride)?)?,
        })
    }

    pub fn offset(self, delta: usize) -> Self {
        Self {
            bytes: self.bytes,
            base: self.base.saturating_add(delta),
        }
    }

    pub fn base(self) -> usize {
        self.base
    }

    pub fn bytes(self) -> &'d [u8] {
        self.bytes
    }
}

#[derive(Debug)]
pub struct CursorMut<'d> {
    bytes: &'d mut [u8],
    base: usize,
}

impl<'d> CursorMut<'d> {
    pub fn new(bytes: &'d mut [u8], base: usize) -> Self {
        Self { bytes, base }
    }

    pub fn set<T: ToField>(&mut self, field: Field<T>, value: T) -> Result<(), WriteError> {
        if !field.writable {
            return Err(WriteError::Structural);
        }
        let size = self.bytes.len();
        let offset = self
            .base
            .checked_add(field.offset)
            .ok_or(WriteError::OutOfBounds {
                offset: field.offset,
                size,
            })?;
        T::write(self.bytes, offset, field.width, value)
    }

    pub fn set_enum(&mut self, field: EnumField<'_>, value: i64) -> Result<(), WriteError> {
        if !field.writable {
            return Err(WriteError::Structural);
        }
        if !field.accepts(value) {
            return Err(WriteError::NotADeclaredValue { value });
        }
        if !field.width.holds(i128::from(value)) {
            return Err(WriteError::DoesNotFit { width: field.width });
        }

        let size = self.bytes.len();
        let offset = self
            .base
            .checked_add(field.offset)
            .ok_or(WriteError::OutOfBounds {
                offset: field.offset,
                size,
            })?;

        if field.width.write(self.bytes, offset, i128::from(value)) {
            Ok(())
        } else {
            Err(WriteError::OutOfBounds { offset, size })
        }
    }

    pub fn as_cursor(&self) -> Cursor<'_> {
        Cursor::new(self.bytes, self.base)
    }

    pub fn at(self, index: usize, stride: usize) -> Option<Self> {
        let base = self.base.checked_add(index.checked_mul(stride)?)?;
        Some(Self {
            bytes: self.bytes,
            base,
        })
    }

    pub fn offset(self, delta: usize) -> Self {
        let base = self.base.saturating_add(delta);
        Self {
            bytes: self.bytes,
            base,
        }
    }

    pub fn base(&self) -> usize {
        self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod geometry {
        use crate::desc::{FieldName, TypeName};
        pub const TYPE: TypeName = TypeName("Geometry");
        pub const VERTICES_COUNT: FieldName = FieldName("vertices_count");
        pub const INDICES_COUNT: FieldName = FieldName("indices_count");
        pub const MATRIX_PALETTE: FieldName = FieldName("matrix_palette");
        pub const PRIMITIVE_TYPE: FieldName = FieldName("primitive_type");
    }
    use crate::desc::pack::{PackField, PackType};

    fn pack_with(fields: Vec<PackField>) -> Pack {
        let mut pack = Pack::new("testtitle", "testtarget");
        pack.types.push(PackType {
            name: geometry::TYPE.as_str().into(),
            size: 0x20,
            fields,
            selector: None,
        });
        pack
    }

    fn int_field(name: &str, offset: usize, width: IntWidth) -> PackField {
        PackField {
            name: name.into(),
            offset,
            repr: FieldRepr::Scalar(Scalar::Int(width)),
            space: None,
            bits: Vec::new(),
            note: None,
        }
    }

    #[test]
    fn an_arrays_count_and_capacity_siblings_are_both_nameable() {
        let pack = pack_with(vec![
            PackField {
                name: "matrix_palette".into(),
                offset: 0x00,
                repr: FieldRepr::Ptr(crate::desc::Pointee::Many {
                    type_name: None,
                    count: crate::desc::CountSource::CountAndCapacity {
                        count: "matrix_count".into(),
                        capacity: "matrix_capacity".into(),
                    },
                    element_size: Some(2),
                }),
                space: Some(crate::desc::AddressSpace::Virtual),
                bits: Vec::new(),
                note: None,
            },
            int_field("matrix_count", 0x08, IntWidth::U16),
        ]);

        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        assert_eq!(
            view.count_field_of(geometry::MATRIX_PALETTE),
            Some("matrix_count")
        );
        assert_eq!(
            view.capacity_field_of(geometry::MATRIX_PALETTE),
            Some("matrix_capacity")
        );
    }

    #[test]
    fn an_array_with_no_capacity_recorded_says_so() {
        let pack = pack_with(vec![
            PackField {
                name: "matrix_palette".into(),
                offset: 0x00,
                repr: FieldRepr::Ptr(crate::desc::Pointee::Many {
                    type_name: None,
                    count: crate::desc::CountSource::Field("matrix_count".into()),
                    element_size: Some(2),
                }),
                space: Some(crate::desc::AddressSpace::Virtual),
                bits: Vec::new(),
                note: None,
            },
            int_field("matrix_count", 0x08, IntWidth::U16),
        ]);

        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        assert_eq!(
            view.count_field_of(geometry::MATRIX_PALETTE),
            Some("matrix_count")
        );
        assert_eq!(view.capacity_field_of(geometry::MATRIX_PALETTE), None);
    }

    #[test]
    fn a_bound_field_reads_at_a_plain_offset() {
        let pack = pack_with(vec![
            int_field("vertices_count", 0x00, IntWidth::U16),
            int_field("indices_count", 0x04, IntWidth::U32),
        ]);

        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        let vertex_count = view
            .field::<u16>(geometry::VERTICES_COUNT)
            .expect("bind field");
        let index_count = view
            .field::<u32>(geometry::INDICES_COUNT)
            .expect("bind field");

        assert_eq!(vertex_count.offset(), 0x00);
        assert_eq!(index_count.offset(), 0x04);

        let mut bytes = vec![0u8; 0x20];
        bytes[0x00..0x02].copy_from_slice(&1204u16.to_le_bytes());
        bytes[0x04..0x08].copy_from_slice(&3612u32.to_le_bytes());

        let cursor = Cursor::new(&bytes, 0);
        assert_eq!(cursor.get(vertex_count), Some(1204));
        assert_eq!(cursor.get(index_count), Some(3612));
    }

    #[test]
    fn a_missing_field_names_the_pack_and_the_field() {
        let pack = pack_with(vec![int_field("vertices_count", 0, IntWidth::U16)]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");

        let err = view
            .field::<u32>(geometry::INDICES_COUNT)
            .expect_err("should not bind");

        assert_eq!(
            err,
            BindError::NoField {
                pack: "testtitle/testtarget".into(),
                type_name: "Geometry".into(),
                field_name: "indices_count".into(),
            }
        );
        let message = err.to_string();
        assert!(message.contains("Geometry"));
        assert!(message.contains("indices_count"));
    }

    #[test]
    fn a_missing_type_is_distinct_from_a_missing_field() {
        let pack = Pack::new("testtitle", "testtarget");
        let err = View::of(&pack, geometry::TYPE).expect_err("should not bind");
        assert!(matches!(err, BindError::NoType { .. }));
    }

    #[test]
    fn reading_a_field_as_the_wrong_type_fails_at_bind_not_at_read() {
        let pack = pack_with(vec![PackField {
            name: "vertices_count".into(),
            offset: 0,
            repr: FieldRepr::Ptr(crate::desc::repr::Pointee::Str),
            space: Some(crate::desc::AddressSpace::Virtual),
            bits: Vec::new(),
            note: None,
        }]);

        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        let err = view
            .field::<u16>(geometry::VERTICES_COUNT)
            .expect_err("should not bind");

        assert!(matches!(err, BindError::WrongRepr(_)));
        assert!(err.to_string().contains("a string"));
    }

    #[test]
    fn a_value_that_does_not_fit_the_requested_type_is_none() {
        let pack = pack_with(vec![int_field("vertices_count", 0, IntWidth::U32)]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        let narrow = view.field::<u16>(geometry::VERTICES_COUNT).expect("bind");

        let mut bytes = vec![0u8; 0x20];
        bytes[0..4].copy_from_slice(&u32::MAX.to_le_bytes());

        assert_eq!(Cursor::new(&bytes, 0).get(narrow), None);
    }

    #[test]
    fn a_read_past_the_end_of_a_truncated_file_is_none() {
        let pack = pack_with(vec![int_field("vertices_count", 0x18, IntWidth::U64)]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        let field = view.field::<u64>(geometry::VERTICES_COUNT).expect("bind");

        let bytes = vec![0u8; 0x10];
        assert_eq!(Cursor::new(&bytes, 0).get(field), None);
    }

    #[test]
    fn an_optional_field_distinguishes_absent_from_wrong() {
        let pack = pack_with(vec![int_field("vertices_count", 0, IntWidth::U16)]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");

        assert!(view
            .optional_field::<u32>(geometry::INDICES_COUNT)
            .expect("absent is not an error")
            .is_none());

        assert!(view
            .optional_field::<u16>(geometry::VERTICES_COUNT)
            .expect("present")
            .is_some());
    }

    #[test]
    fn an_enum_can_be_read_as_its_underlying_integer() {
        let pack = pack_with(vec![PackField {
            name: "primitive_type".into(),
            offset: 0,
            repr: FieldRepr::Enum {
                type_name: "GrmPrimitiveType".into(),
                width: IntWidth::U8,
            },
            space: None,
            bits: Vec::new(),
            note: None,
        }]);

        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        let kind = view.field::<u8>(geometry::PRIMITIVE_TYPE).expect("bind");

        let bytes = vec![4u8; 0x20];
        assert_eq!(Cursor::new(&bytes, 0).get(kind), Some(4));
    }

    #[test]
    fn a_bool_reads_from_either_spelling_of_a_flag() {
        for repr in [
            FieldRepr::Scalar(Scalar::Bool),
            FieldRepr::Scalar(Scalar::Int(IntWidth::U8)),
        ] {
            let pack = pack_with(vec![PackField {
                name: "vertices_count".into(),
                offset: 0,
                repr,
                space: None,
                bits: Vec::new(),
                note: None,
            }]);
            let view = View::of(&pack, geometry::TYPE).expect("bind type");
            let flag = view.field::<bool>(geometry::VERTICES_COUNT).expect("bind");

            let mut bytes = vec![0u8; 0x20];
            assert_eq!(Cursor::new(&bytes, 0).get(flag), Some(false));
            bytes[0] = 7;
            assert_eq!(Cursor::new(&bytes, 0).get(flag), Some(true));
        }
    }

    #[test]
    fn a_wider_integer_does_not_read_as_a_bool() {
        let pack = pack_with(vec![int_field("vertices_count", 0, IntWidth::U32)]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        assert!(view.field::<bool>(geometry::VERTICES_COUNT).is_err());
    }

    #[test]
    fn an_f64_reads_eight_bytes() {
        let pack = pack_with(vec![PackField {
            name: "vertices_count".into(),
            offset: 0x08,
            repr: FieldRepr::Scalar(Scalar::F64),
            space: None,
            bits: Vec::new(),
            note: None,
        }]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        let field = view.field::<f64>(geometry::VERTICES_COUNT).expect("bind");

        let mut bytes = vec![0u8; 0x20];
        bytes[0x08..0x10].copy_from_slice(&(-1.5f64).to_le_bytes());
        assert_eq!(Cursor::new(&bytes, 0).get(field), Some(-1.5));
    }

    fn tagged_pack() -> Pack {
        use crate::desc::pack::PackSelector;

        let mut pack = Pack::new("testtitle", "testtarget");
        pack.types.push(PackType {
            name: "PhBound".into(),
            size: 0x70,
            fields: vec![int_field("bound_type", 0x10, IntWidth::U8)],
            selector: Some(PackSelector {
                tag: "bound_type".into(),
                tag_bits: None,
                variants: vec![(0, "PhBoundSphere".into()), (4, "PhBoundGeometry".into())],
            }),
        });
        pack.types.push(PackType {
            name: "PhBoundSphere".into(),
            size: 0x70,
            fields: vec![],
            selector: None,
        });
        pack.types.push(PackType {
            name: "PhBoundGeometry".into(),
            size: 0x180,
            fields: vec![],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_tagged_base_names_the_layout_its_own_bytes_select() {
        let pack = tagged_pack();
        let view = View::of(&pack, TypeName("PhBound")).expect("bind base");
        assert!(view.is_tagged());

        let mut bytes = vec![0u8; 0x70];
        bytes[0x10] = 4;
        assert_eq!(
            view.concrete_type(Cursor::new(&bytes, 0)),
            Some("PhBoundGeometry")
        );

        bytes[0x10] = 0;
        assert_eq!(
            view.concrete_type(Cursor::new(&bytes, 0)),
            Some("PhBoundSphere")
        );
    }

    #[test]
    fn a_tag_value_nobody_described_is_none_rather_than_a_guess() {
        let pack = tagged_pack();
        let view = View::of(&pack, TypeName("PhBound")).expect("bind base");

        let mut bytes = vec![0u8; 0x70];
        bytes[0x10] = 9;
        assert_eq!(view.concrete_type(Cursor::new(&bytes, 0)), None);
    }

    #[test]
    fn an_ordinary_type_is_not_tagged() {
        let pack = pack_with(vec![int_field("vertices_count", 0, IntWidth::U16)]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        assert!(!view.is_tagged());
        assert_eq!(view.concrete_type(Cursor::new(&[0u8; 0x20], 0)), None);
    }

    #[test]
    fn a_cursor_can_walk_an_array_by_stride() {
        let pack = pack_with(vec![int_field("vertices_count", 0, IntWidth::U16)]);
        let view = View::of(&pack, geometry::TYPE).expect("bind type");
        let field = view.field::<u16>(geometry::VERTICES_COUNT).expect("bind");

        let mut bytes = vec![0u8; 0x60];
        for i in 0..3usize {
            let at = i * view.size();
            bytes[at..at + 2].copy_from_slice(&((i as u16 + 1) * 10).to_le_bytes());
        }

        let base = Cursor::new(&bytes, 0);
        for i in 0..3usize {
            let element = base.at(i, view.size()).expect("in range");
            assert_eq!(element.get(field), Some((i as u16 + 1) * 10));
        }
    }
}
