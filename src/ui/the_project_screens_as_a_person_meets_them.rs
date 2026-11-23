//! The Project workspace as a person meets it at the two sizes of window it is judged at: the Assets table that tells its rows apart,
//! Properties with its buttons in reach, the Problems toolbar that fits its panel, the empty project that offers its doors, the
//! quick look of a garment, and the screen with no false word over it.
//!
//! ⚠ Drawn through the whole shell and read as the text, the controls and the rectangles the frames paint, so a name cut at the
//! end that tells it apart, a button under a scroll or a toolbar past its panel fails here.

use std::path::Path;

use egui::accesskit::Role;
use egui::epaint::ClippedShape;

use super::frames_of_the_shell::Shell;
use super::the_project_screens_on_screen::{a_project_shell, centre_of, names, shows, until};
use super::the_wardrobe_operations_on_screen::a_shell_with_two_jackets;
use crate::state::front_door::{NewProject, Template};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::theme;

const SMALL: (u32, u32) = (1280, 720);
const LARGE: (u32, u32) = (1600, 900);

fn window(size: (u32, u32)) -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32))
}

/// Every text the frames painted.
fn painted(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The titles of the table's columns: the texts on the line of the header that starts with State.
fn column_titles(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Rect, String)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.rect.translate(t.pos.to_vec2()),
                t.galley.text().to_string(),
            )),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut all));
    let (state, _) = all
        .iter()
        .find(|(_, text)| text == "State")
        .expect("the table has a header");
    let mut line: Vec<&(egui::Rect, String)> = all
        .iter()
        .filter(|(rect, _)| (rect.center().y - state.center().y).abs() < 2.0)
        .collect();
    line.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x));
    line.into_iter().map(|(_, text)| text.clone()).collect()
}

fn widget(shell: &Shell, role: Role, name: &str) -> Option<egui::Rect> {
    (shell.described.iter())
        .find(|w| w.role == role && w.name.as_deref() == Some(name))
        .map(|w| w.bounds)
}

/// A press and release at the middle of the widget called `name`, in a window of `size`.
fn click_at(shell: &mut Shell, size: (u32, u32), name: &str) {
    let at = centre_of(shell, name);
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(size, vec![egui::Event::PointerMoved(at), press(true)]);
    shell.draw_sized(size, vec![press(false)]);
    shell.settled_sized(size);
}

/// A clothing project with three jackets and nothing else, so that every column that says nothing on all of them is plain to see.
fn a_pack_shell(dir: &Path) -> Shell {
    let c = "mp_m_freemode_01_mp_m_unity_clothes";
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("pack".into());
    let file = draft.create().expect("a clothing project").file;
    let folder = file.parent().unwrap().to_path_buf();
    let mut dropped = Vec::new();
    for n in 0..3 {
        let model = or_source::sample::drawables_of(&format!("jacket {n}"));
        let texture = or_source::sample::textures_of(&format!("texture {n}"));
        dropped.push(super::the_project_screens_on_screen::write(
            &dir.join("drop").join(format!("{c}^jbib_{n:03}_u.ydd")),
            &model,
        ));
        dropped.push(super::the_project_screens_on_screen::write(
            &dir.join("drop")
                .join(format!("{c}^jbib_diff_{n:03}_a_uni.ytd")),
            &texture,
        ));
    }
    or_wardrobe::edit::add_clothes(&folder.join("clothing"), &dropped).expect("the garments");
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    until(&mut shell, "the garments", |s| {
        (s.state.workbench.rows().iter())
            .filter(|row| row.kind == "Garment")
            .count()
            == 3
    });
    shell
}

#[test]
fn the_table_of_a_pack_leaves_out_the_columns_that_say_nothing_and_a_long_name_keeps_its_end() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_pack_shell(dir.path());

    let shapes = shell.settled_sized(LARGE);
    assert_eq!(
        column_titles(&shapes),
        ["State", "Name", "Kind"],
        "Target is empty and Content and Stored as say one thing on every row"
    );

    // In a panel that leaves the name little room it is cut in the middle, and the three still read apart by their ends.
    // ⚠ A window of its own: a table remembers the widths its columns were drawn at.
    let narrow_dir = tempfile::tempdir().unwrap();
    let mut narrow = a_pack_shell(narrow_dir.path());
    let shapes = narrow.settled_sized((700, 600));
    let said = painted(&shapes);
    let cut: Vec<&String> = said
        .iter()
        .filter(|t| t.contains('…') && t.starts_with("mp_m") && t.ends_with("_u"))
        .collect();
    assert_eq!(
        cut.len(),
        3,
        "the three garments are cut in the middle: {cut:?}"
    );
    for (n, name) in cut.iter().enumerate() {
        assert!(
            name.ends_with(&format!("{n:03}_u")),
            "the end that tells them apart stays: {name}"
        );
    }
}

