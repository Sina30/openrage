use std::collections::HashMap;

use crate::container::paging::DatResource;
use byteorder::{LittleEndian, ReadBytesExt};

use crate::meta::error::{MetaError, Result};

use super::header::RbmHeader;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PsoType {
    Invalid,
    Bool,
    BoolV,
    VecBoolV,
    StructId,
    Struct,
    Pointer32,
    Pointer64,
    S8,
    U8,
    S16,
    U16,
    S32,
    U32,
    S64,
    U64,
    Float16,
    Float,
    ScalarV,
    Double,
    Vec2,
    Vec2V,
    Vec3,
    Vec3V,
    Vec4V,
    Mat33V,
    Mat34V,
    Mat43,
    Mat44V,
    StringMember,
    StringPointer32,
    StringPointer64,
    AtString32,
    AtString64,
    WideStringMember,
    WideStringPointer32,
    WideStringPointer64,
    AtWideString32,
    AtWideString64,
    StringHash,
    PartialStringHash,
    LiteralStringHash,
    NsStringHash,
    ArrayMember,
    AtArray32,
    AtArray64,
    AtFixedArray,
    AtArray32With32BitIdx,
    AtArray64With32BitIdx,
    ArrayPointer32,
    ArrayPointer64,
    ArrayPointer32WithCount,
    ArrayPointer64WithCount,
    Enum8,
    Enum16,
    Enum32,
    Bitset8,
    Bitset16,
    Bitset32,
    AtBitset32,
    AtBitset64,
    AtBinaryMap32,
    AtBinaryMap64,
    Unknown(u8),
}

impl PsoType {
    pub(crate) fn from_byte(b: u8) -> Self {
        use PsoType::*;
        match b {
            0x00 => Invalid,
            0x01 => Bool,
            0x02 => BoolV,
            0x03 => VecBoolV,
            0x04 => StructId,
            0x05 => Struct,
            0x06 => Pointer32,
            0x07 => Pointer64,

            0x10 => S8,
            0x11 => U8,
            0x12 => S16,
            0x13 => U16,
            0x14 => S32,
            0x15 => U32,
            0x16 => S64,
            0x17 => U64,

            0x20 => Float16,
            0x21 => Float,
            0x22 => ScalarV,
            0x23 => Double,

            0x30 => Vec2,
            0x31 => Vec2V,
            0x32 => Vec3,
            0x33 => Vec3V,
            0x34 => Vec4V,
            0x35 => Mat33V,
            0x36 => Mat34V,
            0x37 => Mat43,
            0x38 => Mat44V,

            0x40 => StringMember,
            0x41 => StringPointer32,
            0x42 => StringPointer64,
            0x43 => AtString32,
            0x44 => AtString64,
            0x45 => WideStringMember,
            0x46 => WideStringPointer32,
            0x47 => WideStringPointer64,
            0x48 => AtWideString32,
            0x49 => AtWideString64,
            0x4a => StringHash,
            0x4b => PartialStringHash,
            0x4c => LiteralStringHash,
            0x4d => NsStringHash,

            0x50 => ArrayMember,
            0x51 => AtArray32,
            0x52 => AtArray64,
            0x53 => AtFixedArray,
            0x54 => AtArray32With32BitIdx,
            0x55 => AtArray64With32BitIdx,
            0x56 => ArrayPointer32,
            0x57 => ArrayPointer64,
            0x58 => ArrayPointer32WithCount,
            0x59 => ArrayPointer64WithCount,

            0x60 => Enum8,
            0x61 => Enum16,
            0x62 => Enum32,
            0x63 => Bitset8,
            0x64 => Bitset16,
            0x65 => Bitset32,
            0x66 => AtBitset32,
            0x67 => AtBitset64,

            0x70 => AtBinaryMap32,
            0x71 => AtBinaryMap64,

            other => Unknown(other),
        }
    }

