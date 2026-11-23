//! How anything that is not an integer crosses the boundary.
//!
//! ⚠ A host function returns a [`Status`] and never an answer; the answer is
//! written into guest memory at a pointer the guest supplies. The first host
//! functions returned `i32` with `-1` for "refused", which made "there are no
//! fields" and "you may not ask" the same bits and folded the two refusals
//! [`crate::Because`] keeps apart into one value.
//!
//! ⚠ The guest owns its memory and passes `(ptr, cap)`; the host never calls a
//! guest allocator, which would run guest code inside a host call — a trap
//! mid-function, fuel spent inside host accounting, re-entry halfway through
//! an operation. A short buffer receives nothing at all, not a truncated
//! prefix a decoder that skipped the status would read as garbage, and
//! [`Status::BufferTooSmall`] reports the length that would have fitted.
//!
//! There is no `last_error`: a function handing the guest a sentence would
//! need no capability to pay for it. The sentence goes into the run's
//! diagnostics under the status's stable key.

use or_io::desc::value::ScalarValue;
use or_io::desc::{
    FieldRepr, FieldValue, Kind, Pack, PackType, Record, RecordRef, RecordRun, Value,
};

/// What a host call answers with.
///
/// ⚠ Adding a variant is an ABI change; at [`crate::ABI_VERSION`] zero it costs
/// guests a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    /// The out buffer was too small; the needed length was written to the
    /// length slot and no bytes were written to the buffer.
    BufferTooSmall = 1,
    /// The plugin was not granted the capability this call costs.
    NotGranted = 2,
    /// It was granted, and the embedder does not permit writing right now.
    NotWritable = 3,
    /// The pack describes no type by that name.
    NoSuchType = 4,
    /// The type has no field by that name.
    NoSuchField = 5,
    /// The host understood the request and will not do it.
    Refused = 6,
    /// A pointer, a length, or the bytes at one were not what they claimed.
    BadArgument = 7,
    /// The answer is larger than [`MAX_ENCODED`], so no buffer would fit it.
    TooLarge = 8,
    /// The description places the thing asked for past the end of the document.
    ///
    /// ⚠ Not [`Status::Refused`]: it means the installed pack is not the one this
    /// file was written by, and folded into a refusal it would read as ordinary.
    Truncated = 9,
    /// The pointer asked about is null in this document.
    ///
    /// ⚠ Not an error and not [`Status::Refused`]: a null child is ordinary, and
    /// a guest that could not tell it from "that field is not a pointer" would
    /// treat absence as a fault or a fault as absence.
    NullPointer = 10,
}

impl Status {
    pub const ALL: [Status; 11] = [
        Self::Ok,
        Self::BufferTooSmall,
        Self::NotGranted,
        Self::NotWritable,
        Self::NoSuchType,
        Self::NoSuchField,
        Self::Refused,
        Self::BadArgument,
        Self::TooLarge,
        Self::Truncated,
        Self::NullPointer,
    ];

    pub fn code(self) -> i32 {
        self as i32
    }

    /// The stable key the matching diagnostic carries.
    pub fn key(self) -> &'static str {
        match self {
            Self::Ok => "abi.ok",
            Self::BufferTooSmall => "abi.buffer_too_small",
            Self::NotGranted => "abi.not_granted",
            Self::NotWritable => "abi.not_writable",
            Self::NoSuchType => "abi.no_such_type",
            Self::NoSuchField => "abi.no_such_field",
            Self::Refused => "abi.refused",
            Self::BadArgument => "abi.bad_argument",
            Self::TooLarge => "abi.too_large",
            Self::Truncated => "abi.truncated",
            Self::NullPointer => "abi.null_pointer",
        }
    }

    /// One sentence for whoever installed the plugin.
    pub fn means(self) -> &'static str {
        match self {
            Self::Ok => "the call succeeded",
            Self::BufferTooSmall => {
                "the plugin's buffer was too small; the length it needs was reported to it"
            }
            Self::NotGranted => "the plugin was not granted permission for that",
            Self::NotWritable => "the plugin may do that and writing is not permitted right now",
            Self::NoSuchType => "the installed pack describes no type by that name",
            Self::NoSuchField => "that type has no field by that name",
            Self::Refused => "the host understood the request and refused it",
            Self::BadArgument => {
                "the plugin passed a pointer, a length or bytes that were not valid"
            }
            Self::TooLarge => "the answer is larger than this ABI will carry in one call",
            Self::Truncated => {
                "the pack places that field past the end of the document, so the pack and the \
                 file disagree"
            }
            Self::NullPointer => "that pointer is null in this document, which is not a fault",
        }
    }
}

