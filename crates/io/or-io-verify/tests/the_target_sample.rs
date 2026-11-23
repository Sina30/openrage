//! The file half of the D16 target, run on a copy of each staged Legacy
//! `.ytd` so the corpus is never written.

#[path = "../../or-io/examples/the_target/sample.rs"]
mod sample;

use std::path::{Path, PathBuf};

use or_io::codec::texture::{save_png, RgbaImage};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::{File, GtaDrawable, PgDictionary};
use or_io::Generation;
use or_io_verify::gates;

/// What the sample leaves in a copy of `bytes`, reached without the sample.
fn expected(bytes: &[u8], scratch: &Path) -> or_io::Result<PgDictionary<GrcTexture>> {
    let mut ytd = PgDictionary::<GrcTexture>::from_bytes(bytes)?;
    for tex in ytd.entries.iter().flatten() {
        tex.to_png(scratch.join("expected.png"))?;
    }
    ytd.insert(GrcTexture::from_png(scratch.join("decal.png"))?);
    PgDictionary::<GrcTexture>::from_bytes(&ytd.to_bytes()?)
}

/// Each shard runs in its own process, as nextest runs every test: the sample works in the current directory.
const SHARDS: usize = 8;

/// The files of `shard`, dealt largest first to the lightest shard so each carries about the same bytes.
fn take(paths: Vec<PathBuf>, shard: usize) -> Vec<PathBuf> {
    let mut sized: Vec<(u64, PathBuf)> = paths
        .into_iter()
        .map(|p| (std::fs::metadata(&p).map_or(0, |m| m.len()), p))
        .collect();
    sized.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    let mut load = [0u64; SHARDS];
    let mut mine = Vec::new();
    for (size, path) in sized {
        let to = (0..SHARDS).min_by_key(|&s| load[s]).expect("a shard");
        load[to] += size.max(1);
        if to == shard {
            mine.push(path);
        }
    }
    mine
}

fn run(shard: usize) {
    let Some(paths) = gates::files("ytd", Some(Generation::Gen8), "the target sample") else {
        return;
    };
    let paths = take(paths, shard);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("the_target_sample_{shard}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).expect("the scratch directory is made");
    let mut decal = RgbaImage::new(8, 8);
    for i in 0..8 {
        decal.set_pixel(i, i, [255, 0, 0, 255]);
    }
    save_png(dir.join("decal.png"), &decal).expect("the decal is written");
    std::env::set_current_dir(&dir).expect("the scratch directory is the working directory");

    let (mut ran, mut refused) = (0, 0);
    for path in &paths {
        let name = path.file_name().expect("a corpus file has a name");
        let copy = dir.join(name);
        std::fs::copy(path, &copy).expect("the staged file copies");
        let got = sample::run(&copy);
        let bytes = std::fs::read(path).expect("the staged file reads");
        match expected(&bytes, &dir) {
            Ok(want) => {
                got.unwrap_or_else(|e| panic!("{name:?}: the sample failed: {e}"));
                let saved = PgDictionary::<GrcTexture>::open(&copy)
                    .unwrap_or_else(|e| panic!("{name:?} does not reopen after the sample: {e}"));
                assert!(
                    saved == want,
                    "{name:?}: the sample's save is not its dictionary with the decal inserted"
                );
                ran += 1;
            }
            Err(want) => {
                let got = got.expect_err("a file the owned form refuses fails the sample");
                assert_eq!(
                    got.to_string(),
                    want.to_string(),
                    "{name:?}: the sample fails for another reason than the owned form's"
                );
                println!("refused by name: {name:?}: {got}");
                refused += 1;
            }
        }
    }
    println!(
        "the target sample ran on {ran} of {} Legacy .ytd, {refused} refused by name",
        paths.len()
    );
    assert!(shard > 0 || ran > 0, "the sample ran on no staged file");

    let Some(drawables) = gates::files(
        "ydr",
        Some(Generation::Gen8),
        "the target sample's drawable",
    ) else {
        return;
    };
    let drawables = take(drawables, shard);
    let mut edited = 0;
    for path in &drawables {
        let Ok(mut before) = GtaDrawable::open(path) else {
            continue;
        };
        // The sample removes the second model of the first level of detail, and
        // no staged drawable's first level holds two: the first is doubled.
        let Some(first) = before.lod_group.lods[0].models.first().cloned() else {
            continue;
        };
        before.lod_group.lods[0].models.push(first);
        before
            .save_as(dir.join("police.ydr"))
            .unwrap_or_else(|e| panic!("{path:?}: the doubled drawable saves: {e}"));
        sample::drawable().unwrap_or_else(|e| panic!("{path:?}: the drawable half failed: {e}"));
        let after = GtaDrawable::open(dir.join("police.ydr")).expect("the saved drawable reopens");
        assert_eq!(after.name, "police2");
        assert_eq!(after.lod_group.lod_distances[0], 45.0);
        assert_eq!(
            after.lod_group.lod_distances[1..],
            before.lod_group.lod_distances[1..]
        );
        assert_eq!(
            after.lod_group.lods[0].models.len(),
            before.lod_group.lods[0].models.len() - 1
        );
        edited += 1;
    }
    println!(
        "the target sample's drawable half ran on {edited} of {} Legacy .ydr",
        drawables.len()
    );
    assert!(
        shard > 0 || edited > 0,
        "the drawable half ran on no staged file"
    );
}

mod the_target_sample_runs_on_a_staged_file {
    use super::run;

    #[test]
    fn shard_0() {
        run(0)
    }
    #[test]
    fn shard_1() {
        run(1)
    }
    #[test]
    fn shard_2() {
        run(2)
    }
    #[test]
    fn shard_3() {
        run(3)
    }
    #[test]
    fn shard_4() {
        run(4)
    }
    #[test]
    fn shard_5() {
        run(5)
    }
    #[test]
    fn shard_6() {
        run(6)
    }
    #[test]
    fn shard_7() {
        run(7)
    }
}
