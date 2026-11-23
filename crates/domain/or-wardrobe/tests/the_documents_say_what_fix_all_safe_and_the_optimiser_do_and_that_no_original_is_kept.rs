//! The documents say what P24 built and no longer offer what it removed: Fix all safe, the optimiser and the repair are present in
//! the parity table, and nothing names a restore of an original or the folders that kept one.

use std::path::Path;

fn doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn nothing_offers_a_restore_of_an_original_or_names_the_folders_that_kept_one() {
    for rel in [
        "guides/Workbench_Guide.md",
        "direction/Workbench_UX_Design.md",
        "direction/Wardrobe_Parity.md",
        "reference/Crate_Reference.md",
    ] {
        let text = doc(rel);
        for stale in [
            "Restore original",
            ".openrage/originals",
            ".openrage/rewritten",
        ] {
            assert!(!text.contains(stale), "{rel} still says `{stale}`");
        }
    }
}

/// The optimiser resizes with `MipFilter::Lanczos3` whichever way the size goes, and the box makes only the mips under it.
#[test]
fn the_texture_document_says_every_change_of_size_is_resampled_by_lanczos_and_the_box_makes_the_mips(
) {
    let text = doc("features/BCn_Encoding_And_Mipmaps.md");
    let section = text
        .split("## Optimising a textures source")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("the section on optimising a textures source");
    assert!(
        !section.contains("the box where it shrinks"),
        "a shrink is resampled by Lanczos too: {section}"
    );
    assert!(
        section.contains("whether a side grows or shrinks") && section.contains("box filter"),
        "{section}"
    );
}

#[test]
fn the_guide_and_the_ux_design_say_a_rule_the_project_switched_off_or_waived_keeps_a_texture() {
    for rel in [
        "guides/Workbench_Guide.md",
        "direction/Workbench_UX_Design.md",
    ] {
        let text = doc(rel);
        assert!(
            text.contains("a rule the project switched off or waived is about"),
            "{rel} does not say what holds the Textures step back"
        );
    }
}

#[test]
fn the_parity_table_has_the_fixes_the_optimiser_and_the_repair_as_present() {
    let parity = doc("direction/Wardrobe_Parity.md");
    let row = |start: &str| {
        parity
            .lines()
            .find(|line| line.starts_with(start))
            .unwrap_or_else(|| panic!("no row starts `{start}`"))
            .to_string()
    };
    for start in [
        "| Optimizer (format, mips, power of two, resize)",
        "| One-click fixes and a batch pipeline",
    ] {
        let line = row(start);
        assert!(line.contains("| Present |"), "{line}");
        assert!(!line.contains("Planned"), "{line}");
    }
    assert!(row("| One-click fixes").contains("Fix all safe"));
    let ux = doc("direction/Workbench_UX_Design.md");
    for said in ["Fix all safe", "Optimise textures", "Repair model", "FXAP"] {
        assert!(ux.contains(said), "the UX design does not say `{said}`");
    }
}