/// The largest encoding, in bytes, the host will build for one call.
///
/// ⚠ A cap on the *host's* allocation, not the guest's buffer: encoding stops
/// the moment it passes this, so a million-element subtree costs one megabyte
/// and a [`Status::TooLarge`] rather than whatever the tree weighs.
pub const MAX_ENCODED: usize = 1 << 20;

/// How deeply a decoder will nest before refusing.
///
/// ⚠ **The one crash WebAssembly does not prevent.** Guest bytes reach a
/// recursive host decoder, so a value nested ten thousand deep overflows the
/// *host's* stack — inside a host function, where there is no trap to catch.
/// `a_value_nested_past_the_depth_cap_is_refused_rather_than_overflowing` is
/// the falsification.
pub const MAX_DEPTH: u32 = 32;

/// The tag byte each [`Value`] variant carries.
///
/// ⚠ [`Value::Bytes`] and [`Value::Opaque`] encode identically and keep
/// separate tags: one is a payload the description named, the other a run
/// nothing described, and collapsing them loses which.
pub mod tag {
    pub const INT: u8 = 0x01;
    pub const F32: u8 = 0x02;
    pub const F64: u8 = 0x03;
    pub const BOOL: u8 = 0x04;
    pub const ENUM: u8 = 0x05;
    pub const STRUCT: u8 = 0x06;
    pub const ARRAY: u8 = 0x07;
    pub const BYTES: u8 = 0x08;
    pub const PTR: u8 = 0x09;
    pub const ALIAS: u8 = 0x0A;
    pub const OPAQUE: u8 = 0x0B;
    pub const UNDESCRIBED_PTR: u8 = 0x0C;
    pub const NOT_AN_ADDRESS: u8 = 0x0D;
    pub const BLOB: u8 = 0x0E;
    pub const MEMBER: u8 = 0x0F;
}

/// Why a value would not cross.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    TooLarge,
    TooDeep,
    /// The bytes ended in the middle of something.
    Truncated {
        wanted: usize,
        had: usize,
    },
    UnknownTag(u8),
    NotUtf8,
    /// A bool that is neither 0 nor 1. ⚠ Refused rather than coerced: a decoder
    /// that reads 2 as `true` has silently accepted a guest that is confused
    /// about its own encoding.
    NotABool(u8),
    /// Bytes remained after a complete value. ⚠ Refused, because the surplus is
    /// either a second value the caller did not ask for or a length the guest
    /// got wrong, and neither should be discarded quietly.
    TrailingBytes(usize),
    /// A struct arrived with no pack to rebuild it against. ⚠ The encoding
    /// names fields and the tree holds bytes at offsets, and only the
    /// document's pack says which is where.
    NeedsPack {
        type_name: String,
    },
    UnknownType {
        type_name: String,
    },
    UnknownField {
        type_name: String,
        field: String,
    },
    /// ⚠ Refused rather than last-one-wins: a guest that names a field twice
    /// has one of the two values wrong and nothing says which.
    RepeatedField {
        type_name: String,
        field: String,
    },
    WrongShape {
        type_name: String,
        field: String,
        expected: String,
    },
    /// ⚠ Refused rather than left zeroed: an absent field would otherwise be
    /// written as zero, which is a value the guest never sent.
    MissingField {
        type_name: String,
        field: String,
    },
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(
                f,
                "the value is larger than the {MAX_ENCODED} bytes this ABI carries in one call"
            ),
            Self::TooDeep => write!(f, "the value nests deeper than {MAX_DEPTH}"),
            Self::Truncated { wanted, had } => {
                write!(f, "the encoding wanted {wanted} more bytes and had {had}")
            }
            Self::UnknownTag(tag) => write!(f, "{tag:#04x} is not a value tag this ABI defines"),
            Self::NotUtf8 => f.write_str("a name in the encoding is not UTF-8"),
            Self::NotABool(byte) => {
                write!(f, "{byte} is not a boolean; the encoding permits 0 or 1")
            }
            Self::TrailingBytes(n) => write!(f, "{n} bytes remained after a complete value"),
            Self::NeedsPack { type_name } => write!(
                f,
                "a `{type_name}` cannot be rebuilt without the document's pack, which says \
                 where each of its fields lives"
            ),
            Self::UnknownType { type_name } => {
                write!(f, "the pack describes no type named `{type_name}`")
            }
            Self::UnknownField { type_name, field } => {
                write!(f, "`{type_name}` declares no field named `{field}`")
            }
            Self::RepeatedField { type_name, field } => {
                write!(f, "`{type_name}.{field}` arrived more than once")
            }
            Self::WrongShape {
                type_name,
                field,
                expected,
            } => write!(f, "`{type_name}.{field}` takes {expected}"),
            Self::MissingField { type_name, field } => write!(
                f,
                "`{type_name}.{field}` is declared and did not arrive; a struct crosses whole"
            ),
        }
    }
}

