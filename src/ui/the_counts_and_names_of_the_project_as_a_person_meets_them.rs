//! The counts and the names of the Project workspace as a person meets them: one number for each thing a screen counts, said by
//! the kind of what it counts, and one name for a content and for a build wherever the screen says it, read from the text the frames
//! paint and the controls AccessKit describes at the two sizes of window the screens are judged at.
//!
//! ⚠ Drawn through the whole shell, so a count that one panel says and another does not, or an id that stands in for a name, fails here.

use egui::accesskit::Role;
use egui::epaint::ClippedShape;

use super::frames_of_the_shell::Shell;
use super::the_project_screens_on_screen::{a_project_shell, names, shows, until};
use crate::state::workbench::{Place, Scope};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;
use crate::ui::theme;

const SMALL: (u32, u32) = (1280, 720);
const LARGE: (u32, u32) = (1600, 900);

/// Every text the frames painted, with where it is.
fn painted_at(shapes: &[ClippedShape]) -> Vec<(egui::Rect, String)> {
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
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

fn painted(shapes: &[ClippedShape]) -> Vec<String> {
    painted_at(shapes)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

/// Whether `text` says a number of problems and not of errors or warnings: `3 problems`.
fn says_a_bare_number_of_problems(text: &str) -> bool {
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    words
        .windows(2)
        .any(|pair| pair[0].chars().all(|c| c.is_ascii_digit()) && pair[1].starts_with("problem"))
}

fn widget(shell: &Shell, role: Role, name: &str) -> Option<egui::Rect> {
    (shell.described.iter())
        .find(|w| w.role == role && w.name.as_deref() == Some(name))
        .map(|w| w.bounds)
}

#[test]
fn problems_are_counted_once_by_their_kinds_and_every_screen_says_the_count_of_what_it_is_about() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let output = shell.state.active_output_id().expect("a build");
    let (project, content, build) = {
        let wb = &shell.state.workbench;
        let container = &shell.state.project_carried.container;
        let by_content: Vec<(String, String)> = (container.modules.iter())
            .filter_map(|m| {
                let counts = wb.counts(Scope::Content(&m.id));
                counts.worst().map(|_| (m.id.clone(), counts.said()))
            })
            .collect();
        (
            wb.counts(Scope::Project),
            by_content,
            wb.counts(Scope::Build(&output)),
        )
    };
    assert!(
        project.errors >= 1 && project.warnings >= 1 && build.errors >= 1,
        "the project has a missing file and a duplicate: {project:?} {build:?}"
    );

    for size in [SMALL, LARGE] {
        shell
            .state
            .workspace_docks
            .reveal(Workspace::Project, ids::PROBLEMS);
        let said = painted(&shell.settled_sized(size));
        // One word, which is never a number of its own: errors and warnings are said by their names.
        let bare: Vec<&String> = said
            .iter()
            .filter(|t| says_a_bare_number_of_problems(t))
            .collect();
        assert!(
            bare.is_empty(),
            "at {size:?} `problems` is a number: {bare:?}"
        );
        // The project's own count is the one Properties and Problems say, and the Content rows say each content's.
        let total = project.said();
        assert!(
            said.iter().filter(|t| **t == total).count() >= 2,
            "at {size:?} Properties and Problems say `{total}`: {said:?}"
        );
        for (id, counts) in &content {
            assert!(
                said.iter().any(|t| *t == format!("   ● {counts}")),
                "at {size:?} the row of the content {id} says `{counts}`: {said:?}"
            );
        }
        // The build says what stops it, in the status bar and under its row, by the same count.
        let blocked = format!(
            "Blocked by {}",
            crate::state::front_door::counted(build.errors, "error")
        );
        assert!(
            said.iter().any(|t| t.contains(&blocked)),
            "at {size:?} the status bar says `{blocked}`: {said:?}"
        );
        assert!(
            said.iter().any(|t| *t == format!("   {blocked}")),
            "at {size:?} the build's row says `{blocked}`: {said:?}"
        );
    }

    // The Build tab says the same count as what stops the build, and not `no problems` over a panel that lists some.
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::BUILD);
    let said = painted(&shell.settled_sized(SMALL));
    let stops = build.stopping();
    assert!(
        said.iter().any(|t| t.ends_with(&stops)),
        "the Build tab says `{stops}`: {said:?}"
    );
    assert!(said.iter().all(|t| !t.contains("no problems")), "{said:?}");
}

