//! A described resource as a tree of values: read from an image, or authored
//! from the description alone.
//!
//! ⚠ A scalar edit can go back under the file's own RSC7 header; anything that
//! grows, shrinks or repoints an allocation cannot, because the page table
//! stops being true, so it needs a rebuild.

use std::sync::Arc;

use crate::desc::image::{Image, Unresolvable};
use crate::desc::name::TypeName;
use crate::desc::pack::{Pack, PackField, PackType};
use crate::desc::plan::{Plans, Step};
use crate::desc::record::{is_packable, Record, RecordRun, Shared, IN_PLACE};
use crate::desc::repr::{
    CountSource, FieldRepr, IntWidth, Pointee, Scalar, AT_ARRAY_SIZE, POINTER_SIZE,
};

pub const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum ScalarValue {
    Int(i128),
    F32(f32),
    F64(f64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(ScalarValue),

    Enum {
        enum_name: String,
        value: i64,
    },

    /// A described struct: its own bytes, and a child per pointer field.
    Struct(Record),

    /// A run of structs with no pointer among them, held as one block.
    ///
    /// ⚠ The same run the reader would otherwise give as an
    /// [`Array`](Value::Array) of [`Struct`](Value::Struct)s, and every
    /// consumer takes both: an authored or edited tree may hold either.
    Records(RecordRun),

    /// What a caller's [`BlobReader`] made of an allocation whose regions are
    /// computed, in its own named parts.
    ///
    /// Not a pack type, so not a record: nothing but the codec that made it
    /// reads or writes it, and its part names are the codec's words rather
    /// than the engine's.
    Blob {
        codec: String,
        parts: Vec<(String, Value)>,
    },

    Array {
        elements: Vec<Value>,
        capacity: Option<usize>,
    },

    Bytes(Vec<u8>),

    Ptr {
        source: u64,
        target: Option<Box<Value>>,
    },

    /// A pointer at a target some **earlier** pointer in this tree already
    /// owns, named by the address they share.
    ///
    /// Two pointers at one allocation is a fact about the file: **all 21 `.ybn`
    /// files in the corpus** point `phBoundComposite`'s `last_matrices` and
    /// `current_matrices` at the same run, and a rebuild without this wrote
    /// `num_bounds * 64` bytes the file does not contain and shifted every later
    /// allocation. Which pointers share is a property of the instance, not of the
    /// layout, so the description cannot say it. Deduplicating equal subtrees
    /// instead would collapse them into one allocation and silently discard an
    /// edit to the second; breaking the sharing means replacing the alias with a
    /// [`Value::Ptr`].
    ///
    /// ⚠ Reproducing what the file says includes reproducing aliasing the game
    /// cannot load — a vertex buffer reachable from two LOD levels. Refusing it is
    /// the fixup validator's job on the save path; a writer that un-aliases
    /// invents.
    Alias(u64),

    /// A pointer at a member of the record holding it, named by the member's
    /// path in that record's plan and, for one element of an embedded run,
    /// its index. No address: a rebuild places the owner and points here.
    Member {
        field: String,
        index: Option<usize>,
    },

    Opaque(Vec<u8>),

    UndescribedPtr(u64),

    /// Eight bytes in a pointer slot that are not an address, carried verbatim
    /// with no fixup.
    NotAnAddress(u64),
}

impl Value {
    pub fn int(value: impl Into<i128>) -> Value {
        Value::Scalar(ScalarValue::Int(value.into()))
    }

    pub fn f32(value: f32) -> Value {
        Value::Scalar(ScalarValue::F32(value))
    }

    pub fn f64(value: f64) -> Value {
        Value::Scalar(ScalarValue::F64(value))
    }

    pub fn boolean(value: bool) -> Value {
        Value::Scalar(ScalarValue::Bool(value))
    }

    pub fn ptr_to(target: Value) -> Value {
        Value::Ptr {
            source: 0,
            target: Some(Box::new(target)),
        }
    }

    pub fn null_ptr() -> Value {
        Value::Ptr {
            source: 0,
            target: None,
        }
    }

    pub fn record(&self) -> Option<&Record> {
        match self {
            Value::Struct(record) => Some(record),
            _ => None,
        }
    }

    pub fn record_mut(&mut self) -> Option<&mut Record> {
        match self {
            Value::Struct(record) => Some(record),
            _ => None,
        }
    }

    pub fn records(&self) -> Option<&RecordRun> {
        match self {
            Value::Records(run) => Some(run),
            _ => None,
        }
    }

    pub fn records_mut(&mut self) -> Option<&mut RecordRun> {
        match self {
            Value::Records(run) => Some(run),
            _ => None,
        }
    }

    /// How many elements a run holds, packed or not.
    pub fn run_len(&self) -> Option<usize> {
        match self {
            Value::Array { elements, .. } => Some(elements.len()),
            Value::Records(run) => Some(run.len()),
            _ => None,
        }
    }

    /// Struct `index` of a run, packed or not.
    pub fn struct_at(&self, index: usize) -> Option<crate::desc::record::RecordRef<'_>> {
        match self {
            Value::Array { elements, .. } => elements.get(index)?.record().map(Record::as_ref),
            Value::Records(run) => run.get(index),
            _ => None,
        }
    }

    /// A named part of a codec's [`Value::Blob`].
    pub fn part(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Blob { parts, .. } => parts.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn part_mut(&mut self, name: &str) -> Option<&mut Value> {
        match self {
            Value::Blob { parts, .. } => parts.iter_mut().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Any field of a struct by path, owned; see [`Record::value`].
    pub fn get(&self, path: &str) -> Option<Value> {
        self.record()?.value(path)
    }

    /// A pointer field's target, or a pointer table's or trailing run's value, by path.
    pub fn child(&self, path: &str) -> Option<&Value> {
        self.record()?.child(path)
    }

    pub fn child_mut(&mut self, path: &str) -> Option<&mut Value> {
        self.record_mut()?.child_mut(path)
    }

    pub fn target(&self) -> Option<&Value> {
        match self {
            Value::Ptr {
                target: Some(target),
                ..
            } => Some(target.as_ref()),
            _ => None,
        }
    }

    pub fn target_mut(&mut self) -> Option<&mut Value> {
        match self {
            Value::Ptr {
                target: Some(target),
                ..
            } => Some(target.as_mut()),
            _ => None,
        }
    }

    /// A run's elements, to be written; `None` for a [packed](Value::Records) one.
    ///
    /// ⚠ Adding or removing one is a **structural** edit: the count the layout
    /// reads it by lives in a sibling field, and the description says which.
    /// Nothing here updates it, and [`write::build`](fn@crate::write::build)
    /// refuses a run whose count field disagrees rather than writing a file that
    /// reads back short.
    pub fn elements_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array { elements, .. } => Some(elements),
            _ => None,
        }
    }

    pub fn elements(&self) -> Option<&[Value]> {
        match self {
            Value::Array { elements, .. } => Some(elements),
            _ => None,
        }
    }

    pub fn type_name(&self) -> Option<&str> {
        self.record().map(Record::type_name)
    }

    pub fn as_int(&self) -> Option<i128> {
        match self {
            Value::Scalar(ScalarValue::Int(v)) => Some(*v),
            Value::Scalar(ScalarValue::Bool(b)) => Some(i128::from(*b)),
            Value::Enum { value, .. } => Some(i128::from(*value)),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Scalar(ScalarValue::F32(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) | Value::Opaque(b) => Some(b),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    NoType {
        type_name: String,
    },
    NoCountField {
        field: String,
        count_field: String,
    },
    Truncated {
        offset: usize,
        type_name: String,
    },
    BadPointer {
        field: String,
        reason: Unresolvable,
    },
    TooDeep {
        type_name: String,
    },
    NoTagField {
        type_name: String,
        base: String,
        field: String,
    },

    BlobNotAPointer {
        type_name: String,
        field: String,
        repr: &'static str,
    },

    NotAMember {
        owner: String,
        field: String,
        member: String,
        offset: Option<usize>,
    },
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoType { type_name } => {
                write!(f, "the pack describes no type named `{type_name}`")
            }
            Self::NoCountField { field, count_field } => write!(
                f,
                "`{field}` takes its length from `{count_field}`, which is not a \
                 readable field of the same type"
            ),
            Self::Truncated { offset, type_name } => write!(
                f,
                "reading `{type_name}` ran past the end of the resource at {offset:#x}"
            ),
            Self::BadPointer { field, reason } => {
                write!(f, "`{field}` does not lead anywhere: {reason}")
            }
            Self::TooDeep { type_name } => write!(
                f,
                "reading `{type_name}` went more than {MAX_DEPTH} levels deep, which \
                 is a cycle rather than a layout"
            ),
            Self::NoTagField {
                type_name,
                base,
                field,
            } => write!(
                f,
                "`{type_name}` is a variant of `{base}`, whose tag is `{field}` — and \
                 nothing embedded in `{type_name}` carries that field, so an authored \
                 one cannot say which variant it is"
            ),
            Self::BlobNotAPointer {
                type_name,
                field,
                repr,
            } => write!(
                f,
                "a blob codec is registered on `{type_name}::{field}`, which is {repr} \
                 rather than a plain pointer — a codec takes over one pointer and has \
                 nowhere to state a count or a capacity"
            ),
            Self::NotAMember {
                owner,
                field,
                member,
                offset: Some(offset),
            } => write!(
                f,
                "`{field}` points {offset:#x} bytes into its `{owner}`, where no `{member}` \
                 member starts"
            ),
            Self::NotAMember {
                owner,
                field,
                member,
                offset: None,
            } => write!(
                f,
                "`{field}` points at a `{member}` member of its `{owner}` and lands outside it"
            ),
        }
    }
}

impl std::error::Error for ValueError {}

type Result<T> = std::result::Result<T, ValueError>;

pub fn read(pack: &Pack, type_name: TypeName, image: Image<'_>, at: usize) -> Result<Value> {
    read_with(pack, type_name, image, at, &ElementTypes::new())
}

/// Element types a caller supplies for pointer tables whose element the
/// description does not name.
///
/// Keyed two ways, because a file reaches a table two ways: by the field
/// holding it ([`at`](Self::at)), and by the pointer field the read arrived
/// through ([`reached_through`](Self::reached_through)), which wins.
///
/// ⚠ A site key alone matches every table of that type in the tree: answering
/// `("PgDictionary", "entries") -> GtaDrawable` for a `.ydd` also reads every
/// entry's embedded texture dictionary as drawables, which parses into
/// plausible numbers (a `bounds_sphere.center.z` of 8.77e9 against 8.59e9,
/// measured). A pointer-reached table the caller has not named falls
/// through to the site answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementTypes(BySite<String>);

// Keyed type first, then field, so a lookup borrows rather than allocating:
// the reader asks once per pointer field of every struct.
type ByField<T> = std::collections::BTreeMap<String, std::collections::BTreeMap<String, T>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BySite<T> {
    by_site: ByField<T>,
    by_arrival: ByField<T>,
}

impl<T> Default for BySite<T> {
    fn default() -> Self {
        Self {
            by_site: Default::default(),
            by_arrival: Default::default(),
        }
    }
}

impl<T> BySite<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn at_site(&mut self, type_name: &str, field: &str, answer: T) {
        self.by_site
            .entry(type_name.to_string())
            .or_default()
            .insert(field.to_string(), answer);
    }

    pub fn at_arrival(&mut self, type_name: &str, field: &str, answer: T) {
        self.by_arrival
            .entry(type_name.to_string())
            .or_default()
            .insert(field.to_string(), answer);
    }

    pub fn of(&self, type_name: &str, field: &str) -> Option<&T> {
        self.by_site.get(type_name)?.get(field)
    }

    pub fn behind(&self, type_name: &str, field: &str) -> Option<&T> {
        self.by_arrival.get(type_name)?.get(field)
    }

    pub fn resolve(&self, arrival: Option<(&str, &str)>, site: (&str, &str)) -> Option<&T> {
        arrival
            .and_then(|(ty, field)| self.behind(ty, field))
            .or_else(|| self.of(site.0, site.1))
    }

    pub fn is_empty(&self) -> bool {
        self.by_site.is_empty() && self.by_arrival.is_empty()
    }

    pub fn sites(&self) -> impl Iterator<Item = (&str, &str, &T)> {
        flat(&self.by_site)
    }

    pub fn arrivals(&self) -> impl Iterator<Item = (&str, &str, &T)> {
        flat(&self.by_arrival)
    }
}

fn flat<T>(map: &ByField<T>) -> impl Iterator<Item = (&str, &str, &T)> {
    map.iter().flat_map(|(ty, fields)| {
        fields
            .iter()
            .map(move |(field, answer)| (ty.as_str(), field.as_str(), answer))
    })
}

impl ElementTypes {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn at(mut self, type_name: &str, field: &str, element: &str) -> Self {
        self.0.at_site(type_name, field, element.to_string());
        self
    }

    #[must_use]
    pub fn reached_through(mut self, type_name: &str, field: &str, element: &str) -> Self {
        self.0.at_arrival(type_name, field, element.to_string());
        self
    }

    pub fn of(&self, type_name: &str, field: &str) -> Option<&str> {
        self.0.of(type_name, field).map(String::as_str)
    }

    pub fn behind(&self, type_name: &str, field: &str) -> Option<&str> {
        self.0.behind(type_name, field).map(String::as_str)
    }

    pub fn resolve(&self, arrival: Option<(&str, &str)>, site: (&str, &str)) -> Option<&str> {
        self.0.resolve(arrival, site).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn sites(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.0
            .sites()
            .map(|(ty, field, element)| (ty, field, element.as_str()))
    }

    pub fn arrivals(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.0
            .arrivals()
            .map(|(ty, field, element)| (ty, field, element.as_str()))
    }
}

/// Who walks an allocation whose regions are **computed** rather than located,
/// keyed like [`ElementTypes`].
///
/// A `grcInstanceData`'s `entries` pointer leads to one allocation holding a
/// table of entries, a run of float payloads and a run of name hashes; over 353
/// shaders of the Legacy corpus the float run begins at **20 distinct offsets**
/// and the hash run at **28**, so there is no offset for a description to state.
///
/// ⚠ **A codec is for an allocation whose regions are computed, never for one
/// whose regions are merely undescribed** — that is a pack which has not caught
/// up. The test: *is there a constant to write down?* [`sites`](Self::sites)
/// and [`arrivals`](Self::arrivals) list every allocation the interpreter cannot
/// handle alone.
///
/// A codec is handed its owner's position, not its value: the owner's record is
/// assembled only after its children are read. The write half is
/// `or_io::write::BlobWriters`.
#[derive(Clone, Default)]
pub struct BlobCodecs(BySite<std::sync::Arc<dyn BlobReader>>);

impl std::fmt::Debug for BlobCodecs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        write!(f, "BlobCodecs({})", sites.join(", "))
    }
}

