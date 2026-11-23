//! The `grcEffect` a `.fxc` file holds: its variables, constant buffers, techniques and programs.

use crate::error::{IoError, Result};
use crate::gta5::shaders::sps::DRAW_BUCKET_PRESET;

const RGXE_MAGIC: &[u8; 4] = b"rgxe";

/// The six program banks, in the order a `.fxc` stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrcProgramStage {
    Vertex,
    Pixel,
    Compute,
    Domain,
    Geometry,
    Hull,
}

impl GrcProgramStage {
    pub const ALL: [GrcProgramStage; 6] = [
        Self::Vertex,
        Self::Pixel,
        Self::Compute,
        Self::Domain,
        Self::Geometry,
        Self::Hull,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::Pixel => "pixel",
            Self::Compute => "compute",
            Self::Domain => "domain",
            Self::Geometry => "geometry",
            Self::Hull => "hull",
        }
    }
}

/// What a `grcEffect` variable holds, as the file spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrcVariableType {
    Float,
    Float2,
    Float3,
    Float4,
    Texture,
    Bool,
    Float3x4,
    Float4x4,
    Int,
    Int4,
    Buffer,
    UavBuffer,
    UavTexture,
    Unknown(u8),
}

impl GrcVariableType {
    fn from_code(code: u8) -> Self {
        match code {
            2 => Self::Float,
            3 => Self::Float2,
            4 => Self::Float3,
            5 => Self::Float4,
            6 => Self::Texture,
            7 => Self::Bool,
            8 => Self::Float3x4,
            9 => Self::Float4x4,
            11 => Self::Int,
            14 => Self::Int4,
            15 => Self::Buffer,
            21 => Self::UavBuffer,
            22 => Self::UavTexture,
            other => Self::Unknown(other),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Float => 2,
            Self::Float2 => 3,
            Self::Float3 => 4,
            Self::Float4 => 5,
            Self::Texture => 6,
            Self::Bool => 7,
            Self::Float3x4 => 8,
            Self::Float4x4 => 9,
            Self::Int => 11,
            Self::Int4 => 14,
            Self::Buffer => 15,
            Self::UavBuffer => 21,
            Self::UavTexture => 22,
            Self::Unknown(c) => c,
        }
    }

    /// A resource binding rather than a constant-buffer member.
    pub fn is_resource(self) -> bool {
        matches!(
            self,
            Self::Texture | Self::Buffer | Self::UavBuffer | Self::UavTexture
        )
    }

    /// ⚠ The default-value run is `u32` for these and `f32` for the rest, and
    /// nothing in the record says which — the type is the only discriminator.
    fn values_are_unsigned(self) -> bool {
        matches!(
            self,
            Self::Int | Self::Int4 | Self::Bool | Self::Texture | Self::Buffer
        ) || matches!(self, Self::UavBuffer | Self::UavTexture)
    }
}

/// An annotation on a variable — `UIName`, `UIHint`, `UIWidget` and friends.
#[derive(Debug, Clone, PartialEq)]
pub struct GrcVariableAnnotation {
    pub name: String,
    pub value: GrcAnnotationValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GrcAnnotationValue {
    Int(i32),
    Float(f32),
    String(String),
    Unknown(u8),
}

/// One declared variable: a constant-buffer member, a texture or a sampler.
#[derive(Debug, Clone, PartialEq)]
pub struct GrcEffectVariable {
    pub var_type: GrcVariableType,
    /// Array length, in elements of `var_type`.
    pub count: u8,
    pub slot: u8,
    pub group: u8,
    /// The name the material addresses this by; its hash is a `ShaderEntry::name_hash`.
    pub name: String,
    /// The name inside the shader source, which is often the same and often not.
    pub semantic: String,
    pub offset: u8,
    pub variant: u8,
    /// Jenkins hash of the owning constant buffer's name, or 0 for a global.
    pub cbuffer: u32,
    pub annotations: Vec<GrcVariableAnnotation>,
    pub default_floats: Vec<f32>,
    pub default_uints: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GrcEffectCBuffer {
    pub name: String,
    pub size: u32,
    /// Register slot per stage, in [`GrcProgramStage::ALL`] order.
    pub slots: [u16; 6],
}

/// A `__rage_drawbucket`-style constant the preset system reads before binding.
#[derive(Debug, Clone, PartialEq)]
pub struct GrcEffectPreset {
    pub name: String,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GrcEffectTechnique {
    pub name: String,
    pub passes: Vec<GrcEffectPass>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GrcEffectPass {
    /// Program index per stage, in [`GrcProgramStage::ALL`] order. ⚠ **One-based**:
    /// 0 means the pass binds nothing at that stage.
    pub programs: [u8; 6],
    pub render_states: Vec<(u32, u32)>,
}

/// One compiled program, located in the source bytes rather than copied out of them.
#[derive(Debug, Clone, PartialEq)]
pub struct GrcEffectProgram {
    pub stage: GrcProgramStage,
    pub name: String,
    /// The variables this program binds, by name, in declaration order.
    pub variables: Vec<String>,
    pub cbuffers: Vec<(String, u16)>,
    pub bytecode_offset: usize,
    pub bytecode_size: usize,
    pub shader_model: Option<(u8, u8)>,
}

/// A parsed `.fxc`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GrcEffect {
    pub vertex_type: u32,
    pub presets: Vec<GrcEffectPreset>,
    pub programs: Vec<GrcEffectProgram>,
    pub cbuffers: Vec<GrcEffectCBuffer>,
    pub variables: Vec<GrcEffectVariable>,
    pub techniques: Vec<GrcEffectTechnique>,
}

impl GrcEffect {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::parse_counting(bytes).map(|(effect, _)| effect)
    }

