//! The expression set whose facial controls a GTA VI clip carries, picked among the sets opened
//! from a scan: each control of the clip the set reads, and its value at the playhead.

use std::path::PathBuf;

use or_io::gta6::anim::{track_label, AnimResource, Animation, Expressions};

use super::play::Choice;
use super::timeline::Playing;
use super::ROWS_IN_VIEW;
use crate::state::gta6_face::{expression_set, is_expression_set};
use crate::state::{AppState, DocumentId};
use crate::ui::panel_state::PanelState;

#[derive(Default)]
struct FaceState {
    /// The set picked: its document, its name and what it reads.
    set: Option<(DocumentId, String, Expressions)>,
    /// The DOFs of each animation played that the set reads, by the animation's key.
    read: Vec<(u32, Vec<usize>)>,
}

impl PanelState for FaceState {
    const KEY: &'static str = "gta6-anim-face";
}

/// The expression set picker, and the controls of what `choice` plays at `time` the set reads.
pub(super) fn draw_expressions(
    ui: &mut egui::Ui,
    state: &mut AppState,
    resource: &AnimResource,
    choice: Option<Choice>,
    time: f32,
) {
    let mut face = state.panels.take::<FaceState>();
    egui::CollapsingHeader::new(tr!("Expression set"))
        .id_salt("gta6_anim_expressions")
        .show(ui, |ui| draw(ui, state, resource, choice, time, &mut face));
    state.panels.put(face);
}

fn draw(
    ui: &mut egui::Ui,
    state: &AppState,
    resource: &AnimResource,
    choice: Option<Choice>,
    time: f32,
    face: &mut FaceState,
) {
    if (face.set.as_ref()).is_some_and(|(doc, _, _)| state.document(*doc).is_none()) {
        face.set = None;
        face.read.clear();
    }
    let shown = face.set.as_ref().map(|(_, name, _)| name.clone());
    egui::ComboBox::from_label(tr!("Set"))
        .selected_text(shown.unwrap_or_else(|| tr!("none").to_string()))
        .show_ui(ui, |ui| {
            let sets: Vec<(DocumentId, String, PathBuf)> = (state.documents.iter())
                .filter_map(|d| {
                    let path = d.path.clone()?;
                    is_expression_set(&path).then(|| (d.id, d.file_name(), path))
                })
                .collect();
            if sets.is_empty() {
                ui.label(tr!(
                    "Open an expression set from a scan of a GTA VI archive to pick it here."
                ));
            }
            for (doc, name, path) in sets {
                let now = face.set.as_ref().is_some_and(|(d, _, _)| *d == doc);
                if ui.selectable_label(now, &name).clicked() {
                    face.set = expression_set(&path).map(|set| (doc, name, set));
                    face.read.clear();
                }
            }
        });
    let (Some((_, _, set)), Some(choice)) = (&face.set, choice) else {
        return;
    };
    let playing = Playing::new(resource, choice, &[], time);
    for layer in &playing.layers {
        let animation = layer.animation;
        if !face.read.iter().any(|(key, _)| *key == animation.key) {
            let dofs = (animation.dofs.iter().enumerate())
                .filter(|(_, dof)| set.inputs.tracks.contains(&(dof.id, dof.track)))
                .map(|(d, _)| d)
                .collect();
            face.read.push((animation.key, dofs));
        }
    }
    let read = &face.read;
    let rows: Vec<(&Animation, f32, usize)> = (playing.layers.iter())
        .filter_map(|layer| Some((layer.animation, layer.time?)))
        .filter_map(move |(animation, at)| {
            let (_, dofs) = read.iter().find(|(key, _)| *key == animation.key)?;
            Some(dofs.iter().map(move |&d| (animation, at, d)))
        })
        .flatten()
        .collect();
    ui.label(tr_n!(
        "{count} DOF of what plays is a control the set reads. The set holds no program, so no control poses the face; each one's value at the playhead:",
        "{count} DOFs of what plays are controls the set reads. The set holds no program, so no control poses the face; each one's value at the playhead:",
        rows.len(),
    ));
    let row_height = ui.spacing().interact_size.y;
    egui::ScrollArea::vertical()
        .id_salt("gta6_anim_expression_rows")
        .max_height(row_height * ROWS_IN_VIEW)
        .auto_shrink([false, true])
        .show_rows(ui, row_height, rows.len(), |ui, range| {
            for &(animation, at, d) in &rows[range] {
                let Some(dof) = animation.dofs.get(d) else {
                    continue;
                };
                let value = match animation.sample(d, at) {
                    Ok(s) => {
                        let shown: Vec<String> = (s.components.iter().take(s.count))
                            .map(|x| format!("{x:.4}"))
                            .collect();
                        shown.join(", ")
                    }
                    Err(e) => e.to_string(),
                };
                let track = track_label(dof.track);
                ui.monospace(format!("hash_{:08X} {track} ({value})", dof.id));
            }
        });
}
