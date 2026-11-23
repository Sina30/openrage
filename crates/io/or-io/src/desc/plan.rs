//! What a bound pack answers about each of its types, resolved once: a
//! [`TypePlan`] per type, and the slots a read takes from it.
//!
//! ⚠ Every slot a plan hands out is proved at build time to sit inside its
//! type, and that proof is what lets [`Checked::read`] skip a bounds check.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use thiserror::Error;

use crate::desc::pack::{prettify_name, Pack, PackEnum, PackSelector, PackType};
use crate::desc::repr::{FieldRepr, IntWidth, Pointee, Scalar};
use crate::desc::value::ScalarValue;
use crate::desc::view::WriteError;

/// FNV-1a over a field path; `const` so a call site hashes its literal at compile time.
pub const fn fnv1a(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let mut hash = 0x811c_9dc5u32;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

/// How deep a plan follows embedded types when it names their fields by path;
/// a deeper chain is a cycle in the description, not a layout.
const MAX_INLINE_DEPTH: usize = 16;

/// A type whose flattened field list would pass this stops naming deeper
/// paths. Its copy ranges, which a build depends on, are always complete.
const MAX_ENTRIES: usize = 1 << 16;

/// Epoch 0 is what an unused call-site cache holds, so no plan may have it.
static NEXT_EPOCH: AtomicU32 = AtomicU32::new(1);

/// A multiply-rotate hash for the reader's hot maps: type names and addresses.
///
/// Not collision-resistant, and nothing here needs it to be: the keys are a
/// pack's own type names and a file's own addresses. SipHash measured as a
/// visible share of a whole-file read.
#[derive(Default, Clone, Copy)]
pub struct FastHash(u64);

impl std::hash::Hasher for FastHash {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for chunk in chunks {
            self.write_u64(u64::from_le_bytes(*chunk));
        }
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        self.write_u64(u64::from_le_bytes(tail) ^ rest.len() as u64);
    }

    #[inline]
    fn write_u64(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

pub type FastMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FastHash>>;
pub type FastSet<K> = std::collections::HashSet<K, std::hash::BuildHasherDefault<FastHash>>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlanError {
    #[error("the {pack} pack describes no type named `{type_name}`")]
    NoType { pack: String, type_name: String },

    #[error("the {pack} pack does not describe {type_name}.{field}")]
    NoField {
        pack: String,
        type_name: String,
        field: String,
    },

    #[error("{type_name}.{field} in the {pack} pack is {found}, and was asked for as {expected}")]
    WrongKind {
        pack: String,
        type_name: String,
        field: String,
        found: String,
        expected: &'static str,
    },

    #[error(
        "the {pack} pack describes {type_name} in a way that contradicts itself, so it has no \
         plan: {}",
        problems.join("; ")
    )]
    Incoherent {
        pack: String,
        type_name: String,
        problems: Vec<String>,
    },
}

/// What a scalar field holds, as a read has to know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int(IntWidth),
    Enum(IntWidth),
    F32,
    F64,
    Bool,
}

impl Kind {
    pub fn of(repr: &FieldRepr) -> Option<Kind> {
        match repr {
            FieldRepr::Scalar(scalar) => Some(Kind::of_scalar(*scalar)),
            FieldRepr::Enum { width, .. } => Some(Kind::Enum(*width)),
            _ => None,
        }
    }

    pub fn of_scalar(scalar: Scalar) -> Kind {
        match scalar {
            Scalar::Int(w) => Kind::Int(w),
            Scalar::F32 => Kind::F32,
            Scalar::F64 => Kind::F64,
            Scalar::Bool => Kind::Bool,
        }
    }

    pub const fn size(self) -> usize {
        match self {
            Kind::Int(w) | Kind::Enum(w) => w.size(),
            Kind::F32 => 4,
            Kind::F64 => 8,
            Kind::Bool => 1,
        }
    }

    /// Decode `bytes`, which must be exactly [`size`](Self::size) long.
    pub fn decode(self, bytes: &[u8]) -> Option<ScalarValue> {
        Some(match self {
            Kind::Int(w) | Kind::Enum(w) => ScalarValue::Int(w.read(bytes, 0)?),
            Kind::F32 => ScalarValue::F32(f32::from_le_bytes(bytes.get(..4)?.try_into().ok()?)),
            Kind::F64 => ScalarValue::F64(f64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)),
            Kind::Bool => ScalarValue::Bool(*bytes.first()? != 0),
        })
    }

    /// Encode `value` into `out`, which must be exactly [`size`](Self::size) long.
    pub fn encode(self, value: &ScalarValue, out: &mut [u8]) -> Result<(), WriteError> {
        match (self, value) {
            (Kind::Int(w) | Kind::Enum(w), ScalarValue::Int(v)) => {
                if !w.holds(*v) {
                    return Err(WriteError::DoesNotFit { width: w });
                }
                if w.write(out, 0, *v) {
                    Ok(())
                } else {
                    Err(WriteError::OutOfBounds {
                        offset: 0,
                        size: out.len(),
                    })
                }
            }
            (Kind::Int(w) | Kind::Enum(w), ScalarValue::Bool(b)) => self
                .encode(&ScalarValue::Int(i128::from(*b)), out)
                .map_err(|_| WriteError::DoesNotFit { width: w }),
            (Kind::Bool, ScalarValue::Bool(b)) => {
                out[0] = u8::from(*b);
                Ok(())
            }
            (Kind::Bool, ScalarValue::Int(v)) if (0..=255).contains(v) => {
                out[0] = *v as u8;
                Ok(())
            }
            (Kind::F32, ScalarValue::F32(v)) => {
                out.copy_from_slice(&v.to_le_bytes());
                Ok(())
            }
            (Kind::F64, ScalarValue::F64(v)) => {
                out.copy_from_slice(&v.to_le_bytes());
                Ok(())
            }
            _ => Err(WriteError::WrongKind),
        }
    }
}

