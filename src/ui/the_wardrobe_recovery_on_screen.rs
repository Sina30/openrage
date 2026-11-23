//! The Wardrobe's recovery as it is drawn: Redo beside Undo, the card of a pack that does not read, the pack's versions and the word
//! over a pack another window edits; each read through AccessKit from whole frames of the shell and clicked where the names are.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::Shell;
use crate::state::front_door::{NewProject, Template};
use crate::state::ThemeChoice;

const C: &str = "mp_m_freemode_01_mp_m_demo";

fn a_clothing_shell(dir: &Path) -> (Shell, PathBuf) {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("demo".into());
    let file = draft.create().expect("a clothing project").file;
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file.clone());
    shell.settled();
    (shell, file)
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

fn click_on(shell: &mut Shell, name: &str) {
    let widget = shell
        .described
        .iter()
        .find(|w| w.name.as_deref() == Some(name))
        .or_else(|| {
            (shell.described.iter()).find(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    shell.click(at);
    shell.settled();
}

/// Click the lowest widget named `name`: a list that is drawn newest first has the older entry under the newer, whatever order the widgets are described in, which follows their ids and so the header above them.
fn click_on_the_lowest(shell: &mut Shell, name: &str) {
    let at = (shell.described.iter())
        .filter(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
        .map(|w| w.bounds.center())
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    shell.click(at);
    shell.settled();
}

fn dropped(dir: &Path, list: &[(&str, &[u8])]) -> Vec<PathBuf> {
    list.iter()
        .map(|(name, body)| {
            let path = dir.join("drop").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, or_source::sample::as_a_game_file(name, body)).unwrap();
            path
        })
        .collect()
}

fn garments(shell: &Shell) -> usize {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map_or(0, |read| read.wardrobe.garments.len())
}

/// A shell whose pack holds a jacket and some trousers, drawn.
fn a_shell_with_clothes(dir: &Path) -> (Shell, PathBuf) {
    let (mut shell, file) = a_clothing_shell(dir);
    shell.state.add_clothes(dropped(
        dir,
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^lowr_000_u.ydd"), b"trousers"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    ));
    until(&mut shell, "the garments", |s| {
        garments(s) == 2 && shows(s, "Tops")
    });
    (shell, file)
}

#[test]
fn redo_sits_beside_undo_and_makes_the_change_again() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_clothes(dir.path());
    assert!(shows(&shell, "Redo"), "{:?}", names(&shell));
    let redo = |shell: &Shell| {
        shell
            .described
            .iter()
            .find(|w| {
                w.role == egui::accesskit::Role::Button
                    && w.name.as_deref().is_some_and(|n| n.ends_with("Redo"))
            })
            .map(|w| w.disabled)
    };
    assert_eq!(redo(&shell), Some(true), "nothing was taken back yet");

    click_on(&mut shell, "Undo add (2)");
    until(&mut shell, "the undo", |s| garments(s) == 0);
    until(&mut shell, "a change taken back to be offered again", |s| {
        redo(s) == Some(false)
    });

    click_on(&mut shell, "Redo");
    until(&mut shell, "the redo", |s| garments(s) == 2);
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Wardrobe with Redo", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_pack_that_does_not_read_is_a_card_that_restores_its_last_good_version() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, file) = a_shell_with_clothes(dir.path());
    let manifest = file
        .parent()
        .unwrap()
        .join("clothing")
        .join("wardrobe.json");
    std::fs::write(&manifest, b"{ \"garments\": oops").unwrap();
    shell.state.workbench.want_scan();

    until(&mut shell, "the card", |s| {
        shows(s, "This pack could not be read")
    });
    for text in [
        "Restore the last good version",
        "Reveal",
        "The newest earlier version that reads is from",
        "2 garments",
    ] {
        assert!(
            shows(&shell, text),
            "the card lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the card of a pack that does not read", &mut found);
    assert!(found.is_empty(), "{found:?}");

    click_on(&mut shell, "Reveal");
    assert!(crate::platform::shell::fake::was_revealed(&manifest));

    click_on(&mut shell, "Restore the last good version");
    until(&mut shell, "the pack reads again", |s| garments(s) == 2);
    assert!(!shows(&shell, "This pack could not be read"));
}

#[test]
fn the_versions_menu_lists_what_the_pack_kept_and_restoring_one_is_a_step_of_undo() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_clothes(dir.path());
    let id = shell
        .state
        .wardrobe_pack()
        .and_then(|p| p.read.as_ref().ok())
        .unwrap()
        .wardrobe
        .garments[0]
        .id
        .clone();
    shell
        .state
        .edit_garment(id, or_wardrobe::edit::Edit::Label(Some("renamed".into())));
    until(&mut shell, "the rename", |s| shows(s, "renamed"));

    click_on(&mut shell, "Versions");
    until(&mut shell, "the menu", |s| {
        shows(s, "Restore the pack as it was")
    });
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Versions menu", &mut found);
    assert!(found.is_empty(), "{found:?}");

    // The newest version is the pack as it is; the one before it is the pack as it was dropped.
    click_on_the_lowest(&mut shell, "Restore the pack as it was");
    until(&mut shell, "the restore", |s| {
        (s.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.wardrobe.garments.iter().all(|g| g.label.is_none()))
    });
    assert!(
        (shell.state.workbench.wardrobe.undo_label())
            .is_some_and(|label| label.starts_with("Restored the pack to how it was")),
        "{:?}",
        shell.state.workbench.wardrobe.undo_label()
    );
}

#[test]
fn no_original_is_offered_for_a_texture_and_a_pack_held_elsewhere_says_so() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, file) = a_shell_with_clothes(dir.path());
    let folder = file.parent().unwrap().join("clothing");
    let pack = shell
        .state
        .wardrobe_pack()
        .and_then(|p| p.read.as_ref().ok())
        .unwrap()
        .wardrobe
        .clone();
    let jacket = pack
        .garments
        .iter()
        .find(|g| !g.variations.is_empty())
        .expect("a jacket with a texture");
    // A pack adopted from disk names a compiled texture, and an earlier session left the index of an original it kept of it.
    let texture = "textures/adopted.ytd".to_string();
    std::fs::create_dir_all(folder.join("textures")).unwrap();
    std::fs::write(
        folder.join(&texture),
        or_source::sample::textures_of("adopted"),
    )
    .unwrap();
    std::fs::create_dir_all(folder.join(".openrage/originals")).unwrap();
    std::fs::write(
        folder.join(".openrage/originals/index.json"),
        format!(r#"{{"files":{{"{texture}":{{"sha256":"00","size":1}}}}}}"#),
    )
    .unwrap();
    or_wardrobe::edit::update(&folder, |w| {
        let at = w.garments.iter().position(|g| g.id == jacket.id).unwrap();
        w.garments[at].variations[0].texture = Some(texture.clone());
        Ok(())
    })
    .unwrap();
    shell.state.select_garment(
        &shell.state.wardrobe_module_id().unwrap(),
        Some(jacket.id.clone()),
    );
    shell.state.workbench.want_scan();
    until(&mut shell, "the texture", |s| shows(s, "adopted.ytd"));
    assert!(
        !shows(&shell, "Restore the original"),
        "nothing writes over a file any more, so there is no original to bring back: {:?}",
        names(&shell)
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the garment with a texture", &mut found);
    assert!(found.is_empty(), "{found:?}");

    // Another window holds the pack: this one says it reads and does not write.
    let module = shell.state.wardrobe_module_id().unwrap();
    shell.state.workbench.wardrobe.locked_out.insert(module);
    until(&mut shell, "the word", |s| {
        shows(s, "Another OpenRage window is editing this pack")
    });
    assert!(shows(&shell, "Try again"));
    mute(&shell, "the pack held elsewhere", &mut found);
    assert!(found.is_empty(), "{found:?}");
}
