//! The words a user reads: what a file is and does rather than the engine's
//! internals, an archive called an archive, one ellipsis and one menu
//! separator, and OpenRage by its own name.
//!
//! Every string literal outside test code in the app counts, log text
//! included, because the Log panel shows it to a user too.

use crate::edition_strings::user_facing_literals;

/// The resource container, the metadata schema source, an archive's table of
/// contents and the shader language the viewport compiles to.
const INTERNALS: &[&str] = &["RSC7", "PSC", "TOC", "WGSL"];

/// The places a menu path starts from or passes through.
const MENU_WORDS: &[&str] = &[
    "File", "Edit", "View", "Automate", "Help", "Project", "Settings", "New",
];

/// Each literal in the app that `offends`, as `file:line: text`.
fn offenders(offends: impl Fn(&str) -> bool) -> Vec<String> {
    let (scanned, literals) = user_facing_literals(&["src"]);
    // A walk or a lexer that stopped working finds nothing and passes, so both are held to a floor.
    assert!(
        scanned > 200 && literals.len() > 2000,
        "only {scanned} files and {} literals were read",
        literals.len()
    );
    literals
        .iter()
        .filter(|(_, _, text)| offends(text))
        .map(|(file, line, text)| format!("{file}:{line}: {text}"))
        .collect()
}

/// Whether `text` holds `word` with no letter, digit or underscore either side.
fn says(text: &str, word: &str) -> bool {
    let part_of_a_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(part_of_a_word)
            && !text[at + word.len()..]
                .chars()
                .next()
                .is_some_and(part_of_a_word)
    })
}

#[test]
fn no_default_view_string_names_rsc7_psc_toc_or_wgsl() {
    let found = offenders(|text| INTERNALS.iter().any(|word| says(text, word)));
    assert!(
        found.is_empty(),
        "a user reads what a file is and what OpenRage does with it, never {INTERNALS:?}:\n{}",
        found.join("\n")
    );
}

#[test]
fn an_archive_is_called_an_archive() {
    let found = offenders(|text| text.to_ascii_lowercase().contains("pack file"));
    assert!(
        found.is_empty(),
        "an .rpf is an archive:\n{}",
        found.join("\n")
    );
}

#[test]
fn an_ellipsis_is_one_character() {
    let dots = offenders(|text| text.contains("..."));
    assert!(dots.is_empty(), "an ellipsis is `…`:\n{}", dots.join("\n"));
}

/// Whether `text` writes a step of a menu path with anything but `▸`.
fn takes_another_separator(text: &str) -> bool {
    MENU_WORDS.iter().any(|word| {
        ["→", "->", ">"].iter().any(|arrow| {
            text.contains(&format!("{word} {arrow}")) || text.contains(&format!("{arrow} {word}"))
        })
    })
}

#[test]
fn a_menu_path_has_one_separator() {
    let arrows = offenders(takes_another_separator);
    assert!(
        arrows.is_empty(),
        "a menu path is written `File ▸ Open`:\n{}",
        arrows.join("\n")
    );
}

#[test]
fn the_separator_check_finds_the_paths_it_replaced() {
    for old in [
        "Open a .ycd file via File → Open (Ctrl+O)",
        "Open Settings -> Keys to extract them first.",
        "Configure a GTA install (Settings > Select gta5.exe)",
        "right-click here → New ▸ …",
    ] {
        assert!(takes_another_separator(old), "{old}");
    }
    assert!(!takes_another_separator(
        "No document open — File ▸ Open (Ctrl+O)"
    ));
    assert!(!takes_another_separator(
        "→ {target}, {duration} {}, when {when}"
    ));
}

#[test]
fn openrage_goes_by_its_own_name() {
    let found = offenders(|text| {
        text.to_ascii_lowercase()
            .contains("particle effects editor")
    });
    assert!(
        found.is_empty(),
        "OpenRage edits every file GTA V ships, and calls itself OpenRage:\n{}",
        found.join("\n")
    );
}

/// Whether `text` spells a word as a command line's flag: `--` and a letter, after something that is not a dash.
fn names_a_flag(text: &str) -> bool {
    text.match_indices("--").any(|(at, _)| {
        !text[..at].ends_with('-')
            && (text[at + 2..].chars().next()).is_some_and(|c| c.is_ascii_alphabetic())
    })
}

