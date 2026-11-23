//! What a project file says that only the machine it was written on can use: the lines of Check before sharing that concern the file.

use serde_json::Value;

/// A string of a project file that names a place only this machine has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachinePath {
    /// Where in the file, as `modules[1].path`.
    pub at: String,
    /// The path as the file states it.
    pub path: String,
    pub why: Why,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A drive, a share or a root: a project folder copied elsewhere does not have it.
    Absolute,
    /// Relative, and above the project folder: it breaks when the folder is zipped without its neighbours.
    LeavesTheFolder,
}

/// Every path of the project file `text` that another machine, or the folder alone, cannot reach.
///
/// ⚠ Every string of the file is read, not a list of keys, so a key a later build adds is read too. A
/// string is a path when it looks like one on any host: a file written on Windows is shared with Linux.
pub fn machine_paths(text: &[u8]) -> Result<Vec<MachinePath>, String> {
    let text = text.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(text);
    let value: Value = serde_json::from_slice(text).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("a project file is a JSON object".into());
    }
    let mut found = Vec::new();
    walk(&value, String::new(), &mut found);
    Ok(found)
}

fn walk(value: &Value, at: String, found: &mut Vec<MachinePath>) {
    match value {
        Value::String(path) => {
            if let Some(why) = why(path) {
                found.push(MachinePath {
                    at,
                    path: path.clone(),
                    why,
                });
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, format!("{at}[{index}]"), found);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                let at = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                walk(item, at, found);
            }
        }
        _ => {}
    }
}

fn why(text: &str) -> Option<Why> {
    let bytes = text.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    // A root is a path only when it has no space in it: a sentence that begins with a slash is not one.
    let root = text.len() > 1 && text.starts_with('/') && !text.contains(char::is_whitespace);
    if drive || root || text.starts_with("\\\\") {
        return Some(Why::Absolute);
    }
    let climbs = text == ".." || text.starts_with("../") || text.starts_with("..\\");
    climbs.then_some(Why::LeavesTheFolder)
}
