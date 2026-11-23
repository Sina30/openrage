//! `RDEF`: what a compiled shader says about its own constant buffers and
//! resource bindings.
//!
//! ⚠ **The variable stride depends on the chunk's own version.** Shader model
//! 5 writes 40 bytes per variable and 4 writes 24, and the extra 16 bytes are
//! at the *end* — so a reader using the wrong stride reads the first variable
//! correctly and every later one from the wrong offset. The major version is
//! in the chunk header and is what selects it.

use crate::error::{IoError, Result};

/// What a resource binding is.
///
/// ⚠ The numbering is D3D's `D3D_SHADER_INPUT_TYPE` and is not contiguous in
/// meaning: `Texture` and the typed UAV share a return-type field that a
/// sampler does not use at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DxbcBindKind {
    CBuffer,
    TBuffer,
    Texture,
    Sampler,
    UavTyped,
    Structured,
    UavStructured,
    ByteAddress,
    UavByteAddress,
    UavAppend,
    UavConsume,
    UavCounter,
    Other(u32),
}

impl DxbcBindKind {
    fn of(raw: u32) -> Self {
        match raw {
            0 => Self::CBuffer,
            1 => Self::TBuffer,
            2 => Self::Texture,
            3 => Self::Sampler,
            4 => Self::UavTyped,
            5 => Self::Structured,
            6 => Self::UavStructured,
            7 => Self::ByteAddress,
            8 => Self::UavByteAddress,
            9 => Self::UavAppend,
            10 => Self::UavConsume,
            11 => Self::UavCounter,
            other => Self::Other(other),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CBuffer => "cbuffer",
            Self::TBuffer => "tbuffer",
            Self::Texture => "texture",
            Self::Sampler => "sampler",
            Self::UavTyped => "uav",
            Self::Structured => "structured",
            Self::UavStructured => "uav-structured",
            Self::ByteAddress => "byteaddress",
            Self::UavByteAddress => "uav-byteaddress",
            Self::UavAppend => "uav-append",
            Self::UavConsume => "uav-consume",
            Self::UavCounter => "uav-counter",
            Self::Other(_) => "other",
        }
    }
}

/// The scalar a variable is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DxbcScalar {
    Bool,
    Int,
    Float,
    UInt,
    Double,
    Other(u16),
}

impl DxbcScalar {
    fn of(raw: u16) -> Self {
        match raw {
            1 => Self::Bool,
            2 => Self::Int,
            3 => Self::Float,
            19 => Self::UInt,
            21 => Self::Double,
            other => Self::Other(other),
        }
    }

    /// The scalar as HLSL spells it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::UInt => "uint",
            Self::Double => "double",
            Self::Other(_) => "?",
        }
    }
}

/// A variable's shape: how many rows, columns and array elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DxbcShape {
    pub scalar: DxbcScalar,
    pub rows: u16,
    pub columns: u16,
    /// Zero for a non-array, which is how the chunk spells it.
    pub elements: u16,
}

impl DxbcShape {
    /// The type as HLSL would spell it — `float4`, `float4x4`, `float4[8]`.
    pub fn hlsl(&self) -> String {
        let scalar = self.scalar.as_str();
        let base = match (self.rows, self.columns) {
            (1, 1) => scalar.to_string(),
            (1, c) => format!("{scalar}{c}"),
            (r, c) => format!("{scalar}{r}x{c}"),
        };
        if self.elements > 1 {
            format!("{base}[{}]", self.elements)
        } else {
            base
        }
    }
}

/// One variable inside a constant buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxbcVariable {
    pub name: String,
    /// Byte offset from the start of the buffer.
    pub offset: u32,
    pub size: u32,
    pub shape: Option<DxbcShape>,
    /// ⚠ **The compiler drops what the shader never reads**, and says so
    /// here. A variable that is not used still occupies its offset, so a
    /// caller laying the buffer out must not skip it — but a caller asking
    /// *what this shader needs* should.
    pub used: bool,
}

/// One constant buffer, with its members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxbcConstantBuffer {
    pub name: String,
    pub size: u32,
    pub variables: Vec<DxbcVariable>,
}

impl DxbcConstantBuffer {
    /// The buffer as a struct declaration, for a reader rather than a
    /// compiler.
    ///
    /// ⚠ Offsets are stated rather than implied. A constant buffer is not
    /// tightly packed — a `float4` never straddles a 16-byte boundary — so a
    /// struct written from the member list alone does not lay out the same.
    pub fn as_struct(&self) -> String {
        let mut out = format!("cbuffer {} {{  // {} bytes\n", self.name, self.size);
        for v in &self.variables {
            out.push_str(&format!(
                "    +{:<5} {:<16} {}{}\n",
                v.offset,
                v.shape.map(|s| s.hlsl()).unwrap_or_else(|| "?".into()),
                v.name,
                if v.used { "" } else { "  (unused)" }
            ));
        }
        out.push_str("}\n");
        out
    }
}

/// One resource binding: what it is and which register it sits in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxbcBinding {
    pub name: String,
    pub kind: DxbcBindKind,
    /// The register — `b3`, `t5`, `s12`. This is what the slot table is
    /// keyed by.
    pub register: u32,
    pub count: u32,
}

