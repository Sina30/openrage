//! Whether a title's game reads an install's `mods` overlay, told from the file names beside its executable.
//!
//! The game reads nothing from `mods` unaided: a plugin (an `.asi` file) redirects its reads, and an ASI
//! loader, a DLL the game itself loads, starts that plugin.
//!
//! ⚠ A name is evidence that a file is there and not that it works, and nothing is run: a `dinput8.dll` is
//! taken for an ASI loader because the plugin projects say so, and a loader a user renamed is not seen.

use std::io;
use std::path::Path;

use super::profile::GameProfile;
use super::registry;

/// A third-party project that OpenRage names and links, and never fetches, bundles or installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Project {
    pub name: &'static str,
    pub page: &'static str,
}

/// A plugin that redirects the game's reads to `mods`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redirect {
    pub file: &'static str,
    pub project: Project,
    /// It starts itself, so no ASI loader file has to stand beside it.
    pub loads_itself: bool,
}

/// What a title needs beside its executable for the game to read `mods`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayLoaders {
    /// The plugins that read `mods` for this title, the one to suggest first leading.
    pub redirects: &'static [Redirect],
    /// The names the game loads an ASI loader under.
    pub loader_files: &'static [&'static str],
    /// Where an ASI loader comes from.
    pub loader_projects: &'static [Project],
}

impl OverlayLoaders {
    /// No loader is known for the title, so nothing is said about its overlay.
    pub const NONE: Self = Self {
        redirects: &[],
        loader_files: &[],
        loader_projects: &[],
    };
}

/// What stands beside one install's executable, against what its title needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayLoaderReport {
    pub rules: OverlayLoaders,
    /// The plugins for this title that are there.
    pub redirects: Vec<&'static Redirect>,
    /// The ASI loader file that is there, under the name the rules list.
    pub loader: Option<&'static str>,
    /// Plugins that are there and read `mods` only for another title.
    pub for_other_titles: Vec<&'static str>,
}

impl OverlayLoaderReport {
    /// Whether the title states anything about its overlay.
    pub fn known(&self) -> bool {
        !self.rules.redirects.is_empty()
    }

    /// A plugin for this title is there, and so is whatever starts it.
    pub fn reads(&self) -> bool {
        self.redirects
            .iter()
            .any(|plugin| plugin.loads_itself || self.loader.is_some())
    }

    /// No plugin for this title is there.
    pub fn lacks_redirect(&self) -> bool {
        self.known() && self.redirects.is_empty()
    }

    /// A plugin is there and nothing starts it.
    pub fn lacks_loader(&self) -> bool {
        !self.redirects.is_empty() && !self.reads()
    }

    /// An ASI loader is part of what is missing, or of what a plugin to add would need.
    pub fn needs_loader_file(&self) -> bool {
        !self.reads()
            && self.loader.is_none()
            && (self.lacks_loader() || self.rules.redirects.iter().any(|r| !r.loads_itself))
    }
}

/// Read `root`'s file names against what `profile` needs; an error when the folder cannot be listed.
pub fn check_overlay_loader(
    profile: &dyn GameProfile,
    root: &Path,
) -> io::Result<OverlayLoaderReport> {
    let rules = profile.overlay_loaders();
    let present: Vec<String> = std::fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let has = |file: &str| present.iter().any(|name| name.eq_ignore_ascii_case(file));

    let own = |file: &str| {
        rules
            .redirects
            .iter()
            .any(|r| r.file.eq_ignore_ascii_case(file))
    };
    let mut for_other_titles: Vec<&'static str> = Vec::new();
    for other in registry::all() {
        for plugin in other.overlay_loaders().redirects {
            if !own(plugin.file) && has(plugin.file) && !for_other_titles.contains(&plugin.file) {
                for_other_titles.push(plugin.file);
            }
        }
    }

    Ok(OverlayLoaderReport {
        rules,
        redirects: rules.redirects.iter().filter(|r| has(r.file)).collect(),
        loader: rules.loader_files.iter().copied().find(|file| has(file)),
        for_other_titles,
    })
}
