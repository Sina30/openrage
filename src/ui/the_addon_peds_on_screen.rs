//! A project that holds addon peds as it is drawn: the Content panel names the kind and counts the peds,
//! read through AccessKit from whole frames of the shell.

use std::path::Path;
use std::time::{Duration, Instant};

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::Shell;
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

/// A project of one addon ped content, `ped_count` peds in its model (or a model that is no JSON), opened in the shell.
fn a_shell_with_peds(dir: &Path, ped_count: usize, readable: bool) -> Shell {
    let folder = dir.join("modules").join("peds");
    std::fs::create_dir_all(&folder).unwrap();
    let mut model = or_addonped::AddonPeds::new();
    for n in 0..ped_count {
        model.peds.push(or_addonped::Ped::new(&format!("ped_{n}")));
    }
    model.write(&folder).unwrap();
    if !readable {
        std::fs::write(folder.join("addonped.json"), b"not json").unwrap();
    }
    let container = or_project::Container {
        modules: vec![or_project::Module::addon_ped("peds", "modules/peds")],
        outputs: vec![or_project::Output::fivem_resource(
            "fivem",
            "ped_pack",
            &["peds"],
        )],
    };
    let file = dir.join("peds.openrage-proj");
    or_project::Project::create(&file, &container, &[]).unwrap();
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.settled();
    shell
}

fn names(shell: &Shell) -> Vec<String> {
    shell
        .described
        .iter()
        .filter_map(|widget| widget.name.clone())
        .collect()
}

fn shows(shell: &Shell, text: &str) -> bool {
    names(shell).iter().any(|name| name.contains(text))
}

/// Draw frames, polling what the workers left, until `done`.
fn until(shell: &mut Shell, what: &str, done: impl Fn(&Shell) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        shell.state.poll_workbench();
        shell.draw(Vec::new());
        if done(shell) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what} never happened: {:?}",
            names(shell)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_content_panel_names_addon_peds_and_counts_the_peds_the_model_holds() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_shell_with_peds(dir.path(), 2, true);
    shell.state.active_workspace = Workspace::Project;
    until(&mut shell, "the count", |s| {
        shows(s, "Addon ped  ·  2 peds")
    });

    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Project workspace over addon peds", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_model_that_cannot_be_read_is_said_and_not_counted() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_shell_with_peds(dir.path(), 1, false);
    shell.state.active_workspace = Workspace::Project;
    until(&mut shell, "the row", |s| {
        shows(s, "Addon ped  ·  peds not read")
    });
}
