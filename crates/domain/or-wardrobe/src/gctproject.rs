//! grzyClothTool's project file, the one foreign project format the Wardrobe reads: a zip, every byte XORed with `0x2B`, that holds a resource.
//!
//! The resource is reopened as a project of sources exactly as a built pack is ([`crate::reopen`]). The folder the resource sits
//! in inside the zip, when every file shares one, is not part of the paths. A file whose bytes are not that zip is refused with
//! the reason and nothing is made.
//!
//! ⚠ **The format is held to its description, not to a file of the tool's:** none is staged, so what a real project file holds
//! that is not a clothing resource (a settings file of the tool's own, say) is listed as not placed and kept, never read.

use std::io::{Cursor, Read};
use std::path::Path;

use or_project::OutputKind;
use zip::ZipArchive;

use crate::edit::Watch;
use crate::reopen::{Door, Error, Options, Reopened};

/// What every byte of the file is XORed with.
pub const XOR: u8 = 0x2B;

/// The extension of the file.
pub const EXTENSION: &str = "gctproject";

/// The folder every file of `names` is in, when they all are.
fn shared_folder(names: &[String]) -> Option<String> {
    let first = names.first()?.split_once('/')?.0;
    names
        .iter()
        .all(|n| n.split_once('/').is_some_and(|(top, _)| top == first))
        .then(|| format!("{first}/"))
}

/// Reopen the project file `file` as a project in `into`.
pub fn reopen(file: &Path, into: &Path, options: &Options) -> Result<Reopened, Error> {
    reopen_watched(file, into, options, &Watch::default())
}

/// [`reopen`] that counts the files of the project it has read and, once `watch` is asked to stop, makes nothing.
pub fn reopen_watched(
    file: &Path,
    into: &Path,
    options: &Options,
    watch: &Watch,
) -> Result<Reopened, Error> {
    let mut bytes = std::fs::read(file).map_err(|e| Error::Io(file.display().to_string(), e))?;
    bytes.iter_mut().for_each(|b| *b ^= XOR);
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| {
        // ⚠ A sentence, then the reader's own words after a line break, which a screen puts in a hover.
        Error::Refused(format!(
            "{} is not a grzyClothTool project, so nothing was made. Pick the file the project was saved as.\n{}: its bytes XORed with 0x2B are not a zip ({e})",
            file.file_name().map_or_else(
                || file.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            ),
            file.display()
        ))
    })?;
    let mut names = Vec::new();
    for n in 0..zip.len() {
        match zip.by_index(n) {
            Ok(entry) if !entry.is_dir() => names.push(entry.name().replace('\\', "/")),
            _ => {}
        }
    }
    let top = shared_folder(&names).unwrap_or_default();
    let mut door = Door::new(watch)?;
    door.total = names.len() as u64;
    for n in 0..zip.len() {
        let mut entry = zip
            .by_index(n)
            .map_err(|e| Error::Refused(format!("entry {n} of the project does not read: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        // ⚠ A path that leaves the folder is never written.
        let path = match entry.enclosed_name() {
            Some(_) => name.strip_prefix(&top).unwrap_or(&name).to_string(),
            None => {
                door.skip(&name, "its path leaves the folder, so it is not read");
                continue;
            }
        };
        let mut body = Vec::new();
        entry
            .read_to_end(&mut body)
            .map_err(|e| Error::Refused(format!("{name} does not read: {e}")))?;
        door.take(&path, true, body)
            .map_err(|why| door.refusal(why))?;
    }
    door.finish(into, OutputKind::FivemResource, options)
}
