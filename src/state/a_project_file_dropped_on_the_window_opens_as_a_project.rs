//! A project file dropped on the window is a project wherever it lands, and `.openrage-proj` is a
//! file association OpenRage offers beside the formats it edits.
//!
//! ⚠ Built with `AppState::from_prefs`, which reads no `prefs.json`, over a scratch folder.

use std::path::PathBuf;

use super::front_door::{NewProject, Template};
use super::os_drop::{plan, DropAction, DropTarget};
use super::{AppState, Prefs};
use crate::platform::associations::{candidates, desired};

fn openable(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|ext| ext == "ydr")
}

#[test]
fn a_project_file_is_a_project_aimed_at_a_folder_of_an_archive_or_at_nothing() {
    let project = PathBuf::from("pack.openrage-proj");
    let model = PathBuf::from("a.ydr");
    let paths = vec![project.clone(), model.clone()];

    let nowhere = plan(&paths, DropTarget::Nowhere, None, openable);
    assert_eq!(
        nowhere,
        [
            DropAction::Project(project.clone()),
            DropAction::Open(model)
        ]
    );

    // ⚠ Aimed at an archive's folder everything imports, but a project is not put into an archive.
    let aimed = DropTarget::Folder { doc: 0, folder: 0 };
    let into_a_folder = plan(&paths, aimed, None, openable);
    assert_eq!(into_a_folder[0], DropAction::Project(project));
    assert!(
        matches!(&into_a_folder[1], DropAction::Import { paths, .. } if paths.len() == 1),
        "{into_a_folder:?}"
    );
}

#[test]
fn dropping_a_project_file_opens_the_project_and_one_dropped_over_another_asks() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let make = |name: &str| {
        let mut draft = NewProject::new(Template::Files, dir.path().to_path_buf());
        draft.set_name(name.into());
        draft.create().expect("made").file
    };
    let (first, second) = (make("first"), make("second"));
    let mut state = AppState::from_prefs(Prefs::default());

    state.handle_os_drop(vec![first.clone()], DropTarget::Nowhere);
    assert_eq!(state.project_path.as_deref(), Some(first.as_path()));

    state.handle_os_drop(vec![second.clone()], DropTarget::Nowhere);
    assert_eq!(
        state.project_path.as_deref(),
        Some(first.as_path()),
        "a project is open: the drop asks whether to replace it"
    );
    assert_eq!(
        state.front_door.switching.as_deref(),
        Some(second.as_path())
    );
}

#[test]
fn the_project_file_is_offered_as_a_file_association_and_suggested() {
    let offered = candidates();
    let project = offered
        .iter()
        .find(|c| c.ext == "openrage-proj")
        .expect("the project file is offered");
    assert!(project.suggested, "a file only OpenRage opens is suggested");
    assert_eq!(project.noun, "OpenRage project");

    // The command it registers is the one a launch with a path opens a project with.
    let exe = std::path::Path::new("openrage.exe");
    let wanted = desired(
        exe,
        &[("openrage-proj".to_string(), project.noun.clone())],
        false,
    );
    let command = format!("\"{}\" \"%1\"", exe.display());
    assert!(
        wanted
            .iter()
            .any(|(key, _, data)| key.ends_with(r"open\command") && *data == command),
        "{wanted:?}"
    );
}
