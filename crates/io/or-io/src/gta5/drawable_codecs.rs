//! The allocations a drawable holds whose regions are computed rather
//! than placed, and so are read and written by code rather than described;
//! each site's owned form is the [`Codec`] a `#[codec]` field holds.
//!
//! ⚠ **A codec is for an allocation whose regions are computed, never for one
//! whose regions are merely undescribed.** The test that tells them apart:
//! is there a constant to write down?

use std::sync::Arc;

use super::skeleton::{bucket_bone_tags, BONE_TAG_ENTRY_SIZE};
use crate::decl::{
    alias_copy, Codec, Cx, Described, Heap, Keyed, Kids, Owned, Plain, Ptr, Rx, Site, SiteMut,
};
use crate::desc::value::{BlobCodecs, BlobReader, BlobSource, ScalarValue};
use crate::desc::{FieldRepr, Image, Pack, PackType, Scalar, Value, ValueError};
use crate::error::Error;
use crate::write::{BlobHeap, BlobWriter, BlobWriters, BuildError, Owner, Placement};

const ENTRIES: &str = "entries";
const BLOB: &str = "Material::entries";
const BYTES: &str = "bytes";
const DATA: &str = "data";

const BONE_TAGS: &str = "bone_tags";
const TABLE: &str = "Skeleton::bone_tags";
const TABLE_ENTRY: &str = "Skeleton::bone_tags::entry";
const BUCKETS: &str = "buckets";
const TAG: &str = "tag";
const INDEX: &str = "index";
const BONES: &str = "bones";
const HEAD_WIDTH: usize = 8;

const BLOB_ALIGN: usize = 16;

/// What a site's owner and its records call the fields a codec reads.
///
/// ⚠ Two sets, because the raw twins `.ydd`, `.yft` and `.ypt` still read
/// through keep their older names, and the owned declarations take the engine's.
#[derive(Debug)]
struct Names {
    material: &'static str,
    entry: &'static str,
    data: &'static str,
    value_count: &'static str,
    entry_count: &'static str,
    texture_count: &'static str,
    hash_offset: &'static str,
    blob_size: &'static str,
    /// What a texture slot that is not the drawable's own dictionary's is read as.
    texture: &'static str,
    skeleton: &'static str,
    bone_tag_slots: &'static str,
    bone_count: &'static str,
    bone_tag: &'static str,
    material_g9: &'static str,
    param_infos_type: &'static str,
    buffer_count: &'static str,
    textures_g9: &'static str,
    other_count: &'static str,
    #[cfg_attr(not(feature = "gen9"), allow(dead_code))]
    sampler_count: &'static str,
    copies: &'static str,
    /// The block's texture slot table and its other slot table, in that order.
    siblings: &'static [&'static str; 2],
    texture_g9: &'static str,
}

const RAW: Names = Names {
    material: "Material",
    entry: "MaterialEntry",
    data: "data",
    value_count: "value_count",
    entry_count: "entry_count",
    texture_count: "texture_count",
    hash_offset: "hash_offset",
    blob_size: "blob_size",
    texture: "Texture",
    skeleton: "Skeleton",
    bone_tag_slots: "bone_tag_slots",
    bone_count: "bone_count",
    bone_tag: TAG,
    material_g9: "Material",
    param_infos_type: "ParamInfos",
    buffer_count: "buffer_count",
    textures_g9: "texture_count",
    other_count: "other_count",
    sampler_count: "sampler_count",
    copies: "copies",
    siblings: &["texture_slots", "other_slots"],
    texture_g9: "Texture",
};

const OWNED: Names = Names {
    material: "GrcInstanceData",
    entry: "GrcInstanceDataEntry",
    data: "data",
    value_count: "count",
    entry_count: "count",
    texture_count: "texture_count",
    hash_offset: "spu_size",
    blob_size: "total_size",
    texture: "GrcTextureReference",
    skeleton: "CrSkeletonData",
    bone_tag_slots: "bone_tag_slot_count",
    bone_count: "num_bones",
    bone_tag: "bone_id",
    material_g9: "GrcInstanceDataG9",
    param_infos_type: "ShaderParamInfos",
    buffer_count: "num_buffers",
    textures_g9: "num_textures",
    other_count: "num_unknowns",
    sampler_count: "num_samplers",
    copies: "buffer_copies",
    siblings: &["texture_refs", "other_refs"],
    texture_g9: "GrcTextureReferenceG9",
};

/// The readers a drawable-bearing format's tree is read with, its collision bounds' included.
pub fn readers() -> BlobCodecs {
    let mut codecs = super::bounds::readers();
    for names in [&RAW, &OWNED] {
        codecs = codecs
            .at(names.material, ENTRIES, Arc::new(MaterialBlob(names)))
            .at(names.material_g9, PARAMS, Arc::new(ParamBlock(names)))
            .at(names.skeleton, BONE_TAGS, Arc::new(BoneTagTable(names)));
    }
    codecs
}

/// The writers its canonical save is built with.
pub fn writers() -> BlobWriters {
    let mut codecs = super::bounds::writers();
    for names in [&RAW, &OWNED] {
        codecs = codecs
            .at(names.material, ENTRIES, Arc::new(MaterialBlob(names)))
            .at(names.material_g9, PARAMS, Arc::new(ParamBlock(names)))
            .at(names.skeleton, BONE_TAGS, Arc::new(BoneTagTable(names)));
    }
    codecs
}

/// A material's texture: an entry of the drawable's own dictionary, by its code, or a reference by name the material owns.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum TextureSlot<T, R> {
    #[default]
    Unbound,
    /// An entry of the drawable's texture dictionary.
    Embedded(Keyed<T>),
    /// A texture the game finds by name outside the drawable.
    Reference(Box<R>),
}

impl<T, R: Heap> Heap for TextureSlot<T, R> {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Unbound | Self::Embedded(_) => 0,
            Self::Reference(r) => r.heap_bytes(),
        }
    }
}

impl<T: Owned, R: Owned> Owned for TextureSlot<T, R>
where
    T::Raw: 'static,
    R::Raw: Described + 'static,
{
    type Raw = Ptr<R::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        match kids.iter().find(|(o, _)| *o as usize == at).map(|(_, v)| v) {
            None | Some(Value::Ptr { target: None, .. }) => Ok(Self::Unbound),
            // A second pointer reaches a dictionary's texture, or a reference another slot owns: ped
            // components bind one by-name reference as the normal and the specular map.
            Some(alias @ Value::Alias(_)) => match alias_copy::<R>(alias, rx)? {
                Some(copy) => Ok(Self::Reference(Box::new(copy))),
                None => Keyed::<T>::from_raw(&Plain::zeroed(), kids, at, rx).map(Self::Embedded),
            },
            Some(_) => {
                <Box<R> as Owned>::from_raw(&Plain::zeroed(), kids, at, rx).map(Self::Reference)
            }
        }
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        Self::from_raw(
            &Plain::zeroed(),
            std::slice::from_ref(&(0, v.clone())),
            0,
            rx,
        )
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        match self {
            Self::Unbound => Ok(Value::null_ptr()),
            Self::Embedded(key) => key.to_element(cx),
            Self::Reference(texture) => texture.to_element(cx),
        }
    }
}

fn int_at(image: Image<'_>, ty: &PackType, at: usize, field: &str) -> Option<i128> {
    let field = ty.field_by_name(field)?;
    let width = match &field.repr {
        FieldRepr::Scalar(Scalar::Int(width)) | FieldRepr::Enum { width, .. } => *width,
        _ => return None,
    };
    width.read(image.bytes(), at.checked_add(field.offset)?)
}