    /// The effect, and how many bytes it accounted for — the second is what
    /// says the parse is right, because a `.fxc` carries no table of contents.
    pub fn parse_counting(bytes: &[u8]) -> Result<(Self, usize)> {
        let mut c = Cursor::new(bytes);
        let magic = c.take(4)?;
        if magic != RGXE_MAGIC {
            return Err(IoError::InvalidStructure {
                offset: 0,
                message: format!(
                    "expected `rgxe`, found `{}`",
                    String::from_utf8_lossy(magic).escape_debug()
                ),
            });
        }

        let vertex_type = c.u32()?;

        let preset_count = c.u8()?;
        let mut presets = Vec::with_capacity(preset_count as usize);
        for _ in 0..preset_count {
            let name = c.string()?;
            let _reserved = c.u8()?;
            presets.push(GrcEffectPreset {
                name,
                value: c.u32()?,
            });
        }

        let mut programs = Vec::new();
        for stage in GrcProgramStage::ALL {
            read_program_bank(&mut c, stage, &mut programs)?;
        }

        // Two banks of each, and the file gives no reason for the split. Nothing
        // downstream distinguishes them, so they are concatenated in file order.
        let mut cbuffers = Vec::new();
        let mut variables = Vec::new();
        for _ in 0..2 {
            let cbuffer_count = c.u8()?;
            for _ in 0..cbuffer_count {
                cbuffers.push(read_cbuffer(&mut c)?);
            }
            let variable_count = c.u8()?;
            for _ in 0..variable_count {
                variables.push(read_variable(&mut c)?);
            }
        }

        let technique_count = c.u8()?;
        let mut techniques = Vec::with_capacity(technique_count as usize);
        for _ in 0..technique_count {
            techniques.push(read_technique(&mut c)?);
        }

        Ok((
            Self {
                vertex_type,
                presets,
                programs,
                cbuffers,
                variables,
                techniques,
            },
            c.pos,
        ))
    }

    /// Bytes past the end of what [`Self::parse`] accounted for.
    pub fn trailing_bytes(bytes: &[u8]) -> Result<usize> {
        let (_, consumed) = Self::parse_counting(bytes)?;
        Ok(bytes.len().saturating_sub(consumed))
    }

    pub fn variable(&self, name: &str) -> Option<&GrcEffectVariable> {
        self.variables.iter().find(|v| v.name == name)
    }

