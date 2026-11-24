//! Reading an RPF9 entry's body and mounting a nested archive from the decrypted table.
//!
//! An entry row states where the entry's bytes begin, relative to its archive's own header, and how
//! large they are ([`Gta6Entry`]). Two-thirds of a build's top-level entries are themselves RPF9
//! archives, whose 64-byte header is in the clear at that position; their table decrypts with the key
//! material already held, so a nested archive is opened by decrypting its table at `parent_base +
//! start`, recursively, with no scan and no new key. Reads are ranged — a top-level archive runs to
//! tens of gigabytes — so this takes a `Read + Seek` source and reads only a header, a table, or one
//! body at a time. It is read-only: nothing here writes an archive.

use std::io::{Read, Seek, SeekFrom};

use byteorder::{ByteOrder, LE};

use super::gta6_archive::{Gta6Archive, Gta6Entry};
use super::rpf9::{Rpf9Header, RPF9_HEADER_SIZE, RPF9_MAGIC};
use super::rpf9_scan::read_rpf9_body;
use super::rpf9_toc::GtaViKeys;
use crate::error::{IoError, Result};

/// `[offset, offset + len)` of `src`.
fn read_at<R: Read + Seek>(src: &mut R, offset: u64, len: usize) -> Result<Vec<u8>> {
    src.seek(SeekFrom::Start(offset)).map_err(mount_io)?;
    let mut buf = vec![0u8; len];
    src.read_exact(&mut buf).map_err(mount_io)?;
    Ok(buf)
}

fn mount_io(source: std::io::Error) -> IoError {
    IoError::Invalid {
        what: "reading an RPF9 archive".into(),
        why: source.to_string(),
    }
}

fn no_entry_row(entry: &Gta6Entry) -> IoError {
    IoError::Invalid {
        what: "an RPF9 entry".into(),
        why: format!(
            "entry {} has no entry row, so its start is unknown",
            entry.index
        ),
    }
}

/// Open the RPF9 archive whose header is at absolute `offset` in `src`, decrypting its table with
/// `keys`. The header states the table's end; only the header and table are read, never the content.
pub fn open_at<R: Read + Seek>(src: &mut R, offset: u64, keys: &GtaViKeys) -> Result<Gta6Archive> {
    let head = read_at(src, offset, RPF9_HEADER_SIZE)?;
    let header = Rpf9Header::parse(&head)?;
    let toc = read_at(src, offset, header.toc_end as usize)?;
    Gta6Archive::open(&toc, keys)
}

/// An opened RPF9 archive together with the absolute offset of its header, so its entries' starts —
/// stated relative to that header — resolve to file positions.
#[derive(Debug, Clone)]
pub struct Gta6Mount {
    /// The absolute offset of this archive's header in the file; 0 for the top-level archive.
    pub base: u64,
    pub archive: Gta6Archive,
}

impl Gta6Mount {
    /// Open the top-level archive at the start of `src`.
    pub fn open_root<R: Read + Seek>(src: &mut R, keys: &GtaViKeys) -> Result<Self> {
        Ok(Self {
            base: 0,
            archive: open_at(src, 0, keys)?,
        })
    }

    /// The absolute file offset of `entry`'s bytes, or `None` when it has no entry row.
    pub fn absolute_start(&self, entry: &Gta6Entry) -> Option<u64> {
        entry.start.map(|start| self.base + start)
    }

    /// Whether `entry`'s body begins with the clear `9FPR` header, i.e. it is a nested archive. An
    /// entry with no row is not an archive.
    pub fn entry_begins_archive<R: Read + Seek>(
        &self,
        src: &mut R,
        entry: &Gta6Entry,
    ) -> Result<bool> {
        let Some(start) = self.absolute_start(entry) else {
            return Ok(false);
        };
        let head = read_at(src, start, 4)?;
        Ok(LE::read_u32(&head) == RPF9_MAGIC)
    }

