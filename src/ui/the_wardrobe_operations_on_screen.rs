//! The operations on a garment as a person meets them: the Garment panel offers each one with a name a screen reader can
//! speak, a texture replaced from its row is one click from Undo, and a row's menu copies the garment to the other ped.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are; the file dialogs are the
//! stand-ins of a test build, which answer with the path a case names.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::Shell;
use crate::platform::file_dialogs;
use crate::state::front_door::{NewProject, Template};
use crate::state::ThemeChoice;
use crate::ui::theme;

const C: &str = "mp_m_freemode_01_mp_m_demo";

pub(super) fn a_clothing_shell(dir: &Path) -> Shell {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("demo".into());
    let file = draft.create().expect("a clothing project").file;
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.settled();
    shell
}

pub(super) fn names(shell: &Shell) -> Vec<String> {
    (shell.described.iter())
        .filter_map(|widget| widget.name.clone())
        .collect()
}

pub(super) fn shows(shell: &Shell, text: &str) -> bool {
    names(shell).iter().any(|name| name.contains(text))
}

pub(super) fn until(shell: &mut Shell, what: &str, done: impl Fn(&Shell) -> bool) {
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

pub(super) fn garments(shell: &Shell) -> usize {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map_or(0, |read| read.wardrobe.garments.len())
}

pub(super) fn put(dir: &Path, list: &[(&str, &[u8])]) -> Vec<PathBuf> {
    list.iter()
        .map(|(name, body)| {
            let path = dir.join("drop").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, or_source::sample::as_a_game_file(name, body)).unwrap();
            path
        })
        .collect()
}

pub(super) fn click_on(shell: &mut Shell, name: &str) {
    // A button that carries an icon is named with it in front.
    let widget = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name))
        .or_else(|| {
            (shell.described.iter()).find(|w| {
                w.role == egui::accesskit::Role::Button
                    && w.name.as_deref().is_some_and(|n| n.ends_with(name))
            })
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    shell.click(at);
    shell.settled();
}

/// A shell on a pack of two jackets, the first with two textures, and the first one selected.
pub(super) fn a_shell_with_two_jackets(dir: &Path) -> (Shell, PathBuf) {
    let mut shell = a_clothing_shell(dir);
    until(&mut shell, "the pack", |s| {
        s.state.wardrobe_pack().is_some()
    });
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("the panel is drawn");
    shell.drop_files(
        panel.center(),
        put(
            dir,
            &[
                (&format!("{C}^jbib_000_u.ydd"), b"jacket 0"),
                (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
                (&format!("{C}^jbib_diff_000_b_uni.ytd"), b"blue"),
                (&format!("{C}^jbib_001_u.ydd"), b"jacket 1"),
                (&format!("{C}^jbib_diff_001_a_uni.ytd"), b"green"),
            ],
        ),
    );
    until(&mut shell, "the jackets", |s| garments(s) == 2);
    let module = shell.state.wardrobe_module_id().unwrap();
    let first = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| read.wardrobe.garments[0].id.clone())
        .unwrap();
    shell.state.show_garment(&module, first);
    shell.settled();
    (shell, dir.join("demo").join("clothing"))
}

#[test]
fn the_garment_panel_offers_each_operation_with_a_name_a_screen_reader_can_speak() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (shell, _) = a_shell_with_two_jackets(dir.path());

    for text in [
        "Replace model…",
        "Use a model source folder…",
        "Add texture…",
        "Move garment earlier",
        "Move garment later",
        "Duplicate to the female ped",
        "Export garment files…",
        "Package garment…",
        "Replace texture a",
        "Replace texture b",
        "More for texture a",
        "Tint of texture b",
    ] {
        assert!(
            shows(&shell, text),
            "the Garment panel lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Garment panel with its operations", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_texture_replaced_from_its_row_is_one_click_from_undo() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, pack) = a_shell_with_two_jackets(dir.path());
    let texture = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments[0].variations[1].texture.clone())
        .unwrap();
    let now = |shell: &Shell| {
        (shell.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garments[0].variations[1].texture.clone())
            .unwrap()
    };
    assert!(
        pack.join(&texture).join("asset.json").is_file(),
        "the texture is a source"
    );
    let picked = put(dir.path(), &[("better.ytd", b"a better blue")]).remove(0);

    // Replace texture… opens the picker; Browse… takes a file of elsewhere, which asks before it is imported.
    file_dialogs::answer_next(picked);
    click_on(&mut shell, "Replace texture b");
    until(&mut shell, "the picker", |s| shows(s, "Choose the texture"));
    click_on(&mut shell, "Browse…");
    until(&mut shell, "the question", |s| {
        shows(s, "Only reference it")
    });
    click_on(&mut shell, "Import into the project");
    until(&mut shell, "the replacement", |s| {
        now(s).ends_with("better.textures.src")
    });
    until(&mut shell, "its step", |s| shows(s, "Undo texture"));
    assert!(
        pack.join(&texture).join("asset.json").is_file(),
        "the source it replaced is not touched"
    );

    click_on(&mut shell, "Undo texture");
    until(&mut shell, "the undo", |s| now(s) == texture);
}

