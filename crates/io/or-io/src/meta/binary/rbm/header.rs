use crate::container::paging::DatResource;

use crate::meta::binary::RBM_PRD0_MARKER_LE;
use crate::meta::error::{MetaError, Result};

pub const RBM_PRD0_MARKER: u32 = 0x50524430;

pub const RBM_RESOURCE_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct RbmHeader {
    pub vft: u64,
    pub block_map_ptr: u64,
    pub unknown_10h: u32,
    pub unknown_14h: u16,
    pub has_encrypted_strings: u8,
    pub unknown_17h: u8,
    pub unknown_18h: u32,
    pub root_block_id: u32,
    pub class_refs_ptr: u64,
    pub enum_refs_ptr: u64,
    pub data_blocks_ptr: u64,
    pub name_ptr: u64,
    pub encrypted_strings_ptr: u64,
    pub class_count: u16,
    pub enum_count: u16,
    pub data_block_count: u16,
    pub unknown_4eh: u16,
    pub unknown_50h: [u32; 8],
}

impl RbmHeader {
    pub fn read(rsc: &DatResource<'_>, virtual_base: u64) -> Result<Self> {
        let bytes = rsc
            .read_bytes_at(virtual_base, 112)
            .map_err(|e| MetaError::Other(format!("rbm header: {e}")))?;
        let mut cursor = std::io::Cursor::new(bytes);
        use byteorder::{LittleEndian, ReadBytesExt};

        let vft = cursor.read_u64::<LittleEndian>()?;
        let block_map_ptr = cursor.read_u64::<LittleEndian>()?;
        let unknown_10h = cursor.read_u32::<LittleEndian>()?;
        if unknown_10h != RBM_PRD0_MARKER {
            return Err(MetaError::Other(format!(
                "rbm header: PRD0 sentinel mismatch (got {:#X}, expected {:#X})",
                unknown_10h, RBM_PRD0_MARKER
            )));
        }
        let unknown_14h = cursor.read_u16::<LittleEndian>()?;
        let has_encrypted_strings = cursor.read_u8()?;
        let unknown_17h = cursor.read_u8()?;
        let unknown_18h = cursor.read_u32::<LittleEndian>()?;
        let root_block_id = cursor.read_u32::<LittleEndian>()?;
        let class_refs_ptr = cursor.read_u64::<LittleEndian>()?;
        let enum_refs_ptr = cursor.read_u64::<LittleEndian>()?;
        let data_blocks_ptr = cursor.read_u64::<LittleEndian>()?;
        let name_ptr = cursor.read_u64::<LittleEndian>()?;
        let encrypted_strings_ptr = cursor.read_u64::<LittleEndian>()?;
        let class_count = cursor.read_u16::<LittleEndian>()?;
        let enum_count = cursor.read_u16::<LittleEndian>()?;
        let data_block_count = cursor.read_u16::<LittleEndian>()?;
        let unknown_4eh = cursor.read_u16::<LittleEndian>()?;
        let mut unknown_50h = [0u32; 8];
        for slot in &mut unknown_50h {
            *slot = cursor.read_u32::<LittleEndian>()?;
        }

        Ok(Self {
            vft,
            block_map_ptr,
            unknown_10h,
            unknown_14h,
            has_encrypted_strings,
            unknown_17h,
            unknown_18h,
            root_block_id,
            class_refs_ptr,
            enum_refs_ptr,
            data_blocks_ptr,
            name_ptr,
            encrypted_strings_ptr,
            class_count,
            enum_count,
            data_block_count,
            unknown_4eh,
            unknown_50h,
        })
    }
}

pub fn looks_like_rbm(decompressed: &[u8]) -> bool {
    decompressed
        .get(0x10..0x14)
        .map(|s| s == RBM_PRD0_MARKER_LE)
        .unwrap_or(false)
}
