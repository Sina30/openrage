//! `.ypdb` over the corpus: the owned value against a second walk of the
//! stream by its counts alone, the save as a fixed point, and the file trait's
//! open and save.
//!
//! ⚠ A `.ypdb` is unpaged, so nothing in the file says which install wrote it:
//! both installs' files are read with the one reader.

use std::collections::BTreeMap;
use std::path::PathBuf;

use or_io::gta5::pose_db::{PoseMatcherDatabase, YPDB_SERIALIZER_VERSION};
use or_io::gta5::File;
use or_io_verify::gates::{corpus_root, name};
use or_test_support::ledger::{self, SkipReason};
use or_test_support::Corpus;

fn corpus(what: &str) -> Option<Vec<(PathBuf, Vec<u8>)>> {
    let Some(root) = corpus_root() else {
        ledger::record(SkipReason::Corpus, what, "no corpus is staged");
        return None;
    };
    let paths = Corpus::with_root(root, "ypdb")
        .all_targets()
        .find_or_skip(what)?;
    Some(
        paths
            .into_iter()
            .map(|p| {
                let b = std::fs::read(&p).expect("a corpus file reads");
                (p, b)
            })
            .collect(),
    )
}

/// The file's words, read at the offsets its counts put them, with no
/// knowledge of the reader under test.
struct Walk<'a> {
    b: &'a [u8],
    at: usize,
}

impl Walk<'_> {
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.b[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }

    fn u16(&mut self) -> u16 {
        let v = u16::from_le_bytes(self.b[self.at..self.at + 2].try_into().unwrap());
        self.at += 2;
        v
    }

    fn words(&mut self, n: usize) -> Vec<u32> {
        (0..n).map(|_| self.u32()).collect()
    }
}

fn bits(floats: &[f32]) -> Vec<u32> {
    floats.iter().map(|f| f.to_bits()).collect()
}

#[test]
fn every_ypdb_reads_as_the_stream_reads() {
    let Some(files) = corpus("the .ypdb stream walk") else {
        return;
    };
    let (mut samples, mut points, mut tags) = (0, 0, 0);
    for (path, b) in &files {
        let at = name(path);
        let db = PoseMatcherDatabase::from_bytes(b).unwrap_or_else(|e| panic!("{at}: {e}"));
        let mut w = Walk { b, at: 1 };
        assert_eq!(b[0], 0x1A, "{at}");
        assert_eq!(
            w.words(3),
            [
                db.serializer_version as u32,
                db.pose_matcher_version as u32,
                db.signature
            ],
            "{at}: the header"
        );
        assert_eq!(w.u32() as usize, db.samples.len(), "{at}: the sample count");
        for (i, s) in db.samples.iter().enumerate() {
            assert_eq!(
                w.words(3),
                [s.clip_set, s.clip, s.offset.to_bits()],
                "{at}: sample {i}"
            );
            let n = w.u32() as usize;
            let flat: Vec<f32> = s.cloud.points.iter().flatten().copied().collect();
            assert_eq!(w.words(n * 3), bits(&flat), "{at}: sample {i}'s points");
            let n = w.u32() as usize;
            assert_eq!(
                w.words(n),
                bits(&s.cloud.weights),
                "{at}: sample {i}'s weights"
            );
            let mut tail = s.cloud.bounds_min.to_vec();
            tail.extend(s.cloud.bounds_max);
            tail.push(s.cloud.weights_sum);
            assert_eq!(w.words(7), bits(&tail), "{at}: sample {i}'s bounds and sum");
            points += s.cloud.points.len();
        }
        samples += db.samples.len();
        let n = w.u32() as usize;
        assert_eq!(
            (0..n).map(|_| w.u16()).collect::<Vec<_>>(),
            db.bone_tags,
            "{at}: the bone tags"
        );
        tags += n;
        let n = w.u32() as usize;
        assert_eq!(w.words(n), bits(&db.weight_set), "{at}: the weight set");
        w.words(2);
        assert_eq!(w.u32(), db.signature, "{at}: the closing signature");
        assert_eq!(w.at, b.len(), "{at}: the walk ends where the file does");
    }
    println!(
        "{} .ypdb read as the stream reads: {samples} samples, {points} cloud points, {tags} bone tags",
        files.len()
    );
    assert!(samples > 0 && points > 0, "the walk reached no sample");
}