/// One field a plan can answer for, named by its path from the planned type.
///
/// An embedded type's fields appear as `outer.inner`, an element of a fixed
/// run of embedded types as `run[2].inner`, and an element of a fixed run of
/// scalars as `run[2]`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanField {
    pub path: String,
    /// From the start of the planned type.
    pub offset: usize,
    pub repr: FieldRepr,
    pub kind: Option<Kind>,
    pub note: Option<String>,
    /// A count, a capacity or a tagged base's selector of the type declaring
    /// it: changing one needs a rebuild rather than a patch.
    pub structural: bool,
    /// Several values share its bytes, each in a declared bit range.
    pub bit_packed: bool,
    /// 0 for a field the planned type declares itself.
    pub depth: usize,
}

impl PlanField {
    pub fn name(&self) -> &str {
        let last = self.path.rsplit('.').next().unwrap_or(&self.path);
        last.split('[').next().unwrap_or(last)
    }

    pub fn display_name(&self) -> String {
        prettify_name(self.name())
    }
}

/// A plan the pack built for one described type.
#[derive(Debug)]
pub struct TypePlan {
    epoch: u32,
    index: u32,
    pack: Arc<str>,
    ty: PackType,
    problems: Vec<String>,
    entries: Vec<PlanField>,
    keys: Vec<u32>,
    table: Box<[u32]>,
    copy: Vec<(usize, usize)>,
    children: usize,
    steps: Vec<Step>,
}

/// One field a reader visits to find a record's children, in the order it visits them.
///
/// Every pointer, pointer table and trailing run of the type itself first,
/// then every embedded struct in declaration order — the order a rebuild
/// allocates in, which is why the reader has to match it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Step {
    /// `fields[field]` is a child; `target` is its pointee's or element's plan,
    /// where the description names one the pack has.
    Child { field: u32, target: Option<u32> },
    /// `fields[field]` embeds `count` structs of plan `inner`, where the pack has it.
    Embed {
        field: u32,
        inner: Option<u32>,
        count: u32,
    },
}

fn steps_of(ty: &PackType, by_name: &FastMap<String, u32>) -> Vec<Step> {
    let index = |name: &str| by_name.get(name).copied();
    let target = |repr: &FieldRepr| match repr {
        FieldRepr::Ptr(pointee)
        | FieldRepr::AtArray(pointee)
        | FieldRepr::PtrSlots { pointee, .. } => match pointee {
            Pointee::One { type_name } => index(type_name),
            Pointee::Many { type_name, .. } | Pointee::ManyPtr { type_name, .. } => {
                type_name.as_deref().and_then(index)
            }
            _ => None,
        },
        FieldRepr::Trailing { type_name, .. } => type_name.as_deref().and_then(index),
        _ => None,
    };
    let mut steps = Vec::new();
    for (i, field) in ty.fields.iter().enumerate() {
        if field.repr.is_indirect() {
            steps.push(Step::Child {
                field: i as u32,
                target: target(&field.repr),
            });
        }
    }
    for (i, field) in ty.fields.iter().enumerate() {
        match &field.repr {
            _ if field.repr.is_indirect() => {}
            FieldRepr::Inline { type_name } => steps.push(Step::Embed {
                field: i as u32,
                inner: index(type_name),
                count: 1,
            }),
            FieldRepr::InlineArray { type_name, count } => steps.push(Step::Embed {
                field: i as u32,
                inner: index(type_name),
                count: *count as u32,
            }),
            FieldRepr::Trailing { .. } => steps.push(Step::Child {
                field: i as u32,
                target: target(&field.repr),
            }),
            _ => {}
        }
    }
    steps
}

impl TypePlan {
    /// The fields a reader visits for this type's children, in order.
    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// How many children a record of this type holds: one per pointer,
    /// pointer table and trailing run, embedded structs' included.
    pub fn child_count(&self) -> usize {
        self.children
    }

    pub fn name(&self) -> &str {
        &self.ty.name
    }

    pub fn size(&self) -> usize {
        self.ty.size
    }

    /// Distinct for every plan built in this process; what a call-site cache is tagged with.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// This type's position in its [`Plans`].
    pub fn index(&self) -> u32 {
        self.index
    }

    /// `title/target` of the pack that built this plan.
    pub fn pack_id(&self) -> &str {
        &self.pack
    }

    /// The description the plan was built from.
    pub fn pack_type(&self) -> &PackType {
        &self.ty
    }

    pub fn selector(&self) -> Option<&PackSelector> {
        self.ty.selector.as_ref()
    }

    pub fn is_coherent(&self) -> bool {
        self.problems.is_empty()
    }

    /// Every field by path, depth-first in declaration order.
    pub fn fields(&self) -> &[PlanField] {
        &self.entries
    }

    /// The fields this type declares itself, in declaration order.
    pub fn own_fields(&self) -> impl Iterator<Item = &PlanField> {
        self.entries.iter().filter(|f| f.depth == 0)
    }