#[test]
fn a_rows_menu_copies_its_garment_to_the_other_ped_and_offers_to_move_it_in_its_slot() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let row = (shell.described.iter())
        .find(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.starts_with("000, place 000"))
        })
        .unwrap_or_else(|| panic!("no row: {:?}", names(&shell)))
        .bounds;
    let at = egui::pos2(row.right() - 4.0, row.center().y);
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw(vec![
        egui::Event::PointerMoved(at),
        press(true),
        press(false),
    ]);
    shell.settled();
    for text in [
        "Move earlier in its slot",
        "Move later in its slot",
        "Duplicate to the female ped",
    ] {
        assert!(
            shows(&shell, text),
            "the row's menu lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the menu of a row", &mut found);
    assert!(found.is_empty(), "{found:?}");

    // The Garment panel has a button of the same name, which a toast can lie over: the menu's is the one that opened where the row was pressed
    // (the order a frame lists its widgets in is no order of drawing, so the nearest is what tells them apart).
    let item = (shell.described.iter())
        .filter(|w| w.name.as_deref() == Some("Duplicate to the female ped"))
        .min_by(|a, b| {
            let (da, db) = (
                a.bounds.center().distance(at),
                b.bounds.center().distance(at),
            );
            da.total_cmp(&db)
        })
        .expect("the menu's item")
        .bounds
        .center();
    shell.click(item);
    shell.settled();
    until(&mut shell, "the copy", |s| garments(s) == 3);
}

#[test]
fn a_headings_menu_acts_on_every_garment_under_it_and_one_undo_brings_them_back() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let heading = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Male · Tops · 2 garments"))
        .unwrap_or_else(|| panic!("no heading: {:?}", names(&shell)))
        .bounds;
    let at = heading.center();
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw(vec![
        egui::Event::PointerMoved(at),
        press(true),
        press(false),
    ]);
    shell.settled();
    for text in [
        "Remove these 2 garments from the pack (their files stay)",
        "Set the ped to Female for these 2 garments",
        "Set the slot for these 2 garments",
        "Export the files of these 2 garments…",
    ] {
        assert!(
            shows(&shell, text),
            "the heading's menu lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the menu of a heading", &mut found);
    assert!(found.is_empty(), "{found:?}");

    click_on(
        &mut shell,
        "Remove these 2 garments from the pack (their files stay)",
    );
    until(&mut shell, "the removal", |s| garments(s) == 0);
    until(&mut shell, "its step", |s| shows(s, "Undo removal"));
    click_on(&mut shell, "Undo removal");
    until(&mut shell, "the undo", |s| garments(s) == 2);
}
