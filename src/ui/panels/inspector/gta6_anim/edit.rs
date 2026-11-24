//! The edits a GTA VI animation resource takes: a clip's window and rate, through the clip fields
//! GTA V's clips edit, a group entry's window, a clip's name, a key of any channel a key is
//! written to, and a track held over the whole clip, added or removed.

use std::sync::Arc;

use or_io::codec::anim::AnimationSource;
use or_io::gta6::anim::{track_label, AnimAt, AnimEdit, AnimResource, Animation, ClipPlay, Dof};
use or_io::gta6::hash::name_hash32;

use super::*;

/// What the edit section has picked: a clip to name, a clip of one animation, a clip of groups
/// with one of its entries, an animation, a channel of it and a frame.
#[derive(Clone, Copy, Default)]
struct Pick {
    named: usize,
    clip: usize,
    grouped: usize,
    group: usize,
    entry: usize,
    animation: AnimAt,
    channel: usize,
    frame: u16,
}

pub(super) fn draw_edit(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &Arc<AnimResource>,
) {
    if resource.sections.is_none() {
        return;
    }
    egui::CollapsingHeader::new(tr!("Edit"))
        .id_salt(("gta6_anim_edit", doc_id))
        .show(ui, |ui| {
            let id = ui.make_persistent_id(("gta6_anim_edit_pick", doc_id));
            let mut pick: Pick = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
            draw_name(ui, state, doc_id, resource, &mut pick);
            draw_window(ui, state, doc_id, resource, &mut pick);
            draw_entry(ui, state, doc_id, resource, &mut pick);
            draw_key(ui, state, doc_id, resource, &mut pick);
            ui.data_mut(|d| d.insert_temp(id, pick));
            ui.label(
                egui::RichText::new(tr!("Save writes the edits as a loose resource. A key of section 10 is encoded afresh from its integer steps and any other is written in place; an edited block keeps the hash it was read with, which follows from no rule anyone found, and an edited track list is signed again."))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        });
}

/// A clip's name, which its key hashes, written once the field lets go.
fn draw_name(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &AnimResource,
    pick: &mut Pick,
) {
    let Some(last) = resource.clips.len().checked_sub(1) else {
        return;
    };
    pick.named = pick.named.min(last);
    let clip = pick.named;
    let held = &resource.clips[clip].name;
    let id = egui::Id::new(("gta6_clip_name", doc_id, clip));
    let mut text = (ui.data(|d| d.get_temp::<String>(id))).unwrap_or_else(|| held.clone());
    let mut renamed = None;
    ui.horizontal(|ui| {
        egui::ComboBox::new(("gta6_anim_edit_named", doc_id), tr!("Name"))
            .selected_text(held.as_str())
            .show_ui(ui, |ui| {
                for (i, c) in resource.clips.iter().enumerate() {
                    ui.selectable_value(&mut pick.named, i, &c.name);
                }
            });
        let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
        if response.changed() {
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
        }
        if response.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(id));
            renamed = (text != *held).then(|| text.clone());
        }
    });
    if let Some(name) = renamed {
        state.rename_gta6_clip(doc_id, clip, &name);
    }
}

/// The window and rate of a clip of one animation.
fn draw_window(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &AnimResource,
    pick: &mut Pick,
) {
    let ones: Vec<usize> = (resource.clips.iter().enumerate())
        .filter(|(_, c)| matches!(c.play, ClipPlay::One { .. }))
        .map(|(i, _)| i)
        .collect();
    let Some(&first) = ones.first() else {
        return;
    };
    if !ones.contains(&pick.clip) {
        pick.clip = first;
    }
    egui::ComboBox::new(("gta6_anim_edit_clip", doc_id), tr!("Clip"))
        .selected_text(&resource.clips[pick.clip].name)
        .show_ui(ui, |ui| {
            for &i in &ones {
                ui.selectable_value(&mut pick.clip, i, &resource.clips[i].name);
            }
        });
    let clip = pick.clip as u32;
    let Some(before) = state.clip_field_snapshot(doc_id, clip) else {
        return;
    };
    let mut edited = before.clone();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(tr!("Window"));
        changed |= (ui.add(
            egui::DragValue::new(&mut edited.start_time)
                .speed(0.01)
                .suffix(" s"),
        ))
        .changed();
        ui.label("→");
        changed |= (ui.add(
            egui::DragValue::new(&mut edited.end_time)
                .speed(0.01)
                .suffix(" s"),
        ))
        .changed();
        ui.label(tr!("Rate"));
        changed |= (ui.add(
            egui::DragValue::new(&mut edited.rate)
                .speed(0.01)
                .range(0.01..=16.0),
        ))
        .changed();
    });
    edited.end_time = edited.end_time.max(edited.start_time);
    if changed && edited != before && state.apply_clip_fields(doc_id, clip, &edited) {
        let change = crate::state::Change::ClipFields {
            clip_key: clip,
            other: before,
        };
        state.record(doc_id, tr!("Edit the clip's window"), change);
    }
}

