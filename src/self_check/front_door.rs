//! The units that draw the front door: the start page with its cards, the New project dialog, the question a second project asks,
//! the banner over an opened folder and the status bar's project menu.
//!
//! ⚠ Each is judged against the same screen without it, so a surface that drew nothing ties and fails.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{draw_unit, Outcome, Painted, Report};
use crate::state::front_door::{Loaded, NewProject, Step, Template};
use crate::state::AppState;
use crate::ui::dock::{Tab, Workspace};

/// ⚠ `config_root` is the run's own scratch folder, which the run removes however it ends, so the projects made here are never left in the temporary folder.
pub(super) fn front_door_units(
    state: &mut AppState,
    frames: usize,
    blank: &Painted,
    report: &mut Report,
    config_root: &Path,
) {
    let folder = config_root.join("front-door");
    let _ = std::fs::remove_dir_all(&folder);
    match scratch(&folder) {
        Err(why) => report.record("front door: scratch projects are made", Err(why)),
        Ok((kept, gone)) => {
            report.record(
                "start page: the recent projects as cards, one not found",
                cards(state, frames, blank, &kept, &gone),
            );
            report.record(
                "start page: raised over the open shell",
                raised(state, frames, blank),
            );
            report.record(
                "New project: the gallery",
                dialog(state, frames, &folder, Step::Gallery, Template::Empty),
            );
            report.record(
                "New project: the details of Files",
                dialog(state, frames, &folder, Step::Details, Template::Files),
            );
            report.record(
                "New project: the details of a project from a folder",
                dialog(state, frames, &folder, Step::Details, Template::Adopt),
            );
            report.record(
                "New project: the gallery with a template file chosen",
                shared_template(state, frames, &folder),
            );
            report.record(
                "project: the question a second project asks",
                switching(state, frames, &kept),
            );
            report.record(
                "folder: the banner that offers to make it a project",
                banner(state, frames, &folder),
            );
            report.record(
                "status bar: the project's menu over several recent projects",
                project_menu(state, frames, &kept, &gone),
            );
            report.record(
                "project: the question Close project asks of a changed project",
                closing(state, frames, &kept),
            );
        }
    }
    let _ = std::fs::remove_dir_all(&folder);
}

/// A project that is there and one that is not, and a folder to adopt beside them.
fn scratch(folder: &Path) -> Result<(PathBuf, PathBuf), String> {
    let mut draft = NewProject::new(Template::Files, folder.to_path_buf());
    draft.set_name("police_pack".into());
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let kept = draft.create()?.file;
    std::fs::create_dir_all(folder.join("adoptable")).map_err(|e| e.to_string())?;
    std::fs::write(folder.join("adoptable/client.lua"), b"print('hi')")
        .map_err(|e| e.to_string())?;
    Ok((kept, folder.join("moved").join("moved.openrage-proj")))
}

/// More than `without`, else a sentence that says the surface drew nothing.
fn beats(drawn: Outcome, without: &Painted, what: &str) -> Outcome {
    let drawn = drawn?;
    if drawn.vertices > without.vertices {
        Ok(drawn)
    } else {
        Err(format!(
            "painted {} vertices with {what} and {} without — it drew nothing",
            drawn.vertices, without.vertices
        ))
    }
}

