//! The files of a build read back by the path each has in the plan, whatever the build is: a folder, a pack or a package.
//!
//! A kind of content checks what only it knows of what a build wrote (the clothing engine reads
//! its metadata back) and must not care whether the file is loose, inside the archives of a
//! singleplayer DLC or inside the zip of an `.oiv`.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use or_io::container::rpf::RpfArchive;
use or_title::oiv::{PackageReader, CONTENT};

use crate::container::OutputKind;
use crate::dlc::{archive_of, PACK};
use crate::oiv::{pack_root, EXTENSION, PACKS};
use crate::plan::Plan;

/// What a build wrote, read by the path a file has in its plan.
pub trait BuiltFiles {
    /// The bytes of the file at `path`, or why they cannot be read.
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
}

/// A resource written as a folder: `path` is below it.
pub struct Folder<'a>(pub &'a Path);

impl BuiltFiles for Folder<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(self.0.join(path)).map_err(|e| e.to_string())
    }
}

/// The `dlc.rpf` of a build, with the archives inside it opened once each.
struct Pack {
    root: RpfArchive,
    inside: Mutex<HashMap<String, Arc<RpfArchive>>>,
}

impl Pack {
    fn read_in(&self, held: &str, archive: &RpfArchive, path: &str) -> Result<Vec<u8>, String> {
        if let Some(nested) = archive_of(path) {
            let whole = if held.is_empty() {
                nested.to_string()
            } else {
                format!("{held}/{nested}")
            };
            let opened = self.opened(&whole, archive, nested)?;
            return self.read_in(&whole, &opened, &path[nested.len() + 1..]);
        }
        read_node(archive, path)
    }

    fn opened(
        &self,
        whole: &str,
        archive: &RpfArchive,
        nested: &str,
    ) -> Result<Arc<RpfArchive>, String> {
        let mut held = self.inside.lock().map_err(|e| e.to_string())?;
        if let Some(opened) = held.get(whole) {
            return Ok(opened.clone());
        }
        let bytes = read_node(archive, nested)?;
        let opened = Arc::new(RpfArchive::from_bytes(bytes).map_err(|e| e.to_string())?);
        held.insert(whole.to_string(), opened.clone());
        Ok(opened)
    }
}

impl BuiltFiles for Pack {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.read_in("", &self.root, path)
    }
}

/// The bytes of the file `path` of `archive`; the archive lower-cases every name it stores.
fn read_node(archive: &RpfArchive, path: &str) -> Result<Vec<u8>, String> {
    let wanted = path.to_lowercase();
    let node = (archive.nodes.iter())
        .filter(|node| !node.is_dir())
        .find(|node| {
            node.path()
                .replace('\\', "/")
                .trim_start_matches('/')
                .eq_ignore_ascii_case(&wanted)
        })
        .ok_or_else(|| format!("`{path}` is not in the archive"))?;
    archive
        .read_entry(node.entry_index())
        .map_err(|e| e.to_string())
}

/// The `.oiv` of a build.
struct Package {
    reader: Mutex<PackageReader<BufReader<std::fs::File>>>,
    /// Where it installs a pack it adds: the paths of that pack are read below it.
    root: Option<String>,
}

impl BuiltFiles for Package {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let mut reader = self.reader.lock().map_err(|e| e.to_string())?;
        let packed = self.root.as_ref().map(|root| format!("{root}/{path}"));
        match packed.map(|at| reader.read(&at)) {
            Some(Ok(bytes)) => Ok(bytes),
            _ => reader.read(path).map_err(|e| e.to_string()),
        }
    }
}

/// The files of the build of `plan` that is in `folder`, opened as the kind of build it is.
pub fn built_files(plan: &Plan, folder: &Path) -> Result<Box<dyn BuiltFiles>, String> {
    match plan.kind {
        OutputKind::SingleplayerDlc => {
            let at: PathBuf = folder.join(PACK);
            let root = RpfArchive::open(&at).map_err(|e| format!("`{PACK}` does not open: {e}"))?;
            Ok(Box::new(Pack {
                root,
                inside: Mutex::default(),
            }))
        }
        OutputKind::OivPackage => {
            let name = format!("{}.{EXTENSION}", plan.resource);
            let file = std::fs::File::open(folder.join(&name))
                .map_err(|e| format!("`{name}` does not open: {e}"))?;
            let reader = PackageReader::new(BufReader::new(file))
                .map_err(|e| format!("`{name}` does not open: {e}"))?;
            Ok(Box::new(Package {
                reader: Mutex::new(reader),
                root: plan
                    .dlclist_entry
                    .as_ref()
                    .map(|_| pack_root(&plan.resource)),
            }))
        }
        _ => Ok(Box::new(OwnedFolder(folder.to_path_buf()))),
    }
}

