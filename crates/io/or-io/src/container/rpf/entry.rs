use std::io::{Read, Write};

use crate::container::resource::{DatResourceInfo, ResourceEnvelope, Rsc7};
use crate::decl::*;
use crate::error::{IoError, Result};

use super::header::{
    RPF_DIRECTORY_OFFSET_MARKER, RPF_FILE_ALIGNMENT_SHIFT, RPF_MAX_RESOURCE_CONSUMED_SIZE,
};

pub const RPF_ENTRY_SIZE: usize = 16;

const NAME_OFFSET_MASK: u64 = 0xFFFF;
const CONSUMED_SIZE_MASK: u64 = (1 << 24) - 1;
const FILE_OFFSET_MASK: u64 = (1 << 23) - 1;

/// A body's offset as RPF7's count of 512-byte blocks, which the entry holds in 23 bits.
fn block_of(file_offset: u64) -> Result<u32> {
    let blocks = file_offset >> RPF_FILE_ALIGNMENT_SHIFT;
    if blocks > FILE_OFFSET_MASK {
        return Err(IoError::RpfBadEntry {
            message: format!("file_offset {file_offset:#x} doesn't fit in 23 bits"),
        });
    }
    Ok(blocks as u32)
}

/// One sixteen-byte entry of an RPF7 archive's table: a directory, a file or a resource.
#[record(size = 0x10)]
pub struct FiPackEntry {
    // The engine leaves this bit-field word unnamed, so it takes its four ranges' names.
    #[bits(name_offset, 0, 16, "shifted left by the header's name shift")]
    #[bits(consumed_size, 16, 24)]
    #[bits(file_offset, 40, 23, "in 512-byte blocks")]
    #[bits(is_resource, 63, 1)]
    name_offset_consumed_size_file_offset_is_resource: u64,
    #[select(is_resource = 1 => Resource, file_offset = 0x7F_FFFF => Directory, _ => File)]
    pub u: FiPackEntryU,
}

/// An entry's last eight bytes, laid out as its kind's arm.
#[record(union, size = 0x08)]
pub enum FiPackEntryU {
    File(FiPackEntryFile),
    Directory(FiPackEntryDirectory),
    Resource(FiPackEntryResource),
}

#[record(size = 0x08)]
pub struct FiPackEntryFile {
    pub uncompressed_size: u32,
    pub encrypted: u32,
}

#[record(size = 0x08)]
pub struct FiPackEntryDirectory {
    pub directory_index: u32,
    directory_count: u32,
}

#[record(size = 0x08)]
pub struct FiPackEntryResource {
    pub info: DatResourceInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpfEntryTag {
    pub name_offset: u32,
    pub consumed_size: u32,
    pub file_offset_raw: u32,
    pub is_resource: bool,
}

impl RpfEntryTag {
    pub fn of(block: &[u8; RPF_ENTRY_SIZE]) -> Self {
        let packed = Raw::<FiPackEntry>::read_from(block)
            .expect("sixteen bytes")
            .name_offset_consumed_size_file_offset_is_resource;
        Self {
            name_offset: (packed & NAME_OFFSET_MASK) as u32,
            consumed_size: ((packed >> 16) & CONSUMED_SIZE_MASK) as u32,
            file_offset_raw: ((packed >> 40) & FILE_OFFSET_MASK) as u32,
            is_resource: (packed >> 63) & 1 != 0,
        }
    }