    /// The byte ranges a build copies from a record: every scalar, enum,
    /// padding and undescribed region, embedded types' included.
    ///
    /// Everything else — pointer slots and bytes no field covers — is written
    /// by the builder, which is what makes a build of an unchanged record a
    /// canonical form rather than a copy of whatever the file held there.
    pub fn copy_ranges(&self) -> &[(usize, usize)] {
        &self.copy
    }

    pub fn field(&self, path: &str) -> Option<&PlanField> {
        self.lookup(path).map(|i| &self.entries[i])
    }

    fn lookup(&self, path: &str) -> Option<usize> {
        if self.table.is_empty() {
            return None;
        }
        let key = fnv1a(path);
        let mask = self.table.len() - 1;
        let mut i = key as usize & mask;
        loop {
            let slot = self.table[i];
            if slot == 0 {
                return None;
            }
            let entry = (slot - 1) as usize;
            if self.keys[entry] == key && self.entries[entry].path == path {
                return Some(entry);
            }
            i = (i + 1) & mask;
        }
    }

    /// Resolve a scalar field to a typed slot; what [`at!`](crate::at) memoises.
    ///
    /// ⚠ It hashes the path, so called on every read it measured at about a
    /// hundred times a pass's read.
    pub fn slot<T: PlanScalar>(&self, path: &str) -> Result<Slot<T>, PlanError> {
        let field = self.checked_field(path)?;
        let kind = field.kind.ok_or_else(|| self.wrong(field, T::NAME))?;
        if !T::accepts(kind) {
            return Err(self.wrong(field, T::NAME));
        }
        let end = field.offset + T::SIZE;
        // The one soundness condition for `Rec::get`'s unchecked load: a slot
        // never reaches past its type. An embedded type the coherence check
        // did not measure is held to it here.
        if end > self.ty.size || end > u32::MAX as usize {
            return Err(PlanError::Incoherent {
                pack: self.pack.to_string(),
                type_name: self.ty.name.clone(),
                problems: vec![format!(
                    "`{}` ends at {end:#x}, past the type's {:#x} bytes",
                    field.path, self.ty.size
                )],
            });
        }
        Ok(Slot {
            offset: field.offset as u32,
            end: end as u32,
            epoch: self.epoch,
            writable: !field.structural,
            _t: PhantomData,
        })
    }

    /// Read many records of this plan with no check per field.
    ///
    /// The pass's lifetime brands every slot [`bound`](Pass::bind) and every
    /// record [`checked`](Pass::record) in it, so the two cannot come from
    /// different passes — which is what lets a read skip the check that
    /// [`Rec::get`] makes, the one that measured at twice a read's cost.
    #[inline(always)]
    pub fn pass<R>(&self, f: impl for<'b> FnOnce(Pass<'b, '_>) -> R) -> R {
        f(Pass {
            plan: self,
            _brand: PhantomData,
        })
    }

    /// Resolve any field by path, refusing an incoherent type or an unknown path.
    pub fn checked_field(&self, path: &str) -> Result<&PlanField, PlanError> {
        if !self.problems.is_empty() {
            return Err(PlanError::Incoherent {
                pack: self.pack.to_string(),
                type_name: self.ty.name.clone(),
                problems: self.problems.clone(),
            });
        }
        self.field(path).ok_or_else(|| PlanError::NoField {
            pack: self.pack.to_string(),
            type_name: self.ty.name.clone(),
            field: path.to_string(),
        })
    }

    fn wrong(&self, field: &PlanField, expected: &'static str) -> PlanError {
        PlanError::WrongKind {
            pack: self.pack.to_string(),
            type_name: self.ty.name.clone(),
            field: field.path.clone(),
            found: match field.kind {
                Some(kind) => format!("{kind:?}"),
                None => format!("{:?}", field.repr),
            },
            expected,
        }
    }

    /// Check a record once: `base + size` inside `bytes`.
    ///
    /// Every slot of this plan then reads without a bounds check of its own.
    #[inline(always)]
    pub fn record<'d>(&self, bytes: &'d [u8], base: usize) -> Option<Rec<'d>> {
        let size = self.ty.size;
        if bytes.len() < size || base > bytes.len() - size {
            return None;
        }
        // SAFETY: `base + size <= bytes.len()`, checked above. Built from the
        // plan's own size rather than by slicing, so a read's `end <= len`
        // compares two values that do not change across a pass and the
        // compiler can lift it out of the loop.
        Some(Rec {
            bytes: unsafe { std::slice::from_raw_parts(bytes.as_ptr().add(base), size) },
        })
    }

    #[inline]
    pub fn record_mut<'d>(&self, bytes: &'d mut [u8], base: usize) -> Option<RecMut<'d>> {
        let end = base.checked_add(self.ty.size)?;
        bytes.get_mut(base..end).map(|bytes| RecMut { bytes })
    }

    fn build(
        index: u32,
        pack: &Arc<str>,
        ty: &PackType,
        types: &[PackType],
        by_name: &FastMap<String, u32>,
    ) -> TypePlan {
        let size_of = |name: &str| by_name.get(name).map(|&i| types[i as usize].size);
        let problems = ty.incoherences(size_of);

        let mut entries = Vec::new();
        let mut copy = Vec::new();
        flatten(
            ty,
            types,
            by_name,
            "",
            0,
            0,
            &mut entries,
            &mut copy,
            &mut Vec::new(),
        );
        copy.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(copy.len());
        for (start, end) in copy {
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }

        let keys: Vec<u32> = entries.iter().map(|e| fnv1a(&e.path)).collect();
        let capacity = (entries.len() * 2).next_power_of_two().max(8);
        let mut table = vec![0u32; capacity].into_boxed_slice();
        let mask = capacity - 1;
        for (entry, &key) in keys.iter().enumerate() {
            let mut i = key as usize & mask;
            loop {
                let slot = table[i];
                if slot == 0 {
                    table[i] = entry as u32 + 1;
                    break;
                }
                // The first of two equal paths answers, as `field_by_name` does.
                if keys[(slot - 1) as usize] == key
                    && entries[(slot - 1) as usize].path == entries[entry].path
                {
                    break;
                }
                i = (i + 1) & mask;
            }
        }

        TypePlan {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            index,
            pack: pack.clone(),
            ty: ty.clone(),
            problems,
            entries,
            keys,
            table,
            copy: merged,
            children: count_children(ty, types, by_name, 0),
            steps: steps_of(ty, by_name),
        }
    }
}