/// The window of an entry of a group of a clip of groups; its rate follows from the group's
/// duration.
fn draw_entry(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &AnimResource,
    pick: &mut Pick,
) {
    let grouped: Vec<usize> = (resource.clips.iter().enumerate())
        .filter(|(_, c)| matches!(c.play, ClipPlay::Groups { .. }))
        .map(|(i, _)| i)
        .collect();
    let Some(&first) = grouped.first() else {
        return;
    };
    if !grouped.contains(&pick.grouped) {
        pick.grouped = first;
    }
    egui::ComboBox::new(("gta6_anim_edit_grouped", doc_id), tr!("Clip of groups"))
        .selected_text(&resource.clips[pick.grouped].name)
        .show_ui(ui, |ui| {
            for &i in &grouped {
                ui.selectable_value(&mut pick.grouped, i, &resource.clips[i].name);
            }
        });
    let ClipPlay::Groups { groups, .. } = &resource.clips[pick.grouped].play else {
        return;
    };
    let Some(last_group) = groups.len().checked_sub(1) else {
        return;
    };
    pick.group = pick.group.min(last_group);
    let entries = &groups[pick.group].entries;
    let Some(last_entry) = entries.len().checked_sub(1) else {
        return;
    };
    pick.entry = pick.entry.min(last_entry);
    let held = &entries[pick.entry];
    let (mut start, mut end) = (held.start, held.end);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(tr!("Group"));
        ui.add(egui::DragValue::new(&mut pick.group).range(0..=last_group));
        ui.label(tr!("Entry"));
        ui.add(egui::DragValue::new(&mut pick.entry).range(0..=last_entry));
        ui.label(tr!("Window"));
        changed |= drag_value(ui, &mut start, |d| d.speed(0.01).suffix(" s")).inner;
        ui.label("→");
        changed |= drag_value(ui, &mut end, |d| d.speed(0.01).suffix(" s")).inner;
        ui.label(tr!("rate {rate}", rate = format!("{:.3}", held.rate)));
    });
    if changed {
        let edit = AnimEdit::EntryWindow {
            clip: pick.grouped,
            group: pick.group,
            entry: pick.entry,
            start,
            end,
            frame_rate: None,
        };
        state.edit_gta6_entry(doc_id, edit);
    }
}

/// The keys an animation's channels take, kept while the resource and the animation are the same.
fn key_components(
    ui: &egui::Ui,
    doc_id: DocumentId,
    resource: &AnimResource,
    at: AnimAt,
) -> Arc<Vec<(usize, u8)>> {
    let id = egui::Id::new(("gta6_anim_edit_keys", doc_id));
    let stamp = (std::ptr::from_ref(resource) as usize, at.set, at.animation);
    type Kept = ((usize, usize, usize), Arc<Vec<(usize, u8)>>);
    if let Some((s, kept)) = ui.data(|d| d.get_temp::<Kept>(id)) {
        if s == stamp {
            return kept;
        }
    }
    let pairs = (resource.sets.get(at.set))
        .and_then(|s| s.animations.get(at.animation))
        .map(|a| a.key_components())
        .unwrap_or_default();
    let kept = Arc::new(pairs);
    ui.data_mut(|d| d.insert_temp(id, (stamp, kept.clone())));
    kept
}

