//! A whole pack as one file: its sources and its settings in a zip that another machine imports as an equal pack.
//!
//! The file holds `wardrobe.json`, every model and texture as a source folder, the cloth files (which have no source form) and
//! the shop files the pack carries, and never a compiled game file: a `.ydd` or a `.ytd` the pack names is decompiled on the way
//! out, and a reference is followed to what it leads to, as for [`crate::package`] of one garment. The lock of the last build and
//! the private folder of this machine stay behind. The zip is deterministic: a pack of sources that did not change is a file that
//! did not change (a game file the pack still names is decompiled again on each export, into a source with an id of its own). An
//! import refuses a name that leaves the folder it fills and a compiled game file.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::edit::{Copying, Error};
use crate::facts::{named_paths, stays_inside};
use crate::intake::Scratch;
use crate::model::{Wardrobe, FILE};
use crate::package::gather;

/// The extension of the file.
pub const EXTENSION: &str = "openrage-pack";

/// The first entry of the zip, which says it is a pack package and of what version.
pub const HEADER: &str = "openrage-pack.json";

const FORMAT: &str = "openrage-pack";
const SCHEMA: u32 = 1;

/// The largest an entry is allowed to unpack to, and the largest the whole is: a package is files of a pack, not a bomb.
const MAX_ENTRY: u64 = 4 << 30;
const MAX_TOTAL: u64 = 32 << 30;

fn refused(why: impl Into<String>) -> Error {
    Error::Refused(why.into())
}

fn io(what: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |e| Error::Io(what.display().to_string(), e)
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub garments: usize,
    /// The files of the pack in the zip, `wardrobe.json` among them.
    pub files: usize,
    /// What they unpack to.
    pub bytes: u64,
}

/// What an import made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// The name of the pack.
    pub pack: String,
    pub garments: usize,
    pub files: usize,
}

fn header() -> String {
    format!("{{\n  \"format\": \"{FORMAT}\",\n  \"schema\": {SCHEMA}\n}}\n")
}

