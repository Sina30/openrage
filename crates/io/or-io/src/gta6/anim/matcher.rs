//! A motion dictionary's motion matchers: each cuts its clips into entries of frames, and its
//! database holds a skeleton, a list of features and, for each frame, what those features read.

use std::ops::Range;

use super::{AnimError, Section};
use crate::gta6::hash::name_hash32;
use crate::gta6::headerless::{read_skeleton_at, Skeleton};

const MATCHER_LEN: usize = 0x1C0;
const ENTRY_LEN: usize = 24;
const CLIP_LEN: usize = 16;
const FEATURE_LEN: usize = 0x28;
const FRAME_RECORD_LEN: usize = 48;
/// Sixteen f16 halves.
const BLOCK_LEN: usize = 32;
const LEVEL_LEN: usize = 32;
/// The smallest object at a matcher's 0x48 seen; others run to 48 or 64 bytes.
const OBJECT_48_LEN: usize = 40;
/// The smallest object at a class-1 database's 0x30 seen; others run to 0x70 bytes.
const OBJECT_30_LEN: usize = 0x30;
/// The control byte of an empty slot of a chunked map; a full slot's is below it.
const EMPTY: u8 = 0x80;

/// A motion matcher: its clips, each cut into entries that own a range of its frames, and its database.
#[derive(Debug, Clone)]
pub struct Matcher {
    pub name: String,
    /// The hash of the name without `pack:/` and `.motionmatcher`.
    pub key: u32,
    pub guid: [u8; 16],
    /// In clip index order.
    pub clips: Vec<MatcherClip>,
    /// Clip by clip, the starts of each clip's entries rising.
    pub entries: Vec<MatcherEntry>,
    /// The keys of the map at 0x170 in the order of the indices it maps them to; their role is not known.
    pub keys_at_170: Vec<u32>,
    /// The words at 0x190, whose roles are not known.
    pub words_at_190: [u32; 2],
    /// The first bytes of the object at 0x48, as many as the smallest one seen holds; role not known.
    pub object_at_48: Vec<u8>,
    /// The slot at 0xB0: one byte for each clip, whose role is not known.
    pub clip_bytes: Vec<u8>,
    /// The one byte of the slot at 0x110, whose role is not known.
    pub byte_at_110: u8,
    pub database: MatcherDatabase,
}

