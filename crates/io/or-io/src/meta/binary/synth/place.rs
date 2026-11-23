//! Where each value goes, once the type table says how wide it is.

use std::collections::BTreeMap;

use crate::meta::binary::layout::PsoLayout;
use crate::meta::binary::pso::schema::{
    align_up, element_align, element_stride, map_pair_layout, string_subtype, struct_subtype,
    PsoDataType, PsoSchemaSection, PsoStructEntry, PsoStructInfo, ARRAY_HEADER_BYTES,
    ARRAY_INFO_NAME_HASH, POINTER_BYTES, STRUCT_ALIGN,
};
use crate::meta::binary::pso::sections::{PmapEntry, PmapHeaderKind, PmapSection, PsoSection};
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::{MetaInstance, Value};

/// A block id is twelve bits of a pointer and zero means null.
pub const MAX_BLOCK_COUNT: usize = 0xFFF;

/// A data offset is the next twenty bits.
pub const MAX_DATA_OFFSET: usize = 0xF_FFFF;

const PSIN_HEADER_LEN: usize = 8;
const BLOCK_ALIGN: usize = 16;

/// The block tag for an allocation no pointer ever asks the type of: an array
/// payload of scalars, a string's bytes, a map's pairs.
///
/// ⚠ Only a **struct pointer's** target is resolved through its block's tag —
/// [`crate::meta::binary::pso::reader`] looks the type up there because the pointer
/// itself carries no type. Everything else reaches its bytes through a
/// descriptor that already says what they are.
const UNTYPED: u32 = 0;

struct Block {
    name_hash: u32,
    buf: Vec<u8>,
}

/// Lay `instance` out against `schema` and seal the two into a layout.
pub(super) fn place(
    schema: PsoSchemaSection,
    root_hash: u32,
    instance: &MetaInstance,
) -> Result<PsoLayout> {
    let (pmap, psin) = {
        let mut placer = Placer {
            schema: &schema,
            blocks: Vec::new(),
            open: BTreeMap::new(),
        };
        placer.place_root(root_hash, instance)?;
        placer.finish()?
    };

    Ok(PsoLayout {
        schema,
        pmap,
        psin,
        opaque_sections: Vec::new(),
        section_order: vec![PsoSection::Psin, PsoSection::Pmap, PsoSection::Psch],
    })
}

struct Placer<'a> {
    schema: &'a PsoSchemaSection,
    blocks: Vec<Block>,
    /// The block each type is currently filling. Lookup only; the ordering of
    /// [`Placer::blocks`] is what the output depends on.
    open: BTreeMap<u32, usize>,
}

impl<'a> Placer<'a> {
    fn place_root(&mut self, root_hash: u32, instance: &MetaInstance) -> Result<()> {
        let info = self.struct_for(root_hash)?;
        let size = info.data_size as usize;
        let (block, offset) = self.alloc(root_hash, size, STRUCT_ALIGN)?;
        debug_assert_eq!((block, offset), (0, 0), "the root is the first allocation");
        self.place_struct(info, block, offset, instance, 0)
    }

    /// Reserve `size` bytes tagged `type_hash`, returning the block index and
    /// the offset within it.
    ///
    /// ⚠ Instances of one type share a block, and a new block opens only when
    /// the next start would pass what a pointer can address. Giving each
    /// allocation its own block is simpler and runs out of the 4,095 a
    /// twelve-bit block id allows on the first sizeable document.
    fn alloc(&mut self, type_hash: u32, size: usize, align: usize) -> Result<(usize, usize)> {
        let index = match self.open.get(&type_hash).copied() {
            Some(index) if align_up(self.blocks[index].buf.len(), align) <= MAX_DATA_OFFSET => {
                index
            }
            _ => self.open_block(type_hash)?,
        };
        let block = &mut self.blocks[index];
        let offset = align_up(block.buf.len(), align);
        // ⚠ Zero here is not only determinism. A heap string reserves one byte
        // more than it writes and that byte *is* its terminator, so filling
        // with anything else runs the writer's own length probe off the end of
        // the allocation. Measured with 0xAB: 444 of 2,652 schema structs stop
        // round-tripping, and the corpus fixed point goes from 3,526 documents
        // exercised to 67.
        block.buf.resize(offset + size, 0);
        Ok((index, offset))
    }

