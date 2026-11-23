use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::container::rpf::RpfArchive;
use or_io::error::IoError;
use or_io::gta5::data::materials_palette::parser::build_palette_entries;
use or_io::gta5::data::materials_palette::{MaterialPalette, MaterialPaletteSource};

#[derive(Debug, Clone)]
pub struct GtaInstall {
    pub root: PathBuf,
}

impl GtaInstall {
    pub fn from_path(path: &Path) -> Option<Self> {
        let root = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()?.to_path_buf()
        };
        Some(Self { root })
    }

    pub fn common_rpf(&self) -> PathBuf {
        self.root.join("common.rpf")
    }

    pub fn update_rpf(&self) -> PathBuf {
        self.root.join("update").join("update.rpf")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MaterialPaletteLoadError {
    #[error("GTA install path not configured")]
    NoInstallPath,
    #[error("`{path}` not found in GTA install")]
    Missing { path: PathBuf },
    #[error("Failed to read `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse RPF `{path}`: {source}")]
    Rpf {
        path: PathBuf,
        #[source]
        source: IoError,
    },
    #[error("`{path}` not found inside `{archive}`")]
    EntryMissing { archive: PathBuf, path: String },
    #[error("Failed to extract `{entry}` from `{archive}`: {source}")]
    EntryExtract {
        archive: PathBuf,
        entry: String,
        #[source]
        source: IoError,
    },
    #[error("Parsed palette is empty (no usable rows in materials.dat)")]
    EmptyPalette,
}

pub fn load_from_gta_install(
    gta_install: &GtaInstall,
    crypto: Arc<dyn RpfCryptoProvider>,
) -> Result<MaterialPalette, MaterialPaletteLoadError> {
    let common_path = gta_install.common_rpf();
    if !common_path.exists() {
        return Err(MaterialPaletteLoadError::Missing { path: common_path });
    }

    let common = open_rpf(&common_path, Arc::clone(&crypto))?;

    let materialfx_dat = read_text_entry(&common, &common_path, "data/effects/materialfx.dat")?;

    let materials_dat = match read_materials_dat_from_update(gta_install, crypto)? {
        Some(text) => text,
        None => read_text_entry(&common, &common_path, "data/materials/materials.dat")?,
    };

    let entries = build_palette_entries(&materials_dat, &materialfx_dat);
    if entries.is_empty() {
        return Err(MaterialPaletteLoadError::EmptyPalette);
    }

    Ok(MaterialPalette::from_runtime(
        entries,
        MaterialPaletteSource::GtaInstall {
            install_root: gta_install.root.clone(),
        },
    ))
}

fn read_materials_dat_from_update(
    gta_install: &GtaInstall,
    crypto: Arc<dyn RpfCryptoProvider>,
) -> Result<Option<String>, MaterialPaletteLoadError> {
    let update_path = gta_install.update_rpf();
    if !update_path.exists() {
        return Ok(None);
    }
    let update = open_rpf(&update_path, crypto)?;
    match update.find("common/data/materials/materials.dat") {
        Some(idx) => {
            let bytes =
                update
                    .read_entry(idx)
                    .map_err(|e| MaterialPaletteLoadError::EntryExtract {
                        archive: update_path,
                        entry: "common/data/materials/materials.dat".into(),
                        source: e,
                    })?;
            Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
        }
        None => Ok(None),
    }
}

fn open_rpf(
    path: &Path,
    crypto: Arc<dyn RpfCryptoProvider>,
) -> Result<RpfArchive, MaterialPaletteLoadError> {
    RpfArchive::open_with(path, crypto).map_err(|e| MaterialPaletteLoadError::Rpf {
        path: path.to_path_buf(),
        source: e,
    })
}

fn read_text_entry(
    archive: &RpfArchive,
    archive_path: &Path,
    entry_path: &str,
) -> Result<String, MaterialPaletteLoadError> {
    let entry_idx =
        archive
            .find(entry_path)
            .ok_or_else(|| MaterialPaletteLoadError::EntryMissing {
                archive: archive_path.to_path_buf(),
                path: entry_path.to_string(),
            })?;
    let bytes =
        archive
            .read_entry(entry_idx)
            .map_err(|e| MaterialPaletteLoadError::EntryExtract {
                archive: archive_path.to_path_buf(),
                entry: entry_path.to_string(),
                source: e,
            })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;

    #[test]
    fn from_path_uses_parent_for_files() {
        let exe_like = Path::new("D:/Games/GTA/gta5.exe");
        let install = GtaInstall::from_path(exe_like).unwrap();
        assert_eq!(install.root, Path::new("D:/Games/GTA"));
    }

    #[test]
    fn from_path_uses_dir_directly() {
        let cwd = std::env::current_dir().unwrap();
        let install = GtaInstall::from_path(&cwd).unwrap();
        assert_eq!(install.root, cwd);
    }

    #[test]
    fn missing_install_dir_returns_missing_error() {
        let install = GtaInstall {
            root: PathBuf::from("/nonexistent/gta-install-path"),
        };
        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let err = load_from_gta_install(&install, provider).unwrap_err();
        match err {
            MaterialPaletteLoadError::Missing { path } => {
                assert!(path.ends_with("common.rpf"));
            }
            other => panic!("expected Missing, got {other:?}"),
        }
    }
}
