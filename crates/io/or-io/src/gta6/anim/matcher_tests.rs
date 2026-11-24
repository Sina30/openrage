use super::*;
use crate::gta6::anim::{read, RootKind};

const VIRTUAL: u64 = 0x5000_0000;
const ROOT: &str = "matchers_a";
const DICTIONARY: &str = "dict_b";
const MATCHER: &str = "matcher_t";
const NAME: &str = "pack:/matcher_t.motionmatcher";
const LEAVES: [&str; 2] = ["clip_one", "clip_two"];
const ALIAS: u32 = 0x1F2E_3D4C;
/// Version 1 in byte 7's high nibble, variant 10 in byte 8's top bits.
const GUID: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 0x18, 0x89, 10, 11, 12, 13, 14, 15, 16];
/// Entry i owns frames `FRAME_OFFSETS[i]..FRAME_OFFSETS[i + 1]`.
const FRAME_OFFSETS: [u32; 4] = [0, 2, 3, 5];
const FRAMES: u32 = 5;
const BONES: [&str; 2] = ["SKEL_ROOT", "SKEL_Pelvis"];
/// Each feature's name, dimensions, bone and flags.
const FEATURES: [(&str, u16, &str, u32); 2] = [
    ("feature_a", 6, "SKEL_Pelvis", 3),
    ("feature_b", 12, "SKEL_ROOT", 11),
];
const DIMS: usize = 18;
/// Each dimension's weight at 0xB0.
const WEIGHT: f32 = 1.0 / DIMS as f32;
/// A class-3 database's pairs of stored dimensions.
const PAIRS: usize = DIMS / 2;
const BLOCKS: u32 = 2;
const DATABASE_HASH: u32 = 0x5A5A_1234;
const FRAME_WORDS: [u32; 5] = [1, 0x8000, 0x30, 0xF0, 3];
const KEYS_170: [u32; 2] = [0x0BAD_F00D, 0x7E57_ABCD];
const WORDS_190: [u32; 2] = [0x1357_9BDF, 0x2468_ACE0];
const ROTATIONS: [[f32; 4]; 5] = [
    [0.0, 0.0, 0.0, 1.0],
    [0.6, 0.0, 0.0, 0.8],
    [0.0, 0.6, 0.0, 0.8],
    [0.0, 0.0, 0.6, 0.8],
    [0.8, 0.0, 0.0, 0.6],
];

fn hash(name: &str) -> u32 {
    name_hash32(name.as_bytes())
}

