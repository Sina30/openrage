//! The Project workspace's screens and the questions it asks, as they are drawn: what Escape does to a question,
//! what the Builds window and Clean say, what Problems names on each row, and the menu Open in, read through AccessKit
//! from whole frames of the shell.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::epaint::ClippedShape;
use egui::Shape;

use super::frames_of_the_shell::{key, Shell};
use crate::state::front_door::{NewProject, Template};
use crate::state::workbench::{Fix, NewBuild, Place, Question};
use crate::state::ThemeChoice;
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

const C: &str = "mp_m_freemode_01_mp_m_t";

pub(super) fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

/// A drawable's header over bytes that are no drawable, at the version Legacy ships.
fn a_legacy_drawable() -> Vec<u8> {
    use or_io::container::resource::Rsc7;
    let version = (or_io::gta5::formats::format("ydr")
        .expect("declared")
        .file_type)
        .version_for(or_io::Generation::Gen8)
        .expect("a Legacy version");
    let nibble = |v: u32| (v & 0xF) << 28;
    let header = Rsc7::header_from_entry_flags(nibble(version >> 4), nibble(version));
    [header.header_bytes(), vec![0; 64]].concat()
}

/// A clothing project `tshirts` with three jackets, two of them the same bytes, a script, a Legacy drawable, and a file that is not there.
pub(super) fn a_project_shell(dir: &Path, theme: ThemeChoice) -> Shell {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let file = draft.create().expect("a clothing project").file;
    let folder = file.parent().unwrap().to_path_buf();
    let mut dropped = Vec::new();
    for n in 0..3 {
        let model = if n < 2 {
            or_source::sample::drawables_of("the same jacket")
        } else {
            or_source::sample::drawables_of("another jacket")
        };
        dropped.push(write(
            &dir.join("drop").join(format!("{C}^jbib_{n:03}_u.ydd")),
            &model,
        ));
        dropped.push(write(
            &dir.join("drop")
                .join(format!("{C}^jbib_diff_{n:03}_a_uni.ytd")),
            // The same jacket in the same colour: only a copy of a garment is a duplicate of it.
            if n < 2 {
                or_source::sample::textures_of("the same texture")
            } else {
                or_source::sample::textures_of(&format!("texture {n}"))
            }
            .as_slice(),
        ));
    }
    or_wardrobe::edit::add_clothes(&folder.join("clothing"), &dropped).expect("the garments");
    let mut shell = Shell::new(theme);
    shell.state.load_project_from(file);
    shell.state.add_files_to_project(vec![
        write(&folder.join("scripts/client.lua"), b"print('hello')"),
        write(&folder.join("stream/legacy.ydr"), &a_legacy_drawable()),
        folder.join("missing_hat.ydd"),
    ]);
    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    until(&mut shell, "the garments", |s| {
        s.state.workbench.packs().first().is_some_and(|p| {
            p.read
                .as_ref()
                .is_ok_and(|read| read.wardrobe.garments.len() == 3)
        }) && !s.state.workbench.findings().is_empty()
    });
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

/// Draw frames, polling what the workers left, until `done`.
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

pub(super) fn centre_of(shell: &Shell, name: &str) -> egui::Pos2 {
    let widget = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name))
        .or_else(|| {
            (shell.described.iter()).find(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    widget.bounds.center()
}

/// Right-clicks the widget named `name` and says where.
pub(super) fn right_click_on(shell: &mut Shell, name: &str) -> egui::Pos2 {
    let at = centre_of(shell, name);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw(vec![egui::Event::PointerMoved(at), button(true)]);
    shell.draw(vec![button(false)]);
    shell.settled();
    at
}

/// Clicks the widget whose name starts with `start` that is nearest `near`, which is where a menu opened.
pub(super) fn click_nearest(shell: &mut Shell, start: &str, near: egui::Pos2) {
    let at = (shell.described.iter())
        .filter(|w| w.name.as_deref().is_some_and(|n| n.starts_with(start)))
        .map(|w| w.bounds.center())
        .min_by(|a, b| a.distance(near).total_cmp(&b.distance(near)))
        .unwrap_or_else(|| panic!("nothing starts with `{start}`: {:?}", names(shell)));
    shell.click(at);
    shell.settled();
}

fn count_named(shell: &Shell, name: &str) -> usize {
    names(shell).iter().filter(|n| n.as_str() == name).count()
}

/// Whether some text of `shapes` that ends with `text` is set in `colour`.
fn paints(shapes: &[ClippedShape], text: &str, colour: egui::Color32) -> bool {
    fn walk(shape: &Shape, text: &str, colour: egui::Color32) -> bool {
        match shape {
            Shape::Vec(shapes) => shapes.iter().any(|s| walk(s, text, colour)),
            Shape::Text(shape) => {
                shape.galley.text().ends_with(text)
                    && (shape.galley.job.sections.iter()).any(|s| s.format.color == colour)
            }
            _ => false,
        }
    }
    shapes
        .iter()
        .any(|clipped| walk(&clipped.shape, text, colour))
}

/// What asks a question of a project, with the title the question has.
pub(super) type Ask = Box<dyn Fn(&mut Shell)>;

/// Each question a project asks, by its title: Builds, Clean, a Fix and a Waive of the first finding that has one, and the clothes a drop brought.
pub(super) fn the_questions(shell: &Shell) -> Vec<(&'static str, Ask)> {
    let find = |wanted: fn(&Fix) -> bool| {
        (shell.state.workbench.findings().iter())
            .flat_map(|f| f.fixes.clone())
            .find(wanted)
            .expect("the duplicate jackets have a fix and a waiver")
    };
    let fix = find(|fix| matches!(fix, Fix::Remedy { .. }));
    let waive = find(|fix| matches!(fix, Fix::Waive { .. }));
    vec![
        (
            "Builds",
            Box::new(|shell| {
                commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::AddBuild)
            }),
        ),
        (
            "Clean the builds",
            Box::new(|shell| shell.state.ask_to_clean()),
        ),
        (
            "Fix",
            Box::new(move |shell| shell.state.ask_about_fix(&fix)),
        ),
        (
            "Waive",
            Box::new(move |shell| shell.state.ask_about_fix(&waive)),
        ),
        (
            "Clothes dropped",
            Box::new(|shell| {
                let paths = vec![PathBuf::from("jbib_000_u.ydd")];
                shell.state.workbench.asking = Some(Question::DropClothes { paths });
            }),
        ),
        (
            // The title of the question is what the fix says, so the button is what names it here.
            "Apply the fix",
            Box::new(|shell| {
                let files = or_plugin::Transaction {
                    files: vec![or_plugin::FileWrite {
                        path: "notes/labels.json".into(),
                        bytes: b"{}".to_vec(),
                    }],
                };
                shell.state.workbench.asking = Some(Question::PluginFix {
                    name: "Labeller".into(),
                    code: "plugin.labeller.garment-unlabelled".into(),
                    message: "2 garments have no label, so a list can only show their files".into(),
                    fix: or_plugin::SuggestedFix {
                        label: "Give each a label".into(),
                        files,
                    },
                });
            }),
        ),
    ]
}

#[test]
fn escape_closes_every_question_the_project_asks() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let asks = the_questions(&shell);
    for (title, ask) in asks {
        ask(&mut shell);
        shell.settled();
        assert!(
            shell.state.workbench.asking.is_some() && shows(&shell, title),
            "the question `{title}` was not asked: {:?}",
            names(&shell)
        );

        shell.draw(vec![key(egui::Key::Escape)]);
        shell.settled();

        assert!(
            shell.state.workbench.asking.is_none(),
            "Escape left the question `{title}` open"
        );
    }
}

