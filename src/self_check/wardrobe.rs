//! The units that open a clothing project and draw the Wardrobe workspace over it: its panels, its layout, a drop and the questions Fix and Waive ask.
//!
//! ⚠ The project is made in a scratch folder under the system's temporary one and removed after. It is
//! made by the Clothing pack template and filled by the same drop the window routes to Add clothes, with
//! two garments of one model so that the engine has a duplicate to find, and one whose name says nothing.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{draw_unit, Outcome, Painted, Report};
use crate::state::front_door::{NewProject, Template};
use crate::state::os_drop::DropTarget;
use crate::state::workbench::{Fix, Question, ViewMode};
use crate::state::AppState;
use crate::ui::dock::{Tab, Workspace};
use crate::ui::panels::registry::ids;

const MODEL: &str = "mp_m_freemode_01_mp_m_check^jbib_00";

pub(super) fn wardrobe_units(
    state: &mut AppState,
    frames: usize,
    blank: &Painted,
    bridge: Option<&mut crate::render::RenderBridge>,
    report: &mut Report,
) {
    let folder = std::env::temp_dir().join(format!(
        "openrage-self-check-wardrobe-{}",
        std::process::id()
    ));
    match open(state, &folder) {
        Err(why) => report.record("wardrobe: a scratch clothing project opens", Err(why)),
        Ok(()) => {
            for id in [ids::WARDROBE, ids::DETAILS, ids::DRESSED] {
                let outcome = draw_unit(state, None, frames, move |state| {
                    state.active_workspace = Workspace::Wardrobe;
                    *state.workspace_docks.get_mut(Workspace::Wardrobe) =
                        egui_dock::DockState::new(vec![Tab(id)]);
                })
                .and_then(|painted| beats(painted, blank));
                report.record(format!("panel `{id}` with clothing open"), outcome);
            }
            let outcome = draw_unit(state, None, frames, |state| {
                state.active_workspace = Workspace::Wardrobe;
                state.workspace_docks.reset(Workspace::Wardrobe);
            });
            report.record("workspace `Wardrobe` with clothing open", outcome);

            match bridge {
                Some(bridge) => {
                    report.record(
                        "wardrobe: the dressed ped draws through the viewport",
                        dressed_ped(state, bridge, frames, blank),
                    );
                    views(state, bridge, frames, blank, report);
                }
                None => report.note(
                    "wardrobe: the dressed ped is not drawn through the viewport — pass --viewport",
                ),
            }
            report.record(
                "wardrobe: the garment panel with none selected (the pack)",
                with_selection(state, frames, blank, false),
            );
            report.record(
                "wardrobe: the garment panel with a garment selected",
                with_selection(state, frames, blank, true),
            );
            report.record(
                "wardrobe: the garment panel with a look at the models and the step before",
                mesh_doctor(state, frames),
            );
            report.record(
                "wardrobe: the Fix question",
                asked(state, frames, |state| ask_fix(state, false)),
            );
            report.record(
                "wardrobe: the Waive question",
                asked(state, frames, |state| ask_fix(state, true)),
            );
            report.record(
                "wardrobe: the Rules window",
                asked(state, frames, |state| {
                    if let Some(module) = state.wardrobe_module_id() {
                        state.ask_about_rules(module);
                    }
                }),
            );
        }
    }
    // ⚠ A scratch project the units changed asks to be saved before it closes, which no unit answers: it is the scratch folder's, and goes with it.
    state.project_carried.changed = false;
    state.close_project();
    // ⚠ Closing a project raises the start page over the workspace on screen, which the units after this draw.
    state.hide_start_page();
    state.workspace_docks.reset(Workspace::Wardrobe);
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

/// Make the scratch project with the Clothing pack template, open it, and drop clothes on its Wardrobe.
fn open(state: &mut AppState, folder: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(folder);
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut draft = NewProject::new(Template::Clothing, folder.to_path_buf());
    draft.set_name("check".into());
    let created = draft.create()?;

    let drop = folder.join("drop");
    let write = |name: String, body: &[u8]| -> Result<PathBuf, String> {
        let path = drop.join(&name);
        std::fs::create_dir_all(&drop)
            .and_then(|()| std::fs::write(&path, or_source::sample::as_a_game_file(&name, body)))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    };
    let dropped = vec![
        write(format!("{MODEL}0_u.ydd"), b"a jacket")?,
        write(format!("{MODEL}1_u.ydd"), b"a jacket")?,
        write(format!("{MODEL}0_u.yld"), b"cloth")?,
        write(
            "mp_m_freemode_01_mp_m_check^jbib_diff_000_a_uni.ytd".into(),
            b"red",
        )?,
        write(
            "mp_m_freemode_01_mp_m_check^jbib_diff_001_a_uni.ytd".into(),
            b"red",
        )?,
        write("my scarf.ydd".into(), b"a scarf")?,
    ];
    state.load_project_from(created.file);
    wait(state, "the project's scan", |state| {
        state.workbench.is_scanned()
    })?;
    state.handle_os_drop(dropped, DropTarget::Wardrobe);
    wait(state, "the drop", |state| {
        state
            .wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.wardrobe.garments.len() == 3)
            && state
                .workbench
                .findings()
                .iter()
                .any(|f| f.text.starts_with("Ships the same model as"))
    })?;
    Ok(())
}

