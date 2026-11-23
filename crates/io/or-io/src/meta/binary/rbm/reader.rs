use std::io::Cursor;

use crate::container::paging::DatResource;
use crate::container::resource::{ResourceEnvelope, Rsc7, Rsc7Seal};
use crate::container::FIXED_VIRTUAL_BASE;
use byteorder::{LittleEndian, ReadBytesExt};

use crate::meta::binary::enum_text;
use crate::meta::binary::hash_index::default_hash_index;
use crate::meta::binary::pso::schema::align_up;
use crate::meta::binary::Nesting;
use crate::meta::diagnostics::{Diagnostics, GapKind};
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::{HashedString, MetaInstance, Value};
use crate::meta::schema::SchemaLibrary;

use super::header::RbmHeader;
use super::schema::{PsoType, RbmClass, RbmClassEntry, RbmEnum, RbmModel};

pub fn parse(library: &SchemaLibrary, bytes: &[u8]) -> Result<MetaInstance> {
    let mut diags = Diagnostics::lenient();
    parse_with(library, bytes, &mut diags)
}

pub fn parse_with(
    library: &SchemaLibrary,
    bytes: &[u8],
    diags: &mut Diagnostics,
) -> Result<MetaInstance> {
    parse_full(library, bytes, diags).map(|(inst, _)| inst)
}

pub(crate) fn parse_full(
    _library: &SchemaLibrary,
    bytes: &[u8],
    diags: &mut Diagnostics,
) -> Result<(MetaInstance, RbmParts)> {
    let resource =
        Rsc7::open(bytes).map_err(|e| MetaError::Other(format!("rbm: rsc7 read: {e}")))?;
    let virtual_size = resource.virtual_size as usize;
    let physical_size = resource.physical_size as usize;
    let rsc = DatResource::new(&resource.data, virtual_size, physical_size);

    let header = RbmHeader::read(&rsc, FIXED_VIRTUAL_BASE)?;
    let model = RbmModel::read(&rsc, &header)?;

    let instance = {
        let mut decoder = RbmDecoder {
            model: &model,
            diags,
            nesting: Nesting::default(),
        };
        decoder.decode_root()?
    };
    Ok((
        instance,
        RbmParts {
            header,
            model,
            decompressed: resource.data,
            seal: resource.seal,
            virtual_page_size: virtual_size,
            physical_page_size: physical_size,
        },
    ))
}

pub(crate) struct RbmParts {
    pub header: RbmHeader,
    pub model: RbmModel,
    pub decompressed: Vec<u8>,
    pub seal: Rsc7Seal,
    pub virtual_page_size: usize,
    pub physical_page_size: usize,
}

impl RbmParts {
    pub(crate) fn into_layout(self) -> crate::meta::binary::layout::RbmLayout {
        crate::meta::binary::layout::RbmLayout {
            header: self.header,
            model: self.model,
            decompressed: self.decompressed,
            seal: Some(self.seal),
            virtual_page_size: self.virtual_page_size,
            physical_page_size: self.physical_page_size,
        }
    }
}

struct RbmDecoder<'m, 'd> {
    model: &'m RbmModel,
    diags: &'d mut Diagnostics,
    nesting: Nesting,
}