impl std::error::Error for CodecError {}

impl CodecError {
    pub fn status(&self) -> Status {
        match self {
            Self::TooLarge => Status::TooLarge,
            Self::UnknownType { .. } => Status::NoSuchType,
            Self::UnknownField { .. } => Status::NoSuchField,
            Self::NeedsPack { .. } => Status::Refused,
            _ => Status::BadArgument,
        }
    }
}

/// A described value, as bytes a guest can decode without the pack.
///
/// ⚠ The `match` is exhaustive over [`Value`], so a variant added to the
/// described tree stops this compiling rather than crossing as something else.
pub fn encode(value: &Value) -> Result<Vec<u8>, CodecError> {
    let mut out = Out::default();
    write_value(&mut out, value, 0)?;
    Ok(out.bytes)
}

/// A value a guest built, back into the described tree; a struct is refused.
pub fn decode(bytes: &[u8]) -> Result<Value, CodecError> {
    decode_with(bytes, None)
}

/// A value a guest built, with structs rebuilt against the document's pack.
pub fn decode_with(bytes: &[u8], pack: Option<&Pack>) -> Result<Value, CodecError> {
    let mut input = In { bytes, at: 0 };
    let value = read_value(&mut input, 0, pack)?;
    let left = bytes.len() - input.at;
    if left > 0 {
        return Err(CodecError::TrailingBytes(left));
    }
    Ok(value)
}

#[derive(Default)]
struct Out {
    bytes: Vec<u8>,
}

