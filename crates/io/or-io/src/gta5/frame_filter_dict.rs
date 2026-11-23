//! `.yfd`: the per-(bone, track) weight masks an animation blend applies.

use super::shared::PgDictionary;
use crate::decl::*;
use crate::desc::Value;
use crate::Generation;

/// A `.yfd`: filters keyed by the hash of their name.
pub type CrFrameFilterDictionary = PgDictionary<CrFrameFilterAny>;

impl Resource for CrFrameFilterDictionary {
    const EXT: &'static str = "yfd";
    const TARGET: Generation = Generation::Gen8;
}

/// Which filter layout follows the header.
#[record(enum, width = i32)]
pub enum EFrameFilterType {
    /// None
    None = -1,
    /// FrameFilter
    FrameFilter = 0,
    /// Bone
    Bone = 1,
    /// BoneBasic
    BoneBasic = 2,
    /// BoneMultiWeight
    BoneMultiWeight = 3,
    /// MultiWeight
    MultiWeight = 4,
    /// TrackMultiWeight
    TrackMultiWeight = 5,
    /// Mover
    Mover = 6,
    /// BlendShape
    BlendShape = 7,
    /// Expression
    Expression = 8,
    /// MoveTransition
    MoveTransition = 9,
    /// ShaderVar
    ShaderVar = 10,
}

/// What every filter starts with; `type` says which layout follows.
///
/// Every filter the corpus and both installs ship is a
/// [`CrFrameFilterMultiWeight`], so it is the only variant declared: a layout
/// no file can check is a guess.
#[record(size = 0x18, tag = "type", variant(4 => CrFrameFilterMultiWeight))]
pub struct CrFrameFilter {
    vft: u64,
    #[note("1 in every shipped filter")]
    #[init(1)]
    ref_count: u32,
    #[note("CRC-32 over the track table's bytes, then the weight table's; written at save")]
    signature: u32,
    #[pad_after(4)]
    pub r#type: Open<EFrameFilterType>,
}

/// A weight table, and a sorted table of (track, bone) entries indexing it.
#[record(size = 0x40, recompute = CrFrameFilterMultiWeight::sign)]
pub struct CrFrameFilterMultiWeight {
    pub base: CrFrameFilter,
    /// ⚠ Sorted by [`TrackIdIndex::sort_key`] and binary-searched: an unsorted
    /// table loads and masks the wrong bones.
    pub track_id_indices: Vec<TrackIdIndex>,
    /// ⚠ Shorter than `track_id_indices` in 170 of 173 filters, because entries
    /// share weights: index it by `index`, never by an entry's position.
    pub weights: Vec<f32>,
    /// A runtime reference to the skeleton the filter was built against.
    skel_data: Ptr<Opaque>,
}

impl CrFrameFilterMultiWeight {
    /// The signature a save writes: [`checksum_raw`] over the tables as they are written.
    fn sign(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        cx: &Cx,
    ) -> Result<(), OwnedError> {
        let entries = self
            .track_id_indices
            .iter()
            .map(|e| e.to_raw(&mut Vec::new(), 0, cx))
            .collect::<Result<Vec<_>, _>>()?;
        raw.base.signature = checksum_raw(&entries, &self.weights);
        Ok(())
    }
}

/// One (track, bone) pair and the weight it reads.
#[record(size = 0x08)]
pub struct TrackIdIndex {
    padding: u8,
    pub track: u8,
    /// A bone's tag, not its index in a skeleton.
    pub id: u16,
    pub index: i32,
}

impl TrackIdIndex {
    pub fn new(track: u8, id: u16, index: i32) -> Self {
        Self {
            track,
            id,
            index,
            __carry: Carry::default(),
        }
    }

    /// The key the track table is sorted and searched by.
    pub fn sort_key(&self) -> u32 {
        sort_key(self.track, self.id)
    }
}

fn sort_key(track: u8, id: u16) -> u32 {
    u32::from(id) | (u32::from(track) << 16)
}

/// A filter whose tables the game would misread: why, and which filter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("filter {hash:#010x} has an unsorted track table; it is binary-searched by (track, bone), so the file would load and mask the wrong bones")]
    Unsorted { hash: u32 },
    #[error("filter {hash:#010x} has an entry for bone {id} track {track} whose index is {index} and the weight table holds {weights}")]
    WeightOutOfRange {
        hash: u32,
        id: u16,
        track: u8,
        index: i32,
        weights: usize,
    },
    #[error("the dictionary's codes are not ascending, so a lookup would find the wrong filter")]
    CodesUnsorted,
    #[error("the dictionary holds {codes} codes and {entries} filters")]
    Lengths { codes: usize, entries: usize },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// Every filter in `image`'s dictionary, checked for what the game assumes and never checks.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    let dict = View::<Raw<CrFrameFilterDictionary>>::at_offset(image, root)?;
    let codes = dict.codes()?;
    let entries = dict.entries()?;
    if codes.len() != entries.len() {
        return Err(Broken::Lengths {
            codes: codes.len(),
            entries: entries.len(),
        });
    }
    if !CrFrameFilterDictionary::codes_are_sorted(&codes) {
        return Err(Broken::CodesUnsorted);
    }
    for (i, hash) in codes.iter().enumerate() {
        let Some(filter) = entries.follow(i)? else {
            continue;
        };
        let Some(weighted) = View::variant::<Raw<CrFrameFilterMultiWeight>>(&filter)? else {
            continue;
        };
        let (table, weights) = (weighted.track_id_indices()?, weighted.weights()?);
        if !table
            .windows(2)
            .all(|w| sort_key(w[0].track, w[0].id) <= sort_key(w[1].track, w[1].id))
        {
            return Err(Broken::Unsorted { hash: *hash });
        }
        if let Some(e) = table
            .iter()
            .find(|e| e.index < 0 || e.index as usize >= weights.len())
        {
            return Err(Broken::WeightOutOfRange {
                hash: *hash,
                id: e.id,
                track: e.track,
                index: e.index,
                weights: weights.len(),
            });
        }
    }
    Ok(())
}

/// The signature a weighted filter carries: CRC-32 over its entries' bytes, then its weights'.
pub fn checksum_raw(entries: &[Raw<TrackIdIndex>], weights: &[f32]) -> u32 {
    let mut crc = Crc32::default();
    for e in entries {
        crc.update(e.as_bytes());
    }
    for w in weights {
        crc.update(&w.to_le_bytes());
    }
    crc.finish()
}

/// The reflected CRC-32 of IEEE 802.3, polynomial 0xEDB88320.
struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self(!0)
    }
}

impl Crc32 {
    fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            let mut c = (self.0 ^ u32::from(b)) & 0xFF;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            self.0 = c ^ (self.0 >> 8);
        }
    }

    fn finish(self) -> u32 {
        !self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crc_is_the_ieee_one() {
        let mut crc = Crc32::default();
        crc.update(b"123456789");
        assert_eq!(crc.finish(), 0xCBF4_3926);
    }

    #[test]
    fn an_entrys_sort_key_puts_the_track_above_the_bone() {
        let a = TrackIdIndex::new(0, 0xFFFF, 0);
        let b = TrackIdIndex::new(1, 0, 0);
        assert!(a.sort_key() < b.sort_key());
    }
}
