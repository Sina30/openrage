//! The scan of a pack of 5,000 garments: counted once, from the files, and then from what the folder kept.
//!
//! The measurement is the wall-clock time of `planned_with_model` at `Depth::Measure`, from the call to its return, which
//! reads `wardrobe.json`, looks at the 10,000 files the garments name, counts what each holds, applies every rule and plans
//! the build. The bounds are generous: a machine that is busy with other work must pass them, and the figures that were
//! measured are printed, so a change that makes the scan slower shows in the run before it breaks a bound.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::built::{ytd, Tex};
use common::garment;
use or_io::codec::texture::GrcTextureFormat as F;
use or_project::content::facts::files_read;
use or_project::Module;
use or_wardrobe::{cache, planned_with_model, Depth, Ped, Slot, Wardrobe};

const GARMENTS: usize = 5_000;

/// A pack of `GARMENTS` garments, each with a model and a texture of its own, in folders of a few hundred files as a pack is.
fn pack(folder: &Path) {
    let long_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    let mut w = Wardrobe::new("pack");
    let places: Vec<(Ped, Slot)> = Ped::ALL
        .into_iter()
        .flat_map(|ped| Slot::all().map(move |slot| (ped, slot)))
        .collect();
    // The name of the texture differs for each, so no two files share a digest and none is a duplicate.
    let models = |i: usize| ytd(&[Tex::new(format!("m{i}"), 64, 64, F::DXT1)]);
    let textures = |i: usize| ytd(&[Tex::new(format!("t{i}"), 256, 256, F::DXT1)]);
    for i in 0..GARMENTS {
        let (model, texture) = (
            format!("models/{:02}/m{i}.ytd", i / 250),
            format!("textures/{:02}/t{i}.ytd", i / 250),
        );
        for (name, bytes) in [
            (&model, &models as &dyn Fn(usize) -> Vec<u8>),
            (&texture, &textures),
        ] {
            let path = folder.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes(i)).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        // A slot holds 255 garments at most, so 5,000 are spread over every slot of both peds.
        let (ped, slot) = places[i % places.len()];
        w.garments.push(garment(
            ped,
            slot,
            Some((i / places.len()) as u32),
            &model,
            &[&texture],
        ));
    }
    w.write(folder).unwrap();
}

fn scan(folder: &Path) -> (Duration, usize) {
    let module = Module::clothing("wardrobe", ".");
    let reads = files_read();
    let started = Instant::now();
    let (wardrobe, planned) =
        planned_with_model(folder, &module, Depth::Measure).expect("a pack reads");
    let took = started.elapsed();
    assert_eq!(wardrobe.garments.len(), GARMENTS);
    assert!(
        planned.allocation.placed.len() >= GARMENTS - 4,
        "the garments are placed"
    );
    (took, files_read() - reads)
}

#[test]
fn a_pack_of_five_thousand_garments_is_counted_once_and_then_read_by_what_changed() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());

    let (cold, read) = scan(dir.path());
    eprintln!("cold: {GARMENTS} garments, {read} files read, {cold:?}");
    assert!(read >= 2 * GARMENTS, "the first scan read every file");
    assert!(cold < Duration::from_secs(120), "a cold scan took {cold:?}");

    cache::save(dir.path());
    cache::forget(dir.path());
    let (warm, read) = scan(dir.path());
    eprintln!("warm: {GARMENTS} garments, {read} files read, {warm:?}");
    assert_eq!(read, 0, "the next session read no file");
    assert!(warm < Duration::from_secs(15), "a warm scan took {warm:?}");

    // One garment's texture changes: that one file is read, and the pack is scanned as fast as before.
    let path = dir.path().join("textures/04/t1042.ytd");
    std::fs::write(&path, ytd(&[Tex::new("t", 512, 512, F::DXT1)])).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(1800))
        .unwrap();
    let (again, read) = scan(dir.path());
    eprintln!("one file changed: {read} file read, {again:?}");
    assert_eq!(read, 1);
}
