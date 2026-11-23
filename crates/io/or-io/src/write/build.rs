//! The canonical writer: a value tree laid out on a [`ResourceHeap`].
//!
//! A pointer's target is allocated before an embedded struct's children,
//! whatever order the two are declared in.

use std::fmt;

use crate::container::paging::{PgPointerFixup, ResourceHeap};
use crate::desc::value::ScalarValue;
use crate::desc::{
    CountSource, FieldRepr, Pack, PackField, PackType, Plans, Pointee, Record, RecordRef,
    RecordRun, Value, AT_ARRAY_SIZE, POINTER_SIZE,
};

const STRUCT_ALIGN: usize = 16;

/// What the engine builds a struct into, where that is more than the file stores of it.
///
/// ⚠ **A buffer is placed into more bytes than its record.** The game's placement constructor for a
/// vertex or index buffer links the object into a global list and creates its GPU resource, writing
/// fields past the 0x38 / 0x18 bytes the file keeps (`+0x20` and `+0x28` of an index buffer alone).
/// Retail files leave the room: 0x80 for a vertex buffer, 0x60 for an index buffer, in every ped
/// component. A heap that packs the next object right behind the record has it overwritten before
/// it is placed, and the game dies on the pointer the next constructor reads
/// (`Invalid fixup, address is neither virtual nor physical`).
fn engine_footprint(name: &str, size: usize) -> usize {
    match (name, size) {
        ("GrcVertexBuffer", 0x38) => 0x80,
        ("GrcIndexBuffer", 0x18) => 0x60,
        _ => size,
    }
}

const STRING_ALIGN: usize = 4;

const _: () = assert!(AT_ARRAY_SIZE == POINTER_SIZE + 8);

const PHYSICAL_BLOB_ALIGN: usize = 256;

const PHYSICAL_BLOB_PAD: usize = 128;

/// What an **empty** run reserves, so that its address is its own.
///
/// # Why anything at all
///
/// A field can hold a non-null pointer to a run of nothing, and RAGE files do:
/// `phVerletCloth::collision_inst` is an `atArray` every one of 58 retail
/// garments leaves at count 0 with a live pointer. Writing zero bytes returns
/// the allocator's current address and advances nothing, so the *next*
/// allocation begins there too and the rebuilt heap holds two pointers at one
/// place. RSC7's fixup walk has no visited set, which is why WS9 refuses an
/// aliased block on every save path — and an interpreter should not be the
/// thing producing one.
///
/// # Why one byte
///
/// The reservation's only job is to make the address distinct, and one byte is
/// the least that does it. The two larger candidates each assert something the
/// description does not say. A run's **stride** claims room for an element
/// where the count says there are none, and is unavailable anyway for a
/// `Pointee::Bytes` blob, which has no stride — so it could not be applied
/// consistently across the shapes that need it. The **heap's alignment** is the
/// allocator's default rather than a fact about this field, and it pads by
/// sixteen for a run that occupies nothing.
///
/// Both alternatives also cost more of exactly what is scarce: every byte
/// reserved shifts every later allocation, and byte-identity against the
/// compiled writer is the gate. Measured across all six `pack_builds_*` suites,
/// one byte moves nothing.
///
/// ⚠ **This is deliberately not what the compiled writer does.**
/// Its array writer returns `NULL_PTR` for an empty slice, so
/// `to_resource` turns a live pointer into a null one and the file stops saying
/// the allocation was ever there. That is the null-versus-empty distinction the
/// implementation guide is explicit about, and the drawable gate already names
/// the same defect at `CrSkeletonData::child_parent_indices`. Preserving it
/// here means the interpreter is the more faithful of the two, which is worth a
/// named divergence rather than a matching loss.
// place. RSC7's fixup walk has no visited set, which is why WS9 refuses an
const EMPTY_RUN_RESERVATION: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    NoType {
        type_name: String,
    },

    Mismatch {
        type_name: String,
        field: String,
        expected: &'static str,
        found: &'static str,
    },

    WrongLength {
        type_name: String,
        field: String,
        expected: usize,
        found: usize,
    },

    FieldMissing {
        type_name: String,
        field: String,
    },

    DoesNotFit {
        type_name: String,
        field: String,
        value: i128,
    },

    /// Counts are carried, never recomputed: `PhOptimizedBvh::num_nodes` is an
    /// allocated capacity, so recomputing it shrinks an allocation the game
    /// expects at its declared size.
    CountDisagrees {
        type_name: String,
        field: String,
        count_field: String,
        count_says: usize,
        run_holds: usize,
    },

    TagDisagrees {
        type_name: String,
        base: String,
        tag: i64,
        resolves_to: String,
    },

    DanglingAlias {
        type_name: String,
        field: String,
        address: u64,
    },

    BlobNotAPointer {
        type_name: String,
        field: String,
        repr: &'static str,
    },

    Blob {
        type_name: String,
        field: String,
        message: String,
    },

    /// A member pointer names a path its owner does not have, or one that is
    /// not an embedded struct of the type the field points at.
    NotAMember {
        type_name: String,
        field: String,
        owner: String,
        member: String,
    },

    /// A record carries the pack it was read with, and a Legacy record is
    /// not written under the Enhanced pack or the other way round.
    WrongPack {
        type_name: String,
        record: String,
        pack: String,
    },

    Builder(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoType { type_name } => {
                write!(f, "the pack describes no type named `{type_name}`")
            }
            Self::Mismatch {
                type_name,
                field,
                expected,
                found,
            } => write!(
                f,
                "`{type_name}::{field}` is described as {expected} and the value is {found}"
            ),
            Self::WrongLength {
                type_name,
                field,
                expected,
                found,
            } => write!(
                f,
                "`{type_name}::{field}` holds {expected} element(s) in the layout and \
                 {found} in the value"
            ),
            Self::FieldMissing { type_name, field } => write!(
                f,
                "the value has no `{field}`, which `{type_name}` declares"
            ),
            Self::DoesNotFit {
                type_name,
                field,
                value,
            } => write!(
                f,
                "{value} does not fit the width `{type_name}::{field}` is declared at"
            ),
            Self::CountDisagrees {
                type_name,
                field,
                count_field,
                count_says,
                run_holds,
            } => write!(
                f,
                "`{type_name}::{field}` holds {run_holds} element(s) and \
                 `{type_name}::{count_field}`, which is what the layout reads its \
                 length by, says {count_says}. A run and its count are edited together"
            ),
            Self::TagDisagrees {
                type_name,
                base,
                tag,
                resolves_to,
            } => write!(
                f,
                "the value says it is a `{type_name}`, and the `{base}` tag it carries \
                 is {tag}, which a reader binds as `{resolves_to}`"
            ),
            Self::DanglingAlias {
                type_name,
                field,
                address,
            } => write!(
                f,
                "`{type_name}::{field}` points where {address:#x} led, and nothing in this \
                 tree wrote that address — an alias has to follow the pointer that owns it"
            ),
            Self::BlobNotAPointer {
                type_name,
                field,
                repr,
            } => write!(
                f,
                "a blob codec is registered on `{type_name}::{field}`, which is {repr} rather \
                 than a plain pointer — a codec writes one pointer and has nowhere to state a \
                 count or a capacity"
            ),
            Self::Blob {
                type_name,
                field,
                message,
            } => write!(
                f,
                "the codec for `{type_name}::{field}` refused the value it was handed: {message}"
            ),
            Self::NotAMember {
                type_name,
                field,
                owner,
                member,
            } => write!(
                f,
                "`{type_name}::{field}` points at `{member}` of its `{owner}`, which is not an \
                 embedded member of the type the field points at"
            ),
            Self::WrongPack {
                type_name,
                record,
                pack,
            } => write!(
                f,
                "this `{type_name}` was read with the {record} pack and cannot be written \
                 with the {pack} pack"
            ),
            Self::Builder(message) => write!(f, "the resource builder refused a write: {message}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<BuildError> for crate::IoError {
    fn from(error: BuildError) -> Self {
        crate::IoError::SerializationFailed {
            offset: 0,
            message: error.to_string(),
        }
    }
}

type Result<T> = std::result::Result<T, BuildError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub type_name: String,
    pub field: String,
    pub address: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
    pub type_name: String,
    pub field: String,
    pub value: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildReport {
    pub dropped: Vec<Dropped>,
    pub carried: Vec<Carried>,
}

impl BuildReport {
    pub fn dropped_non_null(&self) -> impl Iterator<Item = &Dropped> {
        self.dropped.iter().filter(|d| d.address != 0)
    }
}

/// The two heaps and their fixups, deliberately not an RSC7 file: page
/// packing and the header are the envelope's.
#[derive(Debug)]
pub struct Built {
    pub root: u64,
    pub virtual_data: Vec<u8>,
    pub physical_data: Vec<u8>,
    pub virtual_pointers: Vec<PgPointerFixup>,
    pub physical_pointers: Vec<PgPointerFixup>,
    pub report: BuildReport,
}

#[derive(Clone, Default)]
pub struct BlobWriters(crate::desc::BySite<std::sync::Arc<dyn BlobWriter>>);

impl fmt::Debug for BlobWriters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sites: Vec<String> = self
            .0
            .sites()
            .map(|(ty, field, _)| format!("{ty}::{field}"))
            .chain(
                self.0
                    .arrivals()
                    .map(|(ty, field, _)| format!("behind {ty}::{field}")),
            )
            .collect();
        write!(f, "BlobWriters({})", sites.join(", "))
    }
}

