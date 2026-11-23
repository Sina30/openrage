//! The front door as it is drawn: the start page with the installs, the recent projects as cards, the chips that start a project and the ways in;
//! the New project dialog; the project menu of the status bar; and the banner over an opened folder.
//!
//! Read through AccessKit from whole frames of the shell, and clicked where the names are.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::frames_of_the_shell::{key, Shell};
use crate::platform::file_dialogs;
use crate::platform::new_window::fake;
use crate::state::front_door::{NewProject, Template};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

/// A shell with nothing open: the start page is what a first launch shows.
pub(super) fn a_first_launch() -> Shell {
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.documents.clear();
    shell.state.active_document = None;
    shell.state.active_workspace = Workspace::Browse;
    shell.settled();
    shell
}

pub(super) fn names(shell: &Shell) -> Vec<String> {
    shell
        .described
        .iter()
        .filter_map(|widget| widget.name.clone())
        .collect()
}

pub(super) fn shows(shell: &Shell, text: &str) -> bool {
    names(shell).iter().any(|name| name.contains(text))
}

/// Draw frames until `text` is on screen; the cards of the start page are read on a worker.
pub(super) fn waits_for(shell: &mut Shell, text: &str) {
    let until = Instant::now() + Duration::from_secs(30);
    while !shows(shell, text) {
        assert!(
            Instant::now() < until,
            "`{text}` never came on screen: {:?}",
            names(shell)
        );
        std::thread::sleep(Duration::from_millis(10));
        shell.draw(Vec::new());
    }
}

/// Draw frames until `text` is on screen, as `waits_for` does, and let the project's own workers answer between them.
fn waits_for_the_project(shell: &mut Shell, text: &str) {
    let until = Instant::now() + Duration::from_secs(30);
    while !shows(shell, text) {
        assert!(
            Instant::now() < until,
            "`{text}` never came on screen: {:?}",
            names(shell)
        );
        std::thread::sleep(Duration::from_millis(10));
        shell.state.poll_workbench();
        shell.draw(Vec::new());
    }
}

/// Draw frames until the project the dialog is making on a worker has opened.
pub(super) fn until_the_project_opens(shell: &mut Shell) {
    let until = Instant::now() + Duration::from_secs(30);
    while shell.state.project_path.is_none() {
        assert!(Instant::now() < until, "the project was never made");
        std::thread::sleep(Duration::from_millis(5));
        shell.draw(Vec::new());
    }
}

