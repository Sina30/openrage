//! GTA VI's `XDAT` payload, which a program's DXIL part holds where LLVM bitcode would be: a
//! digest, one or two `XBCH` blocks, each a zlib stream of an `FSSM` package of native GPU code,
//! and a stub program header whose four bytes of bitcode are `NOBC`. No package holds bitcode.

use std::io::{self, Write};
use std::ops::Range;

use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};

const DXIL_MAGIC: u32 = 0x4C49_5844;
const DXIL_VERSION: u32 = 0x106;
/// Version, words, `DXIL`, its version, the bitcode offset (from +8) and the bitcode size.
const PROGRAM_HEADER: usize = 24;
/// `XDAT`, the entry count, and a word that is 0x24 whatever the count.
const XDAT_HEAD: usize = 12;
const XDAT_WORD: u32 = 0x24;
const ENTRY: usize = 12;
const MIN_PART: usize = 0x60;
const DIGEST: u32 = 0x12;
const XBCH: u32 = 0x0D;
const STUB: u32 = 0x07;
const DIGEST_SIZE: usize = 16;
const STUB_SIZE: usize = 28;
const STUB_BITCODE_OFFSET: u32 = 0x10;
/// Set in the stub's word count; the other 31 bits are kept as read.
const STUB_FLAG: u32 = 0x8000_0000;
const NOBC: &[u8; 4] = b"NOBC";
/// `XBCH`, its version, the u64 compressed size and the u64 raw size.
const BLOCK_HEAD: usize = 24;
const BLOCK_VERSION: u32 = 3;
const FSSM_VERSION: u32 = 0x101;
/// The package's offsets count from the end of its 16-byte head.
const BASE: usize = 0x10;
const SLOTS: usize = 12;
const SLOT_TABLE: usize = 0x18;
const EMPTY: u64 = 0xDEAD_BEEF_DEAD_BEEF;
/// The used slot's descriptor offset, the same in every package.
const DESCRIPTOR: u64 = 0xB0;
const DESCRIPTOR_SIZE: usize = 0x60;
const MIN_PACKAGE: usize = BASE + DESCRIPTOR as usize + DESCRIPTOR_SIZE;
/// The package word after the slot table.
const PACKAGE_WORD: usize = 0x78;
const PACKAGE_WORDS: [u32; 3] = [2, 0x2_0002, 0x3_0002];
const DESCRIPTOR_WORD_18: [u32; 3] = [0, 4, 0xC];
const CODE_ALIGN: usize = 64;
const S_ENDPGM: u32 = 0xBF81_0000;
const S_CODE_END: u32 = 0xBF9F_0000;
/// Bounds a stated raw size before it sizes an allocation.
const MAX_PACKAGE: usize = 16 << 20;
/// The slots of each `XBCH` block in entry order, by the DXIL program kind (version >> 16).
const KIND_SLOTS: [(u32, &[&[usize]]); 5] = [
    (0, &[&[5]]),
    (1, &[&[4], &[2, 4]]),
    (2, &[&[3]]),
    (5, &[&[11]]),
    (13, &[&[8]]),
];
/// The descriptor's stage word each slot states.
const SLOT_STAGE_WORDS: [(usize, u32); 6] = [
    (2, 0x2_0002),
    (3, 0x2_0203),
    (4, 0x2_0004),
    (5, 0x2_0105),
    (8, 0x2_0607),
    (11, 0x2_0306),
];

/// Why a DXIL part's `XDAT` payload is refused, by the rule it breaks.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the DXIL part's XDAT payload breaks rule {rule}: {why}")]
pub struct XdatError {
    /// `P1`–`P2` the program header, `X1`–`X3` the entries, `B1`–`B3` a block, `F1`–`F13` its package, `S1` the stub.
    pub rule: &'static str,
    pub why: String,
}

/// A DXIL part's `XDAT` payload, every block inflated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdatProgram {
    /// The DXIL program version: the kind in the high half, the shader model in the low.
    pub program_version: u32,
    /// Sixteen bytes nobody has derived, kept as read.
    pub digest: [u8; 16],
    pub stages: Vec<XdatStage>,
    /// The low 31 bits of the stub's word count, whose meaning is unknown, kept as read.
    pub stub_words: u32,
}

/// One `XBCH` block's inflated `FSSM` package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdatStage {
    /// The one used slot of the package's twelve.
    pub slot: usize,
    pub stage_word: u32,
    pub package: Vec<u8>,
    /// The code in `package`: GPU instruction words, `s_endpgm` among them, padded with `s_code_end`.
    pub code: Range<usize>,
}

