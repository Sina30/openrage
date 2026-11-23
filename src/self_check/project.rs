//! The units that open a project and draw the Project workspace over it: each of its panels, its layout and its questions.
//!
//! ⚠ The project is made in a scratch folder under the system's temporary one and removed after: a script, a file
//! that is missing, a texture source, and a build written, so every column of the table and every tab has something to say.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{draw_unit, Outcome, Painted, Report};
use crate::state::workbench::Question;
use crate::state::AppState;
use crate::ui::dock::{Tab, Workspace};
use crate::ui::panels::registry::ids;

/// The panels of the Project workspace, each drawn alone over the open project.
const PANELS: [crate::ui::panel_registry::PanelId; 7] = [
    ids::PROJECT,
    ids::ASSETS,
    ids::DETAILS,
    ids::PROBLEMS,
    ids::BUILD,
    ids::HISTORY,
    ids::JOBS,
];

pub(super) fn project_units(
    state: &mut AppState,
    frames: usize,
    blank: &Painted,
    report: &mut Report,
) {
    let folder = std::env::temp_dir().join(format!(
        "openrage-self-check-project-{}",
        std::process::id()
    ));
    let opened = open(state, &folder);
    match opened {
        Err(why) => report.record("project: a scratch project opens", Err(why)),
        Ok(()) => {
            for id in PANELS {
                let outcome = draw_unit(state, None, frames, move |state| {
                    state.active_workspace = Workspace::Project;
                    *state.workspace_docks.get_mut(Workspace::Project) =
                        egui_dock::DockState::new(vec![Tab(id)]);
                })
                .and_then(|painted| beats(painted, blank));
                report.record(format!("panel `{id}` with a project open"), outcome);
            }
            let outcome = draw_unit(state, None, frames, |state| {
                state.active_workspace = Workspace::Project;
                state.workspace_docks.reset(Workspace::Project);
            });
            report.record("workspace `Project` with a project open", outcome);

            report.record(
                "project: the quick look on a source",
                quick_look(state, frames, blank),
            );
            report.record(
                "project: a linked pack with its banner",
                linked(state, frames, &folder),
            );
            report.record(
                "status bar: the project menu open",
                project_menu(state, frames),
            );
            report.record(
                "project: the Builds window with a build being added",
                asked(state, frames, |state| {
                    state.ask_about_builds(true);
                    if let Some(Question::Builds {
                        adding: Some(new), ..
                    }) = &mut state.workbench.asking
                    {
                        new.name = "police_dlc".into();
                        new.kind = or_project::OutputKind::SingleplayerDlc;
                        new.target = Some(or_project::Target::Enhanced);
                    }
                }),
            );
            report.record(
                "project: the report of an import",
                import_report(state, frames, &folder),
            );
            report.record(
                "project: the Import built pack question",
                asked(state, frames, |state| {
                    state.workbench.asking = Some(Question::Reopen {
                        built: "dlc.rpf".into(),
                        place: "projects".into(),
                        name: "dlc".into(),
                        pack: String::new(),
                        error: Some("projects/dlc already holds something".into()),
                        focus: false,
                    })
                }),
            );
            report.record(
                "project: the Replace geometry question",
                asked(state, frames, |state| {
                    state.workbench.asking = Some(Question::ReplaceGeometry {
                        key: "pair.drawables.src".into(),
                        file: "lifted.glb".into(),
                        entries: vec!["alpha".into(), "beta".into()],
                        chosen: 1,
                    })
                }),
            );
            report.record(
                "project: the report of a geometry replacement",
                asked(state, frames, |state| {
                    state.workbench.asking = Some(Question::Geometry {
                        name: "lamp".into(),
                        from: "lamp.glb".into(),
                        replaced: true,
                        lines: vec![
                            "1 geometries from the file".into(),
                            "not carried: 1 materials".into(),
                        ],
                    })
                }),
            );
            report.record(
                "project: the Clean question",
                asked(state, frames, |state| {
                    state.workbench.asking = Some(Question::Clean {
                        resources: vec!["police_pack".into()],
                        files: 12,
                        bytes: 3 << 20,
                    })
                }),
            );
        }
    }
    // ⚠ A scratch project the units changed asks to be saved before it closes, which no unit answers: it is the scratch folder's, and goes with it.
    state.project_carried.changed = false;
    state.close_project();
    // ⚠ Closing a project raises the start page over the workspace on screen, which the units after this draw.
    state.hide_start_page();
    state.workspace_docks.reset(Workspace::Project);
    let _ = std::fs::remove_dir_all(&folder);
}

