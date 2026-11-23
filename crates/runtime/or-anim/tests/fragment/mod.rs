//! A `.yft` opened owned, and the skeleton of its common drawable.

use std::path::Path;

use or_io::gta5::CrSkeletonData;
use or_io::Asset;

/// The skeleton of the common drawable of the fragment at `path`, at either target.
pub fn skeleton(path: &Path) -> Option<CrSkeletonData> {
    match or_io::open(path).ok()? {
        Asset::Fragment(f) => f
            .common_drawable
            .as_deref()?
            .base
            .skeleton
            .as_deref()
            .cloned(),
        Asset::FragmentGen9(f) => f
            .common_drawable
            .as_deref()?
            .base
            .skeleton
            .as_deref()
            .cloned(),
        _ => None,
    }
}
