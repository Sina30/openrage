use super::jobs::{JobKind, Outcome, Then};
use super::*;
use or_io::container::rpf::Rdr2CryptoProvider;

impl AppState {
    pub fn set_active_game(&mut self, game: Game, install_root: PathBuf) {
        if let Some(why) = super::gta_vi::refused_install(game, &install_root) {
            self.show_error_toast(why);
            return;
        }
        self.prefs.game_paths.set(game, Some(install_root.clone()));
        self.prefs.active_game = Some(game);
        crate::diagnostics::crash::note_edition(game.generation().edition_label());
        if game.supports_key_extraction() {
            if let Some(exe) = game.find_executable(&install_root) {
                self.prefs.gta_keys.gta5_path = Some(exe);
            }
        }
        try_cache_icon_for(game, &install_root);
        save_prefs(&self.prefs);
        self.recheck_overlay_loader(&install_root);
        self.use_title_cipher(game, &install_root);
        let install = self.open_game_root(install_root);
        self.set_active_document(install);
        if self.keys.has_keys() {
            if let Some(install) = gta_install_root_from_prefs(&self.prefs) {
                self.materials_palette
                    .start_load(install.clone(), self.keys.provider());
                self.start_shader_definitions_load(install);
            }
        }
        self.maybe_start_name_scan();
        self.check_overlay_staleness();
        self.maybe_auto_extract_keys();
    }

    /// RDR2's archives open with its own cipher, built from the install's executable and a snapshot;
    /// any other title reads with GTA V's keys.
    fn use_title_cipher(&mut self, game: Game, install_root: &Path) {
        let provider = (game == Game::Rdr2).then(|| {
            let built = game.find_executable(install_root).ok_or_else(|| {
                tr!(
                    "{executable_name} was not found in {install_root}",
                    executable_name = game.executable_name(),
                    install_root = install_root.display()
                )
            });
            built.and_then(|exe| {
                Rdr2CryptoProvider::for_install(exe, or_title::config::config_dir().as_deref())
                    .map_err(|e| e.to_string())
            })
        });
        match provider {
            Some(Ok(provider)) => {
                let source = match provider.snapshot_file() {
                    Some(file) => tr!("your snapshot, {file}", file = file.display()),
                    None => tr!("the snapshot bundled with OpenRage").to_string(),
                };
                self.keys
                    .set_title_provider(Some(Arc::new(provider)), Some(Ok(source)));
            }
            Some(Err(why)) => {
                self.keys.set_title_provider(None, Some(Err(why.clone())));
                self.show_error_toast(tr!("RDR2's archives cannot be opened: {why}", why = why));
            }
            None => self.keys.set_title_provider(None, None),
        }
    }

    /// Open `game`'s install at `install_root` in place of the one open, once nothing unsaved opened from that one is lost.
    pub fn request_game_switch(&mut self, game: Game, install_root: PathBuf) {
        self.switch_game_then(game, install_root, None);
    }

    /// [`Self::request_game_switch`], then open `file` once the switch is made; a switch the user cancels opens nothing.
    pub(super) fn switch_game_then(
        &mut self,
        game: Game,
        install_root: PathBuf,
        file: Option<PathBuf>,
    ) {
        // ⚠ Before the close request, which would close the install that is open for nothing.
        if let Some(why) = super::gta_vi::refused_install(game, &install_root) {
            self.show_error_toast(why);
            return;
        }
        let open: Vec<DocumentId> = self
            .documents
            .iter()
            .filter(|d| d.kind == DocumentKind::GameRoot)
            .map(|d| d.id)
            .collect();
        let already = self.prefs.active_game == Some(game)
            && open.iter().any(|&id| {
                self.document(id).and_then(|d| d.path.as_deref()) == Some(install_root.as_path())
            });
        if !already {
            self.request_close(open, AfterClose::SwitchGame(game, install_root, file));
        } else if let Some(file) = file {
            self.load_file(file);
        }
    }

    /// Read the shader definition database this install states, unless an
    /// operator already named one.
    ///
    /// ⚠ The target is the edition's, so an Enhanced install answers from
    /// its shader container and names parameters as Enhanced materials bind
    /// them, including in a Gen8 file it ships.
    pub fn start_shader_definitions_load(&mut self, install_root: PathBuf) {
        if or_io::gta5::shaders::shader_defs::is_installed() || self.shader_defs.is_loading() {
            return;
        }
        let Some(game) = self.prefs.active_game else {
            return;
        };
        self.shader_defs.start_from_install(
            install_root,
            self.keys.provider(),
            game.asset_slug(),
            game.generation().slug(),
        );
    }