    pub fn to_byte(self) -> u8 {
        use PsoType::*;
        match self {
            Invalid => 0x00,
            Bool => 0x01,
            BoolV => 0x02,
            VecBoolV => 0x03,
            StructId => 0x04,
            Struct => 0x05,
            Pointer32 => 0x06,
            Pointer64 => 0x07,

            S8 => 0x10,
            U8 => 0x11,
            S16 => 0x12,
            U16 => 0x13,
            S32 => 0x14,
            U32 => 0x15,
            S64 => 0x16,
            U64 => 0x17,

            Float16 => 0x20,
            Float => 0x21,
            ScalarV => 0x22,
            Double => 0x23,

            Vec2 => 0x30,
            Vec2V => 0x31,
            Vec3 => 0x32,
            Vec3V => 0x33,
            Vec4V => 0x34,
            Mat33V => 0x35,
            Mat34V => 0x36,
            Mat43 => 0x37,
            Mat44V => 0x38,

            StringMember => 0x40,
            StringPointer32 => 0x41,
            StringPointer64 => 0x42,
            AtString32 => 0x43,
            AtString64 => 0x44,
            WideStringMember => 0x45,
            WideStringPointer32 => 0x46,
            WideStringPointer64 => 0x47,
            AtWideString32 => 0x48,
            AtWideString64 => 0x49,
            StringHash => 0x4a,
            PartialStringHash => 0x4b,
            LiteralStringHash => 0x4c,
            NsStringHash => 0x4d,

            ArrayMember => 0x50,
            AtArray32 => 0x51,
            AtArray64 => 0x52,
            AtFixedArray => 0x53,
            AtArray32With32BitIdx => 0x54,
            AtArray64With32BitIdx => 0x55,
            ArrayPointer32 => 0x56,
            ArrayPointer64 => 0x57,
            ArrayPointer32WithCount => 0x58,
            ArrayPointer64WithCount => 0x59,

            Enum8 => 0x60,
            Enum16 => 0x61,
            Enum32 => 0x62,
            Bitset8 => 0x63,
            Bitset16 => 0x64,
            Bitset32 => 0x65,
            AtBitset32 => 0x66,
            AtBitset64 => 0x67,

            AtBinaryMap32 => 0x70,
            AtBinaryMap64 => 0x71,

            Unknown(b) => b,
        }
    }

    pub fn fixed_size(self) -> Option<usize> {
        use PsoType::*;
        Some(match self {
            Bool | S8 | U8 | Enum8 | Bitset8 => 1,
            S16 | U16 | Float16 | Enum16 | Bitset16 => 2,
            S32 | U32 | Float | StructId | StringHash | PartialStringHash | LiteralStringHash
            | Enum32 | Bitset32 => 4,
            S64
            | U64
            | Double
            | Pointer64
            | StringPointer64
            | WideStringPointer64
            | ArrayPointer64
            | ArrayPointer64WithCount
            | NsStringHash => 8,
            Pointer32
            | StringPointer32
            | WideStringPointer32
            | ArrayPointer32
            | ArrayPointer32WithCount => 4,
            Vec2 => 8,
            BoolV | VecBoolV | ScalarV | Vec2V | Vec3 | Vec3V | Vec4V => 16,
            Mat33V | Mat43 => 48,
            Mat34V | Mat44V => 64,
            AtString64
            | AtWideString64
            | AtArray64
            | AtArray64With32BitIdx
            | AtBitset64
            | AtBinaryMap64 => 16,
            AtString32
            | AtWideString32
            | AtArray32
            | AtArray32With32BitIdx
            | AtBitset32
            | AtBinaryMap32 => 8,
            Invalid | Struct | StringMember | WideStringMember | ArrayMember | AtFixedArray
            | Unknown(_) => return None,
        })
    }

    pub fn is_bitset(self) -> bool {
        matches!(
            self,
            PsoType::Bitset8
                | PsoType::Bitset16
                | PsoType::Bitset32
                | PsoType::AtBitset32
                | PsoType::AtBitset64
        )
    }

    pub fn is_enum(self) -> bool {
        matches!(self, PsoType::Enum8 | PsoType::Enum16 | PsoType::Enum32)
    }