#[test]
fn add_build_comes_prefilled_and_focused_and_holds_its_error_until_the_first_edit() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    // The project's name is the name of the build it already declares.
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::AddBuild);
    shell.settled();

    let Some(Question::Builds {
        adding: Some(NewBuild { name, .. }),
        ..
    }) = shell.state.workbench.asking.clone()
    else {
        panic!("Add build was not asked");
    };
    assert!(
        !name.is_empty(),
        "the name is filled in with one that is free"
    );
    assert!(
        !shows(&shell, "Name the build"),
        "an error is not held against a field nobody has touched: {:?}",
        names(&shell)
    );
    let (_, focused) = shell.focused().expect("the field has the keyboard");
    assert_eq!(focused, "Name of the new build");
    for combo in ["Kind", "Edition"] {
        assert!(
            shell
                .described
                .iter()
                .any(|w| w.role == Role::ComboBox && w.name.as_deref() == Some(combo)),
            "the {combo} choice has no name a screen reader speaks: {:?}",
            shell.described
        );
    }

    for _ in 0..name.chars().count() {
        shell.draw(vec![key(egui::Key::Backspace)]);
    }
    shell.settled();
    assert!(
        shows(&shell, "Name the build"),
        "once the name is emptied the reason is given: {:?}",
        names(&shell)
    );
}