impl BlobWriters {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn at(
        mut self,
        type_name: &str,
        field: &str,
        codec: std::sync::Arc<dyn BlobWriter>,
    ) -> Self {
        self.0.at_site(type_name, field, codec);
        self
    }

    #[must_use]
    pub fn reached_through(
        mut self,
        type_name: &str,
        field: &str,
        codec: std::sync::Arc<dyn BlobWriter>,
    ) -> Self {
        self.0.at_arrival(type_name, field, codec);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn sites(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.sites().map(|(ty, field, _)| (ty, field))
    }

    pub fn arrivals(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.arrivals().map(|(ty, field, _)| (ty, field))
    }

    /// Every `(owner type, field)` a registered codec fills as a sibling.
    pub fn sibling_sites(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.sites().flat_map(|(ty, _, codec)| {
            codec
                .writes_siblings()
                .iter()
                .map(move |field| (ty, *field))
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    pub address: u64,
    pub siblings: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Copy)]
pub struct Owner<'a> {
    pub ty: &'a PackType,
    pub record: RecordRef<'a>,
}

impl<'a> Owner<'a> {
    /// A pointer field's value.
    pub fn child(&self, name: &str) -> Option<&'a Value> {
        self.record.child(name)
    }

    pub fn int(&self, name: &str) -> Option<i128> {
        self.record.int(name)
    }
}

pub trait BlobHeap {
    fn pack(&self) -> &Pack;

    fn alloc(&mut self, size: usize, align: usize) -> u64;

    fn offset_of(&self, address: u64) -> usize;

    fn put(&mut self, at: usize, bytes: &[u8]) -> Result<()>;

    // Record that the eight bytes at `at` are an address the game must rebase.
    fn fixup(&mut self, at: usize, address: u64) -> Result<()>;

    fn place(&mut self, owner: &str, field: &str, value: &Value) -> Result<Option<u64>>;
}

pub trait BlobWriter: Send + Sync {
    fn write(&self, heap: &mut dyn BlobHeap, owner: Owner<'_>, value: &Value) -> Result<Placement>;

    /// The owner's **other** fields this codec fills, by way of
    /// [`Placement::siblings`].
    ///
    /// ⚠ Declared rather than inferred because a `Placement` exists only once
    /// a write has happened, and a comparison of two files has to know which
    /// fields are covered before either is written. A codec that returns
    /// siblings and does not name them here reads to
    /// [`compare`](crate::write::compare) as a writer inventing a pointer into a
    /// field nothing describes, and `same_content` refuses the file.
    fn writes_siblings(&self) -> &'static [&'static str] {
        &[]
    }
}

/// Lay out a value tree on a fresh [`ResourceHeap`], which has no bucket
/// packer to reach even by accident.
pub fn build(pack: &Pack, value: &Value) -> Result<Built> {
    let mut builder = ResourceHeap::new();
    let (root, report) = build_into_reporting(&mut builder, pack, value)?;
    let (virtual_data, physical_data, virtual_pointers, physical_pointers) =
        builder.finish_with_fixups();
    Ok(Built {
        root,
        virtual_data,
        physical_data,
        virtual_pointers,
        physical_pointers,
        report,
    })
}

pub fn build_into(builder: &mut ResourceHeap, pack: &Pack, value: &Value) -> Result<u64> {
    build_into_reporting(builder, pack, value).map(|(root, _)| root)
}

pub fn build_into_with(
    builder: &mut ResourceHeap,
    pack: &Pack,
    value: &Value,
    blobs: &BlobWriters,
) -> Result<u64> {
    build_into_reporting_with(builder, pack, value, blobs).map(|(root, _)| root)
}

pub fn build_into_reporting(
    builder: &mut ResourceHeap,
    pack: &Pack,
    value: &Value,
) -> Result<(u64, BuildReport)> {
    build_into_reporting_with(builder, pack, value, &BlobWriters::new())
}

pub fn build_into_reporting_with(
    builder: &mut ResourceHeap,
    pack: &Pack,
    value: &Value,
    blobs: &BlobWriters,
) -> Result<(u64, BuildReport)> {
    build_into_mapping_with(builder, pack, value, blobs).map(|(root, report, _)| (root, report))
}

/// One struct the interpreter allocated, and where it put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructAt {
    pub type_name: String,
    /// Offset into the **virtual** heap. Every described struct lives there;
    /// only payloads go to the physical one.
    pub at: usize,
    /// The header, without any trailing run.
    pub size: usize,
}

/// Where every described struct in a build landed.
///
/// ⚠ **A map, not a coverage claim.** It says a `PhBound` starts at offset N;
/// it does not say a single byte of it was written, and a writer producing
/// this map is in no position to testify that it did. Pairing it with
/// [`canvas`](crate::write::canvas), which measures coverage from outside the writer
/// entirely, is what makes an attribution mean something.
#[derive(Debug, Clone, Default)]
pub struct StructMap {
    pub structs: Vec<StructAt>,
}

pub fn build_into_mapping_with(
    builder: &mut ResourceHeap,
    pack: &Pack,
    value: &Value,
    blobs: &BlobWriters,
) -> Result<(u64, BuildReport, StructMap)> {
    let mut writer = Writer {
        pack,
        plans: pack.plans(),
        pack_id: pack.id(),
        builder,
        blobs,
        report: BuildReport::default(),
        map: StructMap::default(),
        written: Default::default(),
        deferred: Vec::new(),
        extents: Default::default(),
        deferred_extents: Vec::new(),
        tagged_bases: tagged_bases(pack),
        alignments: Default::default(),
    };
    let root = writer.write_struct(value, None, crate::desc::value::root_address(0))?;
    writer.patch_deferred()?;
    Ok((root, writer.report, writer.map))
}

