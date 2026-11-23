use std::marker::PhantomData;
use std::sync::Arc;

use super::{describe::intern, Plain, Record, View, ViewError};
use crate::desc::{Image, Pack, RecordRun, TypeName, Value};

/// Bytes held at 16-byte alignment, so any declared record can be viewed in place.
#[derive(Clone)]
pub struct Aligned {
    blocks: Vec<Block>,
    len: usize,
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct Block([u8; 16]);

impl Aligned {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut blocks = vec![Block([0; 16]); bytes.len().div_ceil(16)];
        for (block, chunk) in blocks.iter_mut().zip(bytes.chunks(16)) {
            block.0[..chunk.len()].copy_from_slice(chunk);
        }
        Self {
            blocks,
            len: bytes.len(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `Block` is sixteen `u8`s with no padding, so the blocks are
        // `16 * blocks.len()` initialised bytes and `len` never exceeds that.
        unsafe { std::slice::from_raw_parts(self.blocks.as_ptr().cast::<u8>(), self.len) }
    }
}

/// A canonical save: the image the writer made, held aligned so it can be viewed at once.
pub struct Saved {
    pub bytes: Aligned,
    pub virtual_size: usize,
    pub root: usize,
}

impl Saved {
    pub fn image(&self) -> Image<'_> {
        Image::new(self.bytes.bytes(), self.virtual_size)
    }

    pub fn view<T: Record>(&self) -> Result<View<'_, T>, ViewError> {
        View::at_offset(self.image(), self.root)
    }
}

fn tree(error: impl std::fmt::Display) -> ViewError {
    ViewError::Tree(error.to_string())
}

/// A resource read as the record tree, rooted at a declared `T`: what an edit goes through.
pub struct Doc<T: Record> {
    pack: Arc<Pack>,
    root: Value,
    _t: PhantomData<fn() -> T>,
}

impl<T: Record> Doc<T> {
    pub fn read(pack: Arc<Pack>, image: Image<'_>, offset: usize) -> Result<Self, ViewError> {
        if let Some(only) = T::target() {
            if only != pack.target {
                return Err(ViewError::Tree(format!(
                    "`{}` is a `{only}` declaration and this is a `{}` description",
                    T::name(),
                    pack.target
                )));
            }
        }
        let root = crate::desc::value::read(&pack, TypeName(intern(&T::name())), image, offset)
            .map_err(tree)?;
        Ok(Self {
            pack,
            root,
            _t: PhantomData,
        })
    }

    pub fn pack(&self) -> &Pack {
        &self.pack
    }

    pub fn value(&self) -> &Value {
        &self.root
    }

    pub fn root(&self) -> T {
        self.root
            .record()
            .and_then(|r| T::read_from(r.bytes()))
            .expect("a document's root is a record of its own type")
    }

    pub fn root_mut(&mut self) -> RecordMut<'_, T> {
        RecordMut::over(&self.pack, &mut self.root)
            .expect("a document's root is a record of its own type")
    }

    /// The canonical form: value-equivalent on the first save, and a fixed point after it.
    pub fn save(&self) -> Result<Saved, ViewError> {
        let built = crate::write::build(&self.pack, &self.root).map_err(tree)?;
        let virtual_size = built.virtual_data.len();
        let mut bytes = built.virtual_data;
        bytes.extend_from_slice(&built.physical_data);
        let root = built
            .root
            .checked_sub(crate::constants::FIXED_VIRTUAL_BASE)
            .ok_or_else(|| tree(format!("the root landed at {:#x}", built.root)))?
            as usize;
        Ok(Saved {
            bytes: Aligned::from_bytes(&bytes),
            virtual_size,
            root,
        })
    }
}

enum Place<'r> {
    Value(&'r mut Value),
    Packed(&'r mut RecordRun, usize),
}

/// What `RecordMut<'r, T>` derefs to: generated per record like [`super::view::Access`],
/// holding the child accessors, and dereffing on to `T` for field assignment.
pub trait Edit<'r, T: Record>: std::ops::DerefMut<Target = T> {
    fn wrap(raw: RawRecord<'r, T>) -> Self;
    fn raw_mut(&mut self) -> &mut RawRecord<'r, T>;
}

/// A record opened for editing: its child accessors, then a copy of its `T`
/// to assign through, written back when it drops.
pub struct RecordMut<'r, T: Record> {
    edit: T::Edit<'r>,
}

impl<'r, T: Record> std::ops::Deref for RecordMut<'r, T> {
    type Target = T::Edit<'r>;
    fn deref(&self) -> &T::Edit<'r> {
        &self.edit
    }
}

impl<T: Record> std::ops::DerefMut for RecordMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.edit
    }
}

/// The copy, the place it came from and the description it answers to.
pub struct RawRecord<'r, T: Plain> {
    pack: &'r Pack,
    place: Place<'r>,
    local: T,
}

impl<T: Plain> RawRecord<'_, T> {
    pub fn get(&self) -> &T {
        &self.local
    }
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.local
    }
}

impl<T: Plain> Drop for RawRecord<'_, T> {
    fn drop(&mut self) {
        write_back(&mut self.place, self.local.as_bytes());
    }
}

fn write_back(place: &mut Place<'_>, bytes: &[u8]) {
    match place {
        Place::Value(value) => {
            if let Some(record) = value.record_mut() {
                record.bytes_mut().copy_from_slice(bytes);
            }
        }
        Place::Packed(run, index) => {
            let at = *index * run.stride();
            run.bytes_mut()[at..at + bytes.len()].copy_from_slice(bytes);
        }
    }
}

