//! A decompressed resource read through its rebased pointers: `0x50000000` and up
//! is system memory, `0x60000000` and up is graphics memory, which follows the
//! system half in the image.

use crate::constants::{FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE, NULL_PTR};

use crate::desc::repr::{Scalar, POINTER_SIZE};
use crate::desc::value::ScalarValue;
use crate::desc::view::{Cursor, CursorMut, WriteError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagingModel {
    // Where system memory starts. RAGE: `0x50000000`.
    pub virtual_base: u64,
    // Where graphics memory starts. RAGE: `0x60000000`.
    pub physical_base: u64,
    pub offset_mask: u64,
}

impl PagingModel {
    pub const RAGE: Self = Self {
        virtual_base: FIXED_VIRTUAL_BASE,
        physical_base: FIXED_PHYSICAL_BASE,
        offset_mask: 0x0FFF_FFFF,
    };

    #[inline]
    pub const fn is_virtual(self, addr: u64) -> bool {
        addr & !self.offset_mask == self.virtual_base
    }

    #[inline]
    pub const fn is_physical(self, addr: u64) -> bool {
        addr & !self.offset_mask == self.physical_base
    }

    #[inline]
    pub const fn page_offset(self, addr: u64) -> usize {
        (addr & self.offset_mask) as usize
    }

    #[inline]
    pub const fn space_nibble(self, addr: u64) -> u8 {
        (addr >> 28) as u8
    }
}

impl Default for PagingModel {
    fn default() -> Self {
        Self::RAGE
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Image<'d> {
    bytes: &'d [u8],
    virtual_size: usize,
    paging: PagingModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolvable {
    Null,
    NotAnAddress { base: u8 },
    OutOfBounds { offset: usize, size: usize },
}

impl std::fmt::Display for Unresolvable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("null pointer"),
            Self::NotAnAddress { base } => {
                write!(f, "{base:#x} is neither a virtual nor a physical page base")
            }
            Self::OutOfBounds { offset, size } => {
                write!(
                    f,
                    "offset {offset:#x} is past the resource's {size:#x} bytes"
                )
            }
        }
    }
}

fn resolve_in(
    addr: u64,
    virtual_size: usize,
    len: usize,
    paging: PagingModel,
) -> Result<usize, Unresolvable> {
    if addr == NULL_PTR {
        return Err(Unresolvable::Null);
    }

    let offset = if paging.is_virtual(addr) {
        paging.page_offset(addr)
    } else if paging.is_physical(addr) {
        paging
            .page_offset(addr)
            .checked_add(virtual_size)
            .ok_or(Unresolvable::OutOfBounds {
                offset: usize::MAX,
                size: len,
            })?
    } else {
        return Err(Unresolvable::NotAnAddress {
            base: paging.space_nibble(addr),
        });
    };

    if offset >= len {
        return Err(Unresolvable::OutOfBounds { offset, size: len });
    }

    Ok(offset)
}

impl<'d> Image<'d> {
    /// `virtual_size` is the system-memory half's length, from the RSC7 header.
    pub fn new(bytes: &'d [u8], virtual_size: usize) -> Self {
        Self {
            bytes,
            virtual_size,
            paging: PagingModel::RAGE,
        }
    }

    pub fn with_paging(mut self, paging: PagingModel) -> Self {
        self.paging = paging;
        self
    }

    pub fn paging(self) -> PagingModel {
        self.paging
    }

