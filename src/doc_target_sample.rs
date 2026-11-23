//! The specification's engine-named target sample, held line for line against the two examples
//! that build it and the packages that owe the rest.

use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel} is named by a document and by this test: {e}"))
}

/// The twin's lines no example builds yet, each with the package that builds it.
const NOT_YET: &[(&str, &str)] = &[
    (r#"let frag = GtaFragType::open("police.yft")?;"#, "P43"),
    (r#"Asset::Fragment(f) => println!("{}", f.name),"#, "P43"),
    (
        r#"Asset::Clips(c) => println!("{} clips", c.entries.len()),"#,
        "P21",
    ),
    (
        r#"let mut car: GtaFragType = gta.open("police.yft")?;"#,
        "P43",
    ),
    (r#"gta.save_to_mod(&car, "mods/police.yft")?;"#, "P43"),
    (r#"let mut map = CMapData::open("hills.ymap")?;"#, "P53"),
    (
        r#"for e in &mut map.entities { if e.archetype_name == "prop_bench_01a" { e.position.z += 1.0; } }"#,
        "P53",
    ),
    ("map.save()?;", "P53"),
];

/// The examples that build the twin: the file half in `or-io`, the install half in `or-title`.
const TARGET_SAMPLES: &[&str] = &[
    "crates/io/or-io/examples/the_target/sample.rs",
    "crates/title/or-title/examples/install/sample.rs",
];

/// The code lines of the twin block that follows `marker`; a line of only
/// braces and punctuation states nothing and is left out.
fn target_twin(spec: &str, marker: &str) -> Vec<String> {
    let at = spec
        .find(marker)
        .unwrap_or_else(|| panic!("the specification has no paragraph starting {marker:?}"));
    let mut lines = spec[at..].lines();
    lines
        .by_ref()
        .find(|line| line.trim() == "```rust")
        .expect("the twin paragraph is followed by a ```rust block");
    lines
        .take_while(|line| line.trim() != "```")
        .map(str::trim)
        .filter(|line| line.chars().any(char::is_alphanumeric))
        .map(str::to_string)
        .collect()
}

/// Code with its whitespace and its trailing commas gone, so a line rustfmt
/// splits still reads as the line it was.
fn squeezed(code: &str) -> String {
    let dense: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    dense
        .replace(",)", ")")
        .replace(",]", "]")
        .replace(",}", "}")
}

/// Each twin line in neither the samples nor `owed`, or in both, and each owed line the twin lacks.
fn target_disagreements(twin: &[String], samples: &str, owed: &[(&str, &str)]) -> Vec<String> {
    let built = squeezed(
        &samples
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut out = Vec::new();
    for line in twin {
        let owner = owed.iter().find(|(l, _)| l == line).map(|(_, p)| *p);
        match (built.contains(squeezed(line).trim_end_matches(',')), owner) {
            (false, None) => out.push(format!("`{line}` is in no example and owed by no package")),
            (true, Some(p)) => out.push(format!("`{line}` is built and still owed by {p}")),
            _ => {}
        }
    }
    for (i, (line, package)) in owed.iter().enumerate() {
        if !twin.iter().any(|l| l == line) {
            out.push(format!(
                "`{line}` is owed by {package} and is not a line of the twin"
            ));
        }
        if owed[..i].iter().any(|(l, _)| l == line) {
            out.push(format!("`{line}` is owed twice"));
        }
        let id = package.strip_prefix('P').unwrap_or("");
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            out.push(format!(
                "`{line}` names {package:?}, which is not a package id"
            ));
        }
    }
    out
}

/// Every line of the specification's engine-named target is built by an example or owed by a named package, never both.
#[test]
fn every_line_of_the_target_is_built_or_owed() {
    let twin = target_twin(
        &read("docs/direction/OR_IO_Specification.md"),
        "The sample in the engine's names",
    );
    assert!(
        twin.len() > 20,
        "the twin block read {} line(s), which is the search failing rather than the sample",
        twin.len()
    );
    let samples: String = TARGET_SAMPLES.iter().map(|rel| read(rel)).collect();
    let wrong = target_disagreements(&twin, &samples, NOT_YET);
    assert!(
        wrong.is_empty(),
        "the target twin in docs/direction/OR_IO_Specification.md, the examples and NOT_YET disagree:\n  {}\n\n\
         A package that builds a line moves it into its example and removes it from NOT_YET in \
         the same change; a line that leaves an example goes back on NOT_YET with its package.",
        wrong.join("\n  ")
    );
}

#[test]
fn the_target_check_can_tell_its_shapes_apart() {
    let twin: Vec<String> = [
        r#"let a = open("a")?;"#,
        "a.save()?;",
        "let b = b();",
        "let c = c();",
        "_ => go(),",
    ]
    .map(str::to_string)
    .to_vec();
    let samples = "fn run() {\n    let a =\n        open(\"a\",)?;\n    // let b = b();\n    let c = c();\n    match c {\n        _ => go(),\n    }\n}\n";
    assert_eq!(
        target_disagreements(
            &twin,
            samples,
            &[
                ("let c = c();", "P1"),
                ("gone();", "P2"),
                ("let c = c();", "Q3")
            ]
        ),
        vec![
            "`a.save()?;` is in no example and owed by no package",
            "`let b = b();` is in no example and owed by no package",
            "`let c = c();` is built and still owed by P1",
            "`gone();` is owed by P2 and is not a line of the twin",
            "`let c = c();` is owed twice",
            "`let c = c();` names \"Q3\", which is not a package id",
        ],
        "a line rustfmt split and a last match arm are built and a commented line is not; a line \
         in both, an owed line the twin lacks, a line owed twice and a malformed package id are \
         each named"
    );
}