    /// The `__rage_drawbucket` preset, which is where a material's render bucket comes from.
    pub fn draw_bucket(&self) -> Option<u32> {
        self.presets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(DRAW_BUCKET_PRESET))
            .map(|p| p.value)
    }
}

fn read_program_bank(
    c: &mut Cursor<'_>,
    stage: GrcProgramStage,
    out: &mut Vec<GrcEffectProgram>,
) -> Result<()> {
    // ⚠ A zero count is a pad byte, not an empty bank: the real count follows it.
    let mut count = c.u8()?;
    if count == 0 {
        count = c.u8()?;
    }
    let _bank_name = c.string()?;
    let _ = c.u8()?;
    let _ = c.u8()?;
    let _ = c.u32()?;

    // The count is one more than the number of programs stored.
    for _ in 1..count {
        out.push(read_program(c, stage)?);
    }
    Ok(())
}

fn read_program(c: &mut Cursor<'_>, stage: GrcProgramStage) -> Result<GrcEffectProgram> {
    let mut name = c.string()?;
    if name.is_empty() {
        name = c.string()?;
    }

    let variables = c.string_array()?;

    let cbuffer_count = c.u8()?;
    let mut cbuffers = Vec::with_capacity(cbuffer_count as usize);
    for _ in 0..cbuffer_count {
        let n = c.string()?;
        cbuffers.push((n, c.u16()?));
    }

    if stage == GrcProgramStage::Geometry {
        let _ = c.u8()?;
    }

    let length = c.u32()? as usize;
    let mut bytecode_offset = 0;
    let mut shader_model = None;
    if length > 0 {
        let at = c.pos;
        let magic = c.peek(4)?;
        if magic != b"DXBC" {
            return Err(IoError::InvalidStructure {
                offset: at as u64,
                message: format!(
                    "program `{name}` declares {length} bytes but does not start with `DXBC`"
                ),
            });
        }
        bytecode_offset = at;
        c.skip(length)?;
        if matches!(
            stage,
            GrcProgramStage::Vertex | GrcProgramStage::Pixel | GrcProgramStage::Geometry
        ) {
            shader_model = Some((c.u8()?, c.u8()?));
        }
    }

    Ok(GrcEffectProgram {
        stage,
        name,
        variables,
        cbuffers,
        bytecode_offset,
        bytecode_size: length,
        shader_model,
    })
}

fn read_cbuffer(c: &mut Cursor<'_>) -> Result<GrcEffectCBuffer> {
    let size = c.u32()?;
    let mut slots = [0u16; 6];
    for slot in slots.iter_mut() {
        *slot = c.u16()?;
    }
    Ok(GrcEffectCBuffer {
        size,
        slots,
        name: c.string()?,
    })
}

fn read_variable(c: &mut Cursor<'_>) -> Result<GrcEffectVariable> {
    let var_type = GrcVariableType::from_code(c.u8()?);
    let count = c.u8()?;
    let slot = c.u8()?;
    let group = c.u8()?;
    let name = c.string()?;
    let semantic = c.string()?;
    let offset = c.u8()?;
    let variant = c.u8()?;
    let _ = c.u8()?;
    let _ = c.u8()?;
    let cbuffer = c.u32()?;

    let annotation_count = c.u8()?;
    let mut annotations = Vec::with_capacity(annotation_count as usize);
    for _ in 0..annotation_count {
        let name = c.string()?;
        let value = match c.u8()? {
            0 => GrcAnnotationValue::Int(c.u32()? as i32),
            1 => GrcAnnotationValue::Float(f32::from_bits(c.u32()?)),
            2 => GrcAnnotationValue::String(c.string()?),
            other => GrcAnnotationValue::Unknown(other),
        };
        annotations.push(GrcVariableAnnotation { name, value });
    }

    let value_count = c.u8()?;
    let mut default_floats = Vec::new();
    let mut default_uints = Vec::new();
    if var_type.values_are_unsigned() {
        default_uints.reserve(value_count as usize);
        for _ in 0..value_count {
            default_uints.push(c.u32()?);
        }
    } else {
        default_floats.reserve(value_count as usize);
        for _ in 0..value_count {
            default_floats.push(f32::from_bits(c.u32()?));
        }
    }

    Ok(GrcEffectVariable {
        var_type,
        count,
        slot,
        group,
        name,
        semantic,
        offset,
        variant,
        cbuffer,
        annotations,
        default_floats,
        default_uints,
    })
}

fn read_technique(c: &mut Cursor<'_>) -> Result<GrcEffectTechnique> {
    let name = c.string()?;
    let pass_count = c.u8()?;
    let mut passes = Vec::with_capacity(pass_count as usize);
    for _ in 0..pass_count {
        let mut programs = [0u8; 6];
        for p in programs.iter_mut() {
            *p = c.u8()?;
        }
        let state_count = c.u8()?;
        let mut render_states = Vec::with_capacity(state_count as usize);
        for _ in 0..state_count {
            render_states.push((c.u32()?, c.u32()?));
        }
        passes.push(GrcEffectPass {
            programs,
            render_states,
        });
    }
    Ok(GrcEffectTechnique { name, passes })
}

pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(IoError::PointerOutOfBounds {
            offset: self.pos as u64,
            size: self.bytes.len(),
        })?;
        if end > self.bytes.len() {
            return Err(IoError::PointerOutOfBounds {
                offset: end as u64,
                size: self.bytes.len(),
            });
        }
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    pub(crate) fn peek(&self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.saturating_add(n);
        if end > self.bytes.len() {
            return Err(IoError::PointerOutOfBounds {
                offset: end as u64,
                size: self.bytes.len(),
            });
        }
        Ok(&self.bytes[self.pos..end])
    }