/// The alignment a type's fields ask for: the widest scalar or pointer in it, at any depth.
pub fn natural_alignment(pack: &Pack, type_name: &str) -> usize {
    fn walk(pack: &Pack, type_name: &str, seen: &mut std::collections::BTreeSet<String>) -> usize {
        if !seen.insert(type_name.to_string()) {
            return 1;
        }
        let Some(ty) = pack.type_by_name(type_name) else {
            return 1;
        };
        ty.fields
            .iter()
            .map(|f| match &f.repr {
                FieldRepr::Scalar(s) | FieldRepr::ScalarArray { element: s, .. } => s.size().max(1),
                FieldRepr::Enum { width, .. } => width.size(),
                FieldRepr::Ptr(_) | FieldRepr::AtArray(_) | FieldRepr::PtrSlots { .. } => {
                    POINTER_SIZE
                }
                FieldRepr::Inline { type_name } | FieldRepr::InlineArray { type_name, .. } => {
                    walk(pack, type_name, seen)
                }
                FieldRepr::Padding { .. }
                | FieldRepr::Opaque { .. }
                | FieldRepr::Trailing { .. } => 1,
            })
            .max()
            .unwrap_or(1)
    }
    walk(pack, type_name, &mut std::collections::BTreeSet::new())
}

/// The count and capacity of the run `inner` a pointer at `pointee` holds.
fn extent(pointee: &Pointee, inner: &Value) -> (usize, usize) {
    match inner {
        Value::Array { elements, capacity } => (elements.len(), capacity.unwrap_or(elements.len())),
        Value::Records(run) => (run.len(), run.capacity().unwrap_or(run.len())),
        Value::Bytes(bytes) => {
            let stride = match pointee {
                Pointee::Many { element_size, .. } => element_size.unwrap_or(1).max(1),
                Pointee::ManyPtr { .. } | Pointee::Members { .. } => POINTER_SIZE,
                _ => 1,
            };
            let count = bytes.len() / stride;
            (count, count)
        }
        _ => (0, 0),
    }
}

fn tagged_bases(pack: &Pack) -> std::collections::HashMap<&str, Vec<&PackType>> {
    let mut bases: std::collections::HashMap<&str, Vec<&PackType>> = Default::default();
    for base in &pack.types {
        let Some(selector) = &base.selector else {
            continue;
        };
        bases.entry(base.name.as_str()).or_default().push(base);
        for (_, variant) in &selector.variants {
            let slot = bases.entry(variant.as_str()).or_default();
            if !slot.iter().any(|b| b.name == base.name) {
                slot.push(base);
            }
        }
    }
    bases
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Indirect,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tail {
    Reserved,
    Forbidden,
}

type Arrival<'p> = Option<(&'p str, &'p str)>;

/// The record being placed, where a member pointer in it or its embedded
/// structs lands: its plan, and its offset in the virtual heap.
#[derive(Debug, Clone, Copy)]
struct Home {
    index: u32,
    base: usize,
}

/// A pointer slot an alias reached before the build placed its target.
struct Deferred<'p> {
    ty: &'p PackType,
    field: &'p PackField,
    source: u64,
    /// Where the slot is, once the caller has put it.
    at: Option<usize>,
}

/// The stand-in address of the `n`th deferred slot is `DEFERRED | n`: no heap reaches these bits.
const DEFERRED: u64 = 0xDEFE_0000_0000_0000;
const DEFERRED_MASK: u64 = 0xFFFF_0000_0000_0000;

struct Writer<'p, 'b> {
    pack: &'p Pack,
    plans: &'p Plans,
    pack_id: String,
    builder: &'b mut ResourceHeap,

    blobs: &'p BlobWriters,

    report: BuildReport,

    map: StructMap,

    written: std::collections::HashMap<u64, u64>,

    /// Pointer slots an alias reached before its target was placed, patched at the end.
    deferred: Vec<Deferred<'p>>,

    /// Per source owning a run, the count and capacity an `atArray` aliasing it states.
    extents: std::collections::HashMap<u64, (usize, usize)>,

    /// Each `atArray` header an alias reached before its owner, and the source it aliases.
    deferred_extents: Vec<(usize, u64)>,

    tagged_bases: std::collections::HashMap<&'p str, Vec<&'p PackType>>,

    /// Each run element type's natural alignment, computed once.
    alignments: std::collections::HashMap<&'p str, usize>,
}

