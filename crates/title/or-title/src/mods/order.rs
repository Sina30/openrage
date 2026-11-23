use super::InstalledMod;

/// Put `mods` in the load order `saved` names, a mod it has not met after them, by name.
///
/// ⚠ **An unmet mod stacks last, so it wins.** A mod the user just added is
/// the one they expect to take effect.
pub fn arrange(mods: &mut [InstalledMod], saved: &[String]) {
    mods.sort_by_cached_key(|m| {
        let rank = if m.place.is_ordered() {
            (saved.iter())
                .position(|s| s.eq_ignore_ascii_case(&m.name))
                .unwrap_or(saved.len())
        } else {
            usize::MAX
        };
        (rank, m.name.to_ascii_lowercase())
    });
}

/// The names to save: every ordered mod, parked ones too, so one that comes back keeps its place.
pub fn order_of(mods: &[InstalledMod]) -> Vec<String> {
    mods.iter()
        .filter(|m| m.place.is_ordered())
        .map(|m| m.name.clone())
        .collect()
}

/// Swap `name` with the nearest mod in the same place above or below it; `false` when it is already at that end.
///
/// ⚠ **An enabled mod swaps with an enabled one, never a parked one**: a swap
/// with a parked neighbour would change the saved order and nothing the
/// stack resolves.
pub fn nudge(mods: &mut [InstalledMod], name: &str, up: bool) -> bool {
    let Some(at) = mods
        .iter()
        .position(|m| m.place.is_ordered() && m.name == name)
    else {
        return false;
    };
    let place = mods[at].place;
    let other = if up {
        mods[..at].iter().rposition(|m| m.place == place)
    } else {
        (mods[at + 1..].iter().position(|m| m.place == place)).map(|i| at + 1 + i)
    };
    let Some(other) = other else {
        return false;
    };
    mods.swap(at, other);
    true
}