impl<'m> RbmDecoder<'m, '_> {
    fn decode_root(&mut self) -> Result<MetaInstance> {
        let block = self
            .model
            .block(self.model.root_block_id as i32)
            .ok_or_else(|| MetaError::Other("rbm: root data block missing".into()))?;
        let class = self.model.class_for(block.data_type).ok_or_else(|| {
            MetaError::Other(format!(
                "rbm: root data block schema {:#X} not found",
                block.data_type
            ))
        })?;
        self.read_struct(class, &block.data, 0)
    }

    /// ⚠ `data` is borrowed from the model, never a copy, so its address
    /// names the block the struct is in.
    fn read_struct(&mut self, class: &RbmClass, data: &[u8], base: usize) -> Result<MetaInstance> {
        let at = (data.as_ptr() as usize).wrapping_add(base);
        let mark = self.nesting.enter(Some((at, class.name_hash)))?;
        let idx = default_hash_index();
        let type_name = idx.name_or_literal(class.name_hash);
        self.diags.push_path(type_name.clone());
        let mut inst = MetaInstance::new(type_name);

        let result = (|| -> Result<()> {
            for entry in &class.entries {
                if entry.name_hash == ARRAY_INFO_NAME_HASH {
                    continue;
                }
                let name = idx.name_or_literal(entry.name_hash);
                let off = base + entry.offset as usize;
                self.diags.push_path(name.clone());
                let decoded = self.decode_member(class, entry, data, off);
                let value = match decoded {
                    Ok(v) => v,
                    Err(err) => {
                        self.diags
                            .record(GapKind::FieldDecodeError, err.to_string())?;
                        Value::Raw(format!("(decode error: {err})"))
                    }
                };
                self.diags.pop_path();
                inst.fields.push((name, value));
            }
            Ok(())
        })();

        self.diags.pop_path();
        self.nesting.leave(mark)?;
        result?;
        Ok(inst)
    }

    fn decode_member(
        &mut self,
        owner: &RbmClass,
        entry: &RbmClassEntry,
        data: &[u8],
        off: usize,
    ) -> Result<Value> {
        match entry.data_type {
            PsoType::Bool => Ok(Value::Bool(read_u8(data, off)? != 0)),
            PsoType::S8 => Ok(Value::Int(read_u8(data, off)? as i8 as i64)),
            PsoType::U8 => Ok(Value::UInt(read_u8(data, off)? as u64)),
            PsoType::S16 => Ok(Value::Int(read_i16_le(data, off)? as i64)),
            PsoType::U16 => Ok(Value::UInt(read_u16_le(data, off)? as u64)),
            PsoType::S32 => Ok(Value::Int(read_i32_le(data, off)? as i64)),
            PsoType::U32 => Ok(Value::UInt(read_u32_le(data, off)? as u64)),
            PsoType::S64 => Ok(Value::Int(read_u64_le(data, off)? as i64)),
            PsoType::U64 => Ok(Value::UInt(read_u64_le(data, off)?)),
            PsoType::Float => Ok(Value::Float(read_f32_le(data, off)? as f64)),
            PsoType::ScalarV => Ok(Value::Float(read_f32_le(data, off)? as f64)),
            PsoType::Double => Ok(Value::Float(f64::from_bits(read_u64_le(data, off)?))),
            PsoType::Float16 => Ok(Value::Float(
                crate::half_to_f32(read_u16_le(data, off)?) as f64
            )),
            PsoType::BoolV => Ok(Value::Bool(read_u32_le(data, off)? != 0)),
            PsoType::Vec2 | PsoType::Vec2V => Ok(Value::Vec2([
                read_f32_le(data, off)?,
                read_f32_le(data, off + 4)?,
            ])),
            PsoType::Vec3 | PsoType::Vec3V => Ok(Value::Vec3([
                read_f32_le(data, off)?,
                read_f32_le(data, off + 4)?,
                read_f32_le(data, off + 8)?,
            ])),
            PsoType::Vec4V | PsoType::VecBoolV => Ok(Value::Vec4([
                read_f32_le(data, off)?,
                read_f32_le(data, off + 4)?,
                read_f32_le(data, off + 8)?,
                read_f32_le(data, off + 12)?,
            ])),
            PsoType::Mat33V | PsoType::Mat43 => read_matrix(data, off, 3),
            PsoType::Mat34V | PsoType::Mat44V => read_matrix(data, off, 4),
            PsoType::StringHash
            | PsoType::LiteralStringHash
            | PsoType::PartialStringHash
            | PsoType::StructId => {
                let h = read_u32_le(data, off)?;
                Ok(Value::Hash(hashed(h)))
            }
            PsoType::NsStringHash => {
                let h = read_u32_le(data, off)?;
                Ok(Value::Hash(hashed(h)))
            }
            PsoType::Enum8 => self.decode_enum(entry, read_u8(data, off)? as i32),
            PsoType::Enum16 => self.decode_enum(entry, read_u16_le(data, off)? as i32),
            PsoType::Enum32 => self.decode_enum(entry, read_i32_le(data, off)?),
            PsoType::Bitset8 => self.decode_bitset(entry, read_u8(data, off)? as u128),
            PsoType::Bitset16 => self.decode_bitset(entry, read_u16_le(data, off)? as u128),
            PsoType::Bitset32 => self.decode_bitset(entry, read_u32_le(data, off)? as u128),
            PsoType::AtBitset32 | PsoType::AtBitset64 => {
                let arr = read_array_header(data, off)?;
                if arr.pointer.is_null() {
                    return self.decode_bitset(entry, 0);
                }
                let block = self.block_data(arr.pointer.block_id())?;
                let base = arr.pointer.data_offset() as usize;
                let words = (arr.count as usize).min(4);
                let mut bits: u128 = 0;
                for i in 0..words {
                    bits |= (read_u32_le(block, base + 4 * i)? as u128) << (32 * i);
                }
                self.decode_bitset(entry, bits)
            }
            PsoType::Struct => {
                let class = self
                    .model
                    .class_for(entry.referent_hash())
                    .ok_or_else(|| {
                        MetaError::Other(format!(
                            "rbm: embedded struct {:#X} not in schema",
                            entry.referent_hash()
                        ))
                    })?
                    .clone();
                Ok(Value::Struct(Box::new(
                    self.read_struct(&class, data, off)?,
                )))
            }
            PsoType::Pointer32 | PsoType::Pointer64 => self.decode_pointer(data, off),
            PsoType::StringMember | PsoType::WideStringMember => {
                let len = entry.string_count() as usize;
                let bytes = data
                    .get(off..off + len)
                    .ok_or_else(|| MetaError::Other("rbm: char array OOB".into()))?;
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(len);
                Ok(Value::String(
                    String::from_utf8_lossy(&bytes[..end]).into_owned(),
                ))
            }
            PsoType::AtString32 | PsoType::AtString64 => self.decode_at_string(data, off),
            PsoType::StringPointer32 | PsoType::StringPointer64 => {
                let ptr = read_pointer(data, off)?;
                if ptr.is_null() {
                    return Ok(Value::String(String::new()));
                }
                let block = self.block_data(ptr.block_id())?;
                let base = ptr.data_offset() as usize;
                let slice = block
                    .get(base..)
                    .ok_or_else(|| MetaError::Other("rbm: string offset OOB".into()))?;
                let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                Ok(Value::String(
                    String::from_utf8_lossy(&slice[..end]).into_owned(),
                ))
            }
            PsoType::ArrayMember | PsoType::AtFixedArray => {
                let count = entry.array_count() as usize;
                self.decode_array_elements(owner, entry, data, off, count)
            }
            PsoType::AtArray32
            | PsoType::AtArray64
            | PsoType::AtArray32With32BitIdx
            | PsoType::AtArray64With32BitIdx => {
                let header = read_array_header(data, off)?;
                if header.pointer.is_null() || header.count == 0 {
                    return Ok(Value::Array(Vec::new()));
                }
                let block = self.block_data(header.pointer.block_id())?;
                let base = header.pointer.data_offset() as usize;
                self.decode_array_elements(owner, entry, block, base, header.count as usize)
            }
            PsoType::ArrayPointer32
            | PsoType::ArrayPointer64
            | PsoType::ArrayPointer32WithCount
            | PsoType::ArrayPointer64WithCount => {
                let ptr = read_pointer(data, off)?;
                if ptr.is_null() {
                    return Ok(Value::Null);
                }
                let block = self.block_data(ptr.block_id())?;
                let base = ptr.data_offset() as usize;
                Ok(Value::ByteArray(block.get(base..).unwrap_or(&[]).to_vec()))
            }
            PsoType::AtBinaryMap32 | PsoType::AtBinaryMap64 => {
                self.decode_map(owner, entry, data, off)
            }
            PsoType::Invalid => Ok(Value::Null),
            PsoType::AtWideString32
            | PsoType::AtWideString64
            | PsoType::WideStringPointer32
            | PsoType::WideStringPointer64 => {
                self.diags.record(
                    GapKind::UnsupportedArrayElement,
                    format!("wide string {:?}", entry.data_type),
                )?;
                Ok(Value::String(String::new()))
            }
            PsoType::Unknown(b) => {
                self.diags
                    .record(GapKind::UnknownDataType, format!("type {b:#04X}"))?;
                Ok(Value::Raw(format!("(rbm datatype {b:#X})")))
            }
        }
    }

    fn decode_enum(&mut self, entry: &RbmClassEntry, value: i32) -> Result<Value> {
        let type_name = default_hash_index().name_or_literal(entry.referent_hash());
        let variant = match self
            .model
            .enum_for(entry.referent_hash())
            .and_then(|e| variant_name(e, value))
        {
            Some(name_hash) => enum_text::variant_text(name_hash),
            None => enum_text::value_text(value),
        };
        Ok(Value::Enum { type_name, variant })
    }

    fn decode_bitset(&mut self, entry: &RbmClassEntry, bits: u128) -> Result<Value> {
        let type_name = default_hash_index().name_or_literal(entry.bitset_enum_hash());
        Ok(Value::Bitset { type_name, bits })
    }

    fn decode_at_string(&mut self, data: &[u8], off: usize) -> Result<Value> {
        let arr = read_array_header(data, off)?;
        if arr.pointer.is_null() || arr.count == 0 {
            return Ok(Value::String(String::new()));
        }
        let block = self.block_data(arr.pointer.block_id())?;
        let base = arr.pointer.data_offset() as usize;
        let len = arr.count as usize;
        let bytes = block
            .get(base..base + len)
            .ok_or_else(|| MetaError::Other("rbm: string OOB".into()))?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(len);
        Ok(Value::String(
            String::from_utf8_lossy(&bytes[..end]).into_owned(),
        ))
    }

    fn decode_pointer(&mut self, data: &[u8], off: usize) -> Result<Value> {
        let ptr = read_pointer(data, off)?;
        if ptr.is_null() {
            return Ok(Value::Pointer(None));
        }
        let Some((block_data, data_type)) = self
            .model
            .block(ptr.block_id() as i32)
            .map(|b| (b.data.as_slice(), b.data_type))
        else {
            return Ok(Value::Pointer(None));
        };
        let Some(class) = self.model.class_for(data_type).cloned() else {
            return Ok(Value::Pointer(None));
        };
        let inner = self.read_struct(&class, block_data, ptr.data_offset() as usize)?;
        Ok(Value::Pointer(Some(Box::new(inner))))
    }

    fn decode_array_elements(
        &mut self,
        owner: &RbmClass,
        array_entry: &RbmClassEntry,
        data: &[u8],
        base: usize,
        count: usize,
    ) -> Result<Value> {
        let Some(elem) = owner.entry(array_entry.contents_index()).copied() else {
            self.diags.record(
                GapKind::MissingSchema,
                format!(
                    "array element descriptor index {} out of range",
                    array_entry.contents_index()
                ),
            )?;
            return Ok(Value::Array(Vec::new()));
        };
        let Some(stride) = self.element_stride(&elem) else {
            self.diags.record(
                GapKind::UnsupportedArrayElement,
                format!("{:?}", elem.data_type),
            )?;
            return Ok(Value::Array(Vec::new()));
        };

        let mark = self.nesting.enter(None)?;
        // ⚠ A fixed array's count is a `u32` the file states; an allocation
        // sized from a corrupt one fails, and that aborts rather than panics.
        let mut items = Vec::with_capacity(count.min(data.len()));
        let decoded: Result<()> = (0..count).try_for_each(|i| {
            items.push(self.decode_member(owner, &elem, data, base + stride * i)?);
            Ok(())
        });
        self.nesting.leave(mark)?;
        decoded?;
        Ok(Value::Array(items))
    }

    fn decode_map(
        &mut self,
        owner: &RbmClass,
        entry: &RbmClassEntry,
        data: &[u8],
        off: usize,
    ) -> Result<Value> {
        let key_desc = owner.entry(entry.map_key_index()).copied();
        let value_desc = owner.entry(entry.map_value_index()).copied();
        let (Some(key_desc), Some(value_desc)) = (key_desc, value_desc) else {
            self.diags.record(
                GapKind::UnsupportedMap,
                format!(
                    "map key/value descriptor index {}/{} out of range",
                    entry.map_key_index(),
                    entry.map_value_index()
                ),
            )?;
            return Ok(Value::Map(Vec::new()));
        };

        let header = read_array_header(data, off)?;
        if header.pointer.is_null() || header.count == 0 {
            return Ok(Value::Map(Vec::new()));
        }
        let (Some(key_stride), Some(value_stride)) = (
            self.element_stride(&key_desc),
            self.element_stride(&value_desc),
        ) else {
            self.diags.record(
                GapKind::UnsupportedMap,
                format!(
                    "map of {:?} -> {:?}",
                    key_desc.data_type, value_desc.data_type
                ),
            )?;
            return Ok(Value::Map(Vec::new()));
        };

        let pair_align = key_stride.max(value_stride).clamp(1, 16);
        let pair_size = align_up(key_stride + value_stride, pair_align);
        let block = self.block_data(header.pointer.block_id())?;
        let base = header.pointer.data_offset() as usize;

        let mark = self.nesting.enter(None)?;
        let mut pairs = Vec::with_capacity(header.count as usize);
        let decoded: Result<()> = (0..header.count as usize).try_for_each(|i| {
            let entry_base = base + pair_size * i;
            let key = self.decode_member(owner, &key_desc, block, entry_base)?;
            let value = self.decode_member(owner, &value_desc, block, entry_base + key_stride)?;
            pairs.push((key, value));
            Ok(())
        });
        self.nesting.leave(mark)?;
        decoded?;
        Ok(Value::Map(pairs))
    }

    fn element_stride(&self, elem: &RbmClassEntry) -> Option<usize> {
        match elem.data_type {
            PsoType::Struct => self
                .model
                .class_for(elem.referent_hash())
                .map(|c| c.data_size as usize),
            PsoType::StringMember | PsoType::WideStringMember => Some(elem.string_count() as usize),
            other => other.fixed_size(),
        }
    }

    fn block_data(&self, block_id: u32) -> Result<&'m [u8]> {
        self.model
            .block(block_id as i32)
            .map(|b| b.data.as_slice())
            .ok_or_else(|| MetaError::Other(format!("rbm: block {block_id} missing")))
    }
}