#[test]
fn a_build_is_called_by_the_name_its_resource_has_in_the_status_bar_the_jobs_and_the_history() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let first = shell.state.active_output_id().expect("a build");
    shell
        .state
        .project_carried
        .container
        .add_build("tshirts_dlc", or_project::OutputKind::SingleplayerDlc, None)
        .expect("a second build");
    let first_name = shell
        .state
        .project_carried
        .container
        .output(&first)
        .map(|o| o.name.clone())
        .unwrap();
    assert_ne!(
        first, first_name,
        "the id the entry is keyed by is not its name"
    );
    let said = painted(&shell.settled_sized(SMALL));
    assert!(
        said.iter()
            .any(|t| t.contains(&format!("Build {first_name}"))),
        "the status bar names the build the Build button builds: {said:?}"
    );
    assert!(
        said.iter().all(|t| !t.contains(&format!("Build {first} "))),
        "and never by its id: {said:?}"
    );

    // The job, and the step of the history that says what was started.
    shell.state.build_all();
    let job = shell.state.project_build_job().expect("the run is a job");
    assert_eq!(job.doing, format!("Building {first_name}"));
    assert!(
        (shell.state.workbench.history().iter())
            .any(|e| e.text == format!("Building {first_name}, tshirts_dlc")),
        "{:?}",
        shell.state.workbench.history()
    );
    until(&mut shell, "the builds to end", |s| {
        !s.state.workbench.builds().is_running()
    });
}

#[test]
fn a_content_is_called_by_one_name_and_never_by_the_id_the_project_file_keeps_it_under() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    // The id of the clothing is the project file's, which nobody typed.
    let clothing = (shell.state.project_carried.container.modules.iter())
        .find(|m| m.kind == or_project::ModuleKind::Clothing)
        .map(|m| m.id.clone())
        .expect("a clothing content");
    let renamed = "uniforms";
    for module in &mut shell.state.project_carried.container.modules {
        if module.id == clothing {
            module.id = renamed.to_string();
        }
    }
    for output in &mut shell.state.project_carried.container.outputs {
        for take in &mut output.take {
            if *take == clothing {
                *take = renamed.to_string();
            }
        }
    }
    shell.state.workbench.want_scan();
    until(&mut shell, "the rows under the new id", |s| {
        (s.state.workbench.findings().iter())
            .any(|f| matches!(&f.place, Place::Garment { content, .. } if content == renamed))
    });
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::PROBLEMS);
    for size in [SMALL, LARGE] {
        let said = painted(&shell.settled_sized(size));
        assert!(
            said.iter()
                .all(|t| t != renamed && !t.contains(&format!(" {renamed} "))),
            "at {size:?} the id stands in for a name: {said:?}"
        );
        assert!(
            said.iter().any(|t| t == "Clothing"),
            "at {size:?} the rows of Problems say Clothing, as the Content panel does: {said:?}"
        );
    }
}

#[test]
fn what_is_wrong_with_the_selected_asset_is_said_in_properties_with_the_buttons_that_mend_it() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let key = (shell.state.workbench.rows().iter())
        .find(|row| row.name == "missing_hat.ydd")
        .map(|row| row.key.clone())
        .expect("the missing file is a row");
    shell.state.select_asset(Some(key));
    for size in [SMALL, LARGE] {
        let shapes = shell.settled_sized(size);
        let right = size.0 as f32 * 0.7;
        let said: Vec<String> = painted_at(&shapes)
            .into_iter()
            .filter(|(at, _)| at.min.x > right)
            .map(|(_, text)| text)
            .collect();
        assert!(
            said.iter().any(|t| t.contains("is missing")),
            "at {size:?} Properties says what is wrong with it: {said:?}"
        );
        for button in ["Locate…", "Remove from project"] {
            let at = (shell.described.iter())
                .filter(|w| w.role == Role::Button && w.name.as_deref() == Some(button))
                .map(|w| w.bounds)
                .find(|b| b.min.x > right);
            assert!(
                at.is_some(),
                "at {size:?} Properties has no `{button}`: {:?}",
                names(&shell)
            );
        }
        assert!(
            (shell.described.iter()).any(|w| {
                w.role == Role::Button
                    && w.name.as_deref().is_some_and(|n| n.starts_with("Quick look"))
            }),
            "at {size:?} the quick look has a button, so the key that opens it is not the only way: {:?}",
            names(&shell)
        );
    }
}

