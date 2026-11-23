//! What a plugin can see and change: plain functions over a [`Pack`], the
//! document's bytes and a type name. [`crate::runtime`] marshals; this decides.
//!
//! ⚠ The host walks one hop and never builds a tree. A whole-document read
//! allocates in proportion to the file, in the *host's* memory, at a guest's
//! request, so the guest drives the walk — [`field_value`], [`follow`],
//! [`element_count`], [`type_size`] — and no call allocates more than one
//! field's worth.
//!
//! ⚠ A read answers with a typed `or_io::desc::Value`, never `or_editor`'s
//! formatted text, which cannot be written back (`"1.500"` does not say `f32`
//! or fixed-point `u16`). The one write is [`patch_field`].

use or_io::desc::value::ScalarValue;
use or_io::desc::{
    CountSource, FieldRepr, Image, ImageMut, IntWidth, Pack, PackType, Pointee, Scalar, Value,
};
use or_io::write::{patch, Edit, PatchError};

use crate::abi::Status;

/// A described field, and where its owner sits in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site<'a> {
    pub type_name: &'a str,
    pub at: usize,
    pub field: &'a str,
}

impl<'a> Site<'a> {
    pub fn new(type_name: &'a str, at: usize, field: &'a str) -> Self {
        Self {
            type_name,
            at,
            field,
        }
    }
}

/// A refusal, as the guest's code and the user's sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub status: Status,
    pub because: String,
}

impl Denied {
    fn new(status: Status, because: impl Into<String>) -> Self {
        Self {
            status,
            because: because.into(),
        }
    }
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.because)
    }
}

type Answer<T> = Result<T, Denied>;

/// Whether a patched document, or a file a guest hands back, would still be a file the game can load.
///
/// ⚠ `or_io::write::patch` refuses the structural fields a description knows —
/// a count, a capacity, a tagged base's selector — but not what only a
/// format's own save check knows, where **the reader tolerates
/// something the engine does not**: a texture slot list read as a prefix with
/// a separately stated count; a binary-searched table that must stay sorted;
/// an index into a companion table shorter than the list it indexes; a
/// scratch-buffer size the runtime allocates from; an array whose allocation
/// was short while its count was right; paired arrays sharing one subscript.
/// A file that breaks one reads back identically here and the game misreads it.
///
/// ⚠ A write routes through this before it is kept, and it is a parameter
/// rather than a default so an embedder cannot get a writing host by
/// forgetting. That check belongs to the format and the embedder supplies it;
/// the host names no format, so it cannot implement one.
pub trait SaveCheck {
    /// `None` to keep the patch, or why the document would be invalid.
    fn why_the_document_would_be_invalid(&self, document: &[u8]) -> Option<String>;

    /// `None` to keep the bytes a guest wrote as the open file, or why they are not one the format and the document accept.
    fn why_the_file_would_be_invalid(&self, file: &[u8]) -> Option<String>;

    /// `None` to keep the files a guest wrote together, each by its path in the project, or why they are not a set the embedder keeps.
    ///
    /// ⚠ The default refuses, unlike the two above, which an embedder must write: a host that has not
    /// said how it checks files written together keeps none of them.
    fn why_the_files_would_be_invalid(&self, _files: &[(&str, &[u8])]) -> Option<String> {
        Some("this host does not accept files written together".to_string())
    }
}

/// A check that accepts any document.
///
/// ⚠ For tests and tools with no format knowledge, **never for the
/// application**, which would be throwing the format's save check away;
/// `nothing_in_the_tree_accepts_any_document` is the gate.
#[derive(Debug, Default, Clone, Copy)]
pub struct AcceptsAnything;

impl SaveCheck for AcceptsAnything {
    fn why_the_document_would_be_invalid(&self, _document: &[u8]) -> Option<String> {
        None
    }

    fn why_the_file_would_be_invalid(&self, _file: &[u8]) -> Option<String> {
        None
    }

    fn why_the_files_would_be_invalid(&self, _files: &[(&str, &[u8])]) -> Option<String> {
        None
    }
}

