//! Where a project's files are, and how a path inside its folder is stored so the folder can travel.

use std::path::{Component, Path, PathBuf};

pub const PROJECT_EXTENSION: &str = "openrage-proj";

/// The folder beside a project file that the VFS reads first:
/// `vfs/common/data/x.dat` is `common:/data/x.dat`.
pub const SHADOW_DIR: &str = "vfs";

/// The folder beside a project file that builds land in, and that is never read back as content.
pub const BUILD_DIR: &str = "build";

/// The folder a project file's relative paths are relative to.
pub fn project_folder(project_path: &Path) -> PathBuf {
    let absolute = std::path::absolute(project_path).unwrap_or_else(|_| project_path.to_path_buf());
    absolute.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The project's shadow folder, whether or not it exists.
pub fn shadow_root(project_path: &Path) -> PathBuf {
    project_folder(project_path).join(SHADOW_DIR)
}

/// The project's build folder, whether or not it exists.
pub fn build_root(project_path: &Path) -> PathBuf {
    project_folder(project_path).join(BUILD_DIR)
}

/// `models/prop.ydr` for a file inside `folder`, the absolute path otherwise.
///
/// ⚠ **Only inside the folder.** A `../shared/x.ydr` would be relative and
/// still break the moment the folder is zipped without its sibling, which is
/// the one thing this form exists to survive.
pub fn store_path(path: &Path, folder: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    relative_inside(&absolute, folder).unwrap_or_else(|| absolute.to_string_lossy().into_owned())
}

/// Whether `text` is one component of a path that every host can create: not empty, `.` or `..`,
/// with no separator, no character Windows refuses, no control character and no trailing dot or space.
pub fn is_component(text: &str) -> bool {
    !matches!(text, "" | "." | "..")
        && !text.ends_with(['.', ' '])
        && !text
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
}

/// Whether `path`, with `/` for a separator, is inside the folder `dir`, whatever the case it is spelled in.
pub(crate) fn is_under(path: &str, dir: &str) -> bool {
    path.split_once('/')
        .is_some_and(|(first, _)| first.eq_ignore_ascii_case(dir))
}

/// `models/prop.ydr` for a file strictly inside `folder`, with `/` on every host.
pub fn relative_inside(path: &Path, folder: &Path) -> Option<String> {
    let mut rest = path.components();
    for want in folder.components() {
        if !same_component(want, rest.next()?) {
            return None;
        }
    }
    let parts = rest
        .map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Whether `path` is `folder` or inside it, comparing component by component.
pub(crate) fn is_within(path: &Path, folder: &Path) -> bool {
    let mut rest = path.components();
    folder
        .components()
        .all(|want| rest.next().is_some_and(|got| same_component(want, got)))
}

/// `path` made absolute with `.` and `..` folded away, by its spelling alone: no link is followed and nothing is read.
pub(crate) fn folded(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut folded = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                folded.pop();
            }
            part => folded.push(part),
        }
    }
    folded
}

/// [`folded`] with links followed as far as the path exists, so two spellings of one place compare equal.
pub(crate) fn resolved(path: &Path) -> PathBuf {
    let folded = folded(path);
    // The part of the path that does not exist yet is appended as it is spelled.
    let mut missing = Vec::new();
    let mut head = folded.as_path();
    loop {
        if let Ok(real) = head.canonicalize() {
            return missing
                .iter()
                .rev()
                .fold(real, |path, part| path.join(part));
        }
        match (head.parent(), head.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                head = parent;
            }
            _ => return folded,
        }
    }
}

/// ⚠ Case-blind on Windows, where `D:\Mods\Proj` and `d:\mods\proj` are one
/// folder and a dialog may hand back either spelling.
pub(crate) fn same_component(a: Component<'_>, b: Component<'_>) -> bool {
    if cfg!(windows) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// A stored path as this machine reads it: schema 2 and later store a path inside the folder
/// relative to it, and schema 1 stored whatever path the process held, relative to `cwd`.
pub fn resolve_member(stored: &str, schema: u32, folder: &Path, cwd: &Path) -> PathBuf {
    resolve_stored(stored, if schema >= 2 { folder } else { cwd })
}

pub(crate) fn resolve_stored(stored: &str, base: &Path) -> PathBuf {
    if looks_absolute(stored) {
        return PathBuf::from(stored);
    }
    let mut path = base.to_path_buf();
    for part in stored.split(['/', '\\']) {
        if !part.is_empty() && part != "." {
            path.push(part);
        }
    }
    path
}

/// ⚠ Also a Windows drive path on any host: a file outside the folder is
/// stored absolute, and read on Linux `C:\x.ydr` would otherwise be joined
/// under the project as a file named `C:`.
fn looks_absolute(stored: &str) -> bool {
    let bytes = stored.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\');
    drive || Path::new(stored).has_root() || Path::new(stored).is_absolute()
}

/// ⚠ What one walk of a project's folder may read, so a project in a folder of a whole drive stays a list.
pub const SEARCH_FOLDERS: usize = 5_000;
pub const SEARCH_DEPTH: usize = 8;

/// Every `.src` folder in `folder`, and in `members`, as far as the bounds read, and whether a bound cut the walk.
///
/// ⚠ The build and shadow folders and every hidden folder are never walked, a source's own folders
/// are never entered, and a link is never followed, so no walk loops and nothing OpenRage wrote is
/// read back as a source.
pub fn find_sources(folder: &Path, members: &[PathBuf]) -> (Vec<PathBuf>, bool) {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut truncated = false;
    let mut visited = 0;
    let mut stack = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited > SEARCH_FOLDERS {
            truncated = true;
            break;
        }
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".src") {
                found.push(entry.path());
            } else if !is_left_alone(&name) {
                if depth < SEARCH_DEPTH {
                    stack.push((entry.path(), depth + 1));
                } else {
                    truncated = true;
                }
            }
        }
    }
    found.extend(
        members
            .iter()
            .filter(|m| {
                m.is_dir()
                    && m.file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with(".src"))
            })
            .cloned(),
    );
    found.sort();
    found.dedup();
    (found, truncated)
}