#[test]
fn the_quick_look_of_a_garment_is_whole_and_leaves_the_table_its_own_room() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let (key, name) = (shell.state.workbench.rows().iter())
        .find(|row| row.kind == "Garment" && shell.state.workbench.findings_of(row).count() > 0)
        .map(|row| (row.key.clone(), row.name.clone()))
        .expect("a garment with a finding");
    shell.state.select_asset(Some(key));
    shell.state.workbench.quick_look = true;
    for size in [SMALL, LARGE] {
        let shapes = shell.settled_sized(size);
        let texts = painted_at(&shapes);
        let title = texts
            .iter()
            .find(|(_, t)| *t == format!("Quick look: {name}"))
            .unwrap_or_else(|| panic!("no quick look of {name}: {:?}", painted(&shapes)));
        let close = widget(&shell, Role::Button, "Close quick look").expect("a way to close it");
        assert!(
            close.min.y < title.0.max.y + 4.0,
            "the close button is on the title line"
        );
        // ⚠ What is known of the asset is on screen: its kind, and what is wrong with it, under the title.
        assert!(
            texts
                .iter()
                .any(|(at, t)| t.starts_with("Garment") && at.min.y > title.0.max.y)
                && texts
                    .iter()
                    .any(|(at, t)| t.starts_with("Warning:") && at.min.y > title.0.max.y),
            "at {size:?} the facts and the findings of {name} are not under the title: {:?}",
            painted(&shapes)
        );
        // ⚠ No row of the table is drawn through the strip: no two texts of the panel share a place.
        let panel: Vec<&(egui::Rect, String)> = texts
            .iter()
            .filter(|(at, _)| at.min.x > 0.2 * size.0 as f32 && at.min.x < 0.78 * size.0 as f32)
            .filter(|(at, _)| at.min.y > 90.0 && at.max.y < 0.64 * size.1 as f32)
            .collect();
        for (n, (a, text_a)) in panel.iter().enumerate() {
            for (b, text_b) in &panel[n + 1..] {
                let overlap = a.intersect(*b);
                assert!(
                    !(overlap.width() > 2.0 && overlap.height() > 4.0),
                    "at {size:?} `{text_a}` and `{text_b}` are drawn over each other"
                );
            }
        }
    }
}

#[test]
fn a_fix_asks_in_the_words_of_what_it_does_and_its_button_says_it_too() {
    use crate::state::workbench::Fix;
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let fix = (shell.state.workbench.findings().iter())
        .flat_map(|f| f.fixes.clone())
        .find(|fix| matches!(fix, Fix::Remedy { .. }))
        .expect("the duplicate jackets have a fix");
    let Fix::Remedy { module, finding } = &fix else {
        unreachable!()
    };
    let act = (shell.state.workbench.packs().iter())
        .find(|p| p.module == *module)
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| or_wardrobe::edit::remedy(&read.wardrobe, finding))
        .map(|remedy| remedy.act)
        .expect("a remedy");
    shell.state.ask_about_fix(&fix);
    for size in [SMALL, LARGE] {
        let said = painted(&shell.settled_sized(size));
        assert!(
            said.iter().any(|t| *t == format!("{act}?")),
            "at {size:?} the question is titled for what it does: {said:?}"
        );
        assert!(
            widget(&shell, Role::Button, &act).is_some(),
            "at {size:?} the button says `{act}`: {:?}",
            names(&shell)
        );
        assert!(
            widget(&shell, Role::Button, "Fix").is_none() && !shows(&shell, " pin"),
            "at {size:?} the generic verb or an engine word is left: {:?}",
            names(&shell)
        );
    }
}

#[test]
fn the_wardrobes_build_chooser_says_the_name_alone_and_keeps_the_end_that_tells_builds_apart() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    // Three builds of one project share the start of their names, as a build and its DLC and its package do.
    let container = &mut shell.state.project_carried.container;
    let dlc = container
        .add_build(
            "tshirts_police_pack_dlc",
            or_project::OutputKind::SingleplayerDlc,
            None,
        )
        .expect("a DLC build");
    let oiv = container
        .add_build(
            "tshirts_police_pack_oiv",
            or_project::OutputKind::OivPackage,
            None,
        )
        .expect("an OIV build");
    assert_ne!(dlc, oiv);
    shell.state.workbench.selected_output = Some(oiv);
    shell.state.active_workspace = Workspace::Wardrobe;
    let said = painted(&shell.settled_sized(SMALL));
    assert!(
        said.iter().any(|t| t.ends_with("pack_oiv")),
        "the chooser shows the end that tells this build from its siblings: {said:?}"
    );
    assert!(
        said.iter().all(|t| !t.starts_with("Build: ")),
        "the Build button names the verb, so the chooser is the name alone: {said:?}"
    );
}
