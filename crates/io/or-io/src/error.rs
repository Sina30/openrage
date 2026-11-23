use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorMode {
    #[default]
    Lenient,

    Strict,
}

/// Every way reading, viewing, opening or saving a file fails.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Failed to read file '{path}': {source}")]
    FileReadFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid RSC7 magic: expected 0x37435352, found {found:#X}")]
    InvalidMagic { found: u32 },

    #[error("Unsupported {resource_type} version: found {found}, expected {expected}")]
    UnsupportedVersionDetailed {
        found: u32,
        resource_type: String,
        expected: String,
    },

    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),

    #[error("Pointer out of bounds: offset {offset:#X} exceeds data size {size}")]
    PointerOutOfBounds { offset: u64, size: usize },

    // Names neither base: the two come from the paging model the reader is
    // given, so a message quoting them would be wrong under another model.
    #[error("Invalid pointer base {base:#X}: the address is in neither the system nor the graphics space")]
    InvalidPointerBase { base: u8 },

    #[error("Invalid structure at offset {offset:#X}: {message}")]
    InvalidStructure { offset: u64, message: String },

    #[error("Unknown behaviour type: {type_id}")]
    UnknownBehaviour { type_id: u32 },

    #[error("String table index out of bounds: {index} >= {count}")]
    StringIndexOutOfBounds { index: u32, count: u32 },

    #[error("Unexpected end of data at offset {offset:#X}: needed {needed} bytes, only {available} available")]
    UnexpectedEof {
        offset: u64,
        needed: usize,
        available: usize,
    },

    #[error("Invalid domain type: {type_id}")]
    InvalidDomainType { type_id: u32 },

    #[error("Data too small: expected at least {expected} bytes, got {actual}")]
    DataTooSmall { expected: usize, actual: usize },

    #[error("Serialization failed at offset {offset:#X}: {message}")]
    SerializationFailed { offset: u64, message: String },

    #[error("Compression failed: {0}")]
    CompressionFailed(String),

    #[error("String table overflow: {count} strings exceeds maximum")]
    StringTableOverflow { count: usize },

    #[error("Data too large: {size} bytes exceeds maximum {max} bytes")]
    DataTooLarge { size: usize, max: usize },

    #[error("Insufficient data: expected {expected} bytes, got {actual}")]
    InsufficientData { expected: usize, actual: usize },

    #[error("Invalid data: {context}")]
    InvalidData { context: String },

    #[error("Unsupported behaviour type for serialization: {type_name}")]
    UnsupportedBehaviourWrite { type_name: String },

    #[error("Failed to write file '{path}': {source}")]
    FileWriteFailed {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Null pointer encountered")]
    NullPointer,

    #[error(
        "Not an archive OpenRage reads: its magic {found:#010X} is no RPF or IMG generation's"
    )]
    RpfBadMagic { found: u32 },

    #[error("This is an RPF9 archive (GTA VI): its table of contents is encrypted and OpenRage has no key for it, so its entries cannot be listed")]
    Rpf9TocEncrypted,

    #[error("Invalid RPF header: {message}")]
    RpfBadHeader { message: String },

    #[error("Invalid RPF entry: {message}")]
    RpfBadEntry { message: String },

    #[error("RPF encryption {key_id:#X} has no key in this provider (GTA V's keys come from your executable, in the game's settings page or with `--keys`; RDR2's come from a snapshot of its cipher tables, which can predate a key a newer patch added)")]
    RpfEncryptionUnsupported { key_id: u32 },

    #[error("Encrypted RPF requires GTA V keys (mode {mode}); none have been extracted yet")]
    RpfKeysMissing { mode: &'static str },

    #[error("RPF crypto operation failed ({mode}): {message}")]
    RpfCryptoFailed { mode: &'static str, message: String },

    #[error("GTA V key extraction failed: {0}")]
    KeyExtractionFailed(String),

    #[error("GTA V keys cache is invalid: {0}")]
    KeysCacheInvalid(String),

    #[error("RPF name heap overflow: needed {size} bytes, max is {max}")]
    RpfNameHeapOverflow { size: usize, max: usize },

    #[error("`{ty}` at {offset:#x} needs {len:#x} bytes and the image holds {size:#x}")]
    OutOfBounds {
        ty: String,
        offset: usize,
        len: usize,
        size: usize,
    },

    #[error("`{ty}` at {offset:#x} is not {align}-byte aligned; declare it packed if the corpus has it here")]
    Misaligned {
        ty: String,
        offset: usize,
        align: usize,
    },

    #[error("`{ty}.{field}` holds {address:#x}, which is not in this image: {reason}")]
    BadPointer {
        ty: Box<str>,
        field: &'static str,
        address: u64,
        reason: Box<str>,
    },

    #[error("`{ty}.{field}` holds {address:#x}, which is not the start of a `{member}` inside its own record")]
    NotAMember {
        ty: Box<str>,
        field: &'static str,
        member: Box<str>,
        address: u64,
    },

    #[error("a `{found}` record is not a `{expected}`")]
    WrongType { expected: String, found: String },

    #[error("{0}")]
    Tree(String),

    #[error("{what} at {at:#x}: expected {expected}, found {found}")]
    Shape {
        what: Box<str>,
        at: usize,
        expected: &'static str,
        found: &'static str,
    },

    #[error("{what}: {why}")]
    Invalid { what: String, why: String },

    #[error(".{ext} is not declared for {edition}")]
    NoTarget { ext: &'static str, edition: String },

    #[error("a .{ext} at RSC7 version {version} is not a layout OpenRage declares")]
    Version { ext: &'static str, version: u32 },

    #[error("the built-in description of .{ext} is inconsistent: {problems}")]
    Description { ext: &'static str, problems: String },

    #[error("reading a .{ext}: {why}")]
    Read { ext: &'static str, why: String },

    #[error("saving a .{ext}: {why}")]
    Save { ext: &'static str, why: String },

    /// The format's validation refused the save; `reason` is its own error type.
    #[error("a .{ext} the game would misread is not saved: {reason}")]
    Refused {
        ext: &'static str,
        #[source]
        reason: Reason,
    },

    #[error("this is {found}, not {wanted}")]
    WrongKind { found: String, wanted: String },

    #[error("this .{ext} holds the {found} layout, which `{type_name}` is not; open it as the {found} type, or with `or_io::open`")]
    WrongTarget {
        ext: &'static str,
        type_name: Box<str>,
        found: &'static str,
    },

    #[error("not a resource: {0}")]
    NotAResource(String),

    #[error("this value was not opened from a file; use `save_as`")]
    NoPath,

    #[error("{kind} at the {edition} layout has no owned form yet")]
    NotOwned {
        ext: &'static str,
        kind: String,
        edition: &'static str,
    },

    #[error("RSC7 version {version} is {formats}; open it by a path whose extension says which")]
    Ambiguous { version: u32, formats: String },

    #[error("RSC7 version {version} is not a layout OpenRage declares")]
    UnknownVersion { version: u32 },
}

/// A format's own error for a save it refused, kept whole so a caller can downcast it.
#[derive(Clone)]
pub struct Reason(Arc<dyn std::error::Error + Send + Sync>);

impl Reason {
    pub fn new(reason: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Arc::new(reason))
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for Reason {
    fn from(reason: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self(Arc::from(reason))
    }
}

// ⚠ Not `std::error::Error`: thiserror's `#[source]` then reaches through the
// deref to the format's error, which a downcast finds. An `Arc` that is itself
// the source would hide it.
impl std::ops::Deref for Reason {
    type Target = dyn std::error::Error + Send + Sync;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl std::fmt::Debug for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Two errors are equal when they are the same variant and say the same thing.
impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
            && self.to_string() == other.to_string()
    }
}

impl Eq for Error {}

fn dup(e: &std::io::Error) -> std::io::Error {
    std::io::Error::new(e.kind(), e.to_string())
}

/// ⚠ An `std::io::Error` is cloned as its kind and message.
impl Clone for Error {
    fn clone(&self) -> Self {
        use Error::*;
        match self {
            FileNotFound(p) => FileNotFound(p.clone()),
            FileReadFailed { path, source } => FileReadFailed {
                path: path.clone(),
                source: dup(source),
            },
            Io(e) => Io(dup(e)),
            InvalidMagic { found } => InvalidMagic { found: *found },
            UnsupportedVersionDetailed {
                found,
                resource_type,
                expected,
            } => UnsupportedVersionDetailed {
                found: *found,
                resource_type: resource_type.clone(),
                expected: expected.clone(),
            },
            DecompressionFailed(s) => DecompressionFailed(s.clone()),
            PointerOutOfBounds { offset, size } => PointerOutOfBounds {
                offset: *offset,
                size: *size,
            },
            InvalidPointerBase { base } => InvalidPointerBase { base: *base },
            InvalidStructure { offset, message } => InvalidStructure {
                offset: *offset,
                message: message.clone(),
            },
            UnknownBehaviour { type_id } => UnknownBehaviour { type_id: *type_id },
            StringIndexOutOfBounds { index, count } => StringIndexOutOfBounds {
                index: *index,
                count: *count,
            },
            UnexpectedEof {
                offset,
                needed,
                available,
            } => UnexpectedEof {
                offset: *offset,
                needed: *needed,
                available: *available,
            },
            InvalidDomainType { type_id } => InvalidDomainType { type_id: *type_id },
            DataTooSmall { expected, actual } => DataTooSmall {
                expected: *expected,
                actual: *actual,
            },
            SerializationFailed { offset, message } => SerializationFailed {
                offset: *offset,
                message: message.clone(),
            },
            CompressionFailed(s) => CompressionFailed(s.clone()),
            StringTableOverflow { count } => StringTableOverflow { count: *count },
            DataTooLarge { size, max } => DataTooLarge {
                size: *size,
                max: *max,
            },
            InsufficientData { expected, actual } => InsufficientData {
                expected: *expected,
                actual: *actual,
            },
            InvalidData { context } => InvalidData {
                context: context.clone(),
            },
            UnsupportedBehaviourWrite { type_name } => UnsupportedBehaviourWrite {
                type_name: type_name.clone(),
            },
            FileWriteFailed { path, source } => FileWriteFailed {
                path: path.clone(),
                source: dup(source),
            },
            NullPointer => NullPointer,
            RpfBadMagic { found } => RpfBadMagic { found: *found },
            Rpf9TocEncrypted => Rpf9TocEncrypted,
            RpfBadHeader { message } => RpfBadHeader {
                message: message.clone(),
            },
            RpfBadEntry { message } => RpfBadEntry {
                message: message.clone(),
            },
            RpfEncryptionUnsupported { key_id } => RpfEncryptionUnsupported { key_id: *key_id },
            RpfKeysMissing { mode } => RpfKeysMissing { mode },
            RpfCryptoFailed { mode, message } => RpfCryptoFailed {
                mode,
                message: message.clone(),
            },
            KeyExtractionFailed(s) => KeyExtractionFailed(s.clone()),
            KeysCacheInvalid(s) => KeysCacheInvalid(s.clone()),
            RpfNameHeapOverflow { size, max } => RpfNameHeapOverflow {
                size: *size,
                max: *max,
            },
            OutOfBounds {
                ty,
                offset,
                len,
                size,
            } => OutOfBounds {
                ty: ty.clone(),
                offset: *offset,
                len: *len,
                size: *size,
            },
            Misaligned { ty, offset, align } => Misaligned {
                ty: ty.clone(),
                offset: *offset,
                align: *align,
            },
            BadPointer {
                ty,
                field,
                address,
                reason,
            } => BadPointer {
                ty: ty.clone(),
                field,
                address: *address,
                reason: reason.clone(),
            },
            NotAMember {
                ty,
                field,
                member,
                address,
            } => NotAMember {
                ty: ty.clone(),
                field,
                member: member.clone(),
                address: *address,
            },
            WrongType { expected, found } => WrongType {
                expected: expected.clone(),
                found: found.clone(),
            },
            Tree(s) => Tree(s.clone()),
            Shape {
                what,
                at,
                expected,
                found,
            } => Shape {
                what: what.clone(),
                at: *at,
                expected,
                found,
            },
            Invalid { what, why } => Invalid {
                what: what.clone(),
                why: why.clone(),
            },
            NoTarget { ext, edition } => NoTarget {
                ext,
                edition: edition.clone(),
            },
            Version { ext, version } => Version {
                ext,
                version: *version,
            },
            Description { ext, problems } => Description {
                ext,
                problems: problems.clone(),
            },
            Read { ext, why } => Read {
                ext,
                why: why.clone(),
            },
            Save { ext, why } => Save {
                ext,
                why: why.clone(),
            },
            Refused { ext, reason } => Refused {
                ext,
                reason: reason.clone(),
            },
            WrongKind { found, wanted } => WrongKind {
                found: found.clone(),
                wanted: wanted.clone(),
            },
            WrongTarget {
                ext,
                type_name,
                found,
            } => WrongTarget {
                ext,
                type_name: type_name.clone(),
                found,
            },
            NotAResource(s) => NotAResource(s.clone()),
            NoPath => NoPath,
            NotOwned { ext, kind, edition } => NotOwned {
                ext,
                kind: kind.clone(),
                edition,
            },
            Ambiguous { version, formats } => Ambiguous {
                version: *version,
                formats: formats.clone(),
            },
            UnknownVersion { version } => UnknownVersion { version: *version },
        }
    }
}

/// The container and pack path's name for [`Error`].
pub type IoError = Error;

/// The owned form's name for [`Error`].
pub type OwnedError = Error;

/// A view's name for [`Error`].
pub type ViewError = Error;

pub type Result<T> = std::result::Result<T, Error>;
