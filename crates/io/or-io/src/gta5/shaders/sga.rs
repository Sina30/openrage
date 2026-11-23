//! `sga_win32_60_final.awc` and its `_init` sibling: the shader container
//! Enhanced ships in place of `.fxc`, holding every compiled program and every
//! effect's declared variables.
//!
//! The file carries no table of contents and no specification exists, so the
//! layout is measured, and consuming every byte ([`SgaFile::parse`] refuses a
//! remainder) together with each name's stored `atStringHash` is the check
//! that the parse landed on the right fields.

use std::ops::Range;

use crate::error::{IoError, Result};
use crate::gta5::shaders::fxc::Cursor;

const MAGIC: &[u8; 4] = b"SGD2";

/// Program banks a file stores and an effect indexes into, one per pipeline stage.
pub const BANKS: usize = 8;

/// Bytes between a program's bytecode and its variable table, unread.
const PROGRAM_STATE: usize = 152;
/// Bytes between an effect's program lists and its rasterizer states, unread.
const EFFECT_STATE: usize = 80;

const RESOURCE: usize = 28;
const CONSTANT: usize = 24;
const ANNOTATION: usize = 8;
const CONSTANT_BUFFER: u8 = 0x30;

/// A parsed container.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SgaFile {
    pub programs: Vec<SgaProgram>,
    pub effects: Vec<SgaEffect>,
}

/// One compiled program, located in the source bytes rather than copied out of them.
#[derive(Debug, Clone, PartialEq)]
pub struct SgaProgram {
    pub bank: usize,
    pub name: String,
    /// The `DXBC` container, as a range of the parsed bytes.
    pub bytecode: Range<usize>,
}

/// One effect: what it declares, and the programs it draws with.
#[derive(Debug, Clone, PartialEq)]
pub struct SgaEffect {
    pub name: String,
    /// Indices into each bank's programs, in [`SgaFile::programs`] bank order.
    pub programs: [Vec<u32>; BANKS],
    pub variables: Vec<SgaVariable>,
}

/// One declared variable: a resource binding, or a member of a constant buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct SgaVariable {
    pub name: String,
    /// The shader-side spelling, when the file states one.
    pub semantic: Option<String>,
    pub var_type: SgaVariableType,
    /// Array length in elements; 0 for a scalar.
    pub count: u16,
    /// A resource's register, or a constant's byte offset into its buffer.
    pub slot: u16,
    /// The constant buffer a constant is a member of.
    pub cbuffer: Option<String>,
    /// The declared default as raw words, for a constant whose width is known.
    pub default: Vec<u32>,
}

/// A variable's type as far as it is measured.
///
/// ⚠ Constants and resources number their types separately, and a resource
/// sets `0x40` for an array of what the low bits name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SgaVariableType {
    Bool,
    Int,
    Float,
    Float2,
    Float3,
    Float4,
    Float3x4,
    Float4x4,
    Texture,
    UavTexture,
    Buffer,
    UavBuffer,
    Sampler,
    /// A constant type code whose meaning is not measured.
    OtherConstant(u8),
    /// A resource type code whose meaning is not measured.
    OtherResource(u8),
}

impl SgaVariableType {
    pub(crate) fn constant(code: u8) -> Self {
        match code {
            0 => Self::Bool,
            1 => Self::Int,
            9 => Self::Float,
            10 => Self::Float2,
            11 => Self::Float3,
            12 => Self::Float4,
            13 => Self::Float3x4,
            14 => Self::Float4x4,
            other => Self::OtherConstant(other),
        }
    }

    pub(crate) fn resource(code: u8) -> Self {
        match code & 0x3F {
            0x02 => Self::Texture,
            0x1C => Self::UavTexture,
            0x01 | 0x05 | 0x07 | 0x0E => Self::Buffer,
            0x14 | 0x16 | 0x18 => Self::UavBuffer,
            0x23 => Self::Sampler,
            _ => Self::OtherResource(code),
        }
    }

