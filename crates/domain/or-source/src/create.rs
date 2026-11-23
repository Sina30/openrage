//! Starting a source of a project: an empty one a person fills, or one made of pictures.
//!
//! A source is platform-free: nothing here asks for Legacy or Enhanced. The manifest names the layout the compile goes
//! through, [`AUTHORED_TARGET`], and a build moves the file it makes to the target the build states.

use std::path::{Path, PathBuf};

use crate::asset::{folder_name, AssetManifest, How, Origin, MANIFEST};
use crate::game_import::{tree_digest, Created, SOURCES_DIR};
use crate::json::canonical;
use crate::{git, textures};

/// The layout an authored source compiles through; the build moves the file to the target it states.
pub const AUTHORED_TARGET: &str = "gen8";

/// A kind of source that can be started empty.
pub struct NewKind {
    pub kind: &'static str,
    /// What a menu calls it: `Texture dictionary`.
    pub label: &'static str,
}

/// Every kind a person can start empty and fill.
pub const KINDS: &[NewKind] = &[NewKind {
    kind: textures::KIND,
    label: "Texture dictionary",
}];

/// Why the other kinds are not started empty: a model, a fragment and a metadata file compile as an edit of a base file of the game, and there is no empty one.
pub const WHY_NOT_THE_REST: &str =
    "a drawable, a drawables, a fragment and a metadata source compile as an edit of a base file, and there is none to start from: import the game file, or a glTF for a model";

/// A new empty source of `kind` called `name` under `sources/` of the project in `root`.
pub fn new_source(root: &Path, kind: &str, name: &str) -> Result<Created, String> {
    match KINDS.iter().find(|k| k.kind == kind) {
        Some(_) => textures_from_images(root, name, &[]),
        None => Err(format!(
            "a {kind} source cannot be started empty: {WHY_NOT_THE_REST}"
        )),
    }
}

/// The name a source made of `images` is given when nobody chose one: the picture's own, or the folder the pictures are in.
pub fn default_name(images: &[PathBuf]) -> String {
    let stem = |name: &str| {
        name.rsplit_once('.')
            .map_or(name.to_string(), |(s, _)| s.to_string())
    };
    let leaf = |path: &Path| path.file_name().map(|n| n.to_string_lossy().into_owned());
    match images {
        [one] => leaf(one).map(|n| stem(&n)),
        _ => images
            .first()
            .and_then(|first| first.parent())
            .and_then(leaf),
    }
    .filter(|name| !name.is_empty())
    .unwrap_or_else(|| "textures".to_string())
}

/// A file and a word about it: the name it has inside a source, or why it is not one.
pub type Named = (PathBuf, String);

/// The pictures of `images` a textures source holds, by name, and each other one with why it is not: not a picture, or named like one before it.
pub fn sort_images(images: &[PathBuf]) -> (Vec<Named>, Vec<Named>) {
    let (mut usable, mut refused) = (Vec::new(), Vec::new());
    let mut taken: Vec<String> = Vec::new();
    for image in images {
        let name = image
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let stem = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem)
            .to_string();
        let why = if !textures::is_texture_file(&name) {
            Some("is not an image or a DDS a textures source compiles from".to_string())
        } else {
            (taken.iter().find(|t| t.eq_ignore_ascii_case(&stem)))
                .map(|first| format!("is named like {first}: a source holds one image of a name"))
        };
        match why {
            Some(reason) => refused.push((image.clone(), reason)),
            None => {
                taken.push(stem);
                usable.push((image.clone(), name));
            }
        }
    }
    (usable, refused)
}

/// A textures source called `name` under `sources/`, holding `images` as they are and no DDS: written as a person authored it.
///
/// With no image it is the empty source New starts. A name taken by another source gets `_2`, `_3` and on.
pub fn textures_from_images(
    root: &Path,
    name: &str,
    images: &[PathBuf],
) -> Result<Created, String> {
    let (usable, refused) = sort_images(images);
    if let Some((image, why)) = refused.first() {
        return Err(format!("{} {why}", image.display()));
    }
    let created = author_textures(&root.join(SOURCES_DIR), name, &usable)?;
    // The ignore rules are the project's, and a failure to update them leaves the source whole.
    let _ = git::update(root);
    Ok(created)
}

/// A textures source called `name` in `parent`, holding the pictures `files` copy in under the name each is given there; no DDS, and no `sources/` folder made around it.
///
/// This is what a picture dropped on a garment becomes: the compile of the source is the build's.
pub fn author_textures(parent: &Path, name: &str, files: &[Named]) -> Result<Created, String> {
    let origin = Origin {
        how: How::Authored,
        from_game: false,
        virtual_path: None,
        game_build: None,
        sha256: None,
        cooked_sha256: None,
        extra: Default::default(),
    };
    let manifest = AssetManifest::new(textures::KIND, name, AUTHORED_TARGET, origin)
        .map_err(|p| p.to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("cannot make {}: {e}", parent.display()))?;
    let (folder, named) = claim(parent, name)?;
    // The folder takes a number and so does the asset: two sources of one name would cook into one file.
    let manifest = AssetManifest {
        name: named,
        ..manifest
    };
    let written = (|| -> Result<(), String> {
        let rules = canonical(&textures::Rules::default()).map_err(|e| e.to_string())?;
        let io = |e: std::io::Error| e.to_string();
        std::fs::create_dir_all(folder.join(textures::DIR)).map_err(io)?;
        std::fs::write(folder.join(MANIFEST), manifest.to_text()).map_err(io)?;
        std::fs::write(folder.join(textures::RULES), rules).map_err(io)?;
        for (path, file) in files {
            std::fs::copy(path, folder.join(textures::DIR).join(file))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        // A picture of 16 bits a channel compiles only with a rule that brings it down, so it is given its whole range, stated.
        let names: Vec<String> = files.iter().map(|(_, file)| file.clone()).collect();
        textures::state_depth_rules(&folder, &names)?;
        Ok(())
    })();
    match written.and_then(|()| tree_digest(&folder).map_err(|e| e.to_string())) {
        Ok(digest) => Ok(Created {
            path: folder,
            digest,
        }),
        Err(why) => {
            let _ = std::fs::remove_dir_all(&folder);
            Err(why)
        }
    }
}

/// The first free folder of `name` under `sources`, made so that two creators never share one; the name it was given.
fn claim(sources: &Path, name: &str) -> Result<(PathBuf, String), String> {
    for n in 1u32.. {
        let named = match n {
            1 => name.to_string(),
            n => format!("{name}_{n}"),
        };
        let candidate = sources.join(folder_name(&named, textures::KIND));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok((candidate, named)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot make {}: {e}", candidate.display())),
        }
    }
    Err("no free name for the source".to_string())
}
