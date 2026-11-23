//! Getting a shader definition database into this process, and saying so when
//! there is none.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;

use or_io::container::RpfCryptoProvider;
use or_io::gta5::shaders::shader_defs::{self, ParamRemap, ShaderDefinitions, SHADER_DEFS_ENV};
use or_title::install::shader_defs::load_shader_definitions;
use or_title::install::GtaInstall;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ShaderDefsStatus {
    /// Nothing has been asked for yet. ⚠ Not the same as an empty database:
    /// with no database the editor must say it cannot name a parameter rather
    /// than report that the parameter has no name.
    #[default]
    Absent,
    Loading {
        source: String,
    },
    Loaded {
        source: String,
        shaders: usize,
        parameters: usize,
    },
    Failed {
        source: String,
        message: String,
    },
}

impl ShaderDefsStatus {
    pub fn failure_message(&self) -> Option<&str> {
        match self {
            Self::Failed { message, .. } => Some(message),
            _ => None,
        }
    }
}

enum Loaded {
    Ok(Box<ShaderDefinitions>),
    Err(String),
}

#[derive(Default)]
pub struct ShaderDefsController {
    pub status: ShaderDefsStatus,
    /// Each parameter's spelling in both editions, which a conversion respells by; empty with no database.
    pub remap: Arc<ParamRemap>,
    rx: Option<Receiver<Loaded>>,
    attempted_root: Option<PathBuf>,
}

impl ShaderDefsController {
    pub fn is_loading(&self) -> bool {
        matches!(self.status, ShaderDefsStatus::Loading { .. })
    }

    /// Read the database `OPENRAGE_SHADER_DEFS` names, if it names one.
    ///
    /// ⚠ **An operator's explicit path outranks the install**, because it is
    /// the only way to run against a database built from a different build of
    /// the game than the one mounted — which is exactly the comparison
    /// `or-shader-tool defs diff` exists for.
    pub fn start_staged(&mut self) {
        let Some(raw) = std::env::var_os(SHADER_DEFS_ENV) else {
            return;
        };
        if raw.is_empty() || self.rx.is_some() {
            return;
        }
        let path = PathBuf::from(&raw);
        self.status = ShaderDefsStatus::Loading {
            source: path.display().to_string(),
        };
        let (tx, rx) = channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let msg = match shader_defs::load_from(&path) {
                Ok(defs) => Loaded::Ok(Box::new(defs)),
                Err(e) => Loaded::Err(e.to_string()),
            };
            let _ = tx.send(msg);
        });
    }

    /// Assemble one from the `.fxc` and `.sps` a mounted install ships.
    pub fn start_from_install(
        &mut self,
        gta_root: PathBuf,
        crypto: Arc<dyn RpfCryptoProvider>,
        title: &str,
        target: &str,
    ) {
        if self.rx.is_some() || self.attempted_root.as_deref() == Some(gta_root.as_path()) {
            return;
        }
        self.attempted_root = Some(gta_root.clone());
        self.status = ShaderDefsStatus::Loading {
            source: gta_root.display().to_string(),
        };

        let install = GtaInstall {
            root: gta_root.clone(),
        };
        let (title, target) = (title.to_string(), target.to_string());
        let (tx, rx) = channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let msg = match load_shader_definitions(&install, crypto, &title, &target) {
                Ok(load) => Loaded::Ok(Box::new(load.definitions)),
                Err(e) => Loaded::Err(e.to_string()),
            };
            let _ = tx.send(msg);
        });
    }

    pub fn reset(&mut self) {
        shader_defs::clear();
        self.status = ShaderDefsStatus::Absent;
        self.remap = Arc::default();
        self.rx = None;
        self.attempted_root = None;
    }

    /// Install what a finished load produced, and report what it was.
    pub fn poll(&mut self) -> Option<ShaderDefsStatus> {
        let message = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        let source = match &self.status {
            ShaderDefsStatus::Loading { source } => source.clone(),
            _ => String::new(),
        };
        self.status = match message {
            Loaded::Ok(defs) => {
                let (shaders, parameters) = (defs.shaders.len(), defs.parameter_count());
                self.remap = Arc::new(ParamRemap::of(&defs));
                shader_defs::install(*defs);
                ShaderDefsStatus::Loaded {
                    source,
                    shaders,
                    parameters,
                }
            }
            Loaded::Err(message) => ShaderDefsStatus::Failed { source, message },
        };
        Some(self.status.clone())
    }
}
