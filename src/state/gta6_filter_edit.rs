//! A scanned GTA VI dictionary's frame filter weights and animation record values, each edited in
//! place as a step of its document's history and written over the body's sections at save, as
//! are a track's or bone's named fields; a track or a bone removed, copied beside the one it
//! copies, or added, moves what follows it at save.

use std::sync::Arc;

use or_io::container::rpf::Rpf9Sections;
use or_io::gta6::headerless::{
    write_filter_tracks, write_filter_weights, write_record_bones, write_record_values, BoneRecord,
    Dictionary, Entry, FilterTrack, HeaderlessError, Resource,
};

use super::edit_history::Change;
use super::{AppState, DocPayload, DocumentId};

/// What an edit of a dictionary's frame filter or animation record sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FilterEdit {
    /// Track `track`'s weight.
    Weight { track: usize, weight: f32 },
    /// Value `value` of bone `bone`, one of the 27 whose roles are not known.
    Value { bone: usize, value: usize, to: f32 },
    /// Track `track` removed.
    RemoveTrack { track: usize },
    /// Track `track` copied, the copy placed after it.
    CopyTrack { track: usize },
    /// Track `track`'s id, group and mode.
    Track {
        track: usize,
        id: u32,
        group: u32,
        mode: u32,
    },
    /// A new track after the last, of the last one's group, weight 1 and mode 0.
    AddTrack,
    /// Bone `bone` removed; a record keeps one.
    RemoveBone { bone: usize },
    /// Bone `bone` copied, the copy placed after it.
    CopyBone { bone: usize },
    /// Bone `bone`'s id, kind and flags.
    Bone {
        bone: usize,
        id: u32,
        kind: u32,
        flags: u32,
    },
    /// A new bone after the last, of the last one's kind and flags, its values zero.
    AddBone,
}

/// `change` as a step of its own, which no later edit of the same entry folds into.
fn alone(mut change: Change) -> Change {
    if let Change::Part { key, .. } = &mut change {
        *key = None;
    }
    change
}

/// What a save writes of one entry: a frame filter's tracks or an animation record's bones.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Values {
    Filter(Vec<FilterTrack>),
    Record(Vec<BoneRecord>),
}

