//! An RBM type table derived from a value tree and a schema alone.

use std::collections::HashMap;

use crate::meta::binary::layout::RbmLayout;
use crate::meta::binary::pso::schema::align_up;
use crate::meta::binary::rbm::schema::{
    PsoType, RbmClass, RbmClassEntry, RbmEnum, RbmEnumValue, RbmModel,
};
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::{MetaInstance, Value};
use crate::meta::schema::{ArrayKind, BitsetStorage, FieldType, SchemaLibrary, StringKind};

use super::{
    fixed_count, name_hash, unsized_fixed_lengths, UnsizedLengths, MAX_DEPTH, VTABLE_BYTES,
};

pub(super) const ARRAY_INFO_NAME_HASH: u32 = 0x100;
pub(super) const POINTER_BYTES: usize = 8;
pub(super) const ARRAY_HEADER_BYTES: usize = 16;

/// Lay `instance` out as an RBM resource: a type table for the types it
/// uses, a block allocation sized for the values it holds, a page image, and
/// an invented RSC7 header.
///
/// ⚠ The result is a pure function of `(library, instance)` — `donor`
/// reaches nothing but the `signature` word of a class or enum it names, and
/// `a_donor_changes_nothing_but_the_signature_words` is what holds that.
/// Anything else carried from the document a caller happens to have would
/// stop `import(export(x))` being a fixed point.
pub fn rbm_layout_for_instance(
    library: &SchemaLibrary,
    instance: &MetaInstance,
    donor: Option<&RbmModel>,
) -> Result<RbmLayout> {
    let mut builder = Builder {
        library,
        donor,
        classes: Vec::new(),
        enums: Vec::new(),
        class_index: HashMap::new(),
        enum_index: HashMap::new(),
        in_progress: Vec::new(),
        symbolic_sizes: unsized_fixed_lengths(library, instance)?,
    };
    let root_hash = builder.add_struct(&instance.type_name)?;
    builder.reach_pointer_targets(&instance.type_name, instance, 0)?;

    super::rbm_place::place(builder.into_model(), root_hash, instance)
}

/// Lay out `root_type` with every heap member empty.
pub fn rbm_layout_for(library: &SchemaLibrary, root_type: &str) -> Result<RbmLayout> {
    rbm_layout_for_instance(library, &MetaInstance::new(root_type), None)
}

struct Builder<'a> {
    library: &'a SchemaLibrary,
    donor: Option<&'a RbmModel>,
    /// ⚠ **These two `Vec`s are the output order**, and the maps beside them
    /// are lookup only. Ordering a type table by a `HashMap`'s iteration
    /// would make two runs of the same input disagree.
    classes: Vec<RbmClass>,
    enums: Vec<RbmEnum>,
    class_index: HashMap<u32, usize>,
    enum_index: HashMap<u32, usize>,
    in_progress: Vec<String>,
    symbolic_sizes: UnsizedLengths,
}

