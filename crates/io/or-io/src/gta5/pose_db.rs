//! `.ypdb`: the pose-matcher database that picks a clip from a pose.
//!
//! ⚠ **Not an RSC7 resource**: no paging, no pointers, no version registry. It
//! is one serialized stream, read and written here as ordinary code. Its first
//! byte is `0x1A`, which the engine's deserializer reads as "this stream is
//! binary". A short read is an error naming the field, because no envelope
//! above this catches a truncation.

use crate::decl::{File, HasOrigin, Origin};
use crate::error::Error;

const EXT: &str = "ypdb";

/// The engine deserializer's "this stream is binary" marker.
pub const YPDB_BINARY_MARKER: u8 = 0x1A;

/// The serializer revision every shipped file carries.
pub const YPDB_SERIALIZER_VERSION: i32 = 2;

/// One pose in the matcher's table, and the clip it selects.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PoseMatcherSample {
    pub clip_set: u32,
    pub clip: u32,
    /// Seconds into the clip the cloud was sampled at, so one clip can
    /// contribute several poses.
    pub offset: f32,
    pub cloud: PoseMatcherPointCloud,
}

/// The point cloud a pose is matched against.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PoseMatcherPointCloud {
    pub points: Vec<[f32; 3]>,
    /// ⚠ One per point, and stored with its own count. The two counts are
    /// equal in every shipped file and are still read separately: a file that
    /// disagreed with itself is a file this would otherwise mis-slice.
    pub weights: Vec<f32>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    /// Carried, never recomputed: summing the weights back gives a different
    /// last bit, and the engine compares this value rather than deriving it.
    pub weights_sum: f32,
}

/// A `.ypdb`, opened and saved through [`File`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PoseMatcherDatabase {
    pub serializer_version: i32,
    pub pose_matcher_version: i32,
    /// Written twice, once in the header and once as the last four bytes,
    /// which is how the engine checks it read the whole stream.
    pub signature: u32,
    pub samples: Vec<PoseMatcherSample>,
    /// Bone tags, in the order the weight set is indexed by.
    pub bone_tags: Vec<u16>,
    /// One weight per bone tag.
    pub weight_set: Vec<f32>,
    /// The two words before the closing signature, carried as the file states
    /// them: every shipped file holds `1/30` as an `f32`, then `1`.
    trailer: [u32; 2],
    origin: Origin,
}

impl PoseMatcherDatabase {
    pub fn point_count(&self) -> usize {
        self.samples.iter().map(|s| s.cloud.points.len()).sum()
    }
}

impl HasOrigin for PoseMatcherDatabase {
    fn origin(&self) -> &Origin {
        &self.origin
    }

    fn origin_mut(&mut self) -> &mut Origin {
        &mut self.origin
    }
}

impl File for PoseMatcherDatabase {
    const EXTENSION: &'static str = EXT;

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        read(bytes).map_err(|why| Error::Read { ext: EXT, why })
    }

    /// Refused when the weight set and the bone tags differ in length.
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        write(self).map_err(|why| Error::Save { ext: EXT, why })
    }
}

/// Whether a blob is a binary pose-matcher database.
pub fn is_ypdb(bytes: &[u8]) -> bool {
    bytes.first() == Some(&YPDB_BINARY_MARKER)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take<const N: usize>(&mut self, what: &str) -> Result<[u8; N], String> {
        let end = self.at.checked_add(N).filter(|&e| e <= self.bytes.len());
        let Some(end) = end else {
            return Err(format!("input ends inside {what} at offset {}", self.at));
        };
        let mut out = [0; N];
        out.copy_from_slice(&self.bytes[self.at..end]);
        self.at = end;
        Ok(out)
    }

    fn u16(&mut self, what: &str) -> Result<u16, String> {
        self.take(what).map(u16::from_le_bytes)
    }

    fn u32(&mut self, what: &str) -> Result<u32, String> {
        self.take(what).map(u32::from_le_bytes)
    }

    fn i32(&mut self, what: &str) -> Result<i32, String> {
        self.take(what).map(i32::from_le_bytes)
    }

    fn f32(&mut self, what: &str) -> Result<f32, String> {
        self.take(what).map(f32::from_le_bytes)
    }

    fn vec3(&mut self, what: &str) -> Result<[f32; 3], String> {
        Ok([self.f32(what)?, self.f32(what)?, self.f32(what)?])
    }

    /// A length prefix, refused when it cannot fit in what is left.
    ///
    /// ⚠ The count is a signed `i32` in the file. A negative one is not a
    /// large array, and allocating for it is how a fuzzed file turns into an
    /// out-of-memory abort rather than an error.
    fn count(&mut self, what: &str, element: usize) -> Result<usize, String> {
        let raw = self.i32(what)?;
        let n = usize::try_from(raw).map_err(|_| format!("{what} count is negative ({raw})"))?;
        let left = self.bytes.len() - self.at;
        let need = n.saturating_mul(element);
        if need > left {
            return Err(format!(
                "{what} declares {n} elements ({need} bytes) and only {left} remain; the file is \
                 truncated"
            ));
        }
        Ok(n)
    }

    fn run<T>(
        &mut self,
        what: &str,
        element: usize,
        mut one: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let n = self.count(what, element)?;
        (0..n).map(|_| one(self)).collect()
    }
}