impl BlobCodecs {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn at(
        mut self,
        type_name: &str,
        field: &str,
        codec: std::sync::Arc<dyn BlobReader>,
    ) -> Self {
        self.0.at_site(type_name, field, codec);
        self
    }

    #[must_use]
    pub fn reached_through(
        mut self,
        type_name: &str,
        field: &str,
        codec: std::sync::Arc<dyn BlobReader>,
    ) -> Self {
        self.0.at_arrival(type_name, field, codec);
        self
    }

    pub fn resolve(
        &self,
        arrival: Option<(&str, &str)>,
        site: (&str, &str),
    ) -> Option<&std::sync::Arc<dyn BlobReader>> {
        self.0.resolve(arrival, site)
    }

    /// The codec keyed to what arrives through `type_name::field`, if any.
    pub fn behind(&self, type_name: &str, field: &str) -> Option<&std::sync::Arc<dyn BlobReader>> {
        self.0.behind(type_name, field)
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
}

pub trait BlobReader: Send + Sync {
    fn read(&self, blob: &dyn BlobSource) -> Result<Value>;
}

pub trait BlobSource {
    fn pack(&self) -> &Pack;

    fn image(&self) -> Image<'_>;

    fn owner(&self) -> (&PackType, usize);

    fn blob(&self) -> (u64, usize);

    fn follow(&self, type_name: &str, address: u64) -> Result<Value>;
}

pub fn read_with(
    pack: &Pack,
    type_name: TypeName,
    image: Image<'_>,
    at: usize,
    elements: &ElementTypes,
) -> Result<Value> {
    read_using(pack, type_name, image, at, elements, &BlobCodecs::new())
}

pub fn read_using(
    pack: &Pack,
    type_name: TypeName,
    image: Image<'_>,
    at: usize,
    elements: &ElementTypes,
    blobs: &BlobCodecs,
) -> Result<Value> {
    // A codec takes over one pointer; one registered on anything else is a
    // caller's mistake, refused before the read starts.
    let plans = pack.plans();
    for (type_name, field) in blobs.sites().chain(blobs.arrivals()) {
        if let Some(described) = plans
            .get(type_name)
            .ok()
            .and_then(|plan| plan.pack_type().field_by_name(field))
        {
            if !matches!(described.repr, FieldRepr::Ptr(_)) {
                return Err(ValueError::BlobNotAPointer {
                    type_name: type_name.to_string(),
                    field: field.to_string(),
                    repr: repr_name(&described.repr),
                });
            }
        }
    }
    let reader = Reader {
        pack,
        plans: plans.clone(),
        image,
        elements,
        blobs,
        owned: std::cell::RefCell::new(crate::desc::plan::FastSet::with_capacity_and_hasher(
            image.virtual_size() / 128,
            Default::default(),
        )),
        shared: Default::default(),
        copied: Default::default(),
    };
    // The root owns its own address, so a pointer back at it — every clip's
    // `dictionary` — is an alias rather than a second copy of the root.
    reader.claim(root_address(at));
    reader.struct_at(type_name.as_str(), at, 0, Tag::Resolve, None)
}

/// The address a root read at `at` stands at: what a pointer back at it holds.
pub fn root_address(at: usize) -> u64 {
    crate::constants::FIXED_VIRTUAL_BASE + at as u64
}

type Arrival<'a> = Option<(&'a str, &'a str)>;

/// The record a field belongs to, which is where a member pointer lands.
#[derive(Debug, Clone, Copy)]
struct OwnerAt {
    index: u32,
    at: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    Resolve,
    AsDeclared,
}

struct Reader<'p, 'd> {
    pack: &'p Pack,
    plans: Arc<Plans>,
    image: Image<'d>,

    elements: &'p ElementTypes,

    blobs: &'p BlobCodecs,

    owned: std::cell::RefCell<crate::desc::plan::FastSet<u64>>,

    shared: std::cell::OnceCell<Arc<[u8]>>,
    copied: std::cell::Cell<usize>,
}

/// How many records a read copies before it copies the image once and shares it.
///
/// ⚠ Both extremes lose a row. Sharing from the first record made whole-file
/// drawable reads 2–4% slower, since the whole half is copied however few
/// records ask for it; never sharing made `.ypt` reads 5–15% slower, since
/// each of its 97,000 records past `IN_PLACE` is then an allocation. A
/// Legacy `.yft` holds about 150 such records, a `.ypt` about 24,000.
/// Measured with `what_a_whole_file_record_read_costs_against_the_compiled_reader`.
const SHARE_AFTER: usize = 1024;

impl Reader<'_, '_> {
    /// `len` bytes at `at` of the image's system-memory half, shared by
    /// every record this read takes from it.
    ///
    /// `None` for the first [`SHARE_AFTER`] asked, which the caller copies.
    fn shared(&self, at: usize, len: usize) -> Option<Shared> {
        if self.shared.get().is_none() {
            let copied = self.copied.get() + 1;
            self.copied.set(copied);
            if copied < SHARE_AFTER {
                return None;
            }
        }
        let source = self.shared.get_or_init(|| {
            let bytes = self.image.bytes();
            Arc::from(&bytes[..self.image.virtual_size().min(bytes.len())])
        });
        Shared::new(source, at, len)
    }

