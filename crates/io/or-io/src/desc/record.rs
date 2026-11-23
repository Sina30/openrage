//! A record: one described struct's own bytes at the pack's offsets, the
//! [`Plans`] of the pack it came from, and one child per pointer, pointer table
//! and trailing run — what [`Value::Struct`](crate::desc::Value::Struct) holds.
//!
//! ⚠ An embedded struct is not a child. Its bytes are the record's, and its
//! pointer fields are children keyed by offset from the record's start.

use std::sync::Arc;

use thiserror::Error;

use crate::desc::pack::PackField;
use crate::desc::plan::{Kind, PlanError, PlanScalar, Plans, Rec, RecMut, Slot, TypePlan};
use crate::desc::repr::FieldRepr;
use crate::desc::value::{ScalarValue, Value};
use crate::desc::view::WriteError;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecordError {
    #[error(transparent)]
    Plan(#[from] PlanError),

    #[error("a `{type_name}` is {expected:#x} bytes; {found:#x} were given")]
    WrongSize {
        type_name: String,
        expected: usize,
        found: usize,
    },

    #[error("`{type_name}.{path}` is not a pointer, a pointer table or a trailing run, so it has no child")]
    NotAChild { type_name: String, path: String },

    #[error("`{type_name}.{path}` is not a scalar")]
    NotAScalar { type_name: String, path: String },

    #[error("`{type_name}.{path}`: {error}")]
    Write {
        type_name: String,
        path: String,
        error: WriteError,
    },

    #[error("a `{type_name}` holds pointers, so a run of them is a run of records rather than one block")]
    NotPackable { type_name: String },

    #[error("a run of `{run}` from `{run_pack}` cannot hold a `{record}` from `{record_pack}`")]
    WrongElement {
        run: String,
        run_pack: String,
        record: String,
        record_pack: String,
    },

    #[error("element {index} of a run of {len}")]
    NoElement { index: usize, len: usize },
}

#[derive(Clone)]
pub struct Record {
    plans: Arc<Plans>,
    ty: u32,
    bytes: Bytes,
    /// Sorted by offset.
    children: Vec<(u32, Value)>,
}

/// Up to this many bytes a record holds its struct in place.
///
/// A vertex, a BVH node or a polygon is a run of thousands of small structs,
/// and one allocation each is what made a whole `.ybn` read slower than the
/// compiled reader's one `Vec` per run.
pub(crate) const IN_PLACE: usize = 24;

#[derive(Clone)]
enum Bytes {
    InPlace {
        len: u8,
        data: [u8; IN_PLACE],
    },
    Heap(Box<[u8]>),
    /// The image the reader read the struct from, copied on the first write.
    Shared(Shared),
}

/// A range of an image a read shares among every record it read from it.
///
/// ⚠ An allocation and a copy per record is most of what a whole-file read
/// costs: 97,000 of a `.ypt` corpus's 132,000 records are over
/// [`IN_PLACE`] bytes. One shared copy of the image is one allocation.
#[derive(Clone)]
pub(crate) struct Shared {
    source: Arc<[u8]>,
    start: u32,
    len: u32,
}

impl Shared {
    pub(crate) fn new(source: &Arc<[u8]>, start: usize, len: usize) -> Option<Shared> {
        let end = start.checked_add(len)?;
        (end <= source.len() && end <= u32::MAX as usize).then(|| Shared {
            source: source.clone(),
            start: start as u32,
            len: len as u32,
        })
    }

    #[inline]
    fn get(&self) -> &[u8] {
        &self.source[self.start as usize..(self.start + self.len) as usize]
    }
}

impl Bytes {
    fn from_slice(bytes: &[u8]) -> Bytes {
        if bytes.len() <= IN_PLACE {
            let mut data = [0u8; IN_PLACE];
            data[..bytes.len()].copy_from_slice(bytes);
            Bytes::InPlace {
                len: bytes.len() as u8,
                data,
            }
        } else {
            Bytes::Heap(bytes.into())
        }
    }

    fn from_vec(bytes: Vec<u8>) -> Bytes {
        if bytes.len() <= IN_PLACE {
            Bytes::from_slice(&bytes)
        } else {
            Bytes::Heap(bytes.into_boxed_slice())
        }
    }
}

impl std::ops::Deref for Bytes {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        match self {
            Bytes::InPlace { len, data } => &data[..*len as usize],
            Bytes::Heap(bytes) => bytes,
            Bytes::Shared(shared) => shared.get(),
        }
    }
}