/// Every file in `folder` whose name ends with `suffix`, as far as the bounds read, and whether a bound cut the walk.
///
/// ⚠ Walked as [`find_sources`] walks: the build and shadow folders, hidden folders and the inside of a source are never entered, and a link is never followed.
pub fn find_files(folder: &Path, suffix: &str) -> (Vec<PathBuf>, bool) {
    let suffix = suffix.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut truncated = false;
    let mut visited = 0;
    let mut stack = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited > SEARCH_FOLDERS {
            truncated = true;
            break;
        }
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_file() && name.to_ascii_lowercase().ends_with(&suffix) {
                found.push(entry.path());
            } else if kind.is_dir() && !name.ends_with(".src") && !is_left_alone(&name) {
                if depth < SEARCH_DEPTH {
                    stack.push((entry.path(), depth + 1));
                } else {
                    truncated = true;
                }
            }
        }
    }
    found.sort();
    (found, truncated)
}

fn is_left_alone(name: &str) -> bool {
    name.starts_with('.')
        || name.eq_ignore_ascii_case(BUILD_DIR)
        || name.eq_ignore_ascii_case(SHADOW_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_path_is_absolute_on_every_host() {
        let base = Path::new("/projects/p");
        assert_eq!(
            resolve_stored("C:\\games\\x.ydr", base),
            PathBuf::from("C:\\games\\x.ydr")
        );
        assert_eq!(
            resolve_stored("sub\\x.ydr", base),
            base.join("sub").join("x.ydr")
        );
        assert_eq!(
            resolve_stored("./sub/x.ydr", base),
            base.join("sub").join("x.ydr")
        );
    }

    #[test]
    fn a_schema_one_path_is_relative_to_where_the_editor_ran() {
        let folder = Path::new("/projects/p");
        let cwd = Path::new("/elsewhere");
        assert_eq!(
            resolve_member("a/b.ydr", 1, folder, cwd),
            cwd.join("a").join("b.ydr")
        );
        assert_eq!(
            resolve_member("a/b.ydr", 2, folder, cwd),
            folder.join("a").join("b.ydr")
        );
    }

    #[test]
    fn two_spellings_of_one_place_compare_equal_where_it_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("a");
        std::fs::create_dir(&real).unwrap();
        // `b`, `c` and `d` are nowhere.
        let dotted = dir
            .path()
            .join("b")
            .join("..")
            .join("a")
            .join("c")
            .join("d");

        assert_eq!(resolved(&dotted), resolved(&real).join("c").join("d"));
        assert!(is_within(&resolved(&dotted), &resolved(&real)));
        assert!(!is_within(&resolved(&real), &resolved(&dotted)));
        assert!(is_within(&real, &real));
    }

    #[cfg(windows)]
    #[test]
    fn the_folder_matches_whichever_case_a_dialog_spelled_it_in() {
        let folder = Path::new("D:\\Mods\\Proj");
        assert_eq!(
            relative_inside(Path::new("d:\\mods\\proj\\models\\a.ydr"), folder).as_deref(),
            Some("models/a.ydr")
        );
        assert_eq!(relative_inside(Path::new("D:\\Mods\\Proj"), folder), None);
        assert_eq!(
            relative_inside(Path::new("D:\\Mods\\Project\\a.ydr"), folder),
            None
        );
    }

    #[test]
    fn a_file_outside_the_folder_is_stored_absolute() {
        let folder = Path::new("/projects/p");
        assert_eq!(
            relative_inside(Path::new("/projects/p/models/a.ydr"), folder).as_deref(),
            Some("models/a.ydr")
        );
        assert_eq!(
            relative_inside(Path::new("/projects/q/a.ydr"), folder),
            None
        );
        assert_eq!(
            relative_inside(Path::new("/projects/p/../q/a.ydr"), folder),
            None
        );
    }

    #[test]
    fn a_walk_finds_sources_but_not_in_builds_shadows_hidden_folders_or_other_sources() {
        let dir = tempfile::tempdir().unwrap();
        let make = |rel: &str| std::fs::create_dir_all(dir.path().join(rel)).unwrap();
        for rel in [
            "sources/a.textures.src",
            "deep/er/b.textures.src",
            "build/c.textures.src",
            "vfs/d.textures.src",
            ".git/e.textures.src",
            "sources/a.textures.src/nested.textures.src",
        ] {
            make(rel);
        }
        let outside = tempfile::tempdir().unwrap();
        let member = outside.path().join("f.drawable.src");
        std::fs::create_dir_all(&member).unwrap();

        let (found, truncated) = find_sources(dir.path(), std::slice::from_ref(&member));
        let mut wanted = vec![
            dir.path().join("sources/a.textures.src"),
            dir.path().join("deep/er/b.textures.src"),
            member,
        ];
        wanted.sort();
        assert_eq!(found, wanted);
        assert!(!truncated);
    }
}