    fn ty(&self, name: &str) -> Result<&PackType> {
        self.index(name).map(|i| self.plans.at(i).pack_type())
    }

    fn index(&self, name: &str) -> Result<u32> {
        self.plans.index_of(name).ok_or_else(|| ValueError::NoType {
            type_name: name.to_string(),
        })
    }

    /// The NUL-terminated string at `at`, NUL included, as the bytes a rebuild writes.
    fn c_string(&self, field: &PackField, at: usize) -> Result<Value> {
        let text = self
            .image
            .c_str_at(at)
            .map_err(|reason| ValueError::BadPointer {
                field: field.name.clone(),
                reason,
            })?;
        let mut raw = Vec::with_capacity(text.len() + 1);
        match std::str::from_utf8(text) {
            Ok(_) => raw.extend_from_slice(text),
            Err(_) => raw.extend_from_slice(String::from_utf8_lossy(text).as_bytes()),
        }
        raw.push(0);
        Ok(Value::Bytes(raw))
    }

    fn claim(&self, address: u64) -> bool {
        self.owned.borrow_mut().insert(address)
    }

    /// Give an address back, for a target that turned out to be inexpressible
    /// or empty.
    ///
    /// Those exits are reached *after* the claim, because a claim has to
    /// precede the recursion it guards. Left claimed, a later pointer at the
    /// address would alias an owner that was never written, and a rebuild would
    /// refuse the file.
    fn release(&self, address: u64) {
        self.owned.borrow_mut().remove(&address);
    }

