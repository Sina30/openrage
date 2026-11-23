use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use or_io::container::RpfCryptoProvider;
use or_io::gta5::data::materials_palette::{MaterialPalette, MaterialPaletteSource};
use or_title::{load_material_palette_from_gta_install, GtaInstall, MaterialPaletteLoadError};

#[derive(Debug, Clone, Default)]
pub enum MaterialPaletteStatus {
    #[default]
    NotLoaded,
    Loading {
        gta_root: PathBuf,
    },
    Runtime {
        gta_root: PathBuf,
    },
    Failed {
        gta_root: PathBuf,
        message: String,
    },
}

impl MaterialPaletteStatus {
    pub fn failure_message(&self) -> Option<&str> {
        match self {
            MaterialPaletteStatus::Failed { message, .. } => Some(message),
            _ => None,
        }
    }
}

pub enum MaterialPaletteLoadResult {
    Success {
        gta_root: PathBuf,
        palette: Arc<MaterialPalette>,
    },
    Failure {
        gta_root: PathBuf,
        error: MaterialPaletteLoadError,
    },
}

pub struct MaterialsPaletteController {
    palette: Arc<MaterialPalette>,
    pub status: MaterialPaletteStatus,
    rx: Option<Receiver<MaterialPaletteLoadResult>>,
}

impl Default for MaterialsPaletteController {
    fn default() -> Self {
        Self {
            palette: Arc::new(MaterialPalette::empty().clone()),
            status: MaterialPaletteStatus::NotLoaded,
            rx: None,
        }
    }
}

impl MaterialsPaletteController {
    pub fn palette(&self) -> Arc<MaterialPalette> {
        Arc::clone(&self.palette)
    }

    pub fn palette_ref(&self) -> &MaterialPalette {
        &self.palette
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.status, MaterialPaletteStatus::Loading { .. })
    }

    pub fn reset(&mut self) {
        self.palette = Arc::new(MaterialPalette::empty().clone());
        self.status = MaterialPaletteStatus::NotLoaded;
        self.rx = None;
    }

    pub fn start_load(&mut self, gta_root: PathBuf, crypto: Arc<dyn RpfCryptoProvider>) {
        if self.rx.is_some() {
            tracing::debug!("Material palette load already in flight; ignoring duplicate request");
            return;
        }
        let install = GtaInstall {
            root: gta_root.clone(),
        };
        self.status = MaterialPaletteStatus::Loading {
            gta_root: gta_root.clone(),
        };
        let (tx, rx): (
            Sender<MaterialPaletteLoadResult>,
            Receiver<MaterialPaletteLoadResult>,
        ) = channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let result = match load_material_palette_from_gta_install(&install, crypto) {
                Ok(palette) => MaterialPaletteLoadResult::Success {
                    gta_root,
                    palette: Arc::new(palette),
                },
                Err(error) => MaterialPaletteLoadResult::Failure { gta_root, error },
            };
            let _ = tx.send(result);
        });
    }

    pub fn poll(&mut self) -> Option<MaterialPaletteLoadResult> {
        let receiver = self.rx.as_ref()?;
        let result = receiver.try_recv().ok()?;
        self.rx = None;
        match &result {
            MaterialPaletteLoadResult::Success { gta_root, palette } => {
                self.palette = Arc::clone(palette);
                let resolved_root = match palette.source() {
                    MaterialPaletteSource::GtaInstall { install_root } => install_root.clone(),
                    MaterialPaletteSource::NotLoaded => gta_root.clone(),
                };
                self.status = MaterialPaletteStatus::Runtime {
                    gta_root: resolved_root,
                };
            }
            MaterialPaletteLoadResult::Failure { gta_root, error } => {
                tracing::warn!(
                    install = %gta_root.display(),
                    error = %error,
                    "Failed to load material palette from GTA install; materials stay neutral"
                );
                self.status = MaterialPaletteStatus::Failed {
                    gta_root: gta_root.clone(),
                    message: error.to_string(),
                };
            }
        }
        Some(result)
    }
}
