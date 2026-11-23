//! `.fxc`: the DXBC blobs inside one, and the `grcEffect` that declares them.
//!
//! Legacy only: a retail Enhanced install ships zero `.fxc`, because Gen9
//! moved the shader parameter model into the resource, so a `Gen9` arm here
//! would describe files that do not exist.
//!
//! Read-only: the blobs are `fxc.exe`'s output and `or-shader-tool` bakes
//! rather than rewrites them. A `.fxc` carries no table of contents, so
//! consuming the file exactly ([`GrcEffect::trailing_bytes`]) is the only
//! check that the parse landed on the right fields.

mod grc_effect;
pub(crate) use grc_effect::Cursor;
mod rdef;

pub use rdef::{
    DxbcBindKind, DxbcBinding, DxbcConstantBuffer, DxbcReflection, DxbcScalar, DxbcShape,
    DxbcVariable,
};

pub use grc_effect::{
    GrcAnnotationValue, GrcEffect, GrcEffectCBuffer, GrcEffectPass, GrcEffectPreset,
    GrcEffectProgram, GrcEffectTechnique, GrcEffectVariable, GrcProgramStage,
    GrcVariableAnnotation, GrcVariableType,
};

use crate::error::{IoError, Result};

const DXBC_MAGIC: &[u8; 4] = b"DXBC";

const DXBC_MIN_HEADER: usize = 32;

const DXBC_SIZE_OFFSET: usize = 24;

const DXBC_PART_COUNT_OFFSET: usize = 28;

const DXBC_PART_TABLE_OFFSET: usize = 32;

#[derive(Debug, Clone)]
pub struct DxbcBlob<'a> {
    pub offset: usize,
    pub size: usize,
    pub bytes: &'a [u8],
}

impl<'a> DxbcBlob<'a> {
    #[must_use]
    pub fn parts(&self) -> Vec<DxbcPart<'a>> {
        let mut out = Vec::new();
        let Some(part_count) = read_u32(self.bytes, DXBC_PART_COUNT_OFFSET) else {
            return out;
        };
        let table_end =
            DXBC_PART_TABLE_OFFSET.saturating_add((part_count as usize).saturating_mul(4));
        if table_end > self.bytes.len() {
            return out;
        }
        for i in 0..part_count as usize {
            let offset_field = DXBC_PART_TABLE_OFFSET + i * 4;
            let Some(part_off) = read_u32(self.bytes, offset_field) else {
                continue;
            };
            let part_off = part_off as usize;
            if part_off + 8 > self.bytes.len() {
                continue;
            }
            let tag: [u8; 4] = self.bytes[part_off..part_off + 4].try_into().unwrap();
            let size = read_u32(self.bytes, part_off + 4).unwrap_or(0) as usize;
            let data_start = part_off + 8;
            let data_end = data_start.saturating_add(size).min(self.bytes.len());
            out.push(DxbcPart {
                tag,
                offset: part_off,
                data: &self.bytes[data_start..data_end],
            });
        }
        out
    }

    /// What this shader says it binds, from its own `RDEF` chunk.
    ///
    /// `None` where the blob carries no `RDEF` — a stripped build does not,
    /// and that is an absence rather than a failure.
    #[must_use]
    pub fn reflect(&self) -> Option<Result<DxbcReflection>> {
        self.parts()
            .into_iter()
            .find(|p| &p.tag == b"RDEF")
            .map(|p| DxbcReflection::parse(p.data))
    }

    #[must_use]
    pub fn detected_stage(&self) -> ShaderStage {
        for part in self.parts() {
            if &part.tag == b"SHEX" || &part.tag == b"SHDR" {
                if part.data.len() < 4 {
                    return ShaderStage::Unknown;
                }
                let token = u32::from_le_bytes(part.data[0..4].try_into().unwrap());
                let prog_type = (token >> 16) & 0xFFFF;
                return match prog_type {
                    0 => ShaderStage::Pixel,
                    1 => ShaderStage::Vertex,
                    2 => ShaderStage::Geometry,
                    3 => ShaderStage::Hull,
                    4 => ShaderStage::Domain,
                    5 => ShaderStage::Compute,
                    _ => ShaderStage::Unknown,
                };
            }
        }
        ShaderStage::Unknown
    }
}