impl std::ops::DerefMut for Bytes {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        if let Bytes::Shared(shared) = self {
            *self = Bytes::from_slice(shared.get());
        }
        match self {
            Bytes::InPlace { len, data } => &mut data[..*len as usize],
            Bytes::Heap(bytes) => bytes,
            Bytes::Shared(_) => unreachable!("copied above"),
        }
    }
}

impl PartialEq for Record {
    fn eq(&self, other: &Self) -> bool {
        self.plans.pack_id() == other.plans.pack_id()
            && self.type_name() == other.type_name()
            && *self.bytes == *other.bytes
            && self.children == other.children
    }
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Record")
            .field("type", &self.type_name())
            .field("pack", &self.plans.pack_id())
            .field("bytes", &self.bytes.len())
            .field("children", &self.children)
            .finish()
    }
}

fn is_child(repr: &FieldRepr) -> bool {
    repr.is_indirect() || repr.is_trailing()
}

impl Record {
    /// A reader's constructor: `bytes` is exactly the plan's size and
    /// `children` is sorted and lands on child fields.
    pub(crate) fn assemble(
        plans: Arc<Plans>,
        ty: u32,
        bytes: &[u8],
        mut children: Vec<(u32, Value)>,
    ) -> Record {
        debug_assert_eq!(bytes.len(), plans.at(ty).size());
        children.sort_by_key(|(at, _)| *at);
        Record {
            plans,
            ty,
            bytes: Bytes::from_slice(bytes),
            children,
        }
    }

    /// [`assemble`](Self::assemble) over a range of the image the read shares.
    pub(crate) fn assemble_shared(
        plans: Arc<Plans>,
        ty: u32,
        bytes: Shared,
        mut children: Vec<(u32, Value)>,
    ) -> Record {
        debug_assert_eq!(bytes.len as usize, plans.at(ty).size());
        children.sort_by_key(|(at, _)| *at);
        Record {
            plans,
            ty,
            bytes: if bytes.len as usize <= IN_PLACE {
                Bytes::from_slice(bytes.get())
            } else {
                Bytes::Shared(bytes)
            },
            children,
        }
    }

    /// A record of `type_name` from its bytes, with no children yet.
    pub fn from_bytes(
        plans: &Arc<Plans>,
        type_name: &str,
        bytes: Vec<u8>,
    ) -> Result<Record, RecordError> {
        let plan = plans.get(type_name)?;
        if bytes.len() != plan.size() {
            return Err(RecordError::WrongSize {
                type_name: type_name.to_string(),
                expected: plan.size(),
                found: bytes.len(),
            });
        }
        Ok(Record {
            plans: plans.clone(),
            ty: plan.index(),
            bytes: Bytes::from_vec(bytes),
            children: Vec::new(),
        })
    }

    /// Replace every child, sorted by offset.
    pub fn set_children(&mut self, mut children: Vec<(u32, Value)>) {
        children.sort_by_key(|(at, _)| *at);
        self.children = children;
    }

    /// Every byte zero and no children: see [`crate::desc::value::empty`] for a blank with both.
    pub fn zeroed(plans: &Arc<Plans>, type_name: &str) -> Result<Record, RecordError> {
        let size = plans.get(type_name)?.size();
        Record::from_bytes(plans, type_name, vec![0; size])
    }

    pub fn plans(&self) -> &Arc<Plans> {
        &self.plans
    }

    pub fn plan(&self) -> &TypePlan {
        self.plans.at(self.ty)
    }

    pub fn type_name(&self) -> &str {
        self.plan().name()
    }

    /// `title/target` of the pack this record was read or authored with.
    pub fn pack_id(&self) -> &str {
        self.plans.pack_id()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The record's bytes with no structural guard: what a patch or a codec writes through.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    #[inline]
    pub fn rec(&self) -> Rec<'_> {
        self.plans
            .at(self.ty)
            .record(&self.bytes, 0)
            .expect("a record's bytes are its plan's size")
    }