/// Frame `f`'s half `i` of the feature matrix.
fn half(f: usize, i: usize) -> u16 {
    0x3C00 + (32 * f + i) as u16
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn halves(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A virtual section laid out object by object, each on 16 bytes.
#[derive(Default)]
struct B(Vec<u8>);

impl B {
    fn alloc(&mut self, len: usize) -> usize {
        let at = self.0.len().next_multiple_of(16);
        self.0.resize(at + len, 0);
        at
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn u16(&mut self, at: usize, v: u16) {
        self.put(at, &v.to_le_bytes());
    }
    fn u32(&mut self, at: usize, v: u32) {
        self.put(at, &v.to_le_bytes());
    }
    fn u64(&mut self, at: usize, v: u64) {
        self.put(at, &v.to_le_bytes());
    }
    fn ptr(&mut self, at: usize, to: usize) {
        self.u64(at, VIRTUAL + to as u64);
    }
    fn text(&mut self, s: &str) -> usize {
        let at = self.alloc(s.len() + 1);
        self.put(at, s.as_bytes());
        at
    }
    fn name(&mut self, at: usize, s: &str) {
        let text = self.text(s);
        self.ptr(at, text);
        self.u16(at + 8, s.len() as u16);
        self.u16(at + 10, s.len() as u16 + 1);
        self.u32(at + 12, 1);
    }
    /// A `{u32 count, u32 count, pointer}` head at `at` of `bytes`.
    fn slot(&mut self, at: usize, bytes: &[u8], count: u32) {
        let first = self.alloc(bytes.len().max(1));
        self.put(first, bytes);
        self.u32(at, count);
        self.u32(at + 4, count);
        self.ptr(at + 8, first);
    }
    /// A map of two chunks at `at`, slot `i` of `slots` in chunk `i % 2` at place `i / 2 + 3`.
    fn chunked(&mut self, at: usize, slot_len: usize, slots: &[(u8, Vec<u8>)]) {
        let chunk = 16 + 16 * slot_len;
        let first = self.alloc(2 * chunk);
        for c in 0..2 {
            self.0[first + c * chunk..first + c * chunk + 16].fill(0x80);
        }
        for (i, (control, body)) in slots.iter().enumerate() {
            let base = first + (i % 2) * chunk;
            let place = i / 2 + 3;
            self.0[base + place] = *control;
            self.put(base + 16 + place * slot_len, body);
        }
        self.u32(at, 2);
        self.u32(at + 4, slots.len() as u32);
        self.ptr(at + 8, first);
    }
    /// The root's bucket map at `at` of `entries`, each `{key, ..., next at next_at}`.
    fn map(&mut self, at: usize, slots: u32, entries: &[(u64, usize)], next_at: usize) {
        let buckets = self.alloc(slots as usize * 8);
        for &(key, entry) in entries {
            let head = buckets + (key % u64::from(slots)) as usize * 8;
            let next: [u8; 8] = self.0[head..head + 8].try_into().expect("eight bytes");
            self.put(entry + next_at, &next);
            self.ptr(head, entry);
        }
        self.ptr(at, buckets);
        self.u32(at + 8, slots);
        self.u32(at + 0x10, entries.len() as u32);
        self.0[at + 0x17] = 1;
    }
}

/// An 8-byte `{key, index}` slot with its control byte, the key's low 7 bits.
fn keyed(key: u32, index: u32) -> (u8, Vec<u8>) {
    ((key & 0x7F) as u8, words(&[key, index]))
}

/// Where a built matcher dictionary's objects are.
struct Built {
    bytes: Vec<u8>,
    matcher: usize,
    database: usize,
    features: [usize; 2],
}

/// A motion dictionary with no metadata region holding one class-1 matcher of two clips of
/// [`DICTIONARY`], three entries and five frames, over a skeleton of two bones.
fn build() -> Built {
    let mut b = B::default();
    let root = b.alloc(0xB0);
    b.name(root + 0x38, ROOT);
    b.u32(root + 0x68, hash(ROOT));
    let list = b.alloc(16);
    b.u32(list + 8, hash(DICTIONARY));
    b.ptr(root + 0x70, list);
    b.u16(root + 0x78, 1);
    b.u16(root + 0x7A, 1);
    let index = b.alloc(0x10);
    b.u32(index, hash(DICTIONARY));
    b.map(root + 0x80, 11, &[(u64::from(hash(DICTIONARY)), index)], 8);

    let skeleton = b.alloc(0x70);
    let records = b.alloc(0x50 * BONES.len());
    let inverses = b.alloc(0x40 * BONES.len());
    b.ptr(skeleton + 0x28, records);
    b.ptr(skeleton + 0x30, inverses);
    b.u16(skeleton + 0x6C, BONES.len() as u16);
    b.u16(skeleton + 0x6E, BONES.len() as u16);
    for (i, bone) in BONES.iter().enumerate() {
        let r = records + 0x50 * i;
        let name = b.text(bone);
        b.ptr(r + 0x30, name);
        b.u32(r + 0x1C, hash(bone));
        b.u16(r + 0x46, if i == 0 { 0xFFFF } else { 0 });
        b.u16(r + 0x48, i as u16);
    }

    let mut pointers = Vec::new();
    let mut features = [0; 2];
    let mut offset = 0;
    for (i, &(_, dims, bone, flags)) in FEATURES.iter().enumerate() {
        let f = b.alloc(0x30);
        b.u32(f + 8, offset);
        b.u16(f + 12, dims);
        b.u16(f + 14, 1);
        b.u32(f + 0x18, hash(bone));
        b.u32(f + 0x1C, flags);
        offset += u32::from(dims);
        pointers.extend((VIRTUAL + f as u64).to_le_bytes());
        features[i] = f;
    }

    let d = b.alloc(0x160);
    b.u64(d + 0x18, 1);
    let weights: Vec<f32> = (0..24)
        .map(|k| if k < DIMS { WEIGHT } else { 0.0 })
        .collect();
    b.slot(d + 0xB0, &floats(&weights), 3);
    b.ptr(d + 0x28, skeleton);
    b.slot(d + 0x38, &pointers, 2);
    let named = [keyed(hash(FEATURES[1].0), 1), keyed(hash(FEATURES[0].0), 0)];
    b.chunked(d + 0x48, 8, &named);
    b.slot(d + 0x68, &words(&[DATABASE_HASH]), 1);
    b.slot(d + 0x90, &words(&FRAME_WORDS), FRAMES);
    b.put(d + 0xC0, &floats(&[12.0, 1.0]));
    b.u32(d + 0xC8, 1);
    let mut frame_records = Vec::new();
    for rotation in &ROTATIONS {
        frame_records.extend(floats(rotation));
        frame_records.extend([0u8; 32]);
    }
    b.slot(d + 0xA0, &frame_records, FRAMES);
    let mut matrix = Vec::new();
    for f in 0..FRAMES as usize {
        let row: Vec<u16> = (0..32)
            .map(|i| if i < DIMS { half(f, i) } else { 0 })
            .collect();
        matrix.extend(halves(&row));
    }
    b.slot(d + 0xE8, &matrix, FRAMES * BLOCKS);
    b.put(d + 0xF8, &words(&[FRAMES, DIMS as u32, BLOCKS, 1]));

    let m = b.alloc(0x1C0);
    b.u64(m + 0x10, 1);
    b.ptr(m + 0x18, root);
    b.ptr(m + 0x20, d);
    b.put(m + 0x28, &GUID);
    b.name(m + 0x38, NAME);
    let mut entries = Vec::new();
    for (start, rate, clip) in [(0.0f32, 30.0f32, 0u32), (0.1, 30.0, 0), (0.0, 15.0, 1)] {
        entries.extend(floats(&[start, rate, -1.0]));
        entries.extend(clip.to_le_bytes());
        entries.extend([0u8; 8]);
    }
    b.slot(m + 0x50, &entries, 3);
    b.slot(m + 0x60, &words(&FRAME_OFFSETS), 4);
    b.slot(m + 0x70, &halves(&[0, 2, 3]), 3);
    let mut clips = Vec::new();
    for leaf in LEAVES {
        clips.extend([0u8; 8]);
        clips.extend(words(&[hash(leaf), hash(DICTIONARY)]));
    }
    b.slot(m + 0x90, &clips, 2);
    let clip_slot = |i: usize| words(&[hash(LEAVES[i]), ALIAS, i as u32, 0]);
    b.chunked(m + 0xA0, 16, &[(0x11, clip_slot(1)), (0x22, clip_slot(0))]);
    b.slot(m + 0xB0, &[0, 1], 2);
    b.slot(m + 0x110, &[7], 1);
    b.slot(m + 0x120, &[0], 1);
    let keys = [keyed(KEYS_170[0], 1), keyed(KEYS_170[1], 0)];
    b.chunked(m + 0x170, 8, &keys);
    b.put(m + 0x188, &words(&[hash(MATCHER), hash(ROOT)]));
    b.put(m + 0x190, &words(&WORDS_190));

    let entry = b.alloc(0x20);
    b.u64(entry, u64::from(hash(MATCHER)));
    b.ptr(entry + 8, m);
    b.map(root + 0x48, 11, &[(u64::from(hash(MATCHER)), entry)], 0x10);
    Built {
        bytes: b.0,
        matcher: m,
        database: d,
        features,
    }
}

fn refused(bytes: &[u8], why: &str) {
    match read(bytes) {
        Err(AnimError::Motion { motion, why: said }) if motion == NAME && said.contains(why) => {}
        other => panic!("{other:?} is not a refusal of {NAME} saying {why:?}"),
    }
}

#[test]
fn a_matcher_dictionary_with_no_metadata_region_reads_its_matcher() {
    let read = read(&build().bytes).expect("reads");
    assert_eq!(read.root, RootKind::MotionDictionary);
    assert!(read.motions.is_empty());
    assert_eq!(read.dictionaries, [hash(DICTIONARY)]);
    let [matcher] = &read.matchers[..] else {
        panic!("{:?}", read.matchers);
    };
    assert_eq!((matcher.name.as_str(), matcher.key), (NAME, hash(MATCHER)));
    assert_eq!(matcher.guid, GUID);
    let clip = |i: usize| MatcherClip {
        leaf: hash(LEAVES[i]),
        alias: ALIAS,
        dictionary: Some(hash(DICTIONARY)),
    };
    assert_eq!(matcher.clips, [clip(0), clip(1)]);
    let entry = |clip, start, rate, i: usize| MatcherEntry {
        clip,
        start,
        rate,
        frames: FRAME_OFFSETS[i]..FRAME_OFFSETS[i + 1],
    };
    assert_eq!(
        matcher.entries,
        [
            entry(0, 0.0, 30.0, 0),
            entry(0, 0.1, 30.0, 1),
            entry(1, 0.0, 15.0, 2)
        ]
    );
    assert_eq!(matcher.keys_at_170, [KEYS_170[1], KEYS_170[0]]);
    assert_eq!(matcher.words_at_190, WORDS_190);

    let database = &matcher.database;
    let bones: Vec<&str> = database
        .skeleton
        .bones
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(bones, BONES);
    let mut offset = 0;
    for (feature, &(name, dims, bone, flags)) in database.features.iter().zip(&FEATURES) {
        assert_eq!(
            *feature,
            MatcherFeature {
                name: hash(name),
                offset,
                dims,
                kind: 1,
                words: [hash(bone), flags, 0],
            }
        );
        offset += u32::from(dims);
    }
    assert_eq!(database.features.len(), FEATURES.len());
    assert_eq!(database.frames, FRAMES);
    assert_eq!(database.hashes_at_68, [DATABASE_HASH]);
    assert_eq!(database.frame_words, FRAME_WORDS);
    assert_eq!(database.word_at_c0, 12.0);
    assert_eq!(database.weights, [WEIGHT; DIMS]);
    let MatcherClass::Vectors { rotations, vectors } = &database.class else {
        panic!("{:?}", database.class);
    };
    assert_eq!(rotations[..], ROTATIONS[..]);
    let expected: Vec<Vec<u16>> = (0..FRAMES as usize)
        .map(|f| (0..DIMS).map(|i| half(f, i)).collect())
        .collect();
    assert_eq!(*vectors, expected);
}

#[test]
fn a_kind_5_feature_carries_its_three_words_with_none_held_to_zero() {
    let built = build();
    let mut b = B(built.bytes);
    let f = built.features[1];
    b.u16(f + 14, 5);
    b.u32(f + 0x18, 0);
    b.u32(f + 0x1C, 0x0C0F_FEE1);
    b.u32(f + 0x24, 0x7AB1_E5E7);
    let read = read(&b.0).expect("reads");
    let feature = read.matchers[0].database.features[1];
    assert_eq!(
        (feature.kind, feature.words),
        (5, [0, 0x0C0F_FEE1, 0x7AB1_E5E7])
    );
}

/// Stored dimension `k` of frame `f` of a class-3 database: rising with the frame in an even
/// dimension, falling below zero in an odd one.
fn stored(k: usize, f: usize) -> u16 {
    let base = if k.is_multiple_of(2) { 0x3C00 } else { 0xBC00 };
    base + 16 * k as u16 + f as u16
}

/// Element q of a level whose every lane covers all five frames: the least of each dimension, or
/// with `greatest` the greatest.
fn bounds(q: usize, greatest: bool) -> [u16; 16] {
    let (first, last) = (0, FRAMES as usize - 1);
    let (even, odd) = if greatest {
        (last, first)
    } else {
        (first, last)
    };
    std::array::from_fn(|h| {
        let (k, f) = [(2 * q, even), (2 * q + 1, odd)][h / 8];
        stored(k, f)
    })
}

/// The built matcher with a class-3 database: the order reversed, one block of [`stored`] for
/// each pair, its padding frames repeating frame 4, and two levels of lanes of 256 and 16 frames.
fn class_3() -> (B, usize) {
    let built = build();
    let mut b = B(built.bytes);
    let d = built.database;
    b.put(d + 0xA0, &[0; 16]);
    b.u32(d + 0xC8, 3);
    let order: Vec<u32> = (0..DIMS as u32).rev().collect();
    b.slot(d + 0xF8, &words(&order), DIMS as u32);
    let mut blocks = Vec::new();
    for q in 0..PAIRS {
        let frame = |h: usize| (h % 8).min(FRAMES as usize - 1);
        let block: Vec<u16> = (0..16).map(|h| stored(2 * q + h / 8, frame(h))).collect();
        blocks.extend(halves(&block));
    }
    b.slot(d + 0xE8, &blocks, PAIRS as u32);
    let heads = b.alloc(64);
    b.u32(d + 0x108, 2);
    b.u32(d + 0x10C, 2);
    b.ptr(d + 0x110, heads);
    for level in 0..2 {
        for (k, greatest) in [false, true].into_iter().enumerate() {
            let bytes: Vec<u8> = (0..PAIRS)
                .flat_map(|q| halves(&bounds(q, greatest)))
                .collect();
            b.slot(heads + 32 * level + 16 * k, &bytes, PAIRS as u32);
        }
    }
    b.slot(d + 0x118, &words(&[16, 16]), 2);
    b.u32(d + 0x128, FRAMES);
    b.u16(d + 0x12C, PAIRS as u16);
    b.u16(d + 0x12E, DIMS as u16);
    b.u32(d + 0x130, 1);
    (b, d)
}

#[test]
fn a_class_3_database_reads_its_frames_from_its_blocks_and_its_levels_of_bounds() {
    let (b, _) = class_3();
    let read = read(&b.0).expect("reads");
    let level = |lane_frames| MatcherLevel {
        lane_frames,
        minima: (0..PAIRS).map(|q| bounds(q, false)).collect(),
        maxima: (0..PAIRS).map(|q| bounds(q, true)).collect(),
    };
    assert_eq!(
        read.matchers[0].database.class,
        MatcherClass::Levels {
            dimension_order: (0..DIMS as u32).rev().collect(),
            pairs: PAIRS as u16,
            stored: (0..FRAMES as usize)
                .map(|f| (0..DIMS).map(|k| stored(k, f)).collect())
                .collect(),
            levels: vec![level(256), level(16)],
        }
    );
}

#[test]
fn a_class_3_database_off_its_blocks_or_levels_is_refused_by_name() {
    let (mut b, d) = class_3();
    let heads = pointer(&b.0, d + 0x110);
    let minima = pointer(&b.0, heads + 8);
    b.u16(minima, stored(0, 1));
    refused(&b.0, "does not bound stored dimension 0");

    let (mut b, d) = class_3();
    let blocks = pointer(&b.0, d + 0xF0);
    b.u16(blocks + 2 * 7, stored(0, 3));
    refused(&b.0, "does not repeat frame 4");

    let (mut b, d) = class_3();
    b.u16(d + 0x12C, PAIRS as u16 + 1);
    refused(&b.0, "not ceil(18 / 2)");

    let (mut b, d) = class_3();
    let words_at = pointer(&b.0, d + 0x120);
    b.u32(words_at + 4, 8);
    refused(&b.0, "where only 16 is known");

    let (mut b, d) = class_3();
    b.u32(d + 0x13C, 1);
    refused(&b.0, "from 0x134 to 0x160");
}

#[test]
fn weights_that_do_not_sum_to_1_are_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    let weights = pointer(&b.0, built.database + 0xB8);
    b.put(weights, &0.5f32.to_le_bytes());
    refused(&b.0, "weights at 0xB0");
}

#[test]
fn an_entry_off_its_clip_is_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    let entries = pointer(&b.0, built.matcher + 0x58);
    b.u32(entries + 24 + 12, 1);
    refused(&b.0, "entry 1");
}

#[test]
fn a_feature_offset_off_the_running_sum_is_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    b.u32(built.features[1] + 8, 7);
    refused(&b.0, "the sum of the earlier features' dimensions");
}