impl<'r, T: Record> RecordMut<'r, T> {
    fn over(pack: &'r Pack, value: &'r mut Value) -> Result<Self, ViewError> {
        let record = value.record().ok_or_else(|| tree("not a record"))?;
        if record.type_name() != T::name() {
            return Err(ViewError::WrongType {
                expected: T::name(),
                found: record.type_name().to_string(),
            });
        }
        let local =
            T::read_from(record.bytes()).ok_or_else(|| tree("a record shorter than its type"))?;
        Ok(Self {
            edit: T::Edit::wrap(RawRecord {
                pack,
                place: Place::Value(value),
                local,
            }),
        })
    }

    fn packed(pack: &'r Pack, run: &'r mut RecordRun, index: usize) -> Result<Self, ViewError> {
        if run.type_name() != T::name() {
            return Err(ViewError::WrongType {
                expected: T::name(),
                found: run.type_name().to_string(),
            });
        }
        let at = index * run.stride();
        let local = run
            .bytes()
            .get(at..)
            .and_then(T::read_from)
            .ok_or_else(|| tree(format!("no element {index}")))?;
        Ok(Self {
            edit: T::Edit::wrap(RawRecord {
                pack,
                place: Place::Packed(run, index),
                local,
            }),
        })
    }
}

impl<'r, T: Record> RawRecord<'r, T> {
    #[doc(hidden)]
    pub fn __one<U: Record>(
        &mut self,
        field: &'static str,
    ) -> Result<Option<RecordMut<'_, U>>, ViewError> {
        let pack = self.pack;
        let Place::Value(value) = &mut self.place else {
            return Ok(None);
        };
        let record = value.record_mut().ok_or_else(|| tree("not a record"))?;
        match record.child_mut(field) {
            None => Ok(None),
            Some(Value::Ptr { target: None, .. }) => Ok(None),
            Some(Value::Ptr {
                target: Some(target),
                ..
            }) => RecordMut::over(pack, target).map(Some),
            Some(child @ Value::Struct(_)) => RecordMut::over(pack, child).map(Some),
            Some(other) => Err(tree(format!("`{field}` holds {other:?}, not a record"))),
        }
    }

    #[doc(hidden)]
    pub fn __run<U: Record>(&mut self, field: &'static str) -> Result<RunMut<'_, U>, ViewError> {
        write_back(&mut self.place, self.local.as_bytes());
        let pack = self.pack;
        let Place::Value(owner) = &mut self.place else {
            return Err(tree(format!(
                "`{field}` is in a packed run, which has no children"
            )));
        };
        Ok(RunMut {
            pack,
            owner,
            field,
            // SAFETY: `T: Plain`, so every byte pattern written here is a valid `T`.
            local: unsafe {
                std::slice::from_raw_parts_mut(
                    (&mut self.local as *mut T).cast::<u8>(),
                    std::mem::size_of::<T>(),
                )
            },
            _u: PhantomData,
        })
    }
}

/// A run opened for editing: its elements, and inserts and removes that keep its count.
pub struct RunMut<'p, U: Record> {
    pack: &'p Pack,
    owner: &'p mut Value,
    field: &'static str,
    local: &'p mut [u8],
    _u: PhantomData<fn() -> U>,
}

fn run_of<'v>(owner: &'v mut Value, field: &str) -> Option<&'v mut Value> {
    let slot = owner.child_mut(field)?;
    match slot {
        Value::Ptr { .. } => slot.target_mut(),
        other => Some(other),
    }
}

impl<U: Record> RunMut<'_, U> {
    pub fn len(&mut self) -> usize {
        run_of(self.owner, self.field)
            .and_then(|run| run.run_len())
            .unwrap_or(0)
    }

    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    pub fn get_mut(&mut self, index: usize) -> Result<Option<RecordMut<'_, U>>, ViewError> {
        let pack = self.pack;
        let Some(run) = run_of(self.owner, self.field) else {
            return Ok(None);
        };
        match run {
            Value::Array { elements, .. } => match elements.get_mut(index) {
                Some(element) => RecordMut::over(pack, element).map(Some),
                None => Ok(None),
            },
            Value::Records(records) => {
                if index >= records.len() {
                    return Ok(None);
                }
                RecordMut::packed(pack, records, index).map(Some)
            }
            other => Err(tree(format!("`{}` holds {other:?}, not a run", self.field))),
        }
    }

    pub fn push(&mut self, element: U) -> Result<(), ViewError> {
        let at = self.len();
        self.insert(at, element)
    }

    pub fn insert(&mut self, index: usize, element: U) -> Result<(), ViewError> {
        let record = crate::desc::Record::from_bytes(
            self.pack.plans(),
            &U::name(),
            element.as_bytes().to_vec(),
        )
        .map_err(tree)?;
        crate::write::edit::insert_at(
            self.pack,
            self.owner,
            self.field,
            index,
            Value::Struct(record),
        )
        .map_err(tree)?;
        self.sync();
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Result<U, ViewError> {
        let removed =
            crate::write::edit::remove(self.pack, self.owner, self.field, index).map_err(tree)?;
        self.sync();
        removed
            .record()
            .and_then(|r| U::read_from(r.bytes()))
            .ok_or_else(|| tree("the removed element was not a record"))
    }

    fn sync(&mut self) {
        let fresh = self.owner.record().map(|r| r.bytes()).unwrap_or(&[]);
        let n = self.local.len().min(fresh.len());
        self.local[..n].copy_from_slice(&fresh[..n]);
    }
}