impl Out {
    fn put(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        if self.bytes.len() + bytes.len() > MAX_ENCODED {
            return Err(CodecError::TooLarge);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn tag(&mut self, tag: u8) -> Result<(), CodecError> {
        self.put(&[tag])
    }

    fn str(&mut self, text: &str) -> Result<(), CodecError> {
        let len = u32::try_from(text.len()).map_err(|_| CodecError::TooLarge)?;
        self.put(&len.to_le_bytes())?;
        self.put(text.as_bytes())
    }

    fn count(&mut self, count: usize) -> Result<(), CodecError> {
        let count = u32::try_from(count).map_err(|_| CodecError::TooLarge)?;
        self.put(&count.to_le_bytes())
    }
}

fn write_scalar(out: &mut Out, value: &ScalarValue) -> Result<(), CodecError> {
    match value {
        ScalarValue::Int(v) => {
            out.tag(tag::INT)?;
            out.put(&v.to_le_bytes())
        }
        ScalarValue::F32(v) => {
            out.tag(tag::F32)?;
            out.put(&v.to_le_bytes())
        }
        ScalarValue::F64(v) => {
            out.tag(tag::F64)?;
            out.put(&v.to_le_bytes())
        }
        ScalarValue::Bool(v) => {
            out.tag(tag::BOOL)?;
            out.put(&[u8::from(*v)])
        }
    }
}

fn write_enum(out: &mut Out, enum_name: &str, value: i64) -> Result<(), CodecError> {
    out.tag(tag::ENUM)?;
    out.str(enum_name)?;
    out.put(&value.to_le_bytes())
}

/// A record as its type's declared fields by name, in declaration order.
///
/// ⚠ By name rather than as the record's bytes, because a guest has no pack:
/// the bytes are only readable to something that knows the offsets.
fn write_record(out: &mut Out, record: RecordRef<'_>, depth: u32) -> Result<(), CodecError> {
    if depth > MAX_DEPTH {
        return Err(CodecError::TooDeep);
    }
    out.tag(tag::STRUCT)?;
    out.str(record.type_name())?;
    out.count(record.plan().pack_type().fields.len())?;
    for (field, value) in record.fields() {
        out.str(&field.name)?;
        write_field(out, value, depth + 1)?;
    }
    Ok(())
}

fn write_field(out: &mut Out, value: FieldValue<'_>, depth: u32) -> Result<(), CodecError> {
    if depth > MAX_DEPTH {
        return Err(CodecError::TooDeep);
    }
    match value {
        FieldValue::Scalar(scalar) => write_scalar(out, &scalar),
        FieldValue::Enum { enum_name, value } => write_enum(out, enum_name, value),
        FieldValue::Scalars(scalars) => {
            out.tag(tag::ARRAY)?;
            out.count(scalars.len())?;
            out.put(&[0])?;
            for scalar in &scalars {
                write_scalar(out, scalar)?;
            }
            Ok(())
        }
        FieldValue::Bytes(bytes) => {
            out.tag(tag::OPAQUE)?;
            out.count(bytes.len())?;
            out.put(bytes)
        }
        FieldValue::Inline(inner) => write_record(out, inner, depth + 1),
        FieldValue::Inlines(inners) => {
            out.tag(tag::ARRAY)?;
            out.count(inners.len())?;
            out.put(&[0])?;
            for inner in inners {
                write_record(out, inner, depth + 1)?;
            }
            Ok(())
        }
        FieldValue::Child(Some(child)) => write_value(out, child, depth),
        FieldValue::Child(None) => write_value(out, &Value::null_ptr(), depth),
    }
}

fn write_value(out: &mut Out, value: &Value, depth: u32) -> Result<(), CodecError> {
    if depth > MAX_DEPTH {
        return Err(CodecError::TooDeep);
    }
    match value {
        Value::Scalar(scalar) => write_scalar(out, scalar),
        Value::Enum { enum_name, value } => write_enum(out, enum_name, *value),
        Value::Struct(record) => write_record(out, record.as_ref(), depth),
        Value::Blob { codec, parts } => {
            out.tag(tag::BLOB)?;
            out.str(codec)?;
            out.count(parts.len())?;
            for (name, part) in parts {
                out.str(name)?;
                write_value(out, part, depth + 1)?;
            }
            Ok(())
        }
        Value::Array { elements, capacity } => {
            write_array_head(out, elements.len(), *capacity)?;
            for element in elements {
                write_value(out, element, depth + 1)?;
            }
            Ok(())
        }
        // ⚠ The same bytes a run of records crosses as: how the host holds a
        // run is not the guest's business, and a new tag would be.
        Value::Records(run) => {
            write_array_head(out, run.len(), run.capacity())?;
            for element in run.iter() {
                write_record(out, element, depth + 1)?;
            }
            Ok(())
        }
        Value::Bytes(bytes) => {
            out.tag(tag::BYTES)?;
            out.count(bytes.len())?;
            out.put(bytes)
        }
        Value::Ptr { source, target } => {
            out.tag(tag::PTR)?;
            out.put(&source.to_le_bytes())?;
            match target {
                Some(target) => {
                    out.put(&[1])?;
                    write_value(out, target, depth + 1)
                }
                None => out.put(&[0]),
            }
        }
        Value::Alias(addr) => {
            out.tag(tag::ALIAS)?;
            out.put(&addr.to_le_bytes())
        }
        Value::Member { field, index } => {
            out.tag(tag::MEMBER)?;
            out.str(field)?;
            match index {
                Some(i) => {
                    out.put(&[1])?;
                    out.put(&(*i as u64).to_le_bytes())
                }
                None => out.put(&[0]),
            }
        }
        Value::Opaque(bytes) => {
            out.tag(tag::OPAQUE)?;
            out.count(bytes.len())?;
            out.put(bytes)
        }
        Value::UndescribedPtr(addr) => {
            out.tag(tag::UNDESCRIBED_PTR)?;
            out.put(&addr.to_le_bytes())
        }
        Value::NotAnAddress(raw) => {
            out.tag(tag::NOT_AN_ADDRESS)?;
            out.put(&raw.to_le_bytes())
        }
    }
}

fn write_array_head(out: &mut Out, len: usize, capacity: Option<usize>) -> Result<(), CodecError> {
    out.tag(tag::ARRAY)?;
    out.count(len)?;
    // A flag, never a `u64::MAX` sentinel, marks an absent capacity.
    match capacity {
        Some(capacity) => {
            out.put(&[1])?;
            out.put(&(capacity as u64).to_le_bytes())
        }
        None => out.put(&[0]),
    }
}

/// A run of records of one [packable](or_io::desc::is_packable) type as
/// the reader would have held it; anything else as it came.
fn packed_if_packable(pack: Option<&Pack>, elements: Vec<Value>, capacity: Option<usize>) -> Value {
    let packable = pack.zip(elements.first()).and_then(|(pack, first)| {
        let first = first.record()?;
        let plans = pack.plans();
        let plan = plans.get(first.type_name()).ok()?;
        let same = elements.iter().all(|e| {
            e.record()
                .is_some_and(|r| r.type_name() == first.type_name() && r.children().is_empty())
        });
        (same && or_io::desc::is_packable(plan)).then(|| RecordRun::new(plans, plan.name()))
    });
    match packable {
        Some(Ok(mut run)) => {
            for element in &elements {
                if run.push(element.record().expect("checked above")).is_err() {
                    return Value::Array { elements, capacity };
                }
            }
            run.set_capacity(capacity);
            Value::Records(run)
        }
        _ => Value::Array { elements, capacity },
    }
}

struct In<'b> {
    bytes: &'b [u8],
    at: usize,
}

impl<'b> In<'b> {
    fn take(&mut self, n: usize) -> Result<&'b [u8], CodecError> {
        let end = self.at.checked_add(n).ok_or(CodecError::Truncated {
            wanted: n,
            had: self.bytes.len() - self.at,
        })?;
        if end > self.bytes.len() {
            return Err(CodecError::Truncated {
                wanted: n,
                had: self.bytes.len() - self.at,
            });
        }
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four")))
    }

