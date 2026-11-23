//! GTA VI's audio game data, GTA V's `.rel` counterpart: a schema word, a data block of items, then
//! a group table, an encrypted name table, an index keyed by each item's lower-cased name hash,
//! and the field tables that say which item fields hold a hash, a wave, a group slot or a
//! reference. Read only: a new item would need a name in the encrypted table.

/// The word every body's data block opens with, which no item holds.
pub const STAMP: u32 = 0x045A_88EA;
/// The stamp's bytes, before the first item.
const ITEM_HEAD: usize = 4;

/// What a body holds, which its schema word states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AudioDataKind {
    Speech,
    ModularSynth,
    Curves,
    AudioConfig,
    Categories,
    DynamicMixer,
    Music,
    Sounds,
    GameObjects,
}

impl AudioDataKind {
    pub fn of_schema(schema: u32) -> Option<Self> {
        Some(match schema {
            18 => Self::Speech,
            20 => Self::ModularSynth,
            25 => Self::Curves,
            26 => Self::AudioConfig,
            41 => Self::Categories,
            61 => Self::DynamicMixer,
            86 => Self::Music,
            166 => Self::Sounds,
            581 => Self::GameObjects,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Speech => "speech",
            Self::ModularSynth => "modular synths",
            Self::Curves => "curves",
            Self::AudioConfig => "audio configuration",
            Self::Categories => "categories",
            Self::DynamicMixer => "dynamic mixes",
            Self::Music => "music",
            Self::Sounds => "sounds",
            Self::GameObjects => "game objects",
        }
    }

    /// The kinds a reference row's mask names, by the kind of the bodies whose keys each bit's
    /// rows hold.
    pub fn of_mask(mask: u32) -> Vec<Self> {
        const BITS: [(u32, AudioDataKind); 7] = [
            (0x001, AudioDataKind::GameObjects),
            (0x002, AudioDataKind::Sounds),
            (0x004, AudioDataKind::Music),
            (0x010, AudioDataKind::ModularSynth),
            (0x020, AudioDataKind::AudioConfig),
            (0x040, AudioDataKind::DynamicMixer),
            (0x100, AudioDataKind::Curves),
        ];
        (BITS.iter())
            .filter(|(bit, _)| mask & bit != 0)
            .map(|(_, k)| *k)
            .collect()
    }

    /// Whether this kind's bodies have the second index of 12-byte rows.
    fn has_y(self) -> bool {
        matches!(self, Self::DynamicMixer | Self::GameObjects)
    }

    /// How many field tables follow the index: A and B, A to C, or A to E.
    fn tables(self) -> usize {
        match self {
            Self::Speech => 2,
            Self::Curves => 3,
            _ => 5,
        }
    }
}

/// Whether `head` opens a body of audio game data: a known schema word and the stamp at 8.
pub fn stamped(head: &[u8]) -> bool {
    let word = |at: usize| {
        head.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    word(0).and_then(AudioDataKind::of_schema).is_some() && word(8) == Some(STAMP)
}

#[derive(Debug, thiserror::Error)]
pub enum AudioDataError {
    #[error("not GTA VI audio game data: {0}")]
    NotAudioData(String),
    #[error("GTA VI audio game data: {0}")]
    Malformed(String),
}

type Result<T> = std::result::Result<T, AudioDataError>;

fn bad(why: String) -> AudioDataError {
    AudioDataError::Malformed(why)
}

/// A body of audio game data. A field is an offset from the body's start; an index offset counts
/// from the data block's.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    pub schema: u32,
    pub kind: AudioDataKind,
    /// The data block, its stamp first.
    pub data: Vec<u8>,
    /// Each group's first hash and hash count, and the hashes; their role is not known.
    pub groups: Vec<(u32, u32)>,
    pub group_hashes: Vec<u32>,
    /// The encrypted name table's name count and ciphertext length, which nothing here decrypts.
    pub names: u32,
    pub name_bytes: usize,
    /// Each item's key, `joaat` of its lower-cased name, and offset; sorted by low byte, then the rest.
    pub index: Vec<(u32, u32)>,
    /// The second index of two kinds: two words of unknown role and an item offset.
    pub index_y: Vec<[u32; 3]>,
    /// Fields that hold a 32-bit hash that is no index key.
    pub hash_fields: Vec<u32>,
    /// Fields that hold a wave container's hash, followed by the wave's hash.
    pub wave_fields: Vec<u32>,
    /// Fields that hold a group-table slot or `0xFFFFFFFF`.
    pub group_fields: Vec<u32>,
    /// Each reference row: the target's index key and the kinds its mask names.
    pub references: Vec<(u32, u32)>,
    /// Fields that hold a row of `references`.
    pub reference_fields: Vec<u32>,
}

/// One item: its key, where it lies, and what its fields are where they were laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioItem {
    pub key: u32,
    pub offset: u32,
    /// Bytes to the next item or the data block's end: an upper bound, since unindexed bytes exist.
    pub span: u32,
    /// The item's first byte; Speech items have none.
    pub class: Option<u8>,
    pub fields: AudioFields,
    pub references: Vec<AudioReference>,
    pub waves: Vec<WaveReference>,
}

