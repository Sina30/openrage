use std::io::Cursor;

use byteorder::{LittleEndian, ReadBytesExt};

use crate::meta::diagnostics::Diagnostics;
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::{MetaInstance, Value};
use crate::meta::schema::SchemaLibrary;

use super::layout::{RbfLayout, RbfStructShape};

const RBF_IDENT: u32 = 0x30464252;
/// GTA VI's `FB FF` token with version 1, which widens 0x10 and 0x40 values to 8 bytes.
pub(crate) const WIDE_MARKER: [u8; 4] = [0xFB, 0xFF, 0x01, 0x00];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RbfDataType {
    None,
    UInt32,
    BoolTrue,
    BoolFalse,
    Float,
    Float3,
    String,
    Other(u8),
}

impl RbfDataType {
    pub(crate) fn from_byte(b: u8) -> Self {
        match b {
            0x00 => RbfDataType::None,
            0x10 => RbfDataType::UInt32,
            0x20 => RbfDataType::BoolTrue,
            0x30 => RbfDataType::BoolFalse,
            0x40 => RbfDataType::Float,
            0x50 => RbfDataType::Float3,
            0x60 => RbfDataType::String,
            other => RbfDataType::Other(other),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Descriptor {
    pub(crate) name: String,
    #[allow(dead_code)]
    pub(crate) ty: RbfDataType,
}

pub fn parse(library: &SchemaLibrary, bytes: &[u8]) -> Result<MetaInstance> {
    let mut diags = Diagnostics::lenient();
    parse_with(library, bytes, &mut diags)
}

pub fn parse_with(
    library: &SchemaLibrary,
    bytes: &[u8],
    diags: &mut Diagnostics,
) -> Result<MetaInstance> {
    parse_full(library, bytes, diags).map(|(inst, _)| inst)
}

pub(crate) fn parse_full(
    _library: &SchemaLibrary,
    bytes: &[u8],
    _diags: &mut Diagnostics,
) -> Result<(MetaInstance, RbfLayout)> {
    let mut layout = RbfLayout::default();
    let mut cursor = Cursor::new(bytes);
    let ident = cursor.read_u32::<LittleEndian>()?;
    if ident != RBF_IDENT {
        return Err(MetaError::Other(format!(
            "rbf: ident mismatch (got {ident:#X}, expected {RBF_IDENT:#X})"
        )));
    }
    if bytes.get(4..6) == Some(&WIDE_MARKER[..2]) {
        if bytes.get(4..8) != Some(&WIDE_MARKER[..]) {
            return Err(MetaError::Other(format!(
                "rbf: the marker after the magic is {:02X?}, where every measured one is FB FF 01 00",
                &bytes[4..bytes.len().min(8)]
            )));
        }
        layout.wide_values = true;
        cursor.set_position(8);
    }
    let wide = layout.wide_values;
    let mut descriptors: Vec<Descriptor> = Vec::new();
    let mut stack: Vec<MetaInstance> = Vec::new();
    let mut current: Option<MetaInstance> = None;

    while (cursor.position() as usize) < bytes.len() {
        let descriptor_index = cursor.read_u8()?;
        if descriptor_index == 0xFF {
            let b = cursor.read_u8()?;
            if b != 0xFF {
                return Err(MetaError::Other(format!(
                    "rbf: expected 0xFF close marker but got {b:#X}"
                )));
            }
            if let Some(parent) = stack.pop() {
                let mut parent = parent;
                if let Some(closed) = current.take() {
                    parent
                        .fields
                        .push((closed.type_name.clone(), Value::Struct(Box::new(closed))));
                }
                current = Some(parent);
            } else {
                if cursor.position() as usize != bytes.len() {
                    return Err(MetaError::Other(
                        "rbf: trailing bytes after root close".into(),
                    ));
                }
                let root = current.ok_or_else(|| MetaError::Other("rbf: empty document".into()))?;
                return Ok((root, layout));
            }
            continue;
        }
        if descriptor_index == 0xFD {
            let b = cursor.read_u8()?;
            if b != 0xFF {
                return Err(MetaError::Other(format!(
                    "rbf: expected 0xFF after 0xFD but got {b:#X}"
                )));
            }
            let data_length = cursor.read_i32::<LittleEndian>()?;
            let payload = take(&mut cursor, data_length.into(), "bytes payload")?.to_vec();

            let cur = current
                .as_mut()
                .ok_or_else(|| MetaError::Other("rbf: bytes payload outside any struct".into()))?;
            let datatype = match cur.fields.iter().find(|(n, _)| n == "content") {
                Some((_, Value::String(s))) => s.clone(),
                _ => "string".to_string(),
            };
            layout
                .content_kinds
                .insert(cur.type_name.clone(), datatype.clone());
            let value = match datatype.as_str() {
                "char_array" => Value::ByteArray(payload),
                "short_array" => {
                    let count = payload.len() / 2;
                    let mut shorts = Vec::with_capacity(count);
                    for i in 0..count {
                        let lo = payload[2 * i] as u16;
                        let hi = payload[2 * i + 1] as u16;
                        shorts.push(Value::UInt(((hi << 8) | lo) as u64));
                    }
                    Value::Array(shorts)
                }
                // With GTA VI's marker a text is kept only when it gives back the payload: its
                // UTF-8 and one NUL (meta-write Q5 re-encodes every token as stored).
                _ if wide
                    && !(payload.split_last()).is_some_and(|(nul, text)| {
                        *nul == 0 && !text.contains(&0) && std::str::from_utf8(text).is_ok()
                    }) =>
                {
                    Value::ByteArray(payload)
                }
                _ => {
                    let end = payload
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(payload.len());
                    Value::String(String::from_utf8_lossy(&payload[..end]).into_owned())
                }
            };
            cur.fields.push(("Content".to_string(), value));
            continue;
        }

        let data_type = RbfDataType::from_byte(cursor.read_u8()?);
        let resolved_index = if descriptor_index as usize == descriptors.len() {
            let name_length = cursor.read_i16::<LittleEndian>()?;
            let name = take(&mut cursor, name_length.into(), "descriptor name")?;
            let name = String::from_utf8_lossy(name).into_owned();
            layout.note_descriptor(&name);
            descriptors.push(Descriptor {
                name,
                ty: data_type,
            });
            descriptors.len() - 1
        } else {
            descriptor_index as usize
        };
        let descriptor = &descriptors
            .get(resolved_index)
            .ok_or_else(|| {
                MetaError::Other(format!(
                    "rbf: descriptor {resolved_index} is past the {} the stream has named",
                    descriptors.len()
                ))
            })?
            .clone();

        match data_type {
            RbfDataType::None => {
                let mut child = MetaInstance::new(&descriptor.name);
                let shape = RbfStructShape {
                    x1: cursor.read_i16::<LittleEndian>()?,
                    x2: cursor.read_i16::<LittleEndian>()?,
                    attribute_count: cursor.read_i16::<LittleEndian>()?,
                };
                layout.record_shape(&descriptor.name, shape);
                (layout.occurrences.entry(descriptor.name.clone()))
                    .or_default()
                    .push(shape);
                if let Some(prev) = current.take() {
                    stack.push(prev);
                }
                child.type_name = descriptor.name.clone();
                current = Some(child);
            }
            RbfDataType::UInt32 if wide => {
                // ⚠ The high word is the low word's sign extension or zero.
                let v = cursor.read_i64::<LittleEndian>()?;
                let value = match v {
                    0.. => Value::UInt(v as u64),
                    _ => Value::Int(v),
                };
                push_field(&mut current, &descriptor.name, value)?;
            }
            RbfDataType::UInt32 => {
                let v = cursor.read_u32::<LittleEndian>()?;
                push_field(&mut current, &descriptor.name, Value::UInt(v as u64))?;
            }
            RbfDataType::Float if wide => {
                let v = cursor.read_f64::<LittleEndian>()?;
                push_field(&mut current, &descriptor.name, Value::Float(v))?;
            }
            RbfDataType::BoolTrue => {
                push_field(&mut current, &descriptor.name, Value::Bool(true))?;
            }
            RbfDataType::BoolFalse => {
                push_field(&mut current, &descriptor.name, Value::Bool(false))?;
            }
            RbfDataType::Float => {
                let v = cursor.read_f32::<LittleEndian>()?;
                push_field(&mut current, &descriptor.name, Value::Float(v as f64))?;
            }
            RbfDataType::Float3 => {
                let x = cursor.read_f32::<LittleEndian>()?;
                let y = cursor.read_f32::<LittleEndian>()?;
                let z = cursor.read_f32::<LittleEndian>()?;
                push_field(&mut current, &descriptor.name, Value::Vec3([x, y, z]))?;
            }
            RbfDataType::String => {
                let len = cursor.read_i16::<LittleEndian>()?;
                let s =
                    String::from_utf8_lossy(take(&mut cursor, len.into(), "string")?).into_owned();
                push_field(&mut current, &descriptor.name, Value::String(s))?;
            }
            RbfDataType::Other(b) => {
                return Err(MetaError::Other(format!(
                    "rbf: unsupported data type {b:#X}"
                )));
            }
        }
    }

    Err(MetaError::Other(
        "rbf: unexpected end of stream (no closing 0xFF 0xFF)".into(),
    ))
}

/// The `len` bytes at the cursor, which it then passes; a negative `len` is refused.
fn take<'a>(cursor: &mut Cursor<&'a [u8]>, len: i64, what: &str) -> Result<&'a [u8]> {
    let all: &'a [u8] = cursor.get_ref();
    let start = cursor.position() as usize;
    let bytes = usize::try_from(len)
        .ok()
        .and_then(|len| all.get(start..start.checked_add(len)?))
        .ok_or_else(|| MetaError::Other(format!("rbf: {what} OOB")))?;
    cursor.set_position((start + bytes.len()) as u64);
    Ok(bytes)
}

fn push_field(current: &mut Option<MetaInstance>, name: &str, value: Value) -> Result<()> {
    let cur = current
        .as_mut()
        .ok_or_else(|| MetaError::Other("rbf: value outside any struct".into()))?;
    cur.fields.push((name.to_string(), value));
    Ok(())
}
