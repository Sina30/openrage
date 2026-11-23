//! The mods an install holds: what each is, the paths it writes, and the
//! load order that decides which of two mods writing one path wins.
//!
//! ⚠ **A mod is one folder directly under `mods` (enabled) or `mods_`
//! (parked), and its files sit below that folder as the install lays them
//! out.** The stack resolves those paths through the VFS, newest device
//! first; it does not write the winner into the overlay.

mod order;
mod resolve;
mod scan;

use std::path::{Path, PathBuf};

pub use order::{arrange, nudge, order_of};
pub use resolve::{resolve, ClaimKind, Conflict, Resolution, Unread};
pub use scan::{read_dlclist, scan_install};

/// The directory a user parks mods in: the overlay, renamed out of the game's sight.
pub const PARKED_DIR: &str = "mods_";

/// What a mod is, from what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    LooseFiles,
    DlcPack,
    AsiPlugin,
    AsiLoader,
    ScriptMod,
}

impl ModKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::LooseFiles => "Loose files",
            Self::DlcPack => "DLC pack",
            Self::AsiPlugin => "ASI plugin",
            Self::AsiLoader => "ASI loader",
            Self::ScriptMod => "Script mod",
        }
    }
}

/// Where a mod sits, which is what decides its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModPlace {
    /// Under `mods`: in the load order.
    Overlay,
    /// Under `mods_`: kept, and out of the load order.
    Parked,
    /// In the game folder itself, which has no load order of ours.
    GameFolder,
}

impl ModPlace {
    pub fn status(self) -> ModStatus {
        match self {
            Self::Parked => ModStatus::Disabled,
            Self::Overlay | Self::GameFolder => ModStatus::Enabled,
        }
    }

    /// Does the load order list a mod here?
    pub fn is_ordered(self) -> bool {
        self != Self::GameFolder
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModStatus {
    Enabled,
    Disabled,
}

impl ModStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
        }
    }
}

/// One `dlc*.rpf` of a DLC pack a mod carries, read from its entry table.
#[derive(Debug, Clone)]
pub struct ModPack {
    /// The pack's folder name, which is the name `dlclist.xml` gives it.
    pub name: String,
    /// The archive, below the mod's folder.
    pub archive: String,
    /// Whether `dlclist.xml` names the pack; `None` when no list was read.
    pub listed: Option<bool>,
    /// Every file inside, nested archives included, bar the pack's own two documents.
    pub entries: Vec<String>,
    /// Why the archive could not be read; `entries` is empty then.
    pub unread: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InstalledMod {
    pub name: String,
    pub kind: ModKind,
    pub place: ModPlace,
    /// The mod's folder, or the file itself when it is one file.
    pub root: PathBuf,
    /// Every loose file, below `root`, `/`-separated.
    pub files: Vec<String>,
    pub packs: Vec<ModPack>,
}

/// What names one install in the preferences: its root, spelled one way.
pub fn install_key(root: &Path) -> String {
    root.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}
