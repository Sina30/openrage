//! The game's own passes in the viewport: the SPIR-V pack the bridge reads, and the scene of the open drawable it prepares for it.

use std::sync::Arc;

use or_renderer::game_shaders::mesh::{Material, MeshScene, Surface};
use or_renderer::game_shaders::pack::GamePack;
use or_renderer::ShadingMode;

use super::{MeshCacheKey, RenderBridge};
use crate::state::AppState;

/// What the surfaces of the open drawable came to when prepared for the game's own passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSceneReport {
    /// Surfaces that will draw.
    pub drawn: usize,
    /// Each surface left out: the effect its material names, and why.
    pub left_out: Vec<(String, String)>,
    /// For a GTA VI drawable: whether its programs are being translated, and what Shaded says.
    pub gta6: Option<(bool, String)>,
}

impl RenderBridge {
    /// Read the game's SPIR-V pack at `root`, which `ShadingMode::Shaded` draws with; `false` where there is none there or it will not read.
    pub fn set_game_pack_path(&mut self, root: &std::path::Path) -> bool {
        match GamePack::open(root) {
            Ok(pack) => {
                // A material names its effect by hash, and the pack's effects are the names that hash back
                or_io::gta5::shaders::shader_names::install(
                    pack.manifest()
                        .effects
                        .iter()
                        .map(|e| (or_io::hash::string_hash_str(&e.name), e.name.clone())),
                );
                tracing::info!(root = %root.display(), effects = pack.manifest().effects.len(), "loaded the game shader pack");
                let pack = Arc::new(pack);
                self.renderer.set_game_pack(Some(pack.clone()));
                self.game_pack = Some(pack);
                self.game_pack_epoch = self.game_pack_epoch.wrapping_add(1);
                // What was built is for the pack that was read before
                self.game_pipelines = Default::default();
                self.renderer.reset_deferred_pipelines();
                self.drop_game_scene();
                true
            }
            Err(e) => {
                tracing::debug!(root = %root.display(), error = %e, "no game shader pack here");
                false
            }
        }
    }

    /// Whether there is a pack of the game's shaders to draw with.
    pub fn game_shaders_available(&self) -> bool {
        self.game_pack.is_some()
    }

    /// What the last scene of the game's passes came to; `None` where there is none.
    pub fn game_scene_report(&self) -> Option<&GameSceneReport> {
        self.game_scene_report.as_ref()
    }

    /// Give the game's passes the pose the animation holds, which is by bone of the skeleton the surfaces were bound to, when it has changed.
    fn refresh_skin(&mut self, state: &AppState) {
        let key = (state.anim.skin_revision(), state.anim.skin_matrices.len());
        if self.skin_key == Some(key) {
            return;
        }
        self.skin_key = Some(key);
        self.held.skin_key = None;
        self.renderer.set_skin(state.anim.skin_matrices.clone());
    }

    fn drop_game_scene(&mut self) {
        self.game_scene_key = None;
        self.held.game_key = None;
        self.decoded = Default::default();
        self.game_scene_report = None;
        self.renderer.set_game_scene(None);
        self.renderer.clear_deferred();
    }

    /// Prepare the active drawable for the game's own passes when the mode draws them, and again when what it is made from changes.
    pub fn refresh_game_scene(&mut self, state: &AppState) {
        let revision = state.clip_regions.sky_revision();
        if self.sky_textures_of != revision {
            self.sky_textures_of = revision;
            self.renderer
                .set_sky_textures((*state.clip_regions.sky_textures()).clone());
            self.renderer
                .set_water_textures((*state.clip_regions.water_textures()).clone());
        }
        if self.mode != ShadingMode::Shaded {
            if self.game_scene_key.is_some() {
                self.drop_game_scene();
            }
            return;
        }
        let Some(doc) = state.active_document else {
            return;
        };
        if let Some(hashes) = crate::render::gta6_shaded::shader_hashes(state) {
            self.refresh_gta6_scene(state, doc, &hashes);
            return;
        }
        if self.game_pack.is_none() {
            // What a GTA VI drawable's programs drew is not this document's
            if self.game_scene_key.is_some() {
                self.drop_game_scene();
            }
            return;
        }
        self.refresh_skin(state);
        let key = MeshCacheKey::of(state, doc, self.game_pack_epoch);
        if self.game_scene_key == Some(key) && !self.renderer.deferred_needs_prepare() {
            return;
        }
        self.game_scene_key = Some(key);
        self.held.game_key = None;
        let Some(surfaces) = crate::render::game_scene::surfaces_shown(state, &mut self.decoded)
        else {
            self.renderer.set_game_scene(None);
            self.renderer.clear_deferred();
            self.game_scene_report = None;
            return;
        };
        self.prepare_game_surfaces(surfaces);
    }

    /// The dressed ped for the game's own passes when the mode draws them, and again when what is worn changes; the ped stands at rest.
    pub fn refresh_dressed_game(&mut self, scene: Option<&crate::render::dressed::DressedScene>) {
        if self.mode != ShadingMode::Shaded {
            if self.held.game_key.take().is_some() {
                self.drop_game_scene();
            }
            return;
        }
        let Some(scene) = scene else {
            return;
        };
        if self.game_pack.is_none() {
            return;
        }
        let key = (scene.key, self.game_pack_epoch);
        if self.held.game_key == Some(key) && !self.renderer.deferred_needs_prepare() {
            return;
        }
        self.held.game_key = Some(key);
        self.game_scene_key = None;
        let surfaces = scene.shaded_surfaces();
        if surfaces.is_empty() {
            self.renderer.set_game_scene(None);
            self.renderer.clear_deferred();
            self.game_scene_report = None;
            return;
        }
        self.prepare_game_surfaces(surfaces);
    }

