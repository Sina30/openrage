//! The DXBC container a DXIL program ships in: its parts, the program header, the feature flags, the
//! pipeline-state resources and the signatures. `PRIV` is never read.

use or_io::gta5::shaders::spirv_pack::SignatureElement;
use or_io::gta5::shaders::xdat::{self, XdatError};

use super::{ResourceClass, ShaderKind};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainerError {
    #[error("not a DXBC container")]
    NotDxbc,
    #[error("the container ends before {what} at +{at:#X}")]
    Truncated { what: &'static str, at: usize },
    #[error("the container has no {0} part")]
    NoPart(&'static str),
    #[error("the DXIL part holds a {magic} payload and no LLVM bitcode, so the program is not translated")]
    NoBitcode { magic: String },
    #[error("the DXIL part's XDAT payload inflates (zlib) to {code_bytes} bytes of native GPU code for the {stage} stage, in {ranges} code range(s), and holds no LLVM bitcode, so the program is not translated")]
    NativeCode {
        stage: String,
        ranges: usize,
        code_bytes: usize,
    },
    #[error(transparent)]
    Xdat(#[from] XdatError),
    #[error("the {part} part does not hold to its layout: {why}")]
    Malformed { part: &'static str, why: String },
}

/// The tag, a 16-byte digest, a version, the container's size and the part count.
const HEADER: usize = 32;
const DXIL_MAGIC: u32 = 0x4C49_5844;
/// The DXIL part's program header and bitcode header together.
const PROGRAM_HEADER: usize = 24;
const LLVM_MAGIC: [u8; 4] = [b'B', b'C', 0xC0, 0xDE];
const WRAPPER_MAGIC: u32 = 0x0B17_C0DE;
/// A signature element: stream, name offset, index, system value, component type, register,
/// mask, read-write mask, padding and minimum precision.
const ELEMENT: usize = 32;
/// A `PSV0` resource record's four words: kind, space, lower and upper bound.
const PSV_RESOURCE_MIN: usize = 16;
/// The runtime info's shader-stage byte, present from its second version (36 bytes) on.
const PSV_STAGE: usize = 24;
/// The runtime info's thread-group size, present from its third version (48 bytes) on.
const PSV_THREADS: usize = 36;

#[derive(Debug, Clone)]
pub struct Part<'a> {
    pub fourcc: [u8; 4],
    pub data: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct Container<'a> {
    pub parts: Vec<Part<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramHeader {
    pub kind: ShaderKind,
    /// Major and minor.
    pub model: (u8, u8),
    pub dxil_version: (u8, u8),
}

#[derive(Debug, Clone)]
pub struct Dxil<'a> {
    pub header: ProgramHeader,
    /// The LLVM bitcode, the wrapper header (0x0B17C0DE) already stepped over.
    pub bitcode: &'a [u8],
}

/// `SFI0`'s word; the bit names are the public D3D12 shader-feature flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeatureFlags(pub u64);

impl FeatureFlags {
    pub const WAVE_OPS: u64 = 0x4000;
    pub const INT64: u64 = 0x8000;
    pub const NATIVE_LOW_PRECISION: u64 = 0x4_0000;
    pub const RESOURCE_HEAP: u64 = 0x200_0000;
    pub const SAMPLER_HEAP: u64 = 0x400_0000;

    pub fn has(self, bit: u64) -> bool {
        self.0 & bit == bit
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsvResourceKind {
    Sampler,
    Cbv,
    SrvTyped,
    SrvRaw,
    SrvStructured,
    UavTyped,
    UavRaw,
    UavStructured,
    UavStructuredWithCounter,
    Other(u32),
}

impl PsvResourceKind {
    /// The kind a `PSV0` record's first word numbers.
    fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Sampler,
            2 => Self::Cbv,
            3 => Self::SrvTyped,
            4 => Self::SrvRaw,
            5 => Self::SrvStructured,
            6 => Self::UavTyped,
            7 => Self::UavRaw,
            8 => Self::UavStructured,
            9 => Self::UavStructuredWithCounter,
            other => Self::Other(other),
        }
    }

    /// The register class; a counted UAV's own register is a `u#`, its counter is not named here.
    pub fn class(self) -> Option<ResourceClass> {
        match self {
            Self::Sampler => Some(ResourceClass::Sampler),
            Self::Cbv => Some(ResourceClass::Cbv),
            Self::SrvTyped | Self::SrvRaw | Self::SrvStructured => Some(ResourceClass::Srv),
            Self::UavTyped
            | Self::UavRaw
            | Self::UavStructured
            | Self::UavStructuredWithCounter => Some(ResourceClass::Uav),
            Self::Other(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsvResource {
    pub kind: PsvResourceKind,
    pub space: u32,
    pub lower: u32,
    /// 0xFFFFFFFF for an unbounded range.
    pub upper: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Psv {
    pub runtime_info_size: u32,
    pub resources: Vec<PsvResource>,
    /// Compute and mesh: the thread group size, where the runtime info states it.
    pub threads: Option<[u32; 3]>,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

/// The word at `at`, or `Truncated` naming `what`.
fn word(bytes: &[u8], at: usize, what: &'static str) -> Result<u32, ContainerError> {
    u32_at(bytes, at).ok_or(ContainerError::Truncated { what, at })
}

/// A payload's leading bytes as a person reads them: the text where it is printable, else hex.
fn magic_of(bytes: &[u8]) -> String {
    let lead = &bytes[..bytes.len().min(4)];
    if lead.is_empty() {
        "empty".to_owned()
    } else if lead.iter().all(u8::is_ascii_graphic) {
        String::from_utf8_lossy(lead).into_owned()
    } else {
        lead.iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl<'a> Container<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self, ContainerError> {
        if bytes.get(..4) != Some(b"DXBC".as_slice()) {
            return Err(ContainerError::NotDxbc);
        }
        let size = word(bytes, 24, "the container header")? as usize;
        let count = word(bytes, 28, "the container header")? as usize;
        if size > bytes.len() {
            return Err(ContainerError::Truncated {
                what: "the size the header states",
                at: bytes.len(),
            });
        }
        let bytes = &bytes[..size];
        let table_end = count
            .checked_mul(4)
            .and_then(|table| table.checked_add(HEADER))
            .filter(|&end| end <= size)
            .ok_or(ContainerError::Truncated {
                what: "the part table",
                at: HEADER,
            })?;
        let mut parts = Vec::with_capacity(count);
        for index in 0..count {
            let offset = word(bytes, HEADER + index * 4, "the part table")? as usize;
            if offset < table_end {
                return Err(ContainerError::Malformed {
                    part: "DXBC",
                    why: format!("part {index} starts at +{offset:#X}, inside the part table"),
                });
            }
            let fourcc: [u8; 4] = bytes
                .get(offset..offset.saturating_add(4))
                .and_then(|tag| tag.try_into().ok())
                .ok_or(ContainerError::Truncated {
                    what: "a part header",
                    at: offset,
                })?;
            let len = word(bytes, offset.saturating_add(4), "a part header")? as usize;
            let start = offset.saturating_add(8);
            let data = start
                .checked_add(len)
                .and_then(|end| bytes.get(start..end))
                .ok_or(ContainerError::Truncated {
                    what: "a part's data",
                    at: start,
                })?;
            parts.push(Part { fourcc, data });
        }
        Ok(Self { parts })
    }

    pub fn part(&self, fourcc: &[u8; 4]) -> Option<&'a [u8]> {
        self.parts
            .iter()
            .find(|part| &part.fourcc == fourcc)
            .map(|part| part.data)
    }

    /// The `DXIL` program part; `NativeCode` for a GTA VI `XDAT` payload, `NoBitcode` for any other payload with no LLVM bitcode.
    pub fn dxil(&self) -> Result<Dxil<'a>, ContainerError> {
        let part = self.part(b"DXIL").ok_or(ContainerError::NoPart("DXIL"))?;
        if part.len() < PROGRAM_HEADER {
            return Err(ContainerError::Truncated {
                what: "the DXIL program header",
                at: part.len(),
            });
        }
        let what = "the DXIL program header";
        let version = word(part, 0, what)?;
        if word(part, 8, what)? != DXIL_MAGIC {
            return Err(ContainerError::Malformed {
                part: "DXIL",
                why: "the bitcode header does not open with the DXIL magic".to_owned(),
            });
        }
        let dxil_version = word(part, 12, what)?;
        let offset = word(part, 16, what)? as usize;
        let len = word(part, 20, what)? as usize;
        let header = ProgramHeader {
            kind: ShaderKind::from_program_version(version),
            model: (((version >> 4) & 0xF) as u8, (version & 0xF) as u8),
            dxil_version: ((dxil_version >> 8) as u8, dxil_version as u8),
        };
        // The stated offset counts from the bitcode header, 8 bytes into the part.
        let start = offset.checked_add(8).ok_or(ContainerError::Truncated {
            what: "the stated bitcode",
            at: offset,
        })?;
        let mut bitcode = start
            .checked_add(len)
            .and_then(|end| part.get(start..end))
            .ok_or(ContainerError::Truncated {
                what: "the stated bitcode",
                at: start,
            })?;
        if u32_at(bitcode, 0) == Some(WRAPPER_MAGIC) {
            let what = "the bitcode wrapper";
            let inner = word(bitcode, 8, what)? as usize;
            let inner_len = word(bitcode, 12, what)? as usize;
            bitcode = inner
                .checked_add(inner_len)
                .and_then(|end| bitcode.get(inner..end))
                .ok_or(ContainerError::Truncated { what, at: inner })?;
        }
        if bitcode.starts_with(&LLVM_MAGIC) {
            Ok(Dxil { header, bitcode })
        } else if part[PROGRAM_HEADER..].starts_with(b"XDAT") {
            let program = xdat::read(part)?;
            Err(ContainerError::NativeCode {
                stage: header.kind.word(),
                ranges: program.stages.len(),
                code_bytes: (program.stages.iter()).map(|stage| stage.code.len()).sum(),
            })
        } else {
            Err(ContainerError::NoBitcode {
                magic: magic_of(&part[PROGRAM_HEADER..]),
            })
        }
    }

    pub fn features(&self) -> FeatureFlags {
        let word: Option<[u8; 8]> = self
            .part(b"SFI0")
            .and_then(|data| data.get(..8)?.try_into().ok());
        FeatureFlags(word.map_or(0, u64::from_le_bytes))
    }

    pub fn psv(&self) -> Result<Psv, ContainerError> {
        let part = self.part(b"PSV0").ok_or(ContainerError::NoPart("PSV0"))?;
        let runtime_info_size = word(part, 0, "PSV0's runtime info size")?;
        let info_end = (runtime_info_size as usize)
            .checked_add(4)
            .filter(|&end| end <= part.len())
            .ok_or(ContainerError::Truncated {
                what: "PSV0's runtime info",
                at: 4,
            })?;
        let info = &part[4..info_end];
        // Compute, mesh and amplification are stages 5, 13 and 14.
        let threads = match info.get(PSV_STAGE).copied() {
            Some(5 | 13 | 14) => match (
                u32_at(info, PSV_THREADS),
                u32_at(info, PSV_THREADS + 4),
                u32_at(info, PSV_THREADS + 8),
            ) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            },
            _ => None,
        };
        let count = word(part, info_end, "PSV0's resource count")? as usize;
        let mut resources = Vec::new();
        if count > 0 {
            let stride = word(part, info_end + 4, "PSV0's resource record size")? as usize;
            if stride < PSV_RESOURCE_MIN {
                return Err(ContainerError::Malformed {
                    part: "PSV0",
                    why: format!(
                        "a resource record of {stride} bytes is shorter than {PSV_RESOURCE_MIN}"
                    ),
                });
            }
            let first = info_end + 8;
            count
                .checked_mul(stride)
                .and_then(|total| first.checked_add(total))
                .filter(|&end| end <= part.len())
                .ok_or(ContainerError::Truncated {
                    what: "PSV0's resources",
                    at: first,
                })?;
            resources.reserve_exact(count);
            for index in 0..count {
                let at = first + index * stride;
                let what = "a PSV0 resource";
                resources.push(PsvResource {
                    kind: PsvResourceKind::from_code(word(part, at, what)?),
                    space: word(part, at + 4, what)?,
                    lower: word(part, at + 8, what)?,
                    upper: word(part, at + 12, what)?,
                });
            }
        }
        Ok(Psv {
            runtime_info_size,
            resources,
            threads,
        })
    }

    /// `ISG1` and `OSG1`, semantic names read from the part's own strings.
    pub fn inputs(&self) -> Result<Vec<SignatureElement>, ContainerError> {
        self.signature(b"ISG1", "ISG1")
    }

    pub fn outputs(&self) -> Result<Vec<SignatureElement>, ContainerError> {
        self.signature(b"OSG1", "OSG1")
    }

    fn signature(
        &self,
        fourcc: &[u8; 4],
        name: &'static str,
    ) -> Result<Vec<SignatureElement>, ContainerError> {
        let part = self.part(fourcc).ok_or(ContainerError::NoPart(name))?;
        let what = "a signature header";
        let count = word(part, 0, what)? as usize;
        let first = word(part, 4, what)? as usize;
        count
            .checked_mul(ELEMENT)
            .and_then(|total| first.checked_add(total))
            .filter(|&end| end <= part.len())
            .ok_or(ContainerError::Truncated {
                what: "a signature element",
                at: first,
            })?;
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let at = first + index * ELEMENT;
            let what = "a signature element";
            let name_at = word(part, at + 4, what)? as usize;
            let semantic = part
                .get(name_at..)
                .and_then(|tail| Some(&tail[..tail.iter().position(|&b| b == 0)?]))
                .ok_or_else(|| ContainerError::Malformed {
                    part: name,
                    why: format!(
                        "element {index}'s semantic name at +{name_at:#X} runs off the part"
                    ),
                })?;
            out.push(SignatureElement {
                semantic: String::from_utf8_lossy(semantic).into_owned(),
                index: word(part, at + 8, what)?,
                system_value: word(part, at + 12, what)?,
                component_type: word(part, at + 16, what)?,
                register: word(part, at + 20, what)?,
                mask: part[at + 24],
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dwords(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// A container of `(tag, data)` parts, each padded to whole dwords.
    fn container(parts: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let table_end = HEADER + parts.len() * 4;
        let mut offsets = Vec::new();
        let mut body = Vec::new();
        for (tag, data) in parts {
            offsets.push((table_end + body.len()) as u32);
            body.extend_from_slice(*tag);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            while !body.len().is_multiple_of(4) {
                body.push(0);
            }
        }
        let mut out = b"DXBC".to_vec();
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(&[1, 0, 0, 0]);
        out.extend_from_slice(&((table_end + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(parts.len() as u32).to_le_bytes());
        for offset in offsets {
            out.extend_from_slice(&offset.to_le_bytes());
        }
        out.extend(body);
        out
    }

    /// A `DXIL` part for a vertex 6.6 program, DXIL 1.6, whose payload after the header is `payload`
    /// and whose stated bitcode is its last `bitcode` bytes.
    fn dxil_part(payload: &[u8], bitcode: usize) -> Vec<u8> {
        let offset = 16 + payload.len() - bitcode;
        let mut part = dwords(&[
            0x0001_0066,
            ((PROGRAM_HEADER + payload.len()) / 4) as u32,
            DXIL_MAGIC,
            0x106,
            offset as u32,
            bitcode as u32,
        ]);
        part.extend_from_slice(payload);
        part
    }

    #[test]
    fn a_payload_with_no_bitcode_is_named_by_its_magic_and_a_broken_xdat_one_by_its_rule() {
        let mut payload = b"QZPK".to_vec();
        payload.extend_from_slice(&[7, 1, 2, 3, 0x11, 0x22, 0x33, 0x44]);
        let bytes = container(&[(b"DXIL", dxil_part(&payload, 4))]);
        let read = Container::read(&bytes).expect("the container reads");
        assert_eq!(
            read.dxil().map(|d| d.bitcode.len()),
            Err(ContainerError::NoBitcode {
                magic: "QZPK".to_owned()
            })
        );

        // An XDAT payload too short for its table.
        payload[..4].copy_from_slice(b"XDAT");
        let bytes = container(&[(b"DXIL", dxil_part(&payload, 4))]);
        let read = Container::read(&bytes).expect("the container reads");
        let Err(ContainerError::Xdat(error)) = read.dxil() else {
            panic!("a broken XDAT payload was not refused by its rule");
        };
        assert_eq!(error.rule, "P1");
        assert!(error.to_string().contains("XDAT"));
    }

    #[test]
    fn llvm_bitcode_is_the_stated_slice() {
        let payload = [0x5A_u8, b'B', b'C', 0xC0, 0xDE, 0x35, 0x14, 0x00];
        // The first byte is not the bitcode: the stated slice starts one byte in.
        let bytes = container(&[(b"DXIL", dxil_part(&payload, payload.len() - 1))]);
        let read = Container::read(&bytes).expect("the container reads");
        let dxil = read.dxil().expect("bitcode");
        assert_eq!(dxil.bitcode, &payload[1..]);
        assert_eq!(dxil.header.model, (6, 6));
        assert_eq!(dxil.header.dxil_version, (1, 6));
        assert_eq!(
            dxil.header.kind,
            ShaderKind::from_program_version(0x0001_0066)
        );
    }

    #[test]
    fn a_bitcode_wrapper_is_stepped_over() {
        let mut payload = dwords(&[WRAPPER_MAGIC, 0, 20, 6, 0]);
        payload.extend_from_slice(&[b'B', b'C', 0xC0, 0xDE, 0x9A, 0x5B, 0, 0]);
        let bytes = container(&[(b"DXIL", dxil_part(&payload, payload.len()))]);
        let read = Container::read(&bytes).expect("the container reads");
        let dxil = read.dxil().expect("bitcode");
        assert_eq!(dxil.bitcode, &[b'B', b'C', 0xC0, 0xDE, 0x9A, 0x5B]);
    }

    #[test]
    fn a_stated_bitcode_past_the_part_is_truncated() {
        let mut part = dxil_part(b"BC\xC0\xDE", 4);
        part[20..24].copy_from_slice(&5u32.to_le_bytes());
        let bytes = container(&[(b"DXIL", part)]);
        let read = Container::read(&bytes).expect("the container reads");
        assert!(matches!(read.dxil(), Err(ContainerError::Truncated { .. })));
    }

    #[test]
    fn a_container_cut_short_is_an_error_not_a_panic() {
        let bytes = container(&[(b"SFI0", vec![1, 2, 3, 4, 5, 6, 7, 8])]);
        for len in 0..bytes.len() {
            assert!(Container::read(&bytes[..len]).is_err(), "length {len}");
        }
        assert!(matches!(
            Container::read(b"DXBX0000"),
            Err(ContainerError::NotDxbc)
        ));
    }

    #[test]
    fn a_missing_dxil_part_is_named() {
        let bytes = container(&[(b"SFI0", vec![0; 8])]);
        let read = Container::read(&bytes).expect("the container reads");
        assert_eq!(read.dxil().map(|_| ()), Err(ContainerError::NoPart("DXIL")));
    }

    #[test]
    fn feature_flags_read_sfi0() {
        let word = FeatureFlags::RESOURCE_HEAP | FeatureFlags::WAVE_OPS | 1 << 40;
        let bytes = container(&[(b"SFI0", word.to_le_bytes().to_vec())]);
        let flags = Container::read(&bytes).expect("reads").features();
        assert_eq!(flags, FeatureFlags(word));
        assert!(flags.has(FeatureFlags::RESOURCE_HEAP));
        assert!(flags.has(FeatureFlags::WAVE_OPS));
        assert!(!flags.has(FeatureFlags::SAMPLER_HEAP));
        assert!(!flags.has(FeatureFlags::INT64));
    }

    /// A `PSV0` of a 52-byte runtime info for `stage` with threads (8, 4, 2) and `records`.
    fn psv_part(stage: u8, stride: u32, records: &[[u32; 4]]) -> Vec<u8> {
        let mut info = vec![0u8; 52];
        info[PSV_STAGE] = stage;
        info[PSV_THREADS..PSV_THREADS + 12].copy_from_slice(&dwords(&[8, 4, 2]));
        let mut part = dwords(&[52]);
        part.extend(info);
        part.extend(dwords(&[records.len() as u32]));
        if !records.is_empty() {
            part.extend(dwords(&[stride]));
            for record in records {
                part.extend(dwords(record));
                part.resize(part.len() + stride as usize - 16, 0xEE);
            }
        }
        part
    }

    #[test]
    fn psv0_resources_read_at_their_stride() {
        let records = [
            [2, 1, 3, 3],
            [3, 0, 7, 7],
            [9, 2, 5, 0xFFFF_FFFF],
            [42, 3, 1, 1],
        ];
        let bytes = container(&[(b"PSV0", psv_part(1, 24, &records))]);
        let psv = Container::read(&bytes).expect("reads").psv().expect("psv");
        assert_eq!(psv.runtime_info_size, 52);
        assert_eq!(psv.threads, None, "a vertex stage states no thread group");
        let expected: Vec<PsvResource> = records
            .iter()
            .map(|r| PsvResource {
                kind: PsvResourceKind::from_code(r[0]),
                space: r[1],
                lower: r[2],
                upper: r[3],
            })
            .collect();
        assert_eq!(psv.resources, expected);
        assert_eq!(psv.resources[0].kind, PsvResourceKind::Cbv);
        assert_eq!(psv.resources[1].kind.class(), Some(ResourceClass::Srv));
        assert_eq!(
            psv.resources[2].kind,
            PsvResourceKind::UavStructuredWithCounter
        );
        assert_eq!(psv.resources[3].kind, PsvResourceKind::Other(42));
        assert_eq!(psv.resources[3].kind.class(), None);
    }

    #[test]
    fn psv0_states_a_compute_thread_group() {
        let bytes = container(&[(b"PSV0", psv_part(5, 16, &[]))]);
        let psv = Container::read(&bytes).expect("reads").psv().expect("psv");
        assert_eq!(psv.threads, Some([8, 4, 2]));
        assert!(psv.resources.is_empty());
    }

    #[test]
    fn psv0_resources_past_the_part_are_truncated() {
        let mut part = psv_part(0, 16, &[[1, 0, 0, 0]]);
        let count_at = 4 + 52;
        part[count_at..count_at + 4].copy_from_slice(&1_000_000u32.to_le_bytes());
        let bytes = container(&[(b"PSV0", part)]);
        let read = Container::read(&bytes).expect("reads");
        assert!(matches!(read.psv(), Err(ContainerError::Truncated { .. })));
    }

    /// A signature part of `(name, index, system value, component type, register, mask)`.
    type Row = (&'static str, u32, u32, u32, u32, u8);

    fn signature_part(elements: &[Row]) -> Vec<u8> {
        let strings_at = 8 + elements.len() * ELEMENT;
        let mut strings = Vec::new();
        let mut part = dwords(&[elements.len() as u32, 8]);
        for &(name, index, system_value, component, register, mask) in elements {
            let name_at = (strings_at + strings.len()) as u32;
            strings.extend_from_slice(name.as_bytes());
            strings.push(0);
            part.extend(dwords(&[
                0,
                name_at,
                index,
                system_value,
                component,
                register,
            ]));
            part.extend_from_slice(&[mask, !mask, 0, 0]);
            part.extend(dwords(&[0]));
        }
        part.extend(strings);
        part
    }

    #[test]
    fn signatures_read_their_elements_and_names() {
        let inputs: [Row; 2] = [("POSITION", 0, 0, 3, 0, 0x7), ("TEXCOORD", 2, 0, 1, 3, 0x3)];
        let outputs: [Row; 2] = [
            ("SV_Position", 0, 1, 3, 0, 0xF),
            ("SV_Target", 1, 64, 3, 1, 0xF),
        ];
        let bytes = container(&[
            (b"ISG1", signature_part(&inputs)),
            (b"OSG1", signature_part(&outputs)),
        ]);
        let read = Container::read(&bytes).expect("reads");
        let element =
            |&(name, index, system_value, component_type, register, mask): &Row| SignatureElement {
                semantic: name.to_owned(),
                index,
                system_value,
                component_type,
                register,
                mask,
            };
        assert_eq!(read.inputs(), Ok(inputs.iter().map(element).collect()));
        assert_eq!(read.outputs(), Ok(outputs.iter().map(element).collect()));
    }

    #[test]
    fn a_signature_name_off_the_part_is_malformed() {
        let mut part = signature_part(&[("COLOR", 0, 0, 3, 0, 0xF)]);
        part[12..16].copy_from_slice(&0x4000u32.to_le_bytes());
        let bytes = container(&[(b"ISG1", part)]);
        let read = Container::read(&bytes).expect("reads");
        assert!(matches!(
            read.inputs(),
            Err(ContainerError::Malformed { part: "ISG1", .. })
        ));
        assert_eq!(read.outputs(), Err(ContainerError::NoPart("OSG1")));
    }
}
