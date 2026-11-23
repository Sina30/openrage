//! The sprite of each sprite and trail rule of a scanned GTA VI particle list, kept by the list's
//! path for the preview to draw.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::gta6::headerless::ParticleEffects;

/// The slot of a single-channel sprite, which the ramp of `RAMP_SLOT` colours.
pub(crate) const SHAPE_SLOT: usize = 6;
/// The slot of the colour ramp a `SHAPE_SLOT` sprite is coloured through.
const RAMP_SLOT: usize = 7;

/// A rule's sprite: its texture key, slot and frames, and the key of the ramp that colours slot 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sprite {
    pub key: u64,
    pub slot: usize,
    pub frames: u32,
    pub ramp: Option<u64>,
}

/// Each rule's sprite, by the rule's name.
pub(crate) type Sprites = BTreeMap<String, Sprite>;

// ponytail: one entry per scanned list opened, never emptied, as its lines are.
static KEPT: Mutex<BTreeMap<PathBuf, Arc<Sprites>>> = Mutex::new(BTreeMap::new());

fn kept() -> MutexGuard<'static, BTreeMap<PathBuf, Arc<Sprites>>> {
    KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The sprites of `list`'s rules that draw no model and set a sprite slot.
pub(crate) fn of_list(list: &ParticleEffects) -> Sprites {
    (list.particles.iter())
        .filter(|p| p.draw_type != 1)
        .filter_map(|p| {
            let slot = p.sprite_slot()?;
            let ramp = (slot == SHAPE_SLOT).then(|| p.texture_keys.get(RAMP_SLOT).copied());
            let sprite = Sprite {
                key: p.texture_keys.get(slot).copied()?,
                slot,
                frames: list.frame_count(p),
                ramp: ramp.flatten().filter(|&key| key != 0),
            };
            Some((p.name.clone(), sprite))
        })
        .collect()
}

/// Keep the sprites of `list`, opened from `path`.
pub(super) fn keep(path: &Path, list: &ParticleEffects) {
    kept().insert(path.to_path_buf(), Arc::new(of_list(list)));
}

/// The sprites of the GTA VI list the document at `path` opened from; `None` for any other.
pub(crate) fn of(path: &Path) -> Option<Arc<Sprites>> {
    kept().get(path).cloned()
}