    /// A binding rather than a member of a constant buffer.
    pub fn is_resource(self) -> bool {
        matches!(
            self,
            Self::Texture
                | Self::UavTexture
                | Self::Buffer
                | Self::UavBuffer
                | Self::Sampler
                | Self::OtherResource(_)
        )
    }

    /// Words one element's default takes, for the types whose width is measured.
    fn words(self) -> Option<usize> {
        Some(match self {
            Self::Bool | Self::Int | Self::Float => 1,
            Self::Float2 => 2,
            Self::Float3 => 3,
            Self::Float4 => 4,
            Self::Float3x4 => 12,
            Self::Float4x4 => 16,
            _ => return None,
        })
    }
}

impl SgaFile {
    /// The whole container; a byte left over after the last effect is refused.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        if c.take(4)? != MAGIC {
            return Err(invalid(0, "expected `SGD2`".into()));
        }
        let mut programs = Vec::new();
        for bank in 0..BANKS {
            for _ in 0..c.u32()? {
                programs.push(read_program(&mut c, bank)?);
            }
        }
        let mut effects = Vec::new();
        for _ in 0..c.u32()? {
            effects.push(read_effect(&mut c)?);
        }
        if c.pos != bytes.len() {
            return Err(invalid(
                c.pos,
                format!("{} bytes past the last effect", bytes.len() - c.pos),
            ));
        }
        Ok(Self { programs, effects })
    }
}

fn read_program(c: &mut Cursor<'_>, bank: usize) -> Result<SgaProgram> {
    let name = c.string()?;
    // ⚠ 0x32, 0x41 or 0x42 in every program shipped; what it distinguishes is not measured.
    c.u8()?;
    let size = c.u32()? as usize;
    let at = c.pos;
    if c.peek(4)? != b"DXBC" {
        return Err(invalid(
            at,
            format!("program `{name}` does not start with `DXBC`"),
        ));
    }
    c.skip(size)?;
    c.skip(PROGRAM_STATE)?;
    let table = c.u32()? as usize;
    c.skip(table)?;
    Ok(SgaProgram {
        bank,
        name,
        bytecode: at..at + size,
    })
}

fn read_effect(c: &mut Cursor<'_>) -> Result<SgaEffect> {
    let name = c.string()?;
    // ⚠ Not the vertex type its `.fxc` states: `default` holds 0x4968 here and 0x59 there.
    c.u32()?;
    let mut programs: [Vec<u32>; BANKS] = Default::default();
    for list in &mut programs {
        for _ in 0..c.u32()? {
            list.push(c.u32()?);
        }
    }
    c.skip(EFFECT_STATE)?;
    // Rasterizer, depth-stencil, blend and sampler states, then techniques.
    for stride in [16, 24, 68, 36, 4] {
        skip_records(c, stride)?;
    }
    let passes = c.u32()? as usize;
    c.skip(passes.saturating_mul(24))?;
    c.u32()?;
    // Four lists of bound name hashes per pass.
    for _ in 0..passes.saturating_mul(4) {
        skip_records(c, 4)?;
    }
    skip_records(c, 2)?;
    let at = c.pos + 4;
    let variables = read_table(sized(c)?, at)?;
    let at = c.pos + 4;
    read_table(sized(c)?, at)?;
    for _ in 0..c.u32()? {
        sized(c)?;
    }
    // Technique and pass names.
    sized(c)?;
    Ok(SgaEffect {
        name,
        programs,
        variables,
    })
}

fn skip_records(c: &mut Cursor<'_>, stride: usize) -> Result<()> {
    let count = c.u32()? as usize;
    c.skip(count.saturating_mul(stride))
}

fn sized<'a>(c: &mut Cursor<'a>) -> Result<&'a [u8]> {
    let size = c.u32()? as usize;
    c.take(size)
}