    pub fn poll_shader_definitions(&mut self) {
        let Some(status) = self.shader_defs.poll() else {
            return;
        };
        match status {
            ShaderDefsStatus::Loaded {
                shaders,
                parameters,
                ..
            } => {
                tracing::info!(shaders, parameters, "Shader definition database installed");
            }
            ShaderDefsStatus::Failed { source, message } => {
                tracing::warn!(
                    source,
                    message,
                    "No shader definition database; a material's parameters stay unnamed"
                );
            }
            _ => {}
        }
    }

    pub fn refresh_game_root_crypto(&mut self) {
        let provider = self.keys.provider();
        for doc in self.documents.iter_mut() {
            if let DocPayload::Rpf(rpf) = &mut doc.payload {
                rpf.session.set_crypto(Arc::clone(&provider));
            }
        }
    }

    /// Remember `game`'s install folder; clearing it first closes its install, asking about unsaved work.
    pub fn set_game_install_folder(&mut self, game: Game, install_root: Option<PathBuf>) {
        if install_root.is_none() {
            let folder = self.prefs.game_paths.get(game).cloned();
            let open: Vec<DocumentId> = (self.documents.iter())
                .filter(|d| {
                    d.kind == DocumentKind::GameRoot && d.path.is_some() && d.path == folder
                })
                .map(|d| d.id)
                .collect();
            if !open.is_empty() {
                self.request_close(open, AfterClose::ClearGameFolder(game));
                return;
            }
        }
        let refused = (install_root.as_deref())
            .filter(|root| !super::gta_vi::holds_unsupported(game, root))
            .and_then(|root| super::gta_vi::refused_install(game, root));
        if let Some(why) = refused {
            self.show_error_toast(why);
            return;
        }
        self.prefs.game_paths.set(game, install_root.clone());
        if self.prefs.active_game == Some(game) && game.supports_key_extraction() {
            if let Some(root) = install_root.as_ref() {
                if let Some(exe) = game.find_executable(root) {
                    self.prefs.gta_keys.gta5_path = Some(exe);
                }
            }
        }
        if let Some(root) = install_root.as_ref() {
            try_cache_icon_for(game, root);
        }
        save_prefs(&self.prefs);
        if self.prefs.active_game == Some(game) {
            self.maybe_auto_extract_keys();
        }
    }

    /// Make `game` active, opening its install through the close request when its folder is there.
    pub fn switch_active_game(&mut self, game: Game) {
        let root = self.prefs.game_paths.get(game).filter(|root| root.is_dir());
        if let Some(root) = root.cloned() {
            self.request_game_switch(game, root);
            return;
        }
        if self.prefs.active_game == Some(game) {
            return;
        }
        self.prefs.active_game = Some(game);
        crate::diagnostics::crash::note_edition(game.generation().edition_label());
        if game.supports_key_extraction() {
            if let Some(root) = self.prefs.game_paths.get(game).cloned() {
                if let Some(exe) = game.find_executable(&root) {
                    self.prefs.gta_keys.gta5_path = Some(exe);
                }
            }
        }
        save_prefs(&self.prefs);
        self.maybe_auto_extract_keys();
    }

    pub fn start_key_extraction(&mut self, gta5_path: PathBuf) {
        self.prefs.gta_keys.gta5_path = Some(gta5_path.clone());
        save_prefs(&self.prefs);
        self.keys.start_extraction(gta5_path);
        self.show_info_toast(tr!("Extracting GTA V keys…"));
    }

    /// Start the background walk that fills the metadata editor's name
    /// pickers, if there is an install to walk and keys to read it with.
    ///
    /// Idempotent: [`NameIndex::start_scan`](super::name_index::NameIndex::start_scan)
    /// refuses a second walk over a root it has already started.
    pub fn maybe_start_name_scan(&mut self) {
        if !self.keys.has_keys() {
            return;
        }
        let Some(install_root) = gta_install_root_from_prefs(&self.prefs) else {
            return;
        };
        if let Some(game) = self.prefs.active_game {
            self.name_index.retarget(game.asset_slug());
        }
        let mount = self.prefs.mount_options();
        self.name_index
            .start_scan(install_root, self.keys.provider(), mount);
    }

