//! `.ycd`: animation clips keyed by name hash, the animations they play,
//! and the tags and properties they carry.
//!
//! ⚠ **The root is a hash map, not a sorted dictionary**: a table of bucket
//! heads, each a chain of entries, and an entry's bucket is its key modulo
//! the slot count. [`validate`] refuses a map whose chains disagree with
//! their keys, because a lookup walks one bucket and misses the rest.
//!
//! ⚠ **An animation's channel data is carried as bytes.** A block stream's
//! nine channel encodings are bit-packed per frame; [`crate::codec::anim`]
//! decodes and re-encodes them through the traits these types implement.
//!
//! ⚠ **A clip names its animation by the animation map's key.** The file
//! holds each animation behind its map entry; a clip's pointer at it is a
//! [`Keyed`] reference to that entry's key, and each animation's reference
//! count is written at save as the clips naming it plus its entry.

use super::shared::{AtString, PgBase, PgBaseRefCounted, QuatV, Vec3V};
use crate::codec::anim;
use crate::decl::*;
use crate::desc::Value;
use crate::Generation;

impl Resource for CrClipDictionary {
    const EXT: &'static str = "ycd";
    const TARGET: Generation = Generation::Gen8;
}

/// `atMap<u32, datOwner<T>>`: a chained hash map from a 32-bit key.
///
/// ⚠ A lookup walks only the bucket its key modulo the bucket count names,
/// so an entry chained under another bucket is never found.
#[record(size = 0x10, recompute = Self::count)]
pub struct AtMap<T> {
    #[len(field = "slots")]
    pub hash: Vec<Option<Box<AtMapEntry<T>>>>,
    #[note("buckets, written from the table's length")]
    slots: u16,
    #[note("entries across every bucket's chain; written at save")]
    used: u16,
    #[note("an empty functor object's byte; carried")]
    hash_fn: u8,
    #[note("an empty functor object's byte; carried")]
    equals: u8,
    #[note("an empty functor object's byte; carried")]
    memory: u8,
    pub allow_re_compute: u8,
}

impl<T: Owned> AtMap<T>
where
    Raw<T>: Record,
{
    /// The entry count a save states: every entry of every bucket's chain.
    fn count(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        let used = self.entries().count();
        raw.used = u16::try_from(used).map_err(|_| OwnedError::Invalid {
            what: "AtMap.hash".into(),
            why: format!("{used} entries, and the count is sixteen bits"),
        })?;
        Ok(())
    }

    /// Every entry, bucket by bucket, each chain in order.
    pub fn entries(&self) -> impl Iterator<Item = &AtMapEntry<T>> {
        self.hash
            .iter()
            .flat_map(|head| std::iter::successors(head.as_deref(), |e| e.next.as_deref()))
    }
}

/// One entry of an [`AtMap`] and the next in its bucket.
#[record(size = 0x18)]
pub struct AtMapEntry<T> {
    #[pad_after(4)]
    pub key: u32,
    #[keyed_by(key)]
    pub data: Option<Box<T>>,
    pub next: Option<Box<AtMapEntry<T>>>,
}

/// A `.ycd`'s root: its clips, and the dictionary of animations they play.
#[record(size = 0x40)]
pub struct CrClipDictionary {
    pub base: PgBaseRefCounted,
    pub anim_dictionary: Option<Box<CrAnimDictionary>>,
    pub anim_dictionary_owner: Bool8,
    pub base_name_keys: Bool8,
    #[pad_after(4)]
    padding: [u8; 2],
    /// ⚠ The engine's dictionary ends here; a resource allocates it to 0x40.
    #[pad_after(8)]
    pub clips: AtMap<CrClipAny>,
}

/// The animations a dictionary's clips play.
#[record(size = 0x30)]
pub struct CrAnimDictionary {
    pub base: PgBaseRefCounted,
    pub animations: AtMap<CrAnimation>,
    pub base_name_keys: Bool8,
    #[pad_after(4)]
    padding: [u8; 3],
}