    fn struct_at(
        &self,
        type_name: &str,
        at: usize,
        depth: usize,
        tag: Tag,
        arrival: Arrival<'_>,
    ) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep {
                type_name: type_name.to_string(),
            });
        }

        let index = self.index(type_name)?;
        self.struct_at_index(index, at, depth, tag, arrival)
    }

    fn struct_at_index(
        &self,
        mut index: u32,
        at: usize,
        depth: usize,
        tag: Tag,
        arrival: Arrival<'_>,
    ) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep {
                type_name: self.plans.at(index).name().to_string(),
            });
        }
        if tag == Tag::Resolve {
            let ty = self.plans.at(index).pack_type();
            if let Some(selector) = &ty.selector {
                if let Some(concrete) = self.concrete_of(ty, selector, at) {
                    index = self.index(concrete)?;
                }
            }
        }
        let plan = self.plans.at(index);
        let ty = plan.pack_type();

        let shared = (ty.size > IN_PLACE)
            .then(|| self.shared(at, ty.size))
            .flatten();
        let bytes = match shared {
            Some(shared) => Ok(shared),
            None => Err(self.header(ty, at)?),
        };
        let mut children = Vec::with_capacity(plan.child_count());
        if plan.child_count() > 0 || (!self.blobs.is_empty() && arrival.is_some()) {
            let owner = OwnerAt { index, at };
            self.children_of(index, at, 0, depth, arrival, owner, &mut children)?;
        }
        let plans = self.plans.clone();
        Ok(Value::Struct(match bytes {
            Ok(shared) => Record::assemble_shared(plans, index, shared, children),
            Err(copied) => Record::assemble(plans, index, &copied, children),
        }))
    }

    /// A struct's own bytes, which is every scalar it and its embedded structs hold.
    ///
    /// ⚠ A type whose tail no field describes may end past the image when it
    /// is the last thing in it; the described part has to be there, and the
    /// rest reads as zero.
    fn header(&self, ty: &PackType, at: usize) -> Result<std::borrow::Cow<'_, [u8]>> {
        let all = self.image.bytes();
        if let Some(end) = at.checked_add(ty.size) {
            if let Some(bytes) = all.get(at..end) {
                return Ok(std::borrow::Cow::Borrowed(bytes));
            }
        }
        let described = ty
            .fields
            .iter()
            .filter_map(|f| {
                let width = f.repr.inline_size().or_else(|| match &f.repr {
                    FieldRepr::Inline { type_name } => self.ty(type_name).ok().map(|t| t.size),
                    FieldRepr::InlineArray { type_name, count } => {
                        self.ty(type_name).ok().map(|t| t.size * count)
                    }
                    _ => None,
                })?;
                Some(f.offset + width)
            })
            .max()
            .unwrap_or(0);
        match all.get(at..) {
            Some(rest) if rest.len() >= described => {
                let mut bytes = rest.to_vec();
                bytes.resize(ty.size, 0);
                Ok(std::borrow::Cow::Owned(bytes))
            }
            _ => Err(ValueError::Truncated {
                offset: at.saturating_add(described),
                type_name: ty.name.clone(),
            }),
        }
    }

    /// Read every pointer field of `ty` at `at`, and of every struct embedded in it.
    ///
    /// Children are keyed by their offset from the start of the record, `base`
    /// being where this embedded struct sits in it.
    #[allow(clippy::too_many_arguments)]
    fn children_of(
        &self,
        index: u32,
        at: usize,
        base: usize,
        depth: usize,
        arrival: Arrival<'_>,
        owner: OwnerAt,
        out: &mut Vec<(u32, Value)>,
    ) -> Result<()> {
        let plan = self.plans.at(index);
        let ty = plan.pack_type();
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep {
                type_name: ty.name.clone(),
            });
        }
        // ⚠ **Two passes, in the order a rebuild allocates rather than the order
        // the type declares.** Pointer fields first, then everything else — the
        // split `FieldRepr::is_indirect` names.
        //
        // This is only about which pointer *claims* a shared target, and it has
        // to match the writer or [`Value::Alias`] is a promise nothing keeps: an
        // alias says "somebody earlier owns these bytes", and if the reader's
        // "earlier" and the writer's "earlier" disagree, the writer meets the
        // alias before the allocation it names and can only refuse.
        //
        // Measured on `.ypt`: a `PtxEffectRule` declares an embedded struct
        // before a pointer field, two keyframes below them share one entries
        // array, and reading in declaration order made the *embedded* one the
        // owner while the writer allocated the *pointer's* first. 88,307 rules
        // failed to build with `DanglingAlias`.
        //
        // An arrival-keyed codec answers for every field of the struct it
        // reaches, so the first one that is not a pointer is the refusal the
        // site-keyed check up front cannot make.
        //
        // The plan's steps are those two passes with the scalars left out.
        let arrival_codec = !self.blobs.is_empty()
            && arrival.is_some_and(|(owner, field)| self.blobs.behind(owner, field).is_some());
        if arrival_codec {
            for indirect in [true, false] {
                for field in &ty.fields {
                    if field.repr.is_indirect() != indirect {
                        continue;
                    }
                    if !indirect && !field.repr.is_trailing() {
                        return Err(ValueError::BlobNotAPointer {
                            type_name: ty.name.clone(),
                            field: field.name.clone(),
                            repr: repr_name(&field.repr),
                        });
                    }
                    let key = base + field.offset;
                    out.push((
                        key as u32,
                        self.field(ty, field, at, depth, arrival, owner, None)?,
                    ));
                }
            }
            return Ok(());
        }
        for step in plan.steps() {
            match *step {
                Step::Child { field, target } => {
                    let field = &ty.fields[field as usize];
                    let key = base + field.offset;
                    out.push((
                        key as u32,
                        self.field(ty, field, at, depth, arrival, owner, target)?,
                    ));
                }
                Step::Embed {
                    field,
                    inner,
                    count,
                } => {
                    let field = &ty.fields[field as usize];
                    let inner = match inner {
                        Some(inner) => inner,
                        None => self.index(embedded_type(field))?,
                    };
                    let stride = self.plans.at(inner).size();
                    let offset = at.saturating_add(field.offset);
                    let key = base + field.offset;
                    for i in 0..count as usize {
                        let step = i * stride;
                        self.children_of(
                            inner,
                            offset + step,
                            key + step,
                            depth + 1,
                            arrival,
                            owner,
                            out,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn member(
        &self,
        owner: OwnerAt,
        field: &PackField,
        member: &str,
        address: u64,
    ) -> Result<Value> {
        let offset = match self.image.resolve(address) {
            Ok(offset) => offset,
            Err(Unresolvable::Null) => return Ok(Value::null_ptr()),
            Err(Unresolvable::NotAnAddress { .. }) => return Ok(Value::NotAnAddress(address)),
            Err(reason) => {
                return Err(ValueError::BadPointer {
                    field: field.name.clone(),
                    reason,
                })
            }
        };
        let refused = |offset| ValueError::NotAMember {
            owner: self.plans.at(owner.index).name().to_string(),
            field: field.name.clone(),
            member: member.to_string(),
            offset,
        };
        let rel = offset.checked_sub(owner.at).ok_or_else(|| refused(None))?;
        match self.plans.member_at(owner.index, member, rel) {
            Some((field, index)) => Ok(Value::Member { field, index }),
            None if rel < self.plans.at(owner.index).size() => Err(refused(Some(rel))),
            None => Err(refused(None)),
        }
    }

    fn concrete_of<'a>(
        &self,
        ty: &'a PackType,
        selector: &'a crate::desc::pack::PackSelector,
        at: usize,
    ) -> Option<&'a str> {
        let tag = selector.read_tag(ty, self.image.bytes(), at)?;
        selector.type_for(tag)
    }

    #[allow(clippy::too_many_arguments)]
    fn field(
        &self,
        ty: &PackType,
        field: &PackField,
        at: usize,
        depth: usize,
        arrival: Arrival<'_>,
        owner: OwnerAt,
        pointee_plan: Option<u32>,
    ) -> Result<Value> {
        let offset = at.saturating_add(field.offset);

        let truncated = || ValueError::Truncated {
            offset,
            type_name: ty.name.clone(),
        };

        let codec = if self.blobs.is_empty() {
            None
        } else {
            self.blobs
                .resolve(arrival, (ty.name.as_str(), field.name.as_str()))
        };
        if codec.is_some() && !matches!(field.repr, FieldRepr::Ptr(_)) {
            return Err(ValueError::BlobNotAPointer {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                repr: repr_name(&field.repr),
            });
        }

        match &field.repr {
            FieldRepr::Scalar(_)
            | FieldRepr::ScalarArray { .. }
            | FieldRepr::Enum { .. }
            | FieldRepr::Inline { .. }
            | FieldRepr::InlineArray { .. }
            | FieldRepr::Padding { .. }
            | FieldRepr::Opaque { .. } => {
                unreachable!("a record holds these as bytes; `children_of` never asks")
            }

            FieldRepr::Ptr(pointee) => match codec {
                Some(codec) => self.blob(ty, field, codec.as_ref(), at, offset, depth),
                None => self.follow(
                    ty,
                    field,
                    pointee,
                    at,
                    offset,
                    None,
                    depth,
                    arrival,
                    owner,
                    pointee_plan,
                ),
            },

            FieldRepr::AtArray(pointee) => {
                let count = self
                    .image
                    .u16_at(offset + POINTER_SIZE)
                    .map_err(|_| truncated())? as usize;
                let capacity = self
                    .image
                    .u16_at(offset + POINTER_SIZE + 2)
                    .map_err(|_| truncated())? as usize;
                let _ = AT_ARRAY_SIZE;

                self.follow(
                    ty,
                    field,
                    pointee,
                    at,
                    offset,
                    Some(Inline { count, capacity }),
                    depth,
                    arrival,
                    owner,
                    pointee_plan,
                )
            }

            FieldRepr::PtrSlots { pointee, count } => {
                let mut elements = Vec::with_capacity(*count);
                for i in 0..*count {
                    elements.push(self.follow(
                        ty,
                        field,
                        pointee,
                        at,
                        offset + i * POINTER_SIZE,
                        None,
                        depth,
                        arrival,
                        owner,
                        pointee_plan,
                    )?);
                }
                Ok(Value::Array {
                    elements,
                    capacity: None,
                })
            }

            FieldRepr::Trailing {
                type_name,
                count,
                element_size,
            } => {
                let Some(len) = self.length(ty, field, count, None, at)? else {
                    return Ok(Value::Bytes(Vec::new()));
                };
                let here: Arrival<'_> = Some((ty.name.as_str(), field.name.as_str()));
                self.run_at(
                    field,
                    type_name.as_deref(),
                    *element_size,
                    offset,
                    len,
                    depth,
                    here,
                    pointee_plan,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_at(
        &self,
        field: &PackField,
        type_name: Option<&str>,
        element_size: Option<usize>,
        offset: usize,
        len: usize,
        depth: usize,
        arrival: Arrival<'_>,
        element_plan: Option<u32>,
    ) -> Result<Value> {
        match type_name {
            Some(name) => {
                let index = match element_plan {
                    Some(index) => index,
                    None => self.index(name)?,
                };
                self.struct_run(index, offset, len, None, depth, arrival)
            }
            None => {
                let stride = element_size.unwrap_or(1);
                Ok(Value::Bytes(
                    self.image
                        .slice_at(offset, len * stride)
                        .map_err(|reason| ValueError::BadPointer {
                            field: field.name.clone(),
                            reason,
                        })?
                        .to_vec(),
                ))
            }
        }
    }

    /// `len` structs of type `index` back to back from `offset`: one
    /// [`RecordRun`] where the type is packable, a record each otherwise.
    ///
    /// ⚠ An arrival-keyed codec answers for every field of what it reaches, so
    /// a run behind one is read a record at a time and meets that refusal; and
    /// a run whose last element the image does not wholly hold is too, so it
    /// gets the one reading [`header`](Self::header) gives a short tail.
    fn struct_run(
        &self,
        index: u32,
        offset: usize,
        len: usize,
        capacity: Option<usize>,
        depth: usize,
        arrival: Arrival<'_>,
    ) -> Result<Value> {
        let plan = self.plans.at(index);
        let stride = plan.size();
        let behind_codec = !self.blobs.is_empty()
            && arrival.is_some_and(|(owner, field)| self.blobs.behind(owner, field).is_some());
        if is_packable(plan) && !behind_codec && depth < MAX_DEPTH {
            if let Some(n) = len.checked_mul(stride) {
                if n > IN_PLACE {
                    if let Some(shared) = self.shared(offset, n) {
                        return Ok(Value::Records(RecordRun::assemble_shared(
                            self.plans.clone(),
                            index,
                            shared,
                            capacity,
                        )));
                    }
                }
                if let Some(block) = offset
                    .checked_add(n)
                    .and_then(|end| self.image.bytes().get(offset..end))
                {
                    return Ok(Value::Records(RecordRun::assemble(
                        self.plans.clone(),
                        index,
                        block,
                        capacity,
                    )));
                }
            }
        }
        let mut elements = self.elements(offset, len, stride);
        for i in 0..len {
            elements.push(self.struct_at_index(
                index,
                offset + i * stride,
                depth + 1,
                Tag::AsDeclared,
                arrival,
            )?);
        }
        Ok(Value::Array { elements, capacity })
    }

    fn blob<'a>(
        &'a self,
        ty: &'a PackType,
        field: &'a PackField,
        codec: &dyn BlobReader,
        struct_at: usize,
        slot: usize,
        depth: usize,
    ) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep {
                type_name: ty.name.clone(),
            });
        }

        let bad = |reason| ValueError::BadPointer {
            field: field.name.clone(),
            reason,
        };
        let address = self.image.pointer_at(slot).map_err(bad)?;
        let blob_at = match self.image.resolve(address) {
            Ok(at) => at,
            Err(Unresolvable::Null) => {
                return Ok(Value::Ptr {
                    source: 0,
                    target: None,
                })
            }
            Err(Unresolvable::NotAnAddress { .. }) => return Ok(Value::NotAnAddress(address)),
            Err(reason) => return Err(bad(reason)),
        };
        if !self.claim(address) {
            return Ok(Value::Alias(address));
        }

        let site = Site {
            reader: self,
            owner: ty,
            owner_at: struct_at,
            field: field.name.as_str(),
            address,
            blob_at,
            depth,
            here: Some((ty.name.as_str(), field.name.as_str())),
        };
        Ok(Value::Ptr {
            source: address,
            target: Some(Box::new(codec.read(&site)?)),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn follow(
        &self,
        ty: &PackType,
        field: &PackField,
        pointee: &Pointee,
        struct_at: usize,
        slot: usize,
        inline: Option<Inline>,
        depth: usize,
        arrival: Arrival<'_>,
        owner: OwnerAt,
        pointee_plan: Option<u32>,
    ) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep {
                type_name: ty.name.clone(),
            });
        }

        let here: Arrival<'_> = Some((ty.name.as_str(), field.name.as_str()));
        let plan_of = |name: &str| match pointee_plan {
            Some(index) => Ok(index),
            None => self.index(name),
        };

        let address = self
            .image
            .pointer_at(slot)
            .map_err(|reason| ValueError::BadPointer {
                field: field.name.clone(),
                reason,
            })?;

        match pointee {
            Pointee::Opaque => return Ok(Value::UndescribedPtr(address)),
            // A member is inside an allocation somebody else owns, so it is
            // never claimed.
            Pointee::Member { type_name } => {
                return self.member(owner, field, type_name, address);
            }
            _ => {}
        }

        let target = match self.image.resolve(address) {
            Ok(offset) => offset,
            Err(Unresolvable::Null) => {
                return Ok(Value::Ptr {
                    source: 0,
                    target: None,
                })
            }
            Err(Unresolvable::NotAnAddress { .. }) => return Ok(Value::NotAnAddress(address)),
            Err(reason) => {
                return Err(ValueError::BadPointer {
                    field: field.name.clone(),
                    reason,
                })
            }
        };

        if !self.claim(address) {
            return Ok(Value::Alias(address));
        }

        let value = match pointee {
            Pointee::Opaque | Pointee::Member { .. } => unreachable!("handled above"),

            Pointee::Members { type_name, count } => {
                let Some(len) = self.length(ty, field, count, inline, struct_at)? else {
                    self.release(address);
                    return Ok(Value::UndescribedPtr(address));
                };
                if len == 0 {
                    self.release(address);
                }
                let mut elements = self.elements(target, len, POINTER_SIZE);
                for i in 0..len {
                    let entry =
                        self.image
                            .pointer_at(target + i * POINTER_SIZE)
                            .map_err(|reason| ValueError::BadPointer {
                                field: field.name.clone(),
                                reason,
                            })?;
                    elements.push(self.member(owner, field, type_name, entry)?);
                }
                Value::Array {
                    elements,
                    capacity: inline.map(|i| i.capacity),
                }
            }

            Pointee::One { type_name } => {
                self.struct_at_index(plan_of(type_name)?, target, depth + 1, Tag::Resolve, here)?
            }

            Pointee::Str => self.c_string(field, target)?,

            Pointee::Strings { count } => {
                let Some(len) = self.length(ty, field, count, inline, struct_at)? else {
                    self.release(address);
                    return Ok(Value::UndescribedPtr(address));
                };
                if len == 0 {
                    self.release(address);
                }
                let mut elements = self.elements(target, len, POINTER_SIZE);
                for i in 0..len {
                    let entry =
                        self.image
                            .pointer_at(target + i * POINTER_SIZE)
                            .map_err(|reason| ValueError::BadPointer {
                                field: field.name.clone(),
                                reason,
                            })?;
                    elements.push(match self.image.resolve(entry) {
                        Err(Unresolvable::Null) => Value::Ptr {
                            source: 0,
                            target: None,
                        },
                        Err(Unresolvable::NotAnAddress { .. }) => Value::NotAnAddress(entry),
                        Err(reason) => {
                            return Err(ValueError::BadPointer {
                                field: field.name.clone(),
                                reason,
                            })
                        }
                        Ok(_) if !self.claim(entry) => Value::Alias(entry),
                        Ok(at) => Value::Ptr {
                            source: entry,
                            target: Some(Box::new(self.c_string(field, at)?)),
                        },
                    });
                }
                Value::Array {
                    elements,
                    capacity: inline.map(|i| i.capacity),
                }
            }

            Pointee::Bytes { length } => {
                let room = self.image.bytes().len().saturating_sub(target);
                let Some(len) = self.length_within(ty, field, length, inline, struct_at, room)?
                else {
                    self.release(address);
                    return Ok(Value::UndescribedPtr(address));
                };
                if len == 0 {
                    self.release(address);
                }
                Value::Bytes(
                    self.image
                        .slice_at(target, len)
                        .map_err(|reason| ValueError::BadPointer {
                            field: field.name.clone(),
                            reason,
                        })?
                        .to_vec(),
                )
            }

            Pointee::ManyPtr { type_name, count } => {
                let Some(len) = self.length(ty, field, count, inline, struct_at)? else {
                    self.release(address);
                    return Ok(Value::UndescribedPtr(address));
                };
                if len == 0 {
                    self.release(address);
                }

                let type_name: Option<&str> = type_name.as_deref().or_else(|| {
                    self.elements
                        .resolve(arrival, (ty.name.as_str(), field.name.as_str()))
                });
                let element_plan = type_name.map(plan_of);

                let mut elements = self.elements(target, len, POINTER_SIZE);
                for i in 0..len {
                    let slot = target + i * POINTER_SIZE;
                    let address =
                        self.image
                            .pointer_at(slot)
                            .map_err(|reason| ValueError::BadPointer {
                                field: field.name.clone(),
                                reason,
                            })?;

                    let element = match (type_name, self.image.resolve(address)) {
                        (_, Err(Unresolvable::Null)) => Value::Ptr {
                            source: 0,
                            target: None,
                        },
                        (_, Err(Unresolvable::NotAnAddress { .. })) => Value::NotAnAddress(address),
                        (_, Err(reason)) => {
                            return Err(ValueError::BadPointer {
                                field: field.name.clone(),
                                reason,
                            })
                        }
                        (Some(_), Ok(_)) if !self.claim(address) => Value::Alias(address),
                        (Some(_), Ok(at)) => Value::Ptr {
                            source: address,
                            target: Some(Box::new(self.struct_at_index(
                                element_plan.clone().expect("named with the type")?,
                                at,
                                depth + 1,
                                Tag::Resolve,
                                here,
                            )?)),
                        },
                        (None, Ok(_)) => Value::UndescribedPtr(address),
                    };
                    elements.push(element);
                }

                Value::Array {
                    elements,
                    capacity: inline.map(|i| i.capacity),
                }
            }

            Pointee::Many {
                type_name,
                count,
                element_size,
            } => {
                let Some(len) = self.length(ty, field, count, inline, struct_at)? else {
                    self.release(address);
                    return Ok(Value::UndescribedPtr(address));
                };
                if len == 0 {
                    self.release(address);
                }

                match type_name {
                    Some(name) => {
                        let index = plan_of(name)?;
                        self.struct_run(
                            index,
                            target,
                            len,
                            inline.map(|i| i.capacity),
                            depth,
                            here,
                        )?
                    }
                    // An empty Bytes states no capacity, so the allocation the file sized is kept as an Array.
                    None if len == 0 && inline.is_some_and(|i| i.capacity > 0) => Value::Array {
                        elements: Vec::new(),
                        capacity: inline.map(|i| i.capacity),
                    },
                    None => {
                        let stride = element_size.unwrap_or(1);
                        Value::Bytes(
                            self.image
                                .slice_at(target, len * stride)
                                .map_err(|reason| ValueError::BadPointer {
                                    field: field.name.clone(),
                                    reason,
                                })?
                                .to_vec(),
                        )
                    }
                }
            }
        };

        Ok(Value::Ptr {
            source: address,
            target: Some(Box::new(value)),
        })
    }

    fn length(
        &self,
        ty: &PackType,
        field: &PackField,
        source: &CountSource,
        inline: Option<Inline>,
        struct_at: usize,
    ) -> Result<Option<usize>> {
        self.length_within(ty, field, source, inline, struct_at, usize::MAX)
    }

    /// [`Reader::length`] for a run with `room` bytes after its pointer
    /// ([`CountSource::length_within`]).
    fn length_within(
        &self,
        ty: &PackType,
        field: &PackField,
        source: &CountSource,
        inline: Option<Inline>,
        struct_at: usize,
        room: usize,
    ) -> Result<Option<usize>> {
        match source {
            CountSource::Fixed(n) => Ok(Some(*n)),
            CountSource::Unknown => Ok(None),
            CountSource::Inline => Ok(inline.map(|i| i.count)),
            _ => {
                let read = |name: &str| -> Result<i128> {
                    let missing = || ValueError::NoCountField {
                        field: field.name.clone(),
                        count_field: name.to_string(),
                    };
                    let sibling = ty.field_by_name(name).ok_or_else(missing)?;
                    let width = int_width(&sibling.repr).ok_or_else(missing)?;
                    width
                        .read(self.image.bytes(), struct_at + sibling.offset)
                        .ok_or_else(missing)
                };
                let names = source.reads_fields();
                let raws = names.iter().map(|n| read(n)).collect::<Result<Vec<_>>>()?;
                let length = if raws.is_empty() {
                    None
                } else {
                    source.length_within(&raws, room)
                };
                let joiner = if matches!(source, CountSource::Sum { .. }) {
                    " + "
                } else {
                    " * "
                };
                length.map(Some).ok_or_else(|| ValueError::NoCountField {
                    field: field.name.clone(),
                    count_field: names.join(joiner),
                })
            }
        }
    }

    /// Room for `len` elements `stride` bytes apart from `at`, capped at what the image holds.
    ///
    /// ⚠ `len` is file data: allocating what a malformed count asks for aborts
    /// the process (measured at 100 GiB), where the loop filling this fails
    /// with an error at the image's end.
    fn elements(&self, at: usize, len: usize, stride: usize) -> Vec<Value> {
        let room = self.image.bytes().len().saturating_sub(at);
        Vec::with_capacity(len.min(room / stride.max(1) + 1))
    }
}

struct Site<'a, 'p, 'd> {
    reader: &'a Reader<'p, 'd>,
    owner: &'a PackType,
    owner_at: usize,
    field: &'a str,
    address: u64,
    blob_at: usize,
    depth: usize,
    here: Arrival<'a>,
}

impl BlobSource for Site<'_, '_, '_> {
    fn pack(&self) -> &Pack {
        self.reader.pack
    }

    fn image(&self) -> Image<'_> {
        self.reader.image
    }

    fn owner(&self) -> (&PackType, usize) {
        (self.owner, self.owner_at)
    }

    fn blob(&self) -> (u64, usize) {
        (self.address, self.blob_at)
    }

    fn follow(&self, type_name: &str, address: u64) -> Result<Value> {
        let bad = |reason| ValueError::BadPointer {
            field: self.field.to_string(),
            reason,
        };
        let at = match self.reader.image.resolve(address) {
            Ok(at) => at,
            Err(Unresolvable::Null) => {
                return Ok(Value::Ptr {
                    source: 0,
                    target: None,
                })
            }
            Err(Unresolvable::NotAnAddress { .. }) => return Ok(Value::NotAnAddress(address)),
            Err(reason) => return Err(bad(reason)),
        };
        if !self.reader.claim(address) {
            return Ok(Value::Alias(address));
        }
        let value =
            self.reader
                .struct_at(type_name, at, self.depth + 1, Tag::Resolve, self.here)?;
        Ok(Value::Ptr {
            source: address,
            target: Some(Box::new(value)),
        })
    }
}