/// The dressed ped's panel with a scene standing, drawn through the render bridge, which must have built a mesh for it.
///
/// ⚠ The scene is a box with a checker texture, where a ped would stand: the garments of this scratch project are
/// stand-ins, and a ped needs no install to be drawn. No poll runs inside a unit, so nothing replaces it.
fn dressed_ped(
    state: &mut AppState,
    bridge: &mut crate::render::RenderBridge,
    frames: usize,
    blank: &Painted,
) -> Outcome {
    state.workbench.wardrobe.dressed.show(stand_in());
    let outcome = draw_unit(state, Some(&mut *bridge), frames, |state| {
        state.active_workspace = Workspace::Wardrobe;
        *state.workspace_docks.get_mut(Workspace::Wardrobe) =
            egui_dock::DockState::new(vec![Tab::DRESSED]);
    });
    let (meshes, _) = bridge.document_geometry();
    state.workbench.wardrobe.dressed = Default::default();
    let painted = outcome.and_then(|painted| beats(painted, blank))?;
    if meshes == 0 {
        return Err("the viewport built no mesh for the dressed ped".into());
    }
    Ok(painted)
}

/// What the viewport drew of the stand-in in a view: what egui painted, the meshes built and the line vertices drawn.
fn drawn_in(
    state: &mut AppState,
    bridge: &mut crate::render::RenderBridge,
    frames: usize,
    (view, bones): (ViewMode, bool),
) -> Result<(Painted, usize, usize), String> {
    let ped = state.dressed_ped();
    let dressed = &mut state.workbench.wardrobe.dressed;
    dressed.show(stand_in());
    dressed.show_skeleton(ped, stand_in_skeleton());
    dressed.view = view;
    dressed.bones = bones;
    let outcome = draw_unit(state, Some(&mut *bridge), frames, |state| {
        state.active_workspace = Workspace::Wardrobe;
        *state.workspace_docks.get_mut(Workspace::Wardrobe) =
            egui_dock::DockState::new(vec![Tab::DRESSED]);
    });
    let (meshes, _) = bridge.document_geometry();
    let lines = bridge.drawn_lines();
    state.workbench.wardrobe.dressed = Default::default();
    Ok((outcome?, meshes, lines))
}

/// Each way of seeing the dressed ped, and its bones, drawn through the viewport: the wireframe adds the edges of the ped to the lines, the bones add the skeleton's, and no other view adds a line.
fn views(
    state: &mut AppState,
    bridge: &mut crate::render::RenderBridge,
    frames: usize,
    blank: &Painted,
    report: &mut Report,
) {
    let base = drawn_in(state, bridge, frames, (ViewMode::Material, false)).map(|(_, _, l)| l);
    for mode in ViewMode::ALL {
        let outcome = base.clone().and_then(|plain| {
            let (painted, meshes, lines) = drawn_in(state, bridge, frames, (mode, false))?;
            let painted = beats(painted, blank)?;
            match (mode, meshes, lines) {
                (_, 0, _) => Err("the viewport built no mesh for the dressed ped".into()),
                (ViewMode::Wireframe, _, l) if l <= plain => Err(format!(
                    "drew {l} line vertices, no more than the {plain} of the grid"
                )),
                (ViewMode::Wireframe, ..) => Ok(painted),
                (_, _, l) if l != plain => {
                    Err(format!("drew {l} line vertices where the grid has {plain}"))
                }
                _ => Ok(painted),
            }
        });
        report.record(
            format!("wardrobe: the dressed ped in the {} view", mode.name()),
            outcome,
        );
    }
    let bones = base.and_then(|plain| {
        let (painted, _, lines) = drawn_in(state, bridge, frames, (ViewMode::Material, true))?;
        match lines > plain {
            true => beats(painted, blank),
            false => Err(format!(
                "drew {lines} line vertices, no more than the {plain} without bones"
            )),
        }
    });
    report.record("wardrobe: the bones of the dressed ped", bones);
}

