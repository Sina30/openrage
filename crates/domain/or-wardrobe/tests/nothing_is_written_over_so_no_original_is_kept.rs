//! A replacement writes over nothing (P33), so the Wardrobe keeps no original of a file and no copy of a file written over in
//! place: the two modules that did are gone, with the field a step of Undo carried for them.

use std::path::Path;

fn src(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel)
}

#[test]
fn the_engine_has_no_module_that_keeps_originals_or_files_written_over() {
    for gone in ["originals.rs", "rewrite.rs"] {
        assert!(!src(gone).exists(), "src/{gone} is still there");
    }
    let lib = std::fs::read_to_string(src("lib.rs")).unwrap();
    for gone in ["mod originals", "mod rewrite"] {
        assert!(!lib.contains(gone), "lib.rs still declares `{gone}`");
    }
}

#[test]
fn a_step_of_undo_carries_the_pack_and_its_copies_and_nothing_written_over() {
    for file in [
        "edit.rs",
        "redo.rs",
        "duplicate.rs",
        "replace.rs",
        "variations.rs",
    ] {
        let text = std::fs::read_to_string(src(file)).unwrap();
        assert!(
            !text.contains("rewritten"),
            "src/{file} still names files written over"
        );
    }
}
