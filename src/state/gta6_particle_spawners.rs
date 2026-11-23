//! The third spawner of each rule of a scanned GTA VI particle list, kept by the list's path for
//! the preview to start where the rule's `ptxu_ZCull` culls a point.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::gta6::headerless::ParticleEffects;
use or_simulation::zcull_spawner::ZCullSpawners;

// ponytail: one entry per scanned list opened, never emptied, as its lines are.
static KEPT: Mutex<BTreeMap<PathBuf, Arc<ZCullSpawners>>> = Mutex::new(BTreeMap::new());

fn kept() -> MutexGuard<'static, BTreeMap<PathBuf, Arc<ZCullSpawners>>> {
    KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Each rule's third spawner that starts an effect, as a GTA V spawner naming the effect.
pub(crate) fn of_list(list: &ParticleEffects) -> ZCullSpawners {
    (list.particles.iter())
        .filter_map(|p| {
            let third = &p.spawners[2];
            let effect = list.effects.get(third.effect?)?;
            let mut spawner = super::spawner(third);
            spawner.name = effect.name.clone();
            Some((p.name.clone(), spawner))
        })
        .collect()
}

/// Keep the third spawners of `list`, opened from `path`.
pub(super) fn keep(path: &Path, list: &ParticleEffects) {
    kept().insert(path.to_path_buf(), Arc::new(of_list(list)));
}

/// The third spawners of the GTA VI list the document at `path` opened from; `None` for any other.
pub(crate) fn of(path: &Path) -> Option<Arc<ZCullSpawners>> {
    kept().get(path).cloned()
}