    pub fn is_map(self) -> bool {
        matches!(self, PsoType::AtBinaryMap32 | PsoType::AtBinaryMap64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RbmClassEntry {
    pub name_hash: u32,
    pub offset: u32,
    pub data_type: PsoType,
    pub(crate) union_9h: u8,
    pub(crate) union_ah: u16,
    pub(crate) union_ch: u32,
}

impl RbmClassEntry {
    /// A descriptor built rather than read.
    ///
    /// ⚠ **`union_9h` starts at zero and that is a measurement, not a
    /// default**: an array's alignment power and flag bits are zero on all
    /// 127,130 member descriptors in the staged metadata corpus. The one
    /// attested exception is a pointer array whose PSC states `align=`, which
    /// [`Self::with_union_9h`] sets.
    pub(crate) fn synthesised(
        name_hash: u32,
        offset: u32,
        data_type: PsoType,
        union_ah: u16,
        union_ch: u32,
    ) -> Self {
        Self {
            name_hash,
            offset,
            data_type,
            union_9h: 0,
            union_ah,
            union_ch,
        }
    }

    /// ⚠ `OccludeModel::verts` carries 4 here — its PSC's `align="16"` as a
    /// power — in every occluder map of both retail installs.
    pub(crate) fn with_union_9h(mut self, union_9h: u8) -> Self {
        self.union_9h = union_9h;
        self
    }

    /// The sixteen bytes this descriptor occupies in a class's entry array.
    pub(crate) fn to_bytes(self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&self.name_hash.to_le_bytes());
        out[4..8].copy_from_slice(&self.offset.to_le_bytes());
        out[8] = self.data_type.to_byte();
        out[9] = self.union_9h;
        out[10..12].copy_from_slice(&self.union_ah.to_le_bytes());
        out[12..16].copy_from_slice(&self.union_ch.to_le_bytes());
        out
    }

    pub fn referent_hash(self) -> u32 {
        self.union_ch
    }

    pub fn string_count(self) -> u32 {
        self.union_ch
    }

    pub fn contents_index(self) -> u16 {
        self.union_ah
    }

    pub fn array_count(self) -> u32 {
        self.union_ch
    }

    pub fn array_align_power(self) -> u8 {
        self.union_9h & 0x1F
    }

    pub fn array_flags(self) -> u8 {
        (self.union_9h >> 5) & 0x7
    }

    pub fn bit_count(self) -> u16 {
        self.union_ah
    }

    pub fn bitset_enum_hash(self) -> u32 {
        self.union_ch
    }

    pub fn map_key_index(self) -> u16 {
        (self.union_ch & 0xFFFF) as u16
    }

    pub fn map_value_index(self) -> u16 {
        (self.union_ch >> 16) as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbmClass {
    pub name_hash: u32,
    pub signature: u32,
    pub flags: u8,
    pub align_power: u8,
    pub data_size: u32,
    pub major_version: u16,
    pub entries: Vec<RbmClassEntry>,
}

impl RbmClass {
    pub fn entry(&self, index: u16) -> Option<&RbmClassEntry> {
        self.entries.get(index as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RbmEnumValue {
    pub name_hash: u32,
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbmEnum {
    pub name_hash: u32,
    pub signature: u32,
    pub flags: u8,
    pub values: Vec<RbmEnumValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbmDataBlock {
    pub data_type: u32,
    pub data: Vec<u8>,
    pub data_addr: u64,
}

#[derive(Debug, Clone)]
pub struct RbmModel {
    pub classes: HashMap<u32, RbmClass>,
    pub enums: HashMap<u32, RbmEnum>,
    pub data_blocks: Vec<RbmDataBlock>,
    pub root_block_id: u32,
    pub class_order: Vec<u32>,
    pub enum_order: Vec<u32>,
}

impl RbmModel {
    pub fn read(rsc: &DatResource<'_>, header: &RbmHeader) -> Result<Self> {
        let mut classes = HashMap::with_capacity(header.class_count as usize);
        let mut class_order = Vec::with_capacity(header.class_count as usize);
        for i in 0..header.class_count as u64 {
            let item_addr = header.class_refs_ptr + i * RBM_CLASS_HEADER_SIZE;
            let class = read_class(rsc, item_addr)?;
            class_order.push(class.name_hash);
            classes.insert(class.name_hash, class);
        }

        let mut enums = HashMap::with_capacity(header.enum_count as usize);
        let mut enum_order = Vec::with_capacity(header.enum_count as usize);
        for i in 0..header.enum_count as u64 {
            let item_addr = header.enum_refs_ptr + i * RBM_ENUM_HEADER_SIZE;
            let enum_def = read_enum(rsc, item_addr)?;
            enum_order.push(enum_def.name_hash);
            enums.insert(enum_def.name_hash, enum_def);
        }

        let mut data_blocks = Vec::with_capacity(header.data_block_count as usize);
        for i in 0..header.data_block_count as u64 {
            let item_addr = header.data_blocks_ptr + i * RBM_DATA_BLOCK_HEADER_SIZE;
            data_blocks.push(read_data_block(rsc, item_addr)?);
        }

        Ok(Self {
            classes,
            enums,
            data_blocks,
            root_block_id: header.root_block_id,
            class_order,
            enum_order,
        })
    }

    pub fn block(&self, block_id: i32) -> Option<&RbmDataBlock> {
        if block_id <= 0 {
            return None;
        }
        self.data_blocks.get((block_id - 1) as usize)
    }

    pub fn class_for(&self, name_hash: u32) -> Option<&RbmClass> {
        self.classes.get(&name_hash)
    }

    pub fn enum_for(&self, name_hash: u32) -> Option<&RbmEnum> {
        self.enums.get(&name_hash)
    }
}

const RBM_CLASS_HEADER_SIZE: u64 = 0x20;
const RBM_ENUM_HEADER_SIZE: u64 = 0x18;
const RBM_DATA_BLOCK_HEADER_SIZE: u64 = 0x10;
const RBM_CLASS_ENTRY_SIZE: u64 = 0x10;
const RBM_ENUM_VALUE_SIZE: u64 = 0x08;

fn read_class(rsc: &DatResource<'_>, addr: u64) -> Result<RbmClass> {
    let bytes = rsc
        .read_bytes_at(addr, RBM_CLASS_HEADER_SIZE as usize)
        .map_err(|e| MetaError::Other(format!("rbm class header: {e}")))?;
    let (mut class, entries_ptr, entries_count) = class_header(bytes)?;
    for i in 0..entries_count as u64 {
        let entry_addr = entries_ptr + i * RBM_CLASS_ENTRY_SIZE;
        class.entries.push(read_class_entry(rsc, entry_addr)?);
    }
    Ok(class)
}

/// A 32-byte class record with no members yet, its members' address and their count.
pub(crate) fn class_header(bytes: &[u8]) -> Result<(RbmClass, u64, u16)> {
    let mut cursor = std::io::Cursor::new(bytes);
    let name_hash = cursor.read_u32::<LittleEndian>()?;
    let signature = cursor.read_u32::<LittleEndian>()?;
    let flags = cursor.read_u8()?;
    let align_power = cursor.read_u8()?;
    let _pad = cursor.read_u48::<LittleEndian>()?;
    let entries_ptr = cursor.read_u64::<LittleEndian>()?;
    let data_size = cursor.read_u32::<LittleEndian>()?;
    let major_version = cursor.read_u16::<LittleEndian>()?;
    let entries_count = cursor.read_u16::<LittleEndian>()?;
    let class = RbmClass {
        name_hash,
        signature,
        flags,
        align_power,
        data_size,
        major_version,
        entries: Vec::with_capacity(entries_count as usize),
    };
    Ok((class, entries_ptr, entries_count))
}

fn read_class_entry(rsc: &DatResource<'_>, addr: u64) -> Result<RbmClassEntry> {
    let bytes = rsc
        .read_bytes_at(addr, RBM_CLASS_ENTRY_SIZE as usize)
        .map_err(|e| MetaError::Other(format!("rbm class entry: {e}")))?;
    class_entry(bytes)
}

/// A 16-byte member record.
pub(crate) fn class_entry(bytes: &[u8]) -> Result<RbmClassEntry> {
    let mut cursor = std::io::Cursor::new(bytes);
    let name_hash = cursor.read_u32::<LittleEndian>()?;
    let offset = cursor.read_u32::<LittleEndian>()?;
    let data_type = PsoType::from_byte(cursor.read_u8()?);
    let union_9h = cursor.read_u8()?;
    let union_ah = cursor.read_u16::<LittleEndian>()?;
    let union_ch = cursor.read_u32::<LittleEndian>()?;
    Ok(RbmClassEntry {
        name_hash,
        offset,
        data_type,
        union_9h,
        union_ah,
        union_ch,
    })
}

fn read_enum(rsc: &DatResource<'_>, addr: u64) -> Result<RbmEnum> {
    let bytes = rsc
        .read_bytes_at(addr, RBM_ENUM_HEADER_SIZE as usize)
        .map_err(|e| MetaError::Other(format!("rbm enum header: {e}")))?;
    let mut cursor = std::io::Cursor::new(bytes);
    let name_hash = cursor.read_u32::<LittleEndian>()?;
    let signature = cursor.read_u32::<LittleEndian>()?;
    let entries_ptr = cursor.read_u64::<LittleEndian>()?;
    let entries_count = cursor.read_u16::<LittleEndian>()?;
    let flags = cursor.read_u8()?;

    let mut values = Vec::with_capacity(entries_count as usize);
    for i in 0..entries_count as u64 {
        let item_addr = entries_ptr + i * RBM_ENUM_VALUE_SIZE;
        let value_bytes = rsc
            .read_bytes_at(item_addr, RBM_ENUM_VALUE_SIZE as usize)
            .map_err(|e| MetaError::Other(format!("rbm enum value: {e}")))?;
        let mut vc = std::io::Cursor::new(value_bytes);
        let v_name = vc.read_u32::<LittleEndian>()?;
        let v_value = vc.read_i32::<LittleEndian>()?;
        values.push(RbmEnumValue {
            name_hash: v_name,
            value: v_value,
        });
    }

    Ok(RbmEnum {
        name_hash,
        signature,
        flags,
        values,
    })
}

fn read_data_block(rsc: &DatResource<'_>, addr: u64) -> Result<RbmDataBlock> {
    let bytes = rsc
        .read_bytes_at(addr, RBM_DATA_BLOCK_HEADER_SIZE as usize)
        .map_err(|e| MetaError::Other(format!("rbm data-block header: {e}")))?;
    let mut cursor = std::io::Cursor::new(bytes);
    let data_type = cursor.read_u32::<LittleEndian>()?;
    let data_length = cursor.read_u32::<LittleEndian>()?;
    let data_ptr = cursor.read_u64::<LittleEndian>()?;
    let data = if data_length == 0 {
        Vec::new()
    } else {
        rsc.read_bytes_at(data_ptr, data_length as usize)
            .map_err(|e| MetaError::Other(format!("rbm data-block payload: {e}")))?
            .to_vec()
    };
    Ok(RbmDataBlock {
        data_type,
        data,
        data_addr: data_ptr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_bytes_round_trip() {
        let known: Vec<u8> = (0x00..=0x07)
            .chain(0x10..=0x17)
            .chain(0x20..=0x23)
            .chain(0x30..=0x38)
            .chain(0x40..=0x4d)
            .chain(0x50..=0x59)
            .chain(0x60..=0x67)
            .chain(0x70..=0x71)
            .collect();
        for b in known {
            let t = PsoType::from_byte(b);
            assert!(
                !matches!(t, PsoType::Unknown(_)),
                "type {b:#04X} should be known"
            );
            assert_eq!(t.to_byte(), b, "type {b:#04X} does not round-trip");
        }
        assert_eq!(PsoType::from_byte(0x08), PsoType::Unknown(0x08));
        assert_eq!(PsoType::from_byte(0x39), PsoType::Unknown(0x39));
    }

    #[test]
    fn array_align_and_flags_unpack() {
        let e = RbmClassEntry {
            name_hash: 0,
            offset: 0,
            data_type: PsoType::AtArray64,
            union_9h: (1 << 5) | 4,
            union_ah: 7,
            union_ch: 0,
        };
        assert_eq!(e.array_align_power(), 4);
        assert_eq!(e.array_flags(), 1);
        assert_eq!(e.contents_index(), 7);
    }
}
