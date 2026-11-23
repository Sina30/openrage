use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use or_io::container::RpfCryptoProvider;
use or_io::gta5::data::ptx_clip_regions::ClipRegionTable;
use or_renderer::game_shaders::dome::SkyTextures;
use or_renderer::game_shaders::water::WaterTextures;
use or_title::{
    build_vfs_for_install_with, load_clip_regions, load_timecycle, mount_shadow,
    read_textures_files, ClipRegionLoadError, GtaInstall, MountOptions, TimecycleData,
    VfsBuildError, SKY_TEXTURES_FILE, WATER_TEXTURES_FILE,
};

#[derive(Debug, Clone, Default)]
pub enum ClipRegionsStatus {
    #[default]
    NotLoaded,
    Loading,
    Loaded,
    Failed,
}

pub enum ClipRegionsLoadResult {
    Success {
        gta_root: PathBuf,
        table: Arc<ClipRegionTable>,
        /// The look of the day the install states, read in the same pass over its files.
        timecycle: Arc<TimecycleData>,
        /// The stars and the face of the moon the install gives its sky, read in the same pass.
        sky: Arc<SkyTextures>,
        /// The ripples and the foam the install gives its water, read in the same pass.
        water: Arc<WaterTextures>,
        /// The project folder read ahead of the install, when one was.
        shadow: Option<PathBuf>,
    },
    Failure {
        gta_root: PathBuf,
        error: ClipRegionsLoadError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ClipRegionsLoadError {
    #[error(transparent)]
    Vfs(#[from] VfsBuildError),
    #[error(transparent)]
    Load(#[from] ClipRegionLoadError),
}

type LoadKey = (PathBuf, MountOptions, Option<PathBuf>);

pub struct ClipRegionsController {
    table: Arc<ClipRegionTable>,
    timecycle: Arc<TimecycleData>,
    sky: Arc<SkyTextures>,
    water: Arc<WaterTextures>,
    /// How many times the maps of the sky have been read, for what draws them to know they changed.
    sky_revision: u64,
    pub status: ClipRegionsStatus,
    rx: Option<Receiver<ClipRegionsLoadResult>>,
    requested: Option<LoadKey>,
}

impl Default for ClipRegionsController {
    fn default() -> Self {
        Self {
            table: Arc::new(ClipRegionTable::empty()),
            timecycle: Arc::new(TimecycleData::default()),
            sky: Arc::new(SkyTextures::default()),
            water: Arc::new(WaterTextures::default()),
            sky_revision: 0,
            status: ClipRegionsStatus::NotLoaded,
            rx: None,
            requested: None,
        }
    }
}

impl ClipRegionsController {
    pub fn table(&self) -> Arc<ClipRegionTable> {
        Arc::clone(&self.table)
    }

    /// The stars and the face of the moon the install gives its sky.
    pub fn sky_textures(&self) -> Arc<SkyTextures> {
        Arc::clone(&self.sky)
    }

    /// The ripples and the foam the install gives its water, read when its sky is.
    pub fn water_textures(&self) -> Arc<WaterTextures> {
        Arc::clone(&self.water)
    }

    /// How many times they have been read: what draws them reads them again when it moves.
    pub fn sky_revision(&self) -> u64 {
        self.sky_revision
    }

    pub fn timecycle(&self) -> Arc<TimecycleData> {
        Arc::clone(&self.timecycle)
    }

    pub fn reset(&mut self) {
        self.table = Arc::new(ClipRegionTable::empty());
        self.timecycle = Arc::new(TimecycleData::default());
        self.sky = Arc::new(SkyTextures::default());
        self.water = Arc::new(WaterTextures::default());
        self.sky_revision += 1;
        self.status = ClipRegionsStatus::NotLoaded;
        self.rx = None;
        self.requested = None;
    }

    /// Read the table on a worker, through `shadow` first when there is one.
    ///
    /// ⚠ **A different request supersedes one in flight rather than being
    /// dropped.** The startup load is usually still running when a project
    /// is opened or the overlay toggle flipped, and ignoring the second
    /// request would leave the table read the old way with nothing saying so.
    /// An identical request is the only one ignored.
    pub fn start_load(
        &mut self,
        gta_root: PathBuf,
        crypto: Arc<dyn RpfCryptoProvider>,
        mount: MountOptions,
        shadow: Option<PathBuf>,
    ) {
        let key = (gta_root.clone(), mount, shadow.clone());
        if self.rx.is_some() && self.requested.as_ref() == Some(&key) {
            tracing::debug!("Clip-regions load already in flight; ignoring duplicate request");
            return;
        }
        self.requested = Some(key);
        let install = GtaInstall {
            root: gta_root.clone(),
        };
        self.status = ClipRegionsStatus::Loading;
        let (tx, rx): (
            Sender<ClipRegionsLoadResult>,
            Receiver<ClipRegionsLoadResult>,
        ) = channel();
        self.rx = Some(rx);
        let sky_crypto = Arc::clone(&crypto);
        let shared_crypto = Arc::clone(&crypto);
        thread::spawn(move || {
            let result = match build_vfs_for_install_with(&install, crypto, mount) {
                Ok(mut vfs) => {
                    if let Some(root) = &shadow {
                        mount_shadow(&mut vfs, root);
                    }
                    // The sky's dictionary and the one the water's maps are in, from one opening of the archive
                    let shared = read_textures_files(
                        &install,
                        Arc::clone(&shared_crypto),
                        &[SKY_TEXTURES_FILE, WATER_TEXTURES_FILE],
                    );
                    match load_clip_regions(&vfs) {
                        Ok(table) => ClipRegionsLoadResult::Success {
                            gta_root,
                            table: Arc::new(table),
                            timecycle: Arc::new(load_timecycle(&vfs)),
                            sky: Arc::new(
                                shared[0]
                                    .as_deref()
                                    .map(|bytes| {
                                        super::sky_textures::decode(bytes, Arc::clone(&sky_crypto))
                                    })
                                    .unwrap_or_default(),
                            ),
                            water: Arc::new(
                                shared[1]
                                    .as_deref()
                                    .map(|bytes| {
                                        super::sky_textures::decode_water(
                                            bytes,
                                            Arc::clone(&sky_crypto),
                                        )
                                    })
                                    .unwrap_or_default(),
                            ),
                            shadow,
                        },
                        Err(e) => ClipRegionsLoadResult::Failure {
                            gta_root,
                            error: ClipRegionsLoadError::Load(e),
                        },
                    }
                }
                Err(e) => ClipRegionsLoadResult::Failure {
                    gta_root,
                    error: ClipRegionsLoadError::Vfs(e),
                },
            };
            let _ = tx.send(result);
        });
    }

    pub fn poll(&mut self) -> Option<ClipRegionsLoadResult> {
        let receiver = self.rx.as_ref()?;
        let result = receiver.try_recv().ok()?;
        self.rx = None;
        match &result {
            ClipRegionsLoadResult::Success {
                table,
                timecycle,
                sky,
                water,
                ..
            } => {
                self.table = Arc::clone(table);
                self.timecycle = Arc::clone(timecycle);
                self.sky = Arc::clone(sky);
                self.water = Arc::clone(water);
                self.sky_revision += 1;
                self.status = ClipRegionsStatus::Loaded;
            }
            ClipRegionsLoadResult::Failure { gta_root, error } => {
                tracing::warn!(
                    install = %gta_root.display(),
                    error = %error,
                    "Failed to load ptxclipregions.dat from GTA install; \
                     falling back to atlas inference"
                );
                self.status = ClipRegionsStatus::Failed;
            }
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;
    use or_io::container::rpf::RpfBuilder;

    #[test]
    fn reset_returns_to_default_state() {
        let mut c = ClipRegionsController {
            status: ClipRegionsStatus::Loaded,
            ..Default::default()
        };
        c.reset();
        assert!(matches!(c.status, ClipRegionsStatus::NotLoaded));
        assert!(c.table().is_empty());
    }

    fn table_text(texture: &str) -> Vec<u8> {
        format!("1 1\n{texture} 1 1 0.0 1.0 0.0 1.0\n").into_bytes()
    }

    /// An install whose `common.rpf` holds a table naming `install_tex`, and a
    /// project shadow whose copy names `project_tex`.
    fn install_and_shadow() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("GTAV");
        std::fs::create_dir_all(&install).unwrap();
        RpfBuilder::default()
            .insert_with(
                "data/effects/ptxclipregions.dat",
                table_text("install_tex"),
                false,
            )
            .unwrap()
            .save(install.join("common.rpf"))
            .unwrap();

        let shadow = dir.path().join("proj").join("vfs");
        let copy = shadow.join("common/data/effects/ptxclipregions.dat");
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::write(copy, table_text("project_tex")).unwrap();
        (dir, install, shadow)
    }

    fn finish(c: &mut ClipRegionsController) -> ClipRegionsLoadResult {
        for _ in 0..500 {
            if let Some(result) = c.poll() {
                return result;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the clip-region load never finished");
    }

    fn crypto() -> Arc<dyn RpfCryptoProvider> {
        Arc::new(NoOpCryptoProvider::new())
    }

    #[test]
    fn an_open_project_supplies_the_table_the_particles_render_with() {
        let (_dir, install, shadow) = install_and_shadow();
        let mut c = ClipRegionsController::default();

        c.start_load(install.clone(), crypto(), MountOptions::default(), None);
        assert!(matches!(
            finish(&mut c),
            ClipRegionsLoadResult::Success { .. }
        ));
        assert!(c.table().get_by_name("install_tex").is_some());

        c.start_load(
            install,
            crypto(),
            MountOptions::default(),
            Some(shadow.clone()),
        );
        match finish(&mut c) {
            ClipRegionsLoadResult::Success { shadow: read, .. } => {
                assert_eq!(read.as_deref(), Some(shadow.as_path()))
            }
            ClipRegionsLoadResult::Failure { error, .. } => panic!("{error}"),
        }
        assert!(c.table().get_by_name("project_tex").is_some());
        assert!(c.table().get_by_name("install_tex").is_none());
    }

    /// ⚠ Opening a project while the startup load runs is the ordinary case.
    #[test]
    fn a_request_through_a_project_supersedes_one_still_in_flight() {
        let (_dir, install, shadow) = install_and_shadow();
        let mut c = ClipRegionsController::default();

        c.start_load(install.clone(), crypto(), MountOptions::default(), None);
        c.start_load(install, crypto(), MountOptions::default(), Some(shadow));

        assert!(matches!(
            finish(&mut c),
            ClipRegionsLoadResult::Success { .. }
        ));
        assert!(
            c.table().get_by_name("project_tex").is_some(),
            "the second request is the one whose answer lands"
        );
        assert!(c.poll().is_none(), "and the superseded one never arrives");
    }
}