/// How many elements a count field's raw integer stands for.
///
/// The reader and `write::build` must read a count the same way, or an edited
/// run whose count nobody updated is written short. A negative count is
/// clamped rather than read as an enormous unsigned length, and a masked count
/// reads only its bits: reading the whole word asks for sixteen million
/// elements on any file that sets a flag, and reads correctly on every file
/// that does not.
///
/// ⚠ **This is the single-field form**: it answers `0` for
/// [`CountSource::Product`], [`Inline`](CountSource::Inline) and
/// [`Unknown`](CountSource::Unknown), and a [`Fixed`](CountSource::Fixed)
/// source's own number whatever `raw` is. Callers that may see any variant use
/// [`CountSource::length_from`] with [`CountSource::reads_fields`].
pub fn elements_from(source: &CountSource, raw: i128) -> usize {
    source.length_from(&[raw]).unwrap_or(0)
}

/// Author an instance of a described type, from the description alone.
///
/// Every field holds the zero its description implies: a scalar is `0`, a
/// pointer is null, a run is empty, and a padding or opaque region is the
/// declared number of zero bytes.
///
/// - ⚠ **A zero vtable pointer is fatal**: the game crashes loading a resource
///   whose `pgBase` carries one, and the pack cannot say so because the value
///   is not a fact about the layout. Whoever authors a root sets it.
/// - **A count field and its run are set separately**, and the build refuses
///   them if they disagree. Filling a run means writing its count too.
///
/// Where the type is one a base's variant table names, the tag is written into
/// whichever embedded base carries it, so `empty(pack, "PhBoundBox")` already
/// says it is a box. Where several tags select one layout — six of
/// `ptxShaderVar`'s eight — the **lowest** is written.
///
/// ⚠ **Naming a base rather than a variant is refused at build time, not
/// here**: a blank `PhBound`'s zero tag selects `PhBoundSphere`.
pub fn empty(pack: &Pack, type_name: &str) -> Result<Value> {
    Author {
        pack,
        plans: pack.plans(),
    }
    .struct_of(type_name, 0)
}

struct Author<'p> {
    pack: &'p Pack,
    plans: &'p Arc<Plans>,
}

