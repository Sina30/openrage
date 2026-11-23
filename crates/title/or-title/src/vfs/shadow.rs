//! Loose files read ahead of everything an install mounts: a project's copies.
//!
//! `<root>/<mount>/<path>` answers `<mount>:/<path>`, so a folder that mirrors
//! the VFS is all a project needs to be what the application reads first.
//!
//! ⚠ **The opposite of the overlay's shape.** The overlay holds *archives* and
//! has to open them where the install's copy mounts — see [`super::overlay`].
//! A project holds the *files* somebody edited, loose, so a [`FolderDevice`]
//! per mount is right here and wrong there. A directory named `props.rpf`
//! inside the shadow is a directory: the path through it is spelled the way
//! the VFS spells a path through an archive.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::folder_device::FolderDevice;
use super::mount::Vfs;

/// What a shadow mount attached, and what it could not attach anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShadowReport {
    /// Each mount name that now reads the shadow first, beside the folder it reads.
    pub shadowed: Vec<(String, PathBuf)>,
    /// Folders naming no mount the VFS holds, which therefore shadow nothing.
    ///
    /// ⚠ **Reported rather than mounted.** Mounting one would invent a device
    /// the game has never heard of, and a `dlc_foo` folder shadowing nothing
    /// because the DLC cutoff left `dlc_foo:` unmounted looks exactly like one
    /// that works unless something says so.
    pub unmatched: Vec<String>,
}

/// Mount every folder below `root` over the VFS mount it is named for.
///
/// ⚠ **Mount order is precedence, so call this last.** `Vfs` walks each stack
/// newest-first; anything mounted after this outranks the project, including
/// an overlay archive.
///
/// ⚠ **A folder shadows its `crc` twin too.** The game mounts `common:` and
/// `commoncrc:` over the same device, and so does every mount that has one;
/// shadowing one alone would have the two disagree about which copy exists.
///
/// ⚠ **`update/common` and `update/x64` also shadow `common:` and
/// `platform:`**, because that is where the install mounts `update.rpf`'s two
/// sub-roots and so where the game reads a patched file. A file copied out of
/// `update.rpf/common/…` would otherwise shadow only `update:/common/…`, which
/// nothing reads for rendering. The plain `common/` spelling is mounted after
/// it and wins when both hold the same path.
pub fn mount_shadow(vfs: &mut Vfs, root: &Path) -> ShadowReport {
    let mut report = ShadowReport::default();
    let Ok(entries) = std::fs::read_dir(root) else {
        return report;
    };
    let mut folders: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| {
            (
                e.file_name().to_string_lossy().to_ascii_lowercase(),
                e.path(),
            )
        })
        .collect();
    folders.sort();

    if let Some((_, update)) = folders.iter().find(|(name, _)| name == "update") {
        for (sub_root, mount) in [("common", "common"), ("x64", "platform")] {
            if let Some(dir) = child_dir(update, sub_root) {
                attach(vfs, mount, &dir, &mut report);
            }
        }
    }

    for (name, dir) in &folders {
        if !attach(vfs, name, dir, &mut report) {
            report.unmatched.push(name.clone());
        }
    }
    report
}

fn attach(vfs: &mut Vfs, mount: &str, dir: &Path, report: &mut ShadowReport) -> bool {
    if !vfs.has_mount(mount) {
        return false;
    }
    let device = Arc::new(FolderDevice::new(dir));
    vfs.mount(mount, device.clone());
    report.shadowed.push((mount.to_string(), dir.to_path_buf()));
    let twin = format!("{mount}crc");
    if vfs.has_mount(&twin) {
        vfs.mount(&twin, device);
        report.shadowed.push((twin, dir.to_path_buf()));
    }
    true
}

fn child_dir(parent: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(parent)
        .ok()?
        .flatten()
        .find(|e| {
            e.file_type().is_ok_and(|t| t.is_dir())
                && e.file_name().to_string_lossy().eq_ignore_ascii_case(name)
        })
        .map(|e| e.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::FileDevice;
    use std::collections::HashMap;

    #[derive(Debug)]
    struct InMem(HashMap<String, Vec<u8>>);

    impl InMem {
        fn arc(entries: &[(&str, &[u8])]) -> Arc<Self> {
            Arc::new(Self(
                entries
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_vec()))
                    .collect(),
            ))
        }
    }

    impl FileDevice for InMem {
        fn open(&self, relative_path: &str) -> Option<Vec<u8>> {
            self.0.get(relative_path.trim_start_matches('/')).cloned()
        }
    }

    fn write(path: &Path, body: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn game() -> Vfs {
        let mut vfs = Vfs::new();
        let common = InMem::arc(&[("data/x.dat", b"install"), ("data/only.dat", b"install")]);
        vfs.mount("common:", common.clone());
        vfs.mount("commoncrc:", common);
        vfs.mount("update:", InMem::arc(&[("common/data/x.dat", b"install")]));
        vfs
    }

    #[test]
    fn a_project_copy_is_read_first_and_a_missing_one_falls_through() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("Common/Data/X.dat"), b"project");
        let mut vfs = game();

        let report = mount_shadow(&mut vfs, dir.path());

        assert_eq!(
            vfs.open("common:/data/x.dat").as_deref(),
            Some(&b"project"[..])
        );
        assert_eq!(
            vfs.open("commoncrc:/data/x.dat").as_deref(),
            Some(&b"project"[..]),
            "the crc twin reads the same device, so it has to read the same copy"
        );
        assert_eq!(
            vfs.open("common:/data/only.dat").as_deref(),
            Some(&b"install"[..]),
            "a file the project does not hold still comes from the install"
        );
        assert_eq!(report.shadowed.len(), 2, "{report:?}");
        assert!(report.unmatched.is_empty(), "{report:?}");
    }

    #[test]
    fn a_folder_naming_no_mount_is_reported_and_mounts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("dlc_nothere/data/x.dat"), b"project");
        write(&dir.path().join("notes.txt"), b"not a folder");
        let mut vfs = game();

        let report = mount_shadow(&mut vfs, dir.path());

        assert_eq!(report.unmatched, vec!["dlc_nothere".to_string()]);
        assert!(report.shadowed.is_empty());
        assert!(
            !vfs.has_mount("dlc_nothere"),
            "a shadow never invents a mount the install does not have"
        );
    }

    #[test]
    fn a_file_copied_out_of_the_title_update_shadows_what_the_game_reads() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("update/common/data/x.dat"),
            b"from-update-copy",
        );
        let mut vfs = game();

        mount_shadow(&mut vfs, dir.path());

        assert_eq!(
            vfs.open("common:/data/x.dat").as_deref(),
            Some(&b"from-update-copy"[..]),
            "`update.rpf/common` is mounted on `common:`, so its project copy must be too"
        );
        assert_eq!(
            vfs.open("update:/common/data/x.dat").as_deref(),
            Some(&b"from-update-copy"[..])
        );

        write(&dir.path().join("common/data/x.dat"), b"plain");
        let mut vfs = game();
        mount_shadow(&mut vfs, dir.path());
        assert_eq!(
            vfs.open("common:/data/x.dat").as_deref(),
            Some(&b"plain"[..]),
            "the plain spelling is mounted last and wins"
        );
    }

    #[test]
    fn a_missing_root_shadows_nothing() {
        let mut vfs = game();
        let report = mount_shadow(&mut vfs, Path::new("Z:/no/such/project/vfs"));
        assert_eq!(report, ShadowReport::default());
        assert_eq!(vfs.device_count("common"), 1);
    }
}