    #[inline]
    pub fn rec_mut(&mut self) -> RecMut<'_> {
        self.plans
            .at(self.ty)
            .record_mut(&mut self.bytes, 0)
            .expect("a record's bytes are its plan's size")
    }

    #[inline]
    pub fn get<T: PlanScalar>(&self, slot: Slot<T>) -> Option<T> {
        self.rec().get(slot)
    }

    pub fn set<T: PlanScalar>(&mut self, slot: Slot<T>, value: T) -> Result<(), WriteError> {
        self.rec_mut().set(slot, value)
    }

    pub fn as_ref(&self) -> RecordRef<'_> {
        RecordRef {
            plans: &self.plans,
            ty: self.ty,
            bytes: &self.bytes,
            base: 0,
            children: &self.children,
        }
    }

    pub fn children(&self) -> &[(u32, Value)] {
        &self.children
    }

    pub fn children_mut(&mut self) -> impl Iterator<Item = (u32, &mut Value)> {
        self.children.iter_mut().map(|(at, v)| (*at, v))
    }

    pub fn child_at(&self, offset: usize) -> Option<&Value> {
        let i = self
            .children
            .binary_search_by_key(&(offset as u32), |(at, _)| *at)
            .ok()?;
        Some(&self.children[i].1)
    }

    pub fn child_at_mut(&mut self, offset: usize) -> Option<&mut Value> {
        let i = self
            .children
            .binary_search_by_key(&(offset as u32), |(at, _)| *at)
            .ok()?;
        Some(&mut self.children[i].1)
    }

    pub fn child(&self, path: &str) -> Option<&Value> {
        self.child_at(self.plan().field(path)?.offset)
    }

    pub fn child_mut(&mut self, path: &str) -> Option<&mut Value> {
        let offset = self.plan().field(path)?.offset;
        self.child_at_mut(offset)
    }

    /// Put `value` at a pointer, pointer table or trailing field, returning what was there.
    pub fn set_child(&mut self, path: &str, value: Value) -> Result<Option<Value>, RecordError> {
        let plan = self.plan();
        let field = plan.checked_field(path)?;
        if !is_child(&field.repr) {
            return Err(RecordError::NotAChild {
                type_name: plan.name().to_string(),
                path: path.to_string(),
            });
        }
        let at = field.offset as u32;
        Ok(match self.children.binary_search_by_key(&at, |(o, _)| *o) {
            Ok(i) => Some(std::mem::replace(&mut self.children[i].1, value)),
            Err(i) => {
                self.children.insert(i, (at, value));
                None
            }
        })
    }

    pub fn scalar(&self, path: &str) -> Option<ScalarValue> {
        self.as_ref().scalar(path)
    }

    /// An integer, an enum's value or a flag, widened.
    pub fn int(&self, path: &str) -> Option<i128> {
        self.as_ref().int(path)
    }

    /// Write a scalar by path. Refuses a count, a capacity or a tag.
    pub fn set_scalar(&mut self, path: &str, value: ScalarValue) -> Result<(), RecordError> {
        self.write_scalar(path, value, false)
    }

    /// Write a scalar by path, structural fields included.
    ///
    /// ⚠ For the edit that keeps a run and its count together; a count
    /// written alone is a file that reads back a different length.
    pub fn set_structural(&mut self, path: &str, value: ScalarValue) -> Result<(), RecordError> {
        self.write_scalar(path, value, true)
    }

    fn write_scalar(
        &mut self,
        path: &str,
        value: ScalarValue,
        structural: bool,
    ) -> Result<(), RecordError> {
        let plan = self.plans.at(self.ty);
        let field = plan.checked_field(path)?;
        let wrong = |error| RecordError::Write {
            type_name: plan.name().to_string(),
            path: path.to_string(),
            error,
        };
        let kind = field.kind.ok_or_else(|| RecordError::NotAScalar {
            type_name: plan.name().to_string(),
            path: path.to_string(),
        })?;
        if field.structural && !structural {
            return Err(wrong(WriteError::Structural));
        }
        let range = field.offset..field.offset + kind.size();
        let size = self.bytes.len();
        let out = self.bytes.get_mut(range).ok_or_else(|| {
            wrong(WriteError::OutOfBounds {
                offset: field.offset,
                size,
            })
        })?;
        kind.encode(&value, out).map_err(wrong)
    }

    pub fn inner(&self, path: &str) -> Option<RecordRef<'_>> {
        self.as_ref().inner(path)
    }

    /// Any field by path as an owned [`Value`]: a scalar decoded, an enum
    /// with its name, a region as bytes, a child cloned.
    ///
    /// The slow path, for tests and diagnostics; a consumer reads a [`Slot`].
    pub fn value(&self, path: &str) -> Option<Value> {
        self.as_ref().value(path)
    }
}

/// Whether a run of `plan`'s type is held as one [`RecordRun`] rather than a record per element.
pub fn is_packable(plan: &TypePlan) -> bool {
    plan.child_count() == 0 && plan.size() > 0
}