/// A clip a matcher names by the 64-bit key `alias << 32 | leaf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatcherClip {
    /// The hash of the clip's leaf name.
    pub leaf: u32,
    /// One word for each clip dictionary; which hash of which name is not known.
    pub alias: u32,
    /// The clip dictionary's key, where the matcher lists its clips.
    pub dictionary: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatcherEntry {
    /// The clip's index into [`Matcher::clips`].
    pub clip: u32,
    /// Seconds into the clip.
    pub start: f32,
    /// A rate in Hz by its values; not confirmed.
    pub rate: f32,
    /// The database frames the entry owns.
    pub frames: Range<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatcherDatabase {
    pub skeleton: Skeleton,
    pub features: Vec<MatcherFeature>,
    pub frames: u32,
    /// The hashes at 0x68: a class-3 database's names its kind-5 feature; a class-1 one's role is not known.
    pub hashes_at_68: Vec<u32>,
    /// One word for each frame at 0x90: flags in class 1 and 16-bit masks in class 3 by their values, whose meaning is not known.
    pub frame_words: Vec<u32>,
    /// The float at 0xC0, whose role is not known.
    pub word_at_c0: f32,
    /// One weight for each dimension at 0xB0, each positive, summing to 1.
    pub weights: Vec<f32>,
    /// The words at 0x78 to 0x88, whose roles are not known.
    pub words_at_78: [u32; 4],
    /// A class-1 database's object at 0x30, its first bytes as many as the smallest one seen holds.
    pub object_at_30: Vec<u8>,
    /// A class-1 frame record's floats 4 to 11 for each frame, whose meaning is not known.
    pub record_floats: Vec<[f32; 8]>,
    pub class: MatcherClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatcherFeature {
    /// The feature map's key for it.
    pub name: u32,
    /// The sum of the earlier features' dimensions.
    pub offset: u32,
    pub dims: u16,
    /// 1: a bone feature; 5: a feature that names no bone, a trajectory by its values.
    pub kind: u16,
    /// The words at 0x18, 0x1C and 0x24: a bone feature's bone hash and flags; a kind-5 feature's hash at 0x18 or 0x1C, by a rule not known.
    pub words: [u32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatcherClass {
    /// Class word 1: each frame's root rotation and its vector of f16 halves, whose meaning is not known.
    Vectors {
        rotations: Vec<[f32; 4]>,
        vectors: Vec<Vec<u16>>,
    },
    /// Class word 3: an order of the dimensions, each frame's stored dimensions and levels of bounds.
    Levels {
        /// Stored dimension k holds feature dimension `dimension_order[k]` by an inferred reading.
        dimension_order: Vec<u32>,
        /// The word at 0x12C: the stored dimensions in pairs, `ceil(dims / 2)`.
        pairs: u16,
        /// Each frame's dimensions in stored order as f16 halves, from the blocks at 0xE8.
        stored: Vec<Vec<u16>>,
        /// Coarsest first.
        levels: Vec<MatcherLevel>,
    },
}

/// A class-3 level: nodes of `pairs` elements, each lane of a node bounding a run of frames.
#[derive(Debug, Clone, PartialEq)]
pub struct MatcherLevel {
    /// The frames a lane covers, the product of the level words from this level on.
    pub lane_frames: u32,
    /// Element `node * pairs + q`: half j the least of dimension 2q in lane j, 8 + j of 2q + 1.
    pub minima: Vec<[u16; 16]>,
    /// Laid out as `minima`, each the greatest.
    pub maxima: Vec<[u16; 16]>,
}

fn s(e: AnimError) -> String {
    e.to_string()
}

fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| *b == 0)
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

impl Section<'_> {
    /// The matcher at `at`, keyed `key` in the root's map, of the dictionary keyed `root_key` whose
    /// clip dictionary list is `dictionaries`.
    pub(super) fn matcher(
        &self,
        at: usize,
        key: u64,
        root_key: u32,
        dictionaries: &[u32],
    ) -> Result<Matcher, AnimError> {
        let name = self.name("motion matcher name", at + 0x38)?;
        self.matcher_at(at, key, root_key, dictionaries, &name)
            .map_err(|why| AnimError::Motion {
                motion: name.clone(),
                why,
            })
    }

    fn matcher_at(
        &self,
        at: usize,
        key: u64,
        root_key: u32,
        dictionaries: &[u32],
        name: &str,
    ) -> Result<Matcher, String> {
        let head = self.bytes("motion matcher", at, MATCHER_LEN).map_err(s)?;
        let leaf = (name.strip_prefix("pack:/").unwrap_or(name))
            .strip_suffix(".motionmatcher")
            .ok_or("its name does not end in .motionmatcher")?;
        if key != u64::from(name_hash32(leaf.as_bytes()))
            || u64::from(le32(head, 0x188)) != key
            || le32(head, 0x18C) != root_key
        {
            return Err(
                "its key words at 0x188 are not its name's hash and its dictionary's key".into(),
            );
        }
        if !zero(&head[..0x10])
            || self.u64("motion matcher", at + 0x10).map_err(s)? != 1
            || self.pointer("motion matcher root", at + 0x18).map_err(s)? != Some(0)
        {
            return Err("its head is not {16 zero bytes, 1, the root}".into());
        }
        let zeros = [
            (0x80, 0x90),
            (0xC0, 0x110),
            (0x130, 0x170),
            (0x180, 0x188),
            (0x198, MATCHER_LEN),
        ];
        if !zeros.iter().all(|&(from, to)| zero(&head[from..to])) {
            return Err("it holds a non-zero byte where every matcher holds zero".into());
        }
        let mut guid = [0u8; 16];
        guid.copy_from_slice(&head[0x28..0x38]);
        if guid[7] >> 4 != 1 || guid[8] >> 6 != 2 {
            return Err("its bytes at 0x28 are not a version-1 GUID".into());
        }

        let (entries_at, count) = self.matcher_slot("matcher entries", at + 0x50, ENTRY_LEN)?;
        let frame_offsets = self.matcher_values("matcher frame offsets", at + 0x60, 4)?;
        let entry_offsets = self.matcher_values("matcher entry offsets", at + 0x70, 2)?;
        let clip_count = le32(head, 0xA4) as usize;
        let (clip_bytes_at, clip_bytes) = self.matcher_slot("matcher clip bytes", at + 0xB0, 1)?;
        if count == 0
            || clip_count == 0
            || frame_offsets.len() != count + 1
            || entry_offsets.len() != clip_count + 1
            || clip_bytes != clip_count
        {
            return Err(format!(
                "its {count} entries, {} frame offsets, {} entry offsets, {clip_count} mapped clips and {clip_bytes} clip bytes do not agree",
                frame_offsets.len(),
                entry_offsets.len()
            ));
        }
        let rises = |o: &[u32]| o.first() == Some(&0) && o.windows(2).all(|w| w[0] < w[1]);
        if !rises(&frame_offsets[..])
            || !rises(&entry_offsets[..])
            || entry_offsets[clip_count] as usize != count
        {
            return Err("its frame or entry offsets do not rise from 0 to their ends".into());
        }
        let frames = frame_offsets[count];
        let mut entries = Vec::with_capacity(count);
        for clip in 0..clip_count {
            let mut last = -1.0f32;
            for i in entry_offsets[clip] as usize..entry_offsets[clip + 1] as usize {
                let e = (self.bytes("matcher entry", entries_at + ENTRY_LEN * i, ENTRY_LEN))
                    .map_err(s)?;
                let float = |o: usize| f32::from_bits(le32(e, o));
                let (start, rate) = (float(0), float(4));
                let rising = start.is_finite() && rate.is_finite() && start >= 0.0 && start > last;
                if le32(e, 12) as usize != clip
                    || float(8).to_bits() != (-1.0f32).to_bits()
                    || !zero(&e[16..])
                    || !rising
                    || rate <= 0.0
                {
                    return Err(format!(
                        "entry {i} is not {{start, rate > 0, -1, clip {clip}, 0}} with starts rising"
                    ));
                }
                last = start;
                entries.push(MatcherEntry {
                    clip: clip as u32,
                    start,
                    rate,
                    frames: frame_offsets[i]..frame_offsets[i + 1],
                });
            }
        }

        let (list_at, listed) = self.matcher_slot("matcher clip list", at + 0x90, CLIP_LEN)?;
        if listed != 0 && listed != clip_count {
            return Err(format!(
                "its clip list holds {listed} clips, not 0 or {clip_count}"
            ));
        }
        let mut list = Vec::with_capacity(listed);
        for i in 0..listed {
            let e = self
                .bytes("matcher clip", list_at + CLIP_LEN * i, CLIP_LEN)
                .map_err(s)?;
            let dictionary = le32(e, 12);
            if !zero(&e[..8]) || !dictionaries.contains(&dictionary) {
                return Err(format!(
                    "clip {i} of its list is not {{0, a leaf, the key of a listed clip dictionary}}"
                ));
            }
            list.push((le32(e, 8), dictionary));
        }
        let mut clips = vec![None; clip_count];
        for slot in self.matcher_map("matcher clip map", at + 0xA0, CLIP_LEN, false)? {
            let e = self.bytes("matcher clip map", slot, CLIP_LEN).map_err(s)?;
            let (leaf, index) = (le32(e, 0), le32(e, 8) as usize);
            let listed = list.get(index);
            if le32(e, 12) != 0 || listed.is_some_and(|l| l.0 != leaf) {
                return Err("a slot of its clip map holds a non-zero last word or a leaf its clip list does not".into());
            }
            let Some(clip) = clips.get_mut(index).filter(|c| c.is_none()) else {
                return Err("its clip map's values are not each clip index once".into());
            };
            *clip = Some(MatcherClip {
                leaf,
                alias: le32(e, 4),
                dictionary: listed.map(|l| l.1),
            });
        }
        let clips = (clips.into_iter().collect::<Option<Vec<_>>>())
            .ok_or("its clip map does not hold every clip")?;

        let (at_110_first, at_110) = self.matcher_slot("matcher byte at 0x110", at + 0x110, 1)?;
        let (byte_at, at_120) = self.matcher_slot("matcher byte at 0x120", at + 0x120, 1)?;
        if at_110 != 1
            || at_120 != 1
            || !zero(self.bytes("matcher byte at 0x120", byte_at, 1).map_err(s)?)
        {
            return Err(
                "its slots at 0x110 and 0x120 do not each hold one byte, the second 0".into(),
            );
        }
        let slots = self.matcher_map("matcher map at 0x170", at + 0x170, 8, true)?;
        let keys_at_170 = self.matcher_permuted("matcher map at 0x170", &slots)?;
        let database = self.target("matcher database", at + 0x20).map_err(s)?;
        let clip_bytes = (self.bytes("matcher clip bytes", clip_bytes_at, clip_count))
            .map_err(s)?
            .to_vec();
        let byte_at_110 = self
            .bytes("matcher byte at 0x110", at_110_first, 1)
            .map_err(s)?[0];
        Ok(Matcher {
            name: name.to_string(),
            key: key as u32,
            guid,
            clips,
            entries,
            keys_at_170,
            words_at_190: [le32(head, 0x190), le32(head, 0x194)],
            object_at_48: self.leading_bytes("matcher object at 0x48", at + 0x48, OBJECT_48_LEN)?,
            clip_bytes,
            byte_at_110,
            database: self.matcher_database(database, frames)?,
        })
    }

    /// The first `len` bytes of the object the pointer at `at` aims at; none where it is null.
    fn leading_bytes(&self, what: &'static str, at: usize, len: usize) -> Result<Vec<u8>, String> {
        match self.pointer(what, at).map_err(s)? {
            Some(object) => Ok(self.bytes(what, object, len).map_err(s)?.to_vec()),
            None => Ok(Vec::new()),
        }
    }

    fn matcher_database(&self, d: usize, frames: u32) -> Result<MatcherDatabase, String> {
        let head = self.bytes("matcher database", d, 0xD0).map_err(s)?;
        if !zero(&head[..0x18])
            || self.u64("matcher database", d + 0x18).map_err(s)? != 1
            || !zero(&head[0x20..0x28])
        {
            return Err("its database's head is not {24 zero bytes, 1, 0}".into());
        }
        let skeleton_at = self.target("matcher skeleton", d + 0x28).map_err(s)?;
        let skeleton = read_skeleton_at(self.image.bytes(), skeleton_at)
            .map_err(|e| format!("its database's skeleton does not read: {e}"))?;
        if skeleton.bones.is_empty() {
            return Err("its database's skeleton holds no bone".into());
        }
        let (features_at, feature_count) = self.matcher_slot("matcher features", d + 0x38, 8)?;
        let slots = self.matcher_map("matcher feature map", d + 0x48, 8, true)?;
        let names = self.matcher_permuted("matcher feature map", &slots)?;
        if names.len() != feature_count {
            return Err(format!(
                "its feature map names {} features of {feature_count}",
                names.len()
            ));
        }
        let mut features = Vec::with_capacity(feature_count);
        let mut dims = 0u64;
        for (i, &name) in names.iter().enumerate() {
            let at = self
                .target("matcher feature", features_at + 8 * i)
                .map_err(s)?;
            let f = self.bytes("matcher feature", at, FEATURE_LEN).map_err(s)?;
            let half = |o: usize| u16::from_le_bytes([f[o], f[o + 1]]);
            let feature = MatcherFeature {
                name,
                offset: le32(f, 8),
                dims: half(12),
                kind: half(14),
                words: [le32(f, 0x18), le32(f, 0x1C), le32(f, 0x24)],
            };
            if !zero(&f[..8])
                || !zero(&f[0x10..0x18])
                || u64::from(feature.offset) != dims
                || feature.dims == 0
            {
                return Err(format!(
                    "feature {i}'s offset {} is not {dims}, the sum of the earlier features' dimensions, or its head is not zero",
                    feature.offset
                ));
            }
            match feature.kind {
                1 if !skeleton.bones.iter().any(|b| b.hash == feature.words[0]) => {
                    return Err(format!("bone feature {i} names no bone of its skeleton"))
                }
                1 | 5 => {}
                kind => {
                    return Err(format!(
                        "feature {i} has kind {kind}, where 1 and 5 are known"
                    ))
                }
            }
            dims += u64::from(feature.dims);
            features.push(feature);
        }
        let hashes_at_68 = self.matcher_values("matcher database hashes", d + 0x68, 4)?;
        let frame_words = self.matcher_values("matcher frame words", d + 0x90, 4)?;
        if frame_words.len() != frames as usize {
            return Err(format!(
                "its database holds {} frame words for {frames} frames",
                frame_words.len()
            ));
        }
        let word_at_c0 = f32::from_bits(le32(head, 0xC0));
        if !word_at_c0.is_finite() || le32(head, 0xC4) != 1f32.to_bits() {
            return Err("its database's floats at 0xC0 are not {a finite float, 1.0}".into());
        }
        let weights = self.matcher_weights(d, dims)?;
        let words_at_78 = std::array::from_fn(|k| le32(head, 0x78 + 4 * k));
        let object_at_30 = self.leading_bytes("matcher object at 0x30", d + 0x30, OBJECT_30_LEN)?;
        let (class, record_floats) = match le32(head, 0xC8) {
            1 => self.matcher_vectors(d, frames as usize, dims)?,
            3 => (self.matcher_levels(d, frames, dims)?, Vec::new()),
            other => {
                return Err(format!(
                    "its database's class word at 0xC8 is {other}, where 1 and 3 are known"
                ))
            }
        };
        Ok(MatcherDatabase {
            skeleton,
            features,
            frames,
            hashes_at_68,
            frame_words,
            word_at_c0,
            weights,
            words_at_78,
            object_at_30,
            record_floats,
            class,
        })
    }

    /// Class 1: a 48-byte record for each frame, opening with a unit quaternion, and a matrix of
    /// `ceil(dims / 16)` blocks for each frame whose halves past `dims` are zero; with each
    /// record's floats 4 to 11.
    fn matcher_vectors(
        &self,
        d: usize,
        frames: usize,
        dims: u64,
    ) -> Result<(MatcherClass, Vec<[f32; 8]>), String> {
        let (records_at, records) =
            self.matcher_slot("matcher frame records", d + 0xA0, FRAME_RECORD_LEN)?;
        if records != frames {
            return Err(format!(
                "its database holds {records} frame records for {frames} frames"
            ));
        }
        let mut rotations = Vec::with_capacity(frames);
        let mut record_floats = Vec::with_capacity(frames);
        for f in 0..frames {
            let r = (self.bytes(
                "matcher frame record",
                records_at + FRAME_RECORD_LEN * f,
                FRAME_RECORD_LEN,
            ))
            .map_err(s)?;
            let q: [f32; 4] = std::array::from_fn(|k| f32::from_bits(le32(r, 4 * k)));
            record_floats.push(std::array::from_fn(|k| f32::from_bits(le32(r, 16 + 4 * k))));
            let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
            if !length.is_finite() || (length - 1.0).abs() >= 1e-3 {
                return Err(format!(
                    "frame record {f} does not open with a unit quaternion"
                ));
            }
            rotations.push(q);
        }
        let blocks = self.u32("matcher database", d + 0x100).map_err(s)? as usize;
        let (matrix_at, cells) =
            self.matcher_slot("matcher feature matrix", d + 0xE8, BLOCK_LEN)?;
        if self.u32("matcher database", d + 0xF8).map_err(s)? as usize != frames
            || u64::from(self.u32("matcher database", d + 0xFC).map_err(s)?) != dims
            || blocks as u64 != dims.div_ceil(16)
            || self.u32("matcher database", d + 0x104).map_err(s)? != 1
            || frames.checked_mul(blocks) != Some(cells)
        {
            return Err(format!(
                "its feature matrix of {cells} blocks is not {frames} frames of ceil({dims} / 16) blocks"
            ));
        }
        let mut vectors = Vec::with_capacity(frames);
        for f in 0..frames {
            let len = BLOCK_LEN * blocks;
            let bytes =
                (self.bytes("matcher feature matrix", matrix_at + len * f, len)).map_err(s)?;
            let halves: Vec<u16> = (bytes.as_chunks::<2>().0.iter())
                .map(|h| u16::from_le_bytes(*h))
                .collect();
            // `blocks` is ceil(dims / 16), so the frame's halves hold its dims.
            let (vector, pad) = halves.split_at(dims as usize);
            if pad.iter().any(|h| *h != 0) {
                return Err(format!(
                    "frame {f}'s halves past its {dims} dimensions are not zero"
                ));
            }
            vectors.push(vector.to_vec());
        }
        Ok((MatcherClass::Vectors { rotations, vectors }, record_floats))
    }

    /// Class 3: a permutation of the dimensions, the frame and dimension counts again, the frames
    /// in blocks and levels of bounds over them.
    fn matcher_levels(&self, d: usize, frames: u32, dims: u64) -> Result<MatcherClass, String> {
        if !zero(self.bytes("matcher database", d + 0xA0, 16).map_err(s)?) {
            return Err("its class-3 database holds frame records at 0xA0".into());
        }
        let order = self.matcher_values("matcher dimension order", d + 0xF8, 4)?;
        let mut seen = vec![false; order.len()];
        for &k in &order {
            let Some(once) = seen.get_mut(k as usize).filter(|s| !**s) else {
                return Err("its dimension order is not a permutation of its dimensions".into());
            };
            *once = true;
        }
        if order.len() as u64 != dims
            || self.u32("matcher database", d + 0x128).map_err(s)? != frames
            || u64::from(self.u16("matcher database", d + 0x12E).map_err(s)?) != dims
        {
            return Err(format!(
                "its dimension order of {} and its words at 0x128 do not state its {frames} frames and {dims} dimensions",
                order.len()
            ));
        }
        let (levels_at, levels) = self.matcher_slot("matcher levels", d + 0x108, LEVEL_LEN)?;
        let words = self.matcher_values("matcher level words", d + 0x118, 4)?;
        if levels != words.len() {
            return Err(format!(
                "its database holds {levels} levels and {} level words",
                words.len()
            ));
        }
        let (pairs, stored, levels) =
            self.matcher_blocks(d, frames, order.len(), levels_at, &words)?;
        Ok(MatcherClass::Levels {
            dimension_order: order,
            pairs,
            stored,
            levels,
        })
    }

    /// The first element and count of the `{u32 count, u32 capacity, pointer}` head at `at`, each
    /// element `stride` bytes inside the section.
    fn matcher_slot(
        &self,
        what: &'static str,
        at: usize,
        stride: usize,
    ) -> Result<(usize, usize), String> {
        let count = self.u32(what, at).map_err(s)? as usize;
        if self.u32(what, at + 4).map_err(s)? as usize != count {
            return Err(format!(
                "the {what} at {at:#X} states a count other than its capacity"
            ));
        }
        if count == 0 {
            return Ok((0, 0));
        }
        let first = self.target(what, at + 8).map_err(s)?;
        let len = (count.checked_mul(stride))
            .ok_or_else(|| format!("the {what} at {at:#X} counts more than a section holds"))?;
        self.bytes(what, first, len).map_err(s)?;
        self.spend(len).map_err(s)?;
        Ok((first, count))
    }

    /// The u16 (`width` 2) or u32 values of the slot at `at`.
    fn matcher_values(
        &self,
        what: &'static str,
        at: usize,
        width: usize,
    ) -> Result<Vec<u32>, String> {
        let (first, count) = self.matcher_slot(what, at, width)?;
        (0..count)
            .map(|i| match width {
                2 => self.u16(what, first + 2 * i).map(u32::from),
                _ => self.u32(what, first + 4 * i),
            })
            .collect::<Result<Vec<u32>, AnimError>>()
            .map_err(s)
    }

    /// The full slots of the map `{u32 chunks, u32 size, pointer}` at `at`: a power of two of chunks
    /// of 16 control bytes and 16 slots of `slot_len` bytes, an empty slot zero; where `low_bits`,
    /// a full slot's control byte is its 32-bit key's low 7 bits.
    fn matcher_map(
        &self,
        what: &'static str,
        at: usize,
        slot_len: usize,
        low_bits: bool,
    ) -> Result<Vec<usize>, String> {
        let chunks = self.u32(what, at).map_err(s)? as usize;
        let size = self.u32(what, at + 4).map_err(s)? as usize;
        if chunks < 2 || !chunks.is_power_of_two() {
            return Err(format!(
                "the {what} at {at:#X} has {chunks} chunks, where a power of two of at least 2 is known"
            ));
        }
        let chunk = 16 + 16 * slot_len;
        let first = self.target(what, at + 8).map_err(s)?;
        let len = (chunks.checked_mul(chunk))
            .ok_or_else(|| format!("the {what} at {at:#X} has more chunks than a section holds"))?;
        let bytes = self.bytes(what, first, len).map_err(s)?;
        self.spend(len).map_err(s)?;
        let mut full = Vec::new();
        for c in 0..chunks {
            let base = c * chunk;
            for j in 0..16 {
                let control = bytes[base + j];
                let slot = base + 16 + j * slot_len;
                let body = &bytes[slot..slot + slot_len];
                if control < EMPTY {
                    if low_bits && u32::from(control) != le32(body, 0) & 0x7F {
                        return Err(format!(
                            "the {what} at {at:#X}: slot {c}.{j}'s control byte is not its key's low 7 bits"
                        ));
                    }
                    full.push(first + slot);
                } else if control != EMPTY || !zero(body) {
                    return Err(format!(
                        "the {what} at {at:#X}: slot {c}.{j} is neither full nor an empty slot of zeros"
                    ));
                }
            }
        }
        if full.len() != size {
            return Err(format!(
                "the {what} at {at:#X} states {size} entries and holds {}",
                full.len()
            ));
        }
        Ok(full)
    }

    /// The u32 keys of the 8-byte `{key, index}` slots, in index order, the indices a permutation.
    fn matcher_permuted(&self, what: &'static str, slots: &[usize]) -> Result<Vec<u32>, String> {
        let mut keys = vec![None; slots.len()];
        for &slot in slots {
            let index = self.u32(what, slot + 4).map_err(s)? as usize;
            let Some(key) = keys.get_mut(index).filter(|k| k.is_none()) else {
                return Err(format!(
                    "the {what}'s values are not each of its {} indices once",
                    slots.len()
                ));
            };
            *key = Some(self.u32(what, slot).map_err(s)?);
        }
        keys.into_iter()
            .collect::<Option<Vec<u32>>>()
            .ok_or_else(|| format!("the {what} misses an index"))
    }
}

#[path = "matcher_levels.rs"]
mod levels;

#[cfg(test)]
#[path = "matcher_tests.rs"]
mod tests;
