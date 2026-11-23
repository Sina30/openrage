//! A drop of many files tells how far it has got, and stops when it is asked to with the pack and its folder as they were.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_wardrobe::edit::{add_clothes_watched, Error, Watch};
use or_wardrobe::Wardrobe;

const C: &str = "mp_m_freemode_01_mp_m_x";

fn module(dir: &Path) -> PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    module
}

/// `count` jackets of different bytes, each with a texture, in a folder of the drop.
fn jackets(drop: &Path, count: u32) {
    std::fs::create_dir_all(drop).unwrap();
    for n in 0..count {
        std::fs::write(
            drop.join(format!("{C}^jbib_{n:03}_u.ydd")),
            common::ydd(&format!("jacket {n}")),
        )
        .unwrap();
        std::fs::write(
            drop.join(format!("{C}^jbib_diff_{n:03}_a_uni.ytd")),
            common::ytd(&format!("texture {n}")),
        )
        .unwrap();
    }
}

#[test]
fn a_drop_says_how_far_it_has_got_and_ends_at_the_whole() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("library");
    jackets(&drop, 40);
    let watch = Watch::default();

    let added = add_clothes_watched(&module, std::slice::from_ref(&drop), &watch).unwrap();

    assert_eq!(added.garments.len(), 40);
    let at = watch.progress.snapshot();
    assert_eq!((at.attempted, at.total), (40, 40), "{at:?}");
    assert!((at.fraction() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn a_drop_that_is_stopped_leaves_the_pack_and_its_folder_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("library");
    jackets(&drop, 1_500);
    let before = Wardrobe::read(&module).unwrap();
    let watch = Watch::default();

    let stopper = {
        let watch = watch.clone();
        std::thread::spawn(move || {
            // Stop it once it has got somewhere.
            let begun = Instant::now();
            while watch.progress.snapshot().attempted < 5
                && begun.elapsed() < Duration::from_secs(60)
            {
                std::thread::yield_now();
            }
            watch.cancel.set();
        })
    };
    let result = add_clothes_watched(&module, std::slice::from_ref(&drop), &watch);
    stopper.join().unwrap();

    assert!(matches!(result, Err(Error::Stopped)), "{result:?}");
    assert_eq!(Wardrobe::read(&module).unwrap(), before);
    assert!(
        !module.join("models").exists() && !module.join("textures").exists(),
        "what it had copied is taken out again"
    );
}

#[test]
fn a_drop_stopped_before_it_begins_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("library");
    jackets(&drop, 3);
    let watch = Watch::default();
    watch.cancel.set();

    let result = add_clothes_watched(&module, std::slice::from_ref(&drop), &watch);

    assert!(matches!(result, Err(Error::Stopped)), "{result:?}");
    assert!(Wardrobe::read(&module).unwrap().garments.is_empty());
}