/// A reference field inside an item: its offset in the item and the row it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioReference {
    pub at: u32,
    pub key: u32,
    pub mask: u32,
}

/// A wave field inside an item: its offset in the item, the container's hash and the wave's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveReference {
    pub at: u32,
    pub bank: u32,
    pub wave: u32,
}

impl WaveReference {
    /// The audio container stream the wave is: its hash's low 29 bits.
    pub fn stream_id(self) -> u32 {
        self.wave & 0x1FFF_FFFF
    }
}

/// An item's fields, where its class's layout was laid out from the bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioFields {
    /// A class whose layout is not known, listed by its class and size.
    Opaque,
    /// An audio configuration item, laid out as GTA V's.
    #[cfg(feature = "rel")]
    Config(crate::rel::config::ConfigItem),
    Curve(Curve),
    /// A sound of class 13: a wave container's hash and the wave's.
    Wave {
        bank: u32,
        wave: u32,
    },
}

/// A curve: the base every class shares, then its shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub flags: u32,
    pub min_input: f32,
    pub max_input: f32,
    pub shape: CurveShape,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CurveShape {
    Constant(f32),
    /// The left and right hand pairs, x and y each.
    Linear([f32; 4]),
    PiecewiseLinear(Vec<[f32; 2]>),
    ValueTable(Vec<f32>),
}

struct Bytes<'a> {
    v: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn u32(&mut self, what: &str) -> Result<u32> {
        let b = (self.v.get(self.at..self.at + 4))
            .ok_or_else(|| bad(format!("{what} runs past the body's end")))?;
        self.at += 4;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A count of `unit`-byte records, refused where they cannot fit in what is left.
    fn count(&mut self, unit: usize, what: &str) -> Result<usize> {
        let n = self.u32(what)? as usize;
        if n.checked_mul(unit)
            .is_none_or(|len| len > self.v.len() - self.at)
        {
            return Err(bad(format!(
                "{what} counts {n} records, past the body's end"
            )));
        }
        Ok(n)
    }

    fn words(&mut self, n: usize, what: &str) -> Result<Vec<u32>> {
        (0..n).map(|_| self.u32(what)).collect()
    }
}

/// The index's order: a key's low byte, then its upper 24 bits.
fn index_order(key: u32) -> u32 {
    (key & 0xFF) << 24 | key >> 8
}