fn beats(painted: Painted, blank: &Painted) -> Outcome {
    if painted.vertices > blank.vertices {
        Ok(painted)
    } else {
        Err(format!(
            "painted {} vertices, no more than the {} an empty panel in the same leaf costs",
            painted.vertices, blank.vertices
        ))
    }
}

/// Make the scratch project, open it, wait for its scan, and give it a recook and a build to show.
fn open(state: &mut AppState, folder: &Path) -> Result<(), String> {
    let write = |path: PathBuf, body: &[u8]| -> Result<(), String> {
        std::fs::create_dir_all(path.parent().ok_or("a file with no folder")?)
            .and_then(|()| std::fs::write(&path, body))
            .map_err(|e| format!("{}: {e}", path.display()))
    };
    let _ = std::fs::remove_dir_all(folder);
    write(folder.join("client.lua"), b"print('police client')")?;
    write(
        folder.join("data/vehicles.meta"),
        b"<CVehicleModelInfo__InitDataList/>",
    )?;
    let source = folder.join("logo.textures.src");
    write(
        source.join("asset.json"),
        br#"{ "created_with": "a self-check", "format": "openrage-source", "id": "7c1e2b0a-5d1f-4e7a-9b3c-2a6f0d8e4c11",
  "kind": "textures", "name": "logo", "notes": "", "origin": { "from_game": false, "how": "authored" },
  "platform": "win64", "schema": 1, "target": "gen8" }"#,
    )?;
    write(source.join("textures.json"), b"{ \"textures\": [] }")?;
    std::fs::create_dir_all(source.join("textures")).map_err(|e| e.to_string())?;
    image::RgbaImage::from_pixel(8, 8, image::Rgba([40, 0, 0, 255]))
        .save(source.join("textures/logo.png"))
        .map_err(|e| e.to_string())?;

    let mut container = or_project::Container::default();
    container.outputs.push(or_project::Output::fivem_resource(
        "fivem",
        "police_pack",
        &["files"],
    ));
    let file = folder.join("police_pack.openrage-proj");
    let members = [
        folder.join("client.lua"),
        folder.join("data/vehicles.meta"),
        folder.join("missing_hat.ydd"),
    ];
    or_project::Project::create(&file, &container, &members).map_err(|e| e.to_string())?;
    state.load_project_from(file);
    wait(state, "the project's scan", |state| {
        state.workbench.is_scanned()
    })?;

    // A recook, so the source's row says what a cook says, and a build that the missing file refuses.
    let then = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    state
        .workbench
        .file_saved(source.join("textures/logo.png"), then);
    wait(state, "the recook", |state| {
        state
            .workbench
            .history()
            .iter()
            .any(|e| e.text.starts_with("Compiled"))
    })?;
    state.build_selected();
    wait(state, "the build", |state| {
        !state.workbench.builds().is_running()
    })?;
    state.select_asset(Some(source.display().to_string()));
    Ok(())
}

