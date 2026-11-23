//! A type table and block allocation derived from a value tree alone.

use std::collections::HashMap;

mod place;
mod rbm;
mod rbm_place;

pub use place::{MAX_BLOCK_COUNT, MAX_DATA_OFFSET};
pub use rbm::{rbm_layout_for, rbm_layout_for_instance};

use crate::meta::binary::layout::PsoLayout;
use crate::meta::binary::pso::schema::{
    align_up, string_subtype, struct_subtype, PsoDataType, PsoEnumEntry, PsoEnumInfo,
    PsoSchemaSection, PsoStructEntry, PsoStructInfo, ARRAY_HEADER_BYTES, ARRAY_INFO_NAME_HASH,
    POINTER_BYTES, STRUCT_ALIGN,
};
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::{MetaInstance, Value};
use crate::meta::schema::{ArrayKind, BitsetStorage, FieldType, SchemaLibrary, StringKind};

/// How deep a value tree may nest before synthesis or a reader gives up.
pub(crate) const MAX_DEPTH: usize = 128;

/// The vtable word a class that is not `simple` starts with.
///
/// ⚠ **An RBM class without it is a different size with every member eight
/// bytes early**, and the game crashes placing a file laid out that way. The
/// word itself is zero in a synthesised file: the game fills it in.
pub const VTABLE_BYTES: usize = 8;

/// The hash a PSO file stores for a type or a field called `name`.
///
/// ⚠ `hash_XXXXXXXX` is read as the number, not hashed as text. The reader
/// spells a name it cannot resolve that way, so a document that came from a
/// file with no name for its root arrives back here under that spelling and
/// has to reach the same type.
///
/// ⚠ A nested class is hashed as `A__B`, the string the game hashes, though
/// the library spells it `A::B`; hashed as spelt, a synthesised table names
/// a type no file or parser has.
pub fn name_hash(name: &str) -> u32 {
    crate::meta::binary::hash_index::spelled_hash(name).unwrap_or_else(|| {
        crate::literal_string_hash(crate::meta::consistent_struct_short_name(name).as_bytes())
    })
}

/// Lay out `root_type` with every heap member empty.
pub fn pso_layout_for(library: &SchemaLibrary, root_type: &str) -> Result<PsoLayout> {
    pso_layout_for_instance(library, &MetaInstance::new(root_type))
}

/// Lay out `instance`: a type table for the types it uses and a block
/// allocation sized for the values it holds.
///
/// ⚠ The result is a pure function of `(library, instance)`. That is what makes
/// `import(export(x))` a fixed point — an allocation that depended on the
/// document it came from would move under an edit that changed nothing.
pub fn pso_layout_for_instance(
    library: &SchemaLibrary,
    instance: &MetaInstance,
) -> Result<PsoLayout> {
    let mut builder = SchemaBuilder {
        library,
        section: PsoSchemaSection::default(),
        in_progress: Vec::new(),
        symbolic_sizes: unsized_fixed_lengths(library, instance)?,
    };
    let root_hash = builder.add_struct(&instance.type_name)?;
    builder.reach_pointer_targets(&instance.type_name, instance, 0)?;

    place::place(builder.section, root_hash, instance)
}

/// `(owner type, member)` to the element count a document holds there.
pub(crate) type UnsizedLengths = HashMap<(String, String), u32>;

/// The length of every fixed array whose declaration states none, taken
/// from the value tree.
///
/// ⚠ **A zero count is a size the PSC spelled symbolically**, as
/// `size="NUM_ANCHORS"` rather than a number, and the schema cannot evaluate
/// it. A fixed array carries every element whatever it holds, so the
/// document states the length the declaration leaves out — measured over the
/// staged corpus, `CPedSelectionSet::propAnchorId` alone is this in 1,832 of
/// 2,338 RBM documents. Two instances of one type that disagree are refused
/// rather than padded, because a type has one layout.
pub(crate) fn unsized_fixed_lengths(
    library: &SchemaLibrary,
    instance: &MetaInstance,
) -> Result<UnsizedLengths> {
    let mut found = UnsizedLengths::new();
    collect_unsized(library, &instance.type_name, instance, &mut found, 0)?;
    Ok(found)
}

