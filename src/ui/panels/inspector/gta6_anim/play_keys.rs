//! The Insert key and Clear key buttons over a GTA VI animation's timeline, as GTA V's clips have
//! under theirs: each keys the selected track rows at the playhead.

use or_io::gta6::anim::{AnimAt, AnimResource, Animation};

use super::super::timeline::{Playing, KIND_DOF, KIND_ID};
use super::{choice, PlayState};
use crate::state::gta6_anim_keys::{frame_at, KeyAction, KeyTarget};
use crate::state::{AppState, DocumentId};
use crate::ui::theme;
use crate::ui::timeline::RowId;

/// Where `animation` is among `resource`'s sets; none for one another body stores.
pub(super) fn place(resource: &AnimResource, animation: &Animation) -> Option<AnimAt> {
    resource.sets.iter().enumerate().find_map(|(set, s)| {
        let a = s
            .animations
            .iter()
            .position(|a| std::ptr::eq(a, animation))?;
        Some(AnimAt { set, animation: a })
    })
}

/// The tracks `rows` select that `resource` stores, each at its layer's playhead: a track row's
/// DOF, and every DOF of a bone's row, as GTA V's bone row keys each of its tracks. A layer whose
/// group does not play at the playhead keys nothing.
fn targets(
    resource: &AnimResource,
    playing: &Playing,
    rows: impl IntoIterator<Item = RowId>,
) -> Vec<KeyTarget> {
    let mut out: Vec<KeyTarget> = Vec::new();
    for row in rows {
        let (kind, id, place_of_row) = (row.kind(), row.primary(), usize::from(row.secondary()));
        if kind != KIND_DOF && kind != KIND_ID {
            continue;
        }
        for layer in &playing.layers {
            let Some(time) = layer.time else {
                continue;
            };
            let dofs = (layer.animation.dofs.iter().enumerate())
                .filter(|(d, dof)| dof.id == id && (kind == KIND_ID || *d == place_of_row))
                .map(|(d, _)| d)
                .collect::<Vec<_>>();
            if dofs.is_empty() {
                continue;
            }
            let Some(animation) = place(resource, layer.animation) else {
                continue;
            };
            for dof in dofs {
                if !out.iter().any(|t| t.animation == animation && t.dof == dof) {
                    out.push(KeyTarget {
                        animation,
                        dof,
                        time,
                    });
                }
            }
        }
    }
    out
}

/// The key bar: the playhead's frame, Insert key and Clear key.
pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &AnimResource,
    play: &PlayState,
) {
    let Some(choice) = choice(resource, play.choice) else {
        return;
    };
    let playing = Playing::new(resource, choice, &[], play.transport.time);
    let targets = targets(
        resource,
        &playing,
        play.timeline.selected_rows.iter().copied(),
    );
    let frame = targets.first().and_then(|t| {
        let at = t.animation;
        frame_at(
            resource.sets.get(at.set)?.animations.get(at.animation)?,
            t.time,
        )
    });
    ui.horizontal(|ui| {
        if let Some(frame) = frame {
            ui.label(
                egui::RichText::new(tr!("frame {frame}", frame = frame))
                    .small()
                    .monospace()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        ui.add_enabled_ui(!targets.is_empty(), |ui| {
            let insert = (ui.small_button(tr!("Insert key"))).on_hover_text(tr!(
                "Set each selected track at the playhead's frame to its value at the playhead."
            ));
            if insert.clicked() {
                state.key_gta6_tracks(doc_id, &targets, KeyAction::Insert);
            }
            let clear = (ui.small_button(tr!("Clear key"))).on_hover_text(tr!(
                "Set each selected track at the playhead's frame to the value between the frames on either side, as though that frame held no key of its own."
            ));
            if clear.clicked() {
                state.key_gta6_tracks(doc_id, &targets, KeyAction::Clear);
            }
        });
        if targets.is_empty() {
            ui.label(
                egui::RichText::new(tr!("Select a track row to key it."))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use or_io::gta6::anim::{AnimSet, Dof, RootKind};

    use super::super::Choice;
    use super::*;

    #[test]
    fn a_selected_track_row_keys_its_dof_at_the_layer_s_playhead_and_no_other_row_keys() {
        let dof = |id| Dof {
            id,
            track: 0,
            kind: 4,
        };
        let walk = Animation {
            name: "walk".into(),
            frames: 31,
            frames_per_chunk: 255,
            duration: 1.0,
            dofs: vec![dof(7), dof(9)],
            ..Animation::default()
        };
        let mut resource = AnimResource {
            root: RootKind::AnimationSet,
            root_word: 0,
            name: String::new(),
            key: 0,
            block_map: None,
            word_at_60: 0,
            word_at_98: None,
            sets: vec![AnimSet {
                name: String::new(),
                key: 0,
                animations: vec![walk],
            }],
            external_sets: Vec::new(),
            animation_keys: Vec::new(),
            clips: Vec::new(),
            motions: Vec::new(),
            matchers: Vec::new(),
            dictionaries: Vec::new(),
            end_region: None,
            sections: None,
            edited: false,
        };
        let at = AnimAt {
            set: 0,
            animation: 0,
        };
        let playing = Playing::new(&resource, Choice::Animation(at), &[], 0.5);
        // The second DOF's row, the first's id on the second's place, and a row of no DOF.
        let rows = [
            RowId::new(KIND_DOF, 9, 1),
            RowId::new(KIND_DOF, 7, 1),
            RowId::new(KIND_DOF + 1, 9, 1),
        ];
        let expected = [KeyTarget {
            animation: at,
            dof: 1,
            time: 0.5,
        }];
        assert_eq!(targets(&resource, &playing, rows), expected);
        assert_eq!(frame_at(&resource.sets[0].animations[0], 0.5), Some(15));

        // A bone's row keys every DOF of its id, once however often it is selected.
        drop(playing);
        resource.sets[0].animations[0].dofs.push(dof(9));
        let playing = Playing::new(&resource, Choice::Animation(at), &[], 0.5);
        let rows = [RowId::new(KIND_ID, 9, 0), RowId::new(KIND_DOF, 9, 1)];
        let expected: Vec<KeyTarget> = [1, 2]
            .map(|dof| KeyTarget {
                animation: at,
                dof,
                time: 0.5,
            })
            .into();
        assert_eq!(targets(&resource, &playing, rows), expected);
    }
}
