use std::collections::HashMap;
use std::sync::Arc;

use super::device::FileDevice;
use super::glob::Pattern;
pub use super::path::VfsPath;

/// What a glob found, and what it could not look in.
///
/// ⚠ A device that cannot enumerate itself answers `None` to
/// [`FileDevice::walk`]; a glob that dropped those would return a short list
/// that looks exhaustive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobResult {
    /// Full `mount:/path` strings, in mount order, newest device first.
    pub matches: Vec<String>,
    /// `mount:` names holding at least one device that could not be walked.
    pub unwalkable: Vec<String>,
}

impl GlobResult {
    pub fn is_complete(&self) -> bool {
        self.unwalkable.is_empty()
    }
}

pub struct Vfs {
    mounts: HashMap<String, Vec<Arc<dyn FileDevice>>>,
}

impl std::fmt::Debug for Vfs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Vfs");
        for (name, stack) in &self.mounts {
            s.field(name, &stack.len());
        }
        s.finish()
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs {
    pub fn new() -> Self {
        Self {
            mounts: HashMap::new(),
        }
    }

    pub fn mount(&mut self, mount_name: &str, device: Arc<dyn FileDevice>) {
        let key = canonical_mount_name(mount_name);
        self.mounts.entry(key).or_default().push(device);
    }

    pub fn has_mount(&self, mount_name: &str) -> bool {
        let key = canonical_mount_name(mount_name);
        self.mounts.contains_key(&key)
    }

    pub fn device_count(&self, mount_name: &str) -> usize {
        let key = canonical_mount_name(mount_name);
        self.mounts.get(&key).map(|v| v.len()).unwrap_or(0)
    }

    pub fn open(&self, vfs_path: &str) -> Option<Vec<u8>> {
        let parsed = VfsPath::parse(vfs_path)?;
        let stack = self.resolve_mount(parsed.mount())?;
        for device in stack.iter().rev() {
            if let Some(bytes) = device.open(parsed.rest()) {
                return Some(bytes);
            }
        }
        None
    }

    /// The bytes of a `.ybn` a reader of its header box asks `open_head` for: its 16-byte header and the start of its deflate stream, which inflates to the 0x70-byte root many times over.
    pub const BOUND_FRONT: usize = 4096;

    /// [`open`](Self::open), reading no more than the first `limit` bytes where the device holding the file can.
    pub fn open_head(&self, vfs_path: &str, limit: usize) -> Option<Vec<u8>> {
        let parsed = VfsPath::parse(vfs_path)?;
        let stack = self.resolve_mount(parsed.mount())?;
        stack
            .iter()
            .rev()
            .find_map(|device| device.open_head(parsed.rest(), limit))
    }

    pub fn locate(&self, vfs_path: &str) -> Option<String> {
        let parsed = VfsPath::parse(vfs_path)?;
        let stack = self.resolve_mount(parsed.mount())?;
        for device in stack.iter().rev() {
            if let Some(loc) = device.locate(parsed.rest()) {
                return Some(loc);
            }
        }
        None
    }

    pub fn exists(&self, vfs_path: &str) -> bool {
        let Some(parsed) = VfsPath::parse(vfs_path) else {
            return false;
        };
        let Some(stack) = self.resolve_mount(parsed.mount()) else {
            return false;
        };
        stack.iter().rev().any(|d| d.exists(parsed.rest()))
    }

    pub fn list(&self, vfs_dir: &str) -> Vec<String> {
        let Some(parsed) = VfsPath::parse(vfs_dir) else {
            return Vec::new();
        };
        let Some(stack) = self.resolve_mount(parsed.mount()) else {
            return Vec::new();
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for device in stack.iter().rev() {
            for entry in device.list(parsed.rest()) {
                let key = entry.to_ascii_lowercase();
                if seen.insert(key) {
                    out.push(entry);
                }
            }
        }
        out
    }

    /// Every path in every mount matching `pattern`, across archive boundaries.
    ///
    /// ⚠ **Always a path, never a name.** The pattern is matched against the
    /// path below the mount name with [`matches_path`](Pattern::matches_path),
    /// so `*.ydr` means *directly under the mount root*; `openrage-cli search`
    /// calls [`matches_name`](Pattern::matches_name), where it means *any
    /// `.ydr`*. Choosing by whether the pattern contains a separator would make
    /// one call mean two things depending on its argument.
    ///
    /// A path present in two devices of one mount is returned once, the newest,
    /// because that is what [`open`](Self::open) hands back.
    pub fn glob(&self, pattern: &str) -> GlobResult {
        let pattern = Pattern::new(pattern);
        let mut out = GlobResult::default();

        let mut names: Vec<&String> = self.mounts.keys().collect();
        names.sort();

        for mount in names {
            let stack = &self.mounts[mount];
            let mut seen = std::collections::BTreeSet::new();
            let mut cannot_walk = false;

            for device in stack.iter().rev() {
                let Some(paths) = device.walk() else {
                    cannot_walk = true;
                    continue;
                };
                for path in paths {
                    if !pattern.matches_path(&path) {
                        continue;
                    }
                    if seen.insert(path.to_ascii_lowercase()) {
                        out.matches.push(format!("{mount}:/{path}"));
                    }
                }
            }
            if cannot_walk {
                out.unwalkable.push(format!("{mount}:"));
            }
        }
        out
    }

    /// Drop every cached handle, so the next lookup re-reads from disk.
    pub fn invalidate(&self) {
        for stack in self.mounts.values() {
            for device in stack {
                device.invalidate();
            }
        }
    }

    fn resolve_mount(&self, mount: &str) -> Option<&Vec<Arc<dyn FileDevice>>> {
        let key = canonical_mount_name(mount);
        let mut best: Option<(&String, &Vec<Arc<dyn FileDevice>>)> = None;
        for (name, stack) in &self.mounts {
            if !key.starts_with(name.as_str()) {
                continue;
            }
            if key.len() == name.len() {
                return Some(stack);
            }
            let after = &key[name.len()..];
            if !after.starts_with('/') {
                continue;
            }
            match best {
                Some((b, _)) if b.len() >= name.len() => {}
                _ => best = Some((name, stack)),
            }
        }
        best.map(|(_, stack)| stack)
    }
}

fn canonical_mount_name(name: &str) -> String {
    name.trim_end_matches(':').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct InMemDevice {
        files: HashMap<String, Vec<u8>>,
    }

    impl InMemDevice {
        fn new(entries: &[(&str, &[u8])]) -> Self {
            let files = entries
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.to_vec()))
                .collect();
            Self { files }
        }
    }

    impl FileDevice for InMemDevice {
        fn open(&self, relative_path: &str) -> Option<Vec<u8>> {
            self.files
                .get(&relative_path.trim_start_matches('/').to_ascii_lowercase())
                .cloned()
        }
    }

    /// ⚠ A device that cannot enumerate makes the glob **incomplete**, and the
    /// result says so rather than looking short.
    #[test]
    fn a_glob_names_the_mounts_it_could_not_walk() {
        let mut vfs = Vfs::new();
        vfs.mount("common:", Arc::new(InMemDevice::new(&[("a.ydr", b"x")])));

        let found = vfs.glob("**/*.ydr");
        assert!(
            found.matches.is_empty(),
            "the in-memory device has no `walk`, so nothing can be found in it"
        );
        assert_eq!(
            found.unwalkable,
            vec!["common:"],
            "and the caller is told which mount was not looked in — an empty \
             `matches` beside an empty `unwalkable` would claim the install \
             holds no drawable"
        );
        assert!(!found.is_complete());
    }

    #[test]
    fn vfs_lookup_walks_devices_newest_first() {
        let mut vfs = Vfs::new();
        let common = Arc::new(InMemDevice::new(&[
            ("data/x.dat", b"from-common"),
            ("data/only-base.dat", b"only-base"),
        ]));
        let update = Arc::new(InMemDevice::new(&[
            ("data/x.dat", b"from-update"),
            ("data/only-update.dat", b"only-update"),
        ]));
        vfs.mount("common:", common);
        vfs.mount("common:", update);
        assert_eq!(vfs.device_count("common"), 2);

        let bytes = vfs.open("common:/data/x.dat").unwrap();
        assert_eq!(bytes, b"from-update", "newest mount must shadow older");

        let only_base = vfs.open("common:/data/only-base.dat").unwrap();
        assert_eq!(
            only_base, b"only-base",
            "fall-through to base when newer mount misses"
        );

        let only_update = vfs.open("common:/data/only-update.dat").unwrap();
        assert_eq!(only_update, b"only-update");
    }

    #[test]
    fn a_front_reads_newest_first_and_falls_through_as_a_whole_read_does() {
        let mut vfs = Vfs::new();
        vfs.mount(
            "common:",
            Arc::new(InMemDevice::new(&[
                ("data/x.dat", b"from-common"),
                ("data/only-base.dat", b"only-base"),
            ])),
        );
        vfs.mount(
            "common:",
            Arc::new(InMemDevice::new(&[("data/x.dat", b"from-update")])),
        );

        assert_eq!(vfs.open_head("common:/data/x.dat", 8).unwrap(), b"from-upd");
        assert_eq!(
            vfs.open_head("common:/data/only-base.dat", 4).unwrap(),
            b"only"
        );
        assert_eq!(
            vfs.open_head("common:/data/x.dat", 100).unwrap(),
            b"from-update",
            "a limit past the file gives all of it"
        );
        assert!(vfs.open_head("common:/data/none.dat", 4).is_none());
        assert!(vfs.open_head("update:/data/x.dat", 4).is_none());
    }

    #[test]
    fn vfs_unknown_mount_returns_none() {
        let mut vfs = Vfs::new();
        vfs.mount("common:", Arc::new(InMemDevice::new(&[("x.dat", b"hi")])));
        assert!(vfs.open("update:/x.dat").is_none());
        assert!(!vfs.exists("nowhere:/x.dat"));
    }

    #[test]
    fn vfs_mount_name_is_case_insensitive_and_colon_optional() {
        let mut vfs = Vfs::new();
        vfs.mount("Common:", Arc::new(InMemDevice::new(&[("x.dat", b"a")])));
        assert!(vfs.has_mount("common"));
        assert!(vfs.has_mount("COMMON:"));
        assert_eq!(vfs.open("COMMON:/x.dat").unwrap(), b"a");
    }

    #[test]
    fn vfs_does_not_alias_overlapping_mount_names() {
        let mut vfs = Vfs::new();
        vfs.mount(
            "common:",
            Arc::new(InMemDevice::new(&[("x.dat", b"common")])),
        );
        vfs.mount(
            "commoncrc:",
            Arc::new(InMemDevice::new(&[("x.dat", b"crc")])),
        );
        assert_eq!(vfs.open("common:/x.dat").unwrap(), b"common");
        assert_eq!(vfs.open("commoncrc:/x.dat").unwrap(), b"crc");
    }
}
