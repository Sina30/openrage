//! The half of resource building that is RAGE's, not RSC7's.
//!
//! [`ResourceHeap`] appends objects to a virtual and a physical heap and records
//! pointer fixups. [`PgRscBuilder`](super::PgRscBuilder) is this plus a bucket
//! packer, and the packer is the part that knows what an RSC7 page table is.
//! [`crate::write`] interprets a description through this type alone, so a call
//! into the packer from there is a compile error, bucketed builder or not.
//!
//! ⚠ The fixup table is bookkeeping RSC7 never reads: rebased addresses are in
//! the data itself, and [`ResourceHeap::finish`] drops both vectors. It is kept
//! because [`crate::write`] returns it and an envelope that stores a fixup
//! table would need it.

use crate::constants::STRING_ALIGNMENT;
use crate::error::{IoError, Result};
use crate::Generation;

use crate::desc::PagingModel;

use super::resource::NULL_PTR;

#[derive(Debug, Clone)]
pub struct PgPointerFixup {
    pub write_offset: u32,

    pub target_address: u64,

    pub is_physical: bool,
}

/// What a freshly reserved byte holds before anybody writes to it.
///
/// ⚠ **Zero is a plausible value for most of what a resource holds** — a null
/// pointer, an empty count, an identity flag — so a writer that reserves space
/// and never fills it emits a file that reads correctly and means nothing it
/// was told. [`crate::write::canvas`] catches it by building the same resource
/// under two fills: a byte that differs between the two is a byte nobody wrote.
pub const DEFAULT_CANVAS_FILL: u8 = 0x00;

pub const POISON_CANVAS_FILL: u8 = 0xCD;

#[derive(Debug)]
pub struct ResourceHeap {
    virtual_data: Vec<u8>,

    physical_data: Vec<u8>,

    virtual_pointers: Vec<PgPointerFixup>,

    physical_pointers: Vec<PgPointerFixup>,

    shared_objects: std::collections::BTreeMap<u64, u64>,

    generation: Generation,

    canvas_fill: u8,

    paging: PagingModel,
}

impl Default for ResourceHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceHeap {
    pub fn new() -> Self {
        Self {
            virtual_data: Vec::new(),
            physical_data: Vec::new(),
            virtual_pointers: Vec::new(),
            physical_pointers: Vec::new(),
            shared_objects: std::collections::BTreeMap::new(),
            generation: Generation::Gen8,
            canvas_fill: DEFAULT_CANVAS_FILL,
            paging: PagingModel::RAGE,
        }
    }

    pub fn with_capacity(virtual_capacity: usize, physical_capacity: usize) -> Self {
        Self {
            virtual_data: Vec::with_capacity(virtual_capacity),
            physical_data: Vec::with_capacity(physical_capacity),
            ..Self::new()
        }
    }

    /// Where the addresses this heap **invents** come from; defaults to
    /// [`PagingModel::RAGE`].
    ///
    /// ⚠ A reader infers a pointer's space from the value in front of it; a
    /// writer has no value yet, so a container that moves either base has to
    /// say so here.
    #[must_use]
    pub fn with_paging(mut self, paging: PagingModel) -> Self {
        self.paging = paging;
        self
    }

    pub fn paging(&self) -> PagingModel {
        self.paging
    }

    /// Where in this heap an address it invented lives: the inverse of
    /// [`virtual_address`](Self::virtual_address) under this heap's
    /// [`PagingModel`], never `addr - FIXED_VIRTUAL_BASE`.
    pub fn virtual_offset_of(&self, address: u64) -> usize {
        address.saturating_sub(self.paging.virtual_base) as usize
    }

    /// What reserved-but-unwritten space holds — see [`DEFAULT_CANVAS_FILL`].
    pub fn set_canvas_fill(&mut self, fill: u8) {
        self.canvas_fill = fill;
    }

    pub fn canvas_fill(&self) -> u8 {
        self.canvas_fill
    }