    fn open_block(&mut self, type_hash: u32) -> Result<usize> {
        if self.blocks.len() >= MAX_BLOCK_COUNT {
            return Err(MetaError::Other(format!(
                "synth: this document needs more than the {MAX_BLOCK_COUNT} blocks a PSO \
                 pointer can name"
            )));
        }
        let index = self.blocks.len();
        self.blocks.push(Block {
            name_hash: type_hash,
            buf: Vec::new(),
        });
        self.open.insert(type_hash, index);
        Ok(index)
    }

    fn place_struct(
        &mut self,
        info: &'a PsoStructInfo,
        block: usize,
        base: usize,
        inst: &MetaInstance,
        depth: usize,
    ) -> Result<()> {
        if depth > super::MAX_DEPTH {
            return Err(MetaError::Other(format!(
                "synth: '{}' nests deeper than {} levels",
                inst.type_name,
                super::MAX_DEPTH
            )));
        }
        let idx = crate::meta::binary::hash_index::default_hash_index();
        for entry in &info.entries {
            if entry.name_hash == ARRAY_INFO_NAME_HASH {
                continue;
            }
            let name = idx.name_or_literal(entry.name_hash);
            let value = inst.get(&name);
            let offset = base + entry.offset() as usize;
            self.place_member(info, entry, block, offset, value, depth)
                .map_err(|e| MetaError::Other(format!("{}.{name}: {e}", inst.type_name)))?;
        }
        Ok(())
    }