/// A check that refuses, carrying the reason.
#[derive(Debug, Clone)]
pub struct WouldBeInvalid(pub String);

impl SaveCheck for WouldBeInvalid {
    fn why_the_document_would_be_invalid(&self, _document: &[u8]) -> Option<String> {
        Some(self.0.clone())
    }

    fn why_the_file_would_be_invalid(&self, _file: &[u8]) -> Option<String> {
        Some(self.0.clone())
    }

    fn why_the_files_would_be_invalid(&self, _files: &[(&str, &[u8])]) -> Option<String> {
        Some(self.0.clone())
    }
}

fn described<'p>(pack: &'p Pack, type_name: &str) -> Answer<&'p PackType> {
    pack.type_by_name(type_name).ok_or_else(|| {
        Denied::new(
            Status::NoSuchType,
            format!("the installed pack describes no type named `{type_name}`"),
        )
    })
}

fn described_field<'p>(ty: &'p PackType, field: &str) -> Answer<&'p or_io::desc::PackField> {
    ty.field_by_name(field).ok_or_else(|| {
        Denied::new(
            Status::NoSuchField,
            format!("`{}` has no field named `{field}`", ty.name),
        )
    })
}

fn truncated(what: &str) -> Denied {
    Denied::new(
        Status::Truncated,
        format!("the pack places {what} past the end of the document, so the pack and the file disagree"),
    )
}

/// How many fields the pack describes for this type.
pub fn field_count(pack: &Pack, type_name: &str) -> Answer<usize> {
    Ok(described(pack, type_name)?.fields.len())
}

/// The name of the type's *index*-th field.
pub fn field_name<'p>(pack: &'p Pack, type_name: &str, index: usize) -> Answer<&'p str> {
    let ty = described(pack, type_name)?;
    ty.fields
        .get(index)
        .map(|f| f.name.as_str())
        .ok_or_else(|| {
            Denied::new(
                Status::NoSuchField,
                format!(
                    "`{}` has {} fields and no field {index}",
                    ty.name,
                    ty.fields.len()
                ),
            )
        })
}

/// How many bytes one of these occupies, which is a run's stride.
pub fn type_size(pack: &Pack, type_name: &str) -> Answer<usize> {
    Ok(described(pack, type_name)?.size)
}

/// One described scalar, enum or fixed scalar run, read out of the document.
///
/// Anything indirect is a hop the guest takes with [`follow`], because its
/// target may be megabytes.
pub fn field_value(pack: &Pack, document: Image<'_>, site: Site<'_>) -> Answer<Value> {
    let Site {
        type_name,
        at,
        field: field_name,
    } = site;
    let ty = described(pack, type_name)?;
    let field = described_field(ty, field_name)?;
    if field.is_bit_packed() {
        return Err(Denied::new(
            Status::Refused,
            format!(
                "`{type_name}::{field_name}` shares its word with other values, so it has no \
                 whole-field value to read"
            ),
        ));
    }
    let bytes = document.bytes();
    let base = at
        .checked_add(field.offset)
        .ok_or_else(|| truncated(&format!("`{type_name}::{field_name}`")))?;

    match &field.repr {
        FieldRepr::Scalar(scalar) => scalar_at(*scalar, bytes, base)
            .ok_or_else(|| truncated(&format!("`{type_name}::{field_name}`"))),
        FieldRepr::Enum {
            type_name: enum_name,
            width,
        } => {
            let raw = width
                .read(bytes, base)
                .ok_or_else(|| truncated(&format!("`{type_name}::{field_name}`")))?;
            Ok(Value::Enum {
                enum_name: enum_name.clone(),
                value: i64::try_from(raw).map_err(|_| {
                    Denied::new(
                        Status::Refused,
                        format!(
                            "`{type_name}::{field_name}` holds {raw}, which is not an enum value"
                        ),
                    )
                })?,
            })
        }
        FieldRepr::ScalarArray { element, count } => {
            let mut elements = Vec::with_capacity(*count);
            for index in 0..*count {
                let at = base + index * element.size();
                elements.push(
                    scalar_at(*element, bytes, at)
                        .ok_or_else(|| truncated(&format!("`{type_name}::{field_name}`")))?,
                );
            }
            Ok(Value::Array {
                elements,
                capacity: None,
            })
        }
        other => Err(Denied::new(
            Status::Refused,
            format!(
                "`{type_name}::{field_name}` is {} rather than a value; follow it instead",
                shape_of(other)
            ),
        )),
    }
}