    /// Put the game's passes' ped in the pose `pose` stands for, numbered by its revision, or at rest for none; a pose already held is not written again.
    pub fn pose_dressed_game(&mut self, pose: Option<(u64, &[glam::Mat4])>) {
        let key = pose.map_or((0, 0), |(revision, skin)| (revision, skin.len()));
        if self.held.skin_key == Some(key) {
            return;
        }
        self.held.skin_key = Some(key);
        self.renderer
            .set_skin(pose.map(|(_, skin)| skin.to_vec()).unwrap_or_default());
    }

    /// Prepare `surfaces` for the game's own passes, lit by its deferred passes where the pack has them and by each pass alone where a pass will not build.
    fn prepare_game_surfaces(&mut self, surfaces: Vec<(Surface, Material)>) {
        let Some(pack) = &self.game_pack else {
            return;
        };
        // The game's deferred passes light it where the pack has them; a pass the device refuses is reported by its error scope and not by a panic
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let deferred = self
            .renderer
            .prepare_deferred(&self.device, &self.queue, pack, &surfaces);
        match (pollster::block_on(validation.pop()), deferred) {
            (None, Ok((drawn, left_out))) => {
                self.renderer.set_game_scene(None);
                self.game_scene_report = Some(GameSceneReport {
                    drawn,
                    left_out,
                    gta6: None,
                });
                return;
            }
            (error, deferred) => {
                tracing::warn!(device = ?error, prepare = ?deferred.err(), "the game's deferred passes did not prepare, so it is lit by each pass alone");
                // A pipeline the device refused is not one to build the next scene with
                self.renderer.reset_deferred_pipelines();
                self.renderer.clear_deferred();
            }
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let scene = MeshScene::prepare_with(
            &mut self.game_pipelines,
            &self.device,
            &self.queue,
            pack,
            &self.renderer.game_target(),
            &surfaces,
        );
        if let Some(error) = pollster::block_on(validation.pop()) {
            tracing::warn!(%error, "the game's passes did not prepare");
            // A pipeline the device refused is not one to build the next scene with
            self.game_pipelines = Default::default();
            self.renderer.set_game_scene(None);
            self.game_scene_report = None;
            return;
        }
        self.game_scene_report = Some(GameSceneReport {
            drawn: scene.drawn(),
            left_out: scene
                .skipped()
                .iter()
                .map(|(effect, error)| (effect.clone(), error.to_string()))
                .collect(),
            gta6: None,
        });
        self.renderer.set_game_scene(Some(scene));
    }

    /// The active GTA VI drawable for the game's own passes, by the programs its effects run as
    /// translated in memory: asked for, prepared again when a new pack of them is made, and drawn
    /// as Material where none of its surfaces draws.
    fn refresh_gta6_scene(
        &mut self,
        state: &AppState,
        doc: crate::state::DocumentId,
        hashes: &[u32],
    ) {
        use crate::state::gta6_shaders;
        let install = (state.prefs.game_paths.get(or_title::Game::GtaVI))
            .map(|root| root.join("shaders.rpf"));
        gta6_shaders::request(hashes, install);
        let running = gta6_shaders::running();
        let pack = gta6_shaders::pack();
        let key = MeshCacheKey::of(state, doc, pack.as_ref().map_or(0, |(number, _)| *number));
        let fresh = self.game_scene_key != Some(key);
        let report = self.game_scene_report.as_ref();
        let was = report.and_then(|r| r.gta6.as_ref()).map(|(was, _)| *was);
        if !fresh && was == Some(running) {
            return;
        }
        let kept = report
            .filter(|_| !fresh)
            .map(|r| (r.drawn, r.left_out.clone()));
        let (drawn, left_out) = match kept {
            Some(kept) => kept,
            None => {
                self.game_scene_key = Some(key);
                self.held.game_key = None;
                self.renderer.clear_deferred();
                match (&pack, crate::render::gta6_shaded::shaded_surfaces(state)) {
                    (Some((_, pack)), Some(surfaces)) => {
                        self.prepare_gta6_surfaces(pack, &surfaces)
                    }
                    _ => {
                        self.renderer.set_game_scene(None);
                        (0, Vec::new())
                    }
                }
            }
        };
        let said = crate::render::gta6_shaded::said(
            &gta6_shaders::status(hashes),
            drawn,
            drawn + left_out.len(),
        );
        self.game_scene_report = Some(GameSceneReport {
            drawn,
            left_out,
            gta6: Some((running, said)),
        });
    }

    /// Prepare `surfaces` with `pack`'s forward passes alone, since GTA VI's effects hold none of
    /// the lighting passes GTA V's deferred route draws with; a scene that draws nothing is not
    /// set, so Shaded draws as Material.
    fn prepare_gta6_surfaces(
        &mut self,
        pack: &GamePack,
        surfaces: &[(Surface, Material)],
    ) -> (usize, Vec<(String, String)>) {
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let scene = MeshScene::prepare_with(
            &mut self.game_pipelines,
            &self.device,
            &self.queue,
            pack,
            &self.renderer.game_target(),
            surfaces,
        );
        if let Some(error) = pollster::block_on(validation.pop()) {
            tracing::warn!(%error, "the GTA VI programs' passes did not prepare");
            // A pipeline the device refused is not one to build the next scene with
            self.game_pipelines = Default::default();
            self.renderer.set_game_scene(None);
            return (0, Vec::new());
        }
        let left_out = (scene.skipped().iter())
            .map(|(effect, error)| (effect.clone(), error.to_string()))
            .collect();
        let drawn = scene.drawn();
        self.renderer.set_game_scene((drawn > 0).then_some(scene));
        (drawn, left_out)
    }
}