    pub fn is_directory(self) -> bool {
        self.file_offset_raw == RPF_DIRECTORY_OFFSET_MARKER && !self.is_resource
    }
}

// A resource whose consumed size saturates at `RPF_MAX_RESOURCE_CONSUMED_SIZE`
// hides its real size in these bytes of its 16-byte header slot, most
// significant first; the loader overwrites the slot with the real RSC7 header
// on extraction.
const HIDDEN_SIZE_BYTES: [usize; 4] = [2, 5, 14, 7];

pub fn read_big_resource_size(slot: &[u8; Rsc7::HEADER_LEN]) -> u32 {
    let mut size = 0u32;
    for (i, &at) in HIDDEN_SIZE_BYTES.iter().enumerate() {
        let shift = 8 * (HIDDEN_SIZE_BYTES.len() - 1 - i) as u32;
        size |= (slot[at] as u32) << shift;
    }
    size
}

pub fn hide_big_resource_size(slot: &mut [u8; Rsc7::HEADER_LEN], size: u32) {
    for (i, &at) in HIDDEN_SIZE_BYTES.iter().enumerate() {
        let shift = 8 * (HIDDEN_SIZE_BYTES.len() - 1 - i) as u32;
        slot[at] = (size >> shift) as u8;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpfEntry {
    Directory(RpfDirectoryEntry),
    Binary(RpfBinaryEntry),
    Resource(RpfResourceEntry),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RpfEntryKind {
    Directory,
    Binary,
    Resource,
}

impl RpfEntry {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let mut block = [0u8; RPF_ENTRY_SIZE];
        reader.read_exact(&mut block)?;
        let lo = u32::from_le_bytes(block[8..12].try_into().expect("4 bytes"));
        let hi = u32::from_le_bytes(block[12..16].try_into().expect("4 bytes"));

        let RpfEntryTag {
            name_offset,
            consumed_size,
            file_offset_raw,
            is_resource,
        } = RpfEntryTag::of(&block);

        if file_offset_raw == RPF_DIRECTORY_OFFSET_MARKER && !is_resource {
            return Ok(RpfEntry::Directory(RpfDirectoryEntry {
                name_offset,
                first_entry_index: lo,
                entry_count: hi,
            }));
        }

        let file_offset = u64::from(file_offset_raw) << RPF_FILE_ALIGNMENT_SHIFT;

        if is_resource {
            Ok(RpfEntry::Resource(RpfResourceEntry {
                name_offset,
                consumed_size,
                file_offset,
                system_info: lo,
                graphics_info: hi,
                compressor: Compressor::None,
                tag: 0,
            }))
        } else {
            let encrypted = hi != 0;
            Ok(RpfEntry::Binary(RpfBinaryEntry {
                name_offset,
                consumed_size,
                file_offset,
                uncompressed_size: lo,
                encrypted,
                encrypted_raw: hi,
                compressor: if consumed_size != 0 {
                    Compressor::Deflate
                } else {
                    Compressor::None
                },
                tag: 0,
            }))
        }
    }

    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        let (name_offset, consumed_size, file_offset_raw, is_resource, lo, hi) = match self {
            RpfEntry::Directory(d) => (
                d.name_offset,
                0u32,
                RPF_DIRECTORY_OFFSET_MARKER,
                false,
                d.first_entry_index,
                d.entry_count,
            ),
            RpfEntry::Binary(b) => {
                let off = block_of(b.file_offset)?;
                if b.consumed_size > RPF_MAX_RESOURCE_CONSUMED_SIZE {
                    return Err(IoError::RpfBadEntry {
                        message: format!("consumed_size {} exceeds 24-bit max", b.consumed_size),
                    });
                }
                (
                    b.name_offset,
                    b.consumed_size,
                    off,
                    false,
                    b.uncompressed_size,
                    if b.encrypted_raw != 0 {
                        b.encrypted_raw
                    } else if b.encrypted {
                        1
                    } else {
                        0
                    },
                )
            }
            RpfEntry::Resource(r) => (
                r.name_offset,
                r.consumed_size,
                block_of(r.file_offset)?,
                true,
                r.system_info,
                r.graphics_info,
            ),
        };

        if name_offset > NAME_OFFSET_MASK as u32 {
            return Err(IoError::RpfBadEntry {
                message: format!("name_offset {} exceeds 16-bit max", name_offset),
            });
        }

        let mut packed: u64 = 0;
        packed |= (name_offset as u64) & NAME_OFFSET_MASK;
        packed |= ((consumed_size as u64) & CONSUMED_SIZE_MASK) << 16;
        packed |= ((file_offset_raw as u64) & FILE_OFFSET_MASK) << 40;
        if is_resource {
            packed |= 1 << 63;
        }

        let mut raw = <Raw<FiPackEntry> as Plain>::zeroed();
        raw.name_offset_consumed_size_file_offset_is_resource = packed;
        match self {
            RpfEntry::Directory(_) => {
                let mut arm = <Raw<FiPackEntryDirectory> as Plain>::zeroed();
                (arm.directory_index, arm.directory_count) = (lo, hi);
                raw.u.directory = arm;
            }
            RpfEntry::Binary(_) => {
                let mut arm = <Raw<FiPackEntryFile> as Plain>::zeroed();
                (arm.uncompressed_size, arm.encrypted) = (lo, hi);
                raw.u.file = arm;
            }
            RpfEntry::Resource(_) => {
                let mut arm = <Raw<FiPackEntryResource> as Plain>::zeroed();
                (arm.info.r#virtual, arm.info.physical) = (lo, hi);
                raw.u.resource = arm;
            }
        }
        writer.write_all(Plain::as_bytes(&raw))?;
        Ok(())
    }

    pub fn kind(&self) -> RpfEntryKind {
        match self {
            RpfEntry::Directory(_) => RpfEntryKind::Directory,
            RpfEntry::Binary(_) => RpfEntryKind::Binary,
            RpfEntry::Resource(_) => RpfEntryKind::Resource,
        }
    }

    pub fn name_offset(&self) -> u32 {
        match self {
            RpfEntry::Directory(d) => d.name_offset,
            RpfEntry::Binary(b) => b.name_offset,
            RpfEntry::Resource(r) => r.name_offset,
        }
    }

    pub fn set_name_offset(&mut self, offset: u32) {
        match self {
            RpfEntry::Directory(d) => d.name_offset = offset,
            RpfEntry::Binary(b) => b.name_offset = offset,
            RpfEntry::Resource(r) => r.name_offset = offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpfDirectoryEntry {
    pub name_offset: u32,
    pub first_entry_index: u32,
    pub entry_count: u32,
}

/// How a binary body is compressed on disk; `consumed_size` is its stored size when it is, and zero when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compressor {
    #[default]
    None,
    Deflate,
    /// RPF8's Kraken: decoded with the `oodle` feature, never encoded, and carried as stored when an archive is rebuilt.
    Oodle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpfBinaryEntry {
    pub name_offset: u32,
    pub consumed_size: u32,
    pub file_offset: u64,
    pub uncompressed_size: u32,
    pub encrypted: bool,
    pub encrypted_raw: u32,
    pub compressor: Compressor,
    /// The row bits no other field names, per [`RpfVersion`](super::RpfVersion): RPF8's key id, config and signature flag.
    pub tag: u32,
}

impl RpfBinaryEntry {
    pub fn read_size(&self) -> u32 {
        if self.consumed_size != 0 {
            self.consumed_size
        } else {
            self.uncompressed_size
        }
    }

    pub fn is_compressed(&self) -> bool {
        self.consumed_size != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpfResourceEntry {
    pub name_offset: u32,
    pub consumed_size: u32,
    pub file_offset: u64,
    pub system_info: u32,
    pub graphics_info: u32,
    /// How the payload is compressed inside the resource, which only RPF8 says; the container never inflates it.
    pub compressor: Compressor,
    /// The row bits no other field names, per [`RpfVersion`](super::RpfVersion): a resource's version before RPF7, RPF8's key id, config and signature flag.
    pub tag: u32,
}

impl RpfResourceEntry {
    pub fn is_big_resource(&self) -> bool {
        self.consumed_size == RPF_MAX_RESOURCE_CONSUMED_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn the_tag_decodes_every_field_from_non_degenerate_values() {
        const NAME_OFFSET: u32 = 0xBEEF;
        const CONSUMED_SIZE: u32 = 0xA5_A5A5;
        const FILE_OFFSET_RAW: u32 = 0x5A_5A5A;

        let packed = (NAME_OFFSET as u64)
            | ((CONSUMED_SIZE as u64) << 16)
            | ((FILE_OFFSET_RAW as u64) << 40)
            | (1u64 << 63);
        let mut block = [0u8; RPF_ENTRY_SIZE];
        block[0..8].copy_from_slice(&packed.to_le_bytes());
        block[8..12].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        block[12..16].copy_from_slice(&0x9ABC_DEF0u32.to_le_bytes());

        let tag = RpfEntryTag::of(&block);
        assert_eq!(tag.name_offset, NAME_OFFSET);
        assert_eq!(tag.consumed_size, CONSUMED_SIZE);
        assert_eq!(tag.file_offset_raw, FILE_OFFSET_RAW);
        assert!(tag.is_resource);
        assert!(!tag.is_directory(), "a resource is never a directory");

        assert_ne!(tag.name_offset, tag.consumed_size & 0xFFFF);
        assert_ne!(tag.consumed_size, tag.file_offset_raw);
    }

    #[test]
    fn the_directory_marker_alone_is_not_a_directory() {
        let mut block = [0u8; RPF_ENTRY_SIZE];
        let marker = (RPF_DIRECTORY_OFFSET_MARKER as u64) << 40;

        block[0..8].copy_from_slice(&marker.to_le_bytes());
        assert!(RpfEntryTag::of(&block).is_directory());

        block[0..8].copy_from_slice(&(marker | (1u64 << 63)).to_le_bytes());
        let tag = RpfEntryTag::of(&block);
        assert_eq!(tag.file_offset_raw, RPF_DIRECTORY_OFFSET_MARKER);
        assert!(tag.is_resource);
        assert!(!tag.is_directory());
    }

    #[test]
    fn a_hidden_resource_size_survives_the_round_trip() {
        const SIZE: u32 = 0x01_23_45_67;

        let mut slot = [0xCDu8; Rsc7::HEADER_LEN];
        hide_big_resource_size(&mut slot, SIZE);
        assert_eq!(read_big_resource_size(&slot), SIZE);

        assert_eq!(slot[2], 0x01, "most significant byte");
        assert_eq!(slot[5], 0x23);
        assert_eq!(slot[14], 0x45);
        assert_eq!(slot[7], 0x67, "least significant byte");

        let untouched = (0..Rsc7::HEADER_LEN)
            .filter(|i| !matches!(i, 2 | 5 | 7 | 14))
            .filter(|&i| slot[i] == 0xCD)
            .count();
        assert_eq!(untouched, Rsc7::HEADER_LEN - 4);
    }

    #[test]
    fn the_hidden_size_covers_the_range_the_field_cannot() {
        for size in [
            RPF_MAX_RESOURCE_CONSUMED_SIZE,
            RPF_MAX_RESOURCE_CONSUMED_SIZE + 1,
            0x1000_0000,
            u32::MAX,
        ] {
            let mut slot = [0u8; Rsc7::HEADER_LEN];
            hide_big_resource_size(&mut slot, size);
            assert_eq!(read_big_resource_size(&slot), size, "size {size:#x}");
        }
    }

    #[test]
    fn the_tag_agrees_with_the_typed_reader() {
        let cases = [
            RpfEntry::Directory(RpfDirectoryEntry {
                name_offset: 0x0102,
                first_entry_index: 1,
                entry_count: 7,
            }),
            RpfEntry::Binary(RpfBinaryEntry {
                name_offset: 0x0304,
                consumed_size: 0x1234,
                file_offset: 0x5A_5A5A << RPF_FILE_ALIGNMENT_SHIFT,
                uncompressed_size: 0x9999,
                encrypted: true,
                encrypted_raw: 1,
                compressor: Compressor::Deflate,
                tag: 0,
            }),
            RpfEntry::Resource(RpfResourceEntry {
                name_offset: 0x0506,
                consumed_size: 0x4321,
                file_offset: 0x12_3456 << RPF_FILE_ALIGNMENT_SHIFT,
                system_info: 0xAAAA_AAAA,
                graphics_info: 0xBBBB_BBBB,
                compressor: Compressor::None,
                tag: 0,
            }),
        ];

        for case in cases {
            let mut buf = Vec::new();
            case.write(&mut buf).expect("writes");
            let block: [u8; RPF_ENTRY_SIZE] = buf.as_slice().try_into().expect("16 bytes");
            let tag = RpfEntryTag::of(&block);
            let parsed = RpfEntry::read(&mut Cursor::new(&buf)).expect("reads");

            assert_eq!(parsed, case, "round trip");
            match &case {
                RpfEntry::Directory(d) => {
                    assert!(tag.is_directory());
                    assert_eq!(tag.name_offset, d.name_offset);
                }
                RpfEntry::Binary(b) => {
                    assert!(!tag.is_directory());
                    assert!(!tag.is_resource);
                    assert_eq!(tag.name_offset, b.name_offset);
                    assert_eq!(tag.consumed_size, b.consumed_size);
                    assert_eq!(
                        u64::from(tag.file_offset_raw) << RPF_FILE_ALIGNMENT_SHIFT,
                        b.file_offset
                    );
                }
                RpfEntry::Resource(r) => {
                    assert!(tag.is_resource);
                    assert!(!tag.is_directory());
                    assert_eq!(tag.name_offset, r.name_offset);
                    assert_eq!(tag.consumed_size, r.consumed_size);
                    assert_eq!(
                        u64::from(tag.file_offset_raw) << RPF_FILE_ALIGNMENT_SHIFT,
                        r.file_offset
                    );
                }
            }
        }
    }

    fn roundtrip(e: RpfEntry) -> RpfEntry {
        let mut buf = Vec::new();
        e.write(&mut buf).unwrap();
        assert_eq!(buf.len(), RPF_ENTRY_SIZE);
        let mut cursor = Cursor::new(&buf);
        RpfEntry::read(&mut cursor).unwrap()
    }

    #[test]
    fn directory_roundtrip() {
        let e = RpfEntry::Directory(RpfDirectoryEntry {
            name_offset: 0x40,
            first_entry_index: 7,
            entry_count: 12,
        });
        assert_eq!(roundtrip(e.clone()), e);
    }

    #[test]
    fn binary_compressed_roundtrip() {
        let e = RpfEntry::Binary(RpfBinaryEntry {
            name_offset: 0x10,
            consumed_size: 0x1234,
            file_offset: 0x4_0000,
            uncompressed_size: 0x9876,
            encrypted: true,
            encrypted_raw: 1,
            compressor: Compressor::Deflate,
            tag: 0,
        });
        assert_eq!(roundtrip(e.clone()), e);
    }

    #[test]
    fn binary_uncompressed_roundtrip() {
        let e = RpfEntry::Binary(RpfBinaryEntry {
            name_offset: 0x10,
            consumed_size: 0,
            file_offset: 0x4_0000,
            uncompressed_size: 9999,
            encrypted: false,
            encrypted_raw: 0,
            compressor: Compressor::None,
            tag: 0,
        });
        assert_eq!(roundtrip(e.clone()), e);
    }

    #[test]
    fn resource_roundtrip() {
        let e = RpfEntry::Resource(RpfResourceEntry {
            name_offset: 0x80,
            consumed_size: 0xABCDE,
            file_offset: 0x10_0000,
            system_info: 0x9000_0001,
            graphics_info: 0xA000_0002,
            compressor: Compressor::None,
            tag: 0,
        });
        assert_eq!(roundtrip(e.clone()), e);
    }

    #[test]
    fn big_resource_roundtrip() {
        let e = RpfEntry::Resource(RpfResourceEntry {
            name_offset: 0xAA,
            consumed_size: RPF_MAX_RESOURCE_CONSUMED_SIZE,
            file_offset: 0x20_0000,
            system_info: 0,
            graphics_info: 0,
            compressor: Compressor::None,
            tag: 0,
        });
        let r = roundtrip(e.clone());
        assert_eq!(r, e);
        if let RpfEntry::Resource(r) = r {
            assert!(r.is_big_resource());
        } else {
            panic!("expected resource");
        }
    }

    #[test]
    fn rejects_unaligned_offset() {
        let e = RpfEntry::Binary(RpfBinaryEntry {
            name_offset: 0,
            consumed_size: 1,
            file_offset: 0x4_0001,
            uncompressed_size: 1,
            encrypted: false,
            encrypted_raw: 0,
            compressor: Compressor::Deflate,
            tag: 0,
        });
        let mut buf = Vec::new();
        e.write(&mut buf).unwrap();
        let parsed = RpfEntry::read(&mut Cursor::new(&buf)).unwrap();
        if let RpfEntry::Binary(b) = parsed {
            assert_eq!(b.file_offset & ((1 << 9) - 1), 0);
        } else {
            panic!("expected binary");
        }
    }
}