    fn u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("eight")))
    }

    fn str(&mut self) -> Result<String, CodecError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| CodecError::NotUtf8)
    }

    /// A declared count, capped by the bytes that are actually left.
    ///
    /// ⚠ **Never allocate from a declared count.** A guest that says four
    /// billion elements and sends nine bytes would otherwise cost the host four
    /// billion `Value`s' worth of capacity before the first `take` failed.
    fn reserve_for(&self, count: u32) -> usize {
        (count as usize).min(self.left())
    }
}

fn read_value(input: &mut In<'_>, depth: u32, pack: Option<&Pack>) -> Result<Value, CodecError> {
    if depth > MAX_DEPTH {
        return Err(CodecError::TooDeep);
    }
    let tag = input.u8()?;
    Ok(match tag {
        tag::INT => Value::Scalar(ScalarValue::Int(i128::from_le_bytes(
            input.take(16)?.try_into().expect("sixteen"),
        ))),
        tag::F32 => Value::Scalar(ScalarValue::F32(f32::from_le_bytes(
            input.take(4)?.try_into().expect("four"),
        ))),
        tag::F64 => Value::Scalar(ScalarValue::F64(f64::from_le_bytes(
            input.take(8)?.try_into().expect("eight"),
        ))),
        tag::BOOL => match input.u8()? {
            0 => Value::Scalar(ScalarValue::Bool(false)),
            1 => Value::Scalar(ScalarValue::Bool(true)),
            other => return Err(CodecError::NotABool(other)),
        },
        tag::ENUM => {
            let enum_name = input.str()?;
            let value = i64::from_le_bytes(input.take(8)?.try_into().expect("eight"));
            Value::Enum { enum_name, value }
        }
        tag::STRUCT => {
            let type_name = input.str()?;
            match pack {
                Some(pack) => read_record(input, depth, pack, type_name)?,
                None => {
                    // ⚠ Read through before refusing, so bytes that are not
                    // this encoding are still told so rather than told to
                    // bring a pack.
                    let count = input.u32()?;
                    for _ in 0..count {
                        input.str()?;
                        read_value(input, depth + 1, None)?;
                    }
                    return Err(CodecError::NeedsPack { type_name });
                }
            }
        }
        tag::BLOB => {
            let codec = input.str()?;
            let count = input.u32()?;
            let mut parts = Vec::with_capacity(input.reserve_for(count));
            for _ in 0..count {
                let name = input.str()?;
                parts.push((name, read_value(input, depth + 1, pack)?));
            }
            Value::Blob { codec, parts }
        }
        tag::ARRAY => {
            let count = input.u32()?;
            let capacity = match input.u8()? {
                0 => None,
                _ => Some(input.u64()? as usize),
            };
            let mut elements = Vec::with_capacity(input.reserve_for(count));
            for _ in 0..count {
                elements.push(read_value(input, depth + 1, pack)?);
            }
            packed_if_packable(pack, elements, capacity)
        }
        tag::BYTES => {
            let len = input.u32()? as usize;
            Value::Bytes(input.take(len)?.to_vec())
        }
        tag::PTR => {
            let source = input.u64()?;
            let target = match input.u8()? {
                0 => None,
                _ => Some(Box::new(read_value(input, depth + 1, pack)?)),
            };
            Value::Ptr { source, target }
        }
        tag::ALIAS => Value::Alias(input.u64()?),
        tag::MEMBER => {
            let field = input.str()?;
            let index = match input.u8()? {
                0 => None,
                _ => Some(input.u64()? as usize),
            };
            Value::Member { field, index }
        }
        tag::OPAQUE => {
            let len = input.u32()? as usize;
            Value::Opaque(input.take(len)?.to_vec())
        }
        tag::UNDESCRIBED_PTR => Value::UndescribedPtr(input.u64()?),
        tag::NOT_AN_ADDRESS => Value::NotAnAddress(input.u64()?),
        other => return Err(CodecError::UnknownTag(other)),
    })
}