impl Author<'_> {
    fn ty(&self, name: &str) -> Result<&PackType> {
        self.plans
            .get(name)
            .map(|plan| plan.pack_type())
            .map_err(|_| ValueError::NoType {
                type_name: name.to_string(),
            })
    }

    fn struct_of(&self, type_name: &str, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep {
                type_name: type_name.to_string(),
            });
        }
        let index = self
            .plans
            .index_of(type_name)
            .ok_or_else(|| ValueError::NoType {
                type_name: type_name.to_string(),
            })?;
        let ty = self.plans.at(index).pack_type();

        let mut bytes = vec![0u8; ty.size];
        let mut children = Vec::new();
        self.children_of(ty, 0, depth, &mut children)?;
        self.stamp_tag(&mut bytes, ty)?;
        Ok(Value::Struct(Record::assemble(
            self.plans.clone(),
            index,
            &bytes,
            children,
        )))
    }

    fn children_of(
        &self,
        ty: &PackType,
        base: usize,
        depth: usize,
        out: &mut Vec<(u32, Value)>,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep {
                type_name: ty.name.clone(),
            });
        }
        for field in &ty.fields {
            let key = base + field.offset;
            let child = match &field.repr {
                FieldRepr::Ptr(_) | FieldRepr::AtArray(_) => Value::null_ptr(),
                FieldRepr::PtrSlots { count, .. } => Value::Array {
                    elements: vec![Value::null_ptr(); *count],
                    capacity: None,
                },
                FieldRepr::Trailing {
                    type_name: None, ..
                } => Value::Bytes(Vec::new()),
                FieldRepr::Trailing { .. } => Value::Array {
                    elements: Vec::new(),
                    capacity: None,
                },
                FieldRepr::Inline { type_name } => {
                    self.children_of(self.ty(type_name)?, key, depth + 1, out)?;
                    continue;
                }
                FieldRepr::InlineArray { type_name, count } => {
                    let inner = self.ty(type_name)?;
                    for i in 0..*count {
                        self.children_of(inner, key + i * inner.size, depth + 1, out)?;
                    }
                    continue;
                }
                _ => continue,
            };
            out.push((key as u32, child));
        }
        Ok(())
    }

    fn stamp_tag(&self, bytes: &mut [u8], ty: &PackType) -> Result<()> {
        for base in &self.pack.types {
            let Some(selector) = &base.selector else {
                continue;
            };
            let Some(tag) = selector
                .variants
                .iter()
                .filter(|(_, name)| name == &ty.name)
                .map(|(value, _)| *value)
                .min()
            else {
                continue;
            };
            let written = self.tag_offset(ty, base, selector, 0, 0).and_then(|at| {
                let field = base.field_by_name(&selector.tag)?;
                write_tag(bytes, at, &field.repr, selector, tag).then_some(())
            });
            if written.is_none() {
                return Err(ValueError::NoTagField {
                    type_name: ty.name.clone(),
                    base: base.name.clone(),
                    field: selector.tag.clone(),
                });
            }
        }
        Ok(())
    }

    /// Where `base`'s tag field sits in `ty`: in `ty` itself, or in a struct it embeds.
    fn tag_offset(
        &self,
        ty: &PackType,
        base: &PackType,
        selector: &crate::desc::pack::PackSelector,
        at: usize,
        depth: usize,
    ) -> Option<usize> {
        if ty.name == base.name {
            return base.field_by_name(&selector.tag).map(|f| at + f.offset);
        }
        if depth > MAX_DEPTH {
            return None;
        }
        ty.fields.iter().find_map(|field| match &field.repr {
            FieldRepr::Inline { type_name } => {
                let inner = self.ty(type_name).ok()?;
                self.tag_offset(inner, base, selector, at + field.offset, depth + 1)
            }
            _ => None,
        })
    }
}

fn write_tag(
    bytes: &mut [u8],
    at: usize,
    repr: &FieldRepr,
    selector: &crate::desc::pack::PackSelector,
    tag: i64,
) -> bool {
    let width = match repr {
        FieldRepr::Scalar(Scalar::Int(w)) | FieldRepr::Enum { width: w, .. } => w.size(),
        FieldRepr::Scalar(scalar @ (Scalar::F32 | Scalar::F64)) if selector.tag_bits.is_some() => {
            scalar.size()
        }
        _ => return false,
    };
    let Some(slot) = bytes.get_mut(at..at + width) else {
        return false;
    };
    let mut buf = [0u8; 8];
    buf[..width].copy_from_slice(slot);
    let raw = u64::from_le_bytes(buf);

    let patched = match &selector.tag_bits {
        None => tag as u64,
        Some(range) => {
            let mask = if range.width >= 64 {
                u64::MAX
            } else {
                (1u64 << range.width) - 1
            };
            let shift = range.low.min(63);
            (raw & !(mask << shift)) | (((tag as u64) & mask) << shift)
        }
    };
    slot.copy_from_slice(&patched.to_le_bytes()[..width]);
    true
}