const ARRAY_INFO_NAME_HASH: u32 = 0x100;

pub(crate) fn hashed(hash: u32) -> HashedString {
    HashedString {
        raw: default_hash_index()
            .lookup(hash)
            .map(str::to_string)
            .unwrap_or_default(),
        hash,
    }
}

pub(crate) fn read_matrix(data: &[u8], off: usize, rows: usize) -> Result<Value> {
    let mut m = [[0.0f32; 4]; 3];
    for (r, row) in m.iter_mut().enumerate() {
        if r >= rows {
            break;
        }
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = read_f32_le(data, off + r * 16 + c * 4)?;
        }
    }
    Ok(Value::Mat34(m))
}

pub(crate) fn variant_name(info: &RbmEnum, value: i32) -> Option<u32> {
    info.values
        .iter()
        .find(|v| v.value == value)
        .map(|v| v.name_hash)
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RbmPointer {
    pub raw: u64,
}

impl RbmPointer {
    pub(super) fn block_id(self) -> u32 {
        (self.raw & 0xFFF) as u32
    }
    pub(super) fn data_offset(self) -> u32 {
        ((self.raw >> 12) & 0xFFFFF) as u32
    }
    pub(super) fn is_null(self) -> bool {
        self.block_id() == 0
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RbmArrayHeader {
    pub pointer: RbmPointer,
    pub count: u16,
    pub _capacity: u16,
    pub _unknown: u32,
}

fn read_pointer(data: &[u8], off: usize) -> Result<RbmPointer> {
    let mut cursor = Cursor::new(data);
    cursor.set_position(off as u64);
    let raw = cursor.read_u64::<LittleEndian>()?;
    Ok(RbmPointer { raw })
}

fn read_array_header(data: &[u8], off: usize) -> Result<RbmArrayHeader> {
    let mut cursor = Cursor::new(data);
    cursor.set_position(off as u64);
    let raw = cursor.read_u64::<LittleEndian>()?;
    let count = cursor.read_u16::<LittleEndian>()?;
    let capacity = cursor.read_u16::<LittleEndian>()?;
    let unknown = cursor.read_u32::<LittleEndian>()?;
    Ok(RbmArrayHeader {
        pointer: RbmPointer { raw },
        count,
        _capacity: capacity,
        _unknown: unknown,
    })
}

pub(crate) fn read_u8(data: &[u8], off: usize) -> Result<u8> {
    data.get(off)
        .copied()
        .ok_or_else(|| MetaError::Other(format!("rbm: u8 read OOB at {off}")))
}
pub(crate) fn read_u16_le(data: &[u8], off: usize) -> Result<u16> {
    let mut c = Cursor::new(data);
    c.set_position(off as u64);
    Ok(c.read_u16::<LittleEndian>()?)
}
pub(crate) fn read_i16_le(data: &[u8], off: usize) -> Result<i16> {
    let mut c = Cursor::new(data);
    c.set_position(off as u64);
    Ok(c.read_i16::<LittleEndian>()?)
}
pub(crate) fn read_u32_le(data: &[u8], off: usize) -> Result<u32> {
    let mut c = Cursor::new(data);
    c.set_position(off as u64);
    Ok(c.read_u32::<LittleEndian>()?)
}
pub(crate) fn read_i32_le(data: &[u8], off: usize) -> Result<i32> {
    let mut c = Cursor::new(data);
    c.set_position(off as u64);
    Ok(c.read_i32::<LittleEndian>()?)
}
pub(crate) fn read_u64_le(data: &[u8], off: usize) -> Result<u64> {
    let mut c = Cursor::new(data);
    c.set_position(off as u64);
    Ok(c.read_u64::<LittleEndian>()?)
}
pub(crate) fn read_f32_le(data: &[u8], off: usize) -> Result<f32> {
    let mut c = Cursor::new(data);
    c.set_position(off as u64);
    Ok(c.read_f32::<LittleEndian>()?)
}