/// A compiled shader's own account of what it binds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DxbcReflection {
    pub constant_buffers: Vec<DxbcConstantBuffer>,
    pub bindings: Vec<DxbcBinding>,
    pub major_version: u8,
    pub minor_version: u8,
}

impl DxbcReflection {
    /// Reflect an `RDEF` chunk's payload.
    pub fn parse(rdef: &[u8]) -> Result<Self> {
        let cb_count = u32_at(rdef, 0x00)?;
        let cb_offset = u32_at(rdef, 0x04)? as usize;
        let bind_count = u32_at(rdef, 0x08)?;
        let bind_offset = u32_at(rdef, 0x0C)? as usize;
        let minor_version = *byte_at(rdef, 0x10)?;
        let major_version = *byte_at(rdef, 0x11)?;

        let variable_stride: usize = if major_version >= 5 { 40 } else { 24 };

        let mut constant_buffers = Vec::with_capacity(cb_count as usize);
        for i in 0..cb_count as usize {
            let at = cb_offset + i * 24;
            let name = string_at(rdef, u32_at(rdef, at)? as usize)?;
            let var_count = u32_at(rdef, at + 0x04)?;
            let var_offset = u32_at(rdef, at + 0x08)? as usize;
            let size = u32_at(rdef, at + 0x0C)?;

            let mut variables = Vec::with_capacity(var_count as usize);
            for v in 0..var_count as usize {
                let vat = var_offset + v * variable_stride;
                variables.push(DxbcVariable {
                    name: string_at(rdef, u32_at(rdef, vat)? as usize)?,
                    offset: u32_at(rdef, vat + 0x04)?,
                    size: u32_at(rdef, vat + 0x08)?,
                    // Bit 1 of the variable flags is `D3D_SVF_USED`.
                    used: u32_at(rdef, vat + 0x0C)? & 0x2 != 0,
                    shape: shape_at(rdef, u32_at(rdef, vat + 0x10)? as usize),
                });
            }
            constant_buffers.push(DxbcConstantBuffer {
                name,
                size,
                variables,
            });
        }

        let mut bindings = Vec::with_capacity(bind_count as usize);
        for i in 0..bind_count as usize {
            let at = bind_offset + i * 32;
            bindings.push(DxbcBinding {
                name: string_at(rdef, u32_at(rdef, at)? as usize)?,
                kind: DxbcBindKind::of(u32_at(rdef, at + 0x04)?),
                register: u32_at(rdef, at + 0x14)?,
                count: u32_at(rdef, at + 0x18)?,
            });
        }

        Ok(Self {
            constant_buffers,
            bindings,
            major_version,
            minor_version,
        })
    }

