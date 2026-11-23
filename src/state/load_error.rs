//! What a file that would not open is said to be: its name, one plain sentence and the next step, with the reader's own words kept for the details.

use std::path::Path;

/// The window's title and the status bar's line: what was being done, to which file.
pub fn headline(path: &Path) -> String {
    tr!("Could not open {jobs}", jobs = super::jobs::file_name(path))
}

/// One plain sentence for why `message`, the words of the reader that refused, ended an open, and what to do next.
///
/// ⚠ The reader's words (a hex magic number, a type of the engine) are never in it: they are the details of the window and the Log's.
pub fn plain_reason(message: &str) -> &'static str {
    if message.contains("No reader registered") {
        tr!("OpenRage cannot open this kind of file yet. Nothing was changed; choose another file.")
    } else {
        tr!("OpenRage could not read this file as the kind of game file its name says. It may be damaged, or be another kind of file with the wrong ending. Nothing was changed; choose another file.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_open_is_a_sentence_with_no_hex_and_no_library_and_names_its_file() {
        let words = plain_reason("Parse error: not a resource: Invalid RSC7 magic: expected 0x37435352, found 0xF166E20C");
        for jargon in ["RSC7", "0x", "Parse error", "magic"] {
            assert!(!words.contains(jargon), "{words}");
        }
        assert_eq!(
            headline(Path::new("C:/mods/harbor_logo.ydd")),
            "Could not open harbor_logo.ydd"
        );
    }
}