fn read(bytes: &[u8]) -> Result<PoseMatcherDatabase, String> {
    let mut c = Cursor { bytes, at: 0 };

    let [marker] = c.take::<1>("the binary marker")?;
    if marker != YPDB_BINARY_MARKER {
        return Err(format!(
            "the first byte is {marker:#04X}, not the {YPDB_BINARY_MARKER:#04X} the engine \
             deserializer reads as binary; a text pose-matcher database is a different format"
        ));
    }

    let serializer_version = c.i32("the serializer version")?;
    let pose_matcher_version = c.i32("the pose-matcher version")?;
    let signature = c.u32("the signature")?;

    let samples = c.run("samples", 12, |c| {
        Ok(PoseMatcherSample {
            clip_set: c.u32("a sample clip set")?,
            clip: c.u32("a sample clip")?,
            offset: c.f32("a sample offset")?,
            cloud: PoseMatcherPointCloud {
                points: c.run("a point cloud", 12, |c| c.vec3("a cloud point"))?,
                weights: c.run("cloud weights", 4, |c| c.f32("a cloud weight"))?,
                bounds_min: c.vec3("a cloud lower bound")?,
                bounds_max: c.vec3("a cloud upper bound")?,
                weights_sum: c.f32("a cloud weight sum")?,
            },
        })
    })?;
    let bone_tags = c.run("bone tags", 2, |c| c.u16("a bone tag"))?;
    let weight_set = c.run("the weight set", 4, |c| c.f32("a set weight"))?;
    let trailer = [c.u32("the trailer")?, c.u32("the trailer")?];

    let closing = c.u32("the closing signature")?;
    if closing != signature {
        return Err(format!(
            "the closing signature is {closing:#010X} and the header's is {signature:#010X}; the \
             stream did not end where the header said it would"
        ));
    }
    if c.at != bytes.len() {
        return Err(format!(
            "{} bytes follow the closing signature",
            bytes.len() - c.at
        ));
    }

    Ok(PoseMatcherDatabase {
        serializer_version,
        pose_matcher_version,
        signature,
        samples,
        bone_tags,
        weight_set,
        trailer,
        origin: Origin::default(),
    })
}

/// ⚠ [`PoseMatcherDatabase::weight_set`] is indexed by a bone tag's position
/// in [`PoseMatcherDatabase::bone_tags`]. The two are equal in all 112 files
/// of both installs; a database that had gained a tag and not a weight would
/// save and weight the wrong bones in game.
fn write(db: &PoseMatcherDatabase) -> Result<Vec<u8>, String> {
    if db.bone_tags.len() != db.weight_set.len() {
        return Err(format!(
            "{} bone tags and {} weights; the weight set is indexed by a tag's position, so the \
             two have to be the same length",
            db.bone_tags.len(),
            db.weight_set.len()
        ));
    }

    let mut out = vec![YPDB_BINARY_MARKER];
    out.extend_from_slice(&db.serializer_version.to_le_bytes());
    out.extend_from_slice(&db.pose_matcher_version.to_le_bytes());
    out.extend_from_slice(&db.signature.to_le_bytes());

    put_count(&mut out, db.samples.len(), "samples")?;
    for sample in &db.samples {
        out.extend_from_slice(&sample.clip_set.to_le_bytes());
        out.extend_from_slice(&sample.clip.to_le_bytes());
        out.extend_from_slice(&sample.offset.to_le_bytes());
        let cloud = &sample.cloud;
        put_count(&mut out, cloud.points.len(), "a point cloud")?;
        for p in &cloud.points {
            put_floats(&mut out, p);
        }
        put_count(&mut out, cloud.weights.len(), "cloud weights")?;
        put_floats(&mut out, &cloud.weights);
        put_floats(&mut out, &cloud.bounds_min);
        put_floats(&mut out, &cloud.bounds_max);
        out.extend_from_slice(&cloud.weights_sum.to_le_bytes());
    }

    put_count(&mut out, db.bone_tags.len(), "bone tags")?;
    for tag in &db.bone_tags {
        out.extend_from_slice(&tag.to_le_bytes());
    }
    put_count(&mut out, db.weight_set.len(), "the weight set")?;
    put_floats(&mut out, &db.weight_set);

    for word in db.trailer {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&db.signature.to_le_bytes());
    Ok(out)
}