impl AudioData {
    /// The body `v` holds, inflated.
    pub fn read(v: &[u8]) -> Result<Self> {
        let mut r = Bytes { v, at: 0 };
        let schema = r.u32("the schema word")?;
        let kind = AudioDataKind::of_schema(schema)
            .ok_or_else(|| AudioDataError::NotAudioData(format!("schema word {schema}")))?;
        let n = r.u32("the data length")? as usize;
        let data = v
            .get(8..8usize.saturating_add(n))
            .ok_or_else(|| bad(format!("the {n}-byte data block runs past the body's end")))?;
        if !stamped(v) {
            return Err(AudioDataError::NotAudioData(
                "the data block does not open with the stamp".into(),
            ));
        }
        r.at = 8 + n;

        let s_len = r.u32("the group table")? as usize;
        let s_end = r.at.saturating_add(s_len);
        let g = r.u32("the group table")? as usize;
        let room = s_len
            .checked_sub(4)
            .filter(|room| s_end <= v.len() && g <= room / 8);
        let Some(m) = room
            .map(|room| room - 8 * g)
            .filter(|m| m % 4 == 0 && (s_len > 4 || g == 0))
        else {
            return Err(bad(format!(
                "a {s_len}-byte group table does not hold {g} groups and whole hashes"
            )));
        };
        let mut groups = Vec::with_capacity(g);
        let mut first = 0u64;
        for _ in 0..g {
            let (at, count) = (r.u32("a group")?, r.u32("a group")?);
            if u64::from(at) != first {
                return Err(bad("the groups do not tile their hashes from 0".into()));
            }
            first += u64::from(count);
            groups.push((at, count));
        }
        if first != (m / 4) as u64 {
            return Err(bad("the groups do not tile their hashes from 0".into()));
        }
        let group_hashes = r.words(m / 4, "the group hashes")?;

        let x_len = r.u32("the name table")? as usize;
        let names = r.u32("the name table")?;
        if x_len < 4 || (x_len == 4) != (names == 0) || x_len - 4 > v.len() - r.at {
            return Err(bad(format!("a {x_len}-byte name table of {names} names")));
        }
        r.at += x_len - 4;

        let count = r.count(8, "the index")?;
        let mut index = Vec::with_capacity(count);
        for _ in 0..count {
            index.push((r.u32("the index")?, r.u32("the index")?));
        }
        if index
            .windows(2)
            .any(|w| index_order(w[0].0) > index_order(w[1].0))
        {
            return Err(bad(
                "the index is not sorted by its keys' low byte, then the rest".into(),
            ));
        }
        let mut offsets: Vec<u32> = index.iter().map(|e| e.1).collect();
        offsets.sort_unstable();
        let repeated = offsets.windows(2).any(|w| w[0] == w[1]);
        if repeated
            || offsets.first().is_some_and(|o| *o != ITEM_HEAD as u32)
            || offsets.last().is_some_and(|o| *o as usize >= n)
        {
            return Err(bad(
                "the index's offsets repeat, do not start at 4 or pass the data block".into(),
            ));
        }

        let mut index_y = Vec::new();
        if kind.has_y() {
            if r.u32("the second index")? as usize != count {
                return Err(bad(
                    "the second index does not list the index's items".into()
                ));
            }
            for _ in 0..count {
                index_y.push([
                    r.u32("the second index")?,
                    r.u32("the second index")?,
                    r.u32("the second index")?,
                ]);
            }
            let mut at: Vec<u32> = index_y.iter().map(|y| y[2]).collect();
            at.sort_unstable();
            let mut pairs: Vec<(u32, u32)> = index_y.iter().map(|y| (y[0], y[1])).collect();
            pairs.sort_unstable();
            if at != offsets || pairs.windows(2).any(|w| w[0] == w[1]) {
                return Err(bad(
                    "the second index does not list the index's items once each".into(),
                ));
            }
        }

        let field_table = |r: &mut Bytes, what: &str| -> Result<Vec<u32>> {
            let c = r.count(4, what)?;
            let fields = r.words(c, what)?;
            let inside = |f: &u32| (8..=8 + n.saturating_sub(4)).contains(&(*f as usize));
            if fields.windows(2).any(|w| w[0] >= w[1]) || !fields.iter().all(inside) {
                return Err(bad(format!(
                    "{what} is not ascending inside the data block"
                )));
            }
            Ok(fields)
        };
        let hash_fields = field_table(&mut r, "the hash fields")?;
        let wave_fields = field_table(&mut r, "the wave fields")?;
        let group_fields = if kind.tables() >= 3 {
            field_table(&mut r, "the group fields")?
        } else {
            Vec::new()
        };
        let (mut references, mut reference_fields) = (Vec::new(), Vec::new());
        if kind.tables() == 5 {
            let c = r.count(8, "the reference rows")?;
            for _ in 0..c {
                references.push((r.u32("the reference rows")?, r.u32("the reference rows")?));
            }
            reference_fields = field_table(&mut r, "the reference fields")?;
        }
        if r.at != v.len() {
            return Err(bad(format!(
                "{} bytes follow the last table",
                v.len() - r.at
            )));
        }

        let word = |f: u32| {
            let at = f as usize;
            u32::from_le_bytes([v[at], v[at + 1], v[at + 2], v[at + 3]])
        };
        let mut banks: Vec<u32> = wave_fields.iter().map(|f| word(*f)).collect();
        banks.sort_unstable();
        banks.dedup();
        if banks.len() != names as usize {
            return Err(bad(format!(
                "{} distinct wave containers, where the name table names {names}",
                banks.len()
            )));
        }
        if let Some(f) = group_fields
            .iter()
            .find(|f| word(**f) != u32::MAX && word(**f) as usize >= g)
        {
            return Err(bad(format!("the group field at {f:#X} names no group")));
        }
        let mut rows = references.clone();
        rows.sort_unstable();
        if rows.windows(2).any(|w| w[0] == w[1]) || rows.iter().any(|r| r.1 == 0) {
            return Err(bad("a reference row repeats or names no kind".into()));
        }
        let mut used = vec![false; references.len()];
        for f in &reference_fields {
            match used.get_mut(word(*f) as usize) {
                Some(u) => *u = true,
                None => return Err(bad(format!("the reference field at {f:#X} names no row"))),
            }
        }
        if used.contains(&false) {
            return Err(bad("a reference row no field names".into()));
        }

        Ok(Self {
            schema,
            kind,
            data: data.to_vec(),
            groups,
            group_hashes,
            names,
            name_bytes: x_len - 4,
            index,
            index_y,
            hash_fields,
            wave_fields,
            group_fields,
            references,
            reference_fields,
        })
    }