impl<'p> Writer<'p, '_> {
    /// Align the heap for a run of `ty`, so every element sits at its natural alignment (OD-ALIGN).
    ///
    /// ⚠ **A run of 16-byte multiples starts on 16.** `natural_alignment` is the widest scalar, so a run
    /// of float pairs (the model's bounds boxes, bone matrices) lands on 4 or 8, and the engine reads
    /// those with aligned SSE loads (`movaps`): the run sat on 16 only when what came before happened
    /// to end there, and a changed neighbour (the buffers' engine footprint) was an access violation.
    fn align_run(&mut self, ty: &'p PackType) {
        let pack = self.pack;
        let align = *self.alignments.entry(&ty.name).or_insert_with(|| {
            let natural = natural_alignment(pack, &ty.name);
            if ty.size.is_multiple_of(16) {
                natural.max(16)
            } else {
                natural
            }
        });
        self.builder.align_virtual(align);
    }

    /// The 16 bytes the engine reads the length of a bone array from, just before its first bone.
    ///
    /// ⚠ **The skeleton's bones are destroyed as an array** (`delete[]`): the destructor reads the count
    /// from the 16 bytes ahead of the run and walks that many bones. Retail files carry it (`count`, then
    /// filler); a run without it leaves the neighbour's bytes there, and the game dies when a drawable
    /// with a skeleton is evicted, long after it loaded (`mov rcx,[rcx]` on a pointer read past the end).
    /// The run itself stays on 16, the cookie being 16 bytes.
    fn write_array_cookie(&mut self, ty: &PackType, count: usize) {
        if ty.name == "CrBoneData" {
            let mut cookie = [0u8; 16];
            cookie[..4].copy_from_slice(&(count as u32).to_le_bytes());
            self.builder.write_bytes(&cookie);
        }
    }

    fn ty(&self, name: &str) -> Result<&'p PackType> {
        self.plans
            .get(name)
            .map(|plan| plan.pack_type())
            .map_err(|_| BuildError::NoType {
                type_name: name.to_string(),
            })
    }

    fn record_type(&self, record: &Record) -> Result<&'p PackType> {
        if record.pack_id() != self.pack_id {
            return Err(BuildError::WrongPack {
                type_name: record.type_name().to_string(),
                record: record.pack_id().to_string(),
                pack: self.pack_id.clone(),
            });
        }
        let ty = self.ty(record.type_name())?;
        if record.bytes().len() != ty.size {
            return Err(BuildError::WrongLength {
                type_name: ty.name.clone(),
                field: "<its own bytes>".into(),
                expected: ty.size,
                found: record.bytes().len(),
            });
        }
        Ok(ty)
    }

    fn run_type(&self, run: &RecordRun) -> Result<&'p PackType> {
        if run.pack_id() != self.pack_id {
            return Err(BuildError::WrongPack {
                type_name: run.type_name().to_string(),
                record: run.pack_id().to_string(),
                pack: self.pack_id.clone(),
            });
        }
        let ty = self.ty(run.type_name())?;
        if run.stride() != ty.size {
            return Err(BuildError::WrongLength {
                type_name: ty.name.clone(),
                field: "<its own bytes>".into(),
                expected: ty.size,
                found: run.stride(),
            });
        }
        Ok(ty)
    }

    /// Allocate and place a struct whose address in the source was `source`.
    ///
    /// ⚠ The source is recorded as soon as the struct has an address, before
    /// its children are placed: a child pointing back at it is an alias, and
    /// has to find the new address.
    fn write_struct(&mut self, value: &Value, arrival: Arrival<'p>, source: u64) -> Result<u64> {
        let Value::Struct(record) = value else {
            return Err(BuildError::Mismatch {
                type_name: "<root>".into(),
                field: String::new(),
                expected: "a described struct",
                found: shape_of(value),
            });
        };
        let ty = self.record_type(record)?;
        let rec = record.as_ref();

        // ⚠ **`PackType::size` is the header, not the allocation.** A type with
        // a trailing run occupies its size plus the run, and only an instance
        // says how long that is — which is why this is the one place a struct's
        // allocation is not `ty.size`, and why every other caller of
        // [`emit_body`](Self::emit_body) refuses a trailing run rather than
        // silently writing it over its neighbour.
        let tail = self.trailing_bytes(ty, rec)?;

        self.builder.align_virtual(STRUCT_ALIGN);
        let addr = self
            .builder
            .alloc_virtual((ty.size + tail).max(engine_footprint(&ty.name, ty.size)));
        if source != 0 {
            self.written.insert(source, addr);
        }
        let base = self.builder.virtual_offset_of(addr);
        self.map.structs.push(StructAt {
            type_name: ty.name.clone(),
            at: base,
            size: ty.size,
        });

        self.place(ty, rec, base, arrival, Tail::Reserved)?;
        self.check_tag(ty, base)?;
        Ok(addr)
    }

    /// Write one record's struct at `base`: its scalars, then its children.
    ///
    /// Only the plan's copy ranges come from the record. A pointer slot is
    /// written by whatever places its target and a byte no field describes
    /// stays zero, so an unchanged record builds to OpenRage's canonical form
    /// rather than to whatever the file happened to hold there.
    fn place(
        &mut self,
        ty: &'p PackType,
        rec: RecordRef<'_>,
        base: usize,
        arrival: Arrival<'p>,
        tail: Tail,
    ) -> Result<()> {
        let plan = self.plans.get(&ty.name).map_err(|_| BuildError::NoType {
            type_name: ty.name.clone(),
        })?;
        let bytes = rec.bytes();
        for &(start, end) in plan.copy_ranges() {
            self.put_bytes(base + start, &bytes[start..end])?;
        }
        let home = Home {
            index: plan.index(),
            base,
        };
        self.emit_body(ty, rec, base, arrival, tail, home)
    }

    fn trailing_bytes(&self, ty: &'p PackType, rec: RecordRef<'_>) -> Result<usize> {
        let Some(field) = ty.trailing() else {
            return Ok(0);
        };
        let FieldRepr::Trailing { type_name, .. } = &field.repr else {
            unreachable!("`PackType::trailing` answers with a trailing field");
        };
        let Some(value) = rec.child_at(field.offset) else {
            return Err(BuildError::FieldMissing {
                type_name: ty.name.clone(),
                field: field.name.clone(),
            });
        };

        match (type_name, value) {
            (Some(name), Value::Array { elements, .. }) => Ok(elements.len() * self.ty(name)?.size),
            (Some(name), Value::Records(run)) => Ok(run.len() * self.ty(name)?.size),
            (None, Value::Bytes(bytes)) => Ok(bytes.len()),
            (_, Value::Array { elements, .. }) if elements.is_empty() => Ok(0),
            (_, Value::Bytes(bytes)) if bytes.is_empty() => Ok(0),
            _ => Err(BuildError::Mismatch {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                expected: match type_name {
                    Some(_) => "a run of described structs",
                    None => "a run of bytes",
                },
                found: shape_of(value),
            }),
        }
    }

    fn arrival_in_pack(&self, owner: &str, field: &str) -> Arrival<'p> {
        let ty = self.pack.type_by_name(owner)?;
        let field = ty.field_by_name(field)?;
        Some((ty.name.as_str(), field.name.as_str()))
    }

    fn check_tag(&self, ty: &'p PackType, base: usize) -> Result<()> {
        let Some(bases) = self.tagged_bases.get(ty.name.as_str()) else {
            return Ok(());
        };

        for candidate in bases {
            let Some(selector) = &candidate.selector else {
                continue;
            };
            let Some(tag) = selector.read_tag(candidate, self.builder.virtual_data(), base) else {
                continue;
            };
            let resolves_to = selector.type_for(tag).unwrap_or(candidate.name.as_str());
            if resolves_to != ty.name {
                return Err(BuildError::TagDisagrees {
                    type_name: ty.name.clone(),
                    base: candidate.name.clone(),
                    tag,
                    resolves_to: resolves_to.to_string(),
                });
            }
        }
        Ok(())
    }

    fn emit_body(
        &mut self,
        ty: &'p PackType,
        rec: RecordRef<'_>,
        base: usize,
        arrival: Arrival<'p>,
        tail: Tail,
        home: Home,
    ) -> Result<()> {
        self.check_counts(ty, rec)?;

        if let Some(field) = ty.trailing() {
            if tail == Tail::Forbidden {
                return Err(BuildError::Mismatch {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    expected: "a position where the size is still open, because this type \
                               trails a run of its own",
                    found: "a fixed-size slot",
                });
            }
        }

        let mut siblings: Vec<(String, u64)> = Vec::new();

        for phase in [Phase::Indirect, Phase::Direct] {
            for field in &ty.fields {
                let belongs = if field.repr.is_indirect() {
                    Phase::Indirect
                } else {
                    Phase::Direct
                };
                if belongs != phase {
                    continue;
                }

                match phase {
                    Phase::Indirect => {
                        let value =
                            rec.child_at(field.offset)
                                .ok_or_else(|| BuildError::FieldMissing {
                                    type_name: ty.name.clone(),
                                    field: field.name.clone(),
                                })?;
                        let codec = if self.blobs.is_empty() {
                            None
                        } else {
                            self.blobs
                                .0
                                .resolve(arrival, (ty.name.as_str(), field.name.as_str()))
                                .cloned()
                        };
                        match codec {
                            Some(codec) => {
                                self.emit_blob(
                                    ty,
                                    field,
                                    codec.as_ref(),
                                    rec,
                                    value,
                                    base,
                                    &mut siblings,
                                )?;
                            }
                            None => self.emit_indirect(ty, field, value, base, home)?,
                        }
                    }
                    Phase::Direct => self.emit_direct(ty, field, rec, base, arrival, home)?,
                }
            }
        }

        for (name, address) in siblings {
            let Some(field) = ty.field_by_name(&name) else {
                return Err(BuildError::FieldMissing {
                    type_name: ty.name.clone(),
                    field: name,
                });
            };
            // ⚠ **Only a field the interpreter leaves alone.** A codec placing a
            // sibling the ordinary emit also wrote would put two fixups at one
            // offset, and the game rebases each of them — the "invalid fixup,
            // address is neither virtual nor physical" load failure.
            // `Ptr(Opaque)` is exactly the set the interpreter writes nothing
            // for, and it is what Enhanced's `texture_refs` and
            // `unknown_params` are. Refused rather than ordered around, because
            // ordering the two would make a codec silently outrank a
            // description somebody added later.
            if !matches!(field.repr, FieldRepr::Ptr(Pointee::Opaque)) {
                return Err(BuildError::Blob {
                    type_name: ty.name.clone(),
                    field: name.clone(),
                    message: format!(
                        "a codec placed `{name}`, which the description says is {} — a \
                         sibling placement is only safe on a pointer the pack declines \
                         to describe, because that is the one the interpreter writes \
                         nothing for",
                        repr_name(&field.repr)
                    ),
                });
            }
            self.put_pointer(field, base + field.offset, address)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_blob(
        &mut self,
        ty: &'p PackType,
        field: &'p PackField,
        codec: &dyn BlobWriter,
        rec: RecordRef<'_>,
        value: &Value,
        base: usize,
        siblings: &mut Vec<(String, u64)>,
    ) -> Result<()> {
        if !matches!(field.repr, FieldRepr::Ptr(_)) {
            return Err(BuildError::BlobNotAPointer {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                repr: repr_name(&field.repr),
            });
        }

        match value {
            Value::Ptr { target: None, .. } => {
                return self.put_null(base + field.offset, POINTER_SIZE)
            }
            Value::NotAnAddress(raw) => {
                self.report.carried.push(Carried {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    value: *raw,
                });
                return self.put_bytes(base + field.offset, &raw.to_le_bytes());
            }
            Value::Alias(address) => {
                return match self.written.get(address) {
                    Some(written) => {
                        let written = *written;
                        self.put_pointer(field, base + field.offset, written)
                    }
                    None => Err(BuildError::DanglingAlias {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        address: *address,
                    }),
                }
            }
            _ => {}
        }

        let (source, inner) = match value {
            Value::Ptr {
                source,
                target: Some(inner),
            } => (*source, inner.as_ref()),
            other => {
                return Err(BuildError::Mismatch {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    expected: "a pointer at an allocation a codec wrote",
                    found: shape_of(other),
                })
            }
        };

        let placement = codec.write(self, Owner { ty, record: rec }, inner)?;
        if placement.address == 0 {
            return Ok(());
        }
        if source != 0 {
            self.written.insert(source, placement.address);
        }
        self.put_pointer(field, base + field.offset, placement.address)?;
        siblings.extend(placement.siblings);
        Ok(())
    }

    fn check_counts(&self, ty: &'p PackType, rec: RecordRef<'_>) -> Result<()> {
        for field in &ty.fields {
            let (source, run_holds) = match &field.repr {
                FieldRepr::Ptr(pointee) => {
                    let Some(source) = length_source(pointee) else {
                        continue;
                    };
                    let Some(value) = rec.child_at(field.offset) else {
                        continue;
                    };
                    let Some(holds) = run_length(pointee, value) else {
                        continue;
                    };
                    (source, holds)
                }
                FieldRepr::Trailing {
                    type_name,
                    count,
                    element_size,
                } => {
                    let Some(value) = rec.child_at(field.offset) else {
                        continue;
                    };
                    let Some(holds) = trailing_length(type_name, *element_size, value) else {
                        continue;
                    };
                    (count, holds)
                }
                _ => continue,
            };

            match source {
                CountSource::Fixed(expected) => {
                    if run_holds != *expected {
                        return Err(BuildError::WrongLength {
                            type_name: ty.name.clone(),
                            field: field.name.clone(),
                            expected: *expected,
                            found: run_holds,
                        });
                    }
                    continue;
                }
                CountSource::Inline | CountSource::Unknown => continue,
                _ => {}
            }

            let names = source.reads_fields();
            let mut raws = Vec::with_capacity(names.len());
            for name in &names {
                let Some(raw) = rec.int(name) else {
                    break;
                };
                raws.push(raw);
            }
            if raws.len() != names.len() {
                continue;
            }
            let Some(count_says) = source.length_from(&raws) else {
                continue;
            };

            if count_says != run_holds {
                return Err(BuildError::CountDisagrees {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    count_field: names.join(" * "),
                    count_says,
                    run_holds,
                });
            }
        }
        Ok(())
    }

    fn emit_direct(
        &mut self,
        ty: &'p PackType,
        field: &PackField,
        rec: RecordRef<'_>,
        base: usize,
        arrival: Arrival<'p>,
        home: Home,
    ) -> Result<()> {
        let at = base + field.offset;
        let wrong = |expected: &'static str, value: &Value| BuildError::Mismatch {
            type_name: ty.name.clone(),
            field: field.name.clone(),
            expected,
            found: shape_of(value),
        };
        let missing = || BuildError::FieldMissing {
            type_name: ty.name.clone(),
            field: field.name.clone(),
        };

        match &field.repr {
            // Scalars, enums and regions came across with the record's copy
            // ranges; pointers are the indirect phase's.
            FieldRepr::Scalar(_)
            | FieldRepr::ScalarArray { .. }
            | FieldRepr::Enum { .. }
            | FieldRepr::Padding { .. }
            | FieldRepr::Opaque { .. }
            | FieldRepr::Ptr(_)
            | FieldRepr::AtArray(_)
            | FieldRepr::PtrSlots { .. } => Ok(()),

            FieldRepr::Inline { type_name } => {
                let inner = self.ty(type_name)?;
                let embedded = rec.embedded(type_name, field.offset).ok_or_else(missing)?;
                self.emit_body(inner, embedded, at, arrival, Tail::Forbidden, home)
            }

            FieldRepr::InlineArray { type_name, count } => {
                let inner = self.ty(type_name)?;
                for i in 0..*count {
                    let step = i * inner.size;
                    let embedded = rec
                        .embedded(type_name, field.offset + step)
                        .ok_or_else(missing)?;
                    self.emit_body(inner, embedded, at + step, arrival, Tail::Forbidden, home)?;
                }
                Ok(())
            }

            FieldRepr::Trailing { type_name, .. } => {
                let value = rec.child_at(field.offset).ok_or_else(missing)?;
                match (type_name, value) {
                    (Some(name), Value::Array { elements, .. }) => {
                        let stride = self.ty(name)?.size;
                        for (i, item) in elements.iter().enumerate() {
                            let Value::Struct(element) = item else {
                                return Err(wrong("a run of described structs", item));
                            };
                            let inner = self.record_type(element)?;
                            self.place(
                                inner,
                                element.as_ref(),
                                at + i * stride,
                                arrival,
                                Tail::Forbidden,
                            )?;
                        }
                        Ok(())
                    }
                    (Some(name), Value::Records(run)) => {
                        let stride = self.ty(name)?.size;
                        let inner = self.run_type(run)?;
                        for (i, element) in run.iter().enumerate() {
                            self.place(inner, element, at + i * stride, arrival, Tail::Forbidden)?;
                        }
                        Ok(())
                    }
                    (None, Value::Bytes(bytes)) => self.put_bytes(at, bytes),
                    (_, Value::Array { elements, .. }) if elements.is_empty() => Ok(()),
                    (_, Value::Bytes(bytes)) if bytes.is_empty() => Ok(()),
                    (Some(_), other) => Err(wrong("a run of described structs", other)),
                    (None, other) => Err(wrong("a run of bytes", other)),
                }
            }
        }
    }

    fn emit_indirect(
        &mut self,
        ty: &'p PackType,
        field: &'p PackField,
        value: &Value,
        base: usize,
        home: Home,
    ) -> Result<()> {
        let at = base + field.offset;

        // Not an address: the shipped file has no fixup for it either, so it
        // goes back verbatim and none is registered.
        if let Value::NotAnAddress(raw) = value {
            self.report.carried.push(Carried {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                value: *raw,
            });
            return self.put_bytes(at, &raw.to_le_bytes());
        }

        match &field.repr {
            FieldRepr::Ptr(pointee) => match self.target_of(ty, field, pointee, value, home)? {
                Some(addr) => self.put_pointer(field, at, addr),
                None => self.put_null(at, POINTER_SIZE),
            },

            FieldRepr::AtArray(pointee) => self.emit_at_array(ty, field, pointee, value, at, home),

            FieldRepr::PtrSlots { pointee, count } => {
                let elements = self.elements_of(ty, field, value, *count)?;
                // Every slot's target is allocated before any pointer is
                // patched, which is what the macro's `as_child_ptr` array arm
                // does and what keeps the targets contiguous.
                //
                // `(bytes, is_an_address)`, carried for the same reason
                // `write_pointer_table` carries it: a slot holding something
                // that is not an address goes back verbatim and must *not* be
                // fixed up, which is the whole of what makes preserving it safe.
                // Refusing one instead refuses 21 of 48 `.ydd` files, whose
                // `RmcLodGroup::lods` carries a small integer in a slot.
                let mut slots: Vec<Option<(u64, bool)>> = Vec::with_capacity(elements.len());
                for item in &elements {
                    slots.push(match item {
                        Value::NotAnAddress(raw) => {
                            self.report.carried.push(Carried {
                                type_name: ty.name.clone(),
                                field: field.name.clone(),
                                value: *raw,
                            });
                            Some((*raw, false))
                        }
                        other => self
                            .target_of(ty, field, pointee, other, home)?
                            .map(|address| (address, true)),
                    });
                }
                for (i, slot) in slots.into_iter().enumerate() {
                    let at = at + i * POINTER_SIZE;
                    let Some((bytes, is_an_address)) = slot else {
                        self.put_null(at, POINTER_SIZE)?;
                        continue;
                    };
                    if is_an_address {
                        self.put_pointer(field, at, bytes)?;
                    } else {
                        self.put_bytes(at, &bytes.to_le_bytes())?;
                    }
                }
                Ok(())
            }

            _ => Ok(()),
        }
    }

    fn emit_at_array(
        &mut self,
        ty: &'p PackType,
        field: &'p PackField,
        pointee: &Pointee,
        value: &Value,
        at: usize,
        home: Home,
    ) -> Result<()> {
        let (count, capacity) = match value {
            Value::Ptr {
                target: Some(inner),
                ..
            } => extent(pointee, inner),
            // An alias states its owner's run; one reached before its owner is patched at the end.
            Value::Alias(source) => match self.extents.get(source) {
                Some(extent) => *extent,
                None => {
                    self.deferred_extents.push((at, *source));
                    (0, 0)
                }
            },
            _ => (0, 0),
        };

        match self.target_of(ty, field, pointee, value, home)? {
            Some(addr) => self.put_pointer(field, at, addr)?,
            None => self.put_null(at, POINTER_SIZE)?,
        }
        self.put_bytes(at + POINTER_SIZE, &(count as u16).to_le_bytes())?;
        self.put_bytes(at + POINTER_SIZE + 2, &(capacity as u16).to_le_bytes())?;
        // An `atArray` header is sixteen bytes and the last four are the
        // engine's own padding, which nothing describes and nothing reads.
        self.put_null(at + POINTER_SIZE + 4, AT_ARRAY_SIZE - POINTER_SIZE - 4)?;
        Ok(())
    }

    fn target_of(
        &mut self,
        ty: &'p PackType,
        field: &'p PackField,
        pointee: &Pointee,
        value: &Value,
        home: Home,
    ) -> Result<Option<u64>> {
        let here: Arrival<'p> = Some((ty.name.as_str(), field.name.as_str()));
        if let Pointee::Member { type_name } = pointee {
            return match value {
                Value::Ptr { target: None, .. } => Ok(None),
                Value::Member { field: path, index } => self
                    .member_address(ty, field, home, type_name, path, *index)
                    .map(Some),
                other => Err(BuildError::Mismatch {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    expected: "a member of its owner",
                    found: shape_of(other),
                }),
            };
        }
        let (source, inner) = match value {
            Value::Ptr { target: None, .. } => return Ok(None),
            Value::Ptr {
                source,
                target: Some(inner),
            } => (*source, inner.as_ref()),

            Value::Alias(address) => return Ok(Some(self.aliased(ty, field, *address))),

            Value::UndescribedPtr(address) => {
                self.report.dropped.push(Dropped {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    address: *address,
                });
                return Ok(None);
            }
            other => {
                return Err(BuildError::Mismatch {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    expected: "a pointer",
                    found: shape_of(other),
                })
            }
        };

        let address = match pointee {
            Pointee::Opaque => {
                return Ok(None);
            }

            Pointee::Member { .. } => unreachable!("answered above"),

            Pointee::Members { type_name, .. } => {
                let Value::Array { elements, .. } = inner else {
                    return Err(BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a run of member pointers",
                        found: shape_of(inner),
                    });
                };
                self.write_member_table(ty, field, home, type_name, elements)?
            }

            Pointee::One { .. } => self.write_struct(inner, here, source)?,

            Pointee::Str => {
                let bytes = inner.as_bytes().ok_or_else(|| BuildError::Mismatch {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    expected: "a string's bytes",
                    found: shape_of(inner),
                })?;
                let address = self.builder.write_bytes(bytes);
                self.builder.align_virtual(STRING_ALIGN);
                address
            }

            Pointee::Bytes { .. } => {
                let bytes = inner.as_bytes().ok_or_else(|| BuildError::Mismatch {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    expected: "a byte payload",
                    found: shape_of(inner),
                })?;
                self.write_blob(field, bytes)
            }

            Pointee::Many { .. } => match inner {
                Value::Bytes(bytes) => self.write_typed_payload(field, bytes),
                Value::Array { elements, .. } => self.write_run(field, elements, here)?,
                Value::Records(run) => self.write_records(field, run, here)?,
                other => {
                    return Err(BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a run of values",
                        found: shape_of(other),
                    })
                }
            },

            Pointee::ManyPtr { .. } => {
                let Value::Array { elements, .. } = inner else {
                    return Err(BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a run of pointers",
                        found: shape_of(inner),
                    });
                };
                self.write_pointer_table(ty, field, elements)?
            }

            Pointee::Strings { .. } => {
                let Value::Array { elements, .. } = inner else {
                    return Err(BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a run of string pointers",
                        found: shape_of(inner),
                    });
                };
                self.write_string_table(ty, field, elements)?
            }
        };

        if source != 0 {
            self.written.insert(source, address);
            if let Pointee::Many { .. }
            | Pointee::ManyPtr { .. }
            | Pointee::Members { .. }
            | Pointee::Strings { .. } = pointee
            {
                self.extents.insert(source, extent(pointee, inner));
            }
        }

        Ok(Some(address))
    }

    /// A table of pointers, each at a string written before it.
    fn write_string_table(
        &mut self,
        ty: &'p PackType,
        field: &'p PackField,
        elements: &[Value],
    ) -> Result<u64> {
        if elements.is_empty() {
            return Ok(self.reserve_empty(field));
        }
        let mut slots: Vec<Option<(u64, bool)>> = Vec::with_capacity(elements.len());
        for element in elements {
            slots.push(match element {
                Value::Ptr { target: None, .. } => None,
                Value::NotAnAddress(raw) => {
                    self.report.carried.push(Carried {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        value: *raw,
                    });
                    Some((*raw, false))
                }
                Value::Alias(address) => Some((self.aliased(ty, field, *address), true)),
                Value::Ptr {
                    source,
                    target: Some(text),
                } => {
                    let bytes = text.as_bytes().ok_or_else(|| BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a string's bytes",
                        found: shape_of(text),
                    })?;
                    let address = self.builder.write_bytes(bytes);
                    self.builder.align_virtual(STRING_ALIGN);
                    if *source != 0 {
                        self.written.insert(*source, address);
                    }
                    Some((address, true))
                }
                other => {
                    return Err(BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a pointer at a string",
                        found: shape_of(other),
                    })
                }
            });
        }
        self.builder.align_virtual(POINTER_SIZE);
        let table = self.builder.alloc_virtual(slots.len() * POINTER_SIZE);
        let base = self.builder.virtual_offset_of(table);
        for (i, slot) in slots.into_iter().enumerate() {
            let at = base + i * POINTER_SIZE;
            match slot {
                None => self.put_null(at, POINTER_SIZE)?,
                Some((address, true)) => self.put_pointer(field, at, address)?,
                Some((raw, false)) => self.put_bytes(at, &raw.to_le_bytes())?,
            }
        }
        Ok(table)
    }

    /// Where the target an alias names was written, or a stand-in the build
    /// patches once it has been, for an alias that comes before its target.
    fn aliased(&mut self, ty: &'p PackType, field: &'p PackField, address: u64) -> u64 {
        match self.written.get(&address) {
            Some(written) => *written,
            None => {
                self.deferred.push(Deferred {
                    ty,
                    field,
                    source: address,
                    at: None,
                });
                DEFERRED | (self.deferred.len() - 1) as u64
            }
        }
    }

    /// Point every slot an alias reached before its target at where that target landed.
    fn patch_deferred(&mut self) -> Result<()> {
        for (at, source) in std::mem::take(&mut self.deferred_extents) {
            if let Some(&(count, capacity)) = self.extents.get(&source) {
                self.put_bytes(at + POINTER_SIZE, &(count as u16).to_le_bytes())?;
                self.put_bytes(at + POINTER_SIZE + 2, &(capacity as u16).to_le_bytes())?;
            }
        }
        for d in std::mem::take(&mut self.deferred) {
            let address =
                self.written
                    .get(&d.source)
                    .copied()
                    .ok_or_else(|| BuildError::DanglingAlias {
                        type_name: d.ty.name.clone(),
                        field: d.field.name.clone(),
                        address: d.source,
                    })?;
            if let Some(at) = d.at {
                self.put_pointer(d.field, at, address)?;
            }
        }
        Ok(())
    }

    /// Where member `path` of the record being placed now lives.
    fn member_address(
        &self,
        ty: &PackType,
        field: &PackField,
        home: Home,
        member: &str,
        path: &str,
        index: Option<usize>,
    ) -> Result<u64> {
        let offset = self
            .plans
            .member_offset(home.index, member, path, index)
            .ok_or_else(|| BuildError::NotAMember {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                owner: self.plans.at(home.index).name().to_string(),
                member: match index {
                    Some(i) => format!("{path}[{i}]"),
                    None => path.to_string(),
                },
            })?;
        let heap_start = self.builder.virtual_address() - u64::from(self.builder.virtual_offset());
        Ok(heap_start + (home.base + offset) as u64)
    }

    /// A table whose entries point at members of the record being placed.
    fn write_member_table(
        &mut self,
        ty: &'p PackType,
        field: &'p PackField,
        home: Home,
        member: &str,
        elements: &[Value],
    ) -> Result<u64> {
        if elements.is_empty() {
            return Ok(self.reserve_empty(field));
        }
        let mut slots: Vec<Option<(u64, bool)>> = Vec::with_capacity(elements.len());
        for item in elements {
            slots.push(match item {
                Value::Ptr { target: None, .. } => None,
                Value::NotAnAddress(raw) => {
                    self.report.carried.push(Carried {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        value: *raw,
                    });
                    Some((*raw, false))
                }
                Value::Member { field: path, index } => Some((
                    self.member_address(ty, field, home, member, path, *index)?,
                    true,
                )),
                other => {
                    return Err(BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a member of its owner, in a table",
                        found: shape_of(other),
                    })
                }
            });
        }
        let table = self.builder.alloc_virtual(slots.len() * POINTER_SIZE);
        let base = self.builder.virtual_offset_of(table);
        for (i, slot) in slots.into_iter().enumerate() {
            let at = base + i * POINTER_SIZE;
            match slot {
                None => self.put_null(at, POINTER_SIZE)?,
                Some((address, true)) => self.put_pointer(field, at, address)?,
                Some((raw, false)) => self.put_bytes(at, &raw.to_le_bytes())?,
            }
        }
        Ok(table)
    }

    fn write_pointer_table(
        &mut self,
        ty: &'p PackType,
        field: &'p PackField,
        elements: &[Value],
    ) -> Result<u64> {
        let here: Arrival<'p> = Some((ty.name.as_str(), field.name.as_str()));
        if elements.is_empty() {
            return Ok(self.reserve_empty(field));
        }

        let mut addresses: Vec<(u64, bool)> = Vec::with_capacity(elements.len());
        for item in elements {
            let address = match item {
                Value::Ptr { target: None, .. } => 0,
                Value::NotAnAddress(raw) => {
                    self.report.carried.push(Carried {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        value: *raw,
                    });
                    addresses.push((*raw, false));
                    continue;
                }
                Value::Ptr {
                    source,
                    target: Some(inner),
                } => self.write_struct(inner, here, *source)?,
                Value::Alias(address) => self.aliased(ty, field, *address),
                Value::UndescribedPtr(address) => {
                    self.report.dropped.push(Dropped {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        address: *address,
                    });
                    0
                }
                other => {
                    return Err(BuildError::Mismatch {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        expected: "a pointer in a table",
                        found: shape_of(other),
                    })
                }
            };
            addresses.push((address, true));
        }

        // ⚠ The heap is linear, so a table placed after a run of `u32`s lands
        // at four modulo eight and no typed view of it opens — every `.yfd`'s
        // dictionary table followed a weight run.
        self.builder.align_virtual(POINTER_SIZE);
        let table = self.builder.alloc_virtual(addresses.len() * POINTER_SIZE);
        let base = self.builder.virtual_offset_of(table);
        for (i, (address, fixup)) in addresses.into_iter().enumerate() {
            let at = base + i * POINTER_SIZE;
            if address == 0 {
                self.put_null(at, POINTER_SIZE)?;
                continue;
            }
            if fixup {
                self.put_pointer(field, at, address)?;
            } else {
                self.put_bytes(at, &address.to_le_bytes())?;
            }
        }
        Ok(table)
    }

    fn write_run(
        &mut self,
        field: &PackField,
        elements: &[Value],
        arrival: Arrival<'p>,
    ) -> Result<u64> {
        if elements.is_empty() {
            return Ok(self.reserve_empty(field));
        }

        let mut records = Vec::with_capacity(elements.len());
        for item in elements {
            let Value::Struct(record) = item else {
                return Err(BuildError::Mismatch {
                    type_name: "<run>".into(),
                    field: field.name.clone(),
                    expected: "a run of described structs",
                    found: shape_of(item),
                });
            };
            records.push((self.record_type(record)?, record));
        }

        for (ty, _) in &records {
            self.align_run(ty);
        }
        if let Some((ty, _)) = records.first() {
            self.write_array_cookie(ty, records.len());
        }
        let total: usize = records.iter().map(|(ty, _)| ty.size).sum();
        let address = self.builder.alloc_virtual(total);
        let mut base = self.builder.virtual_offset_of(address);

        for (ty, record) in records {
            self.map.structs.push(StructAt {
                type_name: ty.name.clone(),
                at: base,
                size: ty.size,
            });
            self.place(ty, record.as_ref(), base, arrival, Tail::Forbidden)?;
            base += ty.size;
        }

        Ok(address)
    }

    /// [`write_run`](Self::write_run) for a packed run: one allocation, and
    /// each element placed through the plan like a record, so an unchanged
    /// run builds to the same canonical form.
    fn write_records(
        &mut self,
        field: &PackField,
        run: &RecordRun,
        arrival: Arrival<'p>,
    ) -> Result<u64> {
        if run.is_empty() {
            return Ok(self.reserve_empty(field));
        }
        let ty = self.run_type(run)?;
        self.align_run(ty);
        self.write_array_cookie(ty, run.len());
        let address = self.builder.alloc_virtual(run.len() * ty.size);
        let mut base = self.builder.virtual_offset_of(address);
        for element in run.iter() {
            self.map.structs.push(StructAt {
                type_name: ty.name.clone(),
                at: base,
                size: ty.size,
            });
            self.place(ty, element, base, arrival, Tail::Forbidden)?;
            base += ty.size;
        }
        Ok(address)
    }

    fn write_blob(&mut self, field: &PackField, bytes: &[u8]) -> u64 {
        if bytes.is_empty() {
            return self.reserve_empty(field);
        }
        if !field.is_physical() {
            return self.builder.write_bytes(bytes);
        }

        self.builder.align_physical(PHYSICAL_BLOB_ALIGN);
        let address = self.builder.write_physical_bytes(bytes);
        let padded = bytes.len().next_multiple_of(PHYSICAL_BLOB_PAD);
        if padded > bytes.len() {
            self.builder.write_physical_padding(padded - bytes.len());
        }
        address
    }

    fn write_typed_payload(&mut self, field: &PackField, bytes: &[u8]) -> u64 {
        if bytes.is_empty() {
            return self.reserve_empty(field);
        }
        if field.is_physical() {
            self.builder.write_physical_bytes(bytes)
        } else {
            self.builder.write_bytes(bytes)
        }
    }

    fn reserve_empty(&mut self, field: &PackField) -> u64 {
        if field.is_physical() {
            self.builder.alloc_physical(EMPTY_RUN_RESERVATION)
        } else {
            self.builder.alloc_virtual(EMPTY_RUN_RESERVATION)
        }
    }

    /// Write a field's bytes as zero rather than leaving them reserved.
    ///
    /// ⚠ The output is identical, because a reservation is zero-filled, but a
    /// null left to the reservation is right only because the canvas held it,
    /// which is the inheritance [`crate::write::canvas`] exists to find. Across
    /// the 21-file `.ybn` corpus that was 96 `pgBase::first_node` and 74
    /// `phBoundGeometry::_octant_verts` among others, every one a null the
    /// interpreter meant and none a null it wrote.
    fn put_null(&mut self, at: usize, width: usize) -> Result<()> {
        const ZEROS: [u8; AT_ARRAY_SIZE] = [0; AT_ARRAY_SIZE];
        self.put_bytes(at, &ZEROS[..width])
    }

    fn put_pointer(&mut self, field: &PackField, at: usize, address: u64) -> Result<()> {
        if address == 0 {
            return self.put_null(at, POINTER_SIZE);
        }
        if address & DEFERRED_MASK == DEFERRED {
            let d = &mut self.deferred[(address & !DEFERRED_MASK) as usize];
            d.at = Some(at);
            return self.put_null(at, POINTER_SIZE);
        }
        self.put_bytes(at, &address.to_le_bytes())?;
        let at = at as u32;
        let registered = if field.is_physical() {
            self.builder.register_physical_fixup(at, address)
        } else {
            self.builder.register_fixup(at, address)
        };
        registered.map_err(|e| BuildError::Builder(e.to_string()))
    }

    fn put_bytes(&mut self, at: usize, bytes: &[u8]) -> Result<()> {
        self.builder
            .write_bytes_at(at, bytes)
            .map_err(|e| BuildError::Builder(e.to_string()))
    }

    fn elements_of(
        &self,
        ty: &PackType,
        field: &PackField,
        value: &Value,
        expected: usize,
    ) -> Result<Vec<Value>> {
        let Value::Array { elements, .. } = value else {
            return Err(BuildError::Mismatch {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                expected: "a fixed run",
                found: shape_of(value),
            });
        };
        if elements.len() != expected {
            return Err(BuildError::WrongLength {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                expected,
                found: elements.len(),
            });
        }
        Ok(elements.clone())
    }
}

