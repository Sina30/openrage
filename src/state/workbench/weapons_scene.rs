//! The assembled weapon's scene: the weapon's own model and the parts it wears, read on a worker and handed to the viewport.
//!
//! ⚠ **The engine decides what is worn and this reads the files.** `or_weapons::assemble` says which part sits at which bone; every
//! file here is a source of the pack, cooked as a build cooks it, and the scene draws each model where its own file puts it.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_renderer::OrbitCamera;

use super::dressed::{diffuse_called_of_bytes, model_of_bytes};
use crate::render::dressed::{DressedScene, ScenePart};
use crate::state::AppState;

/// What a scene is made of: the source folders of the models and the textures source, and the key that stands for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub key: u64,
    pub models: Vec<PathBuf>,
    pub textures: Option<PathBuf>,
}

/// The scene being read, the one on screen and the camera it is seen through.
#[derive(Default)]
pub struct SceneWork {
    pending: Option<(u64, Receiver<Result<DressedScene, String>>)>,
    pub scene: Option<DressedScene>,
    /// Why the last scene could not be read.
    pub error: Option<String>,
    pub camera: OrbitCamera,
    /// The camera is placed on the scene the next time one is on screen.
    pub needs_frame: bool,
    wanted: Option<u64>,
}

impl SceneWork {
    /// Whether a scene is being read.
    pub fn is_reading(&self) -> bool {
        self.pending.is_some()
    }

    /// Ask for the scene `request` stands for, unless it is the one on screen or being read.
    pub fn want(&mut self, request: Request, crypto: Arc<dyn RpfCryptoProvider>) {
        if self.wanted == Some(request.key) {
            return;
        }
        self.wanted = Some(request.key);
        let (sender, receiver) = channel();
        let key = request.key;
        std::thread::spawn(move || {
            let _ = sender.send(read(&request, &crypto));
        });
        self.pending = Some((key, receiver));
    }

    /// Take what the worker left; true when the scene on screen changed.
    pub fn poll(&mut self) -> bool {
        let Some((key, receiver)) = &self.pending else {
            return false;
        };
        let key = *key;
        match receiver.try_recv() {
            Ok(result) => {
                self.pending = None;
                // ⚠ A scene that is no longer wanted is let go: the choice moved on while it was read.
                if self.wanted != Some(key) {
                    return false;
                }
                match result {
                    Ok(scene) => {
                        self.scene = Some(scene);
                        self.error = None;
                        self.needs_frame = true;
                    }
                    Err(why) => {
                        self.scene = None;
                        self.error = Some(why);
                    }
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                false
            }
        }
    }

    /// Nothing is wanted any more: the weapon was deselected.
    pub fn clear(&mut self) {
        self.pending = None;
        self.scene = None;
        self.error = None;
        self.wanted = None;
    }
}

/// Cook the source folder `dir` and say what it made.
fn cooked(dir: &std::path::Path) -> Result<(PathBuf, Vec<u8>), String> {
    let (name, bytes) = or_wardrobe::facts::cooked(dir)?;
    Ok((PathBuf::from(name), bytes))
}

/// The name a weapon's own model goes by: its folder's name up to the first dot, which the diffuse of its textures is called too.
fn called_like(dir: &std::path::Path) -> String {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
    or_weapons::rules::stem(&name.unwrap_or_default())
}

/// Read the scene: every model's meshes at the first level they have, drawn with the texture of the textures source that is called as the weapon's own model, or its first where none is.
///
/// ⚠ A weapon's dictionary holds the decals, the normal and specular maps and the palette beside the diffuse, and the first of them is not the diffuse.
fn read(request: &Request, crypto: &Arc<dyn RpfCryptoProvider>) -> Result<DressedScene, String> {
    let called = request.models.first().map(|dir| called_like(dir));
    let diffuse = match &request.textures {
        Some(dir) => {
            let (name, bytes) = cooked(dir)?;
            let called = called.as_deref().unwrap_or_default();
            Some(Arc::new(diffuse_called_of_bytes(
                &name, &bytes, crypto, called,
            )?))
        }
        None => None,
    };
    let mut parts = Vec::new();
    for dir in &request.models {
        let (name, bytes) = cooked(dir)?;
        let model = model_of_bytes(&name, &bytes, crypto)
            .map_err(|why| format!("{}: {why}", dir.display()))?;
        let Some(level) = model.at(0) else {
            continue;
        };
        parts.push(ScenePart {
            geometry: Arc::clone(level.geometry),
            diffuse: diffuse.clone(),
            tint: [1.0, 1.0, 1.0],
            lift: 0.0,
            garment: None,
        });
    }
    if parts.is_empty() {
        return Err(tr!("The models hold no meshes to draw").to_string());
    }
    Ok(DressedScene::new(request.key, parts))
}

impl AppState {
    /// What the assembled weapon is made of now: the selected weapon's own model and the parts it wears under the choice, with its textures.
    pub fn weapon_scene_request(&self) -> Option<Request> {
        let (pack, weapon) = self.selected_weapon()?;
        let read = pack.read.as_ref().ok()?;
        let assembly = or_weapons::assemble::assemble(weapon, &read.facts);
        let worn = or_weapons::assemble::worn(&assembly, &self.workbench.weapons.choice);
        let mut models: Vec<String> = assembly.base.iter().cloned().collect();
        models.extend(worn.iter().filter_map(|p| p.model.clone()));
        let textures = weapon
            .sources
            .iter()
            .find(|path| read.facts.source(path).is_some_and(|s| s.is_textures()))
            .cloned();
        if models.is_empty() {
            return None;
        }
        // The key stands for the files as they are: a change of any of them reads the scene again.
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        weapon.id.hash(&mut hasher);
        models.hash(&mut hasher);
        textures.hash(&mut hasher);
        self.workbench.weapons.generation.hash(&mut hasher);
        for path in models.iter().chain(textures.iter()) {
            if let Ok(stat) = std::fs::metadata(pack.folder.join(path)) {
                stat.modified().ok().hash(&mut hasher);
            }
        }
        Some(Request {
            key: hasher.finish(),
            models: models.iter().map(|m| pack.folder.join(m)).collect(),
            textures: textures.map(|t| pack.folder.join(t)),
        })
    }

    /// Step the camera of the assembled weapon by what the pointer did, and place it on the weapon when it is asked to.
    pub(crate) fn step_weapon_camera(
        &mut self,
        inputs: crate::render::viewport::CameraInputs,
    ) -> &OrbitCamera {
        let work = &mut self.workbench.weapons.scene;
        inputs.apply_to(&mut work.camera);
        if inputs.frame_active || inputs.frame_selection {
            work.needs_frame = true;
        }
        if work.needs_frame {
            if let Some(drawn) = work.scene.as_ref().filter(|s| !s.parts.is_empty()) {
                work.camera.frame_sphere(drawn.centre, drawn.radius);
                work.needs_frame = false;
            }
        }
        &work.camera
    }
}
