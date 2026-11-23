use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt};

use crate::meta::error::{MetaError, Result};

/// The section GTA VI's table form has second, where every other PSO has `PMAP`.
pub(crate) const LSCH_IDENT: u32 = u32::from_be_bytes(*b"LSCH");

/// Whether a `CHKS` section, frame included, carries GTA VI's tag `zppp`; GTA V and RDR2 carry `yppp`.
pub(crate) fn chks_states_gta6(chks: &[u8]) -> bool {
    chks.get(16..20) == Some(b"zppp")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PsoSection {
    Psin,
    Pmap,
    Psch,
    Strf,
    Strs,
    Stre,
    Psig,
    Chks,
    Other(u32),
}

impl PsoSection {
    pub(crate) fn from_ident(ident: u32) -> Self {
        match ident {
            0x5053494E => PsoSection::Psin,
            0x504D4150 => PsoSection::Pmap,
            0x50534348 => PsoSection::Psch,
            0x53545246 => PsoSection::Strf,
            0x53545253 => PsoSection::Strs,
            0x53545245 => PsoSection::Stre,
            0x50534947 => PsoSection::Psig,
            0x43484B53 => PsoSection::Chks,
            other => PsoSection::Other(other),
        }
    }

    pub(crate) fn ident(self) -> u32 {
        match self {
            PsoSection::Psin => 0x5053494E,
            PsoSection::Pmap => 0x504D4150,
            PsoSection::Psch => 0x50534348,
            PsoSection::Strf => 0x53545246,
            PsoSection::Strs => 0x53545253,
            PsoSection::Stre => 0x53545245,
            PsoSection::Psig => 0x50534947,
            PsoSection::Chks => 0x43484B53,
            PsoSection::Other(v) => v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RawPsoSection {
    pub ident: PsoSection,
    pub bytes: Vec<u8>,
}

pub fn split_sections(bytes: &[u8]) -> Result<Vec<RawPsoSection>> {
    let mut out = Vec::new();
    let mut cursor = Cursor::new(bytes);
    while (cursor.position() as usize) < bytes.len() {
        let ident = cursor
            .read_u32::<BigEndian>()
            .map_err(|e| MetaError::Other(format!("pso: section ident: {e}")))?;
        let length = cursor
            .read_i32::<BigEndian>()
            .map_err(|e| MetaError::Other(format!("pso: section length: {e}")))?;
        if length < 8 {
            return Err(MetaError::Other(format!(
                "pso: section length {length} too small"
            )));
        }
        let start = cursor.position() as usize - 8;
        let end = start + length as usize;
        if end > bytes.len() {
            return Err(MetaError::Other(format!(
                "pso: section {:#X} overruns ({} > {})",
                ident,
                end,
                bytes.len()
            )));
        }
        out.push(RawPsoSection {
            ident: PsoSection::from_ident(ident),
            bytes: bytes[start..end].to_vec(),
        });
        cursor.set_position(end as u64);
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmapHeaderKind {
    Legacy {
        unk_0eh: i16,
    },
    Extended {
        unk_0ch: i16,
        unk_0eh: i16,
        unk_12h: i16,
        unk_14h: i16,
        unk_16h: i16,
    },
}

#[derive(Debug, Clone)]
pub struct PmapSection {
    pub root_id: i32,
    pub header_kind: PmapHeaderKind,
    pub entries: Vec<PmapEntry>,
}

#[derive(Debug, Clone)]
pub struct PmapEntry {
    pub name_hash: u32,
    pub offset: i32,
    pub unknown: i32,
    pub length: i32,
}

impl PmapSection {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let _ident = cursor.read_u32::<BigEndian>()?;
        let _length = cursor.read_i32::<BigEndian>()?;
        let root_id = cursor.read_i32::<BigEndian>()?;
        let first_count = cursor.read_i16::<BigEndian>()?;
        let unk_0eh = cursor.read_i16::<BigEndian>()?;

        let (count, header_kind) = if first_count > 0 {
            (first_count as i32, PmapHeaderKind::Legacy { unk_0eh })
        } else {
            let count = cursor.read_i16::<BigEndian>()?;
            let unk_14h = cursor.read_i16::<BigEndian>()?;
            let unk_16h = cursor.read_i16::<BigEndian>()?;
            let unk_18h = cursor.read_i16::<BigEndian>()?;
            (
                count as i32,
                PmapHeaderKind::Extended {
                    unk_0ch: first_count,
                    unk_0eh,
                    unk_12h: unk_14h,
                    unk_14h: unk_16h,
                    unk_16h: unk_18h,
                },
            )
        };

        let count = count.max(0) as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let name_hash = cursor.read_u32::<BigEndian>()?;
            let offset = cursor.read_i32::<BigEndian>()?;
            let unknown = cursor.read_i32::<BigEndian>()?;
            let length = cursor.read_i32::<BigEndian>()?;
            entries.push(PmapEntry {
                name_hash,
                offset,
                unknown,
                length,
            });
        }
        Ok(Self {
            root_id,
            header_kind,
            entries,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.root_id.to_be_bytes());
        let count = self.entries.len() as i16;
        match self.header_kind {
            PmapHeaderKind::Legacy { unk_0eh } => {
                body.extend_from_slice(&count.to_be_bytes());
                body.extend_from_slice(&unk_0eh.to_be_bytes());
            }
            PmapHeaderKind::Extended {
                unk_0ch,
                unk_0eh,
                unk_12h,
                unk_14h,
                unk_16h,
            } => {
                body.extend_from_slice(&unk_0ch.to_be_bytes());
                body.extend_from_slice(&unk_0eh.to_be_bytes());
                body.extend_from_slice(&count.to_be_bytes());
                body.extend_from_slice(&unk_12h.to_be_bytes());
                body.extend_from_slice(&unk_14h.to_be_bytes());
                body.extend_from_slice(&unk_16h.to_be_bytes());
            }
        }
        for e in &self.entries {
            body.extend_from_slice(&e.name_hash.to_be_bytes());
            body.extend_from_slice(&e.offset.to_be_bytes());
            body.extend_from_slice(&e.unknown.to_be_bytes());
            body.extend_from_slice(&e.length.to_be_bytes());
        }
        wrap_section(PsoSection::Pmap, &body)
    }

    pub fn block(&self, block_id: i32) -> Option<&PmapEntry> {
        if block_id <= 0 {
            return None;
        }
        self.entries.get((block_id - 1) as usize)
    }
}

pub(crate) fn wrap_section(ident: PsoSection, body: &[u8]) -> Vec<u8> {
    let total = (body.len() + 8) as u32;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(&ident.ident().to_be_bytes());
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(body);
    out
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PsoPointer {
    pub raw: u64,
}

impl PsoPointer {
    pub fn read(cursor: &mut Cursor<&[u8]>) -> Result<Self> {
        let low = cursor.read_u32::<BigEndian>()? as u64;
        let high = cursor.read_u32::<BigEndian>()? as u64;
        Ok(Self {
            raw: low | (high << 32),
        })
    }

    pub fn block_id(self) -> u32 {
        (self.raw & 0xFFF) as u32
    }

    pub fn data_offset(self) -> u32 {
        ((self.raw >> 12) & 0xFFFFF) as u32
    }

    /// The offset under GTA VI's packing: bits 20 to 31 of the first word, then
    /// the first byte of the second as bits 12 to 19; `None` when any other bit is set.
    pub fn gta6_data_offset(self) -> Option<u32> {
        let (low, high) = (self.raw as u32, (self.raw >> 32) as u32);
        ((low >> 12) & 0xFF == 0 && high & 0x00FF_FFFF == 0)
            .then_some((low >> 20) | ((high >> 24) << 12))
    }

    /// The offset under the packing of the file's target.
    pub(crate) fn data_offset_for(self, gta6: bool) -> Result<u32> {
        if !gta6 {
            return Ok(self.data_offset());
        }
        self.gta6_data_offset().ok_or_else(|| {
            MetaError::Other(format!(
                "pso: GTA VI pointer {:#018X} sets bits whose meaning is not known",
                self.raw
            ))
        })
    }

    pub fn is_null(self) -> bool {
        self.block_id() == 0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PsoArrayHeader {
    pub pointer: PsoPointer,
    pub count: u32,
    pub _capacity: u32,
    pub _unknown: u32,
}

impl PsoArrayHeader {
    pub fn read(cursor: &mut Cursor<&[u8]>) -> Result<Self> {
        let pointer = PsoPointer::read(cursor)?;
        let count = cursor.read_u16::<BigEndian>()?.into();
        let capacity = cursor.read_u16::<BigEndian>()?.into();
        let unknown = cursor.read_u32::<BigEndian>()?;
        Ok(Self {
            pointer,
            count,
            _capacity: capacity,
            _unknown: unknown,
        })
    }

    /// GTA VI's header of array subtype 10: `{u32 count, u32 capacity, pointer}`.
    pub fn read_count_first(cursor: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = cursor.read_u32::<BigEndian>()?;
        let capacity = cursor.read_u32::<BigEndian>()?;
        let pointer = PsoPointer::read(cursor)?;
        Ok(Self {
            pointer,
            count,
            _capacity: capacity,
            _unknown: 0,
        })
    }
}

pub(crate) fn read_section_header(bytes: &[u8]) -> Result<(usize, usize, u32)> {
    if bytes.len() < 8 {
        return Err(MetaError::Other("pso: section truncated".into()));
    }
    let mut cursor = Cursor::new(bytes);
    let ident = cursor.read_u32::<BigEndian>()?;
    let length = cursor.read_i32::<BigEndian>()? as usize;
    if length < 8 || length > bytes.len() {
        return Err(MetaError::Other(format!(
            "pso: bad section length {} (have {})",
            length,
            bytes.len()
        )));
    }
    Ok((8, length, ident))
}