#[test]
fn a_content_is_named_by_its_kind_and_a_source_is_stored_as_something_a_person_reads() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Project;
    let shapes = shell.settled_sized(LARGE);
    let said = painted(&shapes);
    let titles = column_titles(&shapes);
    for header in ["Content", "Stored as"] {
        assert!(
            titles.iter().any(|t| t == header),
            "no `{header}` column: {titles:?}"
        );
    }
    assert!(
        said.iter().any(|t| t == "Clothing") && said.iter().any(|t| t == "Files"),
        "the kind of content, as the panel on the left says it: {said:?}"
    );
    for stored in ["Loose file", "In the clothing pack"] {
        assert!(
            said.iter().any(|t| t == stored),
            "`{stored}` is not said: {said:?}"
        );
    }
    assert!(
        !titles.iter().any(|t| t == "Source"),
        "the column is not called Source: {titles:?}"
    );
}

#[test]
fn properties_keep_their_buttons_above_the_facts_in_view_and_offer_only_what_applies() {
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        let folder = or_project::project_folder(&shell.state.project_path.clone().unwrap());
        let one = super::the_project_screens_on_screen::write(
            &folder.join("elsewhere/road.ytd"),
            &or_source::sample::textures_of("road"),
        );
        shell.state.import_game_files(vec![one]);
        until(&mut shell, "the import", |s| {
            s.state
                .workbench
                .rows()
                .iter()
                .any(|row| row.name == "road")
        });
        let key = (shell.state.workbench.rows().iter())
            .find(|row| row.name == "road")
            .unwrap()
            .key
            .clone();
        shell.state.active_workspace = Workspace::Project;
        shell.state.workbench.imports.shown = false;
        shell.state.select_asset(Some(key));
        shell.settled_sized(size);

        let compile =
            widget(&shell, Role::Button, "Compile again").expect("a source offers Compile again");
        let reveal =
            widget(&shell, Role::Button, "Reveal asset in the file manager").expect("Reveal");
        for button in [compile, reveal] {
            assert!(
                window(size).contains_rect(button),
                "{button:?} is out of view at {size:?}"
            );
        }
        let came_from = (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("Came from"))
            .expect("what the source came from comes first")
            .bounds;
        assert!(
            compile.max.y <= came_from.min.y + 1.0,
            "the buttons are above the facts, so no scroll hides them: {compile:?} {came_from:?}"
        );
        for never in ["Replace geometry…", "Check against the game"] {
            assert!(
                widget(&shell, Role::Button, never).is_none(),
                "`{never}` never applies to a textures source made here"
            );
        }
        assert!(
            !shows(&shell, "Stable id"),
            "the ids are under More until it is opened"
        );
        click_at(&mut shell, size, "More");
        assert!(
            shows(&shell, "Stable id") && shows(&shell, "Place"),
            "More opens the ids and the place"
        );

        // The project's own Properties: its two buttons are in view at the real minimum too.
        shell.state.select_asset(None);
        shell.settled_sized(size);
        for button in [
            "Reveal in the file manager",
            "Start this project on this workspace",
        ] {
            let at =
                widget(&shell, Role::Button, button).unwrap_or_else(|| panic!("no `{button}`"));
            assert!(
                window(size).contains_rect(at),
                "`{button}` is cut off at {size:?}: {at:?}"
            );
        }
        assert!(
            shows(&shell, "If you keep this folder in Git"),
            "the Git boxes are folded under a heading"
        );
    }
}