/// What kind of clip follows the header.
#[record(enum, width = u32)]
pub enum EClipType {
    /// None
    None = 0,
    /// Animation
    Animation = 1,
    /// Animations
    Animations = 2,
    /// AnimationExpression
    AnimationExpression = 3,
}

/// What every clip starts with; `type` says which layout follows.
///
/// Every clip the corpus holds is one of the two variants declared.
#[record(
    size = 0x50,
    tag = "type",
    variant(1 => CrClipAnimation),
    variant(2 => CrClipAnimations)
)]
pub struct CrClip {
    pub base: PgBase,
    #[pad_after(4)]
    pub r#type: Open<EClipType>,
    pub name: AtString,
    /// The dictionary holding the clip: its root.
    dictionary: Back<CrClipDictionary>,
    #[pad_after(7)]
    pub flags: u8,
    pub tags: Option<Box<CrTags>>,
    pub properties: Option<Box<CrProperties>>,
    #[refs]
    #[pad_after(4)]
    ref_count: u32,
}

/// A clip playing one animation, over a window of it at a rate.
#[record(size = 0x70)]
pub struct CrClipAnimation {
    pub base: CrClip,
    pub animation: Keyed<CrAnimation>,
    pub start_time: f32,
    pub end_time: f32,
    #[pad_after(12)]
    pub rate: f32,
}

/// One animation of a clip that plays several.
#[record(size = 0x18)]
pub struct Animation {
    pub start_time: f32,
    pub end_time: f32,
    #[pad_after(4)]
    pub rate: f32,
    pub animation: Keyed<CrAnimation>,
}

/// A clip playing several animations, in parallel or one after another.
#[record(size = 0x70)]
pub struct CrClipAnimations {
    pub base: CrClip,
    pub animations: Vec<Animation>,
    pub total_duration: f32,
    #[pad_after(11)]
    pub parallel: Bool8,
}

/// One bit-packed block of an animation's channels, its bytes trailing the header.
#[record(size = 0x20)]
pub struct CrBlockStream {
    pub hash: u32,
    #[note("bytes trailing the header, written from the run's length")]
    data_size: u32,
    pub compact_size: u32,
    pub constant_size: u32,
    pub channel_offset: u32,
    pub slop_size: u16,
    pub num_frames: u16,
    pub frame_size: u16,
    pub indirect_size: u16,
    pub quantize_size: u16,
    pub segment_size: u8,
    pub num_mover_channels: u8,
    #[trailing]
    #[len(field = "data_size")]
    pub data: Vec<u8>,
}

/// One degree of freedom an animation drives: a bone, a track and its type.
#[record(size = 0x04)]
pub struct Dof {
    /// A bone's tag, not its index in a skeleton.
    pub id: u16,
    pub r#type: u8,
    pub track: u8,
}

/// An animation: its frames, the blocks holding its channels and the channels it drives.
#[record(size = 0x60)]
pub struct CrAnimation {
    pub base: PgBase,
    pub flags: u16,
    pub project_flags: u16,
    pub num_frames: u16,
    pub frames_per_chunk: u16,
    pub duration: f32,
    pub signature: u32,
    pub name: String,
    #[note("the runtime's track pointers, empty in every shipped animation; carried")]
    tracks: Vec<Ptr<Opaque>>,
    pub max_block_size: u32,
    #[refs]
    ref_count: u32,
    pub blocks: Vec<Option<Box<CrBlockStream>>>,
    pub dofs: Vec<Dof>,
}

/// The tags a clip is marked with over time.
#[record(size = 0x20)]
pub struct CrTags {
    pub tags: Vec<Option<Box<CrTag>>>,
    /// ⚠ The engine's list ends at 0x18; a resource allocates it to 0x20.
    #[pad_after(15)]
    pub has_block_tags: Bool8,
}

/// A clip's named properties.
#[record(size = 0x10)]
pub struct CrProperties {
    pub properties: AtMap<CrProperty>,
}

/// A name and the hash it is looked up by.
#[record(size = 0x10)]
pub struct StringKey {
    pub string: String,
    #[pad_after(4)]
    pub key: u32,
}