/// The value of a channel at a frame.
fn draw_key(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &AnimResource,
    pick: &mut Pick,
) {
    let name = |at: AnimAt| {
        (resource.sets.get(at.set))
            .and_then(|s| s.animations.get(at.animation))
            .map(|a| a.name.as_str())
    };
    if name(pick.animation).is_none() {
        pick.animation = AnimAt::default();
    }
    let Some(selected) = name(pick.animation) else {
        return;
    };
    egui::ComboBox::new(("gta6_anim_edit_animation", doc_id), tr!("Animation"))
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (s, set) in resource.sets.iter().enumerate() {
                for (a, animation) in set.animations.iter().enumerate() {
                    let at = AnimAt {
                        set: s,
                        animation: a,
                    };
                    ui.selectable_value(&mut pick.animation, at, &animation.name);
                }
            }
        });
    let animation = &resource.sets[pick.animation.set].animations[pick.animation.animation];
    draw_add_track(ui, state, doc_id, animation, pick.animation);
    let channels = key_components(ui, doc_id, resource, pick.animation);
    let Some(&first) = channels.first() else {
        ui.label(tr!(
            "This animation holds no channel a key is written to: each is indirect or follows from the others."
        ));
        return;
    };
    let label = |(dof, component): (usize, u8)| {
        let track = animation
            .dofs
            .get(dof)
            .map(|d| d.track_label())
            .unwrap_or_default();
        format!("{dof}.{component} {track}")
    };
    pick.channel = pick.channel.min(channels.len() - 1);
    egui::ComboBox::new(("gta6_anim_edit_channel", doc_id), tr!("Channel"))
        .selected_text(label(channels.get(pick.channel).copied().unwrap_or(first)))
        .show_ui(ui, |ui| {
            for (i, &channel) in channels.iter().enumerate() {
                ui.selectable_value(&mut pick.channel, i, label(channel));
            }
        });
    let (dof, component) = channels[pick.channel];
    pick.frame = pick.frame.min(animation.frames.saturating_sub(1));
    ui.horizontal(|ui| {
        ui.label(tr!("Frame"));
        ui.add(egui::DragValue::new(&mut pick.frame).range(0..=animation.frames.saturating_sub(1)));
        let Some(mut value) = animation.key_value(dof, component, pick.frame) else {
            return;
        };
        ui.label(tr!("Value"));
        if ui
            .add(egui::DragValue::new(&mut value).speed(0.01))
            .changed()
        {
            let edit = AnimEdit::Key {
                animation: pick.animation,
                dof,
                component,
                frame: pick.frame,
                value,
            };
            state.edit_gta6_key(doc_id, edit);
        }
    });
    let time = animation.internal_frame_to_time(f32::from(pick.frame));
    let held = animation.sample(dof, time).map(|s| s.components);
    ui.horizontal(|ui| {
        let hold = (ui.button(tr!("Hold over the whole clip")))
            .on_hover_text(tr!("The track takes its value at this frame at every frame of the clip: one constant in each block in place of its channels."));
        if let (true, Ok(value)) = (hold.clicked(), held) {
            let edit = AnimEdit::Pose {
                animation: pick.animation,
                dof,
                value,
            };
            state.relay_gta6_anim(doc_id, edit);
        }
        if ui.button(tr!("Remove a track")).clicked() {
            let edit = AnimEdit::RemoveDof {
                animation: pick.animation,
                dof,
            };
            state.relay_gta6_anim(doc_id, edit);
        }
    });
    ui.label(
        egui::RichText::new(tr!("A constant channel takes the value at every frame of its block; a quantised one keeps its bits and is quantised afresh when the value lies off its range."))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

/// A track added to the animation: a bone's name, or `hash_` and eight hex digits, and one of the
/// tracks the animation has, whose type the new one takes; it holds a rest value at every frame.
fn draw_add_track(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    animation: &Animation,
    at: AnimAt,
) {
    let mut kinds: Vec<(u32, u32)> = animation.dofs.iter().map(|d| (d.track, d.kind)).collect();
    kinds.sort_unstable();
    kinds.dedup_by_key(|(track, _)| *track);
    let Some(&(first, _)) = kinds.first() else {
        return;
    };
    let id = egui::Id::new(("gta6_anim_add_track", doc_id));
    let (mut text, mut track): (String, u32) =
        (ui.data(|d| d.get_temp(id))).unwrap_or_else(|| (String::new(), first));
    ui.horizontal(|ui| {
        ui.label(tr!("Id"));
        ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0));
        egui::ComboBox::new(("gta6_anim_add_track_kind", doc_id), tr!("Track"))
            .selected_text(track_label(track))
            .show_ui(ui, |ui| {
                for &(t, _) in &kinds {
                    ui.selectable_value(&mut track, t, track_label(t));
                }
            });
        let kind = (kinds.iter()).find(|(t, _)| *t == track).map(|(_, k)| *k);
        if ui.button(tr!("Add a track")).clicked() {
            if let (Some(id), Some(kind)) = (dof_id(&text), kind) {
                let value = rest(kind, &track_label(track));
                let dof = Dof { id, track, kind };
                let edit = AnimEdit::AddDof {
                    animation: at,
                    dof,
                    value,
                };
                state.relay_gta6_anim(doc_id, edit);
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (text, track)));
}

/// The id typed: `hash_` and eight hex digits, else the hash of a name.
fn dof_id(text: &str) -> Option<u32> {
    let text = text.trim();
    match text.get(..5).filter(|p| p.eq_ignore_ascii_case("hash_")) {
        Some(_) => u32::from_str_radix(&text[5..], 16).ok(),
        None => (!text.is_empty()).then(|| name_hash32(text.as_bytes())),
    }
}

/// The value a track takes when added: no rotation, a unit scale, else zero.
fn rest(kind: u32, track: &str) -> [f32; 4] {
    match kind {
        0 => [0.0, 0.0, 0.0, 1.0],
        _ if track.ends_with("Scale") => [1.0, 1.0, 1.0, 0.0],
        _ => [0.0; 4],
    }
}

#[cfg(test)]
mod tests {
    use super::{dof_id, rest};

    #[test]
    fn a_typed_id_is_a_hash_given_or_the_hash_of_a_name() {
        assert_eq!(dof_id(" hash_0000BEEF "), Some(0xBEEF));
        assert_eq!(dof_id("HASH_zz"), None);
        assert_eq!(
            dof_id("SKEL_Head"),
            Some(or_io::gta6::hash::name_hash32(b"SKEL_Head"))
        );
        assert_eq!(dof_id("  "), None);
        assert_eq!(rest(0, "boneRotation"), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(rest(2, "boneScale"), [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(rest(2, "boneTranslation"), [0.0; 4]);
    }
}