#[test]
fn every_control_of_the_problems_toolbar_lies_inside_its_panel_at_both_sizes_and_a_narrow_one() {
    let _serial = theme::serial();
    for size in [SMALL, LARGE, (700, 600)] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        shell.state.active_workspace = Workspace::Project;
        shell.state.select_asset(None);
        shell.settled_sized(size);
        let edge = widget(&shell, Role::Button, "Reveal in the file manager")
            .expect("the project's Properties start the panel to the right")
            .min
            .x;
        for name in [
            "Errors",
            "Warnings",
            "Filter problems",
            "Group the problems",
            "Order the problems",
            "Export…",
            "Rules…",
        ] {
            let at = (shell.described.iter())
                .find(|w| w.name.as_deref() == Some(name) && w.bounds.min.y > size.1 as f32 * 0.45)
                .unwrap_or_else(|| panic!("no `{name}` at {size:?}: {:?}", names(&shell)))
                .bounds;
            assert!(
                at.max.x <= edge + 1.0,
                "`{name}` runs past the Problems panel at {size:?}: {at:?} against {edge}"
            );
        }
    }
}

#[test]
fn a_missing_model_says_why_it_cannot_be_taken_off_its_garment_and_every_button_has_a_sentence() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, folder) = a_shell_with_two_jackets(dir.path());
    let model = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments[0].model.clone())
        .unwrap();
    std::fs::remove_dir_all(folder.join(&model)).unwrap();
    shell.state.workbench.want_scan();
    shell.state.active_workspace = Workspace::Project;
    until(&mut shell, "the missing model", |s| {
        (s.state.workbench.findings().iter()).any(|f| f.code == Some("model-missing"))
    });
    shell.settled_sized(LARGE);

    assert!(
        shows(&shell, "Locate…"),
        "the model can be pointed at another file"
    );
    let take_off = (shell.described.iter())
        .find(|w| w.role == Role::Button && w.name.as_deref() == Some("Remove the model"))
        .expect("the button that cannot be pressed is drawn, so it can say why");
    assert!(take_off.disabled);
    assert!(
        !shows(&shell, "Remove from project"),
        "a garment's file is not a file of the project to remove"
    );

    use crate::state::workbench::{Fix, Missing};
    let path = std::path::PathBuf::from("a/b");
    let garment = |what| Fix::DropFile {
        module: "clothing".into(),
        garment: "g".into(),
        what,
    };
    let fixes = [
        Fix::Locate(path.clone()),
        Fix::Remove(path.clone()),
        Fix::Reid(path),
        garment(Missing::Cloth),
        Fix::WaivePlugin { code: "x".into() },
    ];
    for fix in &fixes {
        assert!(fix.hover().len() > 20, "`{}` has no sentence", fix.label());
    }
    let mut labels: Vec<&str> = fixes.iter().map(Fix::label).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(
        labels.len(),
        fixes.len(),
        "two things share a name: {labels:?}"
    );
}

#[test]
fn an_empty_project_offers_its_doors_by_one_name_and_says_what_each_takes() {
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut draft = NewProject::new(Template::Empty, dir.path().to_path_buf());
        draft.set_name("bare".into());
        let file = draft.create().expect("a project").file;
        let mut shell = Shell::new(ThemeChoice::Dark);
        shell.state.load_project_from(file);
        shell.state.active_workspace = Workspace::Project;
        until(&mut shell, "the landing", |s| {
            s.state.workbench.is_scanned() && shows(s, "holds no files yet")
        });
        shell.settled_sized(size);

        for button in ["Add files…", "Import game files…", "Add a clothing pack…"] {
            assert!(
                widget(&shell, Role::Button, button).is_some(),
                "no `{button}`: {:?}",
                names(&shell)
            );
        }
        assert!(
            widget(&shell, Role::Button, "Content: Add files…").is_some(),
            "the + beside Content is the same command by the same name"
        );
        let said = painted(&shell.settled_sized(size));
        // ⚠ Two buttons that both bring files in are told apart by a line under each, which says what it takes.
        for line in [
            "Your own files: images, models, scripts, folders.",
            "turned into sources you can edit. The originals are not changed.",
            "An empty clothing pack, with the Wardrobe to fill it.",
        ] {
            assert!(
                said.iter().any(|t| t.contains(line)),
                "no line says `{line}`: {said:?}"
            );
        }
        assert!(
            said.iter().any(|t| t == "No content yet") && said.iter().any(|t| t == "No build yet"),
            "{said:?}"
        );
        assert!(
            said.iter()
                .all(|t| !t.contains("Project ▸") && !t.contains("declares")),
            "no menu path quoted in a sentence, and no developer's word: {said:?}"
        );
    }
}

