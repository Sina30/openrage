//! What the disk says of the files a module names, gathered once so that every rule and the plan are pure functions of it.
//!
//! Looking at files is the same for every kind of content, so it is `or_project::content::facts`; what is
//! this module's is which files it names and how the files of a ped that are not paged formats are opened.

use std::path::Path;

pub use or_project::content::facts::{digest, stays_inside, Depth, Fact, Facts};
use or_project::content::facts::{inspect_paths_with, paged};

use crate::model::AddonPeds;

/// Every path a module names: its peds' files and its data file.
pub fn named_paths(model: &AddonPeds) -> impl Iterator<Item = &str> {
    model
        .peds
        .iter()
        .flat_map(|p| p.files.iter().map(|f| f.path.as_str()))
        .chain(model.meta.as_deref())
}

/// The files `model` names, looked at in `folder` as deeply as `depth` says.
pub fn inspect(folder: &Path, model: &AddonPeds, depth: Depth) -> Facts {
    inspect_paths_with(folder, named_paths(model), depth, reader)
}

/// The reader that opens a file of a ped: the metadata reader for a variation file or a data file, else a paged format's.
fn reader(name: &str, bytes: &[u8]) -> Result<String, String> {
    let ext = name
        .rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "ymt" | "meta") {
        return or_io::meta::MetaDocument::parse(or_io::meta::default_library(), bytes.to_vec())
            .map(|_| "metadata".to_string())
            .map_err(|e| e.to_string());
    }
    paged(name, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Ped, PedFile};

    #[test]
    fn the_files_a_model_names_are_looked_at_to_the_depth_asked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.yft"), b"not a fragment").unwrap();
        let mut model = AddonPeds::new();
        let mut ped = Ped::new("a");
        for path in ["a.yft", "missing.ytd", "../out.ytd"] {
            ped.files.push(PedFile {
                path: path.into(),
                at: None,
            });
        }
        model.peds.push(ped);

        let stat = inspect(dir.path(), &model, Depth::Stat);
        assert!(matches!(
            stat.get("a.yft"),
            Some(Fact::File {
                bytes: 14,
                digest: None,
                read: None,
                measured: None
            })
        ));
        assert_eq!(stat.get("missing.ytd"), Some(&Fact::Missing));
        assert_eq!(stat.get("../out.ytd"), Some(&Fact::Escapes));

        let opened = inspect(dir.path(), &model, Depth::Open);
        let Some(Fact::File {
            digest: Some(d),
            read: Some(Err(why)),
            ..
        }) = opened.get("a.yft")
        else {
            panic!("a digest and a refusal");
        };
        assert_eq!(d, &digest(b"not a fragment"));
        assert!(!why.is_empty());
    }
}
