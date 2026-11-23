//! The files a case drops on the Wardrobe: written under `drop/`, a model and a texture as small valid game files, since the door decompiles them.

use std::path::{Path, PathBuf};

/// Write each of `names` under `dir/drop`; the words stand for the file, so equal words are equal bytes ([`or_source::sample::as_a_game_file`]).
pub(super) fn drop_files(dir: &Path, names: &[(&str, &[u8])]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|(name, body)| {
            let path = dir.join("drop").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, or_source::sample::as_a_game_file(name, body)).unwrap();
            path
        })
        .collect()
}