/// The start page as the Document panel draws it with nothing open, with two recent projects.
fn cards(
    state: &mut AppState,
    frames: usize,
    blank: &Painted,
    kept: &Path,
    gone: &Path,
) -> Outcome {
    let open = std::mem::take(&mut state.documents);
    let recents = std::mem::replace(
        &mut state.prefs.recent_projects,
        vec![kept.to_path_buf(), gone.to_path_buf()],
    );
    // The cards are read on a worker; the unit waits for it as the page's next frames would.
    let until = Instant::now() + Duration::from_secs(30);
    let paths = state.prefs.recent_projects.clone();
    while state
        .front_door
        .cards
        .of(&paths)
        .iter()
        .any(Option::is_none)
    {
        if Instant::now() > until {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let drawn = draw_unit(state, None, frames, |state| {
        state.active_workspace = Workspace::Particle;
        *state.workspace_docks.get_mut(Workspace::Particle) =
            egui_dock::DockState::new(vec![Tab::DOCUMENT]);
    });
    state.prefs.recent_projects = recents;
    state.documents = open;
    beats(drawn, blank, "two recent project cards")
}

/// The shell with the start page raised over it, which takes the centre in place of the workspace's panels: judged against a docked panel that paints nothing, and against the shell without it, which it must differ from.
fn raised(state: &mut AppState, frames: usize, blank: &Painted) -> Outcome {
    // ⚠ Over Model and not Browse, which draws the page by itself while nothing is open, as in a run that finds no install.
    let without = draw_unit(state, None, frames, |state| {
        state.active_workspace = Workspace::Model;
        state.hide_start_page();
    })?;
    let drawn = draw_unit(state, None, frames, |state| {
        state.active_workspace = Workspace::Model;
        state.show_start_page();
    });
    state.hide_start_page();
    let drawn = beats(drawn, blank, "the start page raised")?;
    if drawn.vertices == without.vertices {
        return Err("the shell painted the same with the start page raised and without".into());
    }
    Ok(drawn)
}

/// The shell with the New project dialog open on `step` and `template`, judged against the shell without it.
fn dialog(
    state: &mut AppState,
    frames: usize,
    folder: &Path,
    step: Step,
    template: Template,
) -> Outcome {
    let without = draw_unit(state, None, frames, |_| {})?;
    let place = folder.to_path_buf();
    let drawn = draw_unit(state, None, frames, move |state| {
        let mut draft = if template == Template::Adopt {
            NewProject::adopting(&place.join("adoptable"))
        } else {
            let mut draft = NewProject::new(template, place);
            draft.set_name("mod_pack".into());
            draft
        };
        draft.step = step;
        state.front_door.new_project = Some(draft);
    });
    state.front_door.new_project = None;
    beats(drawn, &without, "the dialog")
}

/// The shell with a second project waiting on its answer, judged against the shell without it.
fn switching(state: &mut AppState, frames: usize, project: &Path) -> Outcome {
    let without = draw_unit(state, None, frames, |_| {})?;
    let project = project.to_path_buf();
    let drawn = draw_unit(state, None, frames, move |state| {
        state.front_door.switching = Some(project);
    });
    state.front_door.switching = None;
    beats(drawn, &without, "the question")
}

/// Browse with a folder open and the offer to make it a project, judged against the same folder with the offer declined.
fn banner(state: &mut AppState, frames: usize, folder: &Path) -> Outcome {
    let adoptable = folder.join("adoptable");
    state.open_folder(adoptable.clone());
    let Some(doc) = state.active_document else {
        return Err("the folder did not open".into());
    };
    let drawn = draw_unit(state, None, frames, |state| {
        state.active_workspace = Workspace::Browse;
        state.workspace_docks.reset(Workspace::Browse);
    });
    state.decline_adoption(&adoptable);
    let without = draw_unit(state, None, frames, |_| {});
    super::discard(state, doc);
    let without = without?;
    beats(drawn, &without, "the offer")
}

/// The shell with the New project dialog open on a template file it has read, judged against the shell without it.
fn shared_template(state: &mut AppState, frames: usize, folder: &Path) -> Outcome {
    use or_project::template::Template as File;
    use or_project::{Container, Module, ModuleKind, Output};

    let without = draw_unit(state, None, frames, |_| {})?;
    let mut container = Container {
        modules: vec![
            Module::files("files"),
            Module::clothing("uniforms", "modules/uniforms"),
            Module::linked("logos", ModuleKind::Clothing, "../brand/logos"),
        ],
        outputs: vec![Output::fivem_resource(
            "fivem",
            "police_pack",
            &["files", "uniforms"],
        )],
    };
    container.outputs[0].split = true;
    let (template, findings) = File::from_project(
        "Police pack",
        "Uniforms and a resource for them",
        &container,
        Some("Project"),
        None,
        Some("both"),
    );
    let place = folder.to_path_buf();
    let drawn = draw_unit(state, None, frames, move |state| {
        let mut draft = NewProject::new(Template::Files, place);
        draft.set_name("harbor_police".into());
        draft.offer(Loaded {
            template,
            from: None,
            findings,
        });
        state.front_door.new_project = Some(draft);
    });
    state.front_door.new_project = None;
    beats(drawn, &without, "the dialog on a template file")
}

/// The status bar with the menu of the project's name open over several recent projects, judged against the same bar with it shut.
fn project_menu(state: &mut AppState, frames: usize, kept: &Path, gone: &Path) -> Outcome {
    use crate::ui::panels::status_bar::ProjectMenu;

    let recents = std::mem::replace(
        &mut state.prefs.recent_projects,
        vec![
            kept.to_path_buf(),
            gone.to_path_buf(),
            kept.with_file_name("sheriff.openrage-proj"),
            kept.with_file_name("harbor.openrage-proj"),
        ],
    );
    state.project_path = Some(kept.to_path_buf());
    state.panels.get::<ProjectMenu>().open = false;
    let without = draw_unit(state, None, frames, |_| {});
    let drawn = draw_unit(state, None, frames, |state| {
        state.panels.get::<ProjectMenu>().open = true;
    });
    state.panels.get::<ProjectMenu>().open = false;
    state.project_path = None;
    state.prefs.recent_projects = recents;
    beats(drawn, &without?, "the menu")
}

/// The shell with the question Close project asks of a project that changed, judged against the shell without it.
fn closing(state: &mut AppState, frames: usize, project: &Path) -> Outcome {
    state.project_path = Some(project.to_path_buf());
    state.project_carried.changed = true;
    let without = draw_unit(state, None, frames, |_| {});
    let drawn = draw_unit(state, None, frames, |state| state.close_project());
    state.answer_close(crate::state::CloseAnswer::Cancel);
    state.project_carried.changed = false;
    state.project_path = None;
    beats(drawn, &without?, "the question")
}
