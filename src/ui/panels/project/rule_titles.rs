//! The short name of each rule a reason can set aside, which the Rules window shows in bold above the rule's sentence.
//!
//! ⚠ A title says what is wrong in a few words and never carries the rule's code or a number: the sentence under it says why it matters, and a limit is edited in the row.

/// The title of the rule `code`, or none where this table has no row for it.
pub(super) fn title_of(code: &str) -> Option<&'static str> {
    Some(match code {
        "slot-cap" => tr!("Slot over its collection cap"),
        "duplicate-content" => tr!("Garment that copies another"),
        "renumbered" => tr!("Garment moved since the last build"),
        "replace-mode" => tr!("Files replace the game's own"),
        "replace-unpinned" => tr!("Replacing garment not pinned"),
        "model-unreadable" => tr!("Model cannot be read"),
        "texture-unreadable" => tr!("Texture cannot be read"),
        "texture-count-wraps" => tr!("Too many textures in a slot"),
        "cloth-missing" => tr!("Cloth missing"),
        "cloth-ignored" => tr!("Cloth the game ignores"),
        "cloth-unreadable" => tr!("Cloth cannot be read"),
        "shop-id-clash" => tr!("Shop id used twice"),
        "shop-entry-unplaced" => tr!("Shop entries with no garment"),
        "texture-size" => tr!("Texture too large"),
        "texture-skin-tone-size" => tr!("Skin-tone texture not at its size"),
        "texture-not-power-of-two" => tr!("Texture not a power of two"),
        "texture-mips" => tr!("Texture without mipmaps"),
        "texture-format" => tr!("Texture in an unusual format"),
        "texture-bc7-legacy" => tr!("BC7 texture in a Legacy pack"),
        "dictionary-size" => tr!("Texture dictionary too heavy"),
        "lod-missing" => tr!("Model without lower levels of detail"),
        "lod-medium-heavy" => tr!("Medium level of detail too heavy"),
        "lod-low-heavy" => tr!("Low level of detail too heavy"),
        "polycount-high" => tr!("Model with too many triangles"),
        "diffuse-missing" => tr!("Material without a diffuse texture"),
        "embedded-unused" => tr!("Unused texture inside a model"),
        "material-parameters" => tr!("Material missing constants"),
        "reserved" => tr!("Empty place held"),
        _ => return None,
    })
}

/// What the Rules window calls a rule: its title, else its sentence as the engine states it.
pub(super) fn title_or_sentence(rule: &or_wardrobe::Rule) -> String {
    title_of(rule.code).map_or_else(|| rule.sentence(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ Falsified by a rule added to the table with no row here: the window would show its whole sentence in bold.
    #[test]
    fn every_rule_a_reason_can_set_aside_has_a_short_title_that_is_not_its_sentence_or_its_code() {
        let mut seen = std::collections::BTreeSet::new();
        for rule in or_wardrobe::RULES.iter().filter(|r| r.waivable) {
            let title = title_of(rule.code)
                .unwrap_or_else(|| panic!("the rule {} has no title", rule.code));
            assert!(
                title.chars().count() <= 40 && !title.contains(rule.code) && !title.contains('_'),
                "{title}"
            );
            assert_ne!(title, rule.sentence());
            assert!(seen.insert(title), "two rules are titled {title}");
        }
    }
}