#[derive(Debug, Clone, Copy)]
struct Rows {
    stride: usize,
    data: usize,
    count: usize,
}

impl Rows {
    fn of(pack: &Pack, names: &Names) -> Option<Rows> {
        let ty = pack.plans().get(names.entry).ok()?.pack_type();
        Some(Rows {
            stride: ty.size,
            data: ty.field_by_name(names.data)?.offset,
            count: ty.field_by_name(names.value_count)?.offset,
        })
    }

    fn data_slot(&self, base: usize, index: usize) -> usize {
        base + index * self.stride + self.data
    }
}

/// A Legacy material's blob: carried whole, its interior pointers rebased.
#[derive(Debug, Clone, Copy)]
struct MaterialBlob(&'static Names);

impl BlobReader for MaterialBlob {
    fn read(&self, blob: &dyn BlobSource) -> Result<Value, ValueError> {
        let names = self.0;
        let pack = blob.pack();
        let (owner, owner_at) = blob.owner();
        let (address, blob_at) = blob.blob();
        let image = blob.image();

        let missing = |what: &str| ValueError::NoCountField {
            field: ENTRIES.to_string(),
            count_field: what.to_string(),
        };
        let truncated = || ValueError::Truncated {
            offset: blob_at,
            type_name: BLOB.to_string(),
        };
        let stated = |name: &str| {
            usize::try_from(int_at(image, owner, owner_at, name).ok_or_else(|| missing(name))?)
                .map_err(|_| missing(name))
        };
        let count = stated(names.entry_count)?;
        let total = stated(names.blob_size)?;
        let rows = Rows::of(pack, names).ok_or_else(|| ValueError::NoType {
            type_name: names.entry.to_string(),
        })?;
        if count.saturating_mul(rows.stride) > total {
            return Err(truncated());
        }

        let mut bytes = image
            .slice_at(blob_at, total)
            .map_err(|reason| ValueError::BadPointer {
                field: ENTRIES.to_string(),
                reason,
            })?
            .to_vec();
        // Every data slot is rewritten from `data` on the way out, so the
        // address the file held is carried nowhere.
        for i in 0..count {
            let at = rows.data_slot(0, i);
            if let Some(slot) = bytes.get_mut(at..at + 8) {
                slot.fill(0);
            }
        }

        let end = address.saturating_add(total as u64);
        let mut data = Vec::with_capacity(count);
        for i in 0..count {
            let raw = image
                .pointer_at(rows.data_slot(blob_at, i))
                .map_err(|reason| ValueError::BadPointer {
                    field: ENTRIES.to_string(),
                    reason,
                })?;
            let kind = image
                .bytes()
                .get(blob_at + i * rows.stride + rows.count)
                .copied()
                .ok_or_else(truncated)?;
            data.push(if raw == 0 {
                Value::Ptr {
                    source: 0,
                    target: None,
                }
            } else if raw >= address && raw < end {
                Value::Scalar(ScalarValue::Int((raw - address) as i128))
            } else if kind == 0 {
                blob.follow(names.texture, raw)?
            } else {
                Value::UndescribedPtr(raw)
            });
        }

        Ok(Value::Blob {
            codec: BLOB.to_string(),
            parts: vec![
                (BYTES.to_string(), Value::Opaque(bytes)),
                (
                    DATA.to_string(),
                    Value::Array {
                        elements: data,
                        capacity: None,
                    },
                ),
            ],
        })
    }
}

impl BlobWriter for MaterialBlob {
    fn write(
        &self,
        heap: &mut dyn BlobHeap,
        owner: Owner<'_>,
        value: &Value,
    ) -> Result<Placement, BuildError> {
        let refuse = |message: String| BuildError::Blob {
            type_name: owner.ty.name.clone(),
            field: ENTRIES.to_string(),
            message,
        };
        let Value::Blob { codec, parts } = value else {
            return Err(refuse("the value is not this codec's".into()));
        };
        if codec != BLOB {
            return Err(refuse(format!(
                "the value says it is a `{codec}`, and this codec writes a `{BLOB}`"
            )));
        }
        let part = |name: &str| parts.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        let bytes = part(BYTES)
            .and_then(Value::as_bytes)
            .ok_or_else(|| refuse(format!("no `{BYTES}` to write")))?
            .to_vec();
        let data = part(DATA)
            .and_then(Value::elements)
            .ok_or_else(|| refuse(format!("no `{DATA}` run")))?
            .to_vec();
        let names = self.0;
        if let Some(count) = owner.int(names.entry_count) {
            if count != data.len() as i128 {
                return Err(refuse(format!(
                    "the header says {count} entr(ies) and the run holds {}; a run and its count \
                     are edited together",
                    data.len()
                )));
            }
        }
        let rows = Rows::of(heap.pack(), names)
            .ok_or_else(|| refuse(format!("the description has no `{}`", names.entry)))?;

        let mut placed: Vec<Option<u64>> = Vec::with_capacity(data.len());
        for element in &data {
            placed.push(match element {
                Value::Scalar(_) | Value::NotAnAddress(_) => None,
                other => heap.place(names.material, ENTRIES, other)?,
            });
        }
        if bytes.is_empty() {
            return Ok(Placement {
                address: heap.alloc(1, 1),
                siblings: Vec::new(),
            });
        }

        let address = heap.alloc(bytes.len(), BLOB_ALIGN);
        let base = heap.offset_of(address);
        heap.put(base, &bytes)?;
        for (i, element) in data.iter().enumerate() {
            let at = rows.data_slot(base, i);
            match element {
                Value::Scalar(ScalarValue::Int(offset)) => {
                    let target = address.wrapping_add(*offset as u64);
                    heap.put(at, &target.to_le_bytes())?;
                    heap.fixup(at, target)?;
                }
                Value::NotAnAddress(raw) => heap.put(at, &raw.to_le_bytes())?,
                _ => match placed[i] {
                    Some(target) => {
                        heap.put(at, &target.to_le_bytes())?;
                        heap.fixup(at, target)?;
                    }
                    None => heap.put(at, &0u64.to_le_bytes())?,
                },
            }
        }
        Ok(Placement {
            address,
            siblings: Vec::new(),
        })
    }
}

/// A skeleton's bone-tag table: read by walking its chains, written by
/// deriving it from the bone run, and refused where the two disagree.
///
/// ⚠ Where the bone run is an alias of another skeleton's, the tags are not
/// in this subtree, so the table read is the one written.
#[derive(Debug, Clone, Copy)]
struct BoneTagTable(&'static Names);

type Buckets = Vec<Vec<(u16, u32)>>;

impl BlobReader for BoneTagTable {
    fn read(&self, blob: &dyn BlobSource) -> Result<Value, ValueError> {
        let names = self.0;
        let (owner, owner_at) = blob.owner();
        let (_, blob_at) = blob.blob();
        let image = blob.image();
        let missing = |what: &str| ValueError::NoCountField {
            field: BONE_TAGS.to_string(),
            count_field: what.to_string(),
        };
        let bad = |reason| ValueError::BadPointer {
            field: BONE_TAGS.to_string(),
            reason,
        };
        let malformed = || ValueError::Truncated {
            offset: blob_at,
            type_name: TABLE.to_string(),
        };
        let count = |field: &str| -> Result<usize, ValueError> {
            usize::try_from(int_at(image, owner, owner_at, field).ok_or_else(|| missing(field))?)
                .map_err(|_| missing(field))
        };
        let slots = count(names.bone_tag_slots)?;
        let budget = count(names.bone_count)?;

        let mut read = 0usize;
        let mut buckets = Vec::with_capacity(slots);
        for i in 0..slots {
            let mut chain = Vec::new();
            let mut next = image
                .pointer_at(blob_at.checked_add(i * HEAD_WIDTH).ok_or_else(malformed)?)
                .map_err(bad)?;
            while next != 0 {
                if read == budget {
                    return Err(malformed());
                }
                read += 1;
                let at = image.resolve(next).map_err(bad)?;
                let word = |o: usize| -> Result<u32, ValueError> {
                    let b = image
                        .bytes()
                        .get(at + o..at + o + 4)
                        .ok_or_else(malformed)?;
                    Ok(u32::from_le_bytes(b.try_into().map_err(|_| malformed())?))
                };
                chain.push(Value::Blob {
                    codec: TABLE_ENTRY.to_string(),
                    parts: vec![
                        (
                            TAG.to_string(),
                            Value::Scalar(ScalarValue::Int(word(0)? as i128)),
                        ),
                        (
                            INDEX.to_string(),
                            Value::Scalar(ScalarValue::Int(word(4)? as i128)),
                        ),
                    ],
                });
                next = image.pointer_at(at + HEAD_WIDTH).map_err(bad)?;
            }
            buckets.push(Value::Array {
                elements: chain,
                capacity: None,
            });
        }
        Ok(Value::Blob {
            codec: TABLE.to_string(),
            parts: vec![(
                BUCKETS.to_string(),
                Value::Array {
                    elements: buckets,
                    capacity: None,
                },
            )],
        })
    }
}

impl BlobWriter for BoneTagTable {
    fn write(
        &self,
        heap: &mut dyn BlobHeap,
        owner: Owner<'_>,
        value: &Value,
    ) -> Result<Placement, BuildError> {
        let refuse = |message: String| BuildError::Blob {
            type_name: owner.ty.name.clone(),
            field: BONE_TAGS.to_string(),
            message,
        };
        let names = self.0;
        let carried = buckets_from(value).map_err(refuse)?;
        let slots = owner
            .int(names.bone_tag_slots)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| {
                refuse(format!(
                    "no `{}` to size the slot array by",
                    names.bone_tag_slots
                ))
            })?;
        let derived = match tags_in(owner.child(BONES), names.bone_tag).map_err(refuse)? {
            Some(tags) => {
                let derived = bucket_bone_tags(slots, tags);
                if let Some(carried) = carried.filter(|c| *c != derived) {
                    return Err(refuse(not_the_derivation(&carried, &derived)));
                }
                derived
            }
            None => carried.ok_or_else(|| {
                refuse(
                    "the bones are another skeleton's and no table is carried, so there is \
                     nothing to write"
                        .into(),
                )
            })?,
        };
        if derived.is_empty() {
            return Ok(Placement::default());
        }

        let mut heads = Vec::with_capacity(derived.len());
        for bucket in &derived {
            let mut next = 0u64;
            for &(tag, index) in bucket.iter().rev() {
                let address = heap.alloc(BONE_TAG_ENTRY_SIZE, BONE_TAG_ENTRY_SIZE);
                let at = heap.offset_of(address);
                heap.put(at, &u32::from(tag).to_le_bytes())?;
                heap.put(at + 4, &index.to_le_bytes())?;
                if next != 0 {
                    heap.put(at + HEAD_WIDTH, &next.to_le_bytes())?;
                    heap.fixup(at + HEAD_WIDTH, next)?;
                }
                next = address;
            }
            heads.push(next);
        }
        let address = heap.alloc(derived.len() * HEAD_WIDTH, BLOB_ALIGN);
        let base = heap.offset_of(address);
        for (i, head) in heads.iter().enumerate() {
            if *head == 0 {
                continue;
            }
            let at = base + i * HEAD_WIDTH;
            heap.put(at, &head.to_le_bytes())?;
            heap.fixup(at, *head)?;
        }
        Ok(Placement {
            address,
            siblings: Vec::new(),
        })
    }
}

fn not_the_derivation(carried: &Buckets, derived: &Buckets) -> String {
    format!(
        "the file's bone-tag table is not the one its bone run implies — {} chained entr(ies) \
         against {}; an edit changed the bones without the table, or the file's table was never \
         the derivation",
        carried.iter().map(Vec::len).sum::<usize>(),
        derived.iter().map(Vec::len).sum::<usize>(),
    )
}

/// The table a blob carries; `None` for one that asks the writer to derive it.
fn buckets_from(value: &Value) -> Result<Option<Buckets>, String> {
    let Value::Blob { codec, parts } = value else {
        return Err("the value is not this codec's".into());
    };
    if codec != TABLE {
        return Err(format!(
            "the value says it is a `{codec}`, and this codec writes a `{TABLE}`"
        ));
    }
    let Some((_, buckets)) = parts.iter().find(|(name, _)| name == BUCKETS) else {
        return Ok(None);
    };
    let buckets = buckets
        .elements()
        .ok_or_else(|| format!("`{BUCKETS}` is not a run"))?;
    buckets
        .iter()
        .map(|bucket| {
            bucket
                .elements()
                .ok_or_else(|| format!("a `{BUCKETS}` element is not a run of entries"))?
                .iter()
                .map(|entry| {
                    match (
                        entry.part(TAG).and_then(Value::as_int),
                        entry.part(INDEX).and_then(Value::as_int),
                    ) {
                        (Some(tag), Some(index)) => Ok((tag as u16, index as u32)),
                        _ => Err(format!("a chain element is not a `{TABLE_ENTRY}`")),
                    }
                })
                .collect()
        })
        .collect::<Result<_, _>>()
        .map(Some)
}

/// Each bone's tag, its field `tag`, in bone order; `None` where the bones are another skeleton's.
fn tags_in(bones: Option<&Value>, tag: &str) -> Result<Option<Vec<u16>>, String> {
    let Some(value) = bones else {
        return Err("no `bones` to derive the table from".into());
    };
    let run = match value {
        Value::Ptr { target: None, .. } => return Ok(Some(Vec::new())),
        Value::Ptr {
            target: Some(inner),
            ..
        } => inner.as_ref(),
        Value::Alias(_) => return Ok(None),
        other => other,
    };
    let Some(len) = run.run_len() else {
        return Err("`bones` is not a run of bones, so the table cannot be derived".into());
    };
    (0..len)
        .map(|i| {
            run.struct_at(i)
                .and_then(|b| b.int(tag))
                .map(|tag| tag as u16)
                .ok_or_else(|| {
                    format!("a bone has no `{tag}`, the only thing the table is keyed on")
                })
        })
        .collect::<Result<Vec<u16>, String>>()
        .map(Some)
}

/// A skeleton's bone-tag table, derived from the bones and `bone_tag_slots`, for a private `#[codec]` field.
///
/// ⚠ Opening refuses a table that is not the derivation, which the tree's
/// save refuses too, and one whose bones are another skeleton's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoneTags;

/// The names a skeleton's fields go by, whichever set its declaration uses.
fn skeleton_names(int: impl Fn(&str) -> Option<i128>) -> &'static Names {
    if int(OWNED.bone_tag_slots).is_some() {
        &OWNED
    } else {
        &RAW
    }
}