    pub fn bytes(self) -> &'d [u8] {
        self.bytes
    }

    pub fn virtual_size(self) -> usize {
        self.virtual_size
    }

    pub fn len(self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(self) -> bool {
        self.bytes.is_empty()
    }

    pub fn resolve(self, addr: u64) -> Result<usize, Unresolvable> {
        resolve_in(addr, self.virtual_size, self.bytes.len(), self.paging)
    }

    pub fn root(self) -> Cursor<'d> {
        Cursor::new(self.bytes, 0)
    }

    pub fn at(self, offset: usize) -> Cursor<'d> {
        Cursor::new(self.bytes, offset)
    }

    pub fn follow(self, addr: u64) -> Result<Cursor<'d>, Unresolvable> {
        Ok(Cursor::new(self.bytes, self.resolve(addr)?))
    }

    pub fn follow_at(self, offset: usize) -> Result<Cursor<'d>, Unresolvable> {
        let addr = self.pointer_at(offset)?;
        self.follow(addr)
    }

    pub fn at_array(self, offset: usize) -> Result<Option<Elements<'d>>, Unresolvable> {
        let addr = self.pointer_at(offset)?;
        let count = usize::from(self.u16_at(offset + POINTER_SIZE)?);

        if addr == NULL_PTR || count == 0 {
            return Ok(None);
        }

        Ok(Some(Elements {
            first: self.follow(addr)?,
            count,
        }))
    }

    pub fn ptr_array(self, offset: usize) -> Result<Option<Slots<'d>>, Unresolvable> {
        Ok(self.at_array(offset)?.map(|elements| Slots {
            image: self,
            elements,
        }))
    }

    pub fn string_at(self, offset: usize) -> Result<String, Unresolvable> {
        Ok(String::from_utf8_lossy(self.c_str_at(offset)?).into_owned())
    }

    /// The bytes of the NUL-terminated string at `offset`, without the NUL.
    pub fn c_str_at(self, offset: usize) -> Result<&'d [u8], Unresolvable> {
        let out_of_bounds = Unresolvable::OutOfBounds {
            offset,
            size: self.bytes.len(),
        };
        let tail = self.bytes.get(offset..).ok_or(out_of_bounds)?;
        let end = tail.iter().position(|&b| b == 0).ok_or(out_of_bounds)?;
        Ok(&tail[..end])
    }

    pub fn string_via(self, offset: usize) -> Result<String, Unresolvable> {
        let target = self.follow_at(offset)?;
        self.string_at(target.base())
    }

    pub fn slice_at(self, offset: usize, len: usize) -> Result<&'d [u8], Unresolvable> {
        self.bytes
            .get(offset..offset.saturating_add(len))
            .ok_or(Unresolvable::OutOfBounds {
                offset,
                size: self.bytes.len(),
            })
    }

    pub fn pointer_at(self, offset: usize) -> Result<u64, Unresolvable> {
        let slice = self
            .bytes
            .get(offset..offset.saturating_add(POINTER_SIZE))
            .ok_or(Unresolvable::OutOfBounds {
                offset,
                size: self.bytes.len(),
            })?;

        let mut buf = [0u8; POINTER_SIZE];
        buf.copy_from_slice(slice);
        Ok(u64::from_le_bytes(buf))
    }

    pub fn u16_at(self, offset: usize) -> Result<u16, Unresolvable> {
        let slice =
            self.bytes
                .get(offset..offset.saturating_add(2))
                .ok_or(Unresolvable::OutOfBounds {
                    offset,
                    size: self.bytes.len(),
                })?;
        Ok(u16::from_le_bytes([slice[0], slice[1]]))
    }
}

/// An image whose scalars are patched in place: nothing moves, so the page
/// table the RSC7 header describes stays true.
#[derive(Debug)]
pub struct ImageMut<'d> {
    bytes: &'d mut [u8],
    virtual_size: usize,
    paging: PagingModel,
}

impl<'d> ImageMut<'d> {
    pub fn new(bytes: &'d mut [u8], virtual_size: usize) -> Self {
        Self {
            bytes,
            virtual_size,
            paging: PagingModel::RAGE,
        }
    }

    pub fn with_paging(mut self, paging: PagingModel) -> Self {
        self.paging = paging;
        self
    }

    pub fn as_image(&self) -> Image<'_> {
        Image::new(self.bytes, self.virtual_size).with_paging(self.paging)
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn resolve(&self, addr: u64) -> Result<usize, Unresolvable> {
        resolve_in(addr, self.virtual_size, self.bytes.len(), self.paging)
    }