/// `payload`'s dictionary, while no other holder of the body reads it.
fn dictionary_mut(payload: &mut DocPayload) -> Option<&mut Dictionary> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return None;
    };
    match Arc::get_mut(resource)? {
        Resource::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

fn entry_mut(dict: &mut Dictionary, code: u32) -> Option<&mut Entry> {
    (dict.entries.iter_mut()).find_map(|(c, e)| match e {
        Ok(entry) if *c == code => Some(entry),
        _ => None,
    })
}

fn tracks_mut(payload: &mut DocPayload, code: u32) -> Option<&mut Vec<FilterTrack>> {
    match entry_mut(dictionary_mut(payload)?, code)? {
        Entry::FrameFilter { tracks, .. } => Some(tracks),
        _ => None,
    }
}

fn bones_mut(payload: &mut DocPayload, code: u32) -> Option<&mut Vec<BoneRecord>> {
    match entry_mut(dictionary_mut(payload)?, code)? {
        Entry::AnimationRecord { bones, .. } => Some(bones),
        _ => None,
    }
}

impl AppState {
    /// `edit` made to the frame filter or animation record of code `code`, a step of undo.
    pub(crate) fn edit_gta6_filter(
        &mut self,
        doc: DocumentId,
        code: u32,
        edit: FilterEdit,
    ) -> Result<(), String> {
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        if dictionary_mut(&mut document.payload).is_none() {
            return Err(tr!(
                "The entry is not edited while a task reads the body; try again once it ends."
            )
            .to_string());
        }
        let gone = || tr!("That entry holds no such value.").to_string();
        let change = match edit {
            FilterEdit::Weight { track, weight } => {
                let tracks = tracks_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = tracks.clone();
                tracks.get_mut(track).ok_or_else(gone)?.weight = weight;
                let key = format!("gta6 filter {code} {track}");
                Change::part(key, before, move |p| tracks_mut(p, code))
            }
            FilterEdit::Value { bone, value, to } => {
                let bones = bones_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = bones.clone();
                let held = (bones.get_mut(bone)).and_then(|b| b.values.get_mut(value));
                *held.ok_or_else(gone)? = to;
                let key = format!("gta6 record {code} {bone} {value}");
                Change::part(key, before, move |p| bones_mut(p, code))
            }
            FilterEdit::RemoveTrack { track } | FilterEdit::CopyTrack { track } => {
                let tracks = tracks_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = tracks.clone();
                let held = *tracks.get(track).ok_or_else(gone)?;
                if matches!(edit, FilterEdit::CopyTrack { .. }) {
                    tracks.insert(track + 1, held);
                } else {
                    tracks.remove(track);
                }
                alone(Change::part("", before, move |p| tracks_mut(p, code)))
            }
            FilterEdit::RemoveBone { bone } | FilterEdit::CopyBone { bone } => {
                let bones = bones_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = bones.clone();
                let held = *bones.get(bone).ok_or_else(gone)?;
                if matches!(edit, FilterEdit::CopyBone { .. }) {
                    bones.insert(bone + 1, held);
                } else if bones.len() == 1 {
                    return Err(tr!("A record keeps one bone at least.").to_string());
                } else {
                    bones.remove(bone);
                }
                alone(Change::part("", before, move |p| bones_mut(p, code)))
            }
            FilterEdit::Track {
                track,
                id,
                group,
                mode,
            } => {
                if mode > 2 {
                    return Err(tr!("A track's mode is 0, 1 or 2.").to_string());
                }
                let tracks = tracks_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = tracks.clone();
                let held = tracks.get_mut(track).ok_or_else(gone)?;
                (held.track, held.group, held.word_2c) = (id, group, mode);
                let key = format!("gta6 filter fields {code} {track}");
                Change::part(key, before, move |p| tracks_mut(p, code))
            }
            FilterEdit::AddTrack => {
                let tracks = tracks_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = tracks.clone();
                let group = tracks.last().map_or(0, |t| t.group);
                tracks.push(FilterTrack {
                    track: 0,
                    weight: 1.0,
                    group,
                    word_2c: 0,
                });
                alone(Change::part("", before, move |p| tracks_mut(p, code)))
            }
            FilterEdit::Bone {
                bone,
                id,
                kind,
                flags,
            } => {
                let bones = bones_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = bones.clone();
                let held = bones.get_mut(bone).ok_or_else(gone)?;
                (held.bone, held.kind, held.flags) = (id, kind, flags);
                let key = format!("gta6 record fields {code} {bone}");
                Change::part(key, before, move |p| bones_mut(p, code))
            }
            FilterEdit::AddBone => {
                let bones = bones_mut(&mut document.payload, code).ok_or_else(gone)?;
                let before = bones.clone();
                let last = *bones.last().ok_or_else(gone)?;
                bones.push(BoneRecord {
                    bone: 0,
                    values: [0.0; 27],
                    ..last
                });
                alone(Change::part("", before, move |p| bones_mut(p, code)))
            }
        };
        self.record(doc, tr!("Edit a frame filter or animation record"), change);
        Ok(())
    }
}

/// Each frame filter's and animation record's values in `resource`, by code, as a save writes
/// them; none for a resource that is no dictionary.
pub(super) fn values_of(resource: &Resource) -> Vec<(u32, Values)> {
    let Resource::Dictionary(dict) = resource else {
        return Vec::new();
    };
    (dict.entries.iter())
        .filter_map(|(code, e)| match e {
            Ok(Entry::FrameFilter { tracks, .. }) => Some((*code, Values::Filter(tracks.clone()))),
            Ok(Entry::AnimationRecord { bones, .. }) => {
                Some((*code, Values::Record(bones.clone())))
            }
            _ => None,
        })
        .collect()
}

/// `values` written over `sections`: in place where an entry holds the tracks or bones it was read
/// with, else laid out again through the section's growth.
pub(super) fn write(sections: &mut Rpf9Sections, values: &[(u32, Values)]) -> Result<(), String> {
    for (code, held) in values {
        let v = &mut sections.virtual_section;
        let in_place = match held {
            Values::Filter(tracks) => write_filter_weights(v, *code, tracks),
            Values::Record(bones) => write_record_values(v, *code, bones),
        };
        if !matches!(in_place, Err(HeaderlessError::NotInPlace(_))) {
            in_place.map_err(|e| e.to_string())?;
            continue;
        }
        let relaid = match held {
            Values::Filter(tracks) => write_filter_tracks(sections, *code, tracks),
            Values::Record(bones) => write_record_bones(sections, *code, bones),
        };
        relaid.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::state::Document;

    fn values(state: &AppState, doc: DocumentId) -> Vec<(u32, Values)> {
        match &state.document(doc).expect("open").payload {
            DocPayload::Gta6Drawable(resource) => values_of(resource),
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_weight_and_a_value_edit_as_steps_that_undo() {
        let _serial = crate::ui::theme::serial();
        let mut state = AppState::new();
        let track = |i: u32| FilterTrack {
            track: 0x100 + i,
            weight: 0.5 + i as f32,
            group: 0x200 + i,
            word_2c: i,
        };
        let bone = BoneRecord {
            bone: 0x300,
            kind: 1,
            flags: 2,
            values: std::array::from_fn(|f| f as f32),
        };
        let filter = Entry::FrameFilter {
            name: "pack:/f.iff".into(),
            tracks: vec![track(0), track(1)],
        };
        let record = Entry::AnimationRecord {
            name: "r".into(),
            bones: vec![bone],
        };
        let dict = Dictionary {
            entries: vec![(5, Ok(filter)), (9, Ok(record))],
        };
        let doc = state.alloc_doc_id();
        let payload = DocPayload::Gta6Drawable(Arc::new(Resource::Dictionary(dict)));
        (state.documents).push(Document::new(doc, Some(PathBuf::from("f.dict")), payload));
        state.active_document = Some(doc);
        let before = values(&state, doc);

        let weight = FilterEdit::Weight {
            track: 1,
            weight: -3.0,
        };
        state.edit_gta6_filter(doc, 5, weight).expect("edited");
        let value = FilterEdit::Value {
            bone: 0,
            value: 26,
            to: 4.5,
        };
        state.edit_gta6_filter(doc, 9, value).expect("edited");
        let after = values(&state, doc);
        let mut tracks = vec![track(0), track(1)];
        tracks[1].weight = -3.0;
        let mut bones = vec![bone];
        bones[0].values[26] = 4.5;
        assert_eq!(
            after,
            vec![(5, Values::Filter(tracks)), (9, Values::Record(bones))]
        );

        // An entry of another kind, or a value it lacks, is refused and records nothing.
        let other = FilterEdit::Weight {
            track: 0,
            weight: 1.0,
        };
        assert!(state.edit_gta6_filter(doc, 9, other).is_err());
        let past = FilterEdit::Weight {
            track: 2,
            weight: 1.0,
        };
        assert!(state.edit_gta6_filter(doc, 5, past).is_err());
        assert_eq!(values(&state, doc), after);

        assert!(state.undo().is_some());
        assert!(state.undo().is_some());
        assert_eq!(values(&state, doc), before);
    }

    #[test]
    fn a_track_or_bone_removed_or_copied_is_a_step_of_its_own_that_undoes() {
        let _serial = crate::ui::theme::serial();
        let mut state = AppState::new();
        let track = |i: u32| FilterTrack {
            track: 0x100 + i,
            weight: i as f32,
            group: 0x200,
            word_2c: 0,
        };
        let bone = |i: u32| BoneRecord {
            bone: 0x300 + i,
            kind: 1,
            flags: 0,
            values: [i as f32; 27],
        };
        let dict = Dictionary {
            entries: vec![
                (
                    5,
                    Ok(Entry::FrameFilter {
                        name: "pack:/f.iff".into(),
                        tracks: vec![track(0), track(1), track(2)],
                    }),
                ),
                (
                    9,
                    Ok(Entry::AnimationRecord {
                        name: "r".into(),
                        bones: vec![bone(0), bone(1)],
                    }),
                ),
            ],
        };
        let doc = state.alloc_doc_id();
        let payload = DocPayload::Gta6Drawable(Arc::new(Resource::Dictionary(dict)));
        (state.documents).push(Document::new(doc, Some(PathBuf::from("f.dict")), payload));
        state.active_document = Some(doc);
        let before = values(&state, doc);

        for (code, edit) in [
            (5, FilterEdit::RemoveTrack { track: 0 }),
            (5, FilterEdit::RemoveTrack { track: 0 }),
            (5, FilterEdit::CopyTrack { track: 0 }),
            (9, FilterEdit::CopyBone { bone: 1 }),
            (9, FilterEdit::RemoveBone { bone: 0 }),
        ] {
            state.edit_gta6_filter(doc, code, edit).expect("edited");
        }
        assert_eq!(
            values(&state, doc),
            vec![
                (5, Values::Filter(vec![track(2), track(2)])),
                (9, Values::Record(vec![bone(1), bone(1)])),
            ]
        );
        assert!(state
            .edit_gta6_filter(doc, 5, FilterEdit::CopyTrack { track: 2 })
            .is_err());

        // Two removals in a row are two steps, each undone apart.
        for _ in 0..4 {
            assert!(state.undo().is_some());
        }
        assert_eq!(
            values(&state, doc)[0],
            (5, Values::Filter(vec![track(1), track(2)]))
        );
        assert!(state.undo().is_some());
        assert_eq!(values(&state, doc), before);
    }

    #[test]
    fn a_track_s_and_a_bone_s_fields_and_new_ones_are_steps_that_undo() {
        let _serial = crate::ui::theme::serial();
        let mut state = AppState::new();
        let track = FilterTrack {
            track: 0x100,
            weight: 0.5,
            group: 0x200,
            word_2c: 0,
        };
        let bone = BoneRecord {
            bone: 0x300,
            kind: 1,
            flags: 4,
            values: [2.0; 27],
        };
        let dict = Dictionary {
            entries: vec![
                (
                    5,
                    Ok(Entry::FrameFilter {
                        name: "pack:/f.iff".into(),
                        tracks: vec![track],
                    }),
                ),
                (
                    9,
                    Ok(Entry::AnimationRecord {
                        name: "r".into(),
                        bones: vec![bone],
                    }),
                ),
            ],
        };
        let doc = state.alloc_doc_id();
        let payload = DocPayload::Gta6Drawable(Arc::new(Resource::Dictionary(dict)));
        (state.documents).push(Document::new(doc, Some(PathBuf::from("f.dict")), payload));
        state.active_document = Some(doc);
        let before = values(&state, doc);

        let fields = FilterEdit::Track {
            track: 0,
            id: 0x101,
            group: 0x201,
            mode: 2,
        };
        state.edit_gta6_filter(doc, 5, fields).expect("edited");
        state
            .edit_gta6_filter(doc, 5, FilterEdit::AddTrack)
            .expect("edited");
        let bone_fields = FilterEdit::Bone {
            bone: 0,
            id: 0x301,
            kind: 8,
            flags: 0x204,
        };
        state.edit_gta6_filter(doc, 9, bone_fields).expect("edited");
        state
            .edit_gta6_filter(doc, 9, FilterEdit::AddBone)
            .expect("edited");
        let edited = FilterTrack {
            track: 0x101,
            group: 0x201,
            word_2c: 2,
            ..track
        };
        let added = FilterTrack {
            track: 0,
            weight: 1.0,
            group: 0x201,
            word_2c: 0,
        };
        let moved = BoneRecord {
            bone: 0x301,
            kind: 8,
            flags: 0x204,
            ..bone
        };
        let new = BoneRecord {
            bone: 0,
            values: [0.0; 27],
            ..moved
        };
        assert_eq!(
            values(&state, doc),
            vec![
                (5, Values::Filter(vec![edited, added])),
                (9, Values::Record(vec![moved, new])),
            ]
        );

        // A mode no filter holds, or a record's last bone removed, is refused and records nothing.
        let bad = FilterEdit::Track {
            track: 0,
            id: 1,
            group: 1,
            mode: 3,
        };
        assert!(state.edit_gta6_filter(doc, 5, bad).is_err());
        for _ in 0..2 {
            assert!(state.undo().is_some());
        }
        assert!(state
            .edit_gta6_filter(doc, 9, FilterEdit::RemoveBone { bone: 0 })
            .is_err());
        for _ in 0..2 {
            assert!(state.undo().is_some());
        }
        assert_eq!(values(&state, doc), before);
    }
}
