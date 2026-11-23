//! A review draws the pictures of the two sides of a garment that differs: the garment as the older version of the pack held it and as the newer does.
//!
//! ⚠ Drawn on a worker's own surfaceless device, as the catalogue's pictures are, so it is skipped where there is no adapter.

use std::path::PathBuf;

use or_test_support::skip_corpus;

use super::the_dressed_ped::{fitted_from, staged, until, WHAT};

#[test]
fn a_review_draws_the_garment_as_each_version_held_it_and_the_two_pictures_differ() {
    let Some(mut f) = staged("ydd", 2).and_then(fitted_from) else {
        return;
    };
    if let Err(why) = crate::self_check::headless_bridge() {
        skip_corpus(WHAT, &why);
        return;
    }
    // The jacket's textures are swapped, so the picture of its first texture is another one; the files of both stay in the pack.
    let dir = tempfile::tempdir().unwrap();
    let module = f.state.wardrobe_module_id().unwrap();
    let jacket = f.jacket.clone();
    let first = (f.state.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
        .and_then(|read| read.wardrobe.garment(&jacket))
        .and_then(|g| g.variations[0].texture.clone());
    f.state.move_variation(jacket.clone(), 0, first.clone(), 1);
    until(&mut f.state, "the swap", |s| {
        s.workbench.wardrobe.is_idle()
            && (s.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
                .and_then(|read| read.wardrobe.garment(&jacket))
                .is_some_and(|g| g.variations[0].texture != first)
    });
    f.state.load_versions();
    let before = f
        .state
        .workbench
        .wardrobe
        .versions
        .as_ref()
        .and_then(|v| v.list.get(1))
        .map(|(version, _)| version.n)
        .expect("the pack kept the version before the swap");

    let review = f.state.review_of(&module, Some(before), None).unwrap();
    assert_eq!(review.garments.len(), 1, "{review:?}");
    assert_eq!(review.garments[0].id, jacket);

    let folder = dir.path().join("sides");
    f.state
        .review_pictures(&module, Some(before), None, folder.clone());
    until(&mut f.state, "the pictures", |s| {
        (s.workbench.wardrobe.dressed.catalogue.as_ref()).is_some_and(|c| c.is_finished())
    });
    let job = f.state.workbench.wardrobe.dressed.catalogue.take().unwrap();
    assert!(job.failed.is_empty(), "{:?}", job.failed);
    assert_eq!(job.written, 2, "one picture of each side");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        names[0].ends_with("_after.webp") && names[1].ends_with("_before.webp"),
        "{names:?}"
    );
    let drawn: Vec<usize> = files
        .iter()
        .map(|file| {
            let picture = image::open(file).expect("a picture").to_rgba8();
            picture.pixels().filter(|p| p.0[3] > 0).count()
        })
        .collect();
    assert!(
        drawn.iter().all(|n| *n > 0),
        "both sides draw something: {drawn:?}"
    );
    assert_ne!(
        std::fs::read(&files[0]).unwrap(),
        std::fs::read(&files[1]).unwrap(),
        "the two sides are not the same picture"
    );
}