/// Click the widget named exactly `name`, or with `name` in its name when none is named so.
pub(super) fn click_on(shell: &mut Shell, name: &str) {
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

pub(super) fn a_project(dir: &Path, name: &str, template: Template) -> PathBuf {
    let mut draft = NewProject::new(template, dir.to_path_buf());
    draft.set_name(name.to_string());
    draft.create().expect("a project").file
}

#[test]
fn a_first_launch_shows_the_ways_in_and_goals_only_for_content_that_exists() {
    let shell = a_first_launch();
    for way in [
        "New project…",
        "Open project…",
        "Import built pack…",
        "Open folder…",
        "Open file…",
        "Files project",
        "Clothing pack",
        "Project from a folder",
        "Open an install",
    ] {
        assert!(shows(&shell, way), "the start page does not offer `{way}`");
    }
    assert!(
        !shows(&shell, "Game keys"),
        "the keys have a block of their own with no install opened: {:?}",
        names(&shell)
    );
    for later in ["A map or props", "Wardrobe"] {
        assert!(
            !shows(&shell, later),
            "the start page offers `{later}`, whose content does not exist yet"
        );
    }
}

#[test]
fn new_project_opens_the_dialog_on_the_gallery_of_the_templates_that_exist() {
    let mut shell = a_first_launch();
    click_on(&mut shell, "New project…");

    assert!(shell.state.front_door.new_project.is_some());
    for template in [
        "Empty project",
        "Files project",
        "Clothing pack",
        "Project from a folder",
        "Search templates",
    ] {
        assert!(shows(&shell, template), "the gallery lacks `{template}`");
    }
    assert!(
        shows(&shell, "Opens on"),
        "the right side says which screen it opens on"
    );
}

/// Where the Next button of the dialog's gallery is, on screen.
fn next_at(shell: &Shell) -> egui::Pos2 {
    (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Next"))
        .map(|w| w.bounds.center())
        .expect("the gallery has a Next button")
}

#[test]
fn the_dialog_keeps_its_height_as_frames_go_by() {
    let mut shell = a_first_launch();
    shell.state.start_new_project(Template::Empty);
    // A modal sizes itself over its first frames, then holds.
    for _ in 0..10 {
        shell.draw(Vec::new());
    }
    let first = next_at(&shell);

    for _ in 0..40 {
        shell.draw(Vec::new());
    }

    let later = next_at(&shell);
    assert!(
        (later.y - first.y).abs() < 1.0,
        "the dialog grew: Next was at {first:?} and is at {later:?} forty frames on"
    );
}

/// The window the shell is drawn in, in points.
const SCREEN_HEIGHT: f32 = 900.0;

#[test]
fn the_details_of_every_template_fit_the_window() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for template in Template::ALL {
        let mut shell = a_first_launch();
        let mut draft = if template == Template::Adopt {
            NewProject::adopting(dir.path())
        } else {
            let mut draft = NewProject::new(template, dir.path().to_path_buf());
            draft.set_name("police".into());
            draft
        };
        draft.step = crate::state::front_door::Step::Details;
        shell.state.front_door.new_project = Some(draft);
        for _ in 0..40 {
            shell.draw(Vec::new());
        }

        let buttons: Vec<_> = (shell.described.iter())
            .filter(|w| {
                matches!(
                    w.name.as_deref(),
                    Some("Create" | "Make project" | "Cancel")
                )
            })
            .collect();
        assert_eq!(buttons.len(), 2, "{template:?}: {:?}", names(&shell));
        for button in buttons {
            assert!(
                button.bounds.min.y > 0.0 && button.bounds.max.y < SCREEN_HEIGHT,
                "{template:?}: `{:?}` is off the {SCREEN_HEIGHT} point window at {:?}",
                button.name,
                button.bounds
            );
        }
    }
}

#[test]
fn escape_closes_the_dialog_and_the_question_a_second_project_asks() {
    let mut shell = a_first_launch();
    shell.state.start_new_project(Template::Empty);
    shell.settled();
    assert!(shell.state.front_door.new_project.is_some());
    shell.draw(vec![key(egui::Key::Escape)]);
    assert!(
        shell.state.front_door.new_project.is_none(),
        "Escape left the dialog open"
    );

    shell.state.front_door.switching = Some(PathBuf::from("D:/mods/police/police.openrage-proj"));
    shell.settled();
    shell.draw(vec![key(egui::Key::Escape)]);
    assert!(
        shell.state.front_door.switching.is_none(),
        "Escape left the question open"
    );
    assert!(!fake::was_opened(Path::new(
        "D:/mods/police/police.openrage-proj"
    )));
}

#[test]
fn a_chip_starts_the_dialog_on_its_template() {
    let mut shell = a_first_launch();
    click_on(&mut shell, "Files project");
    let draft = shell
        .state
        .front_door
        .new_project
        .as_ref()
        .expect("the dialog is open");
    assert_eq!(draft.template, Template::Files);
}

#[test]
fn nothing_is_written_while_the_dialog_is_open_and_create_makes_exactly_the_tree_it_shows() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    shell.state.start_new_project(Template::Files);
    shell.settled();
    click_on(&mut shell, "Next");
    let draft = shell.state.front_door.new_project.as_mut().expect("open");
    draft.place = dir.path().display().to_string();
    shell.settled();
    shell.draw(vec![egui::Event::Text("police".into())]);
    shell.settled();

    let draft = shell.state.front_door.new_project.as_ref().expect("open");
    assert_eq!(draft.name, "police", "the name was typed");
    assert_eq!(draft.resource, "police", "and the resource name follows it");
    for line in ["police/", "sources/", "police.openrage-proj"] {
        assert!(
            shows(&shell, line),
            "the tree does not show `{line}`: {:?}",
            names(&shell)
        );
    }
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("a folder").count(),
        0,
        "the dialog wrote something before Create"
    );

    click_on(&mut shell, "Create");

    until_the_project_opens(&mut shell);

    let file = dir.path().join("police").join("police.openrage-proj");
    assert!(file.is_file());
    assert!(dir.path().join("police").join("sources").is_dir());
    assert_eq!(shell.state.project_path.as_deref(), Some(file.as_path()));
    assert_eq!(shell.state.active_workspace, Workspace::Project);
    assert!(
        shell.state.front_door.new_project.is_none(),
        "the dialog closed"
    );
}