/// The names a Legacy material's fields go by, whichever set its declaration uses.
fn material_names(int: impl Fn(&str) -> Option<i128>) -> &'static Names {
    if int(OWNED.blob_size).is_some() {
        &OWNED
    } else {
        &RAW
    }
}

impl Heap for BoneTags {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl Codec for BoneTags {
    fn open(site: &Site<'_>, _: &Rx<'_>) -> Result<Self, Error> {
        let names = skeleton_names(|n| site.int(n));
        let slots = site
            .int(names.bone_tag_slots)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| {
                site.refuse(format!(
                    "no `{}` to size the table by",
                    names.bone_tag_slots
                ))
            })?;
        let carried = match site.value() {
            Some(Value::Ptr { target: None, .. }) if slots == 0 => return Ok(Self),
            Some(Value::Ptr {
                target: Some(blob), ..
            }) if slots != 0 => buckets_from(blob)
                .map_err(|e| site.refuse(e))?
                .ok_or_else(|| site.refuse("the table read carries no buckets"))?,
            _ => {
                return Err(site.refuse(format!(
                    "a table that is not a pointer at {slots} bucket(s), which the owned form \
                     would not write back"
                )))
            }
        };
        let tags = tags_in(site.child(BONES), names.bone_tag)
            .map_err(|e| site.refuse(e))?
            .ok_or_else(|| {
                site.refuse("the bones are another skeleton's, so the table is not derived here")
            })?;
        let derived = bucket_bone_tags(slots, tags);
        if carried != derived {
            return Err(site.refuse(not_the_derivation(&carried, &derived)));
        }
        Ok(Self)
    }

