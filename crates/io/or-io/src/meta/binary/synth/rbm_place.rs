//! Where each value goes in an RBM page, once the type table says how wide
//! it is.

use std::collections::BTreeMap;

use crate::container::resource::Rsc7;

use crate::meta::binary::layout::RbmLayout;
use crate::meta::binary::pso::schema::align_up;
use crate::meta::binary::rbm::header::{RbmHeader, RBM_PRD0_MARKER, RBM_RESOURCE_VERSION};
use crate::meta::binary::rbm::schema::{PsoType, RbmClass, RbmClassEntry, RbmDataBlock, RbmModel};
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::{MetaInstance, Value};

use super::rbm::{natural_align, Model, ARRAY_HEADER_BYTES, ARRAY_INFO_NAME_HASH};
use super::{MAX_BLOCK_COUNT, MAX_DATA_OFFSET};

const VIRTUAL_BASE: u64 = 0x5000_0000;
const HEADER_BYTES: usize = 0x70;
const CLASS_HEADER_BYTES: usize = 0x20;
const ENUM_HEADER_BYTES: usize = 0x18;
const ENUM_VALUE_BYTES: usize = 0x08;
const BLOCK_HEADER_BYTES: usize = 0x10;
const CLASS_ENTRY_BYTES: usize = 0x10;
const BLOCK_ALIGN: usize = 16;

/// The `pgBase` vtable slot a file carries, and the `1` beside it.
///
/// ⚠ **A vtable address, and it is overwritten the moment the game places
/// the resource** — a synthesised file needs *a* value rather than the right
/// one. This is the one 1,174 of 2,338 shipped RBM documents carry and the
/// one CodeWalker's `MetaBuilder` writes.
const FILE_VFT: u64 = (1u64 << 32) | 0x405B_C808;

/// The word at `0x14`, constant on all 2,338 staged RBM documents.
const UNKNOWN_14H: u16 = 0x0079;

/// The block tag for an allocation no pointer ever asks the type of: an
/// array payload of scalars, a string's bytes, a bitset's words.
///
/// ⚠ Only a **pointer's** target is resolved through its block's tag —
/// [`crate::meta::binary::rbm::reader`] looks the class up there because the
/// pointer itself carries no type. Everything else reaches its bytes through
/// a descriptor that already says what they are.
const UNTYPED: u32 = 0;

struct Block {
    name_hash: u32,
    buf: Vec<u8>,
}

pub(super) fn place(model: Model, root_hash: u32, instance: &MetaInstance) -> Result<RbmLayout> {
    let mut placer = Placer {
        model: &model,
        blocks: Vec::new(),
        open: BTreeMap::new(),
    };
    placer.place_root(root_hash, instance)?;
    let blocks = placer.blocks;
    emit(model, blocks)
}

struct Placer<'a> {
    model: &'a Model,
    blocks: Vec<Block>,
    /// The block each type is currently filling. Lookup only; the ordering of
    /// [`Placer::blocks`] is what the output depends on.
    open: BTreeMap<u32, usize>,
}

impl<'a> Placer<'a> {
    fn place_root(&mut self, root_hash: u32, instance: &MetaInstance) -> Result<()> {
        let class = self.class_for(root_hash)?;
        let size = class.data_size as usize;
        let align = 1usize << class.align_power.min(6);
        let (block, offset) = self.alloc(root_hash, size, align)?;
        debug_assert_eq!((block, offset), (0, 0), "the root is the first allocation");
        self.place_struct(class, block, offset, instance, 0)
    }

    /// Reserve `size` bytes tagged `type_hash`, returning the block index and
    /// the offset within it.
    ///
    /// ⚠ Instances of one type share a block, and a new block opens only
    /// when the next start would pass what a pointer can address. Giving each
    /// allocation its own block runs out of the 4,095 a twelve-bit block id
    /// allows on the first sizeable document.
    fn alloc(&mut self, type_hash: u32, size: usize, align: usize) -> Result<(usize, usize)> {
        let index = match self.open.get(&type_hash).copied() {
            Some(index)
                if align_up(self.blocks[index].buf.len(), align) + size <= MAX_DATA_OFFSET =>
            {
                index
            }
            _ => self.open_block(type_hash)?,
        };
        let block = &mut self.blocks[index];
        let offset = align_up(block.buf.len(), align);
        // ⚠ Zero here is not only determinism. A heap string reserves one
        // byte more than it writes and that byte *is* its terminator, so
        // filling with anything else runs the writer's own length probe off
        // the end of the allocation.
        block.buf.resize(offset + size, 0);
        Ok((index, offset))
    }

