//! Several files written together: what a guest hands `write_files`, and what the host checks before it keeps any of it.
//!
//! The host keeps the whole set or none of it. It never writes a file itself: it checks the set,
//! holds it until the run ends, and gives it to the embedder, which applies it as one step of Undo
//! (`crates/domain/or-plugin/tests/several_files_are_written_together_or_not_at_all.rs`). The same
//! shape is a suggested fix, which waits for a person instead of for the end of the run.
//!
//! ⚠ A path is the project's, relative and with `/`, and the guest has no other: the host refuses a
//! path that leaves the project, a drive or a rooted path, an empty or dotted component, and two
//! paths that are one on Windows.

use std::collections::BTreeSet;

use crate::abi::{CodecError, Status};
use crate::host::SaveCheck;
use crate::surface::{In, Out, MAX_NAME};

/// The most files one call keeps.
pub const MAX_WRITTEN_FILES: usize = 64;
/// The most bytes of one file.
pub const MAX_WRITTEN_FILE: usize = 32 << 20;
/// The most bytes of all the files of one call.
pub const MAX_WRITTEN: usize = 64 << 20;

/// One file a plugin writes, by its path in the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWrite {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Files that are written together or not at all.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub files: Vec<FileWrite>,
}

impl Transaction {
    /// The bytes a guest passes: `u32 n`, then for each file its path as a string and its bytes as `u32 len, u8[len]`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        out.u32(self.files.len() as u32);
        for file in &self.files {
            out.str(&file.path);
            out.u32(file.bytes.len() as u32);
            out.0.extend_from_slice(&file.bytes);
        }
        out.0
    }

    /// The files in `bytes`; nothing is reserved from a count, so each file costs the bytes that are there.
    ///
    /// ⚠ The limits are [`Self::problem`]'s: bytes that are not this encoding are the guest's bug, and a set that is too large is a refusal.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut input = In(bytes);
        let count = input.u32()? as usize;
        let mut files = Vec::new();
        for _ in 0..count {
            let path = input.str(MAX_NAME)?;
            let len = input.u32()? as usize;
            files.push(FileWrite {
                path,
                bytes: input.take(len)?.to_vec(),
            });
        }
        input.finish()?;
        Ok(Self { files })
    }

    /// The bytes of all the files.
    pub fn bytes(&self) -> usize {
        self.files.iter().map(|f| f.bytes.len()).sum()
    }

    /// The paths, in the order they were written.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|f| f.path.as_str())
    }

    /// One sentence for the first thing wrong with the set as data, none when it is a set the host would hand on.
    pub fn problem(&self) -> Option<String> {
        if self.files.is_empty() {
            return Some("a set of files holds none, so there is nothing to write".to_string());
        }
        if self.files.len() > MAX_WRITTEN_FILES {
            return Some(format!(
                "{} files are too many, and one call keeps at most {MAX_WRITTEN_FILES}",
                self.files.len()
            ));
        }
        if let Some(big) = self.files.iter().find(|f| f.bytes.len() > MAX_WRITTEN_FILE) {
            return Some(format!(
                "`{}` is {} bytes, and one file may be at most {MAX_WRITTEN_FILE}",
                big.path,
                big.bytes.len()
            ));
        }
        if self.bytes() > MAX_WRITTEN {
            return Some(format!(
                "the files hold {} bytes, and one call keeps at most {MAX_WRITTEN}",
                self.bytes()
            ));
        }
        let mut seen = BTreeSet::new();
        for file in &self.files {
            if let Some(why) = path_problem(&file.path) {
                return Some(format!("`{}` {why}", file.path));
            }
            if !seen.insert(file.path.to_lowercase()) {
                return Some(format!(
                    "`{}` is named twice, or twice in letters that differ only in case",
                    file.path
                ));
            }
        }
        None
    }
}

/// Why `path` is not a place in the project, none when it is.
fn path_problem(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return Some("is an empty path");
    }
    if path
        .chars()
        .any(|c| c.is_control() || matches!(c, '\\' | ':'))
    {
        return Some("holds a character a path in the project cannot have (a backslash, a colon or a control character)");
    }
    if path.starts_with('/') {
        return Some("is rooted, and a path is inside the project");
    }
    for part in path.split('/') {
        if part.is_empty() {
            return Some("has an empty part");
        }
        if part == "." || part == ".." {
            return Some("leaves the project, or says where it already is");
        }
        if part.ends_with(' ') || part.ends_with('.') {
            return Some("has a part that ends in a space or a dot, which Windows does not keep");
        }
    }
    None
}

/// What a guest handed `write_files` comes to: the set the embedder's check accepts, or the status and the sentence that refuse it.
///
/// ⚠ Malformed bytes are the guest's bug (`abi.bad_argument`); a set the host or the embedder will
/// not keep is a refusal (`abi.refused`) whose sentence says that none of the files was written.
pub(crate) fn admit(bytes: &[u8], check: &dyn SaveCheck) -> Result<Transaction, (Status, String)> {
    let set = Transaction::decode(bytes).map_err(|e| (e.status(), e.to_string()))?;
    if let Some(why) = set.problem() {
        return Err((Status::Refused, format!("no file was written: {why}")));
    }
    let files: Vec<(&str, &[u8])> = set
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.bytes.as_slice()))
        .collect();
    match check.why_the_files_would_be_invalid(&files) {
        Some(why) => Err((Status::Refused, format!("no file was written: {why}"))),
        None => Ok(set),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &str, bytes: &[u8]) -> FileWrite {
        FileWrite {
            path: path.into(),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn a_set_survives_the_wire() {
        let set = Transaction {
            files: vec![write("a/b.txt", b"one"), write("c.json", b"")],
        };
        assert_eq!(Transaction::decode(&set.encode()), Ok(set));
    }

    #[test]
    fn a_declared_count_does_not_allocate_before_the_bytes_are_there() {
        let lie = u32::MAX.to_le_bytes();
        assert!(matches!(
            Transaction::decode(&lie),
            Err(CodecError::Truncated { .. })
        ));
        let one_file_too_many = Transaction {
            files: (0..=MAX_WRITTEN_FILES)
                .map(|n| write(&format!("f{n}.txt"), b""))
                .collect(),
        };
        let decoded = Transaction::decode(&one_file_too_many.encode()).expect("it is the encoding");
        assert!(decoded.problem().unwrap().contains("too many"));
    }

    #[test]
    fn a_path_that_leaves_the_project_or_is_not_one_on_windows_is_refused() {
        for bad in [
            "", "/etc/x", "../x", "a/../x", "a//b", "./x", "C:/x", "a\\b", "a/b ", "a/b.", "a/\0",
        ] {
            let set = Transaction {
                files: vec![write(bad, b"x")],
            };
            assert!(set.problem().is_some(), "{bad:?} was kept");
        }
        let fine = Transaction {
            files: vec![write("clothing/wardrobe.json", b"{}")],
        };
        assert_eq!(fine.problem(), None);
    }

    #[test]
    fn two_paths_that_are_one_on_windows_are_refused() {
        let set = Transaction {
            files: vec![write("a/B.txt", b"1"), write("A/b.TXT", b"2")],
        };
        assert!(set.problem().unwrap().contains("twice"));
    }
}
