//! The pictures a texture is made from: which files are one, by the extensions `or_io` reads.
//!
//! There is no convert button. A picture dropped as a texture becomes an authored textures source of its garment
//! ([`crate::intake`]); its compile is the build's.

use std::path::Path;

use or_io::authoring::texture_import::SourceFormat;

/// Whether `path` is an image a texture compiles from, by its extension: the one list `or_io` reads, a `.ytd` not among it.
pub(crate) fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SourceFormat::from_extension(e).is_some())
}