#[test]
fn a_click_is_not_lost_to_a_toast_that_first_shows_in_the_frame_it_lands_in() {
    let mut shell = a_first_launch();
    shell.state.start_new_project(Template::Files);
    shell.settled();

    // A toast a poll just made is drawn once before the click comes, and that first pass is a sizing pass over the dialog.
    shell.state.show_toast("A toast shown as the click comes");
    shell.draw(Vec::new());
    click_on(&mut shell, "Next");
    shell.draw(vec![egui::Event::Text("police".into())]);
    shell.settled();

    let draft = shell.state.front_door.new_project.as_ref().expect("open");
    assert_eq!(
        draft.name, "police",
        "Next opened the page that takes the name"
    );
}

#[test]
fn a_refused_project_says_why_and_create_waits() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::create_dir(dir.path().join("police")).expect("a folder");
    std::fs::write(dir.path().join("police/readme.txt"), b"mine").expect("a file");
    let mut shell = a_first_launch();
    shell.state.start_new_project(Template::Files);
    let mut draft = shell.state.front_door.new_project.take().expect("open");
    draft.place = dir.path().display().to_string();
    draft.set_name("police".into());
    draft.step = crate::state::front_door::Step::Details;
    shell.state.front_door.new_project = Some(draft);
    shell.settled();

    assert!(
        shows(&shell, "already exists and is not empty"),
        "{:?}",
        names(&shell)
    );
    let create = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Create"))
        .expect("a Create button");
    assert!(create.disabled, "Create runs on a refused project");
}

#[test]
fn adopting_names_the_folder_and_says_nothing_is_moved() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(dir.path().join("a.lua"), b"x").expect("a file");
    let mut shell = a_first_launch();
    let mut draft = NewProject::adopting(dir.path());
    draft.focus = false;
    shell.state.front_door.new_project = Some(draft);
    shell.settled();

    assert!(shows(&shell, "Folder to make a project of"));
    assert!(shows(&shell, "Nothing is moved or rewritten"));
    click_on(&mut shell, "Make project");
    until_the_project_opens(&mut shell);
    // A folder with a leading dot is hidden, and a project name cannot start with one.
    let name = dir
        .path()
        .file_name()
        .expect("a name")
        .to_string_lossy()
        .trim_start_matches('.')
        .to_string();
    let file = dir.path().join(format!("{name}.openrage-proj"));
    assert_eq!(shell.state.project_path.as_deref(), Some(file.as_path()));
    assert_eq!(
        std::fs::read(dir.path().join("a.lua")).expect("a file"),
        b"x"
    );
}

#[test]
fn a_returning_launch_shows_each_recent_project_as_a_card_above_what_starts_a_project() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let police = a_project(dir.path(), "police", Template::Files);
    let empty = a_project(dir.path(), "nothing", Template::Empty);
    let mut shell = a_first_launch();
    shell.state.prefs.touch_recent_project(empty);
    shell.state.prefs.touch_recent_project(police);
    shell.draw(Vec::new());

    waits_for(&mut shell, "Not built");
    for card in [
        "police",
        "nothing",
        "Files · 0 files",
        "No build declared",
        "opened just now",
    ] {
        assert!(
            shows(&shell, card),
            "a card lacks `{card}`: {:?}",
            names(&shell)
        );
    }
    let top = |name: &str| {
        (shell.described.iter())
            .filter(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
            .map(|w| w.bounds.min.y)
            .min_by(f32::total_cmp)
            .unwrap_or_else(|| panic!("nothing shows `{name}`: {:?}", names(&shell)))
    };
    assert!(
        top("police") < top("Clothing pack"),
        "what starts a project stands above what was open"
    );
    assert!(
        !shows(&shell, "Read the guide"),
        "the row for someone who does not know where to start is for a first launch"
    );
}