/// Two bones, the second a metre above the first.
fn stand_in_skeleton() -> or_anim::Skeleton {
    use or_io::gta5::{CrBoneData, CrSkeletonData, Vec4V};
    let v4 = |x: f32, y: f32, z: f32, w: f32| Vec4V {
        x,
        y,
        z,
        w,
        ..Vec4V::default()
    };
    let bone = |index: u16, parent: i16, z: f32| CrBoneData {
        rotation: v4(0.0, 0.0, 0.0, 1.0),
        translation: v4(0.0, 0.0, z, 0.0),
        scale: v4(1.0, 1.0, 1.0, 0.0),
        next_index: -1,
        parent_index: parent,
        name: format!("SKEL_{index}"),
        index,
        bone_id: 100 + index,
        ..CrBoneData::default()
    };
    or_anim::Skeleton::build(&CrSkeletonData {
        parent_indices: vec![-1, 0],
        bones: vec![bone(0, -1, 0.0), bone(1, 0, 1.0)],
        ..CrSkeletonData::default()
    })
}

/// A box 0.5 by 0.3 by 1.7 metres standing on the floor, with a checker for its texture.
fn stand_in() -> crate::render::dressed::DressedScene {
    use crate::render::dressed::{Diffuse, DressedScene, Geometry, ScenePart};
    use or_renderer::MeshVertex;
    let (x, y, z) = (0.25_f32, 0.15_f32, 1.7_f32);
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [[-x, -y, z], [x, -y, z], [x, y, z], [-x, y, z]],
        ),
        (
            [0.0, 0.0, -1.0],
            [[-x, y, 0.0], [x, y, 0.0], [x, -y, 0.0], [-x, -y, 0.0]],
        ),
        (
            [0.0, -1.0, 0.0],
            [[-x, -y, 0.0], [x, -y, 0.0], [x, -y, z], [-x, -y, z]],
        ),
        (
            [0.0, 1.0, 0.0],
            [[x, y, 0.0], [-x, y, 0.0], [-x, y, z], [x, y, z]],
        ),
        (
            [-1.0, 0.0, 0.0],
            [[-x, y, 0.0], [-x, -y, 0.0], [-x, -y, z], [-x, y, z]],
        ),
        (
            [1.0, 0.0, 0.0],
            [[x, -y, 0.0], [x, y, 0.0], [x, y, z], [x, -y, z]],
        ),
    ];
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for (normal, corners) in faces {
        let base = vertices.len() as u32;
        for (corner, uv) in corners
            .iter()
            .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        {
            vertices.push(MeshVertex::new(*corner, normal, uv));
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let rgba: Vec<u8> = (0..64 * 64)
        .flat_map(|i| match ((i % 64) / 8 + (i / 64) / 8) % 2 {
            0 => [200, 200, 200, 255],
            _ => [60, 90, 160, 255],
        })
        .collect();
    DressedScene::new(
        1,
        vec![ScenePart {
            geometry: std::sync::Arc::new(vec![Geometry { vertices, indices }]),
            diffuse: Some(std::sync::Arc::new(Diffuse {
                id: u64::MAX,
                width: 64,
                height: 64,
                rgba,
                srgb: true,
            })),
            tint: [1.0; 3],
            lift: 0.0,
            garment: None,
        }],
    )
}

/// The Garment panel drawn with a garment selected and without, the latter being the pack's settings.
fn with_selection(state: &mut AppState, frames: usize, blank: &Painted, select: bool) -> Outcome {
    let module = state.wardrobe_module_id().ok_or("no clothing content")?;
    let first = state
        .wardrobe_pack()
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments.first())
        .map(|g| g.id.clone());
    state.select_garment(&module, select.then_some(first).flatten());
    draw_unit(state, None, frames, |state| {
        state.active_workspace = Workspace::Wardrobe;
        *state.workspace_docks.get_mut(Workspace::Wardrobe) =
            egui_dock::DockState::new(vec![Tab::DETAILS]);
    })
    .and_then(|painted| beats(painted, blank))
}

