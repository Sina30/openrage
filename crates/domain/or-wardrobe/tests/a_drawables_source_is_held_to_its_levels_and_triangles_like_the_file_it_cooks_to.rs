//! A model that is a drawables source is held to the rules that count a model's levels of detail and triangles, as the
//! compiled file it cooks into is: the scan counts the source from its glb header and the rules fire the same.

mod common;

use std::path::Path;

use common::{garment, put_bytes, JBIB};
use or_source::game_import::{decompile, Facts};
use or_source::sample::drawable_dictionary_with_levels as drawables;
use or_wardrobe::{allocate, check, inspect, Depth, Ped, Wardrobe};

/// The four rules that read a model's levels and triangles.
const COUNTED: [&str; 4] = [
    "polycount-high",
    "lod-missing",
    "lod-medium-heavy",
    "lod-low-heavy",
];

/// The codes among `COUNTED` that the pack's one garment is found at fault with, sorted.
fn found(folder: &Path, model: &str) -> Vec<&'static str> {
    let mut w = Wardrobe::new("pack");
    w.rules.set("polycount-high", |s| s.threshold = Some(100));
    let mut g = garment(Ped::Male, JBIB, None, model, &[]);
    g.label = Some("jacket".into());
    w.garments.push(g);
    w.write(folder).unwrap();
    let findings = check(&w, &allocate(&w), &inspect(folder, &w, Depth::Measure));
    let mut codes: Vec<&'static str> = findings
        .iter()
        .map(|f| f.code)
        .filter(|c| COUNTED.contains(c))
        .collect();
    codes.sort_unstable();
    codes
}

/// `bytes` as a compiled file `models/x.ydd` in one pack and as the drawables source it decompiles to in another.
fn both(bytes: &[u8]) -> (tempfile::TempDir, tempfile::TempDir) {
    let (file, source) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_bytes(file.path(), "models/x.ydd", bytes);
    std::fs::create_dir_all(source.path().join("models")).unwrap();
    let made = decompile(
        bytes,
        "x.ydd",
        &source.path().join("models"),
        &Facts::default(),
    )
    .expect("the sample decompiles");
    assert_eq!(made.file_name().unwrap(), "x.drawables.src");
    (file, source)
}

#[test]
fn a_source_with_one_level_and_a_source_with_heavy_levels_break_the_rules_the_file_does() {
    for (name, bytes, wants) in [
        // Only a high level: the medium and the low are missing, and 600 vertices are 200 triangles, over the 100 set above.
        (
            "one level",
            drawables("x", 600, 1.0, 1),
            vec!["lod-missing", "polycount-high"],
        ),
        // Three levels of one geometry: the medium is all of the high one's vertices and the low is over 500.
        (
            "heavy levels",
            drawables("x", 600, 1.0, 3),
            vec!["lod-low-heavy", "lod-medium-heavy", "polycount-high"],
        ),
    ] {
        let (file, source) = both(&bytes);
        let of_file = found(file.path(), "models/x.ydd");
        let of_source = found(source.path(), "models/x.drawables.src");
        assert_eq!(of_file, wants, "{name}: the file");
        assert_eq!(
            of_source, of_file,
            "{name}: the source reads as the file does"
        );
    }
}

#[test]
fn a_source_under_the_triangle_limit_is_not_told_it_is_over_it() {
    // 60 vertices are 20 triangles, under the 100 set above; the one level still lacks a medium and a low.
    let (file, source) = both(&drawables("x", 60, 1.0, 1));
    assert_eq!(found(file.path(), "models/x.ydd"), ["lod-missing"]);
    assert_eq!(
        found(source.path(), "models/x.drawables.src"),
        ["lod-missing"]
    );
}
