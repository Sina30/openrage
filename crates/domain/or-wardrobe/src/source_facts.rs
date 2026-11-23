//! What the disk says of a name that is not a plain file of the pack: a source folder, a file a reference leads to, an id.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use or_project::content::facts::{Depth, Fact, Measured};

use crate::facts::{digest, folder_digest};

/// A file that is looked at and refused, for the reason.
pub(crate) fn refused(bytes: u64, why: String) -> Fact {
    Fact::File {
        bytes,
        digest: None,
        read: Some(Err(why)),
        measured: None,
    }
}

/// The source folder at `at`, looked at as the file a build cooks it into, when it is the kind the name is for.
pub(crate) fn source_fact(at: &Path, depth: Depth, want: Option<&str>) -> Fact {
    let said = match or_source::cook::describe(at) {
        Ok(said) => said,
        Err(why) => return refused(0, why),
    };
    if let Some(want) = want.filter(|want| *want != said.kind) {
        let (part, is) = match want {
            or_source::models::DRAWABLES => ("model", "a drawables source"),
            or_source::textures::KIND => ("texture", "a textures source"),
            _ => ("cloth", "a .yld file, which has no source form"),
        };
        return refused(
            said.bytes,
            format!("it is a {} source, and a {part} is {is}", said.kind),
        );
    }
    let folder = (depth != Depth::Stat).then(|| folder_digest(at));
    let measured = folder
        .as_deref()
        .filter(|_| depth >= Depth::Measure)
        .and_then(|key| match said.kind.as_str() {
            or_source::textures::KIND => measured_texture_source(at, key),
            or_source::models::DRAWABLES => measured_drawables_source(at, key),
            _ => None,
        });
    Fact::File {
        bytes: said.bytes,
        digest: folder,
        read: (depth == Depth::Open).then(|| Ok(format!("a {} source", said.kind))),
        measured,
    }
}

/// What the dictionary a textures source cooks into holds, counted once for each state of the folder.
///
/// ⚠ Cooked in a scratch folder, so the rules that hold a texture to a limit see a texture of a source as they see one of a file.
fn measured_texture_source(at: &Path, key: &str) -> Option<Measured> {
    remembered(key, || {
        crate::facts::cooked(at)
            .ok()
            .and_then(|(name, bytes)| crate::measure::measure(&name, &bytes))
    })
}

/// What the models a drawables source holds draw, read from the header of each glb and counted once for each state of the folder.
fn measured_drawables_source(at: &Path, key: &str) -> Option<Measured> {
    remembered(key, || {
        let drawables = crate::measure::source_drawables(at)?;
        serde_json::to_value(crate::measure::Measure {
            detail: true,
            drawables,
            ..Default::default()
        })
        .ok()
    })
}

/// What `count` says of the source whose folder digest is `key`, kept so that a scan that finds the folder as it was counts nothing again.
fn remembered(key: &str, count: impl FnOnce() -> Option<Measured>) -> Option<Measured> {
    static MEMORY: OnceLock<Mutex<HashMap<String, Option<Measured>>>> = OnceLock::new();
    let memory = MEMORY.get_or_init(Default::default);
    if let Some(held) = memory.lock().ok().and_then(|m| m.get(key).cloned()) {
        return held;
    }
    let counted = count();
    if let Ok(mut memory) = memory.lock() {
        // ponytail: a map cleared when full; a pack of more texture sources than this counts them again on each scan.
        if memory.len() >= 10_000 {
            memory.clear();
        }
        memory.insert(key.to_string(), counted.clone());
    }
    counted
}

/// The file a reference leads to, looked at where it is.
pub(crate) fn file_fact(at: &Path, depth: Depth) -> Fact {
    let (Some(parent), Some(name)) = (at.parent(), at.file_name().and_then(|n| n.to_str())) else {
        return Fact::Missing;
    };
    or_project::content::facts::inspect_paths_measured(
        parent,
        [name],
        depth,
        crate::measure::measure,
    )
    .get(name)
    .cloned()
    .unwrap_or(Fact::Missing)
}

/// A file that is only an id: no bytes, read as an id, and as the digest of that id.
pub(crate) fn id_fact(id: &str, depth: Depth) -> Fact {
    Fact::File {
        bytes: 0,
        digest: (depth != Depth::Stat).then(|| digest(format!("id:{id}").as_bytes())),
        read: (depth == Depth::Open).then(|| Ok(format!("the id {id}, which is no file"))),
        measured: None,
    }
}
