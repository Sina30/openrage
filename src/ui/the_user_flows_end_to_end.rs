//! What a person does across the shell, start to finish: a folder opened, a file opened from it, tabs switched and closed, a command run from the palette,
//! every Settings page visited, and the same screens in both themes; with the measures that keep the shell still and even while they do.
//!
//! Read through AccessKit from whole frames of the shell, and clicked where the names are.

use std::time::{Duration, Instant};

use super::frames_of_the_shell::{key, Described, Shell};
use super::the_front_door_on_screen::{a_first_launch, click_on, names, shows};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::panels::settings::SettingsCategory;

fn named<'a>(shell: &'a Shell, name: &str) -> &'a Described {
    (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name))
        .or_else(|| {
            (shell.described.iter()).find(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
}

/// Open `path` as Open file… does once its file is picked, and draw until its tab is there.
fn opens(shell: &mut Shell, path: &std::path::Path) {
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    shell.state.load_file(path.to_path_buf());
    let until = Instant::now() + Duration::from_secs(30);
    while !shell.state.documents.iter().any(|d| d.file_name() == name) {
        assert!(Instant::now() < until, "{name} never opened");
        shell.state.poll_load_result();
        shell.draw(Vec::new());
        std::thread::sleep(Duration::from_millis(5));
    }
    shell.settled();
}

fn active_name(shell: &Shell) -> Option<String> {
    shell.state.active_document().map(|d| d.file_name())
}

#[test]
fn a_folder_is_opened_a_file_opened_from_it_and_the_tabs_switch_and_close() {
    let _serial = crate::ui::theme::serial();
    let mut shell = a_first_launch();
    let dir = shell.open_a_folder();
    shell.settled();
    for row in ["adder.xml", "notes.txt", "blob.bin"] {
        assert!(
            shows(&shell, row),
            "Files lacks `{row}`: {:?}",
            names(&shell)
        );
    }
    let folder = active_name(&shell).expect("the folder is a tab");

    opens(&mut shell, &dir.path().join("notes.txt"));
    assert_eq!(active_name(&shell).as_deref(), Some("notes.txt"));
    assert!(
        shell
            .state
            .active_workspace
            .handles_kind(shell.state.active_document().unwrap().kind),
        "the workspace on show does not draw the file that was opened"
    );

    // The folder's tab brings the folder back, and the file's tab the file.
    click_on(&mut shell, &folder);
    assert_eq!(active_name(&shell), Some(folder.clone()));
    click_on(&mut shell, "notes.txt");
    assert_eq!(active_name(&shell).as_deref(), Some("notes.txt"));

    // The Close of the file's own tab closes that file and nothing else.
    let tab = named(&shell, "notes.txt").bounds;
    let close = (shell.described.iter())
        .filter(|w| w.name.as_deref() == Some("Close"))
        .filter(|w| {
            w.bounds.left() >= tab.left()
                && (w.bounds.center().y - tab.center().y).abs() < tab.height()
        })
        .min_by(|a, b| a.bounds.left().total_cmp(&b.bounds.left()))
        .expect("the tab has a Close")
        .bounds
        .center();
    shell.click(close);
    shell.settled();
    assert!(
        !shell
            .state
            .documents
            .iter()
            .any(|d| d.file_name() == "notes.txt"),
        "Close left the file open"
    );
    assert_eq!(
        active_name(&shell),
        Some(folder),
        "the folder did not come back in front"
    );
}

#[test]
fn a_command_typed_into_the_palette_runs_on_enter() {
    let _serial = crate::ui::theme::serial();
    let mut shell = a_first_launch();
    let _dir = shell.open_a_folder();
    shell.settled();
    assert!(!shell.state.documents.is_empty());

    crate::ui::panels::command_palette::open(&mut shell.state);
    shell.settled();
    assert!(shows(&shell, "Command palette"), "{:?}", names(&shell));
    shell.draw(vec![egui::Event::Text("close all".into())]);
    shell.settled();
    shell.draw(vec![key(egui::Key::Enter)]);
    shell.settled();

    assert!(
        shell.state.documents.is_empty(),
        "Close all ran nothing: {:?}",
        names(&shell)
    );
    assert!(
        !shows(&shell, "Command palette"),
        "the palette stayed open after its command ran"
    );
}

#[test]
fn every_settings_page_draws_and_keeps_its_buttons_inside_the_window() {
    let _serial = crate::ui::theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    for page in SettingsCategory::ALL {
        shell.settings(page);
        // The search field spans the window: what is right of it is past the page.
        let width = named(&shell, "Type to filter settings").bounds.right();
        assert!(
            shows(&shell, page.label()),
            "{page:?} does not name itself: {:?}",
            names(&shell)
        );
        let outside: Vec<_> = (shell.described.iter())
            .filter(|w| w.role == egui::accesskit::Role::Button && w.bounds.right() > width)
            .map(|w| (w.name.clone(), w.bounds))
            .collect();
        assert!(
            outside.is_empty(),
            "{page:?} draws buttons past the window's edge: {outside:?}"
        );
    }
    // A button the page pushes past its edge is clipped away, and is then not described at all.
    shell.settings(SettingsCategory::Shortcuts);
    for button in ["Edit", "Reset"] {
        assert!(
            (shell.described.iter()).any(|w| w.name.as_deref() == Some(button)),
            "no row of the shortcuts shows its {button} button: {:?}",
            names(&shell)
        );
    }
}

#[test]
fn the_buttons_of_the_files_toolbar_are_one_height() {
    let _serial = crate::ui::theme::serial();
    let mut shell = a_first_launch();
    let _dir = shell.open_a_folder();
    shell.settled();
    // Each is named by its glyph and its word; a row of the tree is named by the word alone.
    let heights: Vec<(String, f32)> = [" Expand", " Collapse", " Places", " Name"]
        .iter()
        .map(|word| {
            let button = (shell.described.iter())
                .find(|w| w.name.as_deref().is_some_and(|n| n.ends_with(word)))
                .unwrap_or_else(|| panic!("no button ends in `{word}`: {:?}", names(&shell)));
            (word.to_string(), button.bounds.height())
        })
        .collect();
    assert!(
        heights.iter().all(|(_, h)| *h == heights[0].1),
        "the toolbar's buttons differ in height: {heights:?}"
    );
}

#[test]
fn the_start_page_and_a_folder_say_the_same_in_both_themes() {
    let _serial = crate::ui::theme::serial();
    let said = |choice| {
        let mut shell = Shell::new(choice);
        shell.state.documents.clear();
        shell.state.active_document = None;
        shell.state.active_workspace = Workspace::Browse;
        shell.settled();
        let start = names(&shell);
        let dir = shell.open_a_folder();
        shell.settled();
        // Only the rows: what a worker is still doing when the frame is drawn differs from run to run.
        let rows = ["adder.xml", "notes.txt", "blob.bin", "folder"];
        let folder: Vec<String> = (names(&shell).into_iter())
            .filter(|n| rows.iter().any(|row| n.ends_with(row)))
            .collect();
        drop(dir);
        (start, folder)
    };
    // Sorted: a toast and the rows a worker reads land in the tree in the order they arrive.
    let said = |choice| {
        let (mut start, mut folder) = said(choice);
        start.sort();
        folder.sort();
        (start, folder)
    };
    let (dark_start, dark_folder) = said(ThemeChoice::Dark);
    let (light_start, light_folder) = said(ThemeChoice::Light);
    assert!(!dark_folder.is_empty(), "the folder's rows were not read");
    assert_eq!(dark_start, light_start);
    assert_eq!(dark_folder, light_folder);
}