/// A run of one type whose records have no children, held as one block:
/// element `i` is the plan's size of bytes at `i * size`.
///
/// A vertex, a BVH node, a polygon and a keyframe entry carry no pointer, and
/// a record each — an `Arc`, a child list and a `Value` — is what made a
/// whole `.ybn`, `.yld` and `.ypt` read slower than the compiled readers'
/// one `Vec` per run.
///
/// ⚠ A run is packed or not by its **type**, never by its instance: a type
/// with any pointer, pointer table or trailing run, embedded structs'
/// included, is a [`Value::Array`] of records even where every pointer is
/// null, so an edit that sets one has somewhere to put it.
#[derive(Clone)]
pub struct RecordRun {
    plans: Arc<Plans>,
    ty: u32,
    bytes: RunBytes,
    capacity: Option<usize>,
}

#[derive(Clone)]
enum RunBytes {
    Owned(Vec<u8>),
    /// Copied on the first write, like a record's.
    Shared(Shared),
}

impl RunBytes {
    fn vec_mut(&mut self) -> &mut Vec<u8> {
        if let RunBytes::Shared(shared) = self {
            *self = RunBytes::Owned(shared.get().to_vec());
        }
        match self {
            RunBytes::Owned(bytes) => bytes,
            RunBytes::Shared(_) => unreachable!("copied above"),
        }
    }
}

impl std::ops::Deref for RunBytes {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        match self {
            RunBytes::Owned(bytes) => bytes,
            RunBytes::Shared(shared) => shared.get(),
        }
    }
}

impl std::ops::DerefMut for RunBytes {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        self.vec_mut()
    }
}

impl PartialEq for RecordRun {
    fn eq(&self, other: &Self) -> bool {
        self.plans.pack_id() == other.plans.pack_id()
            && self.type_name() == other.type_name()
            && self.capacity == other.capacity
            && *self.bytes == *other.bytes
    }
}