#[test]
fn the_project_workspace_says_no_read_only_over_it_and_the_outline_is_a_tab_beside_content() {
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        shell.state.active_workspace = Workspace::Project;
        let said = painted(&shell.settled_sized(size));
        for false_word in ["Read only", "Project: editable"] {
            assert!(
                !said.iter().any(|t| t.contains(false_word)),
                "`{false_word}` is over a screen that edits the project: {said:?}"
            );
        }
        let project = widget(&shell, Role::Button, "Project");
        let outline = (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("Outline"))
            .map(|w| w.bounds);
        // ⚠ The panel's tab, which is the widget of that name level with Outline: the menu, the room of the rail and the screen's tab in the tab row are named Project too.
        let project_tab = outline
            .and_then(|outline| {
                let off = |bounds: &egui::Rect| (bounds.min.y - outline.min.y).abs();
                (shell.described.iter())
                    .filter(|w| w.name.as_deref() == Some("Project"))
                    .map(|w| w.bounds)
                    .min_by(|a, b| off(a).total_cmp(&off(b)))
            })
            .or(project);
        let (project_tab, outline) = (
            project_tab.expect("the Project tab"),
            outline.expect("the Outline tab"),
        );
        assert!(
            (project_tab.min.y - outline.min.y).abs() < 3.0,
            "the Outline is a tab in the strip of Content and Builds: {project_tab:?} {outline:?}"
        );
    }
}

#[test]
fn the_quick_look_of_a_garment_draws_its_first_texture() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_pack_shell(dir.path());
    let key = (shell.state.workbench.rows().iter())
        .find(|row| row.kind == "Garment")
        .unwrap()
        .key
        .clone();
    shell.state.select_asset(Some(key));
    shell.state.workbench.quick_look = true;
    until(&mut shell, "the picture", |s| {
        (s.described.iter()).any(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Picture of"))
        })
    });
    assert!(
        !shows(&shell, "Nothing to draw") && !shows(&shell, "No picture for"),
        "a garment is looked at, not sent to Properties: {:?}",
        names(&shell)
    );
}

#[test]
fn the_question_before_going_back_over_an_import_counts_what_it_deletes_in_plain_words() {
    use crate::state::workbench::Question;
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        let sources: Vec<String> = [
            "jbib_001_u.drawables",
            "jbib_diff_001_a_uni.textures",
            "feet_000_u.drawables",
        ]
        .iter()
        .map(|stem| format!("mp_m_freemode_01_mp_m_unity_clothes^{stem}.src"))
        .chain(["readme.txt".to_string()])
        .collect();
        shell.state.workbench.asking = Some(Question::RollBack {
            step: 1,
            label: "Imported 4 files: 3 became sources, 1 kept as it is".into(),
            sources,
        });
        let said = painted(&shell.settled_sized(size));
        assert!(
            said.iter()
                .any(|t| t == "It removes 3 source folders and 1 file from your project folder:"),
            "what it deletes is counted, a file apart from the folders: {said:?}"
        );
        assert!(
            widget(&shell, Role::Button, "Remove them and go back").is_some(),
            "the button that deletes says so: {:?}",
            names(&shell)
        );
        assert!(widget(&shell, Role::Button, "Go back").is_none());
        assert!(
            said.iter()
                .all(|t| !t.contains("(s)") && !t.contains("not the sources")),
            "{said:?}"
        );
        // Each name is on one line, whole or cut in the middle, and never broken inside.
        for name in said
            .iter()
            .filter(|t| t.contains("mp_m_freemode") && t.ends_with(".src") && !t.contains(' '))
        {
            assert!(!name.contains('\n'), "a name broke inside: {name}");
        }
        shell.draw_sized(
            size,
            vec![super::frames_of_the_shell::key(egui::Key::Escape)],
        );
        shell.settled_sized(size);
        assert!(shell.state.workbench.asking.is_none(), "Escape closes it");
    }
}
