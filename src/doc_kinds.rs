//! The two sentences that list the kinds of content the build has, held to the kinds `or-content` lists.

use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel} is named by this test: {e}"))
}

/// The text from `start` up to the first `end` after it.
fn sentence_from<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text
        .find(start)
        .unwrap_or_else(|| panic!("`{start}` is not in this document any more"));
    let rest = &text[from..];
    &rest[..rest.find(end).unwrap_or(rest.len())]
}

#[test]
fn the_project_model_names_every_kind_of_content_the_build_has() {
    let text = read("docs/direction/Project_Model_Design.md");
    let built = sentence_from(&text, "any kind of content but ", ". ");
    let mut kinds = vec![or_project::ModuleKind::Files.label()];
    kinds.extend(or_content::recognisers().map(|r| r.kind().label()));
    for kind in kinds {
        assert!(
            built.contains(&kind),
            "the kinds the build has are listed without {kind}: {built}"
        );
    }
}

#[test]
fn the_languages_guide_names_the_rules_whose_sentences_stay_english() {
    let text = read("docs/guides/Interface_Languages.md");
    let library = sentence_from(&text, "the sentences a library crate writes (", ")");
    // The Effects panel and Problems show these messages in a translated screen.
    assert!(!or_particles::RULES.is_empty());
    assert!(
        library.to_ascii_lowercase().contains("particles"),
        "the rules of the particles are English library sentences: {library}"
    );
}
