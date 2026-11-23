//! RDR2's resource container: a sixteen-byte header, then one body of virtual bytes followed by
//! physical bytes, compressed as the header says.
//!
//! The layout is CodeX's (`Rpf8File.cs` for the header, `Rsc8Data.cs` for the pointers). A header is
//! the magic, a word that holds the compressor at bit 8 (less one, in five bits) and the key id
//! at bit 16 (plus one), and two flag words whose bits above the low four are the virtual and the
//! physical size. A pointer is `0x5000_0000` plus an offset into the virtual bytes, or `0x6000_0000`
//! plus an offset into the physical bytes, which follow them.

use byteorder::{ByteOrder, LE};

use super::version::RSC8_MAGIC;
use crate::error::{IoError, Result};

const VIRTUAL_BASE: u64 = 0x5000_0000;
const PHYSICAL_BASE: u64 = 0x6000_0000;

fn bad(why: impl Into<String>) -> IoError {
    IoError::Invalid {
        what: "an RSC8 resource".into(),
        why: why.into(),
    }
}

/// A resource, its body decompressed: virtual bytes, then physical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rsc8 {
    version: u32,
    virtual_flags: u32,
    physical_flags: u32,
    data: Vec<u8>,
}

impl Rsc8 {
    pub const HEADER_LEN: usize = 16;

    pub fn detect(bytes: &[u8]) -> bool {
        bytes.len() >= 4 && LE::read_u32(bytes) == RSC8_MAGIC
    }

    /// A resource file: its header and its body, whose key (if any) is already off.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        if !Self::detect(bytes) || bytes.len() < Self::HEADER_LEN {
            return Err(bad("it does not begin with the RSC8 header"));
        }
        let version = LE::read_u32(&bytes[4..]);
        let (virtual_flags, physical_flags) =
            (LE::read_u32(&bytes[8..]), LE::read_u32(&bytes[12..]));
        let total = (virtual_flags & !0xF) as usize + (physical_flags & !0xF) as usize;
        let body = &bytes[Self::HEADER_LEN..];
        let data = match (version >> 8) & 0x1F {
            // Stored: the body may be padded past the size the flags state.
            0x1F => body
                .get(..total)
                .ok_or_else(|| bad(format!("{total} bytes stated, {} stored", body.len())))?
                .to_vec(),
            0 => crate::container::resource::dat_decompress_bounded(body, total)?,
            1 => super::oodle::decompress(body, total)?,
            other => return Err(bad(format!("compressor {other} is not one RSC8 has"))),
        };
        if data.len() != total {
            return Err(bad(format!(
                "the flags state {total} bytes, the body holds {}",
                data.len()
            )));
        }
        Ok(Self {
            version,
            virtual_flags,
            physical_flags,
            data,
        })
    }

    /// The resource id, which names its type: the low byte of the version word.
    pub fn resource_id(&self) -> u8 {
        self.version as u8
    }

    pub fn virtual_size(&self) -> usize {
        (self.virtual_flags & !0xF) as usize
    }

    pub fn physical_size(&self) -> usize {
        (self.physical_flags & !0xF) as usize
    }

    /// Virtual bytes, then physical bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Where in [`data`](Self::data) a pointer lands, or none for a null or foreign one.
    pub fn offset_of(&self, pointer: u64) -> Option<usize> {
        let (virtual_size, physical_size) =
            (self.virtual_size() as u64, self.physical_size() as u64);
        match pointer & 0xF000_0000 {
            VIRTUAL_BASE => {
                let at = pointer & 0x0FFF_FFFF;
                (at < virtual_size).then_some(at as usize)
            }
            PHYSICAL_BASE => {
                let at = pointer & 0x1FFF_FFFF;
                (at < physical_size).then_some((at + virtual_size) as usize)
            }
            _ => None,
        }
    }

    /// `len` bytes at a pointer, if they all lie in the resource.
    pub fn bytes_at(&self, pointer: u64, len: usize) -> Option<&[u8]> {
        let at = self.offset_of(pointer)?;
        self.data.get(at..at.checked_add(len)?)
    }

    /// The little-endian `u16` at a pointer.
    pub fn u16_at(&self, pointer: u64) -> Result<u16> {
        self.bytes_at(pointer, 2)
            .map(LE::read_u16)
            .ok_or_else(|| self.outside(pointer))
    }

    /// The little-endian `u32` at a pointer.
    pub fn u32_at(&self, pointer: u64) -> Result<u32> {
        self.bytes_at(pointer, 4)
            .map(LE::read_u32)
            .ok_or_else(|| self.outside(pointer))
    }

    /// The little-endian `u64` at a pointer.
    pub fn u64_at(&self, pointer: u64) -> Result<u64> {
        self.bytes_at(pointer, 8)
            .map(LE::read_u64)
            .ok_or_else(|| self.outside(pointer))
    }

    /// The null-terminated text at a pointer.
    pub fn string_at(&self, pointer: u64) -> Result<String> {
        let at = self
            .offset_of(pointer)
            .ok_or_else(|| self.outside(pointer))?;
        let rest = &self.data[at..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    fn outside(&self, pointer: u64) -> IoError {
        bad(format!("pointer {pointer:#x} lies outside the resource"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(version: u32, v: u32, p: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [RSC8_MAGIC, version, v, p] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_stored_body_is_virtual_bytes_then_physical_bytes() {
        let body: Vec<u8> = (0..48u8).collect();
        let rsc = Rsc8::open(&file(0x1F << 8 | 0x1D, 32, 16, &body)).expect("opens");
        assert_eq!((rsc.virtual_size(), rsc.physical_size()), (32, 16));
        assert_eq!(rsc.resource_id(), 0x1D);
        assert_eq!(rsc.bytes_at(0x5000_0004, 2), Some(&[4u8, 5][..]));
        assert_eq!(rsc.bytes_at(0x6000_0000, 2), Some(&[32u8, 33][..]));
        assert_eq!(rsc.bytes_at(0x6000_0010, 1), None);
        assert_eq!(rsc.offset_of(0), None);
    }

    #[test]
    fn an_oodle_body_inflates_to_the_size_the_flags_state() {
        let plain: Vec<u8> = b"abcdefgh".iter().cycle().take(4096).copied().collect();
        let packed = super::super::oodle::compress(&plain);
        let rsc = Rsc8::open(&file(1 << 8, 4096, 0, &packed)).expect("opens");
        assert_eq!(rsc.data(), &plain[..]);
    }

    #[test]
    fn a_body_that_does_not_match_its_flags_is_refused() {
        assert!(Rsc8::open(&file(0x1F << 8, 64, 0, &[0; 8])).is_err());
        assert!(Rsc8::open(&file(5 << 8, 64, 0, &[0; 64])).is_err());
        assert!(Rsc8::open(b"RSC7").is_err());
    }
}