#[derive(Debug, Clone)]
pub struct DxbcPart<'a> {
    pub tag: [u8; 4],
    pub offset: usize,
    pub data: &'a [u8],
}

impl<'a> DxbcPart<'a> {
    #[must_use]
    pub fn tag_string(&self) -> String {
        self.tag
            .iter()
            .map(|b| {
                if b.is_ascii_graphic() {
                    *b as char
                } else {
                    '?'
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    Vertex,
    Pixel,
    Geometry,
    Hull,
    Domain,
    Compute,
    Unknown,
}

impl ShaderStage {
    #[must_use]
    pub fn short_code(self) -> &'static str {
        match self {
            ShaderStage::Vertex => "VS",
            ShaderStage::Pixel => "PS",
            ShaderStage::Geometry => "GS",
            ShaderStage::Hull => "HS",
            ShaderStage::Domain => "DS",
            ShaderStage::Compute => "CS",
            ShaderStage::Unknown => "??",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FxcArchive {
    blobs: Vec<DxbcDescriptor>,
    total_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct DxbcDescriptor {
    pub offset: usize,
    pub size: usize,
}

impl FxcArchive {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut blobs = Vec::new();
        let mut i = 0usize;
        while i + DXBC_MIN_HEADER <= bytes.len() {
            if &bytes[i..i + 4] == DXBC_MAGIC {
                let size = match read_u32(bytes, i + DXBC_SIZE_OFFSET) {
                    Some(s) => s as usize,
                    None => break,
                };
                if size < DXBC_MIN_HEADER {
                    return Err(IoError::InvalidStructure {
                        offset: i as u64,
                        message: format!(
                            "DXBC header reports size {size} smaller than minimum {DXBC_MIN_HEADER}"
                        ),
                    });
                }
                if i + size > bytes.len() {
                    return Err(IoError::PointerOutOfBounds {
                        offset: (i + size) as u64,
                        size: bytes.len(),
                    });
                }
                blobs.push(DxbcDescriptor { offset: i, size });
                i += size;
            } else {
                i += 1;
            }
        }
        Ok(Self {
            blobs,
            total_bytes: bytes.len(),
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    #[must_use]
    pub fn descriptors(&self) -> &[DxbcDescriptor] {
        &self.blobs
    }

    pub fn blobs<'a>(&'a self, bytes: &'a [u8]) -> impl Iterator<Item = DxbcBlob<'a>> + 'a {
        self.blobs.iter().filter_map(move |d| {
            if d.offset + d.size > bytes.len() {
                return None;
            }
            Some(DxbcBlob {
                offset: d.offset,
                size: d.size,
                bytes: &bytes[d.offset..d.offset + d.size],
            })
        })
    }

    #[must_use]
    pub fn stage_counts(&self, bytes: &[u8]) -> [usize; 7] {
        let mut counts = [0usize; 7];
        for blob in self.blobs(bytes) {
            let idx = match blob.detected_stage() {
                ShaderStage::Vertex => 0,
                ShaderStage::Pixel => 1,
                ShaderStage::Geometry => 2,
                ShaderStage::Hull => 3,
                ShaderStage::Domain => 4,
                ShaderStage::Compute => 5,
                ShaderStage::Unknown => 6,
            };
            counts[idx] += 1;
        }
        counts
    }
}

fn read_u32(buf: &[u8], offset: usize) -> Option<u32> {
    if offset + 4 > buf.len() {
        return None;
    }
    Some(u32::from_le_bytes(buf[offset..offset + 4].try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dxbc(program_type: u32) -> Vec<u8> {
        let total_size: u32 = 48;
        let mut buf = Vec::with_capacity(total_size as usize);
        buf.extend_from_slice(b"DXBC");
        buf.extend_from_slice(&[0u8; 16]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&total_size.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&36u32.to_le_bytes());
        buf.extend_from_slice(b"SHEX");
        buf.extend_from_slice(&4u32.to_le_bytes());
        let token = (program_type & 0xFFFF) << 16;
        buf.extend_from_slice(&token.to_le_bytes());
        assert_eq!(buf.len(), total_size as usize);
        buf
    }

    #[test]
    fn parses_single_blob() {
        let bytes = make_dxbc(1);
        let archive = FxcArchive::parse(&bytes).unwrap();
        assert_eq!(archive.len(), 1);
        let blob = archive.blobs(&bytes).next().unwrap();
        assert_eq!(blob.offset, 0);
        assert_eq!(blob.size, bytes.len());
        assert_eq!(blob.detected_stage(), ShaderStage::Vertex);
    }

    #[test]
    fn parses_multiple_blobs_back_to_back() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&make_dxbc(1));
        bytes.extend_from_slice(&make_dxbc(0));
        bytes.extend_from_slice(&make_dxbc(2));
        let archive = FxcArchive::parse(&bytes).unwrap();
        assert_eq!(archive.len(), 3);

        let stages: Vec<_> = archive.blobs(&bytes).map(|b| b.detected_stage()).collect();
        assert_eq!(
            stages,
            vec![
                ShaderStage::Vertex,
                ShaderStage::Pixel,
                ShaderStage::Geometry
            ]
        );

        let counts = archive.stage_counts(&bytes);
        assert_eq!(counts[0], 1, "vertex");
        assert_eq!(counts[1], 1, "pixel");
        assert_eq!(counts[2], 1, "geometry");
    }

    #[test]
    fn skips_garbage_between_blobs() {
        let mut bytes = vec![0xAB, 0xCD, 0xEF, 0x01, 0x02];
        bytes.extend_from_slice(&make_dxbc(1));
        bytes.extend_from_slice(&[0xFF; 7]);
        bytes.extend_from_slice(&make_dxbc(0));
        let archive = FxcArchive::parse(&bytes).unwrap();
        assert_eq!(archive.len(), 2);
    }

    #[test]
    fn rejects_blob_with_size_past_eof() {
        let mut bytes = make_dxbc(1);
        let bad_size: u32 = 1024;
        bytes[24..28].copy_from_slice(&bad_size.to_le_bytes());
        let err = FxcArchive::parse(&bytes).unwrap_err();
        match err {
            IoError::PointerOutOfBounds { .. } => {}
            other => panic!("expected PointerOutOfBounds, got {other:?}"),
        }
    }

    #[test]
    fn rejects_blob_with_under_minimum_size() {
        let mut bytes = make_dxbc(1);
        let bad_size: u32 = 8;
        bytes[24..28].copy_from_slice(&bad_size.to_le_bytes());
        let err = FxcArchive::parse(&bytes).unwrap_err();
        match err {
            IoError::InvalidStructure { .. } => {}
            other => panic!("expected InvalidStructure, got {other:?}"),
        }
    }

    #[test]
    fn empty_input_is_empty_archive() {
        let archive = FxcArchive::parse(&[]).unwrap();
        assert!(archive.is_empty());
        assert_eq!(archive.len(), 0);
    }

    #[test]
    fn input_without_dxbc_yields_empty() {
        let archive = FxcArchive::parse(&[0u8; 256]).unwrap();
        assert!(archive.is_empty());
    }

    #[test]
    fn parts_returns_shex_for_simple_blob() {
        let bytes = make_dxbc(1);
        let archive = FxcArchive::parse(&bytes).unwrap();
        let blob = archive.blobs(&bytes).next().unwrap();
        let parts = blob.parts();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].tag_string(), "SHEX");
    }

    #[test]
    fn detects_each_stage() {
        for (code, expected) in [
            (0, ShaderStage::Pixel),
            (1, ShaderStage::Vertex),
            (2, ShaderStage::Geometry),
            (3, ShaderStage::Hull),
            (4, ShaderStage::Domain),
            (5, ShaderStage::Compute),
            (99, ShaderStage::Unknown),
        ] {
            let bytes = make_dxbc(code);
            let archive = FxcArchive::parse(&bytes).unwrap();
            let blob = archive.blobs(&bytes).next().unwrap();
            assert_eq!(blob.detected_stage(), expected, "program_type={code}");
        }
    }
}
