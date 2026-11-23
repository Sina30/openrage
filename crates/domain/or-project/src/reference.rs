//! A reference: the one place a content may name something that is not a source of its project.
//!
//! A project holds sources, and a content owns what it holds. What a content needs and does not own, such as the cloth of a
//! garment, is named by a reference, a small `<name>.ref.json` file in the content's folder, in one of three forms:
//!
//! - **here**: a source or a file of this project, by its path, and by its asset id when it is a source, so that a rename or a
//!   move of the folder is followed;
//! - **other**: a source or a file of another project, by the path of that project's folder (stored as a link stores it) and
//!   the path inside it, with the id likewise;
//! - **by id**: a name or a hash typed by hand, for a file nobody here has. A build names it and ships no file.
//!
//! The reference is resolved when a plan is made ([`resolve`]); one that does not resolve is a problem that names where it
//! looked, and is mended by choosing the file again.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::content::facts::stays_inside;
use crate::links::{project_file_in, store_link};
use crate::paths::{find_sources, folded, resolve_stored};

/// What a reference file's name ends with.
pub const SUFFIX: &str = ".ref.json";
pub const FORMAT: &str = "openrage-reference";
pub const SCHEMA: u32 = 1;
/// The longest id a person may type.
pub const MAX_ID: usize = 128;

/// Whether the path names a reference file.
pub fn is_reference(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(SUFFIX)
}

/// What a content needs and does not own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum Reference {
    /// A source or a file of this project, at `path` inside its folder.
    Here {
        path: String,
        /// The asset id, when it is a source: what the reference follows when the folder is renamed or moved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// A source or a file of another project.
    Other {
        /// The folder of that project as this project stores a link: relative with `..` where the two share a root, else absolute.
        project: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// A name or a hash typed by hand.
    ById { id: String },
}

#[derive(Serialize, Deserialize)]
struct Document {
    format: String,
    schema: u32,
    #[serde(flatten)]
    reference: Reference,
}

/// Where a reference leads, as a plan finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    File {
        path: PathBuf,
        bytes: u64,
    },
    /// A source folder.
    Folder {
        path: PathBuf,
    },
    /// Only an id: the file is not in any project this one can reach.
    Named {
        id: String,
    },
    /// Where it was looked for, and why nothing is there.
    Unresolved {
        wanted: String,
        why: String,
    },
}

impl Resolved {
    /// One sentence a plan or a Problems list says of it.
    pub fn sentence(&self) -> String {
        match self {
            Self::File { path, .. } | Self::Folder { path } => {
                format!("is {}", path.display())
            }
            Self::Named { id } => format!(
                "is the id {id}, and that file is not in the project: the build names it by its id and ships no file for it"
            ),
            Self::Unresolved { wanted, why } => format!("{wanted} {why}"),
        }
    }
}

fn plain_id(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("an id is not empty".into());
    }
    if id.chars().count() > MAX_ID || id.chars().any(char::is_control) {
        return Err(format!("an id is one line of at most {MAX_ID} characters"));
    }
    Ok(())
}

impl Reference {
    /// A source or file of this project at `path` (inside its folder, `/` between names).
    pub fn here(path: &str, id: Option<&str>) -> Self {
        Self::Here {
            path: path.to_string(),
            id: id.map(str::to_string),
        }
    }

    /// A source or file of the project whose folder is `stored` as this project stores a link.
    pub fn other(stored: &str, path: &str, id: Option<&str>) -> Self {
        Self::Other {
            project: stored.to_string(),
            path: path.to_string(),
            id: id.map(str::to_string),
        }
    }

    /// [`Self::other`] for the folder `target`, stored relative to the folder `holder` of the project that holds the reference.
    pub fn to_other(target: &Path, holder: &Path, path: &str, id: Option<&str>) -> Self {
        Self::other(&store_link(target, holder), path, id)
    }

    pub fn by_id(id: &str) -> Self {
        Self::ById {
            id: id.trim().to_string(),
        }
    }

    /// Why this reference cannot be written or read, none when it is well formed.
    pub fn check(&self) -> Result<(), String> {
        let inside = |path: &str| {
            match stays_inside(path) {
            true => Ok(()),
            false => Err(format!(
                "`{path}` is not a path inside the project: it needs `/` between names and cannot leave the folder"
            )),
        }
        };
        match self {
            Self::Here { path, .. } => inside(path),
            Self::Other { project, path, .. } => match project.trim().is_empty() {
                true => Err("a reference to another project names its folder".into()),
                false => inside(path),
            },
            Self::ById { id } => plain_id(id),
        }
    }

    /// What a person reads of it: `the id cloth_001, typed by hand`.
    pub fn describe(&self) -> String {
        match self {
            Self::Here { path, .. } => format!("{path}, in this project"),
            Self::Other { project, path, .. } => format!("{path}, in the project at {project}"),
            Self::ById { id } => format!("the id {id}, typed by hand"),
        }
    }