    pub fn root(&mut self) -> CursorMut<'_> {
        CursorMut::new(self.bytes, 0)
    }

    pub fn at(&mut self, offset: usize) -> CursorMut<'_> {
        CursorMut::new(self.bytes, offset)
    }

    pub fn follow(&mut self, addr: u64) -> Result<CursorMut<'_>, Unresolvable> {
        let offset = self.resolve(addr)?;
        Ok(CursorMut::new(self.bytes, offset))
    }

    pub fn follow_at(&mut self, offset: usize) -> Result<CursorMut<'_>, Unresolvable> {
        let addr = self.as_image().pointer_at(offset)?;
        self.follow(addr)
    }

    /// ⚠ An integer too wide for its field is refused, never truncated.
    pub fn put_scalar(
        &mut self,
        offset: usize,
        scalar: Scalar,
        value: ScalarValue,
    ) -> Result<(), WriteError> {
        let size = self.bytes.len();
        let out_of_bounds = || WriteError::OutOfBounds {
            offset,
            size: scalar.size(),
        };
        let end = offset
            .checked_add(scalar.size())
            .ok_or_else(out_of_bounds)?;
        if end > size {
            return Err(out_of_bounds());
        }

        match (scalar, value) {
            (Scalar::Int(width), ScalarValue::Int(v)) => {
                if !width.write(self.bytes, offset, v) {
                    return Err(WriteError::DoesNotFit { width });
                }
            }
            (Scalar::Bool, ScalarValue::Bool(v)) => self.bytes[offset] = u8::from(v),
            (Scalar::F32, ScalarValue::F32(v)) => {
                self.bytes[offset..end].copy_from_slice(&v.to_le_bytes())
            }
            (Scalar::F64, ScalarValue::F64(v)) => {
                self.bytes[offset..end].copy_from_slice(&v.to_le_bytes())
            }
            _ => return Err(WriteError::WrongKind),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Elements<'d> {
    first: Cursor<'d>,
    count: usize,
}

impl<'d> Elements<'d> {
    pub fn count(self) -> usize {
        self.count
    }

    pub fn first(self) -> Cursor<'d> {
        self.first
    }

    pub fn at(self, index: usize, stride: usize) -> Option<Cursor<'d>> {
        if index >= self.count {
            return None;
        }
        self.first.at(index, stride)
    }

    pub fn iter(self, stride: usize) -> impl Iterator<Item = Cursor<'d>> {
        (0..self.count).filter_map(move |i| self.at(i, stride))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Slots<'d> {
    image: Image<'d>,
    elements: Elements<'d>,
}

impl<'d> Slots<'d> {
    pub fn count(self) -> usize {
        self.elements.count()
    }

