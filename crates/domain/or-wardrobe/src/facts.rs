//! What the disk says of the files a pack names, gathered once so that every rule and the plan are pure functions of it.
//!
//! Looking at files is the same for every kind of content, so it is `or_project::content::facts`; what is
//! a pack's is which files it names.

use std::path::Path;

use crate::follow::Led;
use crate::model::Wardrobe;

pub use or_project::content::facts::{digest, stays_inside, Depth, Fact, Facts};
#[cfg(test)]
use or_project::content::facts::{look, LISTED, LISTED_FROM};

/// Every path a pack names: its models, their cloth and its textures, and the files it carries for the shop.
pub fn named_paths(wardrobe: &Wardrobe) -> impl Iterator<Item = &str> {
    let garments = wardrobe.garments.iter().flat_map(|g| {
        g.model
            .iter()
            .chain(g.alternates.iter())
            .chain(g.cloth.iter())
            .chain(g.variations.iter().filter_map(|v| v.texture.as_ref()))
            .map(String::as_str)
    });
    garments.chain(wardrobe.shop.carried.iter().map(|c| c.file.as_str()))
}

/// The kind of source folder a path names, by its suffix: a drawables source for a model, a textures source for a texture.
pub fn source_kind(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".drawables.src") {
        Some(or_source::models::DRAWABLES)
    } else if lower.ends_with(".textures.src") {
        Some(or_source::textures::KIND)
    } else {
        None
    }
}

/// Whether a path names a source folder, which a build cooks into the file it ships.
pub fn is_source(path: &str) -> bool {
    source_kind(path).is_some()
}

/// The file a build makes of the source folder `dir`, by its name and its bytes, cooked in a scratch folder that is gone when this returns.
pub fn cooked(dir: &Path) -> Result<(String, Vec<u8>), String> {
    let scratch = tempfile::tempdir().map_err(|e| format!("cannot be cooked: {e}"))?;
    let made = or_source::cook::cook_source(dir, scratch.path()).map_err(|why| why.join("; "))?;
    let bytes = std::fs::read(&made).map_err(|e| format!("cannot be read: {e}"))?;
    let name = made
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    Ok((name, bytes))
}

/// What a cloth is, for a name that must not be a source: it has no source form.
pub(crate) const CLOTH: &str = "cloth";

/// What a name is for, which decides the kind of source it may be: a model and its alternates are drawables sources, a texture is a textures source, and a cloth has no source form.
pub(crate) fn parts(wardrobe: &Wardrobe) -> impl Iterator<Item = (&str, Option<&'static str>)> {
    let drawables = Some(or_source::models::DRAWABLES);
    let textures = Some(or_source::textures::KIND);
    wardrobe.garments.iter().flat_map(move |g| {
        let models = g.model.iter().chain(g.alternates.iter());
        let models = models.map(move |m| (m.as_str(), drawables));
        let cloth = g.cloth.iter().map(|c| (c.as_str(), Some(CLOTH)));
        let textures = (g.variations.iter())
            .filter_map(|v| v.texture.as_deref())
            .map(move |t| (t, textures));
        models.chain(cloth).chain(textures)
    })
}

/// The files `wardrobe` names, looked at in `folder` as deeply as `depth` says.
///
/// A name that is a source folder is looked at as the file it becomes: its size is what the cook reads, its digest is the
/// folder's, and it opens when the source is of the kind the name is for. A name that is a reference is looked at as what
/// it leads to, and an id alone as a file of no bytes that is read as an id.
pub fn inspect(folder: &Path, wardrobe: &Wardrobe, depth: Depth) -> Facts {
    let mut facts = or_project::content::facts::inspect_paths_measured(
        folder,
        named_paths(wardrobe),
        depth,
        crate::measure::measure,
    );
    for (named, want) in parts(wardrobe).filter(|(named, _)| stays_inside(named)) {
        let at = |rel: &str| folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let fact = match crate::follow::follow(folder, named) {
            Led::Own if is_source(named) && facts.get(named) == Some(&Fact::Folder) => {
                crate::source_facts::source_fact(&at(named), depth, want)
            }
            Led::Own => continue,
            Led::To { at, source: true } => crate::source_facts::source_fact(&at, depth, want),
            Led::To { at, source: false } => crate::source_facts::file_fact(&at, depth),
            Led::Id(id) => crate::source_facts::id_fact(&id, depth),
            Led::Nowhere {
                unreadable: false, ..
            } => Fact::Missing,
            Led::Nowhere { why, .. } => crate::source_facts::refused(0, why),
        };
        facts.insert(named, fact);
    }
    facts
}

/// `asset.json` without what names the asset and not its content (its id, its name, the build that wrote it and its credit), so that two sources made of one file are one content and a rename is no edit.
fn without_identity(bytes: Vec<u8>) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return bytes;
    };
    if let Some(fields) = value.as_object_mut() {
        for identity in ["id", "name", "created_with", "credit"] {
            fields.remove(identity);
        }
    }
    serde_json::to_vec(&value).unwrap_or(bytes)
}