fn wait(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_workbench();
        if done(state) {
            return Ok(());
        }
        if Instant::now() > until {
            return Err(format!("{what} did not finish in thirty seconds"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The Assets panel with the quick look open on the selected source, judged against the same panel with it shut.
fn quick_look(state: &mut AppState, frames: usize, blank: &Painted) -> Outcome {
    let draw = |state: &mut AppState, open: bool| {
        draw_unit(state, None, frames, move |state| {
            state.active_workspace = Workspace::Project;
            *state.workspace_docks.get_mut(Workspace::Project) =
                egui_dock::DockState::new(vec![Tab::ASSETS]);
            state.workbench.quick_look = open;
        })
    };
    let shut = draw(state, false)?;
    let open = draw(state, true);
    state.workbench.quick_look = false;
    let open = open?;
    if open.vertices > shut.vertices {
        beats(open, blank)
    } else {
        Err(format!(
            "painted {} vertices with the quick look open and {} without — it drew nothing",
            open.vertices, shut.vertices
        ))
    }
}

/// The shell with the status bar's project menu open, judged against the shell with it shut.
fn project_menu(state: &mut AppState, frames: usize) -> Outcome {
    use crate::ui::panels::status_bar::ProjectMenu;

    let shut = draw_unit(state, None, frames, |_| {})?;
    let open = draw_unit(state, None, frames, |state| {
        state.panels.get::<ProjectMenu>().open = true;
    });
    state.panels.get::<ProjectMenu>().open = false;
    let open = open?;
    if open.vertices > shut.vertices {
        Ok(open)
    } else {
        Err(format!(
            "painted {} vertices open and {} shut — the project menu drew nothing",
            open.vertices, shut.vertices
        ))
    }
}

/// The report an import leaves, drawn over the Project workspace, judged against the same workspace with it shut.
fn import_report(state: &mut AppState, frames: usize, folder: &Path) -> Outcome {
    // ⚠ A script the project's own folder holds: a compiled file with no source form is kept, which every import can say.
    state.import_game_files(vec![folder.join("data/vehicles.meta")]);
    wait(state, "the import", |state| {
        !state.workbench.imports.is_running() && state.workbench.imports.report().is_some()
    })?;
    let shown = |state: &mut AppState, shown: bool| {
        draw_unit(state, None, frames, move |state| {
            state.active_workspace = Workspace::Project;
            state.workspace_docks.reset(Workspace::Project);
            state.workbench.imports.shown = shown;
        })
    };
    let without = shown(state, false)?;
    let with = shown(state, true);
    state.workbench.imports.shown = false;
    let with = with?;
    if with.vertices > without.vertices {
        Ok(with)
    } else {
        Err(format!(
            "painted {} vertices with the report open and {} without — it drew nothing",
            with.vertices, without.vertices
        ))
    }
}

/// The Project workspace with a question asked, judged against the same workspace with none.
fn asked(state: &mut AppState, frames: usize, ask: fn(&mut AppState)) -> Outcome {
    let without = draw_unit(state, None, frames, |state| {
        state.active_workspace = Workspace::Project;
        state.workspace_docks.reset(Workspace::Project);
    })?;
    let with = draw_unit(state, None, frames, ask);
    state.workbench.asking = None;
    let with = with?;
    if with.vertices > without.vertices {
        Ok(with)
    } else {
        Err(format!(
            "painted {} vertices with the question asked and {} without — it drew nothing",
            with.vertices, without.vertices
        ))
    }
}

/// The Content panel with a pack another project holds linked, which draws a banner, judged against the same panel without it.
fn linked(state: &mut AppState, frames: usize, folder: &Path) -> Outcome {
    let panel = |state: &mut AppState| {
        draw_unit(state, None, frames, |state| {
            state.active_workspace = Workspace::Project;
            *state.workspace_docks.get_mut(Workspace::Project) =
                egui_dock::DockState::new(vec![Tab::PROJECT]);
        })
    };
    let without = panel(state)?;

    let beside = folder.with_file_name(format!(
        "{}-brand",
        folder
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    ));
    let pack = beside.join("modules").join("uniforms");
    let made = std::fs::create_dir_all(&pack)
        .and_then(|()| std::fs::write(beside.join("brand.openrage-proj"), b"{}"))
        .map_err(|e| e.to_string())
        .and_then(|()| {
            or_wardrobe::Wardrobe::new("brand")
                .write(&pack)
                .map_err(|e| e.to_string())
        });
    let outcome = made
        .and_then(|()| state.link_content(&pack).map(drop))
        .and_then(|()| {
            wait(state, "the scan to show the link", |s| {
                s.workbench.link("uniforms").is_some()
            })
        })
        .and_then(|()| panel(state));
    if state.project_carried.container.module("uniforms").is_some() {
        let _ = state.unlink_content("uniforms");
    }
    let _ = std::fs::remove_dir_all(&beside);
    let drawn = outcome?;
    if drawn.vertices > without.vertices {
        Ok(drawn)
    } else {
        Err(format!(
            "painted {} vertices with a linked pack and {} without — the banner drew nothing",
            drawn.vertices, without.vertices
        ))
    }
}