    pub fn follow(self, index: usize) -> Option<Cursor<'d>> {
        let slot = self.elements.at(index, POINTER_SIZE)?;
        self.image.follow_at(slot.base()).ok()
    }

    pub fn iter(self) -> impl Iterator<Item = Cursor<'d>> {
        (0..self.count()).filter_map(move |i| self.follow(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(len: usize, virtual_size: usize) -> Vec<u8> {
        let _ = virtual_size;
        vec![0u8; len]
    }

    #[test]
    fn a_virtual_address_is_its_own_offset() {
        let bytes = image(0x1000, 0x800);
        let img = Image::new(&bytes, 0x800);
        assert_eq!(img.resolve(0x50000000), Ok(0));
        assert_eq!(img.resolve(0x50000123), Ok(0x123));
    }

    #[test]
    fn a_physical_address_starts_after_the_system_half() {
        let bytes = image(0x1000, 0x800);
        let img = Image::new(&bytes, 0x800);
        assert_eq!(img.resolve(0x60000000), Ok(0x800));
        assert_eq!(img.resolve(0x60000010), Ok(0x810));
    }

    #[test]
    fn null_is_reported_apart_from_a_fault() {
        let bytes = image(0x100, 0x80);
        assert_eq!(Image::new(&bytes, 0x80).resolve(0), Err(Unresolvable::Null));
    }

    #[test]
    fn an_address_in_neither_space_names_the_base_it_found() {
        let bytes = image(0x100, 0x80);
        let err = Image::new(&bytes, 0x80).resolve(0x30000000).unwrap_err();
        assert_eq!(err, Unresolvable::NotAnAddress { base: 3 });
        assert!(err.to_string().contains("0x3"));
    }

    #[test]
    fn resolving_past_the_end_is_an_error_not_a_panic() {
        let bytes = image(0x100, 0x80);
        let img = Image::new(&bytes, 0x80);
        assert!(matches!(
            img.resolve(0x50001000),
            Err(Unresolvable::OutOfBounds { .. })
        ));
        assert!(matches!(
            img.resolve(0x60000FFF),
            Err(Unresolvable::OutOfBounds { .. })
        ));
    }

    #[test]
    fn following_a_stored_pointer_lands_where_it_points() {
        let mut bytes = vec![0u8; 0x200];
        bytes[0x10..0x18].copy_from_slice(&0x5000_0080u64.to_le_bytes());
        bytes[0x80] = 0xAB;

        let img = Image::new(&bytes, 0x100);
        let cursor = img.follow_at(0x10).expect("follow");
        assert_eq!(cursor.base(), 0x80);
        assert_eq!(cursor.bytes()[cursor.base()], 0xAB);
    }

    #[test]
    fn following_a_null_pointer_reports_null_rather_than_reading_zero() {
        let bytes = vec![0u8; 0x100];
        let img = Image::new(&bytes, 0x80);
        assert_eq!(img.follow_at(0x10).err(), Some(Unresolvable::Null));
    }

    fn write_at_array(bytes: &mut [u8], at: usize, elements_at: usize, count: u16) {
        let addr = 0x5000_0000u64 | elements_at as u64;
        bytes[at..at + 8].copy_from_slice(&addr.to_le_bytes());
        bytes[at + 8..at + 10].copy_from_slice(&count.to_le_bytes());
        bytes[at + 10..at + 12].copy_from_slice(&count.to_le_bytes());
    }

    #[test]
    fn an_at_array_carries_its_own_count() {
        let mut bytes = vec![0u8; 0x200];
        write_at_array(&mut bytes, 0x10, 0x80, 3);
        for i in 0..3usize {
            bytes[0x80 + i * 4] = (i as u8 + 1) * 10;
        }

        let img = Image::new(&bytes, 0x100);
        let elements = img.at_array(0x10).expect("read").expect("populated");

        assert_eq!(elements.count(), 3);
        let values: Vec<u8> = elements.iter(4).map(|c| c.bytes()[c.base()]).collect();
        assert_eq!(values, vec![10, 20, 30]);
    }

    #[test]
    fn an_empty_at_array_is_none_rather_than_an_error() {
        let bytes = vec![0u8; 0x100];
        let img = Image::new(&bytes, 0x80);
        assert!(img.at_array(0x10).expect("read").is_none());
    }

    #[test]
    fn an_at_array_with_a_pointer_but_no_elements_is_also_none() {
        let mut bytes = vec![0u8; 0x200];
        write_at_array(&mut bytes, 0x10, 0x80, 0);
        let img = Image::new(&bytes, 0x100);
        assert!(img.at_array(0x10).expect("read").is_none());
    }

    #[test]
    fn reading_past_an_at_arrays_count_is_none_not_a_wild_read() {
        let mut bytes = vec![0u8; 0x200];
        write_at_array(&mut bytes, 0x10, 0x80, 2);
        let img = Image::new(&bytes, 0x100);
        let elements = img.at_array(0x10).expect("read").expect("populated");

        assert!(elements.at(1, 4).is_some());
        assert!(elements.at(2, 4).is_none());
    }

    #[test]
    fn a_pointer_array_takes_two_hops_to_a_value() {
        let mut bytes = vec![0u8; 0x300];
        write_at_array(&mut bytes, 0x10, 0x80, 2);
        bytes[0x80..0x88].copy_from_slice(&0x5000_0100u64.to_le_bytes());
        bytes[0x88..0x90].copy_from_slice(&0x5000_0140u64.to_le_bytes());
        bytes[0x100] = 0xAA;
        bytes[0x140] = 0xBB;

        let img = Image::new(&bytes, 0x200);
        let slots = img.ptr_array(0x10).expect("read").expect("populated");

        assert_eq!(slots.count(), 2);
        let values: Vec<u8> = slots.iter().map(|c| c.bytes()[c.base()]).collect();
        assert_eq!(values, vec![0xAA, 0xBB]);
    }

    #[test]
    fn a_null_slot_is_skipped_rather_than_reported() {
        let mut bytes = vec![0u8; 0x300];
        write_at_array(&mut bytes, 0x10, 0x80, 3);
        bytes[0x80..0x88].copy_from_slice(&0x5000_0100u64.to_le_bytes());
        bytes[0x100] = 0xAA;

        let img = Image::new(&bytes, 0x200);
        let slots = img.ptr_array(0x10).expect("read").expect("populated");

        assert_eq!(slots.count(), 3, "the count is what the file says");
        assert_eq!(slots.iter().count(), 1, "but only one slot leads anywhere");
    }

    #[test]
    fn a_string_stops_at_the_terminator() {
        let mut bytes = vec![0u8; 0x100];
        bytes[0x20..0x27].copy_from_slice(b"prop_v\0");
        let img = Image::new(&bytes, 0x80);
        assert_eq!(img.string_at(0x20).expect("read"), "prop_v");
    }

    #[test]
    fn an_unterminated_string_is_an_error_not_the_rest_of_the_file() {
        let bytes = vec![b'a'; 0x40];
        let img = Image::new(&bytes, 0x20);
        assert!(img.string_at(0x10).is_err());
    }

    #[test]
    fn a_string_can_be_reached_through_its_pointer() {
        let mut bytes = vec![0u8; 0x100];
        bytes[0x10..0x18].copy_from_slice(&0x5000_0040u64.to_le_bytes());
        bytes[0x40..0x45].copy_from_slice(b"lod0\0");
        let img = Image::new(&bytes, 0x80);
        assert_eq!(img.string_via(0x10).expect("read"), "lod0");
    }

    #[test]
    fn a_string_with_a_stray_byte_still_reads() {
        let mut bytes = vec![0u8; 0x100];
        bytes[0x20..0x25].copy_from_slice(&[b'a', 0xFF, b'b', 0, 0]);
        let img = Image::new(&bytes, 0x80);
        let read = img.string_at(0x20).expect("read");
        assert!(read.starts_with('a') && read.ends_with('b'));
    }

    #[test]
    fn a_slice_past_the_end_is_an_error() {
        let bytes = vec![0u8; 0x100];
        let img = Image::new(&bytes, 0x80);
        assert!(img.slice_at(0xF0, 0x10).is_ok());
        assert!(img.slice_at(0xF0, 0x20).is_err());
    }

    use crate::desc::name::{FieldName, TypeName};
    use crate::desc::pack::{Pack, PackField, PackType};
    use crate::desc::repr::{FieldRepr, IntWidth, Scalar};
    use crate::desc::view::View;

    fn scalar_pack() -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Probe".into(),
            size: 0x10,
            fields: vec![
                PackField {
                    name: "count".into(),
                    offset: 4,
                    repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U32)),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "distance".into(),
                    offset: 8,
                    repr: FieldRepr::Scalar(Scalar::F32),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "small".into(),
                    offset: 12,
                    repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
            ],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_scalar_written_through_a_pack_reads_back_through_the_same_pack() {
        let pack = scalar_pack();
        let view = View::of(&pack, TypeName("Probe")).expect("bind");
        let count = view.field::<u32>(FieldName("count")).expect("bind count");
        let distance = view
            .field::<f32>(FieldName("distance"))
            .expect("bind distance");

        let mut bytes = vec![0u8; 0x40];
        let mut img = ImageMut::new(&mut bytes, 0x20);
        let mut cursor = img.at(0x10);
        cursor.set(count, 7).expect("write count");
        cursor.set(distance, 42.5).expect("write distance");

        let read = img.as_image();
        assert_eq!(read.at(0x10).get(count), Some(7));
        assert_eq!(read.at(0x10).get(distance), Some(42.5));
    }

    #[test]
    fn a_write_touches_the_field_and_not_one_byte_more() {
        let pack = scalar_pack();
        let view = View::of(&pack, TypeName("Probe")).expect("bind");
        let count = view.field::<u32>(FieldName("count")).expect("bind");

        let mut bytes: Vec<u8> = (0..0x40u8).collect();
        let before = bytes.clone();
        {
            let mut img = ImageMut::new(&mut bytes, 0x20);
            img.at(0x10).set(count, 0xDEAD_BEEF).expect("write");
        }

        let field = 0x14..0x18;
        assert_eq!(&bytes[..field.start], &before[..field.start]);
        assert_eq!(&bytes[field.end..], &before[field.end..]);
        assert_eq!(&bytes[field], &0xDEAD_BEEFu32.to_le_bytes());
    }

    #[test]
    fn a_value_too_wide_for_its_field_is_refused_rather_than_wrapped() {
        let pack = scalar_pack();
        let view = View::of(&pack, TypeName("Probe")).expect("bind");
        let small = view.field::<u32>(FieldName("small")).expect("bind");

        let mut bytes = vec![0u8; 0x40];
        let mut img = ImageMut::new(&mut bytes, 0x20);
        let err = img.at(0x10).set(small, 70_000).expect_err("should refuse");
        assert!(matches!(err, crate::desc::WriteError::DoesNotFit { .. }));
        assert_eq!(bytes[0x1C..0x1E], [0, 0], "and wrote nothing");
    }

    #[test]
    fn a_write_past_the_end_is_an_error_rather_than_a_panic() {
        let pack = scalar_pack();
        let view = View::of(&pack, TypeName("Probe")).expect("bind");
        let count = view.field::<u32>(FieldName("count")).expect("bind");

        let mut bytes = vec![0u8; 0x10];
        let mut img = ImageMut::new(&mut bytes, 0x8);
        let err = img.at(0x20).set(count, 1).expect_err("should refuse");
        assert!(matches!(err, crate::desc::WriteError::OutOfBounds { .. }));
    }

    #[test]
    fn a_writable_cursor_can_be_reached_through_a_pointer() {
        let pack = scalar_pack();
        let view = View::of(&pack, TypeName("Probe")).expect("bind");
        let count = view.field::<u32>(FieldName("count")).expect("bind");

        let mut bytes = vec![0u8; 0x200];
        bytes[0x10..0x18].copy_from_slice(&0x5000_0080u64.to_le_bytes());

        let mut img = ImageMut::new(&mut bytes, 0x100);
        let mut cursor = img.follow_at(0x10).expect("follow");
        assert_eq!(cursor.base(), 0x80);
        cursor.set(count, 99).expect("write");

        assert_eq!(img.as_image().at(0x80).get(count), Some(99));
    }

    use crate::desc::pack::{PackEnum, PackSelector, PackVariant};
    use crate::desc::repr::{CountSource, Pointee};

    fn only_touched(bytes: &[u8], before: &[u8], field: std::ops::Range<usize>) {
        assert_eq!(
            &bytes[..field.start],
            &before[..field.start],
            "bytes before the field changed"
        );
        assert_eq!(
            &bytes[field.end..],
            &before[field.end..],
            "bytes after the field changed"
        );
    }

    fn distinct_bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8 + 1).collect()
    }

    fn array_pack() -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "LodGroup".into(),
            size: 0x20,
            fields: vec![
                PackField {
                    name: "lod_distances".into(),
                    offset: 0x08,
                    repr: FieldRepr::ScalarArray {
                        element: Scalar::F32,
                        count: 4,
                    },
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "children".into(),
                    offset: 0x00,
                    repr: FieldRepr::Ptr(Pointee::Many {
                        type_name: None,
                        count: CountSource::Field("child_count".into()),
                        element_size: Some(4),
                    }),
                    space: Some(crate::desc::AddressSpace::Virtual),
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "child_count".into(),
                    offset: 0x18,
                    repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
            ],
            selector: None,
        });
        pack
    }

    #[test]
    fn one_element_of_an_inline_run_is_writable_on_its_own() {
        let pack = array_pack();
        let view = View::of(&pack, TypeName("LodGroup")).expect("bind");
        let third = view
            .element_field::<f32>(FieldName("lod_distances"), 2)
            .expect("bind element");
        assert_eq!(third.offset(), 0x08 + 2 * 4);

        let mut bytes = distinct_bytes(0x40);
        let before = bytes.clone();
        {
            let mut img = ImageMut::new(&mut bytes, 0x20);
            img.at(0x00).set(third, 120.0).expect("write");
        }

        only_touched(&bytes, &before, 0x10..0x14);
        assert_eq!(&bytes[0x10..0x14], &120.0f32.to_le_bytes());
    }

    #[test]
    fn an_inline_runs_element_is_reached_relative_to_the_cursor() {
        let pack = array_pack();
        let view = View::of(&pack, TypeName("LodGroup")).expect("bind");
        let first = view
            .element_field::<f32>(FieldName("lod_distances"), 0)
            .expect("bind element");

        let mut bytes = distinct_bytes(0x80);
        let before = bytes.clone();
        {
            let mut img = ImageMut::new(&mut bytes, 0x40);
            img.at(0x30).set(first, -2.5).expect("write");
        }

        only_touched(&bytes, &before, 0x38..0x3C);
        assert_eq!(&bytes[0x38..0x3C], &(-2.5f32).to_le_bytes());
    }

    #[test]
    fn an_index_past_the_declared_run_does_not_bind() {
        let pack = array_pack();
        let view = View::of(&pack, TypeName("LodGroup")).expect("bind");
        assert!(view
            .element_field::<f32>(FieldName("lod_distances"), 4)
            .is_err());
    }

    fn nested_pack() -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Outer".into(),
            size: 0x40,
            fields: vec![PackField {
                name: "transform".into(),
                offset: 0x10,
                repr: FieldRepr::Inline {
                    type_name: "Transform".into(),
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Transform".into(),
            size: 0x20,
            fields: vec![PackField {
                name: "position".into(),
                offset: 0x08,
                repr: FieldRepr::Inline {
                    type_name: "Vec3".into(),
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Vec3".into(),
            size: 0x0C,
            fields: vec![
                PackField {
                    name: "x".into(),
                    offset: 0x00,
                    repr: FieldRepr::Scalar(Scalar::F32),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "y".into(),
                    offset: 0x04,
                    repr: FieldRepr::Scalar(Scalar::F32),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "z".into(),
                    offset: 0x08,
                    repr: FieldRepr::Scalar(Scalar::F32),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
            ],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_scalar_three_structs_deep_is_writable_through_one_cursor() {
        let pack = nested_pack();
        let outer = View::of(&pack, TypeName("Outer")).expect("bind outer");
        let y = outer
            .inner(FieldName("transform"))
            .expect("descend to transform")
            .inner(FieldName("position"))
            .expect("descend to position")
            .field::<f32>(FieldName("y"))
            .expect("bind y");

        assert_eq!(y.offset(), 0x1C);

        let mut bytes = distinct_bytes(0x80);
        let before = bytes.clone();
        {
            let mut img = ImageMut::new(&mut bytes, 0x40);
            img.at(0x00).set(y, 9.75).expect("write");
        }

        only_touched(&bytes, &before, 0x1C..0x20);
        assert_eq!(&bytes[0x1C..0x20], &9.75f32.to_le_bytes());
    }

    #[test]
    fn a_nested_scalar_reads_back_through_the_same_descent() {
        let pack = nested_pack();
        let outer = View::of(&pack, TypeName("Outer")).expect("bind outer");
        let z = outer
            .inner(FieldName("transform"))
            .expect("descend")
            .inner(FieldName("position"))
            .expect("descend")
            .field::<f32>(FieldName("z"))
            .expect("bind z");

        let mut bytes = vec![0u8; 0x80];
        let mut img = ImageMut::new(&mut bytes, 0x40);
        img.at(0x20).set(z, 0.125).expect("write");

        assert_eq!(img.as_image().at(0x20).get(z), Some(0.125));
    }

    #[test]
    fn descending_into_something_that_is_not_embedded_is_refused() {
        let pack = array_pack();
        let view = View::of(&pack, TypeName("LodGroup")).expect("bind");
        let err = view
            .inner(FieldName("children"))
            .expect_err("should refuse");
        assert!(matches!(err, crate::desc::BindError::WrongRepr(_)));
    }

    fn enum_pack() -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Shader".into(),
            size: 0x10,
            fields: vec![PackField {
                name: "blend_mode".into(),
                offset: 0x04,
                repr: FieldRepr::Enum {
                    type_name: "BlendMode".into(),
                    width: IntWidth::U8,
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });
        pack.enums.push(PackEnum {
            name: "BlendMode".into(),
            width: IntWidth::U8,
            variants: vec![
                PackVariant {
                    name: "Normal".into(),
                    value: 0,
                    note: None,
                },
                PackVariant {
                    name: "Add".into(),
                    value: 3,
                    note: None,
                },
            ],
        });
        pack
    }

    #[test]
    fn an_enum_accepts_a_value_the_pack_declares() {
        let pack = enum_pack();
        let view = View::of(&pack, TypeName("Shader")).expect("bind");
        let mode = view
            .enum_field(FieldName("blend_mode"))
            .expect("bind blend_mode");

        let mut bytes = distinct_bytes(0x40);
        let before = bytes.clone();
        {
            let mut img = ImageMut::new(&mut bytes, 0x20);
            img.at(0x10).set_enum(mode, 3).expect("write Add");
        }

        only_touched(&bytes, &before, 0x14..0x15);
        assert_eq!(bytes[0x14], 3);
    }

    #[test]
    fn an_enum_refuses_a_value_no_variant_covers_and_writes_nothing() {
        let pack = enum_pack();
        let view = View::of(&pack, TypeName("Shader")).expect("bind");
        let mode = view.enum_field(FieldName("blend_mode")).expect("bind");

        let mut bytes = distinct_bytes(0x40);
        let before = bytes.clone();
        {
            let mut img = ImageMut::new(&mut bytes, 0x20);
            let err = img.at(0x10).set_enum(mode, 2).expect_err("should refuse");
            assert_eq!(err, crate::desc::WriteError::NotADeclaredValue { value: 2 });
        }

        assert_eq!(bytes, before, "a refused write must change nothing at all");
    }

    #[test]
    fn an_enum_the_pack_names_but_does_not_describe_does_not_bind() {
        let mut pack = enum_pack();
        pack.enums.clear();
        let view = View::of(&pack, TypeName("Shader")).expect("bind");
        assert!(view.enum_field(FieldName("blend_mode")).is_err());
    }

    #[test]
    fn a_count_the_pack_names_as_structural_is_not_writable() {
        let pack = array_pack();
        let view = View::of(&pack, TypeName("LodGroup")).expect("bind");
        let count = view
            .field::<u16>(FieldName("child_count"))
            .expect("binding for reading is fine");
        assert!(!count.is_writable());

        let mut bytes = distinct_bytes(0x40);
        let before = bytes.clone();
        {
            let mut img = ImageMut::new(&mut bytes, 0x20);
            let err = img.at(0x00).set(count, 3).expect_err("should refuse");
            assert_eq!(err, crate::desc::WriteError::Structural);
        }
        assert_eq!(bytes, before);
    }

    #[test]
    fn a_tagged_bases_selector_is_not_writable_even_as_an_enum() {
        let mut pack = enum_pack();
        pack.types[0].selector = Some(PackSelector {
            tag: "blend_mode".into(),
            tag_bits: None,
            variants: vec![(0, "Shader".into())],
        });

        let view = View::of(&pack, TypeName("Shader")).expect("bind");
        assert!(!view
            .field::<u8>(FieldName("blend_mode"))
            .expect("bind")
            .is_writable());
        assert!(!view
            .enum_field(FieldName("blend_mode"))
            .expect("bind")
            .is_writable());

        let mode = view.enum_field(FieldName("blend_mode")).expect("bind");
        let mut bytes = vec![0u8; 0x40];
        let mut img = ImageMut::new(&mut bytes, 0x20);
        assert_eq!(
            img.at(0x00).set_enum(mode, 0),
            Err(crate::desc::WriteError::Structural),
            "a declared value is still refused on a selector"
        );
    }

    #[test]
    fn an_element_of_a_structural_run_is_refused_too() {
        let mut pack = array_pack();
        pack.types[0].fields.push(PackField {
            name: "weights".into(),
            offset: 0x1C,
            repr: FieldRepr::Ptr(Pointee::Many {
                type_name: None,
                count: CountSource::Field("lod_distances".into()),
                element_size: Some(4),
            }),
            space: Some(crate::desc::AddressSpace::Virtual),
            bits: Vec::new(),
            note: None,
        });

        let view = View::of(&pack, TypeName("LodGroup")).expect("bind");
        let element = view
            .element_field::<f32>(FieldName("lod_distances"), 1)
            .expect("bind");
        assert!(!element.is_writable());

        let mut bytes = vec![0u8; 0x40];
        let mut img = ImageMut::new(&mut bytes, 0x20);
        assert_eq!(
            img.at(0x00).set(element, 1.0),
            Err(crate::desc::WriteError::Structural)
        );
    }

    #[test]
    fn ordinary_authored_data_stays_writable() {
        let pack = array_pack();
        let view = View::of(&pack, TypeName("LodGroup")).expect("bind");
        assert!(view
            .element_field::<f32>(FieldName("lod_distances"), 0)
            .expect("bind")
            .is_writable());
    }

    #[test]
    fn both_images_resolve_an_address_the_same_way() {
        let mut bytes = vec![0u8; 0x200];
        for addr in [0x5000_0000u64, 0x5000_0080, 0x6000_0010, 0x0, 0x7000_0000] {
            let expected = Image::new(&bytes, 0x100).resolve(addr);
            let actual = ImageMut::new(&mut bytes, 0x100).resolve(addr);
            assert_eq!(expected, actual, "disagreed about {addr:#x}");
        }
    }
}