    pub(crate) fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// A length byte that counts the terminator, so `0` and `1` both mean empty.
    pub(crate) fn string(&mut self) -> Result<String> {
        let len = self.u8()? as usize;
        if len == 0 {
            return Ok(String::new());
        }
        let raw = self.take(len)?;
        Ok(String::from_utf8_lossy(&raw[..len - 1]).into_owned())
    }

    fn string_array(&mut self) -> Result<Vec<String>> {
        let count = self.u8()? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.string()?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(s: &str) -> Vec<u8> {
        let mut out = vec![s.len() as u8 + 1];
        out.extend_from_slice(s.as_bytes());
        out.push(0);
        out
    }

    fn empty_bank() -> Vec<u8> {
        let mut out = vec![1u8];
        out.extend_from_slice(&string("NULL"));
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        out
    }

    /// A minimal well-formed `.fxc`: header, six empty banks, no tables.
    fn skeleton() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"rgxe");
        out.extend_from_slice(&0x59u32.to_le_bytes());
        out.push(0);
        for _ in 0..6 {
            out.extend_from_slice(&empty_bank());
        }
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.push(0);
        out
    }

    #[test]
    fn a_skeleton_parses_to_nothing_but_its_vertex_type() {
        let effect = GrcEffect::parse(&skeleton()).unwrap();
        assert_eq!(effect.vertex_type, 0x59);
        assert!(effect.programs.is_empty());
        assert!(effect.variables.is_empty());
        assert!(effect.techniques.is_empty());
        assert_eq!(GrcEffect::trailing_bytes(&skeleton()).unwrap(), 0);
    }

    #[test]
    fn a_file_without_the_magic_is_refused() {
        let mut bytes = skeleton();
        bytes[..4].copy_from_slice(b"RSC7");
        assert!(matches!(
            GrcEffect::parse(&bytes),
            Err(IoError::InvalidStructure { offset: 0, .. })
        ));
    }

    #[test]
    fn a_truncated_file_is_refused_rather_than_read_short() {
        let bytes = skeleton();
        for cut in 1..bytes.len() {
            assert!(
                GrcEffect::parse(&bytes[..cut]).is_err(),
                "a {cut}-byte prefix of a {}-byte file parsed",
                bytes.len()
            );
        }
    }

    #[test]
    fn a_preset_carries_its_value() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"rgxe");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&string(DRAW_BUCKET_PRESET));
        bytes.push(0);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for _ in 0..6 {
            bytes.extend_from_slice(&empty_bank());
        }
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.push(0);

        let effect = GrcEffect::parse(&bytes).unwrap();
        assert_eq!(effect.draw_bucket(), Some(2));
    }

    /// ⚠ A bank whose first byte is zero has its count in the *next* byte, and
    /// reading the zero as the count silently shifts every later table by one.
    #[test]
    fn a_padded_bank_takes_its_count_from_the_byte_after_the_pad() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"rgxe");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(0);
        for i in 0..6 {
            if i == 5 {
                bytes.push(0);
            }
            bytes.extend_from_slice(&empty_bank());
        }
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.push(0);

        let effect = GrcEffect::parse(&bytes).unwrap();
        assert!(effect.programs.is_empty());
        assert_eq!(GrcEffect::trailing_bytes(&bytes).unwrap(), 0);
    }

    #[test]
    fn an_unknown_type_code_survives_its_round_trip() {
        let t = GrcVariableType::from_code(200);
        assert_eq!(t, GrcVariableType::Unknown(200));
        assert_eq!(t.code(), 200);
        assert!(!t.is_resource());
    }
}
