//! The dialog that says what garments with unreadable names are, as a person meets it: a button beside the counts opens
//! it for the garments that need it, and one answer sets the ped and the slot of every one of them, read through
//! AccessKit from whole frames of the shell.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::frames_of_the_shell::Shell;
use crate::state::front_door::{NewProject, Template};
use crate::state::ThemeChoice;
use crate::ui::theme;

fn a_clothing_shell(dir: &Path) -> Shell {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("demo".into());
    let file = draft.create().expect("a clothing project").file;
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.settled();
    shell
}

fn names(shell: &Shell) -> Vec<String> {
    (shell.described.iter())
        .filter_map(|widget| widget.name.clone())
        .collect()
}

fn shows(shell: &Shell, text: &str) -> bool {
    names(shell).iter().any(|name| name.contains(text))
}

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

fn pack(shell: &Shell) -> Option<&or_wardrobe::Wardrobe> {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| &read.wardrobe)
}

fn put(dir: &Path, list: &[(&str, &[u8])]) -> Vec<PathBuf> {
    list.iter()
        .map(|(name, body)| {
            let path = dir.join("drop").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, or_source::sample::as_a_game_file(name, body)).unwrap();
            path
        })
        .collect()
}

fn click_on(shell: &mut Shell, name: &str) {
    let at = (shell.described.iter())
        .find(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)))
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
        .bounds
        .center();
    shell.click(at);
    shell.settled();
}

#[test]
fn the_button_beside_the_counts_opens_the_dialog_and_one_answer_says_what_all_three_are() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", |s| pack(s).is_some());
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("the panel is drawn");
    shell.drop_files(
        panel.center(),
        put(
            dir.path(),
            &[
                ("blue shirt.ydd", b"one"),
                ("red shirt.ydd", b"two"),
                ("old shirt.ydd", b"three"),
            ],
        ),
    );
    until(&mut shell, "the garments", |s| {
        pack(s).is_some_and(|w| w.garments.len() == 3) && shows(s, "Say what they are")
    });
    shell.settled();
    assert!(
        !shows(&shell, "Say what these are"),
        "the dialog waits to be asked for"
    );
    let steps = shell.state.workbench.wardrobe.undo.len();

    click_on(&mut shell, "Say what they are");
    assert!(shows(&shell, "Say what these are"), "{:?}", names(&shell));
    assert!(
        shows(&shell, "The names of 3 garments"),
        "{:?}",
        names(&shell)
    );
    let apply = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Apply"))
        .expect("an Apply button");
    assert!(apply.disabled, "nothing is chosen yet");

    click_on(&mut shell, "Male ped");
    click_on(&mut shell, "Slot to set");
    click_on(&mut shell, "Tops (jbib)");
    click_on(&mut shell, "Apply");

    // The scan that reads the answer's pack can come before the worker's answer, which is what makes the step.
    until(&mut shell, "the answer", |s| {
        pack(s).is_some_and(|w| {
            w.garments.iter().all(|g| {
                g.ped == Some(or_wardrobe::Ped::Male)
                    && g.slot == Some(or_wardrobe::Slot::Component(or_wardrobe::Component::Jbib))
            })
        }) && s.state.workbench.wardrobe.is_idle()
    });
    assert!(
        !shows(&shell, "Say what these are"),
        "the dialog closes on its answer"
    );
    assert!(
        !names(&shell).iter().any(|n| n == "Say what they are…"),
        "nothing is left unsaid, so the button is gone: {:?}",
        names(&shell)
    );
    assert_eq!(
        shell.state.workbench.wardrobe.undo.len(),
        steps + 1,
        "the three are one step"
    );
}

#[test]
fn cancel_closes_the_dialog_and_changes_nothing() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", |s| pack(s).is_some());
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("the panel is drawn");
    shell.drop_files(
        panel.center(),
        put(
            dir.path(),
            &[("blue shirt.ydd", b"one"), ("red shirt.ydd", b"two")],
        ),
    );
    until(&mut shell, "the garments", |s| {
        pack(s).is_some_and(|w| w.garments.len() == 2) && shows(s, "Say what they are")
    });
    shell.settled();
    let before = pack(&shell).cloned();

    click_on(&mut shell, "Say what they are");
    click_on(&mut shell, "Female ped");
    click_on(&mut shell, "Cancel");

    assert!(!shows(&shell, "Say what these are"));
    assert_eq!(pack(&shell).cloned(), before);
}