    pub fn set_generation(&mut self, generation: Generation) {
        self.generation = generation;
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub fn is_gen9(&self) -> bool {
        self.generation.is_gen9()
    }

    #[inline]
    pub fn virtual_address(&self) -> u64 {
        self.paging.virtual_base + self.virtual_data.len() as u64
    }

    #[inline]
    pub fn physical_address(&self) -> u64 {
        self.paging.physical_base + self.physical_data.len() as u64
    }

    #[inline]
    pub fn virtual_offset(&self) -> u32 {
        self.virtual_data.len() as u32
    }

    #[inline]
    pub fn physical_offset(&self) -> u32 {
        self.physical_data.len() as u32
    }

    pub fn alloc_virtual(&mut self, size: usize) -> u64 {
        let addr = self.virtual_address();
        let fill = self.canvas_fill;
        self.virtual_data
            .resize(self.virtual_data.len() + size, fill);
        addr
    }

    pub fn alloc_physical(&mut self, size: usize) -> u64 {
        let addr = self.physical_address();
        let fill = self.canvas_fill;
        self.physical_data
            .resize(self.physical_data.len() + size, fill);
        addr
    }

    pub fn align_virtual(&mut self, alignment: usize) {
        let remainder = self.virtual_data.len() % alignment;
        if remainder != 0 {
            let padding = alignment - remainder;
            let fill = self.canvas_fill;
            self.virtual_data
                .resize(self.virtual_data.len() + padding, fill);
        }
    }

    pub fn align_physical(&mut self, alignment: usize) {
        let remainder = self.physical_data.len() % alignment;
        if remainder != 0 {
            let padding = alignment - remainder;
            let fill = self.canvas_fill;
            self.physical_data
                .resize(self.physical_data.len() + padding, fill);
        }
    }

    pub fn shared_object(&self, source_address: u64) -> Option<u64> {
        if source_address == 0 {
            return None;
        }
        self.shared_objects.get(&source_address).copied()
    }

    pub fn record_shared(&mut self, source_address: u64, written_address: u64) {
        if source_address != 0 {
            self.shared_objects.insert(source_address, written_address);
        }
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> u64 {
        let addr = self.virtual_address();
        self.virtual_data.extend_from_slice(bytes);
        addr
    }

    pub fn write_physical_bytes(&mut self, bytes: &[u8]) -> u64 {
        let addr = self.physical_address();
        self.physical_data.extend_from_slice(bytes);
        addr
    }

    pub fn write_padding(&mut self, count: usize) {
        let fill = self.canvas_fill;
        self.virtual_data
            .resize(self.virtual_data.len() + count, fill);
    }

    pub fn write_physical_padding(&mut self, count: usize) {
        let fill = self.canvas_fill;
        self.physical_data
            .resize(self.physical_data.len() + count, fill);
    }

    #[inline]
    pub fn write_u8(&mut self, value: u8) {
        self.virtual_data.push(value);
    }

    #[inline]
    pub fn write_u16(&mut self, value: u16) {
        self.virtual_data.extend_from_slice(&value.to_le_bytes());
    }

    #[inline]
    pub fn write_u32(&mut self, value: u32) {
        self.virtual_data.extend_from_slice(&value.to_le_bytes());
    }

    #[inline]
    pub fn write_u64(&mut self, value: u64) {
        self.virtual_data.extend_from_slice(&value.to_le_bytes());
    }

    #[inline]
    pub fn write_f32(&mut self, value: f32) {
        self.virtual_data
            .extend_from_slice(&value.to_bits().to_le_bytes());
    }

    pub fn write_ptr(&mut self, target: Option<u64>) -> Result<()> {
        let write_offset = self.virtual_offset();
        let addr = target.unwrap_or(NULL_PTR);

        if addr != NULL_PTR {
            self.virtual_pointers.push(PgPointerFixup {
                write_offset,
                target_address: addr,
                is_physical: false,
            });
        }

        self.write_u64(addr);
        Ok(())
    }

    pub fn write_u64_array(&mut self, addrs: &[u64]) -> u64 {
        if addrs.is_empty() {
            return NULL_PTR;
        }
        let addr = self.virtual_address();
        for a in addrs {
            self.write_u64(*a);
            if *a != NULL_PTR {
                self.virtual_pointers.push(PgPointerFixup {
                    write_offset: (self.virtual_data.len() - 8) as u32,
                    target_address: *a,
                    is_physical: false,
                });
            }
        }
        addr
    }

    pub fn write_u64_array_with_capacity(&mut self, addrs: &[u64], capacity: u16) -> u64 {
        if capacity == 0 {
            return NULL_PTR;
        }

        let addr = self.virtual_address();
        let capacity = capacity as usize;

        for i in 0..capacity {
            let target = addrs.get(i).copied().unwrap_or(NULL_PTR);
            self.write_u64(target);
            if target != NULL_PTR {
                self.virtual_pointers.push(PgPointerFixup {
                    write_offset: (self.virtual_data.len() - 8) as u32,
                    target_address: target,
                    is_physical: false,
                });
            }
        }

        addr
    }

    pub fn reserve_ptr(&mut self) -> u32 {
        let offset = self.virtual_offset();
        self.write_u64(NULL_PTR);
        offset
    }

    pub fn patch_ptr(&mut self, offset: u32, target: u64) -> Result<()> {
        let offset = offset as usize;
        if offset + 8 > self.virtual_data.len() {
            return Err(IoError::SerializationFailed {
                offset: offset as u64,
                message: "Pointer patch offset out of bounds".to_string(),
            });
        }

        self.virtual_data[offset..offset + 8].copy_from_slice(&target.to_le_bytes());

        if target != NULL_PTR {
            self.virtual_pointers.push(PgPointerFixup {
                write_offset: offset as u32,
                target_address: target,
                is_physical: false,
            });
        }

        Ok(())
    }

    pub fn write_bytes_at(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        if offset + bytes.len() > self.virtual_data.len() {
            return Err(IoError::SerializationFailed {
                offset: offset as u64,
                message: format!(
                    "Write of {} bytes at offset {offset} exceeds virtual data length {}",
                    bytes.len(),
                    self.virtual_data.len()
                ),
            });
        }
        self.virtual_data[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    pub fn write_u64_at(&mut self, offset: usize, value: u64) -> Result<()> {
        if offset + 8 > self.virtual_data.len() {
            return Err(IoError::SerializationFailed {
                offset: offset as u64,
                message: "Write offset out of bounds".to_string(),
            });
        }
        self.virtual_data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_u32_at(&mut self, offset: usize, value: u32) -> Result<()> {
        if offset + 4 > self.virtual_data.len() {
            return Err(IoError::SerializationFailed {
                offset: offset as u64,
                message: "Write offset out of bounds".to_string(),
            });
        }
        self.virtual_data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_u16_at(&mut self, offset: usize, value: u16) -> Result<()> {
        if offset + 2 > self.virtual_data.len() {
            return Err(IoError::SerializationFailed {
                offset: offset as u64,
                message: "Write offset out of bounds".to_string(),
            });
        }
        self.virtual_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_f32_at(&mut self, offset: usize, value: f32) -> Result<()> {
        if offset + 4 > self.virtual_data.len() {
            return Err(IoError::SerializationFailed {
                offset: offset as u64,
                message: "Write offset out of bounds".to_string(),
            });
        }
        self.virtual_data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn patch_field_ptr(
        &mut self,
        header_offset: u32,
        field_offset: u32,
        addr: u64,
    ) -> Result<()> {
        if addr != NULL_PTR {
            self.write_u64_at((header_offset + field_offset) as usize, addr)?;
            self.register_fixup(header_offset + field_offset, addr)?;
        }
        Ok(())
    }

    pub fn patch_bytes(&mut self, offset: u32, data: &[u8]) -> Result<()> {
        let offset = offset as usize;
        if offset + data.len() > self.virtual_data.len() {
            return Err(IoError::SerializationFailed {
                offset: offset as u64,
                message: format!(
                    "Patch offset {} + len {} exceeds virtual data size {}",
                    offset,
                    data.len(),
                    self.virtual_data.len()
                ),
            });
        }
        self.virtual_data[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    pub fn register_fixup(&mut self, offset: u32, target: u64) -> Result<()> {
        if target != NULL_PTR {
            self.virtual_pointers.push(PgPointerFixup {
                write_offset: offset,
                target_address: target,
                is_physical: false,
            });
        }
        Ok(())
    }

    pub fn register_physical_fixup(&mut self, offset: u32, target: u64) -> Result<()> {
        if target != NULL_PTR {
            self.physical_pointers.push(PgPointerFixup {
                write_offset: offset,
                target_address: target,
                is_physical: true,
            });
        }
        Ok(())
    }

    pub fn write_string(&mut self, s: &str) -> u64 {
        let addr = self.virtual_address();
        self.virtual_data.extend_from_slice(s.as_bytes());
        self.virtual_data.push(0);

        self.align_virtual(STRING_ALIGNMENT);

        addr
    }

    pub fn write_string_unaligned(&mut self, s: &str) -> u64 {
        let addr = self.virtual_address();
        self.virtual_data.extend_from_slice(s.as_bytes());
        self.virtual_data.push(0);
        addr
    }

    pub fn write_array_header(&mut self, count: u16) -> u32 {
        let ptr_offset = self.reserve_ptr();
        self.write_u16(count);
        self.write_u16(count);
        self.write_u32(0);
        ptr_offset
    }

    pub fn write_array_header_with_ptr(&mut self, data_addr: u64, count: u16) {
        self.write_u64(data_addr);
        self.write_u16(count);
        self.write_u16(count);
        self.write_u32(0);

        if data_addr != NULL_PTR {
            let ptr_offset = self.virtual_offset() - 16;
            self.virtual_pointers.push(PgPointerFixup {
                write_offset: ptr_offset,
                target_address: data_addr,
                is_physical: false,
            });
        }
    }

    pub fn virtual_data(&self) -> &[u8] {
        &self.virtual_data
    }

    pub fn physical_data(&self) -> &[u8] {
        &self.physical_data
    }

    pub fn virtual_pointers(&self) -> &[PgPointerFixup] {
        &self.virtual_pointers
    }

    pub fn physical_pointers(&self) -> &[PgPointerFixup] {
        &self.physical_pointers
    }

    pub fn take_virtual_data(self) -> Vec<u8> {
        self.virtual_data
    }

    /// Drops the fixup tables, which RSC7 does not store: rebased addresses are
    /// already in the data.
    pub fn finish(self) -> (Vec<u8>, Vec<u8>) {
        (self.virtual_data, self.physical_data)
    }

    pub fn finish_with_fixups(
        self,
    ) -> (Vec<u8>, Vec<u8>, Vec<PgPointerFixup>, Vec<PgPointerFixup>) {
        (
            self.virtual_data,
            self.physical_data,
            self.virtual_pointers,
            self.physical_pointers,
        )
    }
}