fn embedded_type(field: &PackField) -> &str {
    match &field.repr {
        FieldRepr::Inline { type_name } | FieldRepr::InlineArray { type_name, .. } => type_name,
        _ => unreachable!("a plan embeds only through an embedded field"),
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

#[derive(Debug, Clone, Copy)]
struct Inline {
    count: usize,
    capacity: usize,
}

fn int_width(repr: &FieldRepr) -> Option<IntWidth> {
    match repr {
        FieldRepr::Scalar(Scalar::Int(w)) | FieldRepr::Enum { width: w, .. } => Some(*w),
        FieldRepr::Scalar(Scalar::Bool) => Some(IntWidth::U8),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desc::name::FieldName;
    use crate::desc::pack::{PackEnum, PackField, PackSelector, PackVariant};
    use crate::desc::view::View;

    const VIRTUAL_SIZE: usize = 0x200;
    const ROOT: usize = 0x100;

    fn field(name: &str, offset: usize, repr: FieldRepr) -> PackField {
        PackField {
            name: name.into(),
            offset,
            repr,
            space: None,
            bits: Vec::new(),
            note: None,
        }
    }

    fn int(width: IntWidth) -> FieldRepr {
        FieldRepr::Scalar(Scalar::Int(width))
    }

    fn addr(offset: usize) -> u64 {
        0x5000_0000u64 | offset as u64
    }

    struct Bytes(Vec<u8>);

    impl Bytes {
        fn new(len: usize) -> Self {
            Self(vec![0u8; len])
        }

        fn put(&mut self, at: usize, data: &[u8]) -> &mut Self {
            self.0[at..at + data.len()].copy_from_slice(data);
            self
        }

        fn ptr(&mut self, at: usize, target: usize) -> &mut Self {
            self.put(at, &addr(target).to_le_bytes())
        }

        fn image(&self) -> Image<'_> {
            Image::new(&self.0, VIRTUAL_SIZE)
        }
    }

    fn menagerie() -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Root".into(),
            size: 0x40,
            fields: vec![
                field("flags", 0x00, int(IntWidth::U32)),
                field("ratio", 0x04, FieldRepr::Scalar(Scalar::F32)),
                field(
                    "kind",
                    0x08,
                    FieldRepr::Enum {
                        type_name: "Kind".into(),
                        width: IntWidth::U8,
                    },
                ),
                field("gap", 0x09, FieldRepr::Padding { size: 3 }),
                field("count", 0x0C, int(IntWidth::U16)),
                field(
                    "items",
                    0x10,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: Some("Item".into()),
                        count: CountSource::Field("count".into()),
                        element_size: None,
                    }),
                ),
                field(
                    "blob",
                    0x18,
                    FieldRepr::Ptr(Pointee::Bytes {
                        length: CountSource::Fixed(6),
                    }),
                ),
                field("name", 0x20, FieldRepr::Ptr(Pointee::Str)),
                field(
                    "child",
                    0x28,
                    FieldRepr::Ptr(Pointee::One {
                        type_name: "Item".into(),
                    }),
                ),
                field(
                    "nested",
                    0x30,
                    FieldRepr::Inline {
                        type_name: "Item".into(),
                    },
                ),
                field("mystery", 0x38, FieldRepr::Opaque { size: 8 }),
            ],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Item".into(),
            size: 8,
            fields: vec![
                field("id", 0x00, int(IntWidth::U32)),
                field("weight", 0x04, FieldRepr::Scalar(Scalar::F32)),
            ],
            selector: None,
        });
        pack.enums.push(PackEnum {
            name: "Kind".into(),
            width: IntWidth::U8,
            variants: vec![PackVariant {
                name: "Geometry".into(),
                value: 4,
                note: None,
            }],
        });
        pack
    }

    fn menagerie_bytes() -> Bytes {
        let mut b = Bytes::new(0x400);
        b.put(ROOT, &0xDEAD_BEEFu32.to_le_bytes());
        b.put(ROOT + 0x04, &1.5f32.to_le_bytes());
        b.put(ROOT + 0x08, &[4]);
        b.put(ROOT + 0x09, &[0xA1, 0xA2, 0xA3]);
        b.put(ROOT + 0x0C, &2u16.to_le_bytes());
        b.ptr(ROOT + 0x10, 0x200);
        b.ptr(ROOT + 0x18, 0x220);
        b.ptr(ROOT + 0x20, 0x230);
        b.ptr(ROOT + 0x28, 0x240);
        b.put(ROOT + 0x30, &7u32.to_le_bytes());
        b.put(ROOT + 0x34, &0.25f32.to_le_bytes());
        b.put(
            ROOT + 0x38,
            &[0xC0, 0xFF, 0xEE, 0x01, 0x02, 0x03, 0x04, 0x05],
        );

        b.put(0x200, &10u32.to_le_bytes());
        b.put(0x204, &(-1.0f32).to_le_bytes());
        b.put(0x208, &20u32.to_le_bytes());
        b.put(0x20C, &2.0f32.to_le_bytes());

        b.put(0x220, &[1, 2, 3, 4, 5, 6]);
        b.put(0x230, b"lod0\0");
        b.put(0x240, &99u32.to_le_bytes());
        b.put(0x244, &0.5f32.to_le_bytes());
        b
    }

    #[test]
    fn every_scalar_reads_the_same_through_the_value_tree_and_through_a_view() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        let view = View::of(&pack, TypeName("Root")).expect("bind");
        let cursor = bytes.image().at(ROOT);

        let flags = view.field::<u32>(FieldName("flags")).expect("bind flags");
        assert_eq!(
            value.get("flags").as_ref().and_then(Value::as_int),
            cursor.get(flags).map(i128::from)
        );

        let ratio = view.field::<f32>(FieldName("ratio")).expect("bind ratio");
        assert_eq!(
            value.get("ratio").as_ref().and_then(Value::as_f32),
            cursor.get(ratio)
        );

        let count = view.field::<u16>(FieldName("count")).expect("bind count");
        assert_eq!(
            value.get("count").as_ref().and_then(Value::as_int),
            cursor.get(count).map(i128::from)
        );

        let kind = view.field::<u8>(FieldName("kind")).expect("bind kind");
        assert_eq!(
            value.get("kind").as_ref().and_then(Value::as_int),
            cursor.get(kind).map(i128::from)
        );

        let nested_id = view
            .inner(FieldName("nested"))
            .expect("descend")
            .field::<u32>(FieldName("id"))
            .expect("bind id");
        assert_eq!(
            value.get("nested.id").as_ref().and_then(Value::as_int),
            cursor.get(nested_id).map(i128::from)
        );
    }

    #[test]
    fn an_undescribed_region_comes_back_byte_for_byte() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        let mystery = value.get("mystery").expect("the field is there");
        assert_eq!(
            mystery.as_bytes(),
            Some(&[0xC0, 0xFF, 0xEE, 0x01, 0x02, 0x03, 0x04, 0x05][..])
        );

        let gap = value.get("gap").expect("the field is there");
        assert_eq!(gap.as_bytes(), Some(&[0xA1, 0xA2, 0xA3][..]));
    }

    #[test]
    fn a_record_holds_its_structs_own_bytes() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");
        let record = value.record().expect("a struct");
        assert_eq!(record.bytes(), &bytes.0[ROOT..ROOT + 0x40]);
        assert_eq!(record.pack_id(), "probe/gen8");
        let offsets: Vec<u32> = record.children().iter().map(|(at, _)| *at).collect();
        assert_eq!(
            offsets,
            vec![0x10, 0x18, 0x20, 0x28],
            "one child per pointer field"
        );
    }

    #[test]
    fn an_array_takes_its_length_from_the_sibling_the_pack_names() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        let Some(Value::Ptr {
            target: Some(items),
            ..
        }) = value.child("items")
        else {
            panic!(
                "items should be a populated pointer, got {:?}",
                value.child("items")
            );
        };
        let Value::Records(run) = items.as_ref() else {
            panic!("an `Item` holds no pointer, so a run of them is one packed block");
        };

        assert_eq!(run.len(), 2, "the count field says two");
        assert_eq!(run.get(0).and_then(|e| e.int("id")), Some(10));
        assert_eq!(run.get(1).and_then(|e| e.int("id")), Some(20));
        assert_eq!(
            run.get(1).and_then(|e| e.scalar("weight")),
            Some(ScalarValue::F32(2.0))
        );
        assert_eq!(
            run.bytes(),
            &bytes.0[0x200..0x210],
            "the elements' own bytes"
        );
    }

    #[test]
    fn a_run_of_structs_that_hold_a_pointer_stays_a_record_each() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Root".into(),
            size: 0x10,
            fields: vec![
                field("count", 0x00, int(IntWidth::U16)),
                field(
                    "items",
                    0x08,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: Some("Linked".into()),
                        count: CountSource::Field("count".into()),
                        element_size: None,
                    }),
                ),
            ],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Linked".into(),
            size: 0x10,
            fields: vec![
                field("id", 0x00, int(IntWidth::U32)),
                field(
                    "next",
                    0x08,
                    FieldRepr::Ptr(Pointee::One {
                        type_name: "Linked".into(),
                    }),
                ),
            ],
            selector: None,
        });
        let mut bytes = Bytes::new(0x400);
        bytes.put(ROOT, &2u16.to_le_bytes());
        bytes.ptr(ROOT + 8, 0x200);
        bytes.put(0x200, &10u32.to_le_bytes());
        bytes.put(0x210, &20u32.to_le_bytes());
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        let items = value.child("items").and_then(Value::target).expect("a run");
        let Value::Array { elements, .. } = items else {
            panic!("an element with a pointer has a child to hold, got {items:?}");
        };
        assert_eq!(elements.len(), 2);
        assert_eq!(items.struct_at(1).and_then(|e| e.int("id")), Some(20));
    }

    #[test]
    fn a_packed_element_is_edited_in_its_block_and_nowhere_else() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let mut value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");
        let run = value
            .child_mut("items")
            .and_then(Value::target_mut)
            .and_then(Value::records_mut)
            .expect("a packed run");

        run.set_scalar(1, "id", ScalarValue::Int(99))
            .expect("a scalar");
        assert_eq!(run.get(0).and_then(|e| e.int("id")), Some(10));
        assert_eq!(run.get(1).and_then(|e| e.int("id")), Some(99));
        assert_eq!(&run.bytes()[8..12], &99u32.to_le_bytes());

        let taken = run.remove(0).expect("an element");
        assert_eq!(taken.int("id"), Some(10));
        assert_eq!(run.len(), 1);
        run.insert(1, &taken).expect("the run's own type");
        assert_eq!(
            (0..run.len())
                .map(|i| run.get(i).and_then(|e| e.int("id")))
                .collect::<Vec<_>>(),
            vec![Some(99), Some(10)]
        );
        assert!(run.get(2).is_none());

        let root = Record::zeroed(pack.plans(), "Root").unwrap();
        assert!(matches!(
            run.push(&root),
            Err(crate::desc::record::RecordError::WrongElement { .. })
        ));
    }

    #[test]
    fn a_string_keeps_its_terminator_so_the_bytes_go_back_as_they_came() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        let Some(Value::Ptr {
            target: Some(name), ..
        }) = value.child("name")
        else {
            panic!("name should be a populated pointer");
        };
        assert_eq!(name.as_bytes(), Some(&b"lod0\0"[..]));
    }

    #[test]
    fn a_fixed_length_payload_is_carried_whole() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        let Some(Value::Ptr {
            target: Some(blob), ..
        }) = value.child("blob")
        else {
            panic!("blob should be a populated pointer");
        };
        assert_eq!(blob.as_bytes(), Some(&[1u8, 2, 3, 4, 5, 6][..]));
    }

    #[test]
    fn a_null_pointer_is_absence_rather_than_an_error() {
        let pack = menagerie();
        let mut bytes = menagerie_bytes();
        bytes.put(ROOT + 0x28, &[0u8; 8]);

        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");
        assert!(matches!(
            value.child("child"),
            Some(Value::Ptr { target: None, .. })
        ));
    }

    fn at_array_pack() -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Composite".into(),
            size: 0x20,
            fields: vec![field(
                "bounds",
                0x00,
                FieldRepr::AtArray(Pointee::Many {
                    type_name: Some("Item".into()),
                    count: CountSource::Inline,
                    element_size: None,
                }),
            )],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Item".into(),
            size: 8,
            fields: vec![field("id", 0x00, int(IntWidth::U32))],
            selector: None,
        });
        pack
    }

    #[test]
    fn an_at_array_keeps_the_capacity_apart_from_the_count() {
        let pack = at_array_pack();
        let mut bytes = Bytes::new(0x400);
        bytes.ptr(ROOT, 0x200);
        bytes.put(ROOT + 8, &2u16.to_le_bytes());
        bytes.put(ROOT + 10, &6u16.to_le_bytes());
        bytes.put(0x200, &11u32.to_le_bytes());
        bytes.put(0x208, &22u32.to_le_bytes());

        let value = read(&pack, TypeName("Composite"), bytes.image(), ROOT).expect("read");
        let Some(Value::Ptr {
            target: Some(bounds),
            ..
        }) = value.child("bounds")
        else {
            panic!("bounds should be a populated pointer");
        };
        let Value::Records(run) = bounds.as_ref() else {
            panic!("bounds should point at a packed run");
        };

        assert_eq!(run.len(), 2, "two are populated");
        assert_eq!(run.capacity(), Some(6), "six were allocated");
    }

    fn tagged_pack() -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "PhBound".into(),
            size: 0x10,
            fields: vec![field("bound_type", 0x00, int(IntWidth::U8))],
            selector: Some(PackSelector {
                tag: "bound_type".into(),
                tag_bits: None,
                variants: vec![(4, "PhBoundGeometry".into())],
            }),
        });
        pack.types.push(PackType {
            name: "PhBoundGeometry".into(),
            size: 0x20,
            fields: vec![
                field("bound_type", 0x00, int(IntWidth::U8)),
                field("margin", 0x04, FieldRepr::Scalar(Scalar::F32)),
            ],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_tagged_base_reads_as_the_layout_its_own_tag_selects() {
        let pack = tagged_pack();
        let mut bytes = Bytes::new(0x400);
        bytes.put(ROOT, &[4]);
        bytes.put(ROOT + 4, &0.04f32.to_le_bytes());

        let value = read(&pack, TypeName("PhBound"), bytes.image(), ROOT).expect("read");
        assert_eq!(value.type_name(), Some("PhBoundGeometry"));
        assert_eq!(
            value.get("margin").as_ref().and_then(Value::as_f32),
            Some(0.04)
        );
    }

    #[test]
    fn a_tag_nobody_described_reads_as_the_base_rather_than_failing() {
        let pack = tagged_pack();
        let mut bytes = Bytes::new(0x400);
        bytes.put(ROOT, &[9]);

        let value = read(&pack, TypeName("PhBound"), bytes.image(), ROOT).expect("read");
        assert_eq!(value.type_name(), Some("PhBound"));
    }

    #[test]
    fn a_pointer_cycle_terminates_as_a_shared_target() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Loop".into(),
            size: 8,
            fields: vec![field(
                "next",
                0x00,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Loop".into(),
                }),
            )],
            selector: None,
        });

        let mut bytes = Bytes::new(0x400);
        bytes.ptr(ROOT, ROOT);

        let value = read(&pack, TypeName("Loop"), bytes.image(), ROOT).expect("should stop");

        assert_eq!(
            value.child("next"),
            Some(&Value::Alias(root_address(ROOT))),
            "the root owns its own address, so a pointer back at it is an alias rather \
             than a second copy of the root"
        );
    }

    #[test]
    fn an_undescribed_target_keeps_its_address_rather_than_dropping_the_field() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Root".into(),
            size: 8,
            fields: vec![field("thing", 0x00, FieldRepr::Ptr(Pointee::Opaque))],
            selector: None,
        });

        let mut bytes = Bytes::new(0x400);
        bytes.ptr(ROOT, 0x200);

        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");
        assert_eq!(
            value.child("thing"),
            Some(&Value::UndescribedPtr(addr(0x200)))
        );
    }

    #[test]
    fn fields_come_back_in_the_order_the_type_declares_them() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        let record = value.record().expect("a struct");
        let read_order: Vec<&str> = record
            .as_ref()
            .fields()
            .map(|(f, _)| f.name.as_str())
            .collect();
        let declared: Vec<&str> = pack
            .type_by_name("Root")
            .unwrap()
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .collect();

        assert_eq!(read_order, declared);
    }

    #[test]
    fn a_truncated_resource_is_an_error_rather_than_a_panic() {
        let pack = menagerie();
        let short = Bytes::new(ROOT + 0x10);
        let err = read(&pack, TypeName("Root"), short.image(), ROOT).expect_err("should fail");
        assert!(matches!(
            err,
            ValueError::Truncated { .. } | ValueError::BadPointer { .. }
        ));
    }

    #[test]
    fn a_type_the_pack_does_not_describe_names_itself_in_the_error() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let err = read(&pack, TypeName("Nonexistent"), bytes.image(), ROOT).expect_err("no type");
        assert_eq!(
            err,
            ValueError::NoType {
                type_name: "Nonexistent".into()
            }
        );
        assert!(err.to_string().contains("Nonexistent"));
    }

    #[test]
    fn a_blank_is_readable_by_the_same_reader_that_would_have_read_a_file() {
        let pack = menagerie();
        let blank = empty(&pack, "Root").expect("a described type");
        let read_from_bytes = read(&pack, TypeName("Root"), menagerie_bytes().image(), ROOT)
            .expect("the same type, read");

        let shape = |value: &Value| -> (usize, Vec<u32>) {
            let record = value.record().expect("a struct");
            (
                record.bytes().len(),
                record.children().iter().map(|(at, _)| *at).collect(),
            )
        };
        assert_eq!(shape(&blank), shape(&read_from_bytes));
        assert_eq!(blank.type_name(), read_from_bytes.type_name());
        assert!(blank.record().unwrap().bytes().iter().all(|b| *b == 0));
    }

    #[test]
    fn a_blank_of_a_variant_already_says_which_it_is() {
        let mut pack = tagged_pack();
        pack.types[0]
            .selector
            .as_mut()
            .unwrap()
            .variants
            .push((7, "PhBoundBox".into()));
        pack.types.push(PackType {
            name: "PhBoundBox".into(),
            size: 0x20,
            fields: vec![
                field(
                    "header",
                    0x00,
                    FieldRepr::Inline {
                        type_name: "PhBound".into(),
                    },
                ),
                field("half", 0x10, FieldRepr::Scalar(Scalar::F32)),
            ],
            selector: None,
        });
        let blank = empty(&pack, "PhBoundBox").expect("a variant");
        assert_eq!(
            blank
                .get("header.bound_type")
                .as_ref()
                .and_then(Value::as_int),
            Some(7)
        );
    }

    #[test]
    fn a_path_stops_at_a_pointer_rather_than_stepping_through_it() {
        let pack = menagerie();
        let bytes = menagerie_bytes();
        let mut value = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect("read");

        assert!(
            value.get("child.id").is_none(),
            "`child` is a pointer, so `child.id` is not a path"
        );
        let through = value
            .child_mut("child")
            .and_then(Value::target_mut)
            .and_then(Value::record_mut)
            .expect("the pointer leads to a struct");
        through
            .set_scalar("id", ScalarValue::Int(5))
            .expect("id is a scalar");

        value
            .record_mut()
            .unwrap()
            .set_scalar("nested.id", ScalarValue::Int(42))
            .expect("nested.id is a scalar");
        assert_eq!(
            value.get("nested.id").as_ref().and_then(Value::as_int),
            Some(42)
        );
        assert_eq!(
            value
                .child("child")
                .and_then(Value::target)
                .and_then(|t| t.get("id"))
                .as_ref()
                .and_then(Value::as_int),
            Some(5)
        );
    }

    #[test]
    fn a_count_is_read_the_same_way_it_will_be_checked() {
        assert_eq!(elements_from(&CountSource::Field("n".into()), -3), 0);
        assert_eq!(
            elements_from(
                &CountSource::MaskedField {
                    field: "n".into(),
                    mask: 0x00FF_FFFF
                },
                0x8100_0007
            ),
            7
        );

        for source in [
            CountSource::Product {
                count: "n".into(),
                stride: "w".into(),
            },
            CountSource::Inline,
            CountSource::Unknown,
        ] {
            assert_eq!(elements_from(&source, 9), 0, "{source:?}");
        }
        assert_eq!(elements_from(&CountSource::Fixed(5), 9), 5);
    }

    #[test]
    fn a_length_comes_from_every_field_its_source_names_or_from_none() {
        let product = CountSource::Product {
            count: "n".into(),
            stride: "w".into(),
        };
        assert_eq!(product.reads_fields(), vec!["n", "w"]);
        assert_eq!(product.length_from(&[7, 5]), Some(35));

        assert_eq!(product.length_from(&[7]), None);
        assert_eq!(product.length_from(&[7, 5, 3]), None);

        assert_eq!(product.length_from(&[-7, 5]), Some(0));
        assert_eq!(product.length_from(&[7, -5]), Some(0));

        let masked = CountSource::MaskedField {
            field: "n".into(),
            mask: 0x00FF_FFFF,
        };
        assert_eq!(masked.reads_fields(), vec!["n"]);
        assert_eq!(masked.length_from(&[0x8100_0007]), Some(7));
        assert_eq!(masked.length_from(&[1, 2]), None);

        let sum = CountSource::Sum {
            fields: vec!["a".into(), "b".into(), "c".into()],
        };
        assert_eq!(sum.reads_fields(), vec!["a", "b", "c"]);
        assert_eq!(sum.length_from(&[7, 40, 6]), Some(53));
        assert_eq!(sum.length_from(&[7, 40]), None);
        assert_eq!(sum.length_from(&[7, -40, 6]), Some(13));
    }

    #[test]
    fn a_run_whose_length_is_a_product_of_two_siblings() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Buffer".into(),
            size: 0x10,
            fields: vec![
                field("count", 0x00, int(IntWidth::U32)),
                field("stride", 0x04, int(IntWidth::U16)),
                field(
                    "payload",
                    0x08,
                    FieldRepr::Ptr(Pointee::Bytes {
                        length: CountSource::Product {
                            count: "count".into(),
                            stride: "stride".into(),
                        },
                    }),
                ),
            ],
            selector: None,
        });

        let mut bytes = Bytes::new(0x400);
        bytes.put(ROOT, &7u32.to_le_bytes());
        bytes.put(ROOT + 4, &5u16.to_le_bytes());
        bytes.ptr(ROOT + 8, 0x40);
        bytes.put(0x40, &[0xAB; 36]);

        let value = read(&pack, TypeName("Buffer"), bytes.image(), ROOT).expect("read");
        let payload = match value.child("payload") {
            Some(Value::Ptr {
                target: Some(inner),
                ..
            }) => inner.as_bytes().expect("a byte run"),
            other => panic!("expected a pointer to a byte run, got {other:?}"),
        };

        assert_eq!(payload.len(), 35, "7 * 5, and neither factor alone");
        assert!(payload.iter().all(|b| *b == 0xAB));
    }

    #[test]
    fn a_product_missing_one_of_its_two_fields_is_refused() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Buffer".into(),
            size: 0x10,
            fields: vec![
                field("count", 0x00, int(IntWidth::U32)),
                field(
                    "payload",
                    0x08,
                    FieldRepr::Ptr(Pointee::Bytes {
                        length: CountSource::Product {
                            count: "count".into(),
                            stride: "absent".into(),
                        },
                    }),
                ),
            ],
            selector: None,
        });

        let mut bytes = Bytes::new(0x400);
        bytes.put(ROOT, &7u32.to_le_bytes());
        bytes.ptr(ROOT + 8, 0x40);

        let err = read(&pack, TypeName("Buffer"), bytes.image(), ROOT).expect_err("should fail");
        assert_eq!(
            err,
            ValueError::NoCountField {
                field: "payload".into(),
                count_field: "absent".into(),
            }
        );
    }

    #[test]
    fn an_array_citing_a_count_field_that_is_not_there_says_so() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Root".into(),
            size: 0x10,
            fields: vec![field(
                "items",
                0x00,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: None,
                    count: CountSource::Field("absent".into()),
                    element_size: Some(4),
                }),
            )],
            selector: None,
        });

        let mut bytes = Bytes::new(0x400);
        bytes.ptr(ROOT, 0x200);

        let err = read(&pack, TypeName("Root"), bytes.image(), ROOT).expect_err("should fail");
        assert_eq!(
            err,
            ValueError::NoCountField {
                field: "items".into(),
                count_field: "absent".into()
            }
        );
    }
}