fn error(rule: &'static str, why: impl Into<String>) -> XdatError {
    XdatError {
        rule,
        why: why.into(),
    }
}

fn need(ok: bool, rule: &'static str, why: impl FnOnce() -> String) -> Result<(), XdatError> {
    match ok {
        true => Ok(()),
        false => Err(error(rule, why())),
    }
}

fn word(bytes: &[u8], at: usize, rule: &'static str) -> Result<u32, XdatError> {
    at.checked_add(4)
        .and_then(|end| bytes.get(at..end))
        .and_then(|w| w.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| error(rule, format!("the bytes end before a word at +{at:#X}")))
}

fn dword(bytes: &[u8], at: usize, rule: &'static str) -> Result<u64, XdatError> {
    at.checked_add(8)
        .and_then(|end| bytes.get(at..end))
        .and_then(|w| w.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| error(rule, format!("the bytes end before a u64 at +{at:#X}")))
}

/// The `XDAT` payload of `part`, a DXIL part's data after its tag and size.
pub fn read(part: &[u8]) -> Result<XdatProgram, XdatError> {
    let len = part.len();
    need(len >= MIN_PART, "P1", || {
        format!("a DXIL part of {len} bytes")
    })?;
    let version = word(part, 0, "P1")?;
    let words = word(part, 4, "P1")?;
    need(
        (words as usize).checked_mul(4) == Some(len)
            && word(part, 8, "P1")? == DXIL_MAGIC
            && word(part, 12, "P1")? == DXIL_VERSION,
        "P1",
        || {
            format!(
                "a program header of {words} words, not DXIL {DXIL_VERSION:#X}, for {len} bytes"
            )
        },
    )?;
    let (offset, size) = (word(part, 16, "P2")?, word(part, 20, "P2")?);
    need(
        size == 4 && (offset as usize).checked_add(12) == Some(len) && part.ends_with(NOBC),
        "P2",
        || format!("the stated bitcode {offset}+{size} is not the closing NOBC"),
    )?;
    need(
        &part[PROGRAM_HEADER..PROGRAM_HEADER + 4] == b"XDAT",
        "X1",
        || "no XDAT magic after the program header".to_owned(),
    )?;
    let count = word(part, PROGRAM_HEADER + 4, "X1")? as usize;
    let head_word = word(part, PROGRAM_HEADER + 8, "X1")?;
    need(
        matches!(count, 3 | 4) && head_word == XDAT_WORD,
        "X1",
        || format!("{count} entries, and {head_word:#X} where 0x24 was due"),
    )?;
    let table = PROGRAM_HEADER + XDAT_HEAD;
    let mut at = table + ENTRY * count;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let e = table + ENTRY * index;
        let kind = word(part, e, "X2")?;
        let offset = word(part, e + 4, "X2")? as usize;
        let size = word(part, e + 8, "X2")? as usize;
        need(offset == at, "X2", || {
            format!("entry {kind:#X} at +{offset:#X}, where +{at:#X} was due")
        })?;
        let end = (offset.checked_add(size))
            .filter(|&end| end <= len)
            .ok_or_else(|| error("X2", format!("entry {kind:#X} runs past the part")))?;
        let padded = end.next_multiple_of(4);
        need(
            part.get(end..padded)
                .is_some_and(|pad| pad.iter().all(|&b| b == 0)),
            "X2",
            || format!("entry {kind:#X}'s pad is not zero"),
        )?;
        entries.push((kind, offset..end));
        at = padded;
    }
    let (digest_at, stub_at) = (entries[0].1.clone(), entries[count - 1].1.clone());
    need(stub_at.end == len, "X2", || {
        "the last entry does not end the part".to_owned()
    })?;
    let kinds: Vec<u32> = entries.iter().map(|(kind, _)| *kind).collect();
    need(
        kinds == [DIGEST, XBCH, STUB] || kinds == [DIGEST, XBCH, XBCH, STUB],
        "X3",
        || format!("entry types {kinds:X?}"),
    )?;
    need(
        digest_at.len() == DIGEST_SIZE && stub_at.len() == STUB_SIZE,
        "X3",
        || {
            format!(
                "a digest of {} bytes and a stub of {}",
                digest_at.len(),
                stub_at.len()
            )
        },
    )?;
    let digest = <[u8; 16]>::try_from(&part[digest_at])
        .map_err(|_| error("X3", "the digest is not 16 bytes"))?;
    let stages = (entries[1..count - 1].iter())
        .map(|(_, range)| stage(&part[range.clone()]))
        .collect::<Result<Vec<_>, _>>()?;
    let s = stub_at.start;
    let stub_words = word(part, s + 4, "S1")?;
    need(
        word(part, s, "S1")? == version
            && stub_words & STUB_FLAG != 0
            && word(part, s + 8, "S1")? == DXIL_MAGIC
            && word(part, s + 12, "S1")? == DXIL_VERSION
            && word(part, s + 16, "S1")? == STUB_BITCODE_OFFSET
            && word(part, s + 20, "S1")? == 4
            && &part[s + 24..s + 28] == NOBC,
        "S1",
        || "the stub is not the program's header with a NOBC bitcode".to_owned(),
    )?;
    let kind = version >> 16;
    let slots: Vec<usize> = stages.iter().map(|stage| stage.slot).collect();
    need(
        (KIND_SLOTS.iter()).any(|(k, allowed)| *k == kind && allowed.contains(&slots.as_slice())),
        "F9",
        || format!("a program of kind {kind} with slots {slots:?}"),
    )?;
    Ok(XdatProgram {
        program_version: version,
        digest,
        stages,
        stub_words: stub_words & !STUB_FLAG,
    })
}