/// A named set of attributes.
#[record(size = 0x40)]
pub struct CrProperty {
    pub base: PgBase,
    pub name: StringKey,
    pub attributes: Vec<Option<Box<CrPropertyAttributeAny>>>,
    #[note("a runtime reference to the properties holding it; carried")]
    properties: Ptr<Opaque>,
    #[pad_after(4)]
    pub signature: u32,
}

/// A property over a window of a clip's time, and the tags list holding it.
#[record(size = 0x50)]
pub struct CrTag {
    pub base: CrProperty,
    pub start: f32,
    pub end: f32,
    /// The list the tag is in.
    tags: Back<CrTags>,
}

/// What kind of value an attribute holds.
#[record(enum, width = u32)]
pub enum EType {
    /// None
    None = 0,
    /// Float
    Float = 1,
    /// Int
    Int = 2,
    /// Bool
    Bool = 3,
    /// String
    String = 4,
    /// BitSet
    BitSet = 5,
    /// Vector3
    Vector3 = 6,
    /// Vector4
    Vector4 = 7,
    /// Quaternion
    Quaternion = 8,
    /// Matrix34
    Matrix34 = 9,
    /// Situation
    Situation = 10,
    /// Data
    Data = 11,
    /// HashString
    HashString = 12,
}

/// What every attribute starts with; `type` says which value follows.
#[record(
    size = 0x20,
    tag = "type",
    variant(1 => CrPropertyAttributeFloat),
    variant(2 => CrPropertyAttributeInt),
    variant(3 => CrPropertyAttributeBool),
    variant(6 => CrPropertyAttributeVector3),
    variant(8 => CrPropertyAttributeQuaternion),
    variant(12 => CrPropertyAttributeHashString)
)]
pub struct CrPropertyAttribute {
    #[note("a runtime address, carried")]
    vft: u64,
    #[pad_after(4)]
    pub r#type: Open<EType>,
    pub name: StringKey,
}

/// An attribute holding a float.
#[record(size = 0x28)]
pub struct CrPropertyAttributeFloat {
    pub base: CrPropertyAttribute,
    #[pad_after(4)]
    pub float: f32,
}

/// An attribute holding an integer.
#[record(size = 0x28)]
pub struct CrPropertyAttributeInt {
    pub base: CrPropertyAttribute,
    #[pad_after(4)]
    pub int: i32,
}

/// An attribute holding a string's hash.
#[record(size = 0x28)]
pub struct CrPropertyAttributeHashString {
    pub base: CrPropertyAttribute,
    #[pad_after(4)]
    pub hash_string: u32,
}

/// An attribute holding a flag.
#[record(size = 0x28)]
pub struct CrPropertyAttributeBool {
    pub base: CrPropertyAttribute,
    #[pad_after(7)]
    pub bool: Bool8,
}

/// An attribute holding a vector.
#[record(size = 0x30)]
pub struct CrPropertyAttributeVector3 {
    pub base: CrPropertyAttribute,
    pub vector3: Vec3V,
}

/// An attribute holding a rotation.
#[record(size = 0x30)]
pub struct CrPropertyAttributeQuaternion {
    pub base: CrPropertyAttribute,
    pub quaternion: QuatV,
}

/// A map entry and the index of the bucket it was found in.
pub type Bucketed<'a, T> = (usize, View<'a, __AtMapEntryRaw<T>>);

/// Every entry of a map, bucket by bucket, each with its bucket's index.
pub fn entries<'a, T: Record>(
    map: &View<'a, __AtMapRaw<T>>,
) -> Result<Vec<Bucketed<'a, T>>, ViewError> {
    let buckets = map.hash()?;
    let mut out = Vec::new();
    for b in 0..buckets.len() {
        let mut next = buckets.follow(b)?;
        let mut guard = 0usize;
        while let Some(entry) = next {
            guard += 1;
            if guard > usize::from(map.used) + 1 {
                return Err(ViewError::BadPointer {
                    ty: <__AtMapEntryRaw<T> as Described>::name().into(),
                    field: "next",
                    address: entry.next.raw(),
                    reason: "a bucket's chain is longer than the map".into(),
                });
            }
            next = entry.next()?;
            out.push((b, entry));
        }
    }
    Ok(out)
}