    /// The canonical text of the reference file: keys sorted, two-space indent, LF.
    pub fn to_text(&self) -> String {
        let document = Document {
            format: FORMAT.to_string(),
            schema: SCHEMA,
            reference: self.clone(),
        };
        // A struct of strings and numbers always serialises.
        let mut text = serde_json::to_value(&document)
            .and_then(|value| serde_json::to_string_pretty(&value))
            .unwrap_or_default();
        text.push('\n');
        text
    }

    /// The reference `text` holds, or why it is none.
    pub fn parse(text: &str, shown: &str) -> Result<Self, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("{shown} is not a reference: {e}"))?;
        if let Some(format) = value["format"].as_str().filter(|f| *f != FORMAT) {
            return Err(format!(
                "{shown} is not a reference: its format is `{format}`, not `{FORMAT}`"
            ));
        }
        if let Some(schema) = value["schema"].as_u64().filter(|s| *s > u64::from(SCHEMA)) {
            return Err(format!(
                "{shown} was written by a newer OpenRage (schema {schema}) than this one reads (schema {SCHEMA})"
            ));
        }
        let document: Document = serde_json::from_value(value)
            .map_err(|e| format!("{shown} is not a reference: {e}"))?;
        document
            .reference
            .check()
            .map_err(|why| format!("{shown}: {why}"))?;
        Ok(document.reference)
    }

    pub fn read(file: &Path) -> Result<Self, String> {
        let shown = file.display().to_string();
        let text = std::fs::read_to_string(file).map_err(|e| format!("{shown}: {e}"))?;
        Self::parse(&text, &shown)
    }

    /// Write the reference at `file`, making the folders on the way; the folder of a content is the caller's to have checked.
    pub fn write(&self, file: &Path) -> std::io::Result<()> {
        self.check()
            .map_err(|why| std::io::Error::new(std::io::ErrorKind::InvalidInput, why))?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, self.to_text())
    }
}

/// The asset id of the source folder `dir`, read from its `asset.json` alone.
pub fn asset_id(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("asset.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value["id"].as_str().map(str::to_string)
}

/// The source folder of the project at `folder` that has the asset id `id`, wherever a rename or a move put it.
pub fn find_by_id(folder: &Path, id: &str) -> Option<PathBuf> {
    let (found, _) = find_sources(folder, &[]);
    found
        .into_iter()
        .find(|dir| asset_id(dir).as_deref() == Some(id))
}

/// The folder of the project that holds the content in `content`: the nearest folder at or above it that has a project file.
pub fn holder(content: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(content).unwrap_or_else(|_| content.to_path_buf());
    absolute
        .ancestors()
        .find(|dir| project_file_in(dir).is_some())
        .map(Path::to_path_buf)
}

/// Where `member`, a path inside the project at `folder`, leads, following `id` when it names a source that moved.
fn member(folder: &Path, member: &str, id: Option<&str>) -> Resolved {
    let at = folder.join(member.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Some(id) = id {
        if asset_id(&at).as_deref() == Some(id) {
            return Resolved::Folder { path: at };
        }
        return match find_by_id(folder, id) {
            Some(path) => Resolved::Folder { path },
            None => Resolved::Unresolved {
                wanted: member.to_string(),
                why: format!(
                    "is not there, and no source of {} has the id {id}",
                    folder.display()
                ),
            },
        };
    }
    match std::fs::metadata(&at) {
        Ok(meta) if meta.is_dir() => Resolved::Folder { path: at },
        Ok(meta) => Resolved::File {
            path: at,
            bytes: meta.len(),
        },
        Err(_) => Resolved::Unresolved {
            wanted: member.to_string(),
            why: format!("is not there in {}", folder.display()),
        },
    }
}

/// Where `reference` leads, for a content of the project whose folder is `folder`.
///
/// ⚠ Nothing is read but the ids of source folders, and nothing is written or copied: a reference is followed, never imported.
pub fn resolve(reference: &Reference, folder: &Path) -> Resolved {
    if let Err(why) = reference.check() {
        return Resolved::Unresolved {
            wanted: reference.describe(),
            why,
        };
    }
    match reference {
        Reference::Here { path, id } => member(folder, path, id.as_deref()),
        Reference::Other { project, path, id } => {
            let there = folded(&resolve_stored(project, folder));
            match there.is_dir() {
                true => member(&there, path, id.as_deref()),
                false => Resolved::Unresolved {
                    wanted: format!("{path} in the project at {project}"),
                    why: format!(
                        "is not there: the project at {project} ({}) is not there",
                        there.display()
                    ),
                },
            }
        }
        Reference::ById { id } => Resolved::Named {
            id: id.trim().to_string(),
        },
    }
}