/// One `XBCH` block, inflated and its package read.
fn stage(block: &[u8]) -> Result<XdatStage, XdatError> {
    need(
        block.starts_with(b"XBCH") && word(block, 4, "B1")? == BLOCK_VERSION,
        "B1",
        || "not an XBCH block of version 3".to_owned(),
    )?;
    let compressed = dword(block, 8, "B2")?;
    need(
        compressed.checked_add(BLOCK_HEAD as u64) == Some(block.len() as u64),
        "B2",
        || {
            format!(
                "a compressed size of {compressed} bytes in an entry of {}",
                block.len()
            )
        },
    )?;
    let raw = dword(block, 16, "B3")?;
    let raw = (usize::try_from(raw).ok())
        .filter(|&raw| raw <= MAX_PACKAGE)
        .ok_or_else(|| {
            error(
                "B3",
                format!("a raw size of {raw} bytes, past the {MAX_PACKAGE} one is read to"),
            )
        })?;
    let package = inflate(&block[BLOCK_HEAD..], raw)?;
    let (slot, stage_word, code) = package_stage(&package)?;
    Ok(XdatStage {
        slot,
        stage_word,
        package,
        code,
    })
}

/// `stream` inflated as zlib, where it ends exactly and gives `raw` bytes.
fn inflate(stream: &[u8], raw: usize) -> Result<Vec<u8>, XdatError> {
    let mut z = Decompress::new(true);
    let mut out = Vec::with_capacity(raw + 1);
    let status = (z.decompress_vec(stream, &mut out, FlushDecompress::Finish))
        .map_err(|e| error("B3", format!("the zlib stream does not inflate: {e}")))?;
    need(
        status == Status::StreamEnd && z.total_in() == stream.len() as u64 && out.len() == raw,
        "B3",
        || format!("the zlib stream gives {} of {raw} bytes", out.len()),
    )?;
    Ok(out)
}