impl BlobHeap for Writer<'_, '_> {
    fn pack(&self) -> &Pack {
        self.pack
    }

    fn alloc(&mut self, size: usize, align: usize) -> u64 {
        if align > 1 {
            self.builder.align_virtual(align);
        }
        self.builder.alloc_virtual(size)
    }

    fn offset_of(&self, address: u64) -> usize {
        self.builder.virtual_offset_of(address)
    }

    fn put(&mut self, at: usize, bytes: &[u8]) -> Result<()> {
        self.put_bytes(at, bytes)
    }

    fn fixup(&mut self, at: usize, address: u64) -> Result<()> {
        self.builder
            .register_fixup(at as u32, address)
            .map_err(|e| BuildError::Builder(e.to_string()))
    }

    fn place(&mut self, owner: &str, field: &str, value: &Value) -> Result<Option<u64>> {
        match value {
            Value::Ptr { target: None, .. } => Ok(None),
            Value::Ptr {
                source,
                target: Some(inner),
            } => {
                let arrival = self.arrival_in_pack(owner, field);
                Ok(Some(self.write_struct(inner, arrival, *source)?))
            }
            Value::Alias(address) => match self.written.get(address) {
                Some(written) => Ok(Some(*written)),
                None => Err(BuildError::DanglingAlias {
                    type_name: owner.to_string(),
                    field: field.to_string(),
                    address: *address,
                }),
            },
            Value::UndescribedPtr(address) => {
                self.report.dropped.push(Dropped {
                    type_name: owner.to_string(),
                    field: field.to_string(),
                    address: *address,
                });
                Ok(None)
            }
            Value::NotAnAddress(raw) => {
                self.report.carried.push(Carried {
                    type_name: owner.to_string(),
                    field: field.to_string(),
                    value: *raw,
                });
                Ok(None)
            }
            other => Err(BuildError::Mismatch {
                type_name: owner.to_string(),
                field: field.to_string(),
                expected: "a pointer at an owned child",
                found: shape_of(other),
            }),
        }
    }
}