#[test]
fn a_card_opens_its_project_and_a_second_project_asks_where() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let police = a_project(dir.path(), "police", Template::Files);
    let sheriff = a_project(dir.path(), "sheriff", Template::Files);
    let mut shell = a_first_launch();
    shell.state.prefs.touch_recent_project(sheriff.clone());
    shell.state.prefs.touch_recent_project(police.clone());
    shell.draw(Vec::new());
    waits_for(&mut shell, "Not built");

    click_on(&mut shell, "police");
    assert_eq!(shell.state.project_path.as_deref(), Some(police.as_path()));

    shell.state.show_start_page();
    shell.settled();
    waits_for(&mut shell, "sheriff");
    click_on(&mut shell, "sheriff");
    assert_eq!(
        shell.state.project_path.as_deref(),
        Some(police.as_path()),
        "it asked first"
    );
    assert_eq!(
        shell.state.front_door.switching.as_deref(),
        Some(sheriff.as_path())
    );
    assert!(shows(&shell, "Open in new window"));
    assert!(shows(&shell, "Replace police"));

    // The cards behind the question have a button of this name too: the question's is the one nearest the middle of the window.
    let middle = egui::pos2(800.0, 450.0);
    let at = (shell.described.iter())
        .filter(|w| {
            w.role == egui::accesskit::Role::Button
                && w.name.as_deref() == Some("Open in new window")
        })
        .min_by(|a, b| {
            (a.bounds.center() - middle)
                .length()
                .total_cmp(&(b.bounds.center() - middle).length())
        })
        .expect("the question's button")
        .bounds
        .center();
    shell.click(at);
    shell.settled();
    assert!(fake::was_opened(&sheriff), "a second process was asked for");
    assert_eq!(shell.state.project_path.as_deref(), Some(police.as_path()));
}

/// Click the button whose name ends with `name`: the project's name in the status bar has an icon before it.
fn click_button_ending(shell: &mut Shell, name: &str) {
    let widget = (shell.described.iter())
        .find(|w| {
            w.role == egui::accesskit::Role::Button
                && w.name.as_deref().is_some_and(|n| n.ends_with(name))
        })
        .unwrap_or_else(|| panic!("no button ends with `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    shell.click(at);
    shell.settled();
}

#[test]
fn a_project_that_moved_is_listed_as_not_found_and_remove_forgets_it_and_keeps_its_files() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let kept = a_project(dir.path(), "kept", Template::Empty);
    let gone = dir.path().join("gone").join("gone.openrage-proj");
    let mut shell = a_first_launch();
    shell.state.prefs.touch_recent_project(kept.clone());
    shell.state.prefs.touch_recent_project(gone.clone());
    shell.draw(Vec::new());

    waits_for(&mut shell, "Locate…");
    for text in ["Locate…", "Remove from list", "gone"] {
        assert!(shows(&shell, text), "`{text}`: {:?}", names(&shell));
    }

    click_on(&mut shell, "Remove from list");

    assert_eq!(
        shell.state.prefs.recent_projects,
        std::slice::from_ref(&kept)
    );
    assert!(kept.is_file(), "Remove deleted a file");
    waits_for(&mut shell, "kept");
    assert!(!shows(&shell, "Locate…"), "{:?}", names(&shell));
}

#[test]
fn locate_opens_the_project_where_it_is_now_and_forgets_where_it_was() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let moved = a_project(dir.path(), "moved", Template::Empty);
    let old = dir.path().join("old").join("moved.openrage-proj");
    let mut shell = a_first_launch();
    shell.state.prefs.touch_recent_project(old.clone());
    shell.draw(Vec::new());
    waits_for(&mut shell, "Locate…");

    // The dialog is told where the project is now.
    file_dialogs::answer_next(moved.clone());
    click_on(&mut shell, "Locate…");

    assert_eq!(shell.state.project_path.as_deref(), Some(moved.as_path()));
    assert_eq!(shell.state.prefs.recent_projects, [moved]);
}

