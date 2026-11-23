//! How a game file or a picture becomes a source of the pack: the one rule of the door, applied to a pack.
//!
//! A pack holds no compiled game file that it makes: a `.ydd` or a `.ytd` that is dropped, picked or put in the place of
//! another is decompiled at once into a source folder, a picture becomes an authored textures source with no DDS until it
//! compiles, and a source folder is taken as it is. A cloth has no source form, so it is the one file kept compiled.
//! The build compiles each source for the platform it states.

use std::path::{Path, PathBuf};

use or_source::create::author_textures;
use or_source::game_import::{decompile, Facts};

use crate::edit::{Copying, Error};
use crate::images;

/// The scratch folder the sources of dropped files are made in, made when the first needs it.
#[derive(Default)]
pub(crate) struct Scratch {
    dir: Option<tempfile::TempDir>,
    made: usize,
}

impl Scratch {
    /// A folder of its own for one source, so two files of one name do not meet.
    pub(crate) fn next(&mut self) -> Result<PathBuf, Error> {
        if self.dir.is_none() {
            let dir = tempfile::tempdir()
                .map_err(|e| Error::Io("the folder the sources are made in".into(), e))?;
            self.dir = Some(dir);
        }
        let root = self.dir.as_ref().map(|d| d.path().to_path_buf());
        let at = root.unwrap_or_default().join(self.made.to_string());
        self.made += 1;
        std::fs::create_dir_all(&at).map_err(|e| Error::Io(at.display().to_string(), e))?;
        Ok(at)
    }
}

/// What a file of the pack is made of, as the digest of the game file or picture it came from: a file's own bytes, and a source's
/// origin, else the one picture it holds.
pub(crate) fn content_digest(path: &Path) -> Option<String> {
    if !path.is_dir() {
        return crate::edit::digest_of(path).ok();
    }
    let source = or_source::asset::Source::open(path).ok()?;
    if let Some(sha) = source.manifest.origin.sha256 {
        return Some(sha);
    }
    let mut pictures = std::fs::read_dir(path.join(or_source::textures::DIR))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file());
    let (one, more) = (pictures.next()?, pictures.next());
    more.is_none()
        .then(|| crate::edit::digest_of(&one).ok())
        .flatten()
}

fn has_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(wanted))
}

fn named(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The game file at `path` as a source folder in a scratch folder.
pub(crate) fn decompiled(scratch: &mut Scratch, path: &Path) -> Result<PathBuf, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::Io(path.display().to_string(), e))?;
    let at = scratch.next()?;
    decompile(&bytes, &named(path), &at, &Facts::default()).map_err(|why| {
        Error::Refused(format!(
            "{} could not be turned into a source: {why}",
            named(path)
        ))
    })
}

/// The name a texture has inside its dictionary: the file's own, without the collection a pack's file names start with.
pub(crate) fn texture_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    stem.rsplit('^').next().unwrap_or(&stem).to_string()
}

/// The picture at `path` as an authored textures source in a scratch folder, called as the picture is and holding a texture named as a texture of a pack is.
fn authored(scratch: &mut Scratch, path: &Path) -> Result<PathBuf, Error> {
    let inner = texture_name(path);
    let called = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let at = scratch.next()?;
    author_textures(
        &at,
        &called,
        &[(path.to_path_buf(), format!("{inner}.{extension}"))],
    )
    .and_then(|made| {
        // ⚠ One compile in the scratch folder, kept nowhere: a picture that would compile to no texture is refused here, with the sentence of the compile.
        let source = or_source::asset::Source::open(&made.path).map_err(|p| p.to_string())?;
        or_source::textures::compile(&source).map_err(|problems| {
            (problems.iter())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        Ok(made.path)
    })
    .map_err(|why| {
        Error::Refused(format!(
            "{} could not be turned into a source: {why}",
            named(path)
        ))
    })
}

/// The model `source` as a path inside the pack: a `.ydd` decompiled, or a `.drawables.src` folder taken as it is.
pub(crate) fn model(
    copying: &mut Copying,
    scratch: &mut Scratch,
    source: &Path,
) -> Result<String, Error> {
    if source.is_dir() {
        let said = or_source::cook::describe(source).map_err(Error::Refused)?;
        if said.kind != or_source::models::DRAWABLES {
            return Err(Error::Refused(format!(
                "{} is a {} source, and a model is a drawables source",
                source.display(),
                said.kind
            )));
        }
        return copying.put_tree_in("models", source, true);
    }
    if !has_extension(source, "ydd") {
        return Err(Error::Refused(
            "a model is a .ydd file, or a .drawables.src folder".into(),
        ));
    }
    let folder = decompiled(scratch, source)?;
    copying.put_tree_in("models", &folder, false)
}

/// The texture `source` as a path inside the pack: a `.ytd` decompiled, a picture made an authored textures source, or a textures source taken as it is.
pub(crate) fn texture(
    copying: &mut Copying,
    scratch: &mut Scratch,
    source: &Path,
) -> Result<String, Error> {
    if source.is_dir() {
        let said = or_source::cook::describe(source).map_err(Error::Refused)?;
        if said.kind != or_source::textures::KIND {
            return Err(Error::Refused(format!(
                "{} is a {} source, and a texture is a textures source",
                source.display(),
                said.kind
            )));
        }
        return copying.put_tree_in("textures", source, true);
    }
    let folder = match (has_extension(source, "ytd"), images::is_image(source)) {
        (true, _) => decompiled(scratch, source)?,
        (_, true) => authored(scratch, source)?,
        _ => {
            return Err(Error::Refused(
                "a texture is a .ytd file, a picture or a textures source".into(),
            ))
        }
    };
    copying.put_tree_in("textures", &folder, false)
}