    fn save(&self, site: &mut SiteMut<'_>, _: &Cx) -> Result<(), Error> {
        let names = skeleton_names(|n| site.int(n));
        let slots = site.int(names.bone_tag_slots).ok_or_else(|| {
            site.refuse(format!(
                "no `{}` to size the table by",
                names.bone_tag_slots
            ))
        })?;
        site.put(if slots == 0 {
            Value::null_ptr()
        } else {
            // No buckets: the canonical writer derives them from the bones.
            Value::ptr_to(Value::Blob {
                codec: TABLE.to_string(),
                parts: Vec::new(),
            })
        });
        Ok(())
    }
}

const PARAMS: &str = "params";
const BLOCK: &str = "Material::params";
const SLOTS_BLOB: &str = "Material::params::slots";
const AT: &str = "at";
const TARGET: &str = "target";
const SLOTS: &str = "slots";
const PARAM_INFOS: &str = "param_infos";
const PARAM_DATA_SIZE: &str = "param_data_size";
const SLOT_WIDTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Buffers,
    Textures,
    Others,
}

/// Where each slot table of an Enhanced material's block is, as a byte offset and a slot count.
#[derive(Debug, Clone, Copy)]
struct Windows {
    total: usize,
    table: (usize, usize),
    textures: Option<(usize, usize)>,
    others: Option<(usize, usize)>,
}

fn ptr_at(image: Image<'_>, ty: &PackType, at: usize, field: &str) -> Option<u64> {
    let field = ty.field_by_name(field)?;
    if !matches!(field.repr, FieldRepr::Ptr(_)) {
        return None;
    }
    image.pointer_at(at.checked_add(field.offset)?).ok()
}

impl Windows {
    fn of(
        pack: &Pack,
        names: &Names,
        image: Image<'_>,
        owner: &PackType,
        owner_at: usize,
        address: u64,
    ) -> Result<Windows, ValueError> {
        let missing = |what: &str| ValueError::NoCountField {
            field: PARAMS.to_string(),
            count_field: what.to_string(),
        };
        let malformed = |at: usize| ValueError::Truncated {
            offset: at,
            type_name: BLOCK.to_string(),
        };
        let total = usize::try_from(
            int_at(image, owner, owner_at, PARAM_DATA_SIZE)
                .ok_or_else(|| missing(PARAM_DATA_SIZE))?,
        )
        .map_err(|_| missing(PARAM_DATA_SIZE))?;
        let infos_addr =
            ptr_at(image, owner, owner_at, PARAM_INFOS).ok_or_else(|| missing(PARAM_INFOS))?;
        let infos_at = image
            .resolve(infos_addr)
            .map_err(|reason| ValueError::BadPointer {
                field: PARAM_INFOS.to_string(),
                reason,
            })?;
        let infos = pack
            .plans()
            .get(names.param_infos_type)
            .map_err(|_| ValueError::NoType {
                type_name: names.param_infos_type.to_string(),
            })?
            .pack_type();
        let count = |field: &str| -> Result<usize, ValueError> {
            usize::try_from(int_at(image, infos, infos_at, field).ok_or_else(|| missing(field))?)
                .map_err(|_| missing(field))
        };
        let buffers = count(names.buffer_count)?;
        let textures = count(names.textures_g9)?;
        let others = count(names.other_count)?;
        // ⚠ At least one copy: a file stating zero must not collapse the
        // tables to nothing and lose every slot.
        let copies = count(names.copies)?.max(1);

        let run = |field: &str, per_copy: usize| -> Result<Option<(usize, usize)>, ValueError> {
            let raw = ptr_at(image, owner, owner_at, field).ok_or_else(|| missing(field))?;
            if raw == 0 {
                return Ok(None);
            }
            // ⚠ Checked, not wrapped: a garbage pointer subtracts to near
            // `usize::MAX`, and a debug and a release build would disagree
            // about whether the file is refused.
            let Some(offset) = raw.checked_sub(address).map(|o| o as usize) else {
                return Err(malformed(owner_at));
            };
            let slots = per_copy * copies;
            let end = offset
                .checked_add(slots * SLOT_WIDTH)
                .ok_or_else(|| malformed(owner_at))?;
            if offset % SLOT_WIDTH != 0 || end > total {
                return Err(malformed(owner_at));
            }
            Ok(Some((offset, slots)))
        };

        let table = (0usize, buffers * copies);
        if table.1 * SLOT_WIDTH > total {
            return Err(malformed(owner_at));
        }
        let windows = Windows {
            total,
            table,
            textures: run(names.siblings[0], textures)?,
            others: run(names.siblings[1], others)?,
        };
        let runs = windows.runs();
        for (i, (start, slots, _)) in runs.iter().enumerate() {
            for (other, other_slots, _) in runs.iter().skip(i + 1) {
                if start < &(other + other_slots * SLOT_WIDTH)
                    && other < &(start + slots * SLOT_WIDTH)
                {
                    return Err(malformed(owner_at));
                }
            }
        }
        Ok(windows)
    }

    fn runs(&self) -> Vec<(usize, usize, Run)> {
        let mut out = vec![(self.table.0, self.table.1, Run::Buffers)];
        if let Some((at, slots)) = self.textures {
            out.push((at, slots, Run::Textures));
        }
        if let Some((at, slots)) = self.others {
            out.push((at, slots, Run::Others));
        }
        out
    }
}