/// The used slot, its stage word and its code range in package `m`.
fn package_stage(m: &[u8]) -> Result<(usize, u32, Range<usize>), XdatError> {
    let len = m.len();
    need(
        len >= MIN_PACKAGE && m.starts_with(b"FSSM") && word(m, 4, "F1")? == FSSM_VERSION,
        "F1",
        || format!("not an FSSM package of version 0x101 and {MIN_PACKAGE} bytes or more"),
    )?;
    need(
        word(m, 8, "F2")? as usize == len - 16 && word(m, 12, "F2")? == 0,
        "F2",
        || format!("the head does not state the {} bytes after it", len - 16),
    )?;
    need(
        word(m, 0x10, "F3")? == 6 && word(m, 0x14, "F3")? == 0,
        "F3",
        || "the words after the head are not 6 and 0".to_owned(),
    )?;
    let mut used = Vec::new();
    for slot in 0..SLOTS {
        let value = dword(m, SLOT_TABLE + 8 * slot, "F4")?;
        if value != EMPTY {
            used.push((slot, value));
        }
    }
    let &[(slot, value)] = used.as_slice() else {
        return Err(error("F4", format!("{} slots are used", used.len())));
    };
    need(value == DESCRIPTOR, "F4", || {
        format!("slot {slot}'s descriptor at {value:#X}, where 0xB0 was due")
    })?;
    let package_word = word(m, PACKAGE_WORD, "F10")?;
    need(PACKAGE_WORDS.contains(&package_word), "F10", || {
        format!("the word after the slots is {package_word:#X}")
    })?;
    // F1's minimum size holds the descriptor.
    let d = BASE + DESCRIPTOR as usize;
    let (code_offset, code_size) = (dword(m, d, "F6")?, dword(m, d + 8, "F6")?);
    let code = (usize::try_from(code_offset).ok())
        .and_then(|offset| BASE.checked_add(offset))
        .and_then(|start| Some(start..start.checked_add(usize::try_from(code_size).ok()?)?))
        .filter(|code| !code.is_empty() && code.end <= len)
        .ok_or_else(|| {
            error(
                "F6",
                format!("code at {code_offset:#X}, {code_size} bytes, is outside the package"),
            )
        })?;
    let bytes = &m[code.clone()];
    need(
        bytes.len().is_multiple_of(CODE_ALIGN)
            && bytes.ends_with(&S_CODE_END.to_le_bytes())
            && (bytes.as_chunks::<4>().0.iter()).any(|w| *w == S_ENDPGM.to_le_bytes()),
        "F7",
        || {
            "the code is not whole 64-byte lines holding s_endpgm and ending in s_code_end"
                .to_owned()
        },
    )?;
    let next = dword(m, d + 0x28, "F8")?;
    need(
        next == EMPTY || Some(next) == code_offset.checked_add(code_size),
        "F8",
        || format!("the descriptor's next offset {next:#X} is neither the code's end nor empty"),
    )?;
    need(
        word(m, d + 0x10, "F11")? == 0
            && word(m, d + 0x1C, "F11")? == 0
            && word(m, d + 0x20, "F11")? == 0
            && word(m, d + 0x24, "F11")? == 0
            && DESCRIPTOR_WORD_18.contains(&word(m, d + 0x18, "F11")?),
        "F11",
        || "the descriptor's words at +0x10 to +0x24 are not the measured ones".to_owned(),
    )?;
    let stage_word = word(m, d + 0x40, "F13")?;
    need(
        SLOT_STAGE_WORDS.contains(&(slot, stage_word)),
        "F13",
        || format!("slot {slot} with the stage word {stage_word:#X}"),
    )?;
    Ok((slot, stage_word, code))
}

impl XdatProgram {
    /// The DXIL part: each package deflated again as zlib, the digest and stub words as held.
    pub fn write(&self) -> io::Result<Vec<u8>> {
        let mut pieces = vec![(DIGEST, self.digest.to_vec())];
        for stage in &self.stages {
            pieces.push((XBCH, block(&stage.package)?));
        }
        let mut stub = Vec::with_capacity(STUB_SIZE);
        for w in [
            self.program_version,
            STUB_FLAG | (self.stub_words & !STUB_FLAG),
            DXIL_MAGIC,
            DXIL_VERSION,
            STUB_BITCODE_OFFSET,
            4,
        ] {
            stub.extend_from_slice(&w.to_le_bytes());
        }
        stub.extend_from_slice(NOBC);
        pieces.push((STUB, stub));

        let start = PROGRAM_HEADER + XDAT_HEAD + ENTRY * pieces.len();
        let (mut entries, mut body) = (Vec::with_capacity(pieces.len()), Vec::<u8>::new());
        for (kind, bytes) in &pieces {
            entries.push((*kind, start + body.len(), bytes.len()));
            body.extend_from_slice(bytes);
            body.resize(body.len().next_multiple_of(4), 0);
        }
        let len = start + body.len();
        let size = u32::try_from(len)
            .map_err(|_| io::Error::other(format!("a DXIL part of {len} bytes")))?;
        let mut part = Vec::with_capacity(len);
        for w in [
            self.program_version,
            size / 4,
            DXIL_MAGIC,
            DXIL_VERSION,
            size - 12,
            4,
        ] {
            part.extend_from_slice(&w.to_le_bytes());
        }
        part.extend_from_slice(b"XDAT");
        part.extend_from_slice(&(pieces.len() as u32).to_le_bytes());
        part.extend_from_slice(&XDAT_WORD.to_le_bytes());
        // Every offset and size is below `size`, which fits a u32.
        for (kind, at, n) in entries {
            for w in [kind, at as u32, n as u32] {
                part.extend_from_slice(&w.to_le_bytes());
            }
        }
        part.extend(body);
        Ok(part)
    }
}

/// `package` as an `XBCH` block.
fn block(package: &[u8]) -> io::Result<Vec<u8>> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(package)?;
    let stream = z.finish()?;
    let mut out = b"XBCH".to_vec();
    out.extend_from_slice(&BLOCK_VERSION.to_le_bytes());
    out.extend_from_slice(&(stream.len() as u64).to_le_bytes());
    out.extend_from_slice(&(package.len() as u64).to_le_bytes());
    out.extend(stream);
    Ok(out)
}

#[cfg(test)]
#[path = "xdat_tests.rs"]
mod tests;
