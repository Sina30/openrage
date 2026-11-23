//! The texture dictionaries the game's sky and its water draw from: the stars and the face of the moon, the ripples and the foam.

use std::sync::Arc;

use or_io::container::{RpfArchive, RpfCryptoProvider};

use crate::{ArchiveDevice, GtaInstall, Vfs};

/// The archive of an install that holds the textures the sky draws from.
pub const SKY_TEXTURES_ARCHIVE: &str = "x64a.rpf";

/// The dictionary in it, by its path in the archive.
pub const SKY_TEXTURES_FILE: &str = "textures/skydome.ytd";

/// The dictionary of the shader library, in the same archive, which holds the water's ripples and its foam among the maps every effect shares.
pub const WATER_TEXTURES_FILE: &str = "textures/graphics.ytd";

/// The bytes of the install's `skydome.ytd`; `None` where the install has no such archive or file.
pub fn read_sky_textures(
    install: &GtaInstall,
    crypto: Arc<dyn RpfCryptoProvider>,
) -> Option<Vec<u8>> {
    read_textures_files(install, crypto, &[SKY_TEXTURES_FILE]).remove(0)
}

/// The bytes of the install's `graphics.ytd`; `None` where the install has no such archive or file.
pub fn read_water_textures(
    install: &GtaInstall,
    crypto: Arc<dyn RpfCryptoProvider>,
) -> Option<Vec<u8>> {
    read_textures_files(install, crypto, &[WATER_TEXTURES_FILE]).remove(0)
}

/// The bytes of each of `files` in the archive of textures, which is opened once; `None` for one it lacks, and for all where the install has no such archive.
pub fn read_textures_files(
    install: &GtaInstall,
    crypto: Arc<dyn RpfCryptoProvider>,
    files: &[&str],
) -> Vec<Option<Vec<u8>>> {
    let Ok(archive) = RpfArchive::open_with(install.root.join(SKY_TEXTURES_ARCHIVE), crypto) else {
        return vec![None; files.len()];
    };
    let mut vfs = Vfs::new();
    vfs.mount("textures:", Arc::new(ArchiveDevice::new(Arc::new(archive))));
    files
        .iter()
        .map(|file| vfs.open(&format!("textures:/{file}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;
    use or_io::container::rpf::RpfBuilder;

    fn install_with(files: &[(&str, &[u8])]) -> (tempfile::TempDir, GtaInstall) {
        let dir = tempfile::tempdir().unwrap();
        let mut builder = RpfBuilder::default();
        for (name, body) in files {
            builder.insert_with(name, *body, false).unwrap();
        }
        builder.save(dir.path().join(SKY_TEXTURES_ARCHIVE)).unwrap();
        let install = GtaInstall {
            root: dir.path().to_path_buf(),
        };
        (dir, install)
    }

    fn crypto() -> Arc<dyn RpfCryptoProvider> {
        Arc::new(NoOpCryptoProvider::new())
    }

    #[test]
    fn the_dictionary_is_read_from_the_archive_of_textures() {
        let (_dir, install) =
            install_with(&[(SKY_TEXTURES_FILE, b"stars"), ("textures/other.ytd", b"x")]);
        assert_eq!(
            read_sky_textures(&install, crypto()).as_deref(),
            Some(&b"stars"[..])
        );
    }

    #[test]
    fn the_sky_and_the_water_are_read_from_one_opening_of_the_archive_and_one_it_lacks_is_none() {
        let (_dir, install) = install_with(&[
            (SKY_TEXTURES_FILE, b"stars"),
            (WATER_TEXTURES_FILE, b"ripples"),
        ]);
        let read = read_textures_files(
            &install,
            crypto(),
            &[SKY_TEXTURES_FILE, WATER_TEXTURES_FILE, "textures/none.ytd"],
        );
        assert_eq!(
            read,
            [Some(b"stars".to_vec()), Some(b"ripples".to_vec()), None]
        );
        assert_eq!(
            read_water_textures(&install, crypto()).as_deref(),
            Some(&b"ripples"[..])
        );
        let nowhere = GtaInstall {
            root: install.root.join("no-such-folder"),
        };
        assert_eq!(
            read_textures_files(&nowhere, crypto(), &[SKY_TEXTURES_FILE]),
            [None]
        );
    }

    #[test]
    fn an_install_without_the_dictionary_or_the_archive_has_none() {
        let (_dir, install) = install_with(&[("textures/other.ytd", b"x")]);
        assert!(read_sky_textures(&install, crypto()).is_none());
        let nowhere = GtaInstall {
            root: install.root.join("no-such-folder"),
        };
        assert!(read_sky_textures(&nowhere, crypto()).is_none());
    }
}