/// A resource folder that is held by the box rather than borrowed.
struct OwnedFolder(PathBuf);

impl BuiltFiles for OwnedFolder {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Folder(&self.0).read(path)
    }
}

/// What a built pack or package is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltKind {
    /// A `dlc.rpf`.
    Dlc,
    /// An `.oiv`.
    Oiv,
}

/// What `unpack` hands each file to: its path, whether it is the pack's, and its bytes.
pub type Each<'a> = dyn FnMut(&str, bool, Vec<u8>) -> Result<(), String> + 'a;

/// Hand every file of the archive `archive` (held at `prefix` in the pack) to `each`, the archives inside it opened.
///
/// ⚠ An archive that does not open is handed over as the file it is, so nothing in it is lost without being named.
fn walk(archive: &RpfArchive, prefix: &str, each: &mut Each) -> Result<(), String> {
    for node in archive.nodes.iter().filter(|node| !node.is_dir()) {
        let name = node.path().replace('\\', "/");
        let name = name.trim_start_matches('/');
        let path = match prefix.is_empty() {
            true => name.to_string(),
            false => format!("{prefix}/{name}"),
        };
        let bytes = archive
            .read_entry(node.entry_index())
            .map_err(|e| format!("{path}: {e}"))?;
        if name.to_ascii_lowercase().ends_with(".rpf") {
            if let Ok(inner) = RpfArchive::from_bytes(bytes.clone()) {
                walk(&inner, &path, each)?;
                continue;
            }
        }
        each(&path, true, bytes)?;
    }
    Ok(())
}

/// The path of a file of a package inside the pack the package adds, none for a file that is not the pack's.
fn inside_pack(installed: &str) -> Option<&str> {
    let (_, rest) = installed
        .strip_prefix(PACKS)?
        .strip_prefix('/')?
        .split_once('/')?;
    rest.strip_prefix(&format!("{PACK}/"))
}

/// Hand every file of a built pack to `each`, one at a time: `built` is a `dlc.rpf`, an `.oiv`, or the folder that holds one.
///
/// A file of a `dlc.rpf` is the pack's, by the path it has in the pack with the archives it is in joined by `/`. A file of an
/// `.oiv` is the pack's when the package installs it inside the `dlc.rpf` it adds, and then its path is the pack's own;
/// any other file is told by the path it installs to. The manifest is no file.
pub fn unpack(built: &Path, each: &mut Each) -> Result<BuiltKind, String> {
    let file = if built.is_dir() {
        let found = |ext: &str| {
            let mut names: Vec<PathBuf> = std::fs::read_dir(built)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
                .collect();
            names.sort();
            names.into_iter().next()
        };
        let pack = built.join(PACK);
        match (pack.is_file(), found(EXTENSION)) {
            (true, _) => pack,
            (false, Some(package)) => package,
            (false, None) => {
                return Err(format!(
                    "{} holds no {PACK} and no .{EXTENSION} package",
                    built.display()
                ))
            }
        }
    } else {
        built.to_path_buf()
    };
    let is_package = file
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION));
    let shown = file.display();
    // ⚠ One sentence a person can act on, then a line break and the reader's own words, which a screen puts in a hover: the library's jargon and the whole path are not what the person needs first.
    let leaf = file
        .file_name()
        .map_or_else(|| shown.to_string(), |n| n.to_string_lossy().into_owned());
    let refused = |what: &str, e: &dyn std::fmt::Display| {
        format!(
            "{leaf} is not {what}, so nothing was made. Pick the file the pack was built as, or get it again.\n{shown}: {e}"
        )
    };
    if !is_package {
        let root = RpfArchive::open(&file)
            .map_err(|e| refused("a singleplayer DLC archive OpenRage can read", &e))?;
        walk(&root, "", each)?;
        return Ok(BuiltKind::Dlc);
    }
    let opened = std::fs::File::open(&file).map_err(|e| {
        format!("{leaf} could not be read, so nothing was made. Check that the file is still there.\n{shown}: {e}")
    })?;
    let mut reader = PackageReader::new(BufReader::new(opened))
        .map_err(|e| refused("a complete OIV package", &e))?;
    let prefix = format!("{CONTENT}/");
    for entry in reader.entries() {
        let Some(installed) = entry
            .strip_prefix(&prefix)
            .filter(|p| !p.is_empty() && !p.ends_with('/'))
        else {
            continue;
        };
        let bytes = reader
            .read(installed)
            .map_err(|e| format!("{installed}: {e}"))?;
        match inside_pack(installed) {
            Some(path) => each(path, true, bytes)?,
            None => each(installed, false, bytes)?,
        }
    }
    Ok(BuiltKind::Oiv)
}