/// An Enhanced material's parameter block: carried whole, every slot rebased.
///
/// ⚠ **A read-back cannot validate it.** Each slot table repeats once per
/// worker thread, and a reader following only the first copy parses a block
/// truncated to one copy happily; carrying the block whole is what keeps
/// every copy.
#[derive(Debug, Clone, Copy)]
struct ParamBlock(&'static Names);

impl BlobReader for ParamBlock {
    fn read(&self, blob: &dyn BlobSource) -> Result<Value, ValueError> {
        let names = self.0;
        let pack = blob.pack();
        let (owner, owner_at) = blob.owner();
        let (address, blob_at) = blob.blob();
        let image = blob.image();
        let windows = Windows::of(pack, names, image, owner, owner_at, address)?;

        let mut bytes = image
            .slice_at(blob_at, windows.total)
            .map_err(|reason| ValueError::BadPointer {
                field: PARAMS.to_string(),
                reason,
            })?
            .to_vec();
        for (start, count, _) in windows.runs() {
            for i in 0..count {
                let at = start + i * SLOT_WIDTH;
                if let Some(slot) = bytes.get_mut(at..at + SLOT_WIDTH) {
                    slot.fill(0);
                }
            }
        }

        let end = address.saturating_add(windows.total as u64);
        let (mut ats, mut targets) = (Vec::new(), Vec::new());
        for (start, count, run) in windows.runs() {
            for i in 0..count {
                let at = start + i * SLOT_WIDTH;
                let raw =
                    image
                        .pointer_at(blob_at + at)
                        .map_err(|reason| ValueError::BadPointer {
                            field: PARAMS.to_string(),
                            reason,
                        })?;
                let target = if raw == 0 {
                    Value::Ptr {
                        source: 0,
                        target: None,
                    }
                } else if raw >= address && raw < end {
                    Value::Scalar(ScalarValue::Int((raw - address) as i128))
                } else if run == Run::Textures {
                    blob.follow(names.texture_g9, raw)?
                } else if image.resolve(raw).is_ok() {
                    Value::UndescribedPtr(raw)
                } else {
                    Value::NotAnAddress(raw)
                };
                ats.push(Value::Scalar(ScalarValue::Int(at as i128)));
                targets.push(target);
            }
        }
        let sibling = |window: Option<(usize, usize)>| match window {
            Some((at, _)) => Value::Scalar(ScalarValue::Int(at as i128)),
            None => Value::Ptr {
                source: 0,
                target: None,
            },
        };
        Ok(Value::Blob {
            codec: BLOCK.to_string(),
            parts: vec![
                (BYTES.to_string(), Value::Opaque(bytes)),
                (
                    SLOTS.to_string(),
                    Value::Blob {
                        codec: SLOTS_BLOB.to_string(),
                        parts: vec![
                            (
                                AT.to_string(),
                                Value::Array {
                                    elements: ats,
                                    capacity: None,
                                },
                            ),
                            (
                                TARGET.to_string(),
                                Value::Array {
                                    elements: targets,
                                    capacity: None,
                                },
                            ),
                        ],
                    },
                ),
                (names.siblings[0].to_string(), sibling(windows.textures)),
                (names.siblings[1].to_string(), sibling(windows.others)),
            ],
        })
    }
}

/// A block's slots as `(where, what)`, or `None` unless both runs are there and agree in length.
fn slots_of(slots: &Value) -> Option<Vec<(usize, &Value)>> {
    let Value::Blob { codec, .. } = slots else {
        return None;
    };
    if codec != SLOTS_BLOB {
        return None;
    }
    let ats = slots.part(AT)?.elements()?;
    let targets = slots.part(TARGET)?.elements()?;
    if ats.len() != targets.len() {
        return None;
    }
    ats.iter()
        .zip(targets)
        .map(|(at, target)| Some((usize::try_from(at.as_int()?).ok()?, target)))
        .collect()
}

impl BlobWriter for ParamBlock {
    fn write(
        &self,
        heap: &mut dyn BlobHeap,
        owner: Owner<'_>,
        value: &Value,
    ) -> Result<Placement, BuildError> {
        let refuse = |message: String| BuildError::Blob {
            type_name: owner.ty.name.clone(),
            field: PARAMS.to_string(),
            message,
        };
        let Value::Blob { codec, parts } = value else {
            return Err(refuse("the value is not this codec's".into()));
        };
        if codec != BLOCK {
            return Err(refuse(format!(
                "the value says it is a `{codec}`, and this codec writes a `{BLOCK}`"
            )));
        }
        let part = |name: &str| parts.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        let bytes = part(BYTES)
            .and_then(Value::as_bytes)
            .ok_or_else(|| refuse(format!("no `{BYTES}` to write")))?
            .to_vec();
        let slots = part(SLOTS)
            .and_then(slots_of)
            .ok_or_else(|| refuse(format!("no `{SLOTS}` as a `{SLOTS_BLOB}`")))?;
        if let Some(stated) = owner.int(PARAM_DATA_SIZE) {
            if stated != bytes.len() as i128 {
                return Err(refuse(format!(
                    "the header says {stated} byte(s) of parameter block and the block holds {}; \
                     a block and its size are edited together",
                    bytes.len()
                )));
            }
        }
        let mut placed: Vec<Option<u64>> = Vec::with_capacity(slots.len());
        for &(_, target) in &slots {
            placed.push(match target {
                Value::Scalar(_) | Value::NotAnAddress(_) => None,
                other => heap.place(self.0.material_g9, PARAMS, other)?,
            });
        }
        if bytes.is_empty() {
            return Ok(Placement {
                address: heap.alloc(1, 1),
                siblings: Vec::new(),
            });
        }
        let address = heap.alloc(bytes.len(), BLOB_ALIGN);
        let base = heap.offset_of(address);
        heap.put(base, &bytes)?;
        for (i, &(at, target)) in slots.iter().enumerate() {
            if at
                .checked_add(SLOT_WIDTH)
                .is_none_or(|end| end > bytes.len())
            {
                return Err(refuse(format!(
                    "a slot at +{at:#x} does not fit in a {} byte block",
                    bytes.len()
                )));
            }
            let at = base + at;
            match target {
                Value::Scalar(ScalarValue::Int(offset)) => {
                    let target = address.wrapping_add(*offset as u64);
                    heap.put(at, &target.to_le_bytes())?;
                    heap.fixup(at, target)?;
                }
                Value::NotAnAddress(raw) => heap.put(at, &raw.to_le_bytes())?,
                _ => match placed[i] {
                    Some(target) => {
                        heap.put(at, &target.to_le_bytes())?;
                        heap.fixup(at, target)?;
                    }
                    None => heap.put(at, &0u64.to_le_bytes())?,
                },
            }
        }
        let mut siblings = Vec::new();
        for name in self.0.siblings {
            if let Some(Value::Scalar(ScalarValue::Int(offset))) = part(name) {
                siblings.push((name.to_string(), address.wrapping_add(*offset as u64)));
            }
        }
        Ok(Placement { address, siblings })
    }

    fn writes_siblings(&self) -> &'static [&'static str] {
        self.0.siblings
    }
}

/// The owned form of a Legacy material's parameter blob.
pub mod gen8 {
    use std::mem::{offset_of, size_of};

    use super::{material_names, BLOB, BYTES, DATA};
    use crate::decl::{
        pointer_in, Codec, Cx, Hash32, Heap, Owned, Plain, Raw, Rx, Site, SiteMut, Vec4,
    };
    use crate::desc::value::ScalarValue;
    use crate::desc::Value;
    use crate::error::Error;

    /// The layout of the entry table the codec lays out.
    #[doc(hidden)]
    pub mod layout {
        use crate::decl::*;

        /// `grcInstanceData::Entry`: one parameter or texture binding of a Legacy material.
        #[record(size = 0x10, target = "gen8")]
        pub struct GrcInstanceDataEntry {
            /// Zero for a texture; otherwise how many sixteen-byte rows `data` holds.
            pub count: u8,
            pub register: u8,
            sampler_state_set: u8,
            #[pad_after(4)]
            saved_sampler_state_set: u8,
            data: Ptr<Opaque>,
        }
    }