fn collect_unsized(
    library: &SchemaLibrary,
    type_name: &str,
    inst: &MetaInstance,
    found: &mut UnsizedLengths,
    depth: usize,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(MetaError::Other(format!(
            "synth: '{type_name}' nests deeper than {MAX_DEPTH} levels"
        )));
    }
    let Some(resolved) = library.resolve_any(type_name) else {
        return Ok(());
    };
    let owner = resolved.def.name.clone();
    for field in &resolved.fields {
        let Some(value) = inst.get(&field.name) else {
            continue;
        };
        if let (
            FieldType::Array {
                kind: ArrayKind::AtFixedArray(0) | ArrayKind::Member(0),
                ..
            },
            Value::Array(items),
        ) = (&field.ty, value)
        {
            let len = u32::try_from(items.len()).unwrap_or(u32::MAX);
            let key = (owner.clone(), field.name.clone());
            match found.get(&key) {
                Some(seen) if *seen != len => {
                    return Err(MetaError::Other(format!(
                        "synth: '{owner}.{}' is a fixed array that declares no size, and the \
                         document holds it at both {seen} and {len} elements",
                        field.name
                    )));
                }
                Some(_) => {}
                None => {
                    found.insert(key, len);
                }
            }
        }
        collect_unsized_in(library, &field.ty, value, found, depth + 1)?;
    }
    Ok(())
}

fn collect_unsized_in(
    library: &SchemaLibrary,
    ty: &FieldType,
    value: &Value,
    found: &mut UnsizedLengths,
    depth: usize,
) -> Result<()> {
    match (ty, value) {
        (FieldType::Struct(name), Value::Struct(inner)) => {
            collect_unsized(library, name, inner, found, depth)
        }
        (FieldType::Pointer { .. }, Value::Pointer(Some(inner))) => {
            collect_unsized(library, &inner.type_name, inner, found, depth)
        }
        (FieldType::Array { elem, .. }, Value::Array(items)) => items
            .iter()
            .try_for_each(|item| collect_unsized_in(library, elem, item, found, depth + 1)),
        (FieldType::Map { key, value: val }, Value::Map(pairs)) => {
            pairs.iter().try_for_each(|(k, v)| {
                collect_unsized_in(library, key, k, found, depth + 1)?;
                collect_unsized_in(library, val, v, found, depth + 1)
            })
        }
        _ => Ok(()),
    }
}

/// The count shipped type tables give a fixed array the PSC sizes
/// symbolically, as `(owner, member, count)`.
///
/// ⚠ **Measured, and a suite holds it to the measurement**:
/// `every_shipped_symbolic_size_is_what_the_type_tables_say` fails if an
/// entry disagrees with any staged document that states the member, or is
/// stated by none. It exists for a type an empty array names, which has no
/// instance to count — 1,831 of 2,338 staged RBM documents declare
/// `CPedSelectionSet` that way.
pub const SHIPPED_SYMBOLIC_SIZES: &[(&str, &str, u32)] = &[
    ("CJunctionTemplateArray", "AutoJunctionAdjustments", 8),
    ("CPedSelectionSet", "propAnchorId", 6),
    ("CPedSelectionSet", "propDrawableId", 6),
    ("CPedSelectionSet", "propTexId", 6),
    ("CProceduralInfo", "procTagTable", 255),
    ("CTargettingDifficultyInfo", "CurveSets", 4),
    ("CVehicleModelInfoVarGlobal", "Wheels", 13),
];

/// The bit count shipped type tables give a bitset whose enum names fewer bits than its storage
/// holds, as `(owner, member, bits)`.
///
/// ⚠ A storage width cannot say it: `CPedPropMetaData::renderFlags` is one byte and states 3, the
/// flags `ePropRenderFlags` names, where `CComponentInfo::pedXml_vfxComps` states its whole 16.
/// `every_declared_ped_class_is_the_table_the_documents_state` holds it to the 1,418 documents.
pub const SHIPPED_BIT_COUNTS: &[(&str, &str, u16)] = &[("CPedPropMetaData", "renderFlags", 3)];

pub(crate) fn shipped_bit_count(owner: &str, member: &str) -> Option<u16> {
    SHIPPED_BIT_COUNTS
        .iter()
        .find(|(o, m, _)| *o == owner && *m == member)
        .map(|(.., bits)| *bits)
}

