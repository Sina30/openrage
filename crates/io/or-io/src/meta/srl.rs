//! A binary streaming request list (`.srl`): the names each frame of a
//! cutscene requests and releases, as four big-endian counted `u32` arrays
//! (the requested hashes, each frame's first index into them, then the same
//! two for the released ones).

use crate::meta::error::{MetaError, Result};
use crate::meta::instance::{HashedString, MetaInstance, Value};

/// The record a binary `.srl` reads as.
pub const RECORD: &str = "CStreamingRequestRecord";
/// The type of each of its frames.
pub const FRAME: &str = "CStreamingRequestFrame";

fn other(why: String) -> MetaError {
    MetaError::Other(format!("binary .srl: {why}"))
}

/// `bytes` as a record of frames, refused unless the four arrays are the whole file.
pub fn read(bytes: &[u8]) -> Result<MetaInstance> {
    let mut at = 0;
    let adds = counted(bytes, &mut at)?;
    let add_starts = counted(bytes, &mut at)?;
    let removes = counted(bytes, &mut at)?;
    let remove_starts = counted(bytes, &mut at)?;
    if at != bytes.len() {
        return Err(other(format!(
            "{} bytes follow the four arrays",
            bytes.len() - at
        )));
    }
    if add_starts.len() != remove_starts.len() {
        return Err(other(format!(
            "{} frames request and {} release",
            add_starts.len(),
            remove_starts.len()
        )));
    }
    let frames = per_frame(&adds, &add_starts)?
        .into_iter()
        .zip(per_frame(&removes, &remove_starts)?)
        .map(|(add, remove)| {
            let mut frame = MetaInstance::new(FRAME);
            frame.set("AddList", names(add));
            frame.set("RemoveList", names(remove));
            Value::Struct(Box::new(frame))
        })
        .collect();
    let mut record = MetaInstance::new(RECORD);
    record.set("Frames", Value::Array(frames));
    Ok(record)
}

fn counted(bytes: &[u8], at: &mut usize) -> Result<Vec<u32>> {
    let words = |from: usize, to: usize| {
        bytes[from..to]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]))
            .collect::<Vec<u32>>()
    };
    if bytes.len() < *at + 4 {
        return Err(other(format!("the file ends at {} before a count", *at)));
    }
    let count = words(*at, *at + 4)[0] as usize;
    let end = count
        .checked_mul(4)
        .and_then(|n| n.checked_add(*at + 4))
        .filter(|&end| end <= bytes.len())
        .ok_or_else(|| other(format!("{count} words at {} run past the end", *at)))?;
    let out = words(*at + 4, end);
    *at = end;
    Ok(out)
}

/// `names` cut at each frame's first index, so that every name is in exactly one frame.
fn per_frame<'a>(names: &'a [u32], starts: &[u32]) -> Result<Vec<&'a [u32]>> {
    if starts
        .first()
        .map_or(!names.is_empty(), |&first| first != 0)
    {
        return Err(other("a name comes before the first frame".into()));
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts.get(i + 1).map_or(names.len(), |&e| e as usize);
            names.get(start as usize..end).ok_or_else(|| {
                other(format!(
                    "frame {i} spans {start}..{end} of {} names",
                    names.len()
                ))
            })
        })
        .collect()
}

fn names(hashes: &[u32]) -> Value {
    Value::Array(
        hashes
            .iter()
            .map(|&h| Value::Hash(HashedString::from_hash(h)))
            .collect(),
    )
}

/// `record` as a binary `.srl`, refused when it holds a value the four arrays have no place for.
pub fn write(record: &MetaInstance) -> Result<Vec<u8>> {
    let mut arrays: [Vec<u32>; 4] = Default::default();
    for (name, value) in &record.fields {
        match (name.as_str(), value) {
            ("Frames", Value::Array(frames)) => {
                for (i, frame) in frames.iter().enumerate() {
                    frame_into(frame, &mut arrays)
                        .map_err(|why| other(format!("frame {i}: {why}")))?;
                }
            }
            _ if is_unset(value) => {}
            _ => return Err(other(format!("it has no place for `{name}`"))),
        }
    }
    let mut out = Vec::new();
    for array in &arrays {
        out.extend_from_slice(&word(array.len())?.to_be_bytes());
        for w in array {
            out.extend_from_slice(&w.to_be_bytes());
        }
    }
    Ok(out)
}

fn word(n: usize) -> Result<u32> {
    u32::try_from(n).map_err(|_| other(format!("{n} does not fit a word")))
}

fn frame_into(
    frame: &Value,
    [adds, add_starts, removes, remove_starts]: &mut [Vec<u32>; 4],
) -> std::result::Result<(), String> {
    let (Value::Struct(frame) | Value::Pointer(Some(frame))) = frame else {
        return Err(format!("{frame:?} is not a frame"));
    };
    add_starts.push(word(adds.len()).map_err(|e| e.to_string())?);
    remove_starts.push(word(removes.len()).map_err(|e| e.to_string())?);
    for (name, value) in &frame.fields {
        let into = match name.as_str() {
            "AddList" => &mut *adds,
            "RemoveList" => &mut *removes,
            _ if is_unset(value) => continue,
            _ => return Err(format!("it has no place for `{name}`")),
        };
        if is_unset(value) {
            continue;
        }
        let Value::Array(items) = value else {
            return Err(format!("`{name}` is not a list"));
        };
        for item in items {
            into.push(match item {
                Value::Hash(h) => h.hash,
                Value::String(s) => HashedString::from_xml_text(s).hash,
                other => return Err(format!("`{name}` holds {other:?}, not a name")),
            });
        }
    }
    Ok(())
}

/// Whether `value` is what a member nobody set holds, which a binary `.srl` may drop.
fn is_unset(value: &Value) -> bool {
    match value {
        Value::Null | Value::Pointer(None) => true,
        Value::Bool(b) => !b,
        Value::Int(v) => *v == 0,
        Value::UInt(v) => *v == 0,
        Value::Float(v) => *v == 0.0,
        Value::String(s) | Value::Raw(s) => s.is_empty(),
        Value::Hash(h) => h.hash == 0,
        Value::Vec2(v) => v.iter().all(|c| *c == 0.0),
        Value::Vec3(v) => v.iter().all(|c| *c == 0.0),
        Value::Vec4(v) => v.iter().all(|c| *c == 0.0),
        Value::Color(c) => *c == 0,
        Value::Bitset { bits, .. } => *bits == 0,
        Value::Array(items) => items.is_empty(),
        Value::Map(pairs) => pairs.is_empty(),
        Value::ByteArray(bytes) => bytes.is_empty(),
        Value::Struct(inner) => inner.fields.iter().all(|(_, v)| is_unset(v)),
        Value::Mat34(_) | Value::Enum { .. } | Value::Pointer(Some(_)) => false,
    }
}