    /// One entry of a Legacy material: the variable it binds, by the hash of its name, and what is bound.
    ///
    /// ⚠ A save writes the entry's `sampler_state_set` and
    /// `saved_sampler_state_set` bytes zero. Measured: zero in every entry of
    /// 977 of 977 Legacy material blobs in the shared corpus
    /// (`every_material_and_skeleton_opens_owned_and_saves_as_its_tree_saves`, on c4cec2e0).
    #[derive(Debug, Clone, PartialEq, Default)]
    pub struct GrcInstanceDataEntry<T> {
        pub register: u8,
        pub name_hash: Hash32,
        pub data: EntryData<T>,
    }

    /// What an entry binds: a texture, or rows of four floats.
    #[derive(Debug, Clone, PartialEq)]
    pub enum EntryData<T> {
        Texture(T),
        Float(Vec<Vec4>),
    }

    impl<T: Default> Default for EntryData<T> {
        fn default() -> Self {
            Self::Texture(T::default())
        }
    }

    const ROW: usize = size_of::<Vec4>();
    const HASH: usize = size_of::<u32>();
    /// ⚠ Zero bytes past the hash table rounded up to a row, which nothing
    /// here explains. Measured: 977 of 977 Legacy material blobs in the shared
    /// corpus end in exactly these
    /// (`every_material_and_skeleton_opens_owned_and_saves_as_its_tree_saves`, on c4cec2e0).
    const TAIL: usize = 2 * ROW;

    /// A blob as the canonical writer lays it out: the entry table, each
    /// entry's rows in entry order, then one name hash per entry.
    struct Laid {
        bytes: Vec<u8>,
        /// Where each entry's rows start; `None` for a texture.
        rows: Vec<Option<usize>>,
        hashes: usize,
    }

    fn lay_out<T>(entries: &[GrcInstanceDataEntry<T>]) -> Result<Laid, String> {
        let stride = size_of::<Raw<layout::GrcInstanceDataEntry>>();
        let mut at = entries.len() * stride;
        let mut rows = Vec::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            rows.push(match &e.data {
                EntryData::Texture(_) => None,
                EntryData::Float(r) if r.is_empty() || r.len() > usize::from(u8::MAX) => {
                    return Err(format!(
                        "entry {i} holds {} row(s), and an entry holds 1 to 255",
                        r.len()
                    ))
                }
                EntryData::Float(r) => {
                    let here = at;
                    at += r.len() * ROW;
                    Some(here)
                }
            });
        }
        let hashes = at;
        let mut bytes = vec![0u8; (hashes + entries.len() * HASH).next_multiple_of(ROW) + TAIL];
        for (i, (e, row_at)) in entries.iter().zip(&rows).enumerate() {
            let base = i * stride;
            bytes[base + offset_of!(Raw<layout::GrcInstanceDataEntry>, register)] = e.register;
            if let (EntryData::Float(r), Some(row_at)) = (&e.data, row_at) {
                bytes[base + offset_of!(Raw<layout::GrcInstanceDataEntry>, count)] = r.len() as u8;
                for (j, row) in r.iter().enumerate() {
                    let at = row_at + j * ROW;
                    bytes[at..at + ROW].copy_from_slice(Plain::as_bytes(row));
                }
            }
            let at = hashes + i * HASH;
            bytes[at..at + HASH].copy_from_slice(&e.name_hash.get().to_le_bytes());
        }
        Ok(Laid {
            bytes,
            rows,
            hashes,
        })
    }

    fn is_texture<T>(e: &GrcInstanceDataEntry<T>) -> bool {
        matches!(e.data, EntryData::Texture(_))
    }

    /// How many entries lead as textures; refused where a texture follows a float entry.
    fn texture_count<T>(entries: &[GrcInstanceDataEntry<T>]) -> Result<usize, String> {
        let lead = entries.iter().take_while(|e| is_texture(e)).count();
        match entries[lead..].iter().position(is_texture) {
            Some(i) => Err(format!(
                "entry {} is a texture after a float entry, and a material's textures lead",
                lead + i
            )),
            None => Ok(lead),
        }
    }

    fn offset(v: &Value) -> Option<usize> {
        match v {
            Value::Scalar(ScalarValue::Int(at)) => usize::try_from(*at).ok(),
            _ => None,
        }
    }

    impl<T: Heap> Heap for GrcInstanceDataEntry<T> {
        fn heap_bytes(&self) -> usize {
            match &self.data {
                EntryData::Texture(t) => t.heap_bytes(),
                EntryData::Float(rows) => rows.heap_bytes(),
            }
        }
    }

    impl<T: Owned> Codec for Vec<GrcInstanceDataEntry<T>> {
        fn open(site: &Site<'_>, rx: &Rx<'_>) -> Result<Self, Error> {
            let names = material_names(|n| site.int(n));
            let stated = |name: &str| {
                site.int(name)
                    .ok_or_else(|| site.refuse(format!("no `{name}`")))
            };
            let blob = match site.value() {
                Some(Value::Ptr { target: None, .. }) => {
                    for name in [
                        names.entry_count,
                        names.texture_count,
                        names.hash_offset,
                        names.blob_size,
                    ] {
                        if stated(name)? != 0 {
                            return Err(site.refuse(format!(
                                "a null blob beside a `{name}` that is not zero"
                            )));
                        }
                    }
                    return Ok(Vec::new());
                }
                Some(Value::Ptr {
                    target: Some(blob), ..
                }) => blob.as_ref(),
                _ => return Err(site.refuse("the entries are not a pointer at a blob")),
            };
            let (bytes, data) = match blob {
                Value::Blob { codec, .. } if codec == BLOB => (
                    blob.part(BYTES).and_then(Value::as_bytes),
                    blob.part(DATA).and_then(Value::elements),
                ),
                _ => (None, None),
            };
            let (Some(bytes), Some(data)) = (bytes, data) else {
                return Err(site.refuse(format!("the blob is not a `{BLOB}`")));
            };
            let stride = size_of::<Raw<layout::GrcInstanceDataEntry>>();
            if data.len() * stride > bytes.len() {
                return Err(site.refuse("the entry table runs past the blob"));
            }
            let count = |i: usize| {
                usize::from(
                    bytes[i * stride + offset_of!(Raw<layout::GrcInstanceDataEntry>, count)],
                )
            };
            let hashes = data.len() * stride + (0..data.len()).map(count).sum::<usize>() * ROW;
            let mut entries = Vec::with_capacity(data.len());
            for (i, d) in data.iter().enumerate() {
                let name_hash = bytes
                    .get(hashes + i * HASH..hashes + (i + 1) * HASH)
                    .and_then(|b| <[u8; HASH]>::try_from(b).ok())
                    .map(|b| Hash32::from_raw(u32::from_le_bytes(b)))
                    .ok_or_else(|| site.refuse("the name hashes run past the blob"))?;
                let data = match count(i) {
                    0 => EntryData::Texture(pointer_in::<T>(d, rx)?),
                    rows => {
                        let run = offset(d)
                            .and_then(|at| bytes.get(at..at + rows * ROW))
                            .ok_or_else(|| {
                                site.refuse(format!("entry {i}'s rows are not inside the blob"))
                            })?;
                        EntryData::Float(
                            run.as_chunks::<ROW>()
                                .0
                                .iter()
                                .filter_map(|row| Vec4::read_from(row))
                                .collect(),
                        )
                    }
                };
                entries.push(GrcInstanceDataEntry {
                    register: bytes
                        [i * stride + offset_of!(Raw<layout::GrcInstanceDataEntry>, register)],
                    name_hash,
                    data,
                });
            }
            let laid = lay_out(&entries).map_err(|e| site.refuse(e))?;
            let offsets: Vec<Option<usize>> = entries
                .iter()
                .zip(data)
                .map(|(e, d)| match e.data {
                    EntryData::Texture(_) => None,
                    EntryData::Float(_) => offset(d),
                })
                .collect();
            let canonical = laid.bytes == bytes
                && laid.rows == offsets
                && stated(names.entry_count)? == data.len() as i128
                && stated(names.hash_offset)? == laid.hashes as i128
                && stated(names.blob_size)? == laid.bytes.len() as i128
                && stated(names.texture_count)?
                    == entries.iter().filter(|e| is_texture(e)).count() as i128;
            if !canonical {
                return Err(site.refuse(
                    "the blob is not the one its entries lay out, so the owned value could not \
                     write it back",
                ));
            }
            // ⚠ A tool that appends a sampler after the constants writes a file the engine misreads,
            // since it reads the textures as a prefix; the textures lead from here, so the save fixes it.
            entries.sort_by_key(|e| !is_texture(e));
            Ok(entries)
        }

        fn save(&self, site: &mut SiteMut<'_>, cx: &Cx) -> Result<(), Error> {
            let names = material_names(|n| site.int(n));
            let textures = texture_count(self).map_err(|e| site.refuse(e))?;
            site.set_int(names.entry_count, self.len() as i128)?;
            site.set_int(names.texture_count, textures as i128)?;
            if self.is_empty() {
                site.set_int(names.hash_offset, 0)?;
                site.set_int(names.blob_size, 0)?;
                site.put(Value::null_ptr());
                return Ok(());
            }
            let laid = lay_out(self).map_err(|e| site.refuse(e))?;
            let mut data = Vec::with_capacity(self.len());
            for (e, at) in self.iter().zip(&laid.rows) {
                data.push(match (&e.data, at) {
                    (EntryData::Texture(t), _) => t.to_element(cx)?,
                    (EntryData::Float(_), at) => {
                        Value::Scalar(ScalarValue::Int(at.unwrap_or_default() as i128))
                    }
                });
            }
            site.set_int(names.hash_offset, laid.hashes as i128)?;
            site.set_int(names.blob_size, laid.bytes.len() as i128)?;
            site.put(Value::ptr_to(Value::Blob {
                codec: BLOB.to_string(),
                parts: vec![
                    (BYTES.to_string(), Value::Opaque(laid.bytes)),
                    (
                        DATA.to_string(),
                        Value::Array {
                            elements: data,
                            capacity: None,
                        },
                    ),
                ],
            }));
            Ok(())
        }
    }
}