/// A variable table: resources, then constants, then annotations, then the
/// strings and defaults they reach by offsets relative to their own record.
fn read_table(table: &[u8], at: usize) -> Result<Vec<SgaVariable>> {
    let mut c = Cursor::new(table);
    let resources = c.u16()? as usize;
    let constants = c.u16()? as usize;
    let annotations = c.u16()? as usize;
    let size = c.u16()? as usize;
    let first_constant = 8 + RESOURCE * resources;
    if size != table.len()
        || first_constant + CONSTANT * constants + ANNOTATION * annotations > size
    {
        return Err(invalid(
            at,
            format!(
                "a {}-byte variable table states {size} bytes for {resources} resources, \
                 {constants} constants and {annotations} annotations",
                table.len()
            ),
        ));
    }

    let mut out = Vec::new();
    let mut members: Vec<Option<String>> = vec![None; constants];
    for i in 0..resources {
        let r = 8 + RESOURCE * i;
        let (name, semantic) = names(table, r, r + 10, r + 20, at)?;
        let code = table[r];
        if code == CONSTANT_BUFFER {
            let count = word(table, r + 4) as usize;
            let first = r + word(table, r + 8) as usize;
            let index = first.wrapping_sub(first_constant) / CONSTANT;
            if count > 0
                && (first < first_constant
                    || !(first - first_constant).is_multiple_of(CONSTANT)
                    || index + count > constants)
            {
                return Err(invalid(
                    at + r,
                    format!("constant buffer `{name}` places its members outside the table"),
                ));
            }
            for member in members.iter_mut().skip(index).take(count) {
                *member = Some(name.clone());
            }
            continue;
        }
        out.push(SgaVariable {
            name,
            semantic,
            var_type: SgaVariableType::resource(code),
            count: 0,
            slot: table[r + 2].into(),
            cbuffer: None,
            default: Vec::new(),
        });
    }
    for (k, cbuffer) in members.into_iter().enumerate() {
        let r = first_constant + CONSTANT * k;
        let (name, semantic) = names(table, r, r + 6, r + 16, at)?;
        let var_type = SgaVariableType::constant(table[r]);
        let count = word(table, r + 2);
        let default_at = word(table, r + 10) as usize;
        let default = match var_type.words() {
            Some(words) if default_at != 0 => {
                let len = words * usize::from(count.max(1)) * 4;
                let bytes = table
                    .get(r + default_at..r + default_at + len)
                    .ok_or_else(|| {
                        invalid(at + r, format!("`{name}`'s default runs past its table"))
                    })?;
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                    .collect()
            }
            _ => Vec::new(),
        };
        out.push(SgaVariable {
            name,
            semantic,
            var_type,
            count,
            slot: word(table, r + 4),
            cbuffer,
            default,
        });
    }
    Ok(out)
}

/// A record's name and semantic, each checked against the hash stored after them.
///
/// ⚠ A semantic offset of 0 means none, and its stored hash is then 0.
fn names(
    table: &[u8],
    record: usize,
    offsets: usize,
    hashes: usize,
    at: usize,
) -> Result<(String, Option<String>)> {
    let name = string(table, record + word(table, offsets) as usize, at)?;
    let semantic_at = word(table, offsets + 2) as usize;
    let semantic = (semantic_at != 0)
        .then(|| string(table, record + semantic_at, at))
        .transpose()?;
    let stated = [dword(table, hashes), dword(table, hashes + 4)];
    let computed = [
        crate::string_hash(name.as_bytes()),
        semantic
            .as_deref()
            .map_or(0, |s| crate::string_hash(s.as_bytes())),
    ];
    if stated != computed {
        return Err(invalid(
            at + record,
            format!("`{name}` does not hash to the {stated:08X?} its record states"),
        ));
    }
    Ok((name, semantic))
}

fn string(table: &[u8], from: usize, at: usize) -> Result<String> {
    let tail = table.get(from..).unwrap_or_default();
    let end = tail
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| invalid(at + from, "a name runs past its table".into()))?;
    Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
}