/// The count a fixed array is laid out at: its declaration's, what shipped
/// type tables give a symbolic one, or the document's.
pub(crate) fn fixed_count(
    declared: u32,
    symbolic_sizes: &UnsizedLengths,
    owner: &str,
    name: &str,
) -> Result<u32> {
    if declared != 0 {
        return Ok(declared);
    }
    if let Some((_, _, count)) = SHIPPED_SYMBOLIC_SIZES
        .iter()
        .find(|(o, m, _)| *o == owner && *m == name)
    {
        return Ok(*count);
    }
    match symbolic_sizes.get(&(owner.to_string(), name.to_string())) {
        Some(0) | None => Err(MetaError::Other(format!(
            "synth: '{owner}.{name}' is a fixed array that declares no size and the document \
             holds no elements there, so there is nothing to lay out"
        ))),
        Some(n) => Ok(*n),
    }
}

struct SchemaBuilder<'a> {
    library: &'a SchemaLibrary,
    section: PsoSchemaSection,
    in_progress: Vec<String>,
    symbolic_sizes: UnsizedLengths,
}

impl SchemaBuilder<'_> {
    fn add_struct(&mut self, type_name: &str) -> Result<u32> {
        let library = self.library;
        let resolved = library.resolve_any(type_name).ok_or_else(|| {
            MetaError::Other(format!("synth: '{type_name}' is not in the schema"))
        })?;
        let canonical = resolved.def.name.as_str();
        let hash = name_hash(canonical);
        if self.section.struct_for(hash).is_some() {
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

        let mut entries: Vec<PsoStructEntry> = Vec::new();
        let mut offset: u32 = 0;
        let mut max_align: usize = 1;

        for (name, ty) in &fields {
            if let FieldType::Pad(n) = ty {
                offset += n;
                continue;
            }

            let plan = self.plan_member(&mut entries, canonical, name, ty)?;
            let size = plan.size.ok_or_else(|| {
                MetaError::Other(format!("synth: '{canonical}.{name}' has no fixed size"))
            })?;
            let align = plan.align.max(1);
            offset = align_up(offset as usize, align) as u32;
            max_align = max_align.max(align);
            if offset >= PsoStructEntry::MAX_OFFSET {
                return Err(MetaError::Other(format!(
                    "synth: '{canonical}' is larger than the {} bytes a member offset can \
                     address by the time it reaches '{name}'",
                    PsoStructEntry::MAX_OFFSET
                )));
            }
            entries.push(PsoStructEntry::new(
                name_hash(name),
                plan.data_type,
                offset,
                plan.subtype,
                plan.reference_key,
            ));
            offset += size as u32;
        }

        let data_size = align_up(offset as usize, max_align) as u32;
        self.in_progress.pop();
        self.section.insert_struct(PsoStructInfo {
            name_hash: hash,
            flags: 0,
            data_size,
            major_version: 0,
            pad: 0,
            entries,
        });
        Ok(hash)
    }

    fn add_enum(&mut self, type_name: &str) -> Result<u32> {
        let hash = name_hash(type_name);
        if self.section.enum_for(hash).is_some() {
            return Ok(hash);
        }
        let def = self.library.enum_(type_name).ok_or_else(|| {
            MetaError::Other(format!("synth: enum '{type_name}' is not in the schema"))
        })?;
        let mut next = 0i64;
        let entries = def
            .values
            .iter()
            .map(|v| {
                let value = v.value.unwrap_or(next);
                next = value + 1;
                PsoEnumEntry {
                    name_hash: name_hash(&v.name),
                    value: value as i32,
                }
            })
            .collect();
        self.section.insert_enum(PsoEnumInfo {
            name_hash: hash,
            flags: 0,
            entries,
        });
        Ok(hash)
    }

    /// Describe one member, appending any element descriptors it needs to
    /// `entries` first.
    fn plan_member(
        &mut self,
        entries: &mut Vec<PsoStructEntry>,
        owner: &str,
        name: &str,
        ty: &FieldType,
    ) -> Result<MemberPlan> {
        let simple = |data_type: PsoDataType| MemberPlan {
            data_type,
            subtype: 0,
            reference_key: 0,
            size: data_type.fixed_size(),
            align: data_type.fixed_align().unwrap_or(1),
        };

        Ok(match ty {
            FieldType::Bool => simple(PsoDataType::Bool),
            FieldType::S8 => simple(PsoDataType::SByte),
            FieldType::U8 => simple(PsoDataType::UByte),
            FieldType::S16 => simple(PsoDataType::SShort),
            FieldType::U16 => simple(PsoDataType::UShort),
            FieldType::S32 => simple(PsoDataType::SInt),
            FieldType::U32 => simple(PsoDataType::UInt),
            FieldType::S64 => simple(PsoDataType::Int64),
            FieldType::U64 => simple(PsoDataType::UInt64),
            FieldType::Float => simple(PsoDataType::Float),
            FieldType::Float16 => simple(PsoDataType::Float16),
            FieldType::Double => simple(PsoDataType::Double),
            FieldType::Vector2 => simple(PsoDataType::Vector2),
            FieldType::Vector3 => simple(PsoDataType::Vector3),
            FieldType::Vector4 => simple(PsoDataType::Vector4),
            FieldType::Vec2V => simple(PsoDataType::Vec2V),
            FieldType::Vec3V => simple(PsoDataType::Vec3V),
            FieldType::Vec4V => simple(PsoDataType::Vec4V),
            FieldType::Matrix33 | FieldType::Mat33V => simple(PsoDataType::Mat33V),
            FieldType::Matrix34 | FieldType::Mat34V => simple(PsoDataType::Mat34V),
            FieldType::Matrix44 | FieldType::Mat44V => simple(PsoDataType::Mat44V),
            FieldType::Color32 => simple(PsoDataType::UInt),
            FieldType::String(
                StringKind::AtHashString | StringKind::AtHashValue | StringKind::AtFinalHashString,
            ) => MemberPlan {
                data_type: PsoDataType::String,
                subtype: string_subtype::HASH,
                reference_key: 0,
                size: Some(4),
                align: 4,
            },
            FieldType::String(StringKind::Plain) => MemberPlan {
                data_type: PsoDataType::String,
                subtype: string_subtype::POINTER,
                reference_key: 0,
                size: Some(POINTER_BYTES),
                align: POINTER_BYTES,
            },
            FieldType::String(StringKind::AtString) => MemberPlan {
                data_type: PsoDataType::String,
                subtype: string_subtype::CHAR_ARRAY,
                reference_key: 0,
                size: Some(ARRAY_HEADER_BYTES),
                align: POINTER_BYTES,
            },
            FieldType::Enum(enum_name) => {
                let hash = self.add_enum(enum_name)?;
                MemberPlan {
                    data_type: PsoDataType::Enum,
                    subtype: 0,
                    reference_key: hash,
                    size: Some(4),
                    align: 4,
                }
            }
            FieldType::Bitset { storage, values } => {
                let _ = self.add_enum(values);
                let (size, bits) = match storage {
                    BitsetStorage::Fixed8 => (1usize, 8u32),
                    BitsetStorage::Fixed16 => (2, 16),
                    BitsetStorage::Fixed32 | BitsetStorage::Variable => (4, 32),
                    BitsetStorage::Fixed64 => (8, 64),
                };
                MemberPlan {
                    data_type: PsoDataType::Bitset,
                    subtype: 0,
                    reference_key: (bits << 16) | u32::from(PsoStructEntry::NO_MEMBER_INDEX),
                    size: Some(size),
                    align: size,
                }
            }
            FieldType::Struct(struct_name) => {
                let hash = self.add_struct(struct_name)?;
                let size = self
                    .section
                    .struct_for(hash)
                    .map(|s| s.data_size as usize)
                    .unwrap_or(0);
                MemberPlan {
                    data_type: PsoDataType::Structure,
                    subtype: struct_subtype::EMBEDDED,
                    reference_key: hash,
                    size: Some(size),
                    align: STRUCT_ALIGN,
                }
            }
            FieldType::Pointer { base, .. } => MemberPlan {
                data_type: PsoDataType::Structure,
                subtype: struct_subtype::POINTER,
                reference_key: name_hash(base),
                size: Some(POINTER_BYTES),
                align: POINTER_BYTES,
            },
            FieldType::Array { elem, kind } => {
                let sidecar = self.push_sidecar(entries, owner, name, elem)?;
                // ⚠ The reader and the writer size every array element as a
                // heap header, which an inline array is not, and no corpus
                // type table holds one to measure against. Outside the PSC's
                // own unit tests, `CIKDeltaLimits` is the one type declaring it.
                let elem_desc = entries[usize::from(sidecar)];
                if elem_desc.data_type == PsoDataType::Array && elem_desc.count() != 0 {
                    return Err(MetaError::Other(format!(
                        "synth: '{owner}.{name}' is an array of inline arrays, which no staged \
                         type table holds to encode against"
                    )));
                }
                match kind {
                    // ⚠ The PSO reader has no arm for a pointer array either,
                    // so a layout synthesised here could not be read back.
                    ArrayKind::Pointer { .. } => {
                        return Err(MetaError::Other(format!(
                            "synth: '{owner}.{name}' is a pointer to a counted run, which PSO \
                             synthesis has no encoding for; import it as RBM"
                        )))
                    }
                    ArrayKind::AtArray | ArrayKind::AtPtrArray => MemberPlan {
                        data_type: PsoDataType::Array,
                        subtype: 0,
                        reference_key: u32::from(sidecar),
                        size: Some(ARRAY_HEADER_BYTES),
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
                        let stride = crate::meta::binary::pso::schema::element_stride(
                            &self.section,
                            &elem_desc,
                        )
                        .ok_or_else(|| {
                            MetaError::Other(format!(
                                "synth: '{owner}.{name}' has an element type with no fixed size"
                            ))
                        })?;
                        MemberPlan {
                            data_type: PsoDataType::Array,
                            subtype: 0,
                            reference_key: (count << 16) | u32::from(sidecar),
                            size: Some(stride * *count as usize),
                            align: crate::meta::binary::pso::schema::element_align(&elem_desc),
                        }
                    }
                }
            }
            FieldType::Map { key, value } => {
                let key_idx = self.push_sidecar(entries, owner, name, key)?;
                let value_idx = self.push_sidecar(entries, owner, name, value)?;
                MemberPlan {
                    data_type: PsoDataType::Map,
                    subtype: 0,
                    reference_key: (u32::from(key_idx) << 16) | u32::from(value_idx),
                    size: Some(ARRAY_HEADER_BYTES),
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
        entries: &mut Vec<PsoStructEntry>,
        owner: &str,
        name: &str,
        ty: &FieldType,
    ) -> Result<u16> {
        let plan = self.plan_member(entries, owner, name, ty)?;
        let index = entries.len();
        if index >= usize::from(PsoStructEntry::NO_MEMBER_INDEX) {
            return Err(MetaError::Other(format!(
                "synth: '{owner}' needs more than {} member descriptors",
                PsoStructEntry::NO_MEMBER_INDEX
            )));
        }
        entries.push(PsoStructEntry::new(
            ARRAY_INFO_NAME_HASH,
            plan.data_type,
            0,
            plan.subtype,
            plan.reference_key,
        ));
        Ok(index as u16)
    }

    /// Add the types a pointer actually points at.
    ///
    /// ⚠ A pointer's target is polymorphic, so its concrete type is in the
    /// value tree rather than in the declaration. The declared base is a bound,
    /// not an answer: laying out the base would describe a type the document
    /// does not contain and omit the one it does.
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
        for field in &resolved.fields {
            let Some(value) = inst.get(&field.name) else {
                continue;
            };
            self.reach_in_value(&field.ty, value, depth + 1)?;
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
                self.reach_pointer_targets(name, inner, depth)?;
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

struct MemberPlan {
    data_type: PsoDataType,
    subtype: u8,
    reference_key: u32,
    size: Option<usize>,
    align: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::default_library;

    #[test]
    fn a_hex_spelling_names_the_number_rather_than_hashing_it() {
        assert_eq!(name_hash("0xDEADBEEF"), 0xDEAD_BEEF);
        assert_eq!(name_hash("0Xdeadbeef"), 0xDEAD_BEEF);
        // Too short to be the eight-digit spelling, so it is ordinary text.
        assert_eq!(name_hash("0xDEAD"), crate::literal_string_hash(b"0xDEAD"));
        assert_eq!(
            name_hash("CMapData"),
            crate::literal_string_hash(b"CMapData")
        );
    }

    #[test]
    fn a_nested_class_is_hashed_as_the_string_the_game_hashes() {
        assert_eq!(
            name_hash("CPVDrawblData::CPVClothComponentData"),
            crate::literal_string_hash(b"CPVDrawblData__CPVClothComponentData")
        );
        assert_eq!(
            name_hash("CPVDrawblData__CPVClothComponentData"),
            name_hash("CPVDrawblData::CPVClothComponentData")
        );
    }

    #[test]
    fn an_unknown_root_type_is_named_in_the_error() {
        let err =
            pso_layout_for(default_library(), "CDefinitelyNotAThing").expect_err("should refuse");
        assert!(
            err.to_string().contains("CDefinitelyNotAThing"),
            "unhelpful error: {err}"
        );
    }
}