#[test]
fn no_question_the_app_asks_names_a_command_line_flag() {
    let (scanned, literals) = user_facing_literals(&["src/ui"]);
    assert!(
        scanned > 50 && literals.len() > 1000,
        "only {scanned} files and {} literals were read",
        literals.len()
    );
    let found: Vec<String> = (literals.iter())
        // ⚠ Not the command line a command states for Copy as command: that is the line itself and no question.
        .filter(|(_, _, text)| names_a_flag(text) && !text.starts_with("openrage-cli "))
        .map(|(file, line, text)| format!("{file}:{line}: {text}"))
        .collect();
    assert!(
        found.is_empty(),
        "a question is asked in words, never by the command's flags:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_flag_check_finds_the_flags_the_questions_named() {
    for old in [
        "Every level of detail (--lods all)",
        "Its own textures, as PNG (off: --no-textures)",
        "Fill a channel the mesh leaves out (--fill-missing)",
        "Keep the model's own skinning (--keep-skinning)",
    ] {
        assert!(names_a_flag(old), "{old}");
    }
    assert!(!names_a_flag("Every level of detail"));
    assert!(!names_a_flag("a pause -- or a rule ---"));
}

#[test]
fn a_word_is_found_only_where_it_stands_alone() {
    assert!(says("decrypt the TOC.", "TOC"));
    assert!(says("TOC", "TOC"));
    assert!(!says("TOCTOU", "TOC"));
    assert!(!says("a_TOC", "TOC"));
    assert!(!says("PSCs", "PSC"));
}

/// The labels of the import of a game file into the open project, and of a built pack into a new project of sources: the two things the app calls Import, since both decompile a game file to sources on disk.
const IMPORT_FILES: &[&str] = &[
    "src/platform/file_dialogs.rs",
    "src/state/jobs.rs",
    "src/state/workbench/imports.rs",
    "src/state/workbench/reopen.rs",
    "src/ui/commands/files.rs",
    "src/ui/commands/front_door.rs",
    "src/ui/commands/project.rs",
    "src/ui/os_drop.rs",
    "src/ui/panels/import_report.rs",
    "src/ui/panels/project/reopen_question.rs",
    "src/ui/panels/reopen_report.rs",
    "src/ui/panels/start_page.rs",
    "src/ui/panels/project/assets.rs",
    "src/ui/panels/project/picker.rs",
    "src/ui/panels/project/questions.rs",
];

/// Whether `text`, with an icon's placeholder off its front, opens with the word Import, Imported or Importing.
fn opens_with_import(text: &str) -> bool {
    let mut first = text
        .trim_start_matches("{} ")
        .split(|c: char| !c.is_alphabetic());
    matches!(first.next(), Some("Import" | "Imported" | "Importing"))
}

#[test]
fn only_a_game_file_becoming_a_source_is_called_import() {
    let (_, literals) = user_facing_literals(&["src"]);
    let found: Vec<String> = (literals.iter())
        .filter(|(file, _, text)| opens_with_import(text) && !IMPORT_FILES.contains(&file.as_str()))
        .map(|(file, line, text)| format!("{file}:{line}: {text}"))
        .collect();
    assert!(
        found.is_empty(),
        "Import means one game file becoming a source of the project; a control that replaces or copies says so:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_import_check_finds_the_labels_that_were_renamed() {
    for old in [
        "{} Import SWF",
        "Import Movie from SWF",
        "Import a movie",
        "Imported {}",
        "Import glTF over this model…",
        "Importing {} over {name}",
    ] {
        assert!(opens_with_import(old), "{old}");
    }
    for fine in [
        "Imports ({})",
        "Undo import",
        "Replace from SWF",
        "Important",
    ] {
        assert!(!opens_with_import(fine), "{fine}");
    }
}

/// What the app called a control before it stopped calling it Import, and what a document says now.
const RETIRED_LABELS: &[(&str, &str)] = &[
    (
        "Import, Embed and Unembed",
        "Replace from image…, Embed and Unembed",
    ),
    ("until Import and", "until Replace and"),
    ("its Import puts", "its Replace puts"),
    ("Import Texture…", "Replace from image…"),
    ("Import glTF over this model…", "Replace from glTF…"),
    ("Import File…", "Copy into archive…"),
    ("Import SWF", "Replace from SWF"),
    ("Import Movie from SWF", "Replace from SWF"),
    ("Take back the last change", "Undo clothes change"),
];

#[test]
fn no_document_names_a_control_by_a_label_the_app_retired() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut read = 0;
    let mut found = Vec::new();
    for path in crate::doc_citations::walked() {
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel = rel.to_string_lossy().replace('\\', "/");
        if !rel.starts_with("docs/") || !rel.ends_with(".md") || rel.starts_with("docs/audit/") {
            continue;
        }
        read += 1;
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{rel}: {e}"))
            .replace("\r\n", "\n");
        // A label wrapped onto two lines still reads as one, and a newline is one byte as a space is.
        let flat = text.replace('\n', " ");
        for (old, now) in RETIRED_LABELS {
            for (at, _) in flat.match_indices(old) {
                let number = text[..at].matches('\n').count();
                // The design's table of what each Import was is the one place the old names stand.
                let table = text
                    .lines()
                    .nth(number)
                    .unwrap_or("")
                    .starts_with("| Import ");
                if !(table && rel.ends_with("Workbench_UX_Design.md")) {
                    found.push(format!("{rel}:{}: `{old}` is now `{now}`", number + 1));
                }
            }
        }
    }
    assert!(read > 20, "only {read} documents were read");
    assert!(
        found.is_empty(),
        "a document names a control the app renamed:\n{}",
        found.join("\n")
    );
}