/// What a folder held when its digest was made: how many files, how many bytes, and the newest time any was written.
type Stamp = (usize, u64, Option<std::time::SystemTime>);

/// The hash of everything a folder holds, by the path of each file and its bytes, so that a change to any file is a change of the folder.
///
/// A scan that runs again while a project is on screen does not read the bytes of a source whose files kept their sizes and
/// times; one written within the last two seconds is read each time, as a file is ([`or_project::content::facts`]).
pub(crate) fn folder_digest(at: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static MEMORY: OnceLock<Mutex<HashMap<std::path::PathBuf, (Stamp, String)>>> = OnceLock::new();

    fn files(dir: &Path, out: &mut Vec<(std::path::PathBuf, std::fs::Metadata)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            match (entry.file_type(), entry.metadata()) {
                (Ok(kind), _) if kind.is_dir() => files(&entry.path(), out),
                (Ok(kind), Ok(meta)) if kind.is_file() => out.push((entry.path(), meta)),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    files(at, &mut found);
    let newest = found.iter().filter_map(|(_, m)| m.modified().ok()).max();
    let stamp: Stamp = (
        found.len(),
        found.iter().map(|(_, m)| m.len()).sum(),
        newest,
    );
    let settled = newest.is_some_and(|t| t.elapsed().is_ok_and(|age| age >= SETTLED));
    let memory = MEMORY.get_or_init(Default::default);
    if let Some((held, digest)) = settled
        .then(|| memory.lock().ok()?.get(at).cloned())
        .flatten()
    {
        if held == stamp {
            return digest;
        }
    }
    let mut hasher = Sha256::new();
    for (path, _) in &found {
        let rel = path
            .strip_prefix(at)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(path).unwrap_or_default();
        let bytes = match rel.as_str() {
            "asset.json" => without_identity(bytes),
            _ => bytes,
        };
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if settled {
        if let Ok(mut memory) = memory.lock() {
            // ponytail: a map cleared when full; a pack of more source models than this reads them again on each scan.
            if memory.len() >= 10_000 {
                memory.clear();
            }
            memory.insert(at.to_path_buf(), (stamp, digest.clone()));
        }
    }
    digest
}

/// A folder changed more recently than this is not remembered: a second write within one tick of the file system's clock keeps its size and its time.
const SETTLED: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Garment;

    #[test]
    fn a_name_that_leaves_the_folder_or_no_host_can_create_is_not_looked_for() {
        for bad in [
            "",
            "/etc/passwd",
            "../x.ydd",
            "a/../b.ydd",
            "a//b.ydd",
            "con:.ydd",
            "a\\b.ydd",
        ] {
            assert!(!stays_inside(bad), "{bad:?}");
        }
        for good in ["a.ydd", "models/a b.ydd", "m/n/o.ytd"] {
            assert!(stays_inside(good), "{good:?}");
        }
    }

    #[test]
    fn the_files_a_pack_names_are_looked_at_to_the_depth_asked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("models")).unwrap();
        std::fs::write(dir.path().join("models/a.ydd"), b"not a drawable").unwrap();
        let mut w = Wardrobe::new("p");
        let mut g = Garment::new();
        g.model = Some("models/a.ydd".into());
        g.variations.push(crate::model::Variation {
            texture: Some("missing.ytd".into()),
            ..Default::default()
        });
        g.variations.push(crate::model::Variation {
            texture: Some("../out.ytd".into()),
            ..Default::default()
        });
        w.garments.push(g);

        let stat = inspect(dir.path(), &w, Depth::Stat);
        assert!(matches!(
            stat.get("models/a.ydd"),
            Some(Fact::File {
                bytes: 14,
                digest: None,
                read: None,
                measured: None
            })
        ));
        assert_eq!(stat.get("missing.ytd"), Some(&Fact::Missing));
        assert_eq!(stat.get("../out.ytd"), Some(&Fact::Escapes));

        let digested = inspect(dir.path(), &w, Depth::Digest);
        let Some(Fact::File {
            digest: Some(d),
            read: None,
            ..
        }) = digested.get("models/a.ydd")
        else {
            panic!("a digest and no reading");
        };
        assert_eq!(d, &digest(b"not a drawable"));

        let opened = inspect(dir.path(), &w, Depth::Open);
        let Some(Fact::File {
            read: Some(Err(why)),
            ..
        }) = opened.get("models/a.ydd")
        else {
            panic!("a refusal");
        };
        assert!(!why.is_empty());
    }

    /// Writes `bytes` to `path` and dates it `when`.
    fn written(path: &std::path::Path, bytes: &[u8], when: std::time::SystemTime) {
        std::fs::write(path, bytes).unwrap();
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(when).unwrap();
    }

    #[test]
    fn the_files_of_a_crowded_folder_are_listed_once_and_each_reads_as_if_it_were_asked_for_alone()
    {
        let dir = tempfile::tempdir().unwrap();
        for folder in ["models", "few"] {
            std::fs::create_dir_all(dir.path().join(folder)).unwrap();
        }
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let mut w = Wardrobe::new("p");
        for i in 0..LISTED_FROM + 8 {
            let name = format!("models/m{i:03}.ydd");
            written(
                &dir.path().join(&name),
                format!("model {i}").as_bytes(),
                long_ago,
            );
            let mut g = Garment::new();
            g.model = Some(name);
            w.garments.push(g);
        }
        // A folder where a file is named, files that are not there, and a few files in a folder of their own.
        std::fs::create_dir_all(dir.path().join("models/folder.ydd")).unwrap();
        let mut odd = Garment::new();
        odd.model = Some("models/folder.ydd".into());
        odd.alternates = vec!["models/gone.ydd".into(), "models/M000.ydd".into()];
        for name in ["few/a.ytd", "few/b.ytd", "few/gone.ytd"] {
            if !name.contains("gone") {
                written(&dir.path().join(name), name.as_bytes(), long_ago);
            }
            odd.variations.push(crate::model::Variation {
                texture: Some(name.into()),
                ..Default::default()
            });
        }
        w.garments.push(odd);

        LISTED.with(|n| n.set(0));
        let facts = inspect(dir.path(), &w, Depth::Digest);

        assert_eq!(
            LISTED.with(std::cell::Cell::get),
            1,
            "models/ holds crowds of named files and is listed once; few/ is asked for file by file"
        );
        for path in named_paths(&w) {
            let alone = look(
                &dir.path()
                    .join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
                path,
                Depth::Digest,
                None,
            );
            assert_eq!(facts.get(path), Some(&alone), "{path}");
        }
        assert_eq!(facts.get("models/folder.ydd"), Some(&Fact::Folder));
        assert_eq!(facts.get("models/gone.ydd"), Some(&Fact::Missing));
    }

    #[test]
    fn a_source_folder_not_touched_since_is_not_read_again_and_one_touched_just_now_always_is() {
        let dir = tempfile::tempdir().unwrap();
        let long = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let file = dir.path().join("model.glb");

        written(&file, b"first", long);
        let first = folder_digest(dir.path());
        written(&file, b"other", long);
        assert_eq!(
            folder_digest(dir.path()),
            first,
            "not read again: the folder kept its sizes and times"
        );
        // A new size, a new file or a new time is a change.
        written(&file, b"longer", long);
        let longer = folder_digest(dir.path());
        assert_ne!(longer, first);
        written(&dir.path().join("asset.json"), b"{}", long);
        assert_ne!(folder_digest(dir.path()), longer);

        // Written just now, it is never trusted: two writes of one size in one tick of the clock look alike.
        let now = std::time::SystemTime::now();
        written(&file, b"aaaaa", now);
        let first = folder_digest(dir.path());
        written(&file, b"bbbbb", now);
        assert_ne!(folder_digest(dir.path()), first);
    }

    #[test]
    fn a_file_not_touched_since_is_not_read_again_and_one_touched_just_now_always_is() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Wardrobe::new("p");
        let mut g = Garment::new();
        g.model = Some("a.ydd".into());
        w.garments.push(g);
        let digest_of = |w: &Wardrobe| match inspect(dir.path(), w, Depth::Digest).get("a.ydd") {
            Some(Fact::File {
                digest: Some(d), ..
            }) => d.clone(),
            other => panic!("{other:?}"),
        };

        // Dated long ago, a file is remembered: the same size and time says it is the same file.
        let now = std::time::SystemTime::now();
        let long = now - std::time::Duration::from_secs(3600);
        written(&dir.path().join("a.ydd"), b"first", long);
        let first = digest_of(&w);
        assert_eq!(first, digest(b"first"));
        written(&dir.path().join("a.ydd"), b"other", long);
        assert_eq!(
            digest_of(&w),
            first,
            "not read again: nothing says it changed"
        );
        // A new size or a new time is a change.
        written(&dir.path().join("a.ydd"), b"longer", long);
        assert_eq!(digest_of(&w), digest(b"longer"));
        written(
            &dir.path().join("a.ydd"),
            b"third",
            long + std::time::Duration::from_secs(1),
        );
        assert_eq!(digest_of(&w), digest(b"third"));

        // Written just now, it is never trusted: two writes in one tick of the clock look alike.
        written(
            &dir.path().join("a.ydd"),
            b"aaaaa",
            std::time::SystemTime::now(),
        );
        assert_eq!(digest_of(&w), digest(b"aaaaa"));
        written(
            &dir.path().join("a.ydd"),
            b"bbbbb",
            std::time::SystemTime::now(),
        );
        assert_eq!(digest_of(&w), digest(b"bbbbb"));
    }
}
