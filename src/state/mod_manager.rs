//! The mod manager: what the active install's `mods` and `mods_` folders and game folder hold,
//! the load order saved for that install, and which mod wins where two write one path.
//!
//! ⚠ Moving a mod between `mods` and `mods_` is the only thing here that touches a user's
//! files: it asks the edit gate and the user first, and moving it back undoes it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_title::mods::{self, InstalledMod, ModPlace, Resolution, PARKED_DIR};
use or_title::OVERLAY_DIR;

use super::prefs::save_prefs;
use super::{AppState, GateOutcome};

/// A move between `mods` and `mods_` the user has been shown and has not yet answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingModMove {
    pub name: String,
    /// `true` moves a parked mod into the load order.
    pub enable: bool,
    pub from: PathBuf,
    pub to: PathBuf,
    pub files: usize,
}

#[derive(Default)]
pub struct ModManager {
    /// The install `mods` below was read from.
    pub root: Option<PathBuf>,
    /// In load order: the last enabled mod writing a path wins it.
    pub mods: Vec<InstalledMod>,
    pub resolution: Resolution,
    pub pending_move: Option<PendingModMove>,
}

/// ⚠ Each install's order is read on its own: a value `serde` cannot parse fails the whole
/// preferences, so an unreadable order costs only itself and the install's mods stack by name.
pub fn readable_mod_orders<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(install, order)| {
            Some((install.clone(), serde_json::from_value(order.clone()).ok()?))
        })
        .collect())
}

impl AppState {
    /// Read the active install's mods again, in the load order saved for it.
    ///
    /// ponytail: reads on the calling thread each `dlc*.rpf` entry table of a mod's pack and
    /// the install's `dlclist.xml`, which is a handful of archive tables; a job of its own
    /// when a mods folder of many packs stalls a frame.
    pub fn refresh_mods(&mut self) {
        let Some(root) = self.prefs.active_game_root().cloned() else {
            self.mod_manager = ModManager::default();
            return;
        };
        let crypto = self.keys.provider();
        let list = mods::read_dlclist(&root, Arc::clone(&crypto), self.prefs.mount_options());
        let mut found = mods::scan_install(&root, &crypto, list.as_ref());
        let saved = (self.prefs.mod_orders.get(&mods::install_key(&root))).map_or(&[][..], |o| o);
        mods::arrange(&mut found, saved);
        self.mod_manager.resolution = mods::resolve(&found);
        self.mod_manager.mods = found;
        self.mod_manager.root = Some(root);
    }

    /// Move `name` one place up or down the load order, save the order for this install and resolve again.
    pub fn move_mod(&mut self, name: &str, up: bool) {
        if !mods::nudge(&mut self.mod_manager.mods, name, up) {
            return;
        }
        self.mod_manager.resolution = mods::resolve(&self.mod_manager.mods);
        let Some(root) = &self.mod_manager.root else {
            return;
        };
        let order = mods::order_of(&self.mod_manager.mods);
        self.prefs.mod_orders.insert(mods::install_key(root), order);
        save_prefs(&self.prefs);
    }

    /// Ask whether to move `name` out of the load order into `mods_`, or back into `mods`.
    ///
    /// ⚠ Asks the edit gate about the overlay side of the move, `mods/<name>`, in both
    /// directions: the parked folder is the user's own and holds nothing the game reads.
    pub fn ask_toggle_mod(&mut self, name: &str) {
        let Some(root) = self.mod_manager.root.clone() else {
            return;
        };
        let Some(found) =
            (self.mod_manager.mods.iter()).find(|m| m.name == name && m.place.is_ordered())
        else {
            return;
        };
        let enable = found.place == ModPlace::Parked;
        let files = found.files.len();
        let (from_dir, to_dir) = if enable {
            (PARKED_DIR, OVERLAY_DIR)
        } else {
            (OVERLAY_DIR, PARKED_DIR)
        };
        let (from, to) = (root.join(from_dir).join(name), root.join(to_dir).join(name));
        if let Some(why) = self.mod_move_blocker(&from, &to) {
            self.show_error_toast(why);
            return;
        }
        match self.gate_permits(Some(&root.join(OVERLAY_DIR).join(name))) {
            GateOutcome::Proceed => {}
            // ⚠ Only a shipped file asks, and `mods` holds none.
            GateOutcome::Refused | GateOutcome::Confirm(_) => return,
        }
        self.mod_manager.pending_move = Some(PendingModMove {
            name: name.to_string(),
            enable,
            from,
            to,
            files,
        });
    }

    /// The move the user was shown, done or dropped.
    pub fn answer_mod_move(&mut self, accept: bool) {
        let Some(pending) = self.mod_manager.pending_move.take().filter(|_| accept) else {
            return;
        };
        let Some(root) = self.mod_manager.root.clone() else {
            return;
        };
        match self.gate_permits(Some(&root.join(OVERLAY_DIR).join(&pending.name))) {
            GateOutcome::Proceed => {}
            GateOutcome::Refused | GateOutcome::Confirm(_) => return,
        }
        if let Some(why) = self.mod_move_blocker(&pending.from, &pending.to) {
            self.show_error_toast(why);
            return;
        }
        let moved = (pending.to.parent())
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::rename(&pending.from, &pending.to));
        match moved {
            Ok(()) => {
                let (verb, place) = if pending.enable {
                    (tr!("Enabled"), OVERLAY_DIR)
                } else {
                    (tr!("Disabled"), PARKED_DIR)
                };
                self.show_info_toast(tr!(
                    "{verb} {name}: it is now in {place}",
                    verb = verb,
                    place = place,
                    name = pending.name
                ));
            }
            Err(why) => self.show_error_toast(tr!(
                "{name} could not be moved to {to}: {why}. Nothing was changed",
                why = why,
                name = pending.name,
                to = pending.to.display()
            )),
        }
        self.refresh_mods();
    }

    /// Why `from` cannot move to `to` now, if it cannot.
    fn mod_move_blocker(&self, from: &Path, to: &Path) -> Option<String> {
        if !from.exists() {
            return Some(tr!(
                "{from} is no longer there; refresh the list",
                from = from.display()
            ));
        }
        if to.exists() {
            return Some(tr!(
                "{to} is already there; remove or rename one of the two first",
                to = to.display()
            ));
        }
        if let Some(open) =
            (self.documents.iter()).find(|d| d.path.as_deref().is_some_and(|p| p.starts_with(from)))
        {
            return Some(tr!(
                "{file_name} is open in a tab; close it before moving the mod holding it",
                file_name = open.file_name()
            ));
        }
        self.no_job_holds(from, tr!("move it")).err()
    }
}