/// ⚠ An unpaged stream has no layout for a writer to choose differently, so
/// the canonical form is the shipped bytes, and the fixed point is checked
/// from both.
#[test]
fn every_ypdb_saves_to_a_fixed_point() {
    let Some(files) = corpus("the .ypdb fixed point") else {
        return;
    };
    let mut moved = Vec::new();
    for (path, b) in &files {
        let db = PoseMatcherDatabase::from_bytes(b).unwrap();
        let once = db.to_bytes().unwrap();
        let back = PoseMatcherDatabase::from_bytes(&once).unwrap();
        assert_eq!(back, db, "{}", name(path));
        assert_eq!(back.to_bytes().unwrap(), once, "{}", name(path));
        if once != *b {
            moved.push(name(path));
        }
    }
    println!("{} .ypdb saved to a fixed point", files.len());
    assert!(
        moved.is_empty(),
        "saved bytes differ from the shipped file: {moved:?}"
    );
}

/// What the corpus holds of what the owned value carries or assumes.
#[test]
fn what_a_pose_matcher_database_holds() {
    let Some(files) = corpus("the .ypdb census") else {
        return;
    };
    let mut versions = BTreeMap::<(i32, i32), usize>::new();
    let mut trailers = BTreeMap::<[u32; 2], usize>::new();
    let (mut equal_clouds, mut clouds, mut equal_sets) = (0, 0, 0);
    for (_, b) in &files {
        let db = PoseMatcherDatabase::from_bytes(b).unwrap();
        *versions
            .entry((db.serializer_version, db.pose_matcher_version))
            .or_default() += 1;
        let tail = b.len() - 12;
        let mut w = Walk { b, at: tail };
        *trailers.entry([w.u32(), w.u32()]).or_default() += 1;
        for s in &db.samples {
            clouds += 1;
            equal_clouds += usize::from(s.cloud.points.len() == s.cloud.weights.len());
        }
        equal_sets += usize::from(db.bone_tags.len() == db.weight_set.len());
    }
    println!("{} .ypdb", files.len());
    println!("  (serializer, pose-matcher) versions: {versions:?}");
    println!(
        "  trailers: {:?}",
        trailers
            .iter()
            .map(|(t, n)| ((f32::from_bits(t[0]), t[1]), n))
            .collect::<Vec<_>>()
    );
    println!("  clouds with one weight per point: {equal_clouds}/{clouds}");
    println!(
        "  weight sets as long as their bone tags: {equal_sets}/{}",
        files.len()
    );
    assert_eq!(
        versions.keys().copied().collect::<Vec<_>>(),
        [(YPDB_SERIALIZER_VERSION, 0)],
        "a second serializer revision ships and this reader assumes one"
    );
}

/// ⚠ Every byte of the file has to be consumed. A reader that stopped early
/// would still save a prefix and report success.
#[test]
fn a_flipped_byte_anywhere_changes_the_output() {
    let Some(files) = corpus("the .ypdb coverage") else {
        return;
    };
    let b = &files[0].1;
    let mut unread = Vec::new();
    for i in (0..b.len()).step_by((b.len() / 64).max(1)) {
        let mut poked = b.clone();
        poked[i] ^= 0xFF;
        if let Ok(db) = PoseMatcherDatabase::from_bytes(&poked) {
            if db.to_bytes().unwrap() == *b {
                unread.push(i);
            }
        }
    }
    assert!(
        unread.is_empty(),
        "these offsets reach no field the writer emits: {unread:?}"
    );
}

#[test]
fn a_ypdb_opens_edits_and_saves_through_the_file_trait() {
    let Some(files) = corpus("the .ypdb file trait") else {
        return;
    };
    let (path, b) = &files[0];
    let mut db = PoseMatcherDatabase::open(path).unwrap();
    assert_eq!(db.to_bytes().unwrap(), *b);

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("edited.ypdb");
    db.samples[0].offset += 1.0;
    db.save_as(&out).unwrap();
    let reopened = PoseMatcherDatabase::open(&out).unwrap();
    assert_eq!(reopened, db);
    reopened.save().unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), db.to_bytes().unwrap());
}