#[test]
fn a_bone_feature_naming_no_bone_is_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    b.u32(built.features[0] + 0x18, hash("SKEL_Head"));
    refused(&b.0, "names no bone");
}

#[test]
fn a_control_byte_off_its_keys_low_bits_is_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    let map = pointer(&b.0, built.database + 0x50);
    b.0[map + 3] ^= 0x01;
    refused(&b.0, "low 7 bits");
}

#[test]
fn a_class_word_other_than_1_or_3_is_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    b.u32(built.database + 0xC8, 2);
    refused(&b.0, "class word at 0xC8 is 2");
}

#[test]
fn a_non_zero_half_past_the_dimensions_is_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    let matrix = pointer(&b.0, built.database + 0xF0);
    b.u16(matrix + 2 * 32 + 2 * 31, 0x3C00);
    refused(&b.0, "frame 1's halves past its 18 dimensions");
}

#[test]
fn a_slot_whose_count_is_not_its_capacity_is_refused_by_name() {
    let built = build();
    let mut b = B(built.bytes);
    b.u32(built.matcher + 0x54, 4);
    refused(&b.0, "count other than its capacity");
}

#[test]
fn the_words_no_role_is_known_for_are_read_as_stored() {
    let built = build();
    let read_back = read(&built.bytes).expect("reads");
    let matcher = &read_back.matchers[0];
    assert_eq!(matcher.clip_bytes, [0, 1]);
    assert_eq!(matcher.byte_at_110, 7);
    assert!(
        matcher.object_at_48.is_empty(),
        "a null pointer holds nothing"
    );
    assert!(matcher.database.object_at_30.is_empty());
    assert_eq!(matcher.database.words_at_78, [0; 4]);
    assert_eq!(
        matcher.database.record_floats,
        vec![[0.0; 8]; FRAMES as usize]
    );

    let mut b = B(built.bytes);
    let object: Vec<u8> = (1..=64).collect();
    let at = b.alloc(object.len());
    b.put(at, &object);
    b.ptr(built.matcher + 0x48, at);
    b.ptr(built.database + 0x30, at);
    let stated = [0xFFFF_FFFF, 0x1234_5678, 0x0030_0040, 0xF6];
    b.put(built.database + 0x78, &words(&stated));
    let records = pointer(&b.0, built.database + 0xA8);
    let tail: [f32; 8] = std::array::from_fn(|k| k as f32 + 0.5);
    b.put(records + 48 + 16, &floats(&tail));
    let read_back = read(&b.0).expect("reads");
    let matcher = &read_back.matchers[0];
    assert_eq!(matcher.object_at_48, object[..40]);
    assert_eq!(matcher.database.object_at_30, object[..0x30]);
    assert_eq!(matcher.database.words_at_78, stated);
    assert_eq!(matcher.database.record_floats[0], [0.0; 8]);
    assert_eq!(matcher.database.record_floats[1], tail);
}

/// The offset the virtual pointer at `at` holds.
fn pointer(bytes: &[u8], at: usize) -> usize {
    let word = u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"));
    (word - VIRTUAL) as usize
}
