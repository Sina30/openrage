//! What a build hands `or_project` so it can cook the `.src` folders of a project: what each cooks to.
//!
//! `or-project` knows no kind of source, so the editor says what each folder cooks to and hands in
//! `or_source::cook::cook_source`, the cook the live recook runs, to make the file in a scratch folder.

use std::path::{Path, PathBuf};

use or_project::links::linked_sources;
use or_project::{find_sources, Container, Cooks, SourceFolder};
use or_source::cook::describe;

/// What each of `folders` cooks to, or why this build cannot cook it.
pub fn describe_all(folders: &[PathBuf]) -> Vec<SourceFolder> {
    folders
        .iter()
        .map(|folder| SourceFolder {
            folder: folder.clone(),
            cooks: describe(folder).map(|said| Cooks {
                kind: said.kind,
                name: said.name,
                extension: said.extension,
                bytes: said.bytes,
                game: said.game,
            }),
        })
        .collect()
}

/// The `.src` folders of the project at `folder` and in `members`, and of the projects whose Files content `container` links, each with what it cooks to.
///
/// ⚠ The walk is the scan's. A walk a bound cut is an error: a build that left a source out
/// without saying so would ship less than the project holds.
pub fn source_folders(
    container: &Container,
    folder: &Path,
    members: &[PathBuf],
) -> Result<Vec<SourceFolder>, String> {
    let (mut found, mut cut) = find_sources(folder, members);
    let (linked, linked_cut) = linked_sources(container, folder);
    found.extend(linked);
    cut |= linked_cut;
    if cut {
        return Err(format!(
            "{} holds more folders than a search for sources reads, so a build cannot tell which sources it has: build from a smaller project folder",
            folder.display()
        ));
    }
    Ok(describe_all(&found))
}

#[cfg(test)]
mod tests {
    use or_project::{Module, ModuleKind};

    use super::*;

    #[test]
    fn the_source_folders_of_a_linked_project_are_found_beside_the_projects_own() {
        let dir = tempfile::tempdir().unwrap();
        let (hub, brand) = (dir.path().join("hub"), dir.path().join("brand"));
        std::fs::create_dir_all(brand.join("art/logo.textures.src")).unwrap();
        std::fs::write(brand.join("art/logo.textures.src/asset.json"), b"{}").unwrap();
        std::fs::write(brand.join("brand.openrage-proj"), b"{}").unwrap();
        std::fs::create_dir_all(hub.join("sources/own.textures.src")).unwrap();
        std::fs::write(hub.join("sources/own.textures.src/asset.json"), b"{}").unwrap();
        let container = Container {
            modules: vec![
                Module::files("files"),
                Module::linked("brand", ModuleKind::Files, "../brand"),
            ],
            outputs: Vec::new(),
        };

        let found = source_folders(&container, &hub, &[]).expect("both walks finished");

        let names: Vec<String> = (found.iter())
            .map(|f| f.folder.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["own.textures.src", "logo.textures.src"],
            "{found:?}"
        );
    }
}