/// A clip dictionary the game would misread, and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("the {map} map states {states} entries and its buckets hold {holds}")]
    EntryCount {
        map: &'static str,
        states: u16,
        holds: usize,
    },
    #[error("the {map} map holds key {key:#010x} in bucket {bucket} of {slots}, and a lookup walks bucket {expected}")]
    WrongBucket {
        map: &'static str,
        key: u32,
        bucket: usize,
        expected: usize,
        slots: usize,
    },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// A map's entry count and its buckets, checked against its keys.
pub fn check_map<T: Record>(
    map: &View<'_, __AtMapRaw<T>>,
    what: &'static str,
) -> Result<(), Broken> {
    let slots = usize::from(map.slots);
    let all = entries(map)?;
    if all.len() != usize::from(map.used) {
        return Err(Broken::EntryCount {
            map: what,
            states: map.used,
            holds: all.len(),
        });
    }
    for (bucket, entry) in &all {
        let expected = entry.key as usize % slots.max(1);
        if *bucket != expected {
            return Err(Broken::WrongBucket {
                map: what,
                key: entry.key,
                bucket: *bucket,
                expected,
                slots,
            });
        }
    }
    Ok(())
}

/// The clip map, the animation map and every clip's property map.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    let dict = View::<Raw<CrClipDictionary>>::at_offset(image, root)?;
    let clips = dict.clips();
    check_map(&clips, "clip")?;
    if let Some(animations) = dict.anim_dictionary()? {
        check_map(&animations.animations(), "animation")?;
    }
    for (_, entry) in entries(&clips)? {
        if let Some(clip) = entry.data()? {
            if let Some(properties) = clip.properties()? {
                check_map(&properties.properties(), "property")?;
            }
        }
    }
    Ok(())
}

impl anim::BlockSource for CrBlockStream {
    fn data(&self) -> &[u8] {
        &self.data
    }
    fn compact_size(&self) -> u32 {
        self.compact_size
    }
    fn constant_size(&self) -> u32 {
        self.constant_size
    }
    fn num_frames(&self) -> u16 {
        self.num_frames
    }
    fn frame_size(&self) -> u16 {
        self.frame_size
    }
    fn segment_size(&self) -> u8 {
        self.segment_size
    }
}

impl From<anim::EncodedBlock> for CrBlockStream {
    fn from(block: anim::EncodedBlock) -> Self {
        Self {
            constant_size: block.constant_size,
            channel_offset: block.channel_offset,
            num_frames: block.num_frames,
            frame_size: block.frame_size,
            segment_size: block.segment_size,
            data: block.data,
            ..Self::default()
        }
    }
}

impl anim::AnimationSource for CrAnimation {
    type Block = CrBlockStream;

    fn name(&self) -> &str {
        &self.name
    }
    fn num_frames(&self) -> u16 {
        self.num_frames
    }
    fn frames_per_chunk(&self) -> u16 {
        self.frames_per_chunk
    }
    fn duration(&self) -> f32 {
        self.duration
    }
    fn dof_count(&self) -> usize {
        self.dofs.len()
    }
    fn dof_format(&self, index: usize) -> Option<u8> {
        self.dofs.get(index).map(|d| d.r#type)
    }
    fn find_dof(&self, id: u16, track: u8) -> Option<usize> {
        self.dofs
            .iter()
            .position(|d| d.id == id && d.track == track)
    }
    fn block_count(&self) -> usize {
        self.blocks.len()
    }
    fn block(&self, index: usize) -> Option<&CrBlockStream> {
        self.blocks.get(index)?.as_deref()
    }
}

impl anim::AnimationSink for CrAnimation {
    fn store_blocks(&mut self, blocks: Vec<anim::EncodedBlock>, max_block_size: u32) {
        self.blocks = blocks
            .into_iter()
            .map(|b| Some(Box::new(b.into())))
            .collect();
        self.max_block_size = max_block_size;
        self.flags &= !anim::COMPACT;
    }
}