/// The owned form of an Enhanced material's parameter block.
#[cfg(feature = "gen9")]
pub mod gen9 {
    use super::{
        slots_of, Names, AT, BLOCK, BYTES, OWNED, PARAM_DATA_SIZE, PARAM_INFOS, RAW, SLOTS,
        SLOTS_BLOB, SLOT_WIDTH, TARGET,
    };
    use std::mem::size_of;

    use crate::decl::{pointer_in, Codec, Cx, Heap, Owned, Rx, Site, SiteMut, Vec4};
    use crate::desc::value::ScalarValue;
    use crate::desc::Value;
    use crate::error::Error;

    /// An Enhanced material's parameter block as one worker thread's copy
    /// holds it: each constant buffer's bytes, each texture slot and one byte per sampler.
    ///
    /// ⚠ A save writes the buffer, texture, other and sampler counts into the
    /// material's `param_infos`, writes the buffers and their pointers once per
    /// copy the infos state, and nulls every copy's texture slots but the
    /// first. Measured: 77 of 77 Enhanced blocks in the shared corpus, each
    /// of 12 copies, hold their later copies' texture slots null
    /// (`every_material_and_skeleton_opens_owned_and_saves_as_its_tree_saves`, on c4cec2e0).
    #[derive(Debug, Clone, PartialEq)]
    pub struct Params<T> {
        pub buffers: Vec<Vec<u8>>,
        pub textures: Vec<T>,
        pub samplers: Vec<u8>,
        /// How many other slots, the kind-1 parameters' (`num_unknowns`), each copy holds.
        ///
        /// ⚠ Their number is all a file states of them: a block whose other
        /// slots are not null in every copy is refused. Measured: no drawable
        /// the 317,514 archetypes of both retail installs name is refused
        /// (`what_an_archetype_takes_from_its_drawable`).
        #[doc(hidden)]
        pub others: usize,
    }

    impl<T> Default for Params<T> {
        fn default() -> Self {
            Self {
                buffers: Vec::new(),
                textures: Vec::new(),
                samplers: Vec::new(),
                others: 0,
            }
        }
    }

    /// Bytes in a shader register, the unit a constant buffer is sized in.
    const REGISTER: usize = size_of::<Vec4>();

