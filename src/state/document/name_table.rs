//! A `.nametable` held as text, one name per line.

use super::{LineEnding, TextDocument, TextEncoding};

/// A `.nametable`'s names as text, one per line, in the file's order.
///
/// ⚠ An entry the reader skips is not kept, so a save drops it and shifts the
/// index of every name after it.
pub fn name_table_text(bytes: &[u8]) -> Result<TextDocument, String> {
    let table =
        or_io::gta5::data::nametable::parse(bytes).map_err(|e| format!("Parse error: {e}"))?;
    for skipped in &table.skipped {
        tracing::warn!(
            index = skipped.index,
            reason = %skipped.reason,
            "a .nametable entry is not a name, and a save drops it"
        );
    }
    let text: String = table.names.iter().map(|name| format!("{name}\n")).collect();
    Ok(TextDocument {
        text: ropey::Rope::from_str(&text),
        encoding: TextEncoding::Utf8,
        line_ending: LineEnding::Lf,
    })
}

/// The `.nametable` bytes of `text`'s lines, refusing by line a line that cannot be a name.
pub fn name_table_bytes(text: &TextDocument) -> Result<Vec<u8>, String> {
    let mut names = Vec::new();
    for (index, line) in text.text.lines().enumerate() {
        let line = line.to_string();
        let name = line.trim_end_matches(['\n', '\r']);
        if name.is_empty() {
            continue;
        }
        // The reader's rule: a name holds no ASCII control character.
        if let Some(bad) = name.chars().find(char::is_ascii_control) {
            return Err(format!(
                "line {}, \"{}\", cannot be a name: it holds the control character {:?}",
                index + 1,
                name.escape_debug(),
                bad
            ));
        }
        names.push(name.to_string());
    }
    Ok(or_io::gta5::data::nametable::write(&names))
}
