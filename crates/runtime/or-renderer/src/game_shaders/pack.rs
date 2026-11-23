//! A SPIR-V pack on disk, as `or-shader-tool bake-pack` writes it, or one translated in this process: its manifest, and its modules read into `naga`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::gta5::shaders::spirv_pack::{
    module_path, Outcome, SpirvPack, MANIFEST_FILE, PACK_VERSION,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackError {
    #[error("no pack manifest at '{0}'")]
    ManifestMissing(PathBuf),
    #[error("cannot read the pack manifest '{path}': {source}")]
    ManifestRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the pack manifest '{path}' does not parse: {source}")]
    ManifestParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("the pack is version {found}, and this reads version {expected}")]
    Version { found: u32, expected: u32 },
    #[error("no module for program {key}: {source}")]
    ModuleRead {
        key: String,
        #[source]
        source: std::io::Error,
    },
    #[error("the module of program {key} does not read: {message}")]
    ModuleParse { key: String, message: String },
    #[error("program {key} is not translated: {reason}")]
    NotTranslated { key: String, reason: String },
}

/// A pack's manifest and where its modules are.
#[derive(Debug, Clone)]
pub struct GamePack {
    store: Store,
    manifest: Arc<SpirvPack>,
}

#[derive(Debug, Clone)]
enum Store {
    Folder(PathBuf),
    /// Modules translated in this process, by program key.
    Memory(Arc<HashMap<String, Arc<naga::Module>>>),
}

impl GamePack {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, PackError> {
        let root = root.into();
        let path = root.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => PackError::ManifestMissing(path.clone()),
            _ => PackError::ManifestRead {
                path: path.clone(),
                source,
            },
        })?;
        let manifest: SpirvPack =
            serde_json::from_str(&text).map_err(|source| PackError::ManifestParse {
                path: path.clone(),
                source,
            })?;
        if manifest.schema_version != PACK_VERSION {
            return Err(PackError::Version {
                found: manifest.schema_version,
                expected: PACK_VERSION,
            });
        }
        Ok(Self {
            store: Store::Folder(root),
            manifest: Arc::new(manifest),
        })
    }

    /// A pack of `modules` translated in this process, by the program keys `manifest` names.
    pub fn in_memory(manifest: SpirvPack, modules: HashMap<String, Arc<naga::Module>>) -> Self {
        Self {
            store: Store::Memory(Arc::new(modules)),
            manifest: Arc::new(manifest),
        }
    }

    pub fn manifest(&self) -> &SpirvPack {
        &self.manifest
    }

    /// The folder the modules are in; `None` for a pack held in memory.
    pub fn root(&self) -> Option<&Path> {
        match &self.store {
            Store::Folder(root) => Some(root),
            Store::Memory(_) => None,
        }
    }

    /// The module of the program whose source hashes to `key`.
    ///
    /// The legalizer has taken out what naga's reader would refuse, so it reads with no capability held to the strict list.
    pub fn module(&self, key: &str) -> Result<naga::Module, PackError> {
        let root = match &self.store {
            Store::Folder(root) => root,
            Store::Memory(modules) => {
                return modules.get(key).map(|m| (**m).clone()).ok_or_else(|| {
                    let outcome = self.manifest.programs.get(key).map(|p| &p.outcome);
                    PackError::NotTranslated {
                        key: key.to_string(),
                        reason: match outcome {
                            Some(Outcome::Refused { reason } | Outcome::Skipped { reason }) => {
                                reason.clone()
                            }
                            _ => "the pack holds no module for it".to_string(),
                        },
                    }
                })
            }
        };
        let bytes =
            std::fs::read(root.join(module_path(key))).map_err(|source| PackError::ModuleRead {
                key: key.to_string(),
                source,
            })?;
        let options = naga::front::spv::Options {
            adjust_coordinate_space: false,
            strict_capabilities: false,
            block_ctx_dump_prefix: None,
        };
        naga::front::spv::parse_u8_slice(&bytes, &options).map_err(|e| PackError::ModuleParse {
            key: key.to_string(),
            message: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests;