    /// Change how mounts resolve their archives, and re-read what that decides.
    ///
    /// ⚠ **The re-read is the point.** The name pickers and the clip-region
    /// table are already built from the old answer, and a toggle whose effect
    /// waits for the next launch is one a user concludes does nothing.
    pub fn set_read_overlay_first(&mut self, read_overlay_first: bool) {
        if self.prefs.read_overlay_first == read_overlay_first {
            return;
        }
        self.prefs.read_overlay_first = read_overlay_first;
        save_prefs(&self.prefs);

        let mount = self.prefs.mount_options();
        self.name_index.stop_bg_scan();
        self.maybe_start_name_scan();
        self.check_overlay_staleness();
        if self.keys.has_keys() {
            if let Some(install_root) = gta_install_root_from_prefs(&self.prefs) {
                let shadow = self.project_shadow_root();
                self.clip_regions
                    .start_load(install_root, self.keys.provider(), mount, shadow);
            }
        }
        self.show_toast(if read_overlay_first {
            tr!("Archives now resolve against the mods overlay first")
        } else {
            tr!("The mods overlay is ignored; archives come from the install")
        });
    }

    pub fn maybe_auto_extract_keys(&mut self) {
        // Cached keys that predate any snapshot stay in use while one
        // extraction adds their NG keys; a failure keeps them.
        if (self.keys.has_keys() && !self.keys.predate_a_snapshot())
            || self.keys.extraction.is_running
        {
            return;
        }
        if self.keys.extraction.last_error.is_some() {
            return;
        }
        let Some(game) = self.prefs.active_game else {
            return;
        };
        if !game.supports_key_extraction() {
            return;
        }
        let Some(path) = self.prefs.gta_keys.gta5_path.clone() else {
            return;
        };
        if !path.is_file() {
            return;
        }
        // To the log, not a toast: nobody asked for it and there is nothing to do about it.
        tracing::info!(exe = %path.display(), "Extracting GTA V keys");
        self.keys.start_extraction(path);
    }

    pub fn start_key_extraction_from_magic(&mut self, gta5_path: PathBuf, magic_path: PathBuf) {
        self.prefs.gta_keys.gta5_path = Some(gta5_path.clone());
        save_prefs(&self.prefs);
        self.keys.start_extraction_from_magic(gta5_path, magic_path);
        self.show_info_toast(tr!("Decoding magic.dat with the extracted AES key…"));
    }

    pub fn forget_gta_keys(&mut self) {
        self.keys.forget(&mut self.prefs);
        self.materials_palette.reset();
        self.clip_regions.reset();
        self.shader_defs.reset();
        save_prefs(&self.prefs);
        self.show_toast(tr!("GTA V keys forgotten"));
    }

    pub fn poll_key_extraction(&mut self) {
        let Some(result) = self.keys.poll() else {
            return;
        };
        match result {
            ExtractionResult::Success { cache, .. } => {
                apply_successful_extraction(&mut self.prefs.gta_keys, &cache);
                save_prefs(&self.prefs);
                // To the log, not a toast: the Keys page shows it, and so do the archives it opens.
                tracing::info!(
                    exe = %cache.metadata.gta5_path.display(),
                    "GTA V keys extracted"
                );
                if let Some(install_root) = gta_install_root_from_prefs(&self.prefs) {
                    let mount = self.prefs.mount_options();
                    self.materials_palette
                        .start_load(install_root.clone(), self.keys.provider());
                    let shadow = self.project_shadow_root();
                    self.clip_regions.start_load(
                        install_root.clone(),
                        self.keys.provider(),
                        mount,
                        shadow,
                    );
                    self.start_shader_definitions_load(install_root.clone());
                    self.open_game_root(install_root);
                }
                // New keys can open archives the last scan could not.
                self.name_index.stop_bg_scan();
                self.maybe_start_name_scan();
                self.check_overlay_staleness();
                self.refresh_game_root_crypto();
                let pending = self.keys.drain_pending();
                if !pending.is_empty() {
                    self.load_files(pending);
                }
                let said = tr!(
                    "Game keys found in {gta5_path}",
                    gta5_path = cache.metadata.gta5_path.display()
                );
                self.jobs
                    .record(JobKind::Keys, Outcome::Done(Then::Nothing), said);
            }
            ExtractionResult::Failure { message, .. } => {
                self.show_error_toast(tr!("Key extraction failed: {message}", message = message));
                self.jobs.record(JobKind::Keys, Outcome::Failed, message);
            }
            ExtractionResult::Derived => {
                self.show_toast(tr!("NG encrypt state derived"));
                let said = tr!("NG encrypt state derived");
                self.jobs
                    .record(JobKind::Keys, Outcome::Done(Then::Nothing), said);
            }
            ExtractionResult::Stopped => {
                self.jobs.record(
                    JobKind::Keys,
                    Outcome::Stopped(Then::Nothing),
                    tr!("Nothing was kept"),
                );
            }
        }
    }

    pub fn poll_material_palette_load(&mut self) {
        let Some(result) = self.materials_palette.poll() else {
            return;
        };
        match result {
            MaterialPaletteLoadResult::Success { palette, .. } => {
                tracing::info!(
                    count = palette.len(),
                    "Loaded bound material colours from the GTA install"
                );
            }
            MaterialPaletteLoadResult::Failure { error, .. } => {
                tracing::warn!(error = %error, "Material palette load failed");
            }
        }
    }