fn read_record(
    input: &mut In<'_>,
    depth: u32,
    pack: &Pack,
    type_name: String,
) -> Result<Value, CodecError> {
    let plans = pack.plans().clone();
    let (Ok(plan), Ok(mut record)) = (plans.get(&type_name), Record::zeroed(&plans, &type_name))
    else {
        return Err(CodecError::UnknownType { type_name });
    };
    fill(input, depth, pack, &mut record, plan.pack_type(), 0, "")?;
    Ok(Value::Struct(record))
}

fn wrong_shape(ty: &PackType, field: &str, expected: impl Into<String>) -> CodecError {
    CodecError::WrongShape {
        type_name: ty.name.clone(),
        field: field.to_string(),
        expected: expected.into(),
    }
}

fn put(record: &mut Record, kind: Kind, value: &ScalarValue, at: usize) -> Option<()> {
    let out = record.bytes_mut().get_mut(at..at + kind.size())?;
    kind.encode(value, out).ok()
}

/// The next value must be a `tag::STRUCT` naming `type_name`.
fn open_embedded(input: &mut In<'_>, type_name: &str) -> Result<bool, CodecError> {
    Ok(input.u8()? == tag::STRUCT && input.str()? == type_name)
}

/// One struct's `(name, value)` pairs, written where `ty` declares them in
/// the region of `record` starting at `base`.
fn fill(
    input: &mut In<'_>,
    depth: u32,
    pack: &Pack,
    record: &mut Record,
    ty: &PackType,
    base: usize,
    prefix: &str,
) -> Result<(), CodecError> {
    if depth > MAX_DEPTH {
        return Err(CodecError::TooDeep);
    }
    let plans = pack.plans().clone();
    let count = input.u32()?;
    let mut seen = vec![false; ty.fields.len()];
    for _ in 0..count {
        let name = input.str()?;
        let Some(index) = ty.fields.iter().position(|f| f.name == name) else {
            return Err(CodecError::UnknownField {
                type_name: ty.name.clone(),
                field: name,
            });
        };
        if std::mem::replace(&mut seen[index], true) {
            return Err(CodecError::RepeatedField {
                type_name: ty.name.clone(),
                field: name,
            });
        }
        let field = &ty.fields[index];
        let at = base + field.offset;
        let path = format!("{prefix}{name}");
        match &field.repr {
            FieldRepr::Scalar(scalar) => {
                let kind = Kind::of_scalar(*scalar);
                let expected = || wrong_shape(ty, &name, format!("a scalar that fits {kind:?}"));
                let Value::Scalar(value) = read_value(input, depth + 1, Some(pack))? else {
                    return Err(expected());
                };
                put(record, kind, &value, at).ok_or_else(expected)?;
            }
            FieldRepr::Enum { type_name, width } => {
                let expected = || wrong_shape(ty, &name, format!("a `{type_name}` value"));
                match read_value(input, depth + 1, Some(pack))? {
                    Value::Enum { enum_name, value } if enum_name == *type_name => put(
                        record,
                        Kind::Enum(*width),
                        &ScalarValue::Int(i128::from(value)),
                        at,
                    )
                    .ok_or_else(expected)?,
                    _ => return Err(expected()),
                }
            }
            FieldRepr::ScalarArray { element, count } => {
                let kind = Kind::of_scalar(*element);
                let expected =
                    || wrong_shape(ty, &name, format!("{count} scalars that fit {kind:?}"));
                let Value::Array {
                    elements,
                    capacity: None,
                } = read_value(input, depth + 1, Some(pack))?
                else {
                    return Err(expected());
                };
                if elements.len() != *count {
                    return Err(expected());
                }
                for (i, element) in elements.iter().enumerate() {
                    let Value::Scalar(value) = element else {
                        return Err(expected());
                    };
                    put(record, kind, value, at + i * kind.size()).ok_or_else(expected)?;
                }
            }
            FieldRepr::Padding { size } | FieldRepr::Opaque { size } => {
                let expected = || wrong_shape(ty, &name, format!("{size} opaque bytes"));
                let Value::Opaque(bytes) = read_value(input, depth + 1, Some(pack))? else {
                    return Err(expected());
                };
                let out = record
                    .bytes_mut()
                    .get_mut(at..at + size)
                    .filter(|out| out.len() == bytes.len())
                    .ok_or_else(expected)?;
                out.copy_from_slice(&bytes);
            }
            FieldRepr::Inline { type_name } => {
                let expected = || wrong_shape(ty, &name, format!("an embedded `{type_name}`"));
                let inner = plans.get(type_name).map_err(|_| expected())?;
                if !open_embedded(input, type_name)? {
                    return Err(expected());
                }
                let prefix = format!("{path}.");
                fill(
                    input,
                    depth + 1,
                    pack,
                    record,
                    inner.pack_type(),
                    at,
                    &prefix,
                )?;
            }
            FieldRepr::InlineArray { type_name, count } => {
                let expected = || {
                    wrong_shape(
                        ty,
                        &name,
                        format!("a run of {count} embedded `{type_name}`"),
                    )
                };
                let inner = plans.get(type_name).map_err(|_| expected())?;
                if input.u8()? != tag::ARRAY || input.u32()? as usize != *count || input.u8()? != 0
                {
                    return Err(expected());
                }
                for i in 0..*count {
                    if !open_embedded(input, type_name)? {
                        return Err(expected());
                    }
                    let prefix = format!("{path}[{i}].");
                    let at = at + i * inner.size();
                    fill(
                        input,
                        depth + 1,
                        pack,
                        record,
                        inner.pack_type(),
                        at,
                        &prefix,
                    )?;
                }
            }
            FieldRepr::Ptr(_)
            | FieldRepr::AtArray(_)
            | FieldRepr::PtrSlots { .. }
            | FieldRepr::Trailing { .. } => {
                let child = read_value(input, depth + 1, Some(pack))?;
                record
                    .set_child(&path, child)
                    .map_err(|e| wrong_shape(ty, &name, format!("a child ({e})")))?;
            }
        }
    }
    if let Some(missing) = seen.iter().position(|seen| !seen) {
        return Err(CodecError::MissingField {
            type_name: ty.name.clone(),
            field: ty.fields[missing].name.clone(),
        });
    }
    Ok(())
}