#[test]
fn closing_a_project_puts_the_start_page_over_the_install() {
    let install = tempfile::tempdir().expect("an install folder");
    let dir = tempfile::tempdir().expect("a scratch folder");
    let police = a_project(dir.path(), "police", Template::Files);
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.documents.clear();
    shell.state.active_document = None;
    let mounted = shell.state.open_game_root(install.path().to_path_buf());
    shell.state.set_active_document(mounted);
    shell.state.load_project_from(police);
    shell.settled();
    assert!(
        !shows(&shell, "Open an install"),
        "the project is on screen, not the start page"
    );

    shell.state.close_project();
    shell.settled();

    assert_eq!(shell.state.project_path, None);
    for text in ["New project…", "Open an install", "Back"] {
        assert!(
            shows(&shell, text),
            "closing a project shows the start page: no `{text}`"
        );
    }
    click_on(&mut shell, "Back");
    assert!(
        !shows(&shell, "Open an install"),
        "Back leaves the start page"
    );
}

#[test]
fn the_project_menu_of_the_status_bar_lists_the_recent_projects_and_the_start_page() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let police = a_project(dir.path(), "police", Template::Files);
    let sheriff = a_project(dir.path(), "sheriff", Template::Empty);
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.prefs.touch_recent_project(sheriff);
    shell.state.prefs.touch_recent_project(police.clone());
    shell.state.load_project_from(police);
    shell.settled();

    click_button_ending(&mut shell, "police");

    for text in [
        "Project workspace",
        "Recent projects",
        "sheriff",
        "Start page",
        "Close project",
    ] {
        assert!(
            shows(&shell, text),
            "the project menu lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    click_on(&mut shell, "Start page");
    assert!(shell.state.front_door.raised(shell.state.active_workspace));
}

#[test]
fn an_opened_folder_offers_to_become_a_project_until_it_is_declined() {
    let mut shell = Shell::new(ThemeChoice::Dark);
    let _folder = shell.open_a_folder();
    shell.settled();
    assert!(shows(&shell, "Make this a project"), "{:?}", names(&shell));
    assert!(shows(&shell, "Not now"));

    click_on(&mut shell, "Not now");

    assert!(
        !shows(&shell, "Make this a project"),
        "the offer was declined"
    );
    assert_eq!(shell.state.project_path, None);
}

#[test]
fn making_an_opened_folder_a_project_moves_nothing_in_it() {
    let mut shell = Shell::new(ThemeChoice::Dark);
    let folder = shell.open_a_folder();
    shell.settled();

    click_on(&mut shell, "Make this a project");
    until_the_project_opens(&mut shell);

    let name = folder
        .path()
        .file_name()
        .expect("a name")
        .to_string_lossy()
        .trim_start_matches('.')
        .to_string();
    let file = folder.path().join(format!("{name}.openrage-proj"));
    assert_eq!(shell.state.project_path.as_deref(), Some(file.as_path()));
    assert_eq!(
        std::fs::read(folder.path().join("notes.txt")).expect("a file"),
        b"hello",
        "a file of the folder changed"
    );
    assert!(folder.path().join("blob.bin").is_file() && folder.path().join("folder").is_dir());
}

/// A resource of the pack `harbor` with files that only have names, beside its scripts.
fn a_resource_with_clothing(folder: &Path) {
    use or_wardrobe::{layout, Component, Ped, Skin, Slot};

    let put = |rel: &str| {
        let path = folder.join(rel);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("a folder");
        std::fs::write(path, b"x").expect("a file");
    };
    let slot = Slot::Component(Component::Jbib);
    let directory = layout::directory("harbor", Ped::Male, "mp_m_harbor", slot);
    put(&format!(
        "{directory}/{}",
        layout::model_name(Ped::Male, "mp_m_harbor", slot, 0, Skin::Universal)
    ));
    put(&format!(
        "{directory}/{}",
        layout::texture_name(Ped::Male, "mp_m_harbor", slot, 0, 0, "uni")
    ));
    put("fxmanifest.lua");
    put("client/main.lua");
}

#[test]
fn the_banner_over_an_opened_folder_says_what_was_found_in_it_before_anything_is_written() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    a_resource_with_clothing(dir.path());
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.open_folder(dir.path().to_path_buf());
    shell.settled();

    waits_for(&mut shell, "Found: Clothing, Files (1)");
    assert!(shows(&shell, "Make this a project"));
    assert!(
        !dir.path().join("wardrobe.json").exists(),
        "reading the folder wrote nothing"
    );

    click_on(&mut shell, "Make this a project");
    until_the_project_opens(&mut shell);
    assert!(
        dir.path().join("wardrobe.json").is_file(),
        "the pack is added beside its files"
    );
    assert!(shell.state.project_path.is_some());
}

