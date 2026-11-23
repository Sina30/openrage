//! Where a module's content is, and the names and ids and files a kind of content keeps in it.

use std::path::{Path, PathBuf};

use crate::container::Module;
use crate::paths::is_component;

/// Why the folder of a module is not one.
#[derive(Debug, thiserror::Error)]
pub enum FolderError {
    #[error("the folder of content `{module}` is {why}")]
    Folder { module: String, why: String },
    #[error("{0}")]
    Linked(String),
}

/// The folder `module`'s content is in: inside `project_folder`, or where its link leads.
///
/// `what` names the kind in the sentence that says it has no `path` ("a clothing module").
pub fn module_folder(
    project_folder: &Path,
    module: &Module,
    what: &str,
) -> Result<PathBuf, FolderError> {
    let fail = |why: &str| FolderError::Folder {
        module: module.id.clone(),
        why: why.to_string(),
    };
    if module.is_link() {
        let folder =
            crate::links::folder_of(module, project_folder).map_err(FolderError::Linked)?;
        return if folder.is_dir() {
            Ok(folder)
        } else {
            Err(fail(&format!(
                "a link to {}, which is not there",
                folder.display()
            )))
        };
    }
    let path = module
        .path()
        .ok_or_else(|| fail(&format!("not stated: {what} has a `path`")))?;
    if path == "." {
        return Ok(project_folder.to_path_buf());
    }
    if !path.split('/').all(is_component) {
        return Err(fail("not a path inside the project's folder"));
    }
    Ok(project_folder.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

/// A new stable id: 16 random bytes in hex.
pub fn new_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system has random bytes");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `text` is a name the game resolves a thing by and a file can carry: lower case letters,
/// digits and `_`, from a letter.
///
/// ⚠ The game hashes a name without regard to case and a file name carries it, so a name that is only
/// lower case never meets two spellings of itself.
pub fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Write `text` as `path`, through a hidden file beside it that replaces it, so a failure leaves the file that was there.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let temp = path.with_file_name(format!(".{name}.new"));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_the_project_itself_a_plain_path_in_it_or_nothing_that_climbs_out() {
        let project = Path::new("/projects/pack");
        let at =
            |path: &str| module_folder(project, &Module::clothing("c", path), "a thing module");
        assert_eq!(at(".").unwrap(), project);
        assert_eq!(
            at("modules/uniforms").unwrap(),
            project.join("modules").join("uniforms")
        );
        for bad in ["../up", "a/../b", "/abs", ""] {
            assert!(at(bad).is_err(), "{bad:?}");
        }
        let none = module_folder(project, &Module::files("f"), "a thing module").unwrap_err();
        assert_eq!(
            none.to_string(),
            "the folder of content `f` is not stated: a thing module has a `path`"
        );
    }

    #[test]
    fn a_name_is_an_identifier_when_it_is_lower_case_letters_digits_and_underscores_from_a_letter()
    {
        assert!(is_identifier("a_m_y_business_03"));
        for bad in ["", "My", "2a", "_a", "a b", "a-b"] {
            assert!(!is_identifier(bad), "{bad:?}");
        }
        assert_ne!(new_id(), new_id());
        assert_eq!(new_id().len(), 32);
    }

    #[test]
    fn a_file_is_replaced_whole_and_no_temporary_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