    /// Reserve `size` bytes in a block nothing else joins.
    ///
    /// ⚠ **A pointer array's run is the rest of its block**, because the
    /// reader has no count to stop at — so a second run appended here would
    /// read back as part of the first. Every shipped occluder map gives each
    /// `OccludeModel::verts` a block of its own, exactly `dataSize` long and
    /// tagged with the element's type byte, and this does the same.
    fn alloc_own_block(&mut self, tag: u32, size: usize) -> Result<(usize, usize)> {
        if size > MAX_DATA_OFFSET {
            return Err(MetaError::Other(format!(
                "synth: a run of {size} bytes is longer than a block can address"
            )));
        }
        if self.blocks.len() >= MAX_BLOCK_COUNT {
            return Err(MetaError::Other(format!(
                "synth: this document needs more than the {MAX_BLOCK_COUNT} blocks an RBM \
                 pointer can name"
            )));
        }
        let index = self.blocks.len();
        self.blocks.push(Block {
            name_hash: tag,
            buf: vec![0; size],
        });
        Ok((index, 0))
    }

    fn open_block(&mut self, type_hash: u32) -> Result<usize> {
        if self.blocks.len() >= MAX_BLOCK_COUNT {
            return Err(MetaError::Other(format!(
                "synth: this document needs more than the {MAX_BLOCK_COUNT} blocks an RBM \
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
        class: &'a RbmClass,
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
        for entry in &class.entries {
            if entry.name_hash == ARRAY_INFO_NAME_HASH {
                continue;
            }
            let name = idx.name_or_literal(entry.name_hash);
            let value = inst.get(&name);
            let offset = base + entry.offset as usize;
            self.place_member(class, entry, block, offset, value, depth)
                .map_err(|e| MetaError::Other(format!("{}.{name}: {e}", inst.type_name)))?;
        }
        Ok(())
    }

    fn place_member(
        &mut self,
        owner: &'a RbmClass,
        entry: &RbmClassEntry,
        block: usize,
        offset: usize,
        value: Option<&Value>,
        depth: usize,
    ) -> Result<()> {
        match entry.data_type {
            PsoType::Struct => {
                let Some(Value::Struct(inner)) = value else {
                    return Ok(());
                };
                let class = self.class_for(entry.referent_hash())?;
                self.place_struct(class, block, offset, inner, depth + 1)
            }
            PsoType::Pointer32 | PsoType::Pointer64 => {
                let Some(Value::Pointer(Some(inner))) = value else {
                    return Ok(());
                };
                let target_hash = super::name_hash(&inner.type_name);
                let class = self.class_for(target_hash)?;
                let size = class.data_size as usize;
                let align = 1usize << class.align_power.min(6);
                let (t_block, t_offset) = self.alloc(target_hash, size, align)?;
                self.put_pointer(block, offset, t_block, t_offset)?;
                self.place_struct(class, t_block, t_offset, inner, depth + 1)
            }
            PsoType::StringPointer32 | PsoType::StringPointer64 => {
                let text = value.and_then(text_of).unwrap_or_default();
                if text.is_empty() {
                    return Ok(());
                }
                let (s_block, s_offset) = self.alloc(UNTYPED, text.len() + 1, 1)?;
                self.put(s_block, s_offset, text.as_bytes())?;
                self.put_pointer(block, offset, s_block, s_offset)
            }
            PsoType::AtString32 | PsoType::AtString64 => {
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
            PsoType::AtBitset32 | PsoType::AtBitset64 => {
                // A `u128` of bits is at most four words, and the count is in
                // words rather than bytes — both the reader and the writer
                // clamp to four, so anything wider is unreachable anyway.
                let words: u16 = if entry.bit_count() == 0 {
                    1
                } else {
                    entry.bit_count().div_ceil(32).min(4)
                };
                let (w_block, w_offset) = self.alloc(UNTYPED, usize::from(words) * 4, 4)?;
                self.put_array_header(block, offset, w_block, w_offset, words)
            }
            PsoType::ArrayMember | PsoType::AtFixedArray => {
                let Some(Value::Array(items)) = value else {
                    return Ok(());
                };
                let Some(elem) = owner.entry(entry.contents_index()).copied() else {
                    return Ok(());
                };
                let stride = self.stride_of(&elem)?;
                for (i, item) in items.iter().take(entry.array_count() as usize).enumerate() {
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
            PsoType::AtArray32
            | PsoType::AtArray64
            | PsoType::AtArray32With32BitIdx
            | PsoType::AtArray64With32BitIdx => {
                let Some(Value::Array(items)) = value else {
                    return Ok(());
                };
                if items.is_empty() {
                    return Ok(());
                }
                let Some(elem) = owner.entry(entry.contents_index()).copied() else {
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
            PsoType::ArrayPointer32WithCount | PsoType::ArrayPointer64WithCount => {
                let bytes = match value {
                    Some(Value::ByteArray(bytes)) => bytes.clone(),
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|v| match v {
                            Value::UInt(n) => u8::try_from(*n).ok(),
                            Value::Int(n) => u8::try_from(*n).ok(),
                            _ => None,
                        })
                        .collect::<Option<Vec<u8>>>()
                        .ok_or_else(|| {
                            MetaError::Other(
                                "a pointer array holds something other than bytes".into(),
                            )
                        })?,
                    _ => return Ok(()),
                };
                if bytes.is_empty() {
                    return Ok(());
                }
                let Some(elem) = owner.entry(entry.contents_index()).copied() else {
                    return Err(MetaError::Other(
                        "pointer array has no element descriptor to tag its block with".into(),
                    ));
                };
                let (p_block, p_offset) =
                    self.alloc_own_block(u32::from(elem.data_type.to_byte()), bytes.len())?;
                self.put(p_block, p_offset, &bytes)?;
                self.put_pointer(block, offset, p_block, p_offset)
            }
            PsoType::AtBinaryMap32 | PsoType::AtBinaryMap64 => {
                let Some(Value::Map(pairs)) = value else {
                    return Ok(());
                };
                if pairs.is_empty() {
                    return Ok(());
                }
                let (Some(key_desc), Some(value_desc)) = (
                    owner.entry(entry.map_key_index()).copied(),
                    owner.entry(entry.map_value_index()).copied(),
                ) else {
                    return Err(MetaError::Other(
                        "map has no key/value descriptors to size against".into(),
                    ));
                };
                let key_stride = self.stride_of(&key_desc)?;
                let value_stride = self.stride_of(&value_desc)?;
                let align = key_stride.max(value_stride).clamp(1, 16);
                let pair_size = align_up(key_stride + value_stride, align);
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

    /// Which block an array payload joins, and on what boundary.
    ///
    /// A run of embedded structs is a run of instances of that type, so it
    /// shares their block; anything else is bytes nobody asks the type of.
    fn payload_tag_and_align(&self, elem: &RbmClassEntry) -> (u32, usize) {
        match elem.data_type {
            PsoType::Struct => match self.model.class_for(elem.referent_hash()) {
                Some(class) => (elem.referent_hash(), 1usize << class.align_power.min(6)),
                None => (UNTYPED, 16),
            },
            other => (UNTYPED, natural_align(other)),
        }
    }

    fn stride_of(&self, elem: &RbmClassEntry) -> Result<usize> {
        let stride = match elem.data_type {
            PsoType::Struct => self
                .model
                .class_for(elem.referent_hash())
                .map(|c| c.data_size as usize),
            PsoType::StringMember | PsoType::WideStringMember => Some(elem.string_count() as usize),
            other => other.fixed_size(),
        };
        stride.ok_or_else(|| {
            MetaError::Other(format!(
                "element of type {:?} has no known size",
                elem.data_type
            ))
        })
    }

    fn class_for(&self, hash: u32) -> Result<&'a RbmClass> {
        self.model
            .class_for(hash)
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

    /// ⚠ A block id counts from one, so the index is shifted here and
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
        let packed =
            ((target_block as u64 + 1) & 0xFFF) | ((target_offset as u64 & 0xF_FFFF) << 12);
        self.put(block, offset, &packed.to_le_bytes())
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
        let mut tail = [0u8; ARRAY_HEADER_BYTES - 8];
        tail[..2].copy_from_slice(&count.to_le_bytes());
        // Capacity equals count: the reader discards it, and a synthesised
        // allocation has no spare room by construction.
        tail[2..4].copy_from_slice(&count.to_le_bytes());
        self.put(block, offset + 8, &tail)
    }
}

/// Lay the tables and the blocks out in the page, invent the header, and
/// seal the two together.
///
/// ⚠ The page is deterministic only because the order below is keyed on
/// nothing but the type table's two `Vec`s and the `Vec` the placer filled.
fn emit(model: Model, blocks: Vec<Block>) -> Result<RbmLayout> {
    let mut at = HEADER_BYTES;
    let reserve = |len: usize, align: usize, at: &mut usize| -> usize {
        *at = align_up(*at, align);
        let start = *at;
        *at += len;
        start
    };

    // One virtual chunk, so exactly one system page and no graphics page.
    let pages_info_at = reserve(16 + 8, 16, &mut at);
    let class_table_at = reserve(model.classes.len() * CLASS_HEADER_BYTES, 16, &mut at);
    let class_entries_at: Vec<usize> = model
        .classes
        .iter()
        .map(|c| reserve(c.entries.len() * CLASS_ENTRY_BYTES, 16, &mut at))
        .collect();
    let enum_table_at = reserve(model.enums.len() * ENUM_HEADER_BYTES, 16, &mut at);
    let enum_values_at: Vec<usize> = model
        .enums
        .iter()
        .map(|e| reserve(e.values.len() * ENUM_VALUE_BYTES, 8, &mut at))
        .collect();
    let block_table_at = reserve(blocks.len() * BLOCK_HEADER_BYTES, 16, &mut at);
    let block_data_at: Vec<usize> = blocks
        .iter()
        .map(|b| reserve(b.buf.len(), BLOCK_ALIGN, &mut at))
        .collect();

    let seal = Rsc7::invent_single_chunk_seal(RBM_RESOURCE_VERSION, at)
        .map_err(|e| MetaError::Other(format!("synth: {e}")))?;
    let virtual_page_size = seal.virtual_size() as usize;
    let mut page = vec![0u8; virtual_page_size];

    let addr = |offset: usize| VIRTUAL_BASE | offset as u64;

    // The `pgBase` prologue and the page-count record it names.
    page[0x00..0x08].copy_from_slice(&FILE_VFT.to_le_bytes());
    page[0x08..0x10].copy_from_slice(&addr(pages_info_at).to_le_bytes());
    page[pages_info_at + 8] = 1;

    page[0x10..0x14].copy_from_slice(&RBM_PRD0_MARKER.to_le_bytes());
    page[0x14..0x16].copy_from_slice(&UNKNOWN_14H.to_le_bytes());
    // Names are recovered from hashes, so there is nothing to encrypt.
    page[0x16] = 0;
    page[0x1C..0x20].copy_from_slice(&1u32.to_le_bytes());
    page[0x20..0x28].copy_from_slice(&addr(class_table_at).to_le_bytes());
    page[0x28..0x30].copy_from_slice(&addr(enum_table_at).to_le_bytes());
    page[0x30..0x38].copy_from_slice(&addr(block_table_at).to_le_bytes());
    page[0x48..0x4A].copy_from_slice(&count_of(model.classes.len(), "classes")?.to_le_bytes());
    page[0x4A..0x4C].copy_from_slice(&count_of(model.enums.len(), "enums")?.to_le_bytes());
    page[0x4C..0x4E].copy_from_slice(&count_of(blocks.len(), "data blocks")?.to_le_bytes());

    for (i, class) in model.classes.iter().enumerate() {
        let h = class_table_at + i * CLASS_HEADER_BYTES;
        page[h..h + 4].copy_from_slice(&class.name_hash.to_le_bytes());
        page[h + 4..h + 8].copy_from_slice(&class.signature.to_le_bytes());
        page[h + 8] = class.flags;
        page[h + 9] = class.align_power;
        page[h + 0x10..h + 0x18].copy_from_slice(&addr(class_entries_at[i]).to_le_bytes());
        page[h + 0x18..h + 0x1C].copy_from_slice(&class.data_size.to_le_bytes());
        page[h + 0x1C..h + 0x1E].copy_from_slice(&class.major_version.to_le_bytes());
        page[h + 0x1E..h + 0x20]
            .copy_from_slice(&count_of(class.entries.len(), "class members")?.to_le_bytes());
        for (j, entry) in class.entries.iter().enumerate() {
            let e = class_entries_at[i] + j * CLASS_ENTRY_BYTES;
            page[e..e + CLASS_ENTRY_BYTES].copy_from_slice(&entry.to_bytes());
        }
    }

    for (i, info) in model.enums.iter().enumerate() {
        let h = enum_table_at + i * ENUM_HEADER_BYTES;
        page[h..h + 4].copy_from_slice(&info.name_hash.to_le_bytes());
        page[h + 4..h + 8].copy_from_slice(&info.signature.to_le_bytes());
        page[h + 8..h + 0x10].copy_from_slice(&addr(enum_values_at[i]).to_le_bytes());
        page[h + 0x10..h + 0x12]
            .copy_from_slice(&count_of(info.values.len(), "enum values")?.to_le_bytes());
        page[h + 0x12] = info.flags;
        for (j, v) in info.values.iter().enumerate() {
            let e = enum_values_at[i] + j * ENUM_VALUE_BYTES;
            page[e..e + 4].copy_from_slice(&v.name_hash.to_le_bytes());
            page[e + 4..e + 8].copy_from_slice(&v.value.to_le_bytes());
        }
    }

    let mut data_blocks = Vec::with_capacity(blocks.len());
    for (i, block) in blocks.iter().enumerate() {
        let h = block_table_at + i * BLOCK_HEADER_BYTES;
        page[h..h + 4].copy_from_slice(&block.name_hash.to_le_bytes());
        page[h + 4..h + 8].copy_from_slice(&(block.buf.len() as u32).to_le_bytes());
        page[h + 8..h + 0x10].copy_from_slice(&addr(block_data_at[i]).to_le_bytes());
        let start = block_data_at[i];
        page[start..start + block.buf.len()].copy_from_slice(&block.buf);
        data_blocks.push(RbmDataBlock {
            data_type: block.name_hash,
            data: block.buf.clone(),
            data_addr: addr(start),
        });
    }

    let classes = model
        .classes
        .iter()
        .map(|c| (c.name_hash, c.clone()))
        .collect();
    let enums = model
        .enums
        .iter()
        .map(|e| (e.name_hash, e.clone()))
        .collect();
    let header = RbmHeader {
        vft: FILE_VFT,
        block_map_ptr: addr(pages_info_at),
        unknown_10h: RBM_PRD0_MARKER,
        unknown_14h: UNKNOWN_14H,
        has_encrypted_strings: 0,
        unknown_17h: 0,
        unknown_18h: 0,
        root_block_id: 1,
        class_refs_ptr: addr(class_table_at),
        enum_refs_ptr: addr(enum_table_at),
        data_blocks_ptr: addr(block_table_at),
        name_ptr: 0,
        encrypted_strings_ptr: 0,
        class_count: model.classes.len() as u16,
        enum_count: model.enums.len() as u16,
        data_block_count: blocks.len() as u16,
        unknown_4eh: 0,
        unknown_50h: [0; 8],
    };

    Ok(RbmLayout {
        header,
        model: RbmModel {
            classes,
            enums,
            data_blocks,
            root_block_id: 1,
            class_order: model.classes.iter().map(|c| c.name_hash).collect(),
            enum_order: model.enums.iter().map(|e| e.name_hash).collect(),
        },
        decompressed: page,
        seal: Some(seal),
        virtual_page_size,
        physical_page_size: 0,
    })
}

fn count_of(n: usize, what: &str) -> Result<u16> {
    u16::try_from(n).map_err(|_| {
        MetaError::Other(format!(
            "synth: {n} {what} is more than the count field holds"
        ))
    })
}

fn text_of(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s.as_str()),
        Value::Hash(h) => Some(h.raw.as_str()),
        _ => None,
    }
}