/// Every file under `dir` by its `/` path.
fn files_under(dir: &Path) -> Result<Vec<(String, PathBuf)>, Error> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), Error> {
        for entry in std::fs::read_dir(dir).map_err(io(dir))?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push((rel.to_string_lossy().replace('\\', "/"), path));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// Write the pack in `pack` as the package `file`, replacing a file that is there.
pub fn export(pack: &Path, file: &Path) -> Result<Packed, Error> {
    let mut wardrobe = Wardrobe::read(pack)?;
    let staging = tempfile::tempdir().map_err(io(pack))?;
    let mut copying = Copying::new(staging.path());
    let mut scratch = Scratch::default();
    let mut gathered = Vec::new();
    for garment in &mut wardrobe.garments {
        gather(pack, &mut copying, &mut scratch, &mut gathered, garment)?;
    }
    for carried in &wardrobe.shop.carried {
        let from = pack.join(carried.file.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !stays_inside(&carried.file) || !from.is_file() {
            return Err(refused(format!(
                "the pack carries {}, which is not a file inside it",
                carried.file
            )));
        }
        let to = staging
            .path()
            .join(carried.file.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        std::fs::copy(&from, &to).map_err(io(&from))?;
    }
    wardrobe.lock = None;
    let settings = staging.path().join(FILE);
    std::fs::write(&settings, wardrobe.to_json()).map_err(io(&settings))?;

    let held = files_under(staging.path())?;
    let part = file.with_extension(format!("{EXTENSION}.part"));
    let written = (|| -> Result<u64, Error> {
        let out = std::fs::File::create(&part).map_err(io(&part))?;
        let mut zip = ZipWriter::new(out);
        // A fixed time and a fixed order, so that equal packs are equal files.
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(DateTime::default());
        let mut bytes = 0u64;
        let mut put = |name: &str, data: &[u8]| -> Result<(), Error> {
            zip.start_file(name, options)
                .map_err(|e| refused(e.to_string()))?;
            zip.write_all(data).map_err(io(&part))?;
            bytes += data.len() as u64;
            Ok(())
        };
        put(HEADER, header().as_bytes())?;
        for (name, path) in &held {
            put(name, &std::fs::read(path).map_err(io(path))?)?;
        }
        zip.finish().map_err(|e| refused(e.to_string()))?;
        Ok(bytes)
    })();
    match written {
        Ok(bytes) => {
            std::fs::rename(&part, file).map_err(io(file))?;
            Ok(Packed {
                garments: wardrobe.garments.len(),
                files: held.len(),
                bytes,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Whether an entry name stays inside the folder it is unpacked into on every host.
fn is_safe(name: &str) -> bool {
    stays_inside(name)
        && !name.contains(['\\', ':'])
        && name.split('/').all(or_project::paths::is_component)
}

/// Whether `name` is a compiled game file outside a source folder, which a package never holds: a cloth is the one exception.
fn is_compiled(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.split('/').any(|part| part.ends_with(".src")) || lower.ends_with(".yld") {
        return false;
    }
    lower
        .rsplit_once('.')
        .is_some_and(|(_, ext)| or_io::gta5::formats::format(ext).is_some())
}

/// A package opened, with the index and name of each file in it that is a file of the pack.
type Opened = (ZipArchive<std::fs::File>, Vec<(usize, String)>);

/// Read the package `file` and say what it holds without making anything.
fn read_entries(file: &Path) -> Result<Opened, Error> {
    let open = std::fs::File::open(file).map_err(io(file))?;
    let mut zip = ZipArchive::new(open)
        .map_err(|e| refused(format!("{} is not a pack package: {e}", file.display())))?;
    let mut entries = Vec::new();
    let (mut total, mut seen_header) = (0u64, false);
    for at in 0..zip.len() {
        let entry = zip.by_index(at).map_err(|e| refused(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name == HEADER {
            let mut text = String::new();
            let mut entry = entry;
            entry
                .read_to_string(&mut text)
                .map_err(|e| refused(format!("{HEADER} does not read: {e}")))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| refused(format!("{HEADER} does not read: {e}")))?;
            if value["format"] != FORMAT {
                return Err(refused(format!("{} is not a pack package", file.display())));
            }
            let schema = value["schema"].as_u64().unwrap_or(0);
            if schema == 0 || schema > u64::from(SCHEMA) {
                return Err(refused(format!(
                    "{} is schema {schema}, and this build reads schema {SCHEMA}",
                    file.display()
                )));
            }
            seen_header = true;
            continue;
        }
        if !is_safe(&name) {
            return Err(refused(format!(
                "the package holds {name}, a name that leaves the folder it is unpacked into"
            )));
        }
        if is_compiled(&name) {
            return Err(refused(format!(
                "the package holds {name}, a compiled game file: a package holds sources"
            )));
        }
        if entry.size() > MAX_ENTRY {
            return Err(refused(format!("{name} is larger than a package holds")));
        }
        total += entry.size();
        entries.push((at, name));
    }
    if !seen_header {
        return Err(refused(format!("{} is not a pack package", file.display())));
    }
    if total > MAX_TOTAL {
        return Err(refused("the package unpacks to more than a package holds"));
    }
    if !entries.iter().any(|(_, name)| name == FILE) {
        return Err(refused(format!("the package holds no {FILE}")));
    }
    Ok((zip, entries))
}

/// Unpack the package `file` into the folder `into`, which is made and must not hold anything.
///
/// Nothing is left behind when it fails: the pack that comes out is read back, and a package whose `wardrobe.json` does not read,
/// or names a file it does not hold, is refused.
pub fn import(file: &Path, into: &Path) -> Result<Imported, Error> {
    let (mut zip, entries) = read_entries(file)?;
    let existed = into.exists();
    if existed && std::fs::read_dir(into).is_ok_and(|mut d| d.next().is_some()) {
        return Err(refused(format!(
            "{} is not empty: a package is unpacked into a folder of its own",
            into.display()
        )));
    }
    let made = (|| -> Result<Imported, Error> {
        for (at, name) in &entries {
            let mut entry = zip.by_index(*at).map_err(|e| refused(e.to_string()))?;
            let to = into.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            let mut out = std::fs::File::create(&to).map_err(io(&to))?;
            // The declared size is the zip's own claim, so what is written is held to the cap as well.
            let wrote =
                std::io::copy(&mut (&mut entry).take(MAX_ENTRY + 1), &mut out).map_err(io(&to))?;
            if wrote > MAX_ENTRY {
                return Err(refused(format!("{name} is larger than a package holds")));
            }
        }
        let wardrobe = Wardrobe::read(into)?;
        for named in named_paths(&wardrobe) {
            let at = into.join(named.replace('/', std::path::MAIN_SEPARATOR_STR));
            if !stays_inside(named) || !at.exists() {
                return Err(refused(format!(
                    "the package names {named}, which it does not hold"
                )));
            }
        }
        Ok(Imported {
            pack: wardrobe.pack,
            garments: wardrobe.garments.len(),
            files: entries.len(),
        })
    })();
    if made.is_err() {
        match existed {
            false => {
                let _ = std::fs::remove_dir_all(into);
            }
            true => {
                for child in std::fs::read_dir(into).into_iter().flatten().flatten() {
                    let path = child.path();
                    let _ = match path.is_dir() {
                        true => std::fs::remove_dir_all(&path),
                        false => std::fs::remove_file(&path),
                    };
                }
            }
        }
    }
    made
}