    /// Reserve whatever heap storage this member needs and write the pointer
    /// or header that reaches it. A member with no heap of its own occupies
    /// space the enclosing struct already reserved and is left at zero.
    fn place_member(
        &mut self,
        owner: &'a PsoStructInfo,
        entry: &PsoStructEntry,
        block: usize,
        offset: usize,
        value: Option<&Value>,
        depth: usize,
    ) -> Result<()> {
        match entry.data_type {
            PsoDataType::Structure => match entry.subtype() {
                struct_subtype::EMBEDDED => {
                    let Some(Value::Struct(inner)) = value else {
                        return Ok(());
                    };
                    let info = self.struct_for(entry.referent_hash())?;
                    self.place_struct(info, block, offset, inner, depth + 1)
                }
                _ => {
                    let Some(Value::Pointer(Some(inner))) = value else {
                        return Ok(());
                    };
                    let target_hash = super::name_hash(&inner.type_name);
                    let info = self.struct_for(target_hash)?;
                    let size = info.data_size as usize;
                    let (t_block, t_offset) = self.alloc(target_hash, size, STRUCT_ALIGN)?;
                    self.put_pointer(block, offset, t_block, t_offset)?;
                    self.place_struct(info, t_block, t_offset, inner, depth + 1)
                }
            },
            PsoDataType::String => match entry.subtype() {
                string_subtype::POINTER | string_subtype::POINTER_ALT => {
                    let text = value.and_then(text_of).unwrap_or_default();
                    if text.is_empty() {
                        return Ok(());
                    }
                    let (s_block, s_offset) = self.alloc(UNTYPED, text.len() + 1, 1)?;
                    self.put(s_block, s_offset, text.as_bytes())?;
                    self.put_pointer(block, offset, s_block, s_offset)
                }
                string_subtype::CHAR_ARRAY => {
                    let text = value.and_then(text_of).unwrap_or_default();
                    if text.is_empty() {
                        return Ok(());
                    }
                    let len = text.len() + 1;
                    let count = u16::try_from(len).map_err(|_| {
                        MetaError::Other(format!(
                            "string of {} bytes is longer than an atString count holds",
                            text.len()
                        ))
                    })?;
                    let (s_block, s_offset) = self.alloc(UNTYPED, len, 1)?;
                    self.put(s_block, s_offset, text.as_bytes())?;
                    self.put_array_header(block, offset, s_block, s_offset, count)
                }
                _ => Ok(()),
            },
            PsoDataType::Array => {
                if entry.count() != 0 {
                    return self.place_inline_array(owner, entry, block, offset, value, depth);
                }
                let Some(Value::Array(items)) = value else {
                    return Ok(());
                };
                if items.is_empty() {
                    return Ok(());
                }
                let Some(elem) = entry.member_index().and_then(|i| owner.entry(i)).copied() else {
                    return Err(MetaError::Other(
                        "array has no element descriptor to size against".into(),
                    ));
                };
                let stride = self.stride_of(&elem)?;
                let count = u16::try_from(items.len()).map_err(|_| {
                    MetaError::Other(format!(
                        "array of {} elements is longer than the count field holds",
                        items.len()
                    ))
                })?;
                let (tag, align) = self.payload_tag_and_align(&elem);
                let (p_block, p_offset) = self.alloc(tag, stride * items.len(), align)?;
                self.put_array_header(block, offset, p_block, p_offset, count)?;
                for (i, item) in items.iter().enumerate() {
                    self.place_member(
                        owner,
                        &elem,
                        p_block,
                        p_offset + stride * i,
                        Some(item),
                        depth + 1,
                    )?;
                }
                Ok(())
            }
            PsoDataType::Map => {
                let Some(Value::Map(pairs)) = value else {
                    return Ok(());
                };
                if pairs.is_empty() {
                    return Ok(());
                }
                let (Some(key_desc), Some(value_desc)) = (
                    owner.entry(entry.key_index()).copied(),
                    owner.entry(entry.value_index()).copied(),
                ) else {
                    return Err(MetaError::Other(
                        "map has no key/value descriptors to size against".into(),
                    ));
                };
                let key_stride = self.stride_of(&key_desc)?;
                let value_stride = self.stride_of(&value_desc)?;
                let (align, pair_size) = map_pair_layout(key_stride, value_stride);
                let count = u16::try_from(pairs.len()).map_err(|_| {
                    MetaError::Other(format!(
                        "map of {} entries is longer than the count field holds",
                        pairs.len()
                    ))
                })?;
                let (p_block, p_offset) = self.alloc(UNTYPED, pair_size * pairs.len(), align)?;
                self.put_array_header(block, offset, p_block, p_offset, count)?;
                for (i, (k, v)) in pairs.iter().enumerate() {
                    let at = p_offset + pair_size * i;
                    self.place_member(owner, &key_desc, p_block, at, Some(k), depth + 1)?;
                    self.place_member(
                        owner,
                        &value_desc,
                        p_block,
                        at + key_stride,
                        Some(v),
                        depth + 1,
                    )?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn place_inline_array(
        &mut self,
        owner: &'a PsoStructInfo,
        entry: &PsoStructEntry,
        block: usize,
        offset: usize,
        value: Option<&Value>,
        depth: usize,
    ) -> Result<()> {
        let Some(Value::Array(items)) = value else {
            return Ok(());
        };
        let Some(elem) = entry.member_index().and_then(|i| owner.entry(i)).copied() else {
            return Ok(());
        };
        let stride = self.stride_of(&elem)?;
        for (i, item) in items.iter().take(entry.count() as usize).enumerate() {
            self.place_member(
                owner,
                &elem,
                block,
                offset + stride * i,
                Some(item),
                depth + 1,
            )?;
        }
        Ok(())
    }

    /// Which block an array payload joins, and on what boundary.
    ///
    /// A run of embedded structs is a run of instances of that type, so it
    /// shares their block; anything else is bytes nobody asks the type of.
    fn payload_tag_and_align(&self, elem: &PsoStructEntry) -> (u32, usize) {
        let align = element_align(elem);
        match elem.data_type {
            PsoDataType::Structure if elem.subtype() == struct_subtype::EMBEDDED => {
                (elem.referent_hash(), align)
            }
            _ => (UNTYPED, align),
        }
    }

    fn stride_of(&self, elem: &PsoStructEntry) -> Result<usize> {
        element_stride(self.schema, elem).ok_or_else(|| {
            MetaError::Other(format!(
                "element of type {:?} subtype {} has no known size",
                elem.data_type,
                elem.subtype()
            ))
        })
    }

    fn struct_for(&self, hash: u32) -> Result<&'a PsoStructInfo> {
        let schema: &'a PsoSchemaSection = self.schema;
        schema
            .struct_for(hash)
            .ok_or_else(|| MetaError::Other(format!("type {hash:#010X} is not in the type table")))
    }

    fn put(&mut self, block: usize, offset: usize, bytes: &[u8]) -> Result<()> {
        let slot = self.blocks[block]
            .buf
            .get_mut(offset..offset + bytes.len())
            .ok_or_else(|| {
                MetaError::Other(format!("synth: write past block {block} at {offset:#X}"))
            })?;
        slot.copy_from_slice(bytes);
        Ok(())
    }

    /// ⚠ A block id counts from one, so the index has to be shifted here and
    /// nowhere else; zero is the null pointer.
    fn put_pointer(
        &mut self,
        block: usize,
        offset: usize,
        target_block: usize,
        target_offset: usize,
    ) -> Result<()> {
        if target_offset > MAX_DATA_OFFSET {
            return Err(MetaError::Other(format!(
                "synth: offset {target_offset:#X} is past what a pointer can address"
            )));
        }
        let packed = ((target_block as u32 + 1) & 0xFFF) | ((target_offset as u32) << 12);
        let mut bytes = [0u8; POINTER_BYTES];
        bytes[..4].copy_from_slice(&packed.to_be_bytes());
        self.put(block, offset, &bytes)
    }

    fn put_array_header(
        &mut self,
        block: usize,
        offset: usize,
        target_block: usize,
        target_offset: usize,
        count: u16,
    ) -> Result<()> {
        self.put_pointer(block, offset, target_block, target_offset)?;
        let mut tail = [0u8; ARRAY_HEADER_BYTES - POINTER_BYTES];
        tail[..2].copy_from_slice(&count.to_be_bytes());
        // Capacity equals count: the reader discards it, and no shipped header
        // in the corpus declares more room than it uses.
        tail[2..4].copy_from_slice(&count.to_be_bytes());
        self.put(block, offset + POINTER_BYTES, &tail)
    }

    fn finish(self) -> Result<(PmapSection, Vec<u8>)> {
        let mut psin = vec![0u8; PSIN_HEADER_LEN];
        let mut entries = Vec::with_capacity(self.blocks.len());
        for block in &self.blocks {
            let start = align_up(psin.len(), BLOCK_ALIGN);
            psin.resize(start, 0);
            psin.extend_from_slice(&block.buf);
            entries.push(PmapEntry {
                name_hash: block.name_hash,
                offset: i32::try_from(start).map_err(|_| {
                    MetaError::Other("synth: the document is larger than a block offset".into())
                })?,
                unknown: 0,
                length: block.buf.len() as i32,
            });
        }
        let len = align_up(psin.len(), BLOCK_ALIGN);
        psin.resize(len, 0);
        psin[..4].copy_from_slice(&PsoSection::Psin.ident().to_be_bytes());
        psin[4..8].copy_from_slice(
            &(u32::try_from(len).map_err(|_| {
                MetaError::Other("synth: the document is larger than a PSIN section".into())
            })?)
            .to_be_bytes(),
        );

        Ok((
            PmapSection {
                root_id: 1,
                header_kind: PmapHeaderKind::Legacy { unk_0eh: 0 },
                entries,
            },
            psin,
        ))
    }
}

fn text_of(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s.as_str()),
        Value::Hash(h) => Some(h.raw.as_str()),
        _ => None,
    }
}