/// Where a pointer field lands, as an offset into the document.
pub fn follow(pack: &Pack, document: Image<'_>, site: Site<'_>) -> Answer<usize> {
    let Site {
        type_name,
        at,
        field: field_name,
    } = site;
    let ty = described(pack, type_name)?;
    let field = described_field(ty, field_name)?;
    let base = at
        .checked_add(field.offset)
        .ok_or_else(|| truncated(&format!("`{type_name}::{field_name}`")))?;

    let address = match &field.repr {
        FieldRepr::Ptr(_) => document
            .pointer_at(base)
            .map_err(|_| truncated(&format!("`{type_name}::{field_name}`")))?,
        // ⚠ An `atArray`'s pointer is at the head of its own header, so the hop
        // is the same hop. Its *count* is in the header and is `element_count`'s.
        FieldRepr::AtArray(_) => document
            .pointer_at(base)
            .map_err(|_| truncated(&format!("`{type_name}::{field_name}`")))?,
        other => {
            return Err(Denied::new(
                Status::Refused,
                format!(
                    "`{type_name}::{field_name}` is {} and there is nothing to follow",
                    shape_of(other)
                ),
            ))
        }
    };

    if address == 0 {
        return Err(Denied::new(
            Status::NullPointer,
            format!("`{type_name}::{field_name}` is null in this document"),
        ));
    }
    document.resolve(address).map_err(|_| {
        Denied::new(
            Status::Refused,
            format!("`{type_name}::{field_name}` points at {address:#x}, which this document does not contain"),
        )
    })
}

/// How many elements a described run holds in this document.
pub fn element_count(pack: &Pack, document: Image<'_>, site: Site<'_>) -> Answer<usize> {
    let Site {
        type_name,
        at,
        field: field_name,
    } = site;
    let ty = described(pack, type_name)?;
    let field = described_field(ty, field_name)?;
    let bytes = document.bytes();
    let base = at
        .checked_add(field.offset)
        .ok_or_else(|| truncated(&format!("`{type_name}::{field_name}`")))?;

    match &field.repr {
        FieldRepr::ScalarArray { count, .. }
        | FieldRepr::InlineArray { count, .. }
        | FieldRepr::PtrSlots { count, .. } => Ok(*count),

        // ⚠ The count is in the array's own header, not in a sibling field.
        FieldRepr::AtArray(_) => document
            .at_array(base)
            .map_err(|_| truncated(&format!("`{type_name}::{field_name}`")))?
            .map(|elements| elements.count())
            .ok_or_else(|| {
                Denied::new(
                    Status::NullPointer,
                    format!("`{type_name}::{field_name}` is null in this document"),
                )
            }),

        FieldRepr::Ptr(Pointee::Many { count, .. })
        | FieldRepr::Ptr(Pointee::ManyPtr { count, .. })
        | FieldRepr::Ptr(Pointee::Bytes { length: count }) => {
            count_from_siblings(ty, count, bytes, at, type_name, field_name)
        }

        other => Err(Denied::new(
            Status::Refused,
            format!(
                "`{type_name}::{field_name}` is {} and is not a run",
                shape_of(other)
            ),
        )),
    }
}

