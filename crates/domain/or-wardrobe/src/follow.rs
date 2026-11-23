//! Where a name in a garment leads: a file or a source in the pack's folder, or, through a reference, somewhere else.
//!
//! A garment names its model, textures and cloth by a path inside the module's folder. A path that ends in `.ref.json` is a
//! reference ([`or_project::reference`]): a source or file of this project, one of another project, or only an id. The
//! facts of a scan and the plan follow it, and nothing else in the Wardrobe has to know there is one.

use std::path::{Path, PathBuf};

use or_project::reference::{self, Reference, Resolved};

use crate::facts::stays_inside;

/// What a name leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Led {
    /// The name is a path of the pack's own folder, and the file or source is there.
    Own,
    /// A reference leads to `at`, a source folder when `source` and a file otherwise.
    To { at: PathBuf, source: bool },
    /// A reference that is only an id: the file is not in any project.
    Id(String),
    /// A reference that leads nowhere: not there (`unreadable` false) or not a reference at all.
    Nowhere { why: String, unreadable: bool },
}

/// The folder a reference of the pack in `folder` is resolved against: the project that holds it, else the pack's own.
fn base(folder: &Path) -> PathBuf {
    reference::holder(folder).unwrap_or_else(|| folder.to_path_buf())
}

/// Where the name `named`, a path inside `folder`, leads.
pub fn follow(folder: &Path, named: &str) -> Led {
    if !reference::is_reference(named) {
        return Led::Own;
    }
    if !stays_inside(named) {
        return Led::Nowhere {
            why: format!("{named} leaves the pack's folder"),
            unreadable: true,
        };
    }
    let file = folder.join(named.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !file.is_file() {
        return Led::Nowhere {
            why: format!("{named} is not there"),
            unreadable: false,
        };
    }
    let reference = match Reference::read(&file) {
        Ok(reference) => reference,
        Err(why) => {
            return Led::Nowhere {
                why,
                unreadable: true,
            }
        }
    };
    match reference::resolve(&reference, &base(folder)) {
        Resolved::File { path, .. } => Led::To {
            at: path,
            source: false,
        },
        Resolved::Folder { path } => Led::To {
            at: path,
            source: true,
        },
        Resolved::Named { id } => Led::Id(id),
        Resolved::Unresolved { wanted, why } => Led::Nowhere {
            why: format!("the reference {named} leads nowhere: {wanted} {why}"),
            unreadable: false,
        },
    }
}

/// The sentence a plan says of a file that is only an id.
pub fn named_only(who: &str, part: &str, id: &str) -> String {
    format!("{who}: the {part} {id} is not in the project, so the build names it by its id and ships no file for it")
}