fn word(table: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([table[at], table[at + 1]])
}

fn dword(table: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([table[at], table[at + 1], table[at + 2], table[at + 3]])
}

fn invalid(offset: usize, message: String) -> IoError {
    IoError::InvalidStructure {
        offset: offset as u64,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(s: &str) -> [u8; 4] {
        crate::string_hash(s.as_bytes()).to_le_bytes()
    }

    /// One constant buffer `locals` holding one `float4 colour` defaulting to 1.0.
    fn table() -> Vec<u8> {
        let strings_at = 8 + RESOURCE + CONSTANT;
        let mut t = vec![1, 0, 1, 0, 0, 0, 0, 0];
        let mut cb = vec![CONSTANT_BUFFER, 4, 3, 0, 1, 0, 0, 0];
        cb.extend_from_slice(&(RESOURCE as u16).to_le_bytes());
        cb.extend_from_slice(&((strings_at - 8) as u16).to_le_bytes());
        cb.extend_from_slice(&((strings_at - 8) as u16).to_le_bytes());
        cb.extend_from_slice(&[16, 0, 0, 0, 0, 0]);
        cb.extend_from_slice(&hash("locals"));
        cb.extend_from_slice(&hash("locals"));
        t.extend_from_slice(&cb);
        let r = t.len();
        let name_at = (strings_at + 7 - r) as u16;
        let default_at = (strings_at + 7 + 7 - r) as u16;
        t.extend_from_slice(&[12, 0, 0, 0, 0, 0]);
        t.extend_from_slice(&name_at.to_le_bytes());
        t.extend_from_slice(&name_at.to_le_bytes());
        t.extend_from_slice(&default_at.to_le_bytes());
        t.extend_from_slice(&[0, 0, 0, 0]);
        t.extend_from_slice(&hash("colour"));
        t.extend_from_slice(&hash("colour"));
        t.extend_from_slice(b"locals\0colour\0");
        for _ in 0..4 {
            t.extend_from_slice(&1.0f32.to_le_bytes());
        }
        let size = t.len() as u16;
        t[6..8].copy_from_slice(&size.to_le_bytes());
        t
    }

    #[test]
    fn a_constant_takes_its_buffer_its_offset_and_its_default() {
        let vars = read_table(&table(), 0).unwrap();
        assert_eq!(vars.len(), 1, "a constant buffer is not itself a variable");
        let v = &vars[0];
        assert_eq!(v.name, "colour");
        assert_eq!(v.var_type, SgaVariableType::Float4);
        assert_eq!(v.cbuffer.as_deref(), Some("locals"));
        assert_eq!(v.default, vec![1.0f32.to_bits(); 4]);
    }

    #[test]
    fn a_name_that_does_not_hash_to_its_record_is_refused() {
        let mut t = table();
        let at = t.len() - 16 - 7;
        t[at] = b'x';
        assert!(read_table(&t, 0).is_err());
    }

    #[test]
    fn a_table_whose_stated_size_disagrees_is_refused() {
        let mut t = table();
        t.push(0);
        assert!(read_table(&t, 0).is_err());
    }

    #[test]
    fn an_empty_container_parses_and_a_remainder_is_refused() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&[0; 4 * (BANKS + 1)]);
        assert_eq!(SgaFile::parse(&bytes).unwrap(), SgaFile::default());
        bytes.push(0);
        assert!(SgaFile::parse(&bytes).is_err());
    }

    #[test]
    fn an_array_of_textures_is_a_texture() {
        assert_eq!(SgaVariableType::resource(0x42), SgaVariableType::Texture);
        assert_eq!(SgaVariableType::resource(0x5C), SgaVariableType::UavTexture);
        assert_eq!(
            SgaVariableType::constant(5),
            SgaVariableType::OtherConstant(5)
        );
    }
}
