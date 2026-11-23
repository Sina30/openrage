use std::path::{Path, PathBuf};

use super::device::FileDevice;

#[derive(Debug, Clone)]
pub struct FolderDevice {
    root: PathBuf,
}

impl FolderDevice {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, relative_path: &str) -> Option<PathBuf> {
        let trimmed = relative_path.trim_start_matches('/').replace('\\', "/");
        if trimmed.is_empty() {
            return Some(self.root.clone());
        }
        let mut current = self.root.clone();
        for segment in trimmed.split('/') {
            if segment.is_empty() {
                continue;
            }
            let mut hit: Option<PathBuf> = None;
            let exact = current.join(segment);
            if exact.exists() {
                hit = Some(exact);
            } else if let Ok(entries) = std::fs::read_dir(&current) {
                let want = segment.to_ascii_lowercase();
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().to_ascii_lowercase() == want {
                        hit = Some(entry.path());
                        break;
                    }
                }
            }
            current = hit?;
        }
        Some(current)
    }
}

impl FileDevice for FolderDevice {
    fn open(&self, relative_path: &str) -> Option<Vec<u8>> {
        let path = self.resolve(relative_path)?;
        if !path.is_file() {
            return None;
        }
        std::fs::read(path).ok()
    }

    fn exists(&self, relative_path: &str) -> bool {
        self.resolve(relative_path)
            .map(|p| p.is_file())
            .unwrap_or(false)
    }

    fn locate(&self, relative_path: &str) -> Option<String> {
        let path = self.resolve(relative_path)?;
        if path.is_file() {
            Some(path.to_string_lossy().replace('\\', "/"))
        } else {
            None
        }
    }

    fn list(&self, relative_dir: &str) -> Vec<String> {
        let Some(path) = self.resolve(relative_dir) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }

    fn walk(&self) -> Option<Vec<String>> {
        let mut out = Vec::new();
        walk_dir(&self.root, "", 0, &mut out);
        Some(out)
    }
}

/// How deep a folder walk will go before it stops.
///
/// ⚠ **This is the symlink-cycle guard.** A visited set would have to
/// canonicalise every path, and `std::fs::canonicalize` on Windows resolves
/// junctions and reparse points differently per filesystem, so it would be
/// right on one machine and silently wrong on another; a cap bounds the work
/// the same way everywhere. A real RAGE install is four or five levels deep.
const MAX_FOLDER_DEPTH: u32 = 32;

fn walk_dir(dir: &Path, prefix: &str, depth: u32, out: &mut Vec<String>) {
    if depth >= MAX_FOLDER_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let here = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk_dir(&entry.path(), &here, depth + 1, out),
            // `file_type` does not traverse a symlink, so one is never
            // followed: following one to a directory is what makes a cycle
            // reachable at all, and nothing in a RAGE install needs it.
            Ok(kind) if kind.is_file() => out.push(here),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A folder removed when the first of the pair drops, so hold it as long as the second is used.
    fn make_temp_dir(name: &str) -> (tempfile::TempDir, PathBuf) {
        let scratch = tempfile::Builder::new()
            .prefix(&format!("or-title-vfs-fd-{name}-"))
            .tempdir()
            .unwrap();
        let dir = scratch.path().to_path_buf();
        (scratch, dir)
    }

    fn write(path: &Path, body: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(body).unwrap();
    }

    #[test]
    fn folder_device_resolves_files_case_insensitive() {
        let (_scratch, root) = make_temp_dir("case");
        write(&root.join("data/effects/ptxclipregions.dat"), b"hello");
        let dev = FolderDevice::new(&root);
        let exact = dev.open("data/effects/ptxclipregions.dat").unwrap();
        assert_eq!(exact, b"hello");
        let upper = dev.open("DATA/Effects/PTXCLIPREGIONS.DAT");
        assert_eq!(upper.as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn folder_device_missing_file_returns_none() {
        let (_scratch, root) = make_temp_dir("missing");
        let dev = FolderDevice::new(&root);
        assert!(dev.open("nope.dat").is_none());
        assert!(!dev.exists("nope.dat"));
    }

    #[test]
    fn folder_device_lists_directory_children() {
        let (_scratch, root) = make_temp_dir("list");
        write(&root.join("a.txt"), b"a");
        write(&root.join("b.txt"), b"b");
        write(&root.join("sub/c.txt"), b"c");
        let dev = FolderDevice::new(&root);
        let mut entries = dev.list("");
        entries.sort();
        assert_eq!(entries, vec!["a.txt", "b.txt", "sub"]);
        let sub = dev.list("sub");
        assert_eq!(sub, vec!["c.txt"]);
    }
}