#[test]
fn clean_is_marked_destructive_and_states_how_many_files_and_how_much_it_removes() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let builds = dir.path().join("tshirts").join("build");
    write(&builds.join("old_name/fxmanifest.lua"), b"x");
    write(&builds.join("old_name/data/a.meta"), b"yy");
    write(&builds.join(".old_name.openrage-build"), b"marker");
    write(
        &builds.join("theirs/fxmanifest.lua"),
        b"not ours, never counted",
    );

    shell.state.ask_to_clean();
    let shapes = shell.settled();

    assert!(
        shows(&shell, "2 file(s)") && shows(&shell, &or_job::human_bytes(3)),
        "the question counts what it would remove: {:?}",
        names(&shell)
    );
    assert!(
        paints(&shapes, "Clean", crate::ui::theme::ERROR.get()),
        "the button that deletes is marked as the danger it is"
    );
}

/// The y of the row a widget is on, as the middle of its bounds.
fn row_of(shell: &Shell, role: Role, name: &str) -> f32 {
    shell
        .described
        .iter()
        .find(|w| w.role == role && w.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no {role:?} named `{name}`: {:?}", names(shell)))
        .bounds
        .center()
        .y
}

#[test]
fn every_row_of_problems_names_its_content_or_its_build() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.build_selected();
    until(&mut shell, "the refusal", |s| {
        s.state.workbench.builds().last("fivem").is_some()
    });
    until(&mut shell, "the rows of Problems", |s| shows(s, " error"));

    // ⚠ A build that stops brings the Build tab forward; the rows read here are those of Problems.
    let problems = crate::ui::panels::registry::ids::PROBLEMS;
    (shell.state.workspace_docks).reveal(crate::ui::dock::Workspace::Project, problems);
    shell.settled();
    let missing = row_of(&shell, Role::Link, "missing_hat.ydd");
    // ⚠ The name the Content panel gives each content, and not the id the project file keeps it under.
    let named = |kind: or_project::ModuleKind| {
        let container = &shell.state.project_carried.container;
        let id = (container.modules.iter())
            .find(|m| m.kind == kind)
            .expect("the content")
            .id
            .clone();
        crate::state::workbench::content_name(container, &id)
    };
    let files = named(or_project::ModuleKind::Files);
    let clothing = named(or_project::ModuleKind::Clothing);
    let on_the_row = |y: f32, name: &str| {
        shell
            .described
            .iter()
            .any(|w| w.name.as_deref() == Some(name) && (w.bounds.center().y - y).abs() < 2.0)
    };
    assert!(
        on_the_row(missing, &files),
        "the row of a missing file says which content it is in: {:?}",
        names(&shell)
    );
    let garment = shell
        .described
        .iter()
        .find(|w| w.role == Role::Link && w.name.as_deref().is_some_and(|n| n.contains("jbib_")))
        .expect("a row about a garment")
        .bounds
        .center()
        .y;
    assert!(
        on_the_row(garment, &clothing),
        "the row of a garment says which content it is in"
    );
    // A build that stopped for twenty files is one row, not twenty.
    let stopped = (shell.state.workbench.findings().iter())
        .filter(|f| matches!(&f.place, Place::Build(_)))
        .count();
    assert_eq!(stopped, 1, "{:?}", shell.state.workbench.findings());
    // A build's row names it, and is an error that stays until a build goes through.
    let build = row_of(&shell, Role::Link, "Build tshirts");
    assert!(shell
        .described
        .iter()
        .any(|w| w.role == Role::Label && (w.bounds.center().y - build).abs() < 2.0));
}