/// The Garment panel with a look at the models and the step before standing, judged against the same panel with neither.
fn mesh_doctor(state: &mut AppState, frames: usize) -> Outcome {
    use crate::state::workbench::{Before, Looked};
    use or_wardrobe::mesh::{Count, Examination, ModelExam};
    let module = state.wardrobe_module_id().ok_or("no clothing content")?;
    let garment = state
        .wardrobe_pack()
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments.iter().find(|g| g.model.is_some()))
        .map(|g| (g.id.clone(), g.model.clone().unwrap_or_default()))
        .ok_or("no garment holds a model")?;
    state.select_garment(&module, Some(garment.0.clone()));
    let unit = |state: &mut AppState| {
        draw_unit(state, None, frames, |state| {
            state.active_workspace = Workspace::Wardrobe;
            *state.workspace_docks.get_mut(Workspace::Wardrobe) =
                egui_dock::DockState::new(vec![Tab::DETAILS]);
        })
    };
    let without = unit(state)?;
    let count = |triangles| Count {
        triangles,
        vertices: triangles,
    };
    let desk = &mut state.workbench.wardrobe.mesh;
    desk.looked = Some(Looked {
        garment: garment.0.clone(),
        result: Ok(Examination {
            models: vec![ModelExam {
                file: garment.1.clone(),
                levels: [count(1200), count(0), count(0), count(0)],
                missing: vec![1, 2, 3],
                rig: vec!["12 vertices have no weight at all".into()],
                heel: None,
                uv_used: Some(0.4),
                uv_crop: None,
                unreadable: None,
            }],
        }),
    });
    desk.before = Some(Before {
        garment: garment.0.clone(),
        was: "models/was.drawables.src".into(),
        now: garment.1.clone(),
        shown: false,
    });
    let with = unit(state);
    state.workbench.wardrobe.mesh = Default::default();
    let with = with?;
    if with.vertices > without.vertices {
        Ok(with)
    } else {
        Err(format!(
            "painted {} vertices with a look and the step before standing and {} without — it drew nothing",
            with.vertices, without.vertices
        ))
    }
}

/// Ask the Fix question (or the Waive one) of the duplicate the engine found.
fn ask_fix(state: &mut AppState, waive: bool) {
    let found = state
        .workbench
        .findings()
        .iter()
        .find(|f| f.text.starts_with("Ships the same model as"))
        .cloned();
    let Some(finding) = found else {
        return;
    };
    let wanted = |fix: &&Fix| matches!(fix, Fix::Waive { .. }) == waive;
    if let Some(fix) = finding.fixes.iter().find(wanted) {
        state.ask_about_fix(fix);
    }
}

/// The Wardrobe workspace with a question asked, judged against the same workspace with none.
fn asked(state: &mut AppState, frames: usize, ask: fn(&mut AppState)) -> Outcome {
    let without = draw_unit(state, None, frames, |state| {
        state.active_workspace = Workspace::Wardrobe;
        state.workspace_docks.reset(Workspace::Wardrobe);
    })?;
    let with = draw_unit(state, None, frames, ask);
    let was_asked = matches!(
        state.workbench.asking,
        Some(Question::Fix { .. } | Question::Waive { .. } | Question::Rules { .. })
    );
    state.workbench.asking = None;
    let with = with?;
    if !was_asked {
        return Err("the question was not asked".into());
    }
    if with.vertices > without.vertices {
        Ok(with)
    } else {
        Err(format!(
            "painted {} vertices with the question asked and {} without — it drew nothing",
            with.vertices, without.vertices
        ))
    }
}
