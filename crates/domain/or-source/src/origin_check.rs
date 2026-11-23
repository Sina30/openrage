//! Whether the game's file a source was taken from is still the file it was taken from.
//!
//! ⚠ **On demand only.** The check reads the install, so a scan, a build and a recook never
//! call it: a person asks for it, for one source.

use crate::asset::{sha256_hex, AssetManifest};

/// What the caller found in the install at the path a source records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    Found(Vec<u8>),
    /// The install holds no file at that path.
    Missing,
    /// The install could not be read, and why.
    Unreadable(String),
}

/// What the install said of the file a source was taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The install's file is the one `origin.sha256` names.
    Unchanged,
    /// The install's file hashes to `now` and the source was taken from `recorded`.
    Changed { recorded: String, now: String },
    /// The install holds no file at `path`.
    Missing { path: String },
    /// Nothing was compared, and why.
    Unchecked(String),
}

/// The words the Assets row and Problems use for a source whose original is another file now.
pub const CHANGED: &str = "the game's original changed";

impl Verdict {
    pub fn is_changed(&self) -> bool {
        matches!(self, Self::Changed { .. })
    }

    /// One sentence a person can read.
    pub fn sentence(&self) -> String {
        match self {
            Self::Unchanged => "the game's original is the file this was taken from".to_string(),
            Self::Changed { recorded, now } => format!(
                "{CHANGED}: this was taken from a file with SHA-256 {}, and the install now holds {}",
                short(recorded),
                short(now)
            ),
            Self::Missing { path } => format!(
                "the install holds no file at {path}: the game removed it, or this source was imported from a folder opened below the install"
            ),
            Self::Unchecked(why) => why.clone(),
        }
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// Compares `origin.sha256` of `manifest` with the file `install` reads at `origin.virtual_path`.
///
/// `install` is called once, and not at all when the manifest states nothing to compare.
pub fn check(manifest: &AssetManifest, install: impl FnOnce(&str) -> Installed) -> Verdict {
    let origin = &manifest.origin;
    if !origin.from_game {
        return Verdict::Unchecked(
            "this source is not the game's file, so there is no original to compare".to_string(),
        );
    }
    let Some(recorded) = &origin.sha256 else {
        return Verdict::Unchecked("this source records no hash of its original".to_string());
    };
    let Some(path) = &origin.virtual_path else {
        return Verdict::Unchecked(
            "this source records no place in the game for its original: a file picked from disk has none"
                .to_string(),
        );
    };
    match install(path) {
        Installed::Missing => Verdict::Missing { path: path.clone() },
        Installed::Unreadable(why) => {
            Verdict::Unchecked(format!("the install could not be read: {why}"))
        }
        Installed::Found(bytes) => match sha256_hex(&bytes) {
            now if now.eq_ignore_ascii_case(recorded) => Verdict::Unchanged,
            now => Verdict::Changed {
                recorded: recorded.clone(),
                now,
            },
        },
    }
}
