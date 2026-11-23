//! The readings of a value the table form's writer takes, from what the reader writes and from
//! what XML read with no schema gives, and the hashes its tables place keys by.

use crate::meta::instance::value::{HashedString, MetaInstance, Value};

pub(super) fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}

pub(super) fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    k ^= k >> 33;
    k = k.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    k ^ (k >> 33)
}

pub(super) fn mask(v: u64, width: usize) -> u64 {
    match width {
        8 => v,
        w => v & ((1u64 << (8 * w)) - 1),
    }
}

/// `v` in `width` little-endian bytes, `None` where it does not fit.
pub(super) fn in_width(v: i128, width: usize, signed: bool) -> Option<Vec<u8>> {
    let bits = 8 * width as u32;
    let fits = match signed {
        true => v >= -(1i128 << (bits - 1)) && v < (1i128 << (bits - 1)),
        false => v >= 0 && v < (1i128 << bits),
    };
    fits.then(|| (v as u128).to_le_bytes()[..width].to_vec())
}

pub(super) fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub(super) fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "nothing",
        Value::Bool(_) => "a bool",
        Value::Int(_) | Value::UInt(_) => "an integer",
        Value::Float(_) => "a float",
        Value::String(_) => "text",
        Value::Hash(_) => "a hash",
        Value::Vec2(_) | Value::Vec3(_) | Value::Vec4(_) => "a vector",
        Value::Mat34(_) => "a matrix",
        Value::Color(_) => "a colour",
        Value::Enum { .. } => "an enum",
        Value::Bitset { .. } => "a bitset",
        Value::Struct(_) => "a structure",
        Value::Pointer(_) => "a pointer",
        Value::Array(_) => "an array",
        Value::Map(_) => "a map",
        Value::ByteArray(_) => "bytes",
        Value::Raw(_) => "an undecoded placeholder",
    }
}

// The readings below take what the reader writes and what XML read with no schema gives.

pub(super) fn int(v: &Value) -> Option<i128> {
    match v {
        Value::Int(i) => Some((*i).into()),
        Value::UInt(u) => Some((*u).into()),
        Value::Bool(b) => Some((*b).into()),
        Value::Float(f) if f.fract() == 0.0 && f.abs() < 1e18 => Some(*f as i128),
        Value::String(s) => match s.trim().strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).ok().map(i128::from),
            None => s.trim().parse().ok(),
        },
        _ => None,
    }
}

pub(super) fn float(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub(super) fn boolean(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Int(0) | Value::UInt(0) => Some(false),
        Value::Int(1) | Value::UInt(1) => Some(true),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub(super) fn floats(v: &Value, n: usize) -> Option<Vec<f32>> {
    let all: Vec<f32> = match v {
        Value::Vec2(a) => a.to_vec(),
        Value::Vec3(a) => a.to_vec(),
        Value::Vec4(a) => a.to_vec(),
        Value::Mat34(m) => m.concat(),
        Value::String(s) => s
            .split_ascii_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()
            .ok()?,
        _ => return None,
    };
    (all.len() == n).then_some(all)
}

pub(super) fn hash32(v: &Value) -> Option<u32> {
    match v {
        Value::Hash(h) => Some(h.hash),
        Value::String(s) => Some(HashedString::from_xml_text(s).hash),
        other => int(other).and_then(|v| u32::try_from(v).ok()),
    }
}

/// A 64-bit string hash: the number, or the hash of the text that names it.
pub(super) fn hash64(v: &Value) -> Option<u64> {
    match v {
        // ⚠ The text as it is, empty included: the reader names a hash by any text that hashes to it.
        Value::String(s) => Some(crate::gta6::hash::name_hash64(s.as_bytes())),
        other => int(other).and_then(|v| u64::try_from(v).ok()),
    }
}

pub(super) fn text(v: &Value) -> Option<&str> {
    match v {
        Value::String(s) => Some(s),
        _ => None,
    }
}

pub(super) fn instance(v: &Value) -> Option<&MetaInstance> {
    match v {
        Value::Struct(inst) => Some(inst),
        _ => None,
    }
}

pub(super) fn items(v: &Value) -> Option<&[Value]> {
    match v {
        Value::Array(items) => Some(items),
        Value::String(s) if s.is_empty() => Some(&[]),
        _ => None,
    }
}

/// A map's pairs: a map, or the `Key` and `Value` children XML gives it.
pub(super) fn pairs(v: &Value) -> Option<Vec<(&Value, &Value)>> {
    match v {
        Value::Map(pairs) => Some(pairs.iter().map(|(k, v)| (k, v)).collect()),
        Value::String(s) if s.is_empty() => Some(Vec::new()),
        Value::Struct(inst) => {
            let fields = &inst.fields;
            let paired = fields.len() % 2 == 0
                && fields
                    .chunks(2)
                    .all(|c| c[0].0 == "Key" && c[1].0 == "Value");
            paired.then(|| fields.chunks(2).map(|c| (&c[0].1, &c[1].1)).collect())
        }
        _ => None,
    }
}

pub(super) fn bitset(v: &Value) -> Option<u128> {
    match v {
        Value::Bitset { bits, .. } => Some(*bits),
        Value::String(s) => u128::from_str_radix(s.trim().strip_prefix("0x")?, 16).ok(),
        other => int(other).and_then(|v| u128::try_from(v).ok()),
    }
}

/// A bitset's words and the bits it needs.
pub(super) fn bitset_value(v: &Value) -> Option<(Vec<u8>, usize)> {
    match v {
        Value::ByteArray(bytes) => {
            let high = bytes.iter().rposition(|&b| b != 0);
            Some((
                bytes.clone(),
                high.map_or(0, |i| 8 * i + 8 - bytes[i].leading_zeros() as usize),
            ))
        }
        other => {
            let bits = bitset(other)?;
            Some((
                bits.to_le_bytes().to_vec(),
                128 - bits.leading_zeros() as usize,
            ))
        }
    }
}

/// A bitset as `len` bytes, `None` where its bits do not fit.
pub(super) fn bitset_words(v: &Value, len: usize) -> Option<Vec<u8>> {
    let (mut bytes, _) = bitset_value(v)?;
    if bytes.iter().skip(len).any(|&b| b != 0) {
        return None;
    }
    bytes.resize(len, 0);
    Some(bytes)
}
