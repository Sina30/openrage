//! The documents say what P27 built: a drawable's mask is read and defaults by skin, the limit of alternates is a rule, and Fix
//! all safe has the mask step. A statement the wave made false is checked by the words it used.

use std::path::Path;

/// The document with every run of white space one space, so that a line break inside a sentence does not hide it.
fn doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs")
        .join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_crate_reference_says_the_mask_is_read_and_defaults_by_skin() {
    let text = doc("reference/Crate_Reference.md");
    for stale in [
        "has no resolved meaning",
        "a garment that states none gets 1 and nothing reads it",
    ] {
        assert!(
            !text.contains(stale),
            "Crate_Reference.md still says `{stale}`"
        );
    }
    assert!(
        text.contains("gets the mask its skin implies") && text.contains("`mask-skin`"),
        "Crate_Reference.md does not say the mask follows the skin"
    );
}

#[test]
fn the_clothing_design_says_the_limit_of_alternates_is_built() {
    let text = doc("direction/Clothing_Tool_Design.md");
    assert!(
        !text.contains("until the alternate-variations metadata is typed"),
        "Clothing_Tool_Design.md still says the limit of 160 alternates is not built"
    );
    assert!(
        text.contains("The limit of alternates is built") && text.contains("`alternate-limit`"),
        "Clothing_Tool_Design.md does not say where the limit of alternates is"
    );
}

#[test]
fn every_document_that_counts_the_steps_of_fix_all_safe_counts_the_mask() {
    for rel in [
        "guides/Workbench_Guide.md",
        "direction/Workbench_UX_Design.md",
    ] {
        let text = doc(rel);
        assert!(
            !text.contains("three steps"),
            "{rel} still counts three steps"
        );
        assert!(
            text.contains("four steps"),
            "{rel} does not count four steps"
        );
    }
    for (rel, said) in [
        (
            "reference/Crate_Reference.md",
            "the repair, textures, order and mask steps",
        ),
        (
            "direction/Clothing_Tool_Design.md",
            "a pipeline of repair, textures, order and mask",
        ),
        (
            "direction/Wardrobe_Parity.md",
            "runs repair, textures, order and mask",
        ),
    ] {
        assert!(doc(rel).contains(said), "{rel} does not say `{said}`");
    }
}
