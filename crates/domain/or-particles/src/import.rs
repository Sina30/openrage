//! Adding particle files to a pack: each `.ypt` becomes a source in the module's folder and an asset of the model.
//!
//! The door is the one every game file comes in by (`or_source::game_import::decompile`), so a `.ypt` that the source form cannot
//! hold is refused with the sentence that says why, and nothing of it is kept. A new, empty list is made the same way.

use std::path::{Path, PathBuf};

use or_io::decl::{File, Gen8};
use or_io::gta5::particle::PtxFxList;
use or_source::game_import::{self, Facts as Origin};

use crate::model::{Asset, Particles, FILE};

/// A file to add.
#[derive(Debug, Clone)]
pub struct Incoming<'a> {
    /// The name it has in the game, with its extension.
    pub name: &'a str,
    pub bytes: &'a [u8],
    pub from_game: bool,
    /// Its path inside the game's archives, when it came out of one.
    pub virtual_path: Option<&'a str>,
}

/// What became of the files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Added {
    /// The source folders made.
    pub made: Vec<PathBuf>,
    /// The files that were not added, each with why.
    pub refused: Vec<(String, String)>,
}

fn model_of(folder: &Path) -> Result<Particles, String> {
    if folder.join(FILE).is_file() {
        Particles::read(folder).map_err(|e| e.to_string())
    } else {
        Ok(Particles::new())
    }
}

/// The path of `dir` below `folder` with `/` between names.
fn relative(folder: &Path, dir: &Path) -> String {
    dir.strip_prefix(folder)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| dir.display().to_string())
}

/// Make a source of each of `incoming` in `folder` and an asset of each source the pack did not already name.
pub fn add_files(folder: &Path, incoming: &[Incoming<'_>]) -> Result<Added, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut model = model_of(folder)?;
    let mut added = Added::default();
    for file in incoming {
        if !file.name.to_ascii_lowercase().ends_with(".ypt") {
            added.refused.push((
                file.name.to_string(),
                "is not a particle file; a particles pack holds `.ypt` files".to_string(),
            ));
            continue;
        }
        let made = game_import::decompile(
            file.bytes,
            file.name,
            folder,
            &Origin {
                from_game: file.from_game,
                virtual_path: file.virtual_path.map(str::to_string),
                ..Origin::default()
            },
        );
        match made {
            Ok(dir) => {
                let source = relative(folder, &dir);
                if !model.assets.iter().any(|a| a.source == source) {
                    model.assets.push(Asset::new(&source));
                }
                added.made.push(dir);
            }
            Err(why) => added.refused.push((file.name.to_string(), why)),
        }
    }
    if !added.made.is_empty() {
        model.write(folder).map_err(|e| e.to_string())?;
    }
    Ok(added)
}

/// Make an empty list called `name` a source in `folder` and an asset of the pack; the folder of the source.
pub fn new_asset(folder: &Path, name: &str) -> Result<PathBuf, String> {
    let list = PtxFxList::<Gen8>::default();
    let bytes = list
        .to_bytes()
        .map_err(|e| format!("an empty list cannot be written: {e}"))?;
    let file = format!("{name}.ypt");
    let added = add_files(
        folder,
        &[Incoming {
            name: &file,
            bytes: &bytes,
            from_game: false,
            virtual_path: None,
        }],
    )?;
    match (
        added.made.into_iter().next(),
        added.refused.into_iter().next(),
    ) {
        (Some(dir), _) => Ok(dir),
        (None, Some((_, why))) => Err(why),
        (None, None) => Err("nothing was made".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_list_becomes_a_source_and_an_asset_and_a_file_that_is_no_ypt_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let source = new_asset(dir.path(), "fx_new").unwrap();
        assert!(source.ends_with("fx_new.particles.src"));
        let model = Particles::read(dir.path()).unwrap();
        assert_eq!(model.assets.len(), 1);
        assert_eq!(model.assets[0].source, "fx_new.particles.src");

        // The same name again is refused: an import only adds.
        assert!(new_asset(dir.path(), "fx_new").is_err());
        let added = add_files(
            dir.path(),
            &[Incoming {
                name: "notes.txt",
                bytes: b"x",
                from_game: false,
                virtual_path: None,
            }],
        )
        .unwrap();
        assert!(added.made.is_empty() && added.refused.len() == 1);
        assert_eq!(Particles::read(dir.path()).unwrap(), model);
    }
}
