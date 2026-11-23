//! A drop of a whole library is not cut at 20,000 files: the garments after them are read as the rest are.

mod common;

use std::path::{Path, PathBuf};

use or_wardrobe::edit::add_clothes;
use or_wardrobe::Wardrobe;

const C: &str = "mp_m_freemode_01_mp_m_x";

fn module(dir: &Path) -> PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    module
}

#[test]
fn a_drop_past_the_old_ceiling_of_twenty_thousand_files_reads_the_garments_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("library");
    // The folders are read in the order of their names, so everything in `a` is read before `z`.
    let junk = drop.join("a");
    std::fs::create_dir_all(&junk).unwrap();
    for n in 0..20_001u32 {
        std::fs::write(junk.join(format!("note{n:05}.txt")), b"").unwrap();
    }
    let library = drop.join("z");
    std::fs::create_dir_all(&library).unwrap();
    for n in 0..2u32 {
        std::fs::write(
            library.join(format!("{C}^jbib_{n:03}_u.ydd")),
            common::ydd(&format!("jacket {n}")),
        )
        .unwrap();
        std::fs::write(
            library.join(format!("{C}^jbib_diff_{n:03}_a_uni.ytd")),
            common::ytd(&format!("texture {n}")),
        )
        .unwrap();
    }

    let added = add_clothes(&module, std::slice::from_ref(&drop)).expect("the drop is read");

    assert_eq!(added.garments.len(), 2, "{:?}", added.left_out);
    assert!(
        added.left_out.is_empty(),
        "nothing was cut off: {:?}",
        added.left_out
    );
}