    pub fn poll_clip_regions_load(&mut self) {
        let Some(result) = self.clip_regions.poll() else {
            return;
        };
        match result {
            ClipRegionsLoadResult::Success {
                table,
                gta_root,
                shadow,
                sky,
                ..
            } => {
                tracing::info!(
                    install = %gta_root.display(),
                    project = ?shadow,
                    count = table.len(),
                    stars = sky.stars.is_some(),
                    moon = sky.moon.is_some(),
                    "Loaded particle clip-regions table"
                );
            }
            ClipRegionsLoadResult::Failure { error, gta_root } => {
                tracing::warn!(
                    install = %gta_root.display(),
                    error = %error,
                    "Clip-regions load failed; particle atlas dims will fall back to inference"
                );
                self.show_warning_toast(tr!(
                    "Particle clip-regions load failed: {error}",
                    error = error
                ));
            }
        }
    }

    /// Bake the pack of the game's shaders for the viewport once an install is set and none is current.
    pub fn maybe_auto_bake_game_pack(&mut self) -> Option<PathBuf> {
        let root = gta_install_root_from_prefs(&self.prefs)?;
        let event = self.game_pack.maybe_auto_bake(&root)?;
        self.note_game_pack(event)
    }

    /// The pack's root when a bake of it just finished, for the viewport to read.
    pub fn poll_game_pack(&mut self) -> Option<PathBuf> {
        let event = self.game_pack.poll()?;
        self.note_game_pack(event)
    }

    fn note_game_pack(&mut self, event: game_pack::GamePackEvent) -> Option<PathBuf> {
        use game_pack::GamePackEvent;
        match event {
            GamePackEvent::Ready { root, baked } => {
                if baked {
                    let said = self.game_pack.status().short_label();
                    self.show_toast(said.clone());
                    self.jobs
                        .record(JobKind::GamePackBake, Outcome::Done(Then::Nothing), said);
                }
                Some(root)
            }
            GamePackEvent::Failed { message } => {
                self.show_warning_toast(tr!(
                    "Baking the game's shaders failed: {message}",
                    message = message
                ));
                self.jobs
                    .record(JobKind::GamePackBake, Outcome::Failed, message);
                None
            }
            GamePackEvent::Stopped => {
                let said = tr!("The game's shaders were not baked");
                self.jobs
                    .record(JobKind::GamePackBake, Outcome::Stopped(Then::Nothing), said);
                None
            }
            GamePackEvent::Progress => None,
        }
    }

    /// Bake the pack of the game's shaders again now, over the one on disk.
    pub fn force_rebake_game_pack(&mut self) {
        let Some(install_root) = gta_install_root_from_prefs(&self.prefs) else {
            self.show_warning_toast(tr!(
                "Configure a GTA V install path in Settings before baking shaders."
            ));
            return;
        };
        // The helper finds the keys it needs itself
        if let Some(event) = self.game_pack.rebake(&install_root) {
            self.note_game_pack(event);
        }
    }

    pub fn poll_game_discovery(&mut self) {
        let Some(report) = self.discovery.poll() else {
            return;
        };
        let mut wrote_prefs = false;
        for install in &report.installs {
            if self.prefs.game_paths.get(install.game).is_none() {
                self.prefs
                    .game_paths
                    .set(install.game, Some(install.install_dir.clone()));
                wrote_prefs = true;
                tracing::info!(
                    game = ?install.game,
                    source = ?install.source,
                    path = %install.install_dir.display(),
                    "Auto-detected install",
                );
            }
        }
        if wrote_prefs {
            save_prefs(&self.prefs);
        }
    }

    pub fn reload_material_palette(&mut self) {
        let Some(install_root) = gta_install_root_from_prefs(&self.prefs) else {
            self.show_error_toast(tr!(
                "Choose a GTA V install (Settings ▸ Games) before reloading the palette"
            ));
            return;
        };
        if !self.keys.has_keys() {
            self.show_error_toast(tr!(
                "Set up the GTA V keys (Settings ▸ Games ▸ Keys) before reloading the palette"
            ));
            return;
        }
        self.materials_palette
            .start_load(install_root, self.keys.provider());
    }

    pub fn maybe_defer_encrypted_rpf(&mut self, path: &Path) -> bool {
        if self.keys.has_keys() {
            return false;
        }
        match rpf_needs_keys(path) {
            Some(true) => {
                self.keys.queue_pending(path.to_path_buf());
                self.keys.show_prompt = true;
                true
            }
            _ => false,
        }
    }
}