/// Resolve a [`CountSource`] by reading the sibling fields it names.
fn count_from_siblings(
    ty: &PackType,
    count: &CountSource,
    bytes: &[u8],
    at: usize,
    type_name: &str,
    field_name: &str,
) -> Answer<usize> {
    let mut raws = Vec::new();
    for sibling in count.reads_fields() {
        let sibling_field = described_field(ty, sibling)?;
        let width = match sibling_field.repr {
            FieldRepr::Scalar(Scalar::Int(width)) => width,
            FieldRepr::Enum { width, .. } => width,
            _ => {
                return Err(Denied::new(
                    Status::Refused,
                    format!(
                        "`{type_name}::{field_name}` takes its length from `{sibling}`, which is \
                         not an integer"
                    ),
                ))
            }
        };
        raws.push(
            read_int(width, bytes, at + sibling_field.offset)
                .ok_or_else(|| truncated(&format!("`{type_name}::{sibling}`")))?,
        );
    }
    count.length_from(&raws).ok_or_else(|| {
        Denied::new(
            Status::Refused,
            format!("the pack does not state how long `{type_name}::{field_name}` is"),
        )
    })
}

/// Change one described scalar in place, if `or_io::write::patch`'s structural
/// refusals and then `check` both allow it.
///
/// ⚠ `check` needs the patched document, so the patch happens first and is
/// undone byte-identically if `check` refuses, or a refused write would still
/// have changed the file; that is why this takes the bytes rather than an
/// `ImageMut`. `a_refused_patch_leaves_the_document_byte_identical` holds it.
pub fn patch_field(
    pack: &Pack,
    document: &mut [u8],
    virtual_size: usize,
    site: Site<'_>,
    value: ScalarValue,
    check: &dyn SaveCheck,
) -> Answer<()> {
    let Site {
        type_name,
        at,
        field: field_name,
    } = site;
    let ty = described(pack, type_name)?;
    described_field(ty, field_name)?;

    // ⚠ Bounded by the type, not by the file. A scalar's offset is inside its
    // own type, so this is tens of bytes however large the document is.
    let end = at.saturating_add(ty.size).min(document.len());
    let start = at.min(end);
    let before: Vec<u8> = document[start..end].to_vec();

    {
        let mut image = ImageMut::new(document, virtual_size);
        patch(
            pack,
            type_name,
            &mut image,
            at,
            &[Edit::new(field_name, value)],
        )
        .map_err(denied_by_pack)?;
    }

    if let Some(why) = check.why_the_document_would_be_invalid(document) {
        document[start..end].copy_from_slice(&before);
        return Err(Denied::new(
            Status::Refused,
            format!("`{type_name}::{field_name}` was not changed: {why}"),
        ));
    }
    Ok(())
}

/// The pack's own refusal, kept as the pack worded it.
fn denied_by_pack(error: PatchError) -> Denied {
    let status = match error {
        PatchError::NoType { .. } => Status::NoSuchType,
        PatchError::NoField { .. } => Status::NoSuchField,
        _ => Status::Refused,
    };
    Denied::new(status, error.to_string())
}

fn scalar_at(scalar: Scalar, bytes: &[u8], at: usize) -> Option<Value> {
    Some(match scalar {
        Scalar::Bool => Value::boolean(*bytes.get(at)? != 0),
        Scalar::F32 => Value::f32(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)),
        Scalar::F64 => Value::f64(f64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?)),
        Scalar::Int(width) => Value::int(width.read(bytes, at)?),
    })
}

fn read_int(width: IntWidth, bytes: &[u8], at: usize) -> Option<i128> {
    width.read(bytes, at)
}

/// What a field is, for a refusal a plugin author can act on.
fn shape_of(repr: &FieldRepr) -> &'static str {
    match repr {
        FieldRepr::Scalar(_) => "a scalar",
        FieldRepr::ScalarArray { .. } => "a fixed run of scalars",
        FieldRepr::Inline { .. } => "an embedded struct",
        FieldRepr::InlineArray { .. } => "a fixed run of embedded structs",
        FieldRepr::Ptr(_) => "a pointer",
        FieldRepr::AtArray(_) => "an atArray",
        FieldRepr::PtrSlots { .. } => "a run of pointers",
        FieldRepr::Enum { .. } => "an enum",
        FieldRepr::Padding { .. } => "padding",
        FieldRepr::Opaque { .. } => "a run nothing describes",
        FieldRepr::Trailing { .. } => "a run after the struct",
    }
}
