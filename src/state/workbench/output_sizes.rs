//! What the files each clothing content writes for one build weigh, as that build's own options plan them.

use std::path::Path;

use or_project::{Container, ModuleKind, Output};
use or_wardrobe::sizes::Sizes;
use or_wardrobe::Depth;

use super::wardrobe::Pack;

/// The sizes of the clothing contents `output` takes, by content.
///
/// ⚠ Runs on the scan's worker. A build with the default options gets the sizes its packs were already planned with; one with options of
/// its own is planned again for itself, because a tag rule or another layout changes which files it writes.
pub(super) fn of_output(
    container: &Container,
    folder: &Path,
    output: &Output,
    packs: &[Pack],
) -> Vec<(String, Sizes)> {
    let standard = or_wardrobe::is_standard(output);
    (container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Clothing && output.take.contains(&m.id))
        .filter_map(|module| {
            let sizes = match standard {
                true => (packs.iter().find(|p| p.module == module.id))?
                    .read
                    .as_ref()
                    .ok()?
                    .sizes
                    .clone(),
                false => {
                    let (wardrobe, planned) =
                        or_wardrobe::planned_for(folder, module, Depth::Measure, output).ok()?;
                    or_wardrobe::sizes::sizes(&wardrobe, &planned, &[])
                }
            };
            Some((module.id.clone(), sizes))
        })
        .collect()
}
