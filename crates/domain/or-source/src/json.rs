//! The JSON every file in a source folder is written as: keys sorted, two-space indent, LF and one final newline.

use serde::Serialize;

/// `value` as canonical JSON text.
///
/// ⚠ The keys come out sorted because `serde_json` keeps an object in a `BTreeMap`; a
/// build that turns on its `preserve_order` feature writes struct order instead, and
/// the canonical-text tests of `asset` fail.
pub fn canonical(value: &impl Serialize) -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(&serde_json::to_value(value)?)?;
    text.push('\n');
    Ok(text)
}