#[test]
fn the_assets_table_has_a_target_column_and_a_loose_drawable_reads_the_edition_its_header_names() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let shell = a_project_shell(dir.path(), ThemeChoice::Dark);

    let header = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Target"))
        .expect("a column for the target")
        .bounds;
    let drawable = row_of(&shell, Role::Label, "legacy.ydr");
    let edition = or_io::Generation::Gen8.edition_label();
    assert!(
        shell.described.iter().any(|w| {
            w.name.as_deref() == Some(edition)
                && (w.bounds.center().y - drawable).abs() < 2.0
                // A cell starts where its column's heading does; how far it runs is its own word's width, in whatever font.
                && (w.bounds.min.x - header.min.x).abs() < 2.0
        }),
        "the drawable's row reads {edition} under Target: {:?}",
        names(&shell)
    );
}

#[test]
fn a_right_click_on_an_asset_or_a_content_offers_open_in_with_every_workspace() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let offered: Vec<&str> = shell
        .state
        .workspaces_offered()
        .iter()
        .map(Workspace::label)
        .collect();
    assert!(
        offered.len() >= 8,
        "a clothing project is offered all: {offered:?}"
    );
    let opens = |shell: &Shell| {
        (names(shell).iter())
            .filter(|n| n.starts_with("Open in"))
            .count()
    };

    let content = names(&shell)
        .into_iter()
        .find(|n| n.starts_with("Files") && n.contains(" assets"))
        .unwrap_or_else(|| panic!("no row for the Files content: {:?}", names(&shell)));
    for (what, target) in [
        ("a content", content),
        ("an asset", "client.lua".to_string()),
    ] {
        let at = right_click_on(&mut shell, &target);
        assert!(
            shows(&shell, "Quick look") || what == "a content",
            "the menu of {what} is open: {:?}",
            names(&shell)
        );
        // Properties has its own Open in once the right click has selected a row; the menu is to have another.
        let menus = opens(&shell);
        let before: Vec<usize> = offered.iter().map(|w| count_named(&shell, w)).collect();
        assert!(
            menus >= if what == "an asset" { 2 } else { 1 },
            "the menu of {what} offers no Open in: {:?}",
            names(&shell)
        );
        click_nearest(&mut shell, "Open in", at);
        for (word, was) in offered.iter().zip(&before) {
            assert!(
                count_named(&shell, word) > *was,
                "the menu of {what} lacks the workspace `{word}`: {:?}",
                names(&shell)
            );
        }
        shell.draw(vec![key(egui::Key::Escape)]);
        shell.draw(vec![key(egui::Key::Escape)]);
        shell.settled();
    }
}

/// The controls with no name to speak in the project workspaces and in each question a project asks, with a project open.
pub(super) fn mute_with_a_project_open(found: &mut std::collections::BTreeMap<String, usize>) {
    use super::every_control_has_a_name_a_screen_reader_can_speak::mute;

    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    for workspace in Workspace::ALL {
        shell.state.active_workspace = workspace;
        shell.settled();
        mute(
            &shell,
            &format!("the {workspace:?} workspace with a project open"),
            found,
        );
    }
    // An asset selected fills Properties, and the quick look is docked under the Assets table.
    shell.state.active_workspace = Workspace::Project;
    let asset = (shell.state.workbench.rows().iter())
        .find(|row| row.name == "client.lua")
        .expect("the script's row")
        .key
        .clone();
    shell.state.select_asset(Some(asset));
    shell.state.workbench.quick_look = true;
    shell.settled();
    mute(
        &shell,
        "the Project workspace with an asset and its quick look",
        found,
    );
    shell.state.workbench.quick_look = false;

    for (title, ask) in the_questions(&shell) {
        ask(&mut shell);
        shell.settled();
        mute(&shell, &format!("the question `{title}`"), found);
        shell.state.workbench.asking = None;
    }
}