#[test]
fn a_template_file_is_read_into_the_gallery_and_its_content_and_builds_are_said() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let made = a_project(dir.path(), "police", Template::Files);
    let mut shell = a_first_launch();
    shell.state.load_project_from(made);
    let file = dir.path().join("police.openrage-template");
    shell
        .state
        .save_project_as_template(&file)
        .expect("the project is saved as a template");
    shell.state.close_project();
    shell.state.hide_start_page();

    shell.state.start_new_project(Template::Empty);
    shell.settled();
    file_dialogs::answer_next(file);
    click_on(&mut shell, "Template file…");

    assert!(shows(&shell, "police"), "{:?}", names(&shell));
    assert!(
        // The dialog named the project after the template, and the resource after the project.
        shows(&shell, "FiveM resource named police"),
        "the builds the template starts the project with: {:?}",
        names(&shell)
    );
    assert!(shows(&shell, "Files (files)"), "{:?}", names(&shell));
}

#[test]
fn linked_content_is_shown_with_the_project_that_writes_it() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let pack = dir
        .path()
        .join("shared")
        .join("brand")
        .join("modules")
        .join("uniforms");
    std::fs::create_dir_all(&pack).expect("a folder");
    or_wardrobe::Wardrobe::new("brand")
        .write(&pack)
        .expect("a pack");
    std::fs::write(
        dir.path()
            .join("shared")
            .join("brand")
            .join("brand.openrage-proj"),
        b"{}",
    )
    .expect("a project file");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("a folder");
    let made = a_project(&work, "police", Template::Files);

    let mut shell = a_first_launch();
    shell.state.load_project_from(made);
    shell.state.link_content(&pack).expect("linked");
    shell.state.active_workspace = Workspace::Project;
    shell.settled();

    waits_for_the_project(&mut shell, "it is edited there");
    assert!(shows(&shell, "brand.openrage-proj"), "{:?}", names(&shell));
    assert!(shows(&shell, "Unlink") && shows(&shell, "Open project"));
}

#[test]
fn adopting_a_pack_lists_what_it_adds_among_what_create_writes_and_what_it_could_not_place() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    a_resource_with_clothing(dir.path());
    let mut shell = a_first_launch();
    let mut draft = NewProject::adopting(dir.path());
    draft.focus = false;
    shell.state.front_door.new_project = Some(draft);
    shell.settled();

    // The tree shows the pack file beside the project file once the folder is read.
    waits_for(&mut shell, "+   wardrobe.json");
    assert!(shows(&shell, "The folder holds"), "{:?}", names(&shell));
    assert!(
        shows(&shell, "Clothing: 1 model and"),
        "{:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "Not carried over (1)"),
        "the old manifest: {:?}",
        names(&shell)
    );
    assert!(
        !dir.path().join("wardrobe.json").exists(),
        "nothing is written before Create"
    );
}