fn count_children(
    ty: &PackType,
    types: &[PackType],
    by_name: &FastMap<String, u32>,
    depth: usize,
) -> usize {
    if depth > MAX_INLINE_DEPTH {
        return 0;
    }
    ty.fields
        .iter()
        .map(|field| match &field.repr {
            FieldRepr::Ptr(_)
            | FieldRepr::AtArray(_)
            | FieldRepr::PtrSlots { .. }
            | FieldRepr::Trailing { .. } => 1,
            FieldRepr::Inline { type_name } => by_name.get(type_name).map_or(0, |&i| {
                count_children(&types[i as usize], types, by_name, depth + 1)
            }),
            FieldRepr::InlineArray { type_name, count } => by_name.get(type_name).map_or(0, |&i| {
                count * count_children(&types[i as usize], types, by_name, depth + 1)
            }),
            _ => 0,
        })
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn flatten(
    ty: &PackType,
    types: &[PackType],
    by_name: &FastMap<String, u32>,
    prefix: &str,
    base: usize,
    depth: usize,
    out: &mut Vec<PlanField>,
    copy: &mut Vec<(usize, usize)>,
    chain: &mut Vec<String>,
) {
    if depth > MAX_INLINE_DEPTH || chain.iter().any(|t| t == &ty.name) {
        return;
    }
    chain.push(ty.name.clone());
    let structural = ty.structural_fields();
    for field in &ty.fields {
        let path = format!("{prefix}{}", field.name);
        let at = base + field.offset;
        let is_structural = structural.contains(field.name.as_str());
        if out.len() < MAX_ENTRIES {
            out.push(PlanField {
                path: path.clone(),
                offset: at,
                repr: field.repr.clone(),
                kind: Kind::of(&field.repr),
                note: field.note.clone(),
                structural: is_structural,
                bit_packed: field.is_bit_packed(),
                depth,
            });
        }
        match &field.repr {
            FieldRepr::Scalar(_)
            | FieldRepr::Enum { .. }
            | FieldRepr::Padding { .. }
            | FieldRepr::Opaque { .. } => {
                let width = field.repr.inline_size().unwrap_or(0);
                if width > 0 {
                    copy.push((at, at + width));
                }
            }
            FieldRepr::ScalarArray { element, count } => {
                copy.push((at, at + element.size() * count));
                for i in 0..*count {
                    if out.len() >= MAX_ENTRIES {
                        break;
                    }
                    out.push(PlanField {
                        path: format!("{path}[{i}]"),
                        offset: at + i * element.size(),
                        repr: FieldRepr::Scalar(*element),
                        kind: Some(Kind::of_scalar(*element)),
                        note: field.note.clone(),
                        structural: is_structural,
                        bit_packed: false,
                        depth: depth + 1,
                    });
                }
            }
            FieldRepr::Inline { type_name } => {
                if let Some(&inner) = by_name.get(type_name) {
                    flatten(
                        &types[inner as usize],
                        types,
                        by_name,
                        &format!("{path}."),
                        at,
                        depth + 1,
                        out,
                        copy,
                        chain,
                    );
                }
            }
            FieldRepr::InlineArray { type_name, count } => {
                if let Some(&inner) = by_name.get(type_name) {
                    let inner = &types[inner as usize];
                    for i in 0..*count {
                        flatten(
                            inner,
                            types,
                            by_name,
                            &format!("{path}[{i}]."),
                            at + i * inner.size,
                            depth + 1,
                            out,
                            copy,
                            chain,
                        );
                    }
                }
            }
            FieldRepr::Ptr(_)
            | FieldRepr::AtArray(_)
            | FieldRepr::PtrSlots { .. }
            | FieldRepr::Trailing { .. } => {}
        }
    }
    chain.pop();
}

/// Every type plan one pack built, indexed.
///
/// Self-contained: it holds the descriptions it was built from, so a record
/// carrying it carries its pack.
#[derive(Debug)]
pub struct Plans {
    pack: Arc<str>,
    title: String,
    target: String,
    types: Vec<TypePlan>,
    by_name: FastMap<String, u32>,
    enums: Vec<PackEnum>,
}

impl Plans {
    pub fn build(pack: &Pack) -> Plans {
        let id: Arc<str> = pack.id().into();
        let mut by_name = FastMap::with_capacity_and_hasher(pack.types.len(), Default::default());
        for (i, ty) in pack.types.iter().enumerate() {
            by_name.entry(ty.name.clone()).or_insert(i as u32);
        }
        let types = pack
            .types
            .iter()
            .enumerate()
            .map(|(i, ty)| TypePlan::build(i as u32, &id, ty, &pack.types, &by_name))
            .collect();
        Plans {
            pack: id,
            title: pack.title.clone(),
            target: pack.target.clone(),
            types,
            by_name,
            enums: pack.enums.clone(),
        }
    }

    /// `title/target`, as [`Pack::id`] spells it.
    pub fn pack_id(&self) -> &str {
        &self.pack
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn index_of(&self, type_name: &str) -> Option<u32> {
        self.by_name.get(type_name).copied()
    }

    pub fn get(&self, type_name: &str) -> Result<&TypePlan, PlanError> {
        self.index_of(type_name)
            .map(|i| &self.types[i as usize])
            .ok_or_else(|| PlanError::NoType {
                pack: self.pack.to_string(),
                type_name: type_name.to_string(),
            })
    }

    /// The plan at `index`, which came from [`index_of`](Self::index_of) or a record.
    pub fn at(&self, index: u32) -> &TypePlan {
        &self.types[index as usize]
    }

    pub fn types(&self) -> &[TypePlan] {
        &self.types
    }

    pub fn enum_by_name(&self, name: &str) -> Option<&PackEnum> {
        self.enums.iter().find(|e| e.name == name)
    }

    /// Which embedded `member` of a record of plan `owner` starts at `offset`:
    /// its path, and its index where it is one element of an embedded run.
    ///
    /// ⚠ The start of a member of that type, never the nearest one: an address
    /// anywhere else is a pointer this description does not explain.
    pub fn member_at(
        &self,
        owner: u32,
        member: &str,
        offset: usize,
    ) -> Option<(String, Option<usize>)> {
        for field in self.at(owner).fields() {
            match &field.repr {
                FieldRepr::Inline { type_name }
                    if type_name == member && field.offset == offset =>
                {
                    return Some((field.path.clone(), None));
                }
                FieldRepr::InlineArray { type_name, count } if type_name == member => {
                    let stride = self.get(type_name).ok()?.size();
                    let Some(rel) = offset.checked_sub(field.offset) else {
                        continue;
                    };
                    if stride > 0 && rel % stride == 0 && rel / stride < *count {
                        return Some((field.path.clone(), Some(rel / stride)));
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Where `member` at `path` (and `index`) starts in a record of plan
    /// `owner`, if it is an embedded `member` there.
    pub fn member_offset(
        &self,
        owner: u32,
        member: &str,
        path: &str,
        index: Option<usize>,
    ) -> Option<usize> {
        let field = self.at(owner).field(path)?;
        match (&field.repr, index) {
            (FieldRepr::Inline { type_name }, None) if type_name == member => Some(field.offset),
            (FieldRepr::InlineArray { type_name, count }, Some(i))
                if type_name == member && i < *count =>
            {
                Some(field.offset + i * self.get(type_name).ok()?.size())
            }
            _ => None,
        }
    }
}

/// Where a pack keeps the plans it has built. Not serialised, and a clone
/// starts empty so a copy edited before it is asked builds its own.
#[derive(Default)]
pub(crate) struct PlanCache(OnceLock<Arc<Plans>>);

impl PlanCache {
    pub(crate) fn get_or_build(&self, pack: &Pack) -> &Arc<Plans> {
        self.0.get_or_init(|| Arc::new(Plans::build(pack)))
    }
}

impl Clone for PlanCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for PlanCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for PlanCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.get().is_some() {
            "PlanCache(built)"
        } else {
            "PlanCache(empty)"
        })
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A Rust scalar a slot can be typed as.
pub trait PlanScalar: Copy + sealed::Sealed {
    const SIZE: usize;
    const NAME: &'static str;
    fn accepts(kind: Kind) -> bool;
    /// # Safety
    /// `p` must be valid for reading `SIZE` bytes.
    unsafe fn load(p: *const u8) -> Self;
    /// `out` is exactly `SIZE` bytes.
    fn store(self, out: &mut [u8]);
}

macro_rules! plan_int {
    ($($ty:ty => $width:ident),* $(,)?) => {$(
        impl sealed::Sealed for $ty {}
        impl PlanScalar for $ty {
            const SIZE: usize = std::mem::size_of::<$ty>();
            const NAME: &'static str = stringify!($ty);
            fn accepts(kind: Kind) -> bool {
                match kind {
                    Kind::Int(w) => w == IntWidth::$width,
                    // An enum's signedness is how the engine declared its
                    // storage, not a property of its values.
                    Kind::Enum(w) => w.size() == Self::SIZE,
                    Kind::Bool => Self::SIZE == 1,
                    _ => false,
                }
            }
            #[inline(always)]
            unsafe fn load(p: *const u8) -> Self {
                <$ty>::from_le(std::ptr::read_unaligned(p as *const $ty))
            }
            #[inline(always)]
            fn store(self, out: &mut [u8]) {
                out.copy_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}

plan_int!(u8 => U8, u16 => U16, u32 => U32, u64 => U64, i8 => I8, i16 => I16, i32 => I32, i64 => I64);

impl sealed::Sealed for f32 {}
impl PlanScalar for f32 {
    const SIZE: usize = 4;
    const NAME: &'static str = "f32";
    fn accepts(kind: Kind) -> bool {
        kind == Kind::F32
    }
    #[inline(always)]
    unsafe fn load(p: *const u8) -> Self {
        f32::from_bits(u32::from_le(std::ptr::read_unaligned(p as *const u32)))
    }
    #[inline(always)]
    fn store(self, out: &mut [u8]) {
        out.copy_from_slice(&self.to_le_bytes());
    }
}

impl sealed::Sealed for f64 {}
impl PlanScalar for f64 {
    const SIZE: usize = 8;
    const NAME: &'static str = "f64";
    fn accepts(kind: Kind) -> bool {
        kind == Kind::F64
    }
    #[inline(always)]
    unsafe fn load(p: *const u8) -> Self {
        f64::from_bits(u64::from_le(std::ptr::read_unaligned(p as *const u64)))
    }
    #[inline(always)]
    fn store(self, out: &mut [u8]) {
        out.copy_from_slice(&self.to_le_bytes());
    }
}

impl sealed::Sealed for bool {}
impl PlanScalar for bool {
    const SIZE: usize = 1;
    const NAME: &'static str = "bool";
    fn accepts(kind: Kind) -> bool {
        matches!(kind, Kind::Bool | Kind::Int(IntWidth::U8))
    }
    #[inline(always)]
    unsafe fn load(p: *const u8) -> Self {
        *p != 0
    }
    #[inline(always)]
    fn store(self, out: &mut [u8]) {
        out[0] = u8::from(self);
    }
}

/// A resolved scalar field: an offset proved to fit its type, and its kind in `T`.
pub struct Slot<T> {
    offset: u32,
    end: u32,
    epoch: u32,
    writable: bool,
    _t: PhantomData<fn() -> T>,
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Slot<T> {}

impl<T> std::fmt::Debug for Slot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Slot({:#x}..{:#x})", self.offset, self.end)
    }
}

impl<T> PartialEq for Slot<T> {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset
            && self.end == other.end
            && self.epoch == other.epoch
            && self.writable == other.writable
    }
}

impl<T> Slot<T> {
    pub fn offset(self) -> usize {
        self.offset as usize
    }

    pub fn is_writable(self) -> bool {
        self.writable
    }
}

/// One struct's bytes, checked once against its plan's size.
#[derive(Debug, Clone, Copy)]
pub struct Rec<'d> {
    bytes: &'d [u8],
}

impl<'d> Rec<'d> {
    /// `None` only for a slot from a larger type's plan.
    #[inline(always)]
    pub fn get<T: PlanScalar>(self, slot: Slot<T>) -> Option<T> {
        if slot.end as usize <= self.bytes.len() {
            // SAFETY: `offset + SIZE == end <= len`, by `Slot`'s only constructor.
            Some(unsafe { T::load(self.bytes.as_ptr().add(slot.offset as usize)) })
        } else {
            None
        }
    }

    /// Decode any scalar field of this record's plan, whatever its kind.
    pub fn scalar(self, field: &PlanField) -> Option<ScalarValue> {
        let kind = field.kind?;
        kind.decode(self.bytes.get(field.offset..field.offset + kind.size())?)
    }

    pub fn bytes(self) -> &'d [u8] {
        self.bytes
    }
}

/// One plan read over many records; see [`TypePlan::pass`].
///
/// Invariant in `'b`, and `'b` is chosen by `pass`, so no slot or record of
/// one pass can be named in another.
#[derive(Clone, Copy)]
pub struct Pass<'b, 'p> {
    plan: &'p TypePlan,
    _brand: PhantomData<fn(&'b ()) -> &'b ()>,
}

/// A slot proved to be this pass's plan's.
pub struct Bound<'b, T> {
    offset: u32,
    _brand: PhantomData<fn(&'b ()) -> &'b ()>,
    _t: PhantomData<fn() -> T>,
}

impl<T> Clone for Bound<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Bound<'_, T> {}

/// A record proved to hold this pass's plan's size.
#[derive(Clone, Copy)]
pub struct Checked<'b, 'd> {
    at: *const u8,
    _bytes: PhantomData<&'d [u8]>,
    _brand: PhantomData<fn(&'b ()) -> &'b ()>,
}

impl<'b, 'p> Pass<'b, 'p> {
    pub fn plan(self) -> &'p TypePlan {
        self.plan
    }

    /// `None` for a slot another plan resolved.
    #[inline(always)]
    pub fn bind<T: PlanScalar>(self, slot: Slot<T>) -> Option<Bound<'b, T>> {
        (slot.epoch == self.plan.epoch).then_some(Bound {
            offset: slot.offset,
            _brand: PhantomData,
            _t: PhantomData,
        })
    }

    /// [`TypePlan::record`], branded for this pass.
    #[inline(always)]
    pub fn record<'d>(self, bytes: &'d [u8], base: usize) -> Option<Checked<'b, 'd>> {
        let rec = self.plan.record(bytes, base)?;
        Some(Checked {
            at: rec.bytes.as_ptr(),
            _bytes: PhantomData,
            _brand: PhantomData,
        })
    }
}

impl<'b> Checked<'b, '_> {
    #[inline(always)]
    pub fn read<T: PlanScalar>(self, slot: Bound<'b, T>) -> T {
        // SAFETY: the slot was bound by this pass, so its plan resolved it and
        // proved `offset + SIZE <= size`; the record was checked by this pass,
        // so `size` bytes follow `at`. The brand keeps the two in one pass.
        unsafe { T::load(self.at.add(slot.offset as usize)) }
    }
}

/// One struct's bytes, writable through its plan's slots.
#[derive(Debug)]
pub struct RecMut<'d> {
    bytes: &'d mut [u8],
}

impl RecMut<'_> {
    /// Refuses a structural field: a count, a capacity or a tag moves an allocation.
    pub fn set<T: PlanScalar>(&mut self, slot: Slot<T>, value: T) -> Result<(), WriteError> {
        if !slot.writable {
            return Err(WriteError::Structural);
        }
        let size = self.bytes.len();
        let out = self
            .bytes
            .get_mut(slot.offset as usize..slot.end as usize)
            .ok_or(WriteError::OutOfBounds {
                offset: slot.offset as usize,
                size,
            })?;
        value.store(out);
        Ok(())
    }

    pub fn as_rec(&self) -> Rec<'_> {
        Rec { bytes: self.bytes }
    }
}

/// A call site's cached answer, tagged with the plan it came from.
///
/// Built by [`at!`](crate::at); nothing else needs one.
pub struct Site {
    path: &'static str,
    cache: AtomicU64,
}

impl Site {
    pub const fn new(path: &'static str) -> Self {
        Self {
            path,
            cache: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    pub fn slot<T: PlanScalar>(&self, plan: &TypePlan) -> Result<Slot<T>, PlanError> {
        let cached = self.cache.load(Ordering::Relaxed);
        if (cached >> 32) as u32 == plan.epoch {
            let low = cached as u32;
            let offset = low & 0x7FFF_FFFF;
            return Ok(Slot {
                offset,
                end: offset + T::SIZE as u32,
                epoch: plan.epoch,
                writable: low >> 31 == 1,
                _t: PhantomData,
            });
        }
        self.resolve(plan)
    }

    #[cold]
    fn resolve<T: PlanScalar>(&self, plan: &TypePlan) -> Result<Slot<T>, PlanError> {
        let slot = plan.slot::<T>(self.path)?;
        if slot.offset <= 0x7FFF_FFFF {
            let low = slot.offset | (u32::from(slot.writable) << 31);
            self.cache.store(
                (u64::from(plan.epoch) << 32) | u64::from(low),
                Ordering::Relaxed,
            );
        }
        Ok(slot)
    }
}

/// Resolve a field of a plan to a typed [`Slot`], memoised at this call site.
///
/// ```
/// use or_io::at;
/// use or_io::desc::{FieldRepr, IntWidth, Pack, PackField, PackType, Scalar};
///
/// let mut pack = Pack::new("synthetic", "t0");
/// pack.types.push(PackType {
///     name: "Widget".into(),
///     size: 8,
///     fields: vec![PackField {
///         name: "weight".into(),
///         offset: 4,
///         repr: FieldRepr::Scalar(Scalar::F32),
///         space: None,
///         bits: Vec::new(),
///         note: None,
///     }],
///     selector: None,
/// });
/// let plan = pack.plan("Widget").unwrap();
/// let weight = at!(plan, f32, "weight").unwrap();
/// let bytes = [0, 0, 0, 0, 0, 0, 0x80, 0x3f];
/// assert_eq!(plan.record(&bytes, 0).unwrap().get(weight), Some(1.0));
/// ```
#[macro_export]
macro_rules! at {
    ($plan:expr, $ty:ty, $path:expr) => {{
        static SITE: $crate::desc::plan::Site = $crate::desc::plan::Site::new($path);
        SITE.slot::<$ty>(&$plan)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desc::pack::PackField;
    use crate::desc::repr::{CountSource, Pointee};

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

    fn synthetic() -> Pack {
        let mut pack = Pack::new("synthetic", "t0");
        pack.types.push(PackType {
            name: "Inner".into(),
            size: 0x08,
            fields: vec![
                field("a", 0, FieldRepr::Scalar(Scalar::Int(IntWidth::U16))),
                field("b", 4, FieldRepr::Scalar(Scalar::F32)),
            ],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Outer".into(),
            size: 0x30,
            fields: vec![
                field("count", 0, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
                field("weight", 4, FieldRepr::Scalar(Scalar::F32)),
                field(
                    "inner",
                    8,
                    FieldRepr::Inline {
                        type_name: "Inner".into(),
                    },
                ),
                field(
                    "pair",
                    0x10,
                    FieldRepr::InlineArray {
                        type_name: "Inner".into(),
                        count: 2,
                    },
                ),
                field(
                    "items",
                    0x20,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: None,
                        count: CountSource::Field("count".into()),
                        element_size: Some(4),
                    }),
                ),
                field(
                    "lanes",
                    0x28,
                    FieldRepr::ScalarArray {
                        element: Scalar::Int(IntWidth::U8),
                        count: 4,
                    },
                ),
            ],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_plan_names_embedded_fields_by_path() {
        let pack = synthetic();
        let plan = pack.plan("Outer").unwrap();
        assert_eq!(plan.field("inner.b").map(|f| f.offset), Some(0x0C));
        assert_eq!(plan.field("pair[1].a").map(|f| f.offset), Some(0x18));
        assert_eq!(plan.field("lanes[3]").map(|f| f.offset), Some(0x2B));
        assert!(plan.field("inner.c").is_none());
        assert_eq!(plan.own_fields().count(), 6);
    }

    #[test]
    fn a_slot_reads_and_writes_at_its_offset() {
        let pack = synthetic();
        let plan = pack.plan("Outer").unwrap();
        let weight = plan.slot::<f32>("weight").unwrap();
        let b = plan.slot::<f32>("pair[1].b").unwrap();
        let mut bytes = vec![0u8; 0x40];
        {
            let mut rec = plan.record_mut(&mut bytes, 0x10).unwrap();
            rec.set(weight, 2.5).unwrap();
            rec.set(b, -1.0).unwrap();
        }
        let rec = plan.record(&bytes, 0x10).unwrap();
        assert_eq!(rec.get(weight), Some(2.5));
        assert_eq!(rec.get(b), Some(-1.0));
        assert_eq!(&bytes[0x14..0x18], &2.5f32.to_le_bytes());
    }

    #[test]
    fn a_record_that_does_not_fit_is_refused_once() {
        let pack = synthetic();
        let plan = pack.plan("Outer").unwrap();
        assert!(plan.record(&[0u8; 0x2F], 0).is_none());
        assert!(plan.record(&[0u8; 0x40], 0x11).is_none());
        assert!(plan.record(&[0u8; 0x40], 0x10).is_some());
    }

    #[test]
    fn a_missing_field_names_the_pack_the_type_and_the_field() {
        let pack = synthetic();
        let plan = pack.plan("Outer").unwrap();
        let err = plan.slot::<f32>("zoom_level").unwrap_err();
        assert_eq!(
            err,
            PlanError::NoField {
                pack: "synthetic/t0".into(),
                type_name: "Outer".into(),
                field: "zoom_level".into(),
            }
        );
        assert_eq!(
            err.to_string(),
            "the synthetic/t0 pack does not describe Outer.zoom_level"
        );
    }

    #[test]
    fn a_slot_of_the_wrong_kind_is_refused_when_asked_for() {
        let pack = synthetic();
        let plan = pack.plan("Outer").unwrap();
        assert!(matches!(
            plan.slot::<u32>("weight"),
            Err(PlanError::WrongKind { .. })
        ));
        assert!(matches!(
            plan.slot::<u16>("count"),
            Err(PlanError::WrongKind { .. })
        ));
        assert!(matches!(
            plan.slot::<u64>("items"),
            Err(PlanError::WrongKind { .. })
        ));
    }

    #[test]
    fn a_count_is_structural_and_its_slot_refuses_a_write() {
        let pack = synthetic();
        let plan = pack.plan("Outer").unwrap();
        let count = plan.slot::<u32>("count").unwrap();
        assert!(!count.is_writable());
        let mut bytes = vec![0u8; 0x30];
        let mut rec = plan.record_mut(&mut bytes, 0).unwrap();
        assert_eq!(rec.set(count, 3), Err(WriteError::Structural));
    }

    #[test]
    fn an_incoherent_type_gets_no_slots() {
        let mut pack = synthetic();
        pack.types[0].fields[1].offset = 6; // `b` now ends past `Inner`'s eight bytes
        let plan = pack.plan("Inner").unwrap();
        assert!(!plan.is_coherent());
        assert!(matches!(
            plan.slot::<u16>("a"),
            Err(PlanError::Incoherent { .. })
        ));
    }

    #[test]
    fn a_nested_field_past_its_outer_type_is_refused() {
        let mut pack = synthetic();
        pack.types[0].size = 0x40; // `Inner` claims more than `Outer` gives `pair[1]`
        pack.types[0].fields[1].offset = 0x20;
        let plan = pack.plan("Outer").unwrap();
        // `Outer` itself is incoherent now, so the refusal is the coherence one.
        assert!(plan.slot::<f32>("pair[1].b").is_err());
    }

    #[test]
    fn copy_ranges_cover_every_scalar_and_skip_pointers() {
        let pack = synthetic();
        let plan = pack.plan("Outer").unwrap();
        assert_eq!(
            plan.copy_ranges(),
            &[
                (0x00, 0x0A),
                (0x0C, 0x12),
                (0x14, 0x1A),
                (0x1C, 0x20),
                (0x28, 0x2C)
            ]
        );
    }

    #[test]
    fn a_call_site_memo_follows_the_plan_it_is_handed() {
        fn weight_of(plan: &TypePlan, bytes: &[u8]) -> Result<Option<f32>, PlanError> {
            let slot = crate::at!(plan, f32, "weight")?;
            Ok(plan.record(bytes, 0).and_then(|r| r.get(slot)))
        }
        let a = synthetic();
        let mut b = synthetic();
        b.types[1].fields[1].offset = 0x2C; // the same name, somewhere else
        let mut bytes = vec![0u8; 0x30];
        bytes[4..8].copy_from_slice(&1.0f32.to_le_bytes());
        bytes[0x2C..0x30].copy_from_slice(&2.0f32.to_le_bytes());

        let (pa, pb) = (a.plan("Outer").unwrap(), b.plan("Outer").unwrap());
        for _ in 0..3 {
            assert_eq!(weight_of(pa, &bytes), Ok(Some(1.0)));
            assert_eq!(weight_of(pb, &bytes), Ok(Some(2.0)));
        }
    }

    #[test]
    fn a_cloned_pack_builds_its_own_plans() {
        let pack = synthetic();
        let first = pack.plan("Outer").unwrap().epoch();
        let mut copy = pack.clone();
        copy.types[1].fields[1].offset = 0x2C;
        assert_ne!(copy.plan("Outer").unwrap().epoch(), first);
        assert_eq!(
            copy.plan("Outer").unwrap().field("weight").unwrap().offset,
            0x2C
        );
        assert_eq!(
            pack.plan("Outer").unwrap().field("weight").unwrap().offset,
            4
        );
    }
}
