//! The front door's sentences: a count with its plural, and a sentence another crate wrote as a person reads it.

/// "1 file" and "2 files", in the language in use.
///
/// ⚠ A noun this does not name is said in English: every noun a call passes as a literal is an arm here, which
/// `every_noun_a_count_is_said_of_has_its_own_words` holds.
pub fn counted(n: usize, noun: &str) -> String {
    in_words(n, noun).unwrap_or_else(|| {
        if n == 1 {
            format!("1 {noun}")
        } else {
            format!("{n} {noun}s")
        }
    })
}

fn in_words(n: usize, noun: &str) -> Option<String> {
    Some(match noun {
        "file" => tr_n!("{count} file", "{count} files", n),
        "garment" => tr_n!("{count} garment", "{count} garments", n),
        "weapon" => tr_n!("{count} weapon", "{count} weapons", n),
        "effect" => tr_n!("{count} effect", "{count} effects", n),
        "particle file" => tr_n!("{count} particle file", "{count} particle files", n),
        "map set" => tr_n!("{count} map set", "{count} map sets", n),
        "entity" => tr_n!("{count} entity", "{count} entities", n),
        "archetype" => tr_n!("{count} archetype", "{count} archetypes", n),
        "room" => tr_n!("{count} room", "{count} rooms", n),
        "portal" => tr_n!("{count} portal", "{count} portals", n),
        "component" => tr_n!("{count} component", "{count} components", n),
        "texture" => tr_n!("{count} texture", "{count} textures", n),
        "warning" => tr_n!("{count} warning", "{count} warnings", n),
        "error" => tr_n!("{count} error", "{count} errors", n),
        "problem" => tr_n!("{count} problem", "{count} problems", n),
        "asset" => tr_n!("{count} asset", "{count} assets", n),
        "value" => tr_n!("{count} value", "{count} values", n),
        "tag" => tr_n!("{count} tag", "{count} tags", n),
        "source" => tr_n!("{count} source", "{count} sources", n),
        "resource" => tr_n!("{count} resource", "{count} resources", n),
        "place" => tr_n!("{count} place", "{count} places", n),
        "picture" => tr_n!("{count} picture", "{count} pictures", n),
        "ped" => tr_n!("{count} ped", "{count} peds", n),
        "pack check" => tr_n!("{count} pack check", "{count} pack checks", n),
        "id" => tr_n!("{count} id", "{count} ids", n),
        "game name" => tr_n!("{count} game name", "{count} game names", n),
        "duplicate" => tr_n!("{count} duplicate", "{count} duplicates", n),
        "drawable" => tr_n!("{count} drawable", "{count} drawables", n),
        "document" => tr_n!("{count} document", "{count} documents", n),
        "column" => tr_n!("{count} column", "{count} columns", n),
        "cloth file" => tr_n!("{count} cloth file", "{count} cloth files", n),
        "build" => tr_n!("{count} build", "{count} builds", n),
        "source folder" => tr_n!("{count} source folder", "{count} source folders", n),
        "item" => tr_n!("{count} item", "{count} items", n),
        "other element" => tr_n!("{count} other element", "{count} other elements", n),
        "alternate model" => tr_n!("{count} alternate model", "{count} alternate models", n),
        _ => return None,
    })
}

/// What the screens call a kind of content or of build, in the language in use: the crate that defines them says them in English.
pub trait KindWords {
    fn words(&self) -> String;
}

impl KindWords for or_project::OutputKind {
    fn words(&self) -> String {
        match self {
            Self::FivemResource => tr!("FiveM resource").to_string(),
            Self::SingleplayerDlc => tr!("singleplayer DLC").to_string(),
            Self::OivPackage => tr!("OIV package").to_string(),
            Self::Unknown(word) => tr!("{word} (a newer version's)", word = word),
        }
    }
}

impl KindWords for or_project::ModuleKind {
    fn words(&self) -> String {
        match self {
            Self::Files => tr!("Files").to_string(),
            Self::Clothing => tr!("Clothing").to_string(),
            Self::AddonPed => tr!("Addon ped").to_string(),
            Self::Weapons => tr!("Weapons").to_string(),
            Self::Particles => tr!("Particles").to_string(),
            Self::Maps => tr!("Maps").to_string(),
            Self::Unknown(word) => tr!("{word} (a newer version's)", word = word),
        }
    }
}

/// `text` as a person reads it: "3 file(s)" is "3 files", "1 file(s)" is "1 file", and a backtick, which marks code in a document, is dropped.
pub fn spoken(text: &str) -> String {
    let text = text.replace('`', "");
    let mut out = String::with_capacity(text.len());
    let mut previous = "";
    for (ix, word) in text.split(' ').enumerate() {
        if ix > 0 {
            out.push(' ');
        }
        if word.contains("(s)") {
            out.push_str(&word.replace("(s)", if previous == "1" { "" } else { "s" }));
        } else {
            out.push_str(word);
        }
        previous = word;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ Read from the source: a noun a call passes that has no arm is said in English in every language.
    #[test]
    fn every_noun_a_count_is_said_of_has_its_own_words() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let call = regex::Regex::new(r#"counted\([^;"]*?, *"([a-z ]+)"\)"#).expect("a pattern");
        let mut lacking = std::collections::BTreeSet::new();
        for file in crate::i18n::used::rust_files(&src) {
            let text = std::fs::read_to_string(&file).expect("a file reads");
            for found in call.captures_iter(&text) {
                if in_words(2, &found[1]).is_none() {
                    lacking.insert(format!("{} ({})", &found[1], file.display()));
                }
            }
        }
        assert!(
            lacking.is_empty(),
            "nouns with no words of their own: {lacking:?}"
        );
    }

    #[test]
    fn a_count_takes_its_plural() {
        assert_eq!(counted(0, "file"), "0 files");
        assert_eq!(counted(1, "file"), "1 file");
        assert_eq!(counted(2, "file"), "2 files");
    }

    #[test]
    fn a_sentence_from_another_crate_is_read_with_real_plurals_and_no_backticks() {
        assert_eq!(
            spoken("12 model(s) and 1 texture(s) of the pack `uniforms`, 0 place(s) held"),
            "12 models and 1 texture of the pack uniforms, 0 places held"
        );
        assert_eq!(
            spoken("3 ped(s), 1 file(s): a.rpf"),
            "3 peds, 1 file: a.rpf"
        );
        assert_eq!(spoken("2 file(s) of a pack"), "2 files of a pack");
        assert_eq!(spoken("nothing to count"), "nothing to count");
    }
}