fn repr_name(repr: &FieldRepr) -> &'static str {
    match repr {
        FieldRepr::Scalar(_) => "a scalar",
        FieldRepr::ScalarArray { .. } => "a fixed run of scalars",
        FieldRepr::Enum { .. } => "an enum",
        FieldRepr::Inline { .. } => "an embedded struct",
        FieldRepr::InlineArray { .. } => "a fixed run of embedded structs",
        FieldRepr::Padding { .. } => "padding",
        FieldRepr::Opaque { .. } => "an undescribed region",
        FieldRepr::Ptr(_) => "a pointer",
        FieldRepr::AtArray(_) => "an atArray header",
        FieldRepr::PtrSlots { .. } => "a fixed run of pointer slots",
        FieldRepr::Trailing { .. } => "a run trailing the struct",
    }
}

fn length_source(pointee: &Pointee) -> Option<&crate::desc::CountSource> {
    match pointee {
        Pointee::Many { count, .. }
        | Pointee::ManyPtr { count, .. }
        | Pointee::Members { count, .. }
        | Pointee::Strings { count } => Some(count),
        Pointee::Bytes { length } => Some(length),
        Pointee::One { .. } | Pointee::Str | Pointee::Opaque | Pointee::Member { .. } => None,
    }
}

fn trailing_length(
    type_name: &Option<String>,
    element_size: Option<usize>,
    value: &Value,
) -> Option<usize> {
    match value {
        Value::Array { elements, .. } => Some(elements.len()),
        Value::Records(run) => Some(run.len()),
        Value::Bytes(bytes) if type_name.is_none() => {
            Some(bytes.len() / element_size.unwrap_or(1).max(1))
        }
        _ => None,
    }
}