impl std::fmt::Debug for RecordRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordRun")
            .field("type", &self.type_name())
            .field("pack", &self.plans.pack_id())
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl RecordRun {
    /// A reader's constructor: `bytes` is whole elements of a packable type.
    pub(crate) fn assemble(
        plans: Arc<Plans>,
        ty: u32,
        bytes: &[u8],
        capacity: Option<usize>,
    ) -> RecordRun {
        debug_assert!(is_packable(plans.at(ty)));
        debug_assert_eq!(bytes.len() % plans.at(ty).size(), 0);
        RecordRun {
            plans,
            ty,
            bytes: RunBytes::Owned(bytes.to_vec()),
            capacity,
        }
    }

    /// [`assemble`](Self::assemble) over a range of the image the read shares.
    pub(crate) fn assemble_shared(
        plans: Arc<Plans>,
        ty: u32,
        bytes: Shared,
        capacity: Option<usize>,
    ) -> RecordRun {
        debug_assert!(is_packable(plans.at(ty)));
        debug_assert_eq!(bytes.len as usize % plans.at(ty).size(), 0);
        RecordRun {
            plans,
            ty,
            bytes: RunBytes::Shared(bytes),
            capacity,
        }
    }

    /// An empty run of `type_name`, which must be [packable](is_packable).
    pub fn new(plans: &Arc<Plans>, type_name: &str) -> Result<RecordRun, RecordError> {
        let plan = plans.get(type_name)?;
        if !is_packable(plan) {
            return Err(RecordError::NotPackable {
                type_name: type_name.to_string(),
            });
        }
        Ok(RecordRun {
            plans: plans.clone(),
            ty: plan.index(),
            bytes: RunBytes::Owned(Vec::new()),
            capacity: None,
        })
    }

    /// A run of `type_name` from whole elements' bytes.
    pub fn from_bytes(
        plans: &Arc<Plans>,
        type_name: &str,
        bytes: Vec<u8>,
    ) -> Result<RecordRun, RecordError> {
        let mut run = RecordRun::new(plans, type_name)?;
        if !bytes.len().is_multiple_of(run.stride()) {
            return Err(RecordError::WrongSize {
                type_name: type_name.to_string(),
                expected: bytes.len().next_multiple_of(run.stride()),
                found: bytes.len(),
            });
        }
        run.bytes = RunBytes::Owned(bytes);
        Ok(run)
    }

    pub fn plans(&self) -> &Arc<Plans> {
        &self.plans
    }

    pub fn plan(&self) -> &TypePlan {
        self.plans.at(self.ty)
    }

    pub fn type_name(&self) -> &str {
        self.plan().name()
    }

    pub fn pack_id(&self) -> &str {
        self.plans.pack_id()
    }

    /// Bytes from one element to the next: the plan's size.
    #[inline]
    pub fn stride(&self) -> usize {
        self.plan().size()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.bytes.len() / self.stride()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The elements an `atArray` header allocated, where one says.
    pub fn capacity(&self) -> Option<usize> {
        self.capacity
    }

    pub fn set_capacity(&mut self, capacity: Option<usize>) {
        self.capacity = capacity;
    }

    /// Every element's bytes, back to back.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Every element's bytes with no structural guard: what a patch writes through.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    #[inline]
    fn range(&self, index: usize) -> Option<std::ops::Range<usize>> {
        let stride = self.stride();
        let start = index.checked_mul(stride)?;
        let end = start.checked_add(stride)?;
        (end <= self.bytes.len()).then_some(start..end)
    }

    /// Element `index`, read the way a record is read.
    #[inline]
    pub fn get(&self, index: usize) -> Option<RecordRef<'_>> {
        let range = self.range(index)?;
        Some(RecordRef {
            plans: &self.plans,
            ty: self.ty,
            bytes: &self.bytes[range],
            base: 0,
            children: &[],
        })
    }

    #[inline]
    pub fn rec(&self, index: usize) -> Option<Rec<'_>> {
        self.plan().record(&self.bytes, self.range(index)?.start)
    }

    #[inline]
    pub fn rec_mut(&mut self, index: usize) -> Option<RecMut<'_>> {
        let start = self.range(index)?.start;
        self.plans.at(self.ty).record_mut(&mut self.bytes, start)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = RecordRef<'_>> + '_ {
        (0..self.len()).map(|i| self.get(i).expect("an index below the run's length"))
    }

    /// Element `index` as a record of its own.
    pub fn record(&self, index: usize) -> Option<Record> {
        let range = self.range(index)?;
        Some(Record::assemble(
            self.plans.clone(),
            self.ty,
            &self.bytes[range],
            Vec::new(),
        ))
    }

    /// Every element as a record of its own: the run as a [`Value::Array`] would hold it.
    pub fn records(&self) -> Vec<Record> {
        (0..self.len())
            .map(|i| self.record(i).expect("an index below the run's length"))
            .collect()
    }

    /// Write one element's scalar by path. Refuses a count, a capacity or a tag.
    pub fn set_scalar(
        &mut self,
        index: usize,
        path: &str,
        value: ScalarValue,
    ) -> Result<(), RecordError> {
        let len = self.len();
        let range = self
            .range(index)
            .ok_or(RecordError::NoElement { index, len })?;
        let plan = self.plans.at(self.ty);
        let field = plan.checked_field(path)?;
        let wrong = |error| RecordError::Write {
            type_name: plan.name().to_string(),
            path: path.to_string(),
            error,
        };
        let kind = field.kind.ok_or_else(|| RecordError::NotAScalar {
            type_name: plan.name().to_string(),
            path: path.to_string(),
        })?;
        if field.structural {
            return Err(wrong(WriteError::Structural));
        }
        let element = &mut self.bytes[range];
        let size = element.len();
        let out = element
            .get_mut(field.offset..field.offset + kind.size())
            .ok_or_else(|| {
                wrong(WriteError::OutOfBounds {
                    offset: field.offset,
                    size,
                })
            })?;
        kind.encode(&value, out).map_err(wrong)
    }

    fn check_element(&self, record: &Record) -> Result<(), RecordError> {
        if record.pack_id() != self.pack_id()
            || record.type_name() != self.type_name()
            || !record.children().is_empty()
        {
            return Err(RecordError::WrongElement {
                run: self.type_name().to_string(),
                run_pack: self.pack_id().to_string(),
                record: record.type_name().to_string(),
                record_pack: record.pack_id().to_string(),
            });
        }
        Ok(())
    }

    /// Put `record`'s bytes at `index`, moving every later element up one.
    pub fn insert(&mut self, index: usize, record: &Record) -> Result<(), RecordError> {
        self.check_element(record)?;
        let len = self.len();
        if index > len {
            return Err(RecordError::NoElement { index, len });
        }
        let at = index * self.stride();
        self.bytes
            .vec_mut()
            .splice(at..at, record.bytes().iter().copied());
        Ok(())
    }

    pub fn push(&mut self, record: &Record) -> Result<(), RecordError> {
        self.insert(self.len(), record)
    }

    /// Take element `index` out as a record, moving every later element down one.
    pub fn remove(&mut self, index: usize) -> Result<Record, RecordError> {
        let len = self.len();
        let taken = self
            .record(index)
            .ok_or(RecordError::NoElement { index, len })?;
        let range = self.range(index).expect("checked by `record`");
        self.bytes.vec_mut().drain(range);
        Ok(taken)
    }
}