    /// Mount the nested archive `entry` points at, decrypting its table at `base + entry.start`. The
    /// returned mount carries that absolute base, so a deeper nesting resolves the same way.
    pub fn open_nested<R: Read + Seek>(
        &self,
        src: &mut R,
        entry: &Gta6Entry,
        keys: &GtaViKeys,
    ) -> Result<Self> {
        let start = self
            .absolute_start(entry)
            .ok_or_else(|| no_entry_row(entry))?;
        Ok(Self {
            base: start,
            archive: open_at(src, start, keys)?,
        })
    }

    /// Read `entry`'s body, inflated. [`read_rpf9_body`]'s detector reads the body it recognises at the
    /// row's start, capped by the row's real (inflated) size where it states one; a body the detector
    /// does not recognise at that start is an error. For a leaf body (a resource, a PSO, a movie); a
    /// nested archive is opened with [`Self::open_nested`], and an enciphered body
    /// (`entry.cipher.is_some()`) is not opened by this.
    pub fn read_entry<R: Read + Seek>(&self, src: R, entry: &Gta6Entry) -> Result<Vec<u8>> {
        let start = self
            .absolute_start(entry)
            .ok_or_else(|| no_entry_row(entry))?;
        // The inflated size, where the row states it, caps the inflated output; a 20-byte row states
        // none, so the detector's own measure of the body stands.
        let limit = entry.real.unwrap_or(u64::MAX);
        read_rpf9_body(src, start, limit)
            .map(|(_, bytes)| bytes)
            .map_err(|source| IoError::Invalid {
                what: "an RPF9 entry body".into(),
                why: source.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A nested archive's header must be found and parsed at `base + start` from the entry row. This
    /// builds a tiny two-level file in memory: a byte run, then a minimal valid RPF9 header, and
    /// checks that an entry whose start points at the header is seen as an archive at the right place.
    #[test]
    fn an_entry_start_locates_a_nested_header() {
        let mut file = vec![0u8; 0x200];
        // A plausible RPF9 magic at 0x100; `entry_begins_archive` only reads the 4-byte magic.
        file[0x100..0x104].copy_from_slice(&RPF9_MAGIC.to_le_bytes());
        let mut src = Cursor::new(file);

        let entry = Gta6Entry {
            index: 0,
            name64: 0,
            name32: None,
            name: None,
            start: Some(0x100),
            stored: Some(0x40),
            real: Some(0x40),
            class: None,
            cipher: None,
        };
        let mount = Gta6Mount {
            base: 0,
            archive: Gta6Archive {
                toc: minimal_toc(),
                entries: vec![],
            },
        };
        assert_eq!(mount.absolute_start(&entry), Some(0x100));
        assert!(mount.entry_begins_archive(&mut src, &entry).unwrap());

        // With a non-zero base the start is taken relative to the nested header.
        let nested = Gta6Mount {
            base: 0x40,
            ..mount.clone()
        };
        let child = Gta6Entry {
            start: Some(0xC0),
            ..entry.clone()
        };
        assert_eq!(nested.absolute_start(&child), Some(0x100));
        assert!(nested.entry_begins_archive(&mut src, &child).unwrap());
    }

    #[test]
    fn an_entry_without_a_row_has_no_start_and_is_not_an_archive() {
        let mut src = Cursor::new(vec![0u8; 0x80]);
        let mount = Gta6Mount {
            base: 0,
            archive: Gta6Archive {
                toc: minimal_toc(),
                entries: vec![],
            },
        };
        let rowless = Gta6Entry {
            index: 7,
            name64: 0,
            name32: None,
            name: None,
            start: None,
            stored: None,
            real: None,
            class: None,
            cipher: None,
        };
        // With no entry row, nothing locates the entry: `open_nested` and `read_entry` would both
        // refuse here (they call `absolute_start` first), so this is their error gate.
        assert_eq!(mount.absolute_start(&rowless), None);
        assert!(!mount.entry_begins_archive(&mut src, &rowless).unwrap());
    }

    fn minimal_toc() -> super::super::rpf9_toc::Rpf9Toc {
        super::super::rpf9_toc::Rpf9Toc {
            bytes: vec![0u8; 0x40],
            key_id: 0,
            registry_id: 0,
            entry_count: 0,
            groups: vec![],
        }
    }
}