    /// The items in index order, each with the fields its class's layout and the field tables give.
    pub fn items(&self) -> Vec<AudioItem> {
        let mut starts: Vec<u32> = self.index.iter().map(|e| e.1).collect();
        starts.sort_unstable();
        let n = self.data.len() as u32;
        let word = |at: usize| {
            (self.data.get(at..at + 4)).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        // A field's body offset is 8 past its data offset.
        let within = |fields: &[u32], from: u32, to: u32| -> Vec<u32> {
            let (a, b) = (
                fields.partition_point(|f| *f < from + 8),
                fields.partition_point(|f| *f < to + 8),
            );
            fields[a..b].iter().map(|f| f - 8).collect()
        };
        (self.index.iter())
            .map(|&(key, offset)| {
                let next = starts.partition_point(|s| *s <= offset);
                let end = starts.get(next).copied().unwrap_or(n);
                let bytes = self
                    .data
                    .get(offset as usize..end as usize)
                    .unwrap_or_default();
                let class = bytes
                    .first()
                    .filter(|_| self.kind != AudioDataKind::Speech)
                    .copied();
                let references = (within(&self.reference_fields, offset, end).into_iter())
                    .filter_map(|at| {
                        let (key, mask) = *self.references.get(word(at as usize) as usize)?;
                        Some(AudioReference {
                            at: at - offset,
                            key,
                            mask,
                        })
                    })
                    .collect();
                let waves = (within(&self.wave_fields, offset, end).into_iter())
                    .map(|at| WaveReference {
                        at: at - offset,
                        bank: word(at as usize),
                        wave: word(at as usize + 4),
                    })
                    .collect();
                AudioItem {
                    key,
                    offset,
                    span: end - offset,
                    class,
                    fields: fields(self.kind, bytes),
                    references,
                    waves,
                }
            })
            .collect()
    }
}

/// The fields of an item of `kind` whose bytes, to the next item, are `bytes`, where its class's
/// layout is known and the bytes fill it exactly.
fn fields(kind: AudioDataKind, bytes: &[u8]) -> AudioFields {
    let u32_at = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let f32_at = |at: usize| u32_at(at).map(f32::from_bits);
    match kind {
        #[cfg(feature = "rel")]
        AudioDataKind::AudioConfig => crate::rel::config::ConfigItem::decode(bytes)
            .map_or(AudioFields::Opaque, AudioFields::Config),
        AudioDataKind::Curves => {
            curve(bytes, u32_at, f32_at).map_or(AudioFields::Opaque, AudioFields::Curve)
        }
        AudioDataKind::Sounds if bytes.len() == 11 && bytes[0] == 13 => AudioFields::Wave {
            bank: u32_at(3).unwrap_or(0),
            wave: u32_at(7).unwrap_or(0),
        },
        _ => AudioFields::Opaque,
    }
}

fn curve(
    bytes: &[u8],
    u32_at: impl Fn(usize) -> Option<u32>,
    f32_at: impl Fn(usize) -> Option<f32>,
) -> Option<Curve> {
    if bytes.get(5..8)? != [0, 0, 0] {
        return None;
    }
    let floats = |from: usize, n: usize| {
        (0..n)
            .map(|i| f32_at(from + 4 * i))
            .collect::<Option<Vec<f32>>>()
    };
    let (shape, len) = match bytes[0] {
        1 => (CurveShape::Constant(f32_at(16)?), 20),
        2 => {
            let f = floats(16, 4)?;
            (CurveShape::Linear([f[0], f[1], f[2], f[3]]), 32)
        }
        4 => {
            let n = u32_at(16)? as usize;
            let f = floats(20, 2 * n.min(bytes.len() / 8))?;
            (
                CurveShape::PiecewiseLinear(f.chunks(2).map(|p| [p[0], p[1]]).collect()),
                20 + 8 * n,
            )
        }
        6 => {
            let n = usize::from(u16::from_le_bytes([*bytes.get(16)?, *bytes.get(17)?]));
            (CurveShape::ValueTable(floats(20, n)?), 20 + 4 * n)
        }
        _ => return None,
    };
    (bytes.len() == len).then(|| Curve {
        flags: u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]),
        min_input: f32_at(8).unwrap_or(0.0),
        max_input: f32_at(12).unwrap_or(0.0),
        shape,
    })
}

#[cfg(test)]
#[path = "audio_data_tests.rs"]
mod tests;