fn run_length(pointee: &Pointee, value: &Value) -> Option<usize> {
    let Value::Ptr {
        target: Some(inner),
        ..
    } = value
    else {
        return None;
    };

    match inner.as_ref() {
        Value::Array { elements, .. } => Some(elements.len()),
        Value::Records(run) => Some(run.len()),
        Value::Bytes(bytes) => match pointee {
            Pointee::Many { element_size, .. } => {
                Some(bytes.len() / element_size.unwrap_or(1).max(1))
            }
            Pointee::Bytes { .. } => Some(bytes.len()),
            _ => None,
        },
        _ => None,
    }
}

fn shape_of(value: &Value) -> &'static str {
    match value {
        Value::Scalar(ScalarValue::Int(_)) => "an integer",
        Value::Scalar(ScalarValue::F32(_)) => "an f32",
        Value::Scalar(ScalarValue::F64(_)) => "an f64",
        Value::Scalar(ScalarValue::Bool(_)) => "a bool",
        Value::Enum { .. } => "an enum value",
        Value::Struct(_) => "a struct",
        Value::Records(_) => "a packed run of structs",
        Value::Blob { .. } => "a codec's own value",
        Value::Array { .. } => "a run",
        Value::Bytes(_) => "bytes",
        Value::Ptr { target: None, .. } => "a null pointer",
        Value::Ptr { .. } => "a pointer",
        Value::Alias(_) => "a pointer at a shared target",
        Value::Member { .. } => "a pointer at a member of its owner",
        Value::Opaque(_) => "opaque bytes",
        Value::UndescribedPtr(_) => "an undescribed pointer",
        Value::NotAnAddress(_) => "eight bytes that are not an address",
    }
}