/// A described struct inside a record: the record itself, or a struct embedded in it.
#[derive(Clone, Copy)]
pub struct RecordRef<'a> {
    plans: &'a Arc<Plans>,
    ty: u32,
    bytes: &'a [u8],
    base: usize,
    children: &'a [(u32, Value)],
}

impl std::fmt::Debug for RecordRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordRef")
            .field("type", &self.type_name())
            .field("base", &self.base)
            .finish()
    }
}

/// One field somewhere in a record, found by walking its description.
#[derive(Debug, Clone)]
pub struct FieldSite<'a> {
    /// From the record, as a plan names it: `inner.x`, `run[2].x`.
    pub path: String,
    /// From the start of the record.
    pub at: usize,
    /// The type that declares the field: the record's own, or one it embeds.
    pub owner: &'a crate::desc::pack::PackType,
    pub field: &'a PackField,
}

impl FieldSite<'_> {
    /// A pointer, pointer table or trailing run: a field the record holds as a child.
    pub fn is_child(&self) -> bool {
        is_child(&self.field.repr)
    }
}

/// One declared field of a struct, as a record holds it.
#[derive(Debug, Clone)]
pub enum FieldValue<'a> {
    Scalar(ScalarValue),
    Enum {
        enum_name: &'a str,
        value: i64,
    },
    Scalars(Vec<ScalarValue>),
    /// Padding or an undescribed region, verbatim.
    Bytes(&'a [u8]),
    Inline(RecordRef<'a>),
    Inlines(Vec<RecordRef<'a>>),
    /// A pointer, a pointer table or a trailing run; `None` where the record has none.
    Child(Option<&'a Value>),
}

impl<'a> RecordRef<'a> {
    pub fn plans(&self) -> &'a Arc<Plans> {
        self.plans
    }

    pub fn plan(&self) -> &'a TypePlan {
        self.plans.at(self.ty)
    }

    pub fn type_name(&self) -> &'a str {
        self.plan().name()
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Where this struct starts in the record that owns its bytes.
    pub fn base(&self) -> usize {
        self.base
    }

    #[inline]
    pub fn rec(&self) -> Rec<'a> {
        self.plan()
            .record(self.bytes, 0)
            .expect("a record region is its plan's size")
    }

    pub fn scalar(&self, path: &str) -> Option<ScalarValue> {
        let field = self.plan().field(path)?;
        self.rec().scalar(field)
    }

    pub fn int(&self, path: &str) -> Option<i128> {
        match self.scalar(path)? {
            ScalarValue::Int(v) => Some(v),
            ScalarValue::Bool(b) => Some(i128::from(b)),
            _ => None,
        }
    }

    pub fn child_at(&self, offset: usize) -> Option<&'a Value> {
        let at = (self.base + offset) as u32;
        let i = self.children.binary_search_by_key(&at, |(o, _)| *o).ok()?;
        Some(&self.children[i].1)
    }

    pub fn child(&self, path: &str) -> Option<&'a Value> {
        self.child_at(self.plan().field(path)?.offset)
    }

    /// An embedded struct, or one element of an embedded run, by path.
    pub fn inner(&self, path: &str) -> Option<RecordRef<'a>> {
        let (type_name, offset) = match self.plan().field(path) {
            Some(field) => match &field.repr {
                FieldRepr::Inline { type_name } => (type_name.as_str(), field.offset),
                _ => return None,
            },
            None => {
                // `run[2]` names an element of a fixed run of embedded structs.
                let (run, rest) = path.rsplit_once('[')?;
                let index: usize = rest.strip_suffix(']')?.parse().ok()?;
                let field = self.plan().field(run)?;
                let FieldRepr::InlineArray { type_name, count } = &field.repr else {
                    return None;
                };
                if index >= *count {
                    return None;
                }
                let size = self.plans.get(type_name).ok()?.size();
                (type_name.as_str(), field.offset + index * size)
            }
        };
        self.embedded(type_name, offset)
    }

    /// The struct of `type_name` embedded at `offset` in this one.
    pub fn embedded(&self, type_name: &str, offset: usize) -> Option<RecordRef<'a>> {
        let plan = self.plans.get(type_name).ok()?;
        Some(RecordRef {
            plans: self.plans,
            ty: plan.index(),
            bytes: self.bytes.get(offset..offset + plan.size())?,
            base: self.base + offset,
            children: self.children,
        })
    }

    /// See [`Record::value`].
    pub fn value(&self, path: &str) -> Option<Value> {
        let field = self.plan().field(path)?;
        let at = field.offset;
        Some(match &field.repr {
            FieldRepr::Scalar(_) => Value::Scalar(self.rec().scalar(field)?),
            FieldRepr::Enum { type_name, width } => Value::Enum {
                enum_name: type_name.clone(),
                value: width.read(self.bytes, at)? as i64,
            },
            FieldRepr::ScalarArray { element, count } => Value::Array {
                elements: (0..*count)
                    .map(|i| {
                        let at = at + i * element.size();
                        Kind::of_scalar(*element)
                            .decode(self.bytes.get(at..at + element.size())?)
                            .map(Value::Scalar)
                    })
                    .collect::<Option<_>>()?,
                capacity: None,
            },
            FieldRepr::Padding { size } | FieldRepr::Opaque { size } => {
                Value::Opaque(self.bytes.get(at..at + size)?.to_vec())
            }
            FieldRepr::Inline { .. } | FieldRepr::InlineArray { .. } => return None,
            FieldRepr::Ptr(_)
            | FieldRepr::AtArray(_)
            | FieldRepr::PtrSlots { .. }
            | FieldRepr::Trailing { .. } => self.child_at(at)?.clone(),
        })
    }

    /// Every field of this struct and of every struct it embeds, depth-first
    /// in declaration order, each embedded struct expanded where it sits.
    ///
    /// An embedded struct is not itself a site; its fields are.
    pub fn walk(&self) -> Vec<FieldSite<'a>> {
        fn go<'a>(
            plans: &'a Plans,
            ty: &'a crate::desc::pack::PackType,
            prefix: &str,
            base: usize,
            depth: usize,
            out: &mut Vec<FieldSite<'a>>,
        ) {
            if depth > crate::desc::value::MAX_DEPTH {
                return;
            }
            for field in &ty.fields {
                let path = format!("{prefix}{}", field.name);
                let at = base + field.offset;
                match &field.repr {
                    FieldRepr::Inline { type_name } => {
                        if let Ok(inner) = plans.get(type_name) {
                            go(
                                plans,
                                inner.pack_type(),
                                &format!("{path}."),
                                at,
                                depth + 1,
                                out,
                            );
                        }
                    }
                    FieldRepr::InlineArray { type_name, count } => {
                        if let Ok(inner) = plans.get(type_name) {
                            for i in 0..*count {
                                go(
                                    plans,
                                    inner.pack_type(),
                                    &format!("{path}[{i}]."),
                                    at + i * inner.size(),
                                    depth + 1,
                                    out,
                                );
                            }
                        }
                    }
                    _ => out.push(FieldSite {
                        path,
                        at,
                        owner: ty,
                        field,
                    }),
                }
            }
        }
        let mut out = Vec::new();
        go(self.plans, self.plan().pack_type(), "", 0, 0, &mut out);
        out
    }

    /// Every field this struct declares, in declaration order.
    pub fn fields(&self) -> impl Iterator<Item = (&'a PackField, FieldValue<'a>)> + 'a {
        let this = *self;
        self.plan()
            .pack_type()
            .fields
            .iter()
            .map(move |field| (field, this.value_of(field)))
    }

    fn value_of(&self, field: &'a PackField) -> FieldValue<'a> {
        let at = field.offset;
        let bytes = self.bytes;
        let scalar = |kind: Kind, at: usize| {
            bytes
                .get(at..at + kind.size())
                .and_then(|b| kind.decode(b))
                .unwrap_or(ScalarValue::Int(0))
        };
        match &field.repr {
            FieldRepr::Scalar(s) => FieldValue::Scalar(scalar(Kind::of_scalar(*s), at)),
            FieldRepr::Enum { type_name, width } => FieldValue::Enum {
                enum_name: type_name,
                value: width.read(bytes, at).unwrap_or(0) as i64,
            },
            FieldRepr::ScalarArray { element, count } => FieldValue::Scalars(
                (0..*count)
                    .map(|i| scalar(Kind::of_scalar(*element), at + i * element.size()))
                    .collect(),
            ),
            FieldRepr::Padding { size } | FieldRepr::Opaque { size } => {
                FieldValue::Bytes(bytes.get(at..at + size).unwrap_or(&[]))
            }
            FieldRepr::Inline { type_name } => match self.embedded(type_name, at) {
                Some(inner) => FieldValue::Inline(inner),
                None => FieldValue::Child(None),
            },
            FieldRepr::InlineArray { type_name, count } => {
                let size = self.plans.get(type_name).map(|p| p.size()).unwrap_or(0);
                FieldValue::Inlines(
                    (0..*count)
                        .filter_map(|i| self.embedded(type_name, at + i * size))
                        .collect(),
                )
            }
            FieldRepr::Ptr(_)
            | FieldRepr::AtArray(_)
            | FieldRepr::PtrSlots { .. }
            | FieldRepr::Trailing { .. } => FieldValue::Child(self.child_at(at)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desc::pack::{Pack, PackType};
    use crate::desc::repr::{CountSource, IntWidth, Pointee, Scalar};

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

    fn pack() -> Pack {
        let mut pack = Pack::new("synthetic", "t1");
        pack.types.push(PackType {
            name: "Leaf".into(),
            size: 0x10,
            fields: vec![
                field("weight", 0, FieldRepr::Scalar(Scalar::F32)),
                field(
                    "next",
                    8,
                    FieldRepr::Ptr(Pointee::One {
                        type_name: "Leaf".into(),
                    }),
                ),
            ],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Root".into(),
            size: 0x28,
            fields: vec![
                field("count", 0, FieldRepr::Scalar(Scalar::Int(IntWidth::U16))),
                field(
                    "leaf",
                    8,
                    FieldRepr::Inline {
                        type_name: "Leaf".into(),
                    },
                ),
                field(
                    "items",
                    0x18,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: None,
                        count: CountSource::Field("count".into()),
                        element_size: Some(1),
                    }),
                ),
                field("flag", 0x20, FieldRepr::Scalar(Scalar::Bool)),
            ],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_slot_bound_on_an_image_reads_the_same_field_of_a_record() {
        let pack = pack();
        let plans = pack.plans();
        let plan = pack.plan("Root").unwrap();
        let weight = plan.slot::<f32>("leaf.weight").unwrap();

        let mut image = vec![0u8; 0x40];
        image[0x10 + 8..0x10 + 12].copy_from_slice(&3.5f32.to_le_bytes());
        let from_image = plan.record(&image, 0x10).unwrap().get(weight);

        let mut record =
            Record::from_bytes(plans, "Root", image[0x10..0x10 + 0x28].to_vec()).unwrap();
        assert_eq!(record.get(weight), from_image);

        record.set(weight, -2.0).unwrap();
        assert_eq!(record.get(weight), Some(-2.0));
        assert_eq!(record.scalar("leaf.weight"), Some(ScalarValue::F32(-2.0)));
    }

    #[test]
    fn a_child_is_keyed_by_its_offset_embedded_or_not() {
        let pack = pack();
        let mut record = Record::zeroed(pack.plans(), "Root").unwrap();
        record.set_child("items", Value::Bytes(vec![1, 2])).unwrap();
        record.set_child("leaf.next", Value::null_ptr()).unwrap();
        assert_eq!(record.children().len(), 2);
        assert_eq!(record.child_at(0x10), Some(&Value::null_ptr()));
        assert_eq!(
            record.inner("leaf").unwrap().child("next"),
            Some(&Value::null_ptr())
        );
        assert!(matches!(
            record.set_child("count", Value::null_ptr()),
            Err(RecordError::NotAChild { .. })
        ));
    }

    #[test]
    fn a_structural_scalar_is_written_only_on_purpose() {
        let pack = pack();
        let mut record = Record::zeroed(pack.plans(), "Root").unwrap();
        assert!(matches!(
            record.set_scalar("count", ScalarValue::Int(2)),
            Err(RecordError::Write {
                error: WriteError::Structural,
                ..
            })
        ));
        record.set_structural("count", ScalarValue::Int(2)).unwrap();
        assert_eq!(record.int("count"), Some(2));
        assert!(matches!(
            record.set_structural("count", ScalarValue::Int(70_000)),
            Err(RecordError::Write {
                error: WriteError::DoesNotFit { .. },
                ..
            })
        ));
    }

    #[test]
    fn a_record_carries_the_pack_it_came_from() {
        let a = pack();
        let mut b = pack();
        b.target = "t2".into();
        let ra = Record::zeroed(a.plans(), "Root").unwrap();
        let rb = Record::zeroed(b.plans(), "Root").unwrap();
        assert_eq!(ra.pack_id(), "synthetic/t1");
        assert_eq!(rb.pack_id(), "synthetic/t2");
        assert_ne!(ra, rb);
    }
}
