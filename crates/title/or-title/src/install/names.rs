//! Which completion names an install holds, and what an install cannot hold.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::vfs::MountedGame;

/// A category of name a metadata field's picker completes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InstalledNameKind {
    Model,
    Texture,
    ClipSet,
}

impl InstalledNameKind {
    pub const ALL: &'static [Self] = &[Self::Model, Self::Texture, Self::ClipSet];
}

/// The whole of the classification: a file's extension decides which category
/// its stem belongs to.
///
/// ⚠ An effect **rule** name is deliberately absent and cannot be added here.
/// A rule name is a string *inside* a `.ypt`, not the stem of one, so no
/// listing of an archive can supply it. Whatever offers that category has to
/// read a document.
pub const NAME_KIND_BY_EXTENSION: &[(&str, InstalledNameKind)] = &[
    ("ydr", InstalledNameKind::Model),
    ("ydd", InstalledNameKind::Model),
    ("yft", InstalledNameKind::Model),
    ("ytd", InstalledNameKind::Texture),
    ("ycd", InstalledNameKind::ClipSet),
];

/// How many names accumulate before the scan hands a batch to its caller.
pub const DEFAULT_BATCH: usize = 512;

/// The category and stem of a leaf name, or `None` if it completes nothing.
pub fn classify_leaf(leaf: &str) -> Option<(InstalledNameKind, &str)> {
    let (stem, extension) = leaf.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    let extension = extension.to_ascii_lowercase();
    let kind = NAME_KIND_BY_EXTENSION
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map(|(_, kind)| *kind)?;
    Some((kind, stem))
}

/// Accumulates classified names and hands them over a batch at a time.
pub struct NameBatcher<'c, F> {
    batch: Vec<(InstalledNameKind, String)>,
    size: usize,
    cancelled: &'c AtomicBool,
    emit: F,
}

impl<'c, F> NameBatcher<'c, F>
where
    F: FnMut(Vec<(InstalledNameKind, String)>),
{
    pub fn new(cancelled: &'c AtomicBool, size: usize, emit: F) -> Self {
        let size = size.max(1);
        Self {
            batch: Vec::with_capacity(size),
            size,
            cancelled,
            emit,
        }
    }

    pub fn offer(&mut self, leaf: &str) {
        if self.cancelled.load(Ordering::Relaxed) {
            return;
        }
        let Some((kind, stem)) = classify_leaf(leaf) else {
            return;
        };
        self.batch.push((kind, stem.to_string()));
        if self.batch.len() >= self.size {
            let full = std::mem::replace(&mut self.batch, Vec::with_capacity(self.size));
            (self.emit)(full);
        }
    }

    pub fn finish(mut self) {
        if self.batch.is_empty() || self.cancelled.load(Ordering::Relaxed) {
            return;
        }
        let last = std::mem::take(&mut self.batch);
        (self.emit)(last);
    }
}

/// Walk a mounted install and hand `emit` the completion names it holds.
///
/// ⚠ **Cancellation stops the naming, not the walking.** `MountedGame`'s walk
/// has no early exit, so a cancelled scan still descends the archives it has
/// left; what it stops doing is classifying, allocating and emitting. The
/// thread therefore ends on its own rather than at once.
pub fn scan_install_names<F>(game: &MountedGame, cancelled: &AtomicBool, batch_size: usize, emit: F)
where
    F: FnMut(Vec<(InstalledNameKind, String)>),
{
    let mut batcher = NameBatcher::new(cancelled, batch_size, emit);
    game.for_each_file(|_logical, _container, leaf| batcher.offer(leaf));
    batcher.finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(leaves: &[&str], size: usize) -> Vec<Vec<(InstalledNameKind, String)>> {
        let cancelled = AtomicBool::new(false);
        let mut out = Vec::new();
        {
            let mut batcher = NameBatcher::new(&cancelled, size, |b| out.push(b));
            for leaf in leaves {
                batcher.offer(leaf);
            }
            batcher.finish();
        }
        out
    }

    #[test]
    fn a_leaf_is_classified_by_its_extension_and_keeps_its_stem() {
        assert_eq!(
            classify_leaf("prop_bench_01a.ydr"),
            Some((InstalledNameKind::Model, "prop_bench_01a"))
        );
        assert_eq!(
            classify_leaf("mp_m_freemode_01.ydd"),
            Some((InstalledNameKind::Model, "mp_m_freemode_01"))
        );
        assert_eq!(
            classify_leaf("adder.yft"),
            Some((InstalledNameKind::Model, "adder"))
        );
        assert_eq!(
            classify_leaf("vehshare.ytd"),
            Some((InstalledNameKind::Texture, "vehshare"))
        );
        assert_eq!(
            classify_leaf("move_m@generic.ycd"),
            Some((InstalledNameKind::ClipSet, "move_m@generic"))
        );
    }

    #[test]
    fn an_extension_is_matched_whatever_its_case() {
        assert_eq!(
            classify_leaf("PROP_BENCH.YDR"),
            Some((InstalledNameKind::Model, "PROP_BENCH")),
            "an archive leaf arrives in the case the packer wrote it, and the \
             stem keeps that case — normalising is the index's job"
        );
    }

    #[test]
    fn a_leaf_that_completes_nothing_is_not_offered() {
        assert_eq!(classify_leaf("common.rpf"), None);
        assert_eq!(classify_leaf("vehicles.meta"), None);
        assert_eq!(classify_leaf("noextension"), None);
        assert_eq!(classify_leaf(".ydr"), None, "an empty stem names nothing");
    }

    /// A `.ypt`'s stem is its ptfx asset name, never an effect rule inside it,
    /// so classifying it as one would fill the picker with the wrong strings.
    #[test]
    fn a_particle_file_supplies_no_effect_rule_name() {
        assert_eq!(classify_leaf("core.ypt"), None);
        assert!(
            !NAME_KIND_BY_EXTENSION.iter().any(|(ext, _)| *ext == "ypt"),
            "an install listing cannot see inside a .ypt"
        );
    }

    #[test]
    fn every_category_the_scan_offers_has_at_least_one_extension() {
        for kind in InstalledNameKind::ALL {
            assert!(
                NAME_KIND_BY_EXTENSION.iter().any(|(_, k)| k == kind),
                "{kind:?} is a category this scan offers and nothing produces it"
            );
        }
    }

    #[test]
    fn a_batch_is_handed_over_as_soon_as_it_fills() {
        let batches = drain(
            &["a.ydr", "b.ydr", "readme.txt", "c.ytd", "d.ycd", "e.ydd"],
            2,
        );
        assert_eq!(
            batches.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![2, 2, 1],
            "an unclassifiable leaf must not count towards a batch"
        );
        let flat: Vec<&str> = batches
            .iter()
            .flatten()
            .map(|(_, name)| name.as_str())
            .collect();
        assert_eq!(flat, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn a_cancelled_scan_emits_nothing_further() {
        let cancelled = AtomicBool::new(false);
        let mut out: Vec<Vec<(InstalledNameKind, String)>> = Vec::new();
        {
            let mut batcher = NameBatcher::new(&cancelled, 8, |b| out.push(b));
            batcher.offer("a.ydr");
            cancelled.store(true, Ordering::Relaxed);
            batcher.offer("b.ydr");
            batcher.finish();
        }
        assert!(
            out.is_empty(),
            "a cancelled scan must not deliver the partial batch it had"
        );
    }
}