    /// Every register this shader binds, by kind.
    pub fn registers_of(&self, kind: DxbcBindKind) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .bindings
            .iter()
            .filter(|b| b.kind == kind)
            .map(|b| b.register)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn shape_at(rdef: &[u8], at: usize) -> Option<DxbcShape> {
    let u16_at = |off: usize| -> Option<u16> {
        let bytes = rdef.get(at + off..at + off + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    };
    Some(DxbcShape {
        scalar: DxbcScalar::of(u16_at(0x02)?),
        rows: u16_at(0x04)?,
        columns: u16_at(0x06)?,
        elements: u16_at(0x08)?,
    })
}

fn byte_at(bytes: &[u8], at: usize) -> Result<&u8> {
    bytes.get(at).ok_or_else(|| IoError::InvalidData {
        context: format!("RDEF is {} bytes and offset {at} was wanted", bytes.len()),
    })
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let slice = bytes.get(at..at + 4).ok_or_else(|| IoError::InvalidData {
        context: format!(
            "RDEF is {} bytes and a word at {at} was wanted",
            bytes.len()
        ),
    })?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// A NUL-terminated name at a chunk-relative offset.
fn string_at(bytes: &[u8], at: usize) -> Result<String> {
    let tail = bytes.get(at..).ok_or_else(|| IoError::InvalidData {
        context: format!(
            "RDEF is {} bytes and a name at {at} was wanted",
            bytes.len()
        ),
    })?;
    let end = tail.iter().position(|b| *b == 0).unwrap_or(tail.len());
    Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One buffer holding one `float4x4`, and one texture at `t3`.
    fn rdef(major: u8) -> Vec<u8> {
        let variable_stride: usize = if major >= 5 { 40 } else { 24 };
        // Layout: header (0x1C), cb table, binding table, var table, type,
        // strings.
        let cb_at = 0x20usize;
        let bind_at = cb_at + 24;
        let var_at = bind_at + 32;
        let type_at = var_at + variable_stride;
        let strings_at = type_at + 16;

        let mut out = vec![0u8; strings_at];
        let mut names = Vec::new();
        let push_name = |s: &str, names: &mut Vec<u8>| -> u32 {
            let at = strings_at + names.len();
            names.extend_from_slice(s.as_bytes());
            names.push(0);
            at as u32
        };
        let cb_name = push_name("Globals", &mut names);
        let var_name = push_name("gWorld", &mut names);
        let tex_name = push_name("DiffuseSampler", &mut names);

        let w = |out: &mut Vec<u8>, at: usize, v: u32| {
            out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        };
        w(&mut out, 0x00, 1); // one constant buffer
        w(&mut out, 0x04, cb_at as u32);
        w(&mut out, 0x08, 1); // one binding
        w(&mut out, 0x0C, bind_at as u32);
        out[0x10] = 0; // minor
        out[0x11] = major;

        w(&mut out, cb_at, cb_name);
        w(&mut out, cb_at + 0x04, 1); // one variable
        w(&mut out, cb_at + 0x08, var_at as u32);
        w(&mut out, cb_at + 0x0C, 64); // bytes

        w(&mut out, var_at, var_name);
        w(&mut out, var_at + 0x04, 0); // offset
        w(&mut out, var_at + 0x08, 64); // size
        w(&mut out, var_at + 0x0C, 0x2); // used
        w(&mut out, var_at + 0x10, type_at as u32);

        // class=matrix_columns(3), type=float(3), 4 rows, 4 columns.
        out[type_at..type_at + 2].copy_from_slice(&3u16.to_le_bytes());
        out[type_at + 2..type_at + 4].copy_from_slice(&3u16.to_le_bytes());
        out[type_at + 4..type_at + 6].copy_from_slice(&4u16.to_le_bytes());
        out[type_at + 6..type_at + 8].copy_from_slice(&4u16.to_le_bytes());

        w(&mut out, bind_at, tex_name);
        w(&mut out, bind_at + 0x04, 2); // texture
        w(&mut out, bind_at + 0x14, 3); // register t3
        w(&mut out, bind_at + 0x18, 1); // count

        out.extend_from_slice(&names);
        out
    }

    #[test]
    fn a_constant_buffer_reads_as_named_typed_members() {
        let reflection = DxbcReflection::parse(&rdef(5)).expect("parses");
        assert_eq!(reflection.major_version, 5);
        assert_eq!(reflection.constant_buffers.len(), 1);

        let cb = &reflection.constant_buffers[0];
        assert_eq!(cb.name, "Globals");
        assert_eq!(cb.size, 64);
        assert_eq!(cb.variables.len(), 1);

        let v = &cb.variables[0];
        assert_eq!(v.name, "gWorld");
        assert_eq!(v.offset, 0);
        assert!(v.used);
        assert_eq!(v.shape.expect("typed").hlsl(), "float4x4");
    }

    /// ⚠ The stride the version selects. A shader-model-4 chunk read at
    /// model 5's stride gets the first variable right and the rest wrong, so
    /// both are exercised.
    #[test]
    fn both_variable_strides_read_the_same_variable() {
        for major in [4u8, 5] {
            let reflection = DxbcReflection::parse(&rdef(major)).expect("parses");
            assert_eq!(reflection.major_version, major);
            let v = &reflection.constant_buffers[0].variables[0];
            assert_eq!(v.name, "gWorld", "major {major}");
            assert_eq!(v.size, 64, "major {major}");
        }
    }

    #[test]
    fn a_binding_names_its_register_and_kind() {
        let reflection = DxbcReflection::parse(&rdef(5)).expect("parses");
        assert_eq!(reflection.bindings.len(), 1);
        let b = &reflection.bindings[0];
        assert_eq!(b.name, "DiffuseSampler");
        assert_eq!(b.kind, DxbcBindKind::Texture);
        assert_eq!(b.register, 3);
        assert_eq!(reflection.registers_of(DxbcBindKind::Texture), vec![3]);
        assert!(reflection.registers_of(DxbcBindKind::CBuffer).is_empty());
    }

    /// ⚠ A struct printed from the member list alone would not lay out the
    /// same, so the offsets are stated rather than implied.
    #[test]
    fn the_struct_form_states_every_offset() {
        let reflection = DxbcReflection::parse(&rdef(5)).expect("parses");
        let text = reflection.constant_buffers[0].as_struct();
        assert!(text.contains("64 bytes"), "{text}");
        assert!(text.contains("+0"), "{text}");
        assert!(text.contains("gWorld"), "{text}");
    }

    #[test]
    fn a_truncated_chunk_is_refused_rather_than_read_past() {
        let full = rdef(5);
        for len in [0usize, 4, 0x10, 0x20] {
            assert!(
                DxbcReflection::parse(&full[..len.min(full.len())]).is_err(),
                "a {len}-byte RDEF must not parse"
            );
        }
    }

    #[test]
    fn a_shape_spells_scalars_vectors_matrices_and_arrays() {
        let shape = |rows, columns, elements| DxbcShape {
            scalar: DxbcScalar::Float,
            rows,
            columns,
            elements,
        };
        assert_eq!(shape(1, 1, 0).hlsl(), "float");
        assert_eq!(shape(1, 4, 0).hlsl(), "float4");
        assert_eq!(shape(4, 4, 0).hlsl(), "float4x4");
        assert_eq!(shape(1, 4, 8).hlsl(), "float4[8]");
    }
}