fn put_count(out: &mut Vec<u8>, n: usize, what: &str) -> Result<(), String> {
    let n =
        i32::try_from(n).map_err(|_| format!("{what} holds {n} elements; the count is an i32"))?;
    out.extend_from_slice(&n.to_le_bytes());
    Ok(())
}

fn put_floats(out: &mut Vec<u8>, floats: &[f32]) {
    for f in floats {
        out.extend_from_slice(&f.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_db() -> PoseMatcherDatabase {
        PoseMatcherDatabase {
            serializer_version: YPDB_SERIALIZER_VERSION,
            pose_matcher_version: 0,
            signature: 0x9907_4A2E,
            samples: vec![PoseMatcherSample {
                clip_set: 0x9E87_FE9A,
                clip: 0x15A0_6554,
                offset: 0.266,
                cloud: PoseMatcherPointCloud {
                    points: vec![[1.0, 2.0, 3.0], [-1.0, 0.5, 0.0]],
                    weights: vec![0.25, 0.75],
                    bounds_min: [-1.0, 0.0, 0.0],
                    bounds_max: [1.0, 2.0, 3.0],
                    weights_sum: 1.0,
                },
            }],
            bone_tags: vec![0x0010, 0xB1CA],
            weight_set: vec![1.0, 0.5],
            trailer: [(1.0f32 / 30.0).to_bits(), 1],
            origin: Origin::default(),
        }
    }

    #[test]
    fn a_database_authored_from_nothing_is_byte_identical_on_reload() {
        let db = sample_db();
        let bytes = db.to_bytes().unwrap();
        let back = PoseMatcherDatabase::from_bytes(&bytes).unwrap();
        assert_eq!(back, db);
        assert_eq!(back.trailer, db.trailer);
        assert_eq!(back.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn a_default_database_saves_and_reopens() {
        let bytes = PoseMatcherDatabase::default().to_bytes().unwrap();
        assert!(is_ypdb(&bytes));
        assert_eq!(
            PoseMatcherDatabase::from_bytes(&bytes).unwrap(),
            PoseMatcherDatabase::default()
        );
    }

    /// ⚠ The falsifier for the write-side refusal: the edit reads back
    /// perfectly and the game weights the wrong bones.
    #[test]
    fn a_weight_set_shorter_than_its_bone_tags_is_refused_on_save() {
        let mut db = sample_db();
        db.bone_tags.push(0x1234);
        let why = db.to_bytes().expect_err("should refuse");
        assert!(why.to_string().contains("bone tags"), "{why}");
        assert!(sample_db().to_bytes().is_ok(), "the sample itself is fine");
    }

    #[test]
    fn a_stream_that_is_not_the_binary_form_is_refused() {
        let why = PoseMatcherDatabase::from_bytes(b"<PoseMatcher>").expect_err("should refuse");
        assert!(why.to_string().contains("binary"), "{why}");
    }

    /// ⚠ The closing signature is the engine's own end-of-stream check, and
    /// it is the only thing in the format that can catch a truncation that
    /// happens to land on a plausible count.
    #[test]
    fn a_closing_signature_that_disagrees_is_refused() {
        let mut bytes = sample_db().to_bytes().unwrap();
        let n = bytes.len();
        bytes[n - 1] ^= 0xFF;
        let why = PoseMatcherDatabase::from_bytes(&bytes).expect_err("should refuse");
        assert!(why.to_string().contains("closing signature"), "{why}");
    }

    #[test]
    fn bytes_after_the_closing_signature_are_refused() {
        let mut bytes = sample_db().to_bytes().unwrap();
        bytes.push(0);
        let why = PoseMatcherDatabase::from_bytes(&bytes).expect_err("should refuse");
        assert!(why.to_string().contains("follow"), "{why}");
    }

    #[test]
    fn a_negative_count_is_refused_rather_than_allocated_for() {
        let mut bytes = sample_db().to_bytes().unwrap();
        bytes[13..17].copy_from_slice(&(-1i32).to_le_bytes());
        let why = PoseMatcherDatabase::from_bytes(&bytes).expect_err("should refuse");
        assert!(why.to_string().contains("negative"), "{why}");
    }

    #[test]
    fn a_count_larger_than_the_input_is_refused() {
        let mut bytes = sample_db().to_bytes().unwrap();
        bytes[13..17].copy_from_slice(&1_000_000i32.to_le_bytes());
        let why = PoseMatcherDatabase::from_bytes(&bytes).expect_err("should refuse");
        assert!(why.to_string().contains("truncated"), "{why}");
    }

    #[test]
    fn a_truncated_file_errors_rather_than_returning_a_short_database() {
        let bytes = sample_db().to_bytes().unwrap();
        for cut in [1, 5, 13, 20, bytes.len() - 1] {
            assert!(
                PoseMatcherDatabase::from_bytes(&bytes[..cut]).is_err(),
                "cut at {cut}"
            );
        }
    }
}