    /// What `param_infos` says the block holds.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Counts {
        buffers: usize,
        textures: usize,
        others: usize,
        samplers: usize,
        copies: usize,
    }

    /// The names the infos' counts go by, whichever set their declaration uses.
    fn infos_names(infos: Option<&Value>) -> &'static Names {
        let owned = match infos {
            Some(Value::Ptr {
                target: Some(t), ..
            }) => t
                .record()
                .is_some_and(|r| r.int(OWNED.buffer_count).is_some()),
            _ => false,
        };
        if owned {
            &OWNED
        } else {
            &RAW
        }
    }

    fn counts(infos: Option<&Value>, names: &Names) -> Result<Counts, String> {
        let record = match infos {
            Some(Value::Ptr {
                target: Some(t), ..
            }) => t.record(),
            _ => None,
        }
        .ok_or("`param_infos` is not a pointer at the infos that count the block's slots")?;
        let n = |field: &str| {
            record
                .int(field)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| format!("the infos state no `{field}`"))
        };
        Ok(Counts {
            buffers: n(names.buffer_count)?,
            textures: n(names.textures_g9)?,
            others: n(names.other_count)?,
            samplers: n(names.sampler_count)?,
            // ⚠ At least one copy, as the reader takes it.
            copies: n(names.copies)?.max(1),
        })
    }

    /// Make the infos count what `p` holds.
    fn state_counts<T>(
        infos: Option<&mut Value>,
        p: &Params<T>,
        names: &Names,
    ) -> Result<(), String> {
        let record = infos
            .and_then(Value::target_mut)
            .and_then(Value::record_mut)
            .ok_or("`param_infos` is not a pointer at the infos that count the block's slots")?;
        for (field, n) in [
            (names.buffer_count, p.buffers.len()),
            (names.textures_g9, p.textures.len()),
            (names.other_count, p.others),
            (names.sampler_count, p.samplers.len()),
        ] {
            record
                .set_structural(field, ScalarValue::Int(n as i128))
                .map_err(|e| format!("the infos cannot count {n} for `{field}`: {e}"))?;
        }
        Ok(())
    }

    enum Slot {
        Offset(usize),
        Texture(usize),
        Null,
    }

    /// A block as the canonical writer lays it out: the buffer pointers, the
    /// buffers copy by copy, the texture slots copy by copy, the other slots
    /// copy by copy, then the samplers.
    struct Laid {
        bytes: Vec<u8>,
        slots: Vec<(usize, Slot)>,
        textures_at: Option<usize>,
        others_at: Option<usize>,
    }

    fn lay_out<T>(p: &Params<T>, copies: usize) -> Result<Laid, String> {
        if let Some((i, b)) = p
            .buffers
            .iter()
            .enumerate()
            .find(|(_, b)| b.len() % REGISTER != 0)
        {
            return Err(format!(
                "constant buffer {i} is {} byte(s), not whole {REGISTER}-byte registers",
                b.len()
            ));
        }
        let (buffers, textures) = (p.buffers.len(), p.textures.len());
        let mut slots = Vec::with_capacity((buffers + textures + p.others) * copies);
        let mut payloads = Vec::with_capacity(buffers * copies);
        let mut at = (buffers * copies * SLOT_WIDTH).next_multiple_of(REGISTER);
        for copy in 0..copies {
            for (k, b) in p.buffers.iter().enumerate() {
                slots.push(((copy * buffers + k) * SLOT_WIDTH, Slot::Offset(at)));
                payloads.push((at, b));
                at += b.len();
            }
        }
        let textures_at = (textures > 0).then_some(at);
        for copy in 0..copies {
            for t in 0..textures {
                slots.push((
                    at,
                    if copy == 0 {
                        Slot::Texture(t)
                    } else {
                        Slot::Null
                    },
                ));
                at += SLOT_WIDTH;
            }
        }
        let others_at = (p.others > 0).then_some(at);
        for _ in 0..p.others * copies {
            slots.push((at, Slot::Null));
            at += SLOT_WIDTH;
        }
        let mut bytes = vec![0u8; at + p.samplers.len()];
        for (start, b) in payloads {
            bytes[start..start + b.len()].copy_from_slice(b);
        }
        bytes[at..].copy_from_slice(&p.samplers);
        Ok(Laid {
            bytes,
            slots,
            textures_at,
            others_at,
        })
    }

    fn offset(v: &Value) -> Option<usize> {
        match v {
            Value::Scalar(ScalarValue::Int(at)) => usize::try_from(*at).ok(),
            _ => None,
        }
    }

    impl<T: Heap> Heap for Params<T> {
        fn heap_bytes(&self) -> usize {
            self.buffers.heap_bytes() + self.textures.heap_bytes() + self.samplers.heap_bytes()
        }
    }

    impl<T: Owned> Codec for Params<T> {
        fn open(site: &Site<'_>, rx: &Rx<'_>) -> Result<Self, Error> {
            let names = infos_names(site.child(PARAM_INFOS));
            let n = counts(site.child(PARAM_INFOS), names).map_err(|e| site.refuse(e))?;
            let size = site
                .int(PARAM_DATA_SIZE)
                .ok_or_else(|| site.refuse("no `param_data_size`"))?;
            let blob = match site.value() {
                Some(Value::Ptr { target: None, .. })
                    if n.buffers + n.textures + n.others + n.samplers == 0 && size == 0 =>
                {
                    return Ok(Self::default())
                }
                Some(Value::Ptr {
                    target: Some(blob), ..
                }) => blob.as_ref(),
                _ => return Err(site.refuse("the parameters are not a pointer at a block")),
            };
            let (bytes, slots) = match blob {
                Value::Blob { codec, .. } if codec == BLOCK => (
                    blob.part(BYTES).and_then(Value::as_bytes),
                    blob.part(SLOTS).and_then(slots_of),
                ),
                _ => (None, None),
            };
            let (Some(bytes), Some(slots)) = (bytes, slots) else {
                return Err(site.refuse(format!("the block is not a `{BLOCK}`")));
            };
            let malformed = || {
                site.refuse(
                    "the block is not the one its buffers, texture and other slots and samplers \
                     lay out, so the owned value could not write it back",
                )
            };
            let table = n.buffers * n.copies;
            let textures_at = blob.part(names.siblings[0]).and_then(offset);
            let samplers_at = bytes.len().checked_sub(n.samplers).ok_or_else(malformed)?;
            // Each buffer runs to where the next one's pointer says the next begins.
            let starts = slots
                .iter()
                .take(table)
                .map(|(_, t)| offset(t))
                .collect::<Option<Vec<usize>>>()
                .filter(|s| s.len() == table)
                .ok_or_else(malformed)?;
            let others_at = blob.part(names.siblings[1]).and_then(offset);
            let end = textures_at.or(others_at).unwrap_or(samplers_at);
            let mut buffers = Vec::with_capacity(n.buffers);
            for (k, start) in starts.iter().take(n.buffers).enumerate() {
                let stop = starts.get(k + 1).copied().unwrap_or(end);
                buffers.push(bytes.get(*start..stop).ok_or_else(malformed)?.to_vec());
            }
            let textures = slots
                .iter()
                .skip(table)
                .take(n.textures)
                .map(|(_, t)| pointer_in::<T>(t, rx))
                .collect::<Result<Vec<T>, Error>>()?;
            let params = Self {
                buffers,
                textures,
                samplers: bytes[samplers_at..].to_vec(),
                others: n.others,
            };
            let laid = lay_out(&params, n.copies).map_err(|e| site.refuse(e))?;
            let canonical = laid.bytes == bytes
                && size == bytes.len() as i128
                && laid.textures_at == textures_at
                && laid.others_at == others_at
                && laid.slots.len() == slots.len()
                && laid
                    .slots
                    .iter()
                    .zip(&slots)
                    .all(|((at, want), (got_at, got))| {
                        at == got_at
                            && match want {
                                Slot::Offset(o) => offset(got) == Some(*o),
                                Slot::Texture(_) => true,
                                Slot::Null => matches!(got, Value::Ptr { target: None, .. }),
                            }
                    });
            if !canonical {
                return Err(malformed());
            }
            Ok(params)
        }

        fn save(&self, site: &mut SiteMut<'_>, cx: &Cx) -> Result<(), Error> {
            let names = infos_names(site.child(PARAM_INFOS));
            if let Err(e) = state_counts(site.child_mut(PARAM_INFOS), self, names) {
                return Err(site.refuse(e));
            }
            let n = counts(site.child(PARAM_INFOS), names).map_err(|e| site.refuse(e))?;
            if (n.buffers, n.textures, n.others, n.samplers) == (0, 0, 0, 0) {
                site.set_int(PARAM_DATA_SIZE, 0)?;
                site.put(Value::null_ptr());
                return Ok(());
            }
            let laid = lay_out(self, n.copies).map_err(|e| site.refuse(e))?;
            let mut ats = Vec::with_capacity(laid.slots.len());
            let mut targets = Vec::with_capacity(laid.slots.len());
            for (at, slot) in &laid.slots {
                ats.push(Value::Scalar(ScalarValue::Int(*at as i128)));
                targets.push(match slot {
                    Slot::Offset(o) => Value::Scalar(ScalarValue::Int(*o as i128)),
                    Slot::Texture(t) => self.textures[*t].to_element(cx)?,
                    Slot::Null => Value::null_ptr(),
                });
            }
            site.set_int(PARAM_DATA_SIZE, laid.bytes.len() as i128)?;
            let run = |elements| Value::Array {
                elements,
                capacity: None,
            };
            site.put(Value::ptr_to(Value::Blob {
                codec: BLOCK.to_string(),
                parts: vec![
                    (BYTES.to_string(), Value::Opaque(laid.bytes)),
                    (
                        SLOTS.to_string(),
                        Value::Blob {
                            codec: SLOTS_BLOB.to_string(),
                            parts: vec![
                                (AT.to_string(), run(ats)),
                                (TARGET.to_string(), run(targets)),
                            ],
                        },
                    ),
                    (
                        names.siblings[0].to_string(),
                        laid.textures_at.map_or_else(Value::null_ptr, |at| {
                            Value::Scalar(ScalarValue::Int(at as i128))
                        }),
                    ),
                    (
                        names.siblings[1].to_string(),
                        laid.others_at.map_or_else(Value::null_ptr, |at| {
                            Value::Scalar(ScalarValue::Int(at as i128))
                        }),
                    ),
                ],
            }));
            Ok(())
        }
    }
}