impl Builder<'_> {
    fn into_model(self) -> Model {
        Model {
            classes: self.classes,
            enums: self.enums,
        }
    }

    /// The `signature` word a shipped file would carry for this type.
    ///
    /// ⚠ **Nothing in this tree can compute one.** It is a value the game's
    /// own parser holds for the struct it compiled, and the corpus is the
    /// only place a tool can read it: 20,511 of 20,511 classes and 5,636 of
    /// 5,636 enums in the staged metadata corpus carry a non-zero one, and a
    /// given class carries the same one in every document that holds it (30
    /// of 31 distinct classes; the one that varies has two field sets).
    /// CodeWalker does not derive it either — `MetaTypes.GetStructureInfo`
    /// returns a harvested table. So a synthesiser writes zero unless it is
    /// handed a document that already names the type.
    fn signature_of_class(&self, hash: u32) -> u32 {
        self.donor
            .and_then(|m| m.class_for(hash))
            .map(|c| c.signature)
            .unwrap_or(0)
    }

    fn signature_of_enum(&self, hash: u32) -> u32 {
        self.donor
            .and_then(|m| m.enum_for(hash))
            .map(|e| e.signature)
            .unwrap_or(0)
    }

    fn add_struct(&mut self, type_name: &str) -> Result<u32> {
        let library = self.library;
        let resolved = library.resolve_any(type_name).ok_or_else(|| {
            MetaError::Other(format!("synth: '{type_name}' is not in the schema"))
        })?;
        let canonical = resolved.def.name.as_str();
        let hash = name_hash(canonical);
        if self.class_index.contains_key(&hash) {
            return Ok(hash);
        }
        if self.in_progress.iter().any(|t| t == canonical) {
            return Err(MetaError::Other(format!(
                "synth: '{canonical}' contains itself by value, which has no finite layout"
            )));
        }
        self.in_progress.push(canonical.to_string());

        let fields: Vec<(String, FieldType)> = resolved
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();

        let mut entries: Vec<RbmClassEntry> = Vec::new();
        let (mut offset, mut max_align) = if resolved.def.simple {
            (0, 1)
        } else {
            (VTABLE_BYTES as u32, POINTER_BYTES)
        };

        for (name, ty) in &fields {
            if let FieldType::Pad(n) = ty {
                offset += n;
                continue;
            }
            let plan = self.plan_member(&mut entries, canonical, name, ty)?;
            let align = plan.align.max(1);
            offset = align_up(offset as usize, align) as u32;
            max_align = max_align.max(align);
            if offset > MAX_MEMBER_OFFSET {
                return Err(MetaError::Other(format!(
                    "synth: '{canonical}' is larger than the {MAX_MEMBER_OFFSET} bytes a \
                     member offset can address by the time it reaches '{name}'"
                )));
            }
            entries.push(
                RbmClassEntry::synthesised(
                    name_hash(name),
                    offset,
                    plan.data_type,
                    plan.union_ah,
                    plan.union_ch,
                )
                .with_union_9h(plan.union_9h),
            );
            offset += plan.size as u32;
        }

        let data_size = align_up(offset as usize, max_align) as u32;
        self.in_progress.pop();
        let index = self.classes.len();
        self.classes.push(RbmClass {
            name_hash: hash,
            signature: self.signature_of_class(hash),
            flags: 0,
            align_power: max_align.trailing_zeros() as u8,
            data_size,
            major_version: 0,
            entries,
        });
        self.class_index.insert(hash, index);
        Ok(hash)
    }

    fn add_enum(&mut self, type_name: &str) -> Result<u32> {
        let hash = name_hash(type_name);
        if self.enum_index.contains_key(&hash) {
            return Ok(hash);
        }
        let library = self.library;
        let def = library.enum_(type_name).ok_or_else(|| {
            MetaError::Other(format!("synth: enum '{type_name}' is not in the schema"))
        })?;
        let mut next = 0i64;
        let values = def
            .values
            .iter()
            .map(|v| {
                let value = v.value.unwrap_or(next);
                next = value + 1;
                RbmEnumValue {
                    name_hash: name_hash(&v.name),
                    value: value as i32,
                }
            })
            .collect();
        let index = self.enums.len();
        self.enums.push(RbmEnum {
            name_hash: hash,
            signature: self.signature_of_enum(hash),
            flags: 0,
            values,
        });
        self.enum_index.insert(hash, index);
        Ok(hash)
    }

    fn plan_member(
        &mut self,
        entries: &mut Vec<RbmClassEntry>,
        owner: &str,
        name: &str,
        ty: &FieldType,
    ) -> Result<MemberPlan> {
        let simple = |data_type: PsoType| -> Result<MemberPlan> {
            let size = data_type.fixed_size().ok_or_else(|| {
                MetaError::Other(format!("synth: '{owner}.{name}' has no fixed size"))
            })?;
            Ok(MemberPlan {
                data_type,
                union_9h: 0,
                union_ah: 0,
                union_ch: 0,
                size,
                align: natural_align(data_type),
            })
        };

        Ok(match ty {
            FieldType::Bool => simple(PsoType::Bool)?,
            FieldType::S8 => simple(PsoType::S8)?,
            FieldType::U8 => simple(PsoType::U8)?,
            FieldType::S16 => simple(PsoType::S16)?,
            FieldType::U16 => simple(PsoType::U16)?,
            FieldType::S32 => simple(PsoType::S32)?,
            FieldType::U32 | FieldType::Color32 => simple(PsoType::U32)?,
            FieldType::S64 => simple(PsoType::S64)?,
            FieldType::U64 => simple(PsoType::U64)?,
            FieldType::Float => simple(PsoType::Float)?,
            FieldType::Float16 => simple(PsoType::Float16)?,
            FieldType::Double => simple(PsoType::Double)?,
            FieldType::Vector2 => simple(PsoType::Vec2)?,
            FieldType::Vec2V => simple(PsoType::Vec2V)?,
            FieldType::Vector3 | FieldType::Vec3V => simple(PsoType::Vec3V)?,
            FieldType::Vector4 | FieldType::Vec4V => simple(PsoType::Vec4V)?,
            FieldType::Matrix33 | FieldType::Mat33V => simple(PsoType::Mat33V)?,
            FieldType::Matrix34 | FieldType::Mat34V => simple(PsoType::Mat34V)?,
            FieldType::Matrix44 | FieldType::Mat44V => simple(PsoType::Mat44V)?,
            FieldType::String(
                StringKind::AtHashString | StringKind::AtHashValue | StringKind::AtFinalHashString,
            ) => simple(PsoType::StringHash)?,
            FieldType::String(StringKind::Plain) => simple(PsoType::StringPointer64)?,
            FieldType::String(StringKind::AtString) => simple(PsoType::AtString64)?,
            FieldType::Enum(enum_name) => {
                let hash = self.add_enum(enum_name)?;
                MemberPlan {
                    data_type: PsoType::Enum32,
                    union_9h: 0,
                    union_ah: 0,
                    union_ch: hash,
                    size: 4,
                    align: 4,
                }
            }
            FieldType::Bitset { storage, values } => {
                let _ = self.add_enum(values);
                let (data_type, bits) = match storage {
                    BitsetStorage::Fixed8 => (PsoType::Bitset8, 8u16),
                    BitsetStorage::Fixed16 => (PsoType::Bitset16, 16),
                    BitsetStorage::Fixed32 | BitsetStorage::Variable => (PsoType::Bitset32, 32),
                    BitsetStorage::Fixed64 => (PsoType::AtBitset64, 64),
                };
                let bits = super::shipped_bit_count(owner, name).unwrap_or(bits);
                MemberPlan {
                    data_type,
                    union_9h: 0,
                    union_ah: bits,
                    union_ch: name_hash(values),
                    size: data_type.fixed_size().unwrap_or(ARRAY_HEADER_BYTES),
                    align: natural_align(data_type),
                }
            }
            FieldType::Struct(struct_name) => {
                let hash = self.add_struct(struct_name)?;
                let class = self
                    .class_index
                    .get(&hash)
                    .map(|i| &self.classes[*i])
                    .ok_or_else(|| {
                        MetaError::Other(format!("synth: '{struct_name}' was not laid out"))
                    })?;
                MemberPlan {
                    data_type: PsoType::Struct,
                    union_9h: 0,
                    union_ah: 0,
                    union_ch: hash,
                    size: class.data_size as usize,
                    align: 1usize << class.align_power.min(6),
                }
            }
            FieldType::Pointer { base, .. } => MemberPlan {
                data_type: PsoType::Pointer64,
                union_9h: 0,
                union_ah: 0,
                union_ch: name_hash(base),
                size: POINTER_BYTES,
                align: POINTER_BYTES,
            },
            FieldType::Array { elem, kind } => {
                let sidecar = self.push_sidecar(entries, owner, name, elem)?;
                if matches!(
                    entries[usize::from(sidecar)].data_type,
                    PsoType::ArrayMember | PsoType::AtFixedArray
                ) {
                    return Err(MetaError::Other(format!(
                        "synth: '{owner}.{name}' is an array of inline arrays, which no staged \
                         type table holds to encode against"
                    )));
                }
                match kind {
                    ArrayKind::Pointer { size_var, align } => {
                        return pointer_array_plan(
                            entries,
                            owner,
                            name,
                            sidecar,
                            size_var.as_deref(),
                            *align,
                        )
                    }
                    ArrayKind::AtArray | ArrayKind::AtPtrArray => MemberPlan {
                        data_type: PsoType::AtArray64,
                        union_9h: 0,
                        union_ah: sidecar,
                        union_ch: 0,
                        size: ARRAY_HEADER_BYTES,
                        align: POINTER_BYTES,
                    },
                    ArrayKind::AtFixedArray(declared) | ArrayKind::Member(declared) => {
                        let count = &fixed_count(*declared, &self.symbolic_sizes, owner, name)?;
                        if *count > u32::from(u16::MAX) {
                            return Err(MetaError::Other(format!(
                                "synth: '{owner}.{name}' declares {count} elements, more than \
                                 the count field holds"
                            )));
                        }
                        let elem_desc = entries[sidecar as usize];
                        let stride = self.stride_of(&elem_desc).ok_or_else(|| {
                            MetaError::Other(format!(
                                "synth: '{owner}.{name}' has an element type with no fixed size"
                            ))
                        })?;
                        MemberPlan {
                            data_type: PsoType::ArrayMember,
                            union_9h: 0,
                            union_ah: sidecar,
                            union_ch: *count,
                            size: stride * *count as usize,
                            align: self.align_of(&elem_desc),
                        }
                    }
                }
            }
            FieldType::Map { key, value } => {
                let key_idx = self.push_sidecar(entries, owner, name, key)?;
                let value_idx = self.push_sidecar(entries, owner, name, value)?;
                MemberPlan {
                    data_type: PsoType::AtBinaryMap64,
                    union_9h: 0,
                    union_ah: 0,
                    union_ch: (u32::from(value_idx) << 16) | u32::from(key_idx),
                    size: ARRAY_HEADER_BYTES,
                    align: POINTER_BYTES,
                }
            }
            other => {
                return Err(MetaError::Other(format!(
                    "synth: '{owner}.{name}' has type {other:?}, which synthesis does not \
                     lay out; load an existing file to obtain its layout instead"
                )))
            }
        })
    }

    fn push_sidecar(
        &mut self,
        entries: &mut Vec<RbmClassEntry>,
        owner: &str,
        name: &str,
        ty: &FieldType,
    ) -> Result<u16> {
        let plan = self.plan_member(entries, owner, name, ty)?;
        let index = entries.len();
        if index > usize::from(u16::MAX) {
            return Err(MetaError::Other(format!(
                "synth: '{owner}' needs more member descriptors than an index holds"
            )));
        }
        entries.push(
            RbmClassEntry::synthesised(
                ARRAY_INFO_NAME_HASH,
                0,
                plan.data_type,
                plan.union_ah,
                plan.union_ch,
            )
            .with_union_9h(plan.union_9h),
        );
        Ok(index as u16)
    }

    fn stride_of(&self, elem: &RbmClassEntry) -> Option<usize> {
        match elem.data_type {
            PsoType::Struct => self
                .class_index
                .get(&elem.referent_hash())
                .map(|i| self.classes[*i].data_size as usize),
            PsoType::StringMember | PsoType::WideStringMember => Some(elem.string_count() as usize),
            other => other.fixed_size(),
        }
    }

    fn align_of(&self, elem: &RbmClassEntry) -> usize {
        match elem.data_type {
            PsoType::Struct => self
                .class_index
                .get(&elem.referent_hash())
                .map(|i| 1usize << self.classes[*i].align_power.min(6))
                .unwrap_or(1),
            other => natural_align(other),
        }
    }

    /// Add the types a pointer actually points at.
    ///
    /// ⚠ A pointer's target is polymorphic, so its concrete type is in the
    /// value tree rather than in the declaration.
    fn reach_pointer_targets(
        &mut self,
        type_name: &str,
        inst: &MetaInstance,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(MetaError::Other(format!(
                "synth: '{type_name}' nests deeper than {MAX_DEPTH} levels"
            )));
        }
        let library = self.library;
        let Some(resolved) = library.resolve_any(type_name) else {
            return Ok(());
        };
        let fields: Vec<(String, FieldType)> = resolved
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();
        for (name, ty) in &fields {
            let Some(value) = inst.get(name) else {
                continue;
            };
            self.reach_in_value(ty, value, depth + 1)?;
        }
        Ok(())
    }

    fn reach_in_value(&mut self, ty: &FieldType, value: &Value, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(MetaError::Other(format!(
                "synth: the value tree nests deeper than {MAX_DEPTH} levels"
            )));
        }
        match (ty, value) {
            (FieldType::Struct(name), Value::Struct(inner)) => {
                let name = name.clone();
                self.reach_pointer_targets(&name, inner, depth)?;
            }
            (FieldType::Pointer { .. }, Value::Pointer(Some(inner))) => {
                self.add_struct(&inner.type_name)?;
                let target = inner.type_name.clone();
                self.reach_pointer_targets(&target, inner, depth)?;
            }
            (FieldType::Array { elem, .. }, Value::Array(items)) => {
                for item in items {
                    self.reach_in_value(elem, item, depth + 1)?;
                }
            }
            (FieldType::Map { key, value: val }, Value::Map(pairs)) => {
                for (k, v) in pairs {
                    self.reach_in_value(key, k, depth + 1)?;
                    self.reach_in_value(val, v, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// The type table, in the order it will be written.
pub(super) struct Model {
    pub classes: Vec<RbmClass>,
    pub enums: Vec<RbmEnum>,
}

impl Model {
    pub fn class_for(&self, hash: u32) -> Option<&RbmClass> {
        self.classes.iter().find(|c| c.name_hash == hash)
    }
}

struct MemberPlan {
    data_type: PsoType,
    union_9h: u8,
    union_ah: u16,
    union_ch: u32,
    size: usize,
    align: usize,
}

/// A `T*` counted by an earlier member, described the way the 670 shipped
/// occluder maps describe `OccludeModel::verts`: the element descriptor in
/// `union_ah`, the counting member's index in `union_ch`, and the PSC's
/// `align=` as a power of two in the flag byte.
///
/// ⚠ **Only a run of bytes.** The reader hands any pointer array back as the
/// bytes it points at, so a wider element would come back as its bytes and
/// never as itself.
fn pointer_array_plan(
    entries: &[RbmClassEntry],
    owner: &str,
    name: &str,
    sidecar: u16,
    size_var: Option<&str>,
    align: u32,
) -> Result<MemberPlan> {
    let elem = entries[usize::from(sidecar)];
    if elem.data_type.fixed_size() != Some(1) {
        return Err(MetaError::Other(format!(
            "synth: '{owner}.{name}' points at a run of {:?}, and a pointer array reads back \
             as bytes, so only a run of bytes can be laid out",
            elem.data_type
        )));
    }
    let size_var = size_var.ok_or_else(|| {
        MetaError::Other(format!(
            "synth: '{owner}.{name}' is a pointer array whose declaration names no member \
             that counts it"
        ))
    })?;
    let counted_by = entries
        .iter()
        .position(|e| e.name_hash == name_hash(size_var))
        .ok_or_else(|| {
            MetaError::Other(format!(
                "synth: '{owner}.{name}' is counted by '{size_var}', which is not declared \
                 before it"
            ))
        })?;
    if !align.is_power_of_two() && align != 0 {
        return Err(MetaError::Other(format!(
            "synth: '{owner}.{name}' asks for {align}-byte alignment, which a power of two \
             cannot state"
        )));
    }
    Ok(MemberPlan {
        data_type: PsoType::ArrayPointer64WithCount,
        union_9h: if align == 0 {
            0
        } else {
            align.trailing_zeros() as u8
        },
        union_ah: sidecar,
        union_ch: counted_by as u32,
        size: POINTER_BYTES,
        align: POINTER_BYTES,
    })
}

/// A member offset is a `u32` in the file, but a class larger than this is a
/// class a twenty-bit data offset could not reach inside either.
const MAX_MEMBER_OFFSET: u32 = 0xF_FFFF;

/// The boundary a value of `t` has to start on.
///
/// ⚠ **Not derived from the size.** A `Vec2` is eight bytes of two floats and
/// aligns to four; a `Vec3V` is sixteen bytes of a SIMD register and aligns
/// to sixteen. Deriving alignment from width gets both wrong.
pub(super) fn natural_align(t: PsoType) -> usize {
    use PsoType::*;
    match t {
        Bool | S8 | U8 | Enum8 | Bitset8 | StringMember | WideStringMember => 1,
        S16 | U16 | Float16 | Enum16 | Bitset16 => 2,
        S32 | U32 | Float | StructId | StringHash | PartialStringHash | LiteralStringHash
        | Enum32 | Bitset32 | Vec2 => 4,
        BoolV | VecBoolV | ScalarV | Vec2V | Vec3 | Vec3V | Vec4V | Mat33V | Mat43 | Mat34V
        | Mat44V => 16,
        _ => POINTER_BYTES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_is_not_the_width() {
        assert_eq!(PsoType::Vec2.fixed_size(), Some(8));
        assert_eq!(natural_align(PsoType::Vec2), 4);
        assert_eq!(PsoType::Vec3V.fixed_size(), Some(16));
        assert_eq!(natural_align(PsoType::Vec3V), 16);
    }

    #[test]
    fn a_member_offset_stays_inside_what_a_pointer_can_reach() {
        assert_eq!(MAX_MEMBER_OFFSET, super::super::MAX_DATA_OFFSET as u32);
    }
}
