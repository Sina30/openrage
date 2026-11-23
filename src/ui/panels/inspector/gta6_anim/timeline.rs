//! The rows a GTA VI animation or group clip shows on the timeline: its clip's tags, a group clip's
//! groups and the layers each plays, the mover, camera, face and generic tracks under rows of
//! their own, and every other DOF under the bone, or the id, it names.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use or_anim::gta6::{entry_frame_times, group_starts, layers_at, Binding, Skipped};
use or_io::gta6::anim::{AnimAt, AnimResource, Animation, Clip, ClipEntry, ClipGroup, ClipPlay};
use or_io::gta6::headerless::Skeleton;
use or_transport::TimeRange;

use super::play::Choice;
use crate::ui::panels::timeline_ycd::{ROTATION, SCALE, TAG_SPAN, TRANSLATION};
use crate::ui::theme;
use crate::ui::timeline::{
    ChannelRow, KeyEvent, KeyShape, RowBadge, RowEvents, RowId, RowKind, SpanEvent, TimelineSource,
};

const KIND_TAGS: u8 = 1;
pub(super) const KIND_ID: u8 = 2;
pub(super) const KIND_DOF: u8 = 3;
const KIND_FAMILY: u8 = 4;
const KIND_GROUP: u8 = 5;
const KIND_LAYER: u8 = 6;
const KIND_WAITING: u8 = 7;

/// The first words of the track names gathered under a row of their own, and that row's family.
const FAMILIES: [(&str, u8); 5] = [
    ("mover", 0),
    ("camera", 1),
    ("firstPersonCamera", 1),
    ("facial", 2),
    ("generic", 3),
];

/// An animation as it plays on the timeline.
pub(super) struct Layer<'a> {
    pub animation: &'a Animation,
    /// Timeline seconds at which its frames play.
    pub frame_times: Vec<f32>,
    /// Seconds into the animation at the playhead; none while its group does not play.
    pub time: Option<f32>,
    /// Seconds into the animation at which its window starts, where its root motion is zero.
    pub start: f32,
    /// A group clip's layer: its group and its rate.
    pub group: Option<(usize, f32)>,
}

/// What a choice plays: one animation, a clip's window of one, or a group clip's groups one after
/// another.
pub(super) struct Playing<'a> {
    pub layers: Vec<Layer<'a>>,
    pub groups: Cow<'a, [ClipGroup]>,
    /// One animation, alone or as a clip of one, shown with no group rows.
    pub one: bool,
    /// Each entry whose set no open body stores: its group and its animation's key.
    pub waiting: Vec<(usize, u32)>,
    pub duration: f32,
}

/// The seconds a window from `start` to `end` lasts at `rate`; none for a rate that does not advance.
pub(super) fn window(start: f32, end: f32, rate: f32) -> Option<f32> {
    (rate > 0.0 && rate.is_finite() && end >= start).then(|| (end - start) / rate)
}

/// The keys of the sets `resource` names that none of `external` is.
pub(super) fn waiting_sets(resource: &AnimResource, external: &[Arc<AnimResource>]) -> Vec<u32> {
    (resource.external_sets.iter().copied())
        .filter(|key| !external.iter().any(|e| e.key == *key))
        .collect()
}

/// `keys` as `hash_XXXXXXXX`, joined.
pub(super) fn keys(keys: &[u32]) -> String {
    let shown: Vec<String> = keys.iter().map(|k| format!("hash_{k:08X}")).collect();
    shown.join(", ")
}

impl<'a> Playing<'a> {
    /// What `choice` plays at `time`, the playhead's; an entry of a set another body stores plays
    /// from the first of `external` whose set holds its key.
    pub(super) fn new(
        resource: &'a AnimResource,
        choice: Choice,
        external: &'a [Arc<AnimResource>],
        time: f32,
    ) -> Self {
        let animation = |at: AnimAt| {
            resource
                .sets
                .get(at.set)
                .and_then(|s| s.animations.get(at.animation))
        };
        let outside = |key: u32| {
            (external.iter())
                .flat_map(|r| r.sets.iter().flat_map(|s| &s.animations))
                .find(|a| a.key == key)
        };
        let (groups, one) = match choice {
            Choice::Animation(at) => {
                let layers = animation(at).map(|animation| {
                    let last = f32::from(animation.frames.saturating_sub(1)).max(1.0);
                    let duration = animation.duration.max(1e-3);
                    Layer {
                        animation,
                        frame_times: (0..animation.frames)
                            .map(|f| f32::from(f) / last * duration)
                            .collect(),
                        time: Some(time.clamp(0.0, duration)),
                        start: 0.0,
                        group: None,
                    }
                });
                let duration = layers.as_ref().map_or(0.0, |l| l.animation.duration);
                return Self {
                    layers: layers.into_iter().collect(),
                    groups: Cow::Borrowed(&[]),
                    one: true,
                    waiting: Vec::new(),
                    duration: duration.max(1e-3),
                };
            }
            Choice::Clip(c) => match resource.clips.get(c).map(|c| &c.play) {
                // A clip of one animation plays its window at its rate, as one group of one entry.
                Some(&ClipPlay::One {
                    animation: at,
                    key,
                    start,
                    end,
                    rate,
                }) => {
                    let duration = window(start, end, rate)
                        .or_else(|| animation(at).map(|a| a.duration))
                        .unwrap_or(0.0);
                    let group = ClipGroup {
                        duration,
                        entries: vec![ClipEntry {
                            animation: Some(at),
                            key,
                            start,
                            end,
                            rate,
                            ..ClipEntry::default()
                        }],
                    };
                    (Cow::Owned(vec![group]), true)
                }
                Some(ClipPlay::Groups { groups, .. }) => (Cow::Borrowed(groups.as_slice()), false),
                Some(ClipPlay::Unresolved) | None => (Cow::Borrowed(&[][..]), false),
            },
        };
        let now = layers_at(&groups, time);
        let starts = group_starts(&groups);
        let mut layers = Vec::new();
        let mut waiting = Vec::new();
        for (g, group) in groups.iter().enumerate() {
            for (e, entry) in group.entries.iter().enumerate() {
                let found = match entry.animation {
                    Some(at) => animation(at),
                    None => outside(entry.key),
                };
                let Some(animation) = found else {
                    waiting.push((g, entry.key));
                    continue;
                };
                layers.push(Layer {
                    animation,
                    frame_times: entry_frame_times(starts[g], entry, animation),
                    time: (now.iter())
                        .find(|l| l.group == g && l.entry == e)
                        .map(|l| l.time),
                    start: entry.start,
                    group: Some((g, entry.rate)),
                });
            }
        }
        let duration = groups.iter().map(|g| g.duration).sum::<f32>().max(1e-3);
        Self {
            layers,
            groups,
            one,
            waiting,
            duration,
        }
    }
}

enum Target {
    Tags,
    Group(usize),
    /// The frames of one layer.
    Keys(usize),
    Nothing,
}

pub(super) struct Gta6Timeline {
    rows: Vec<ChannelRow>,
    targets: Vec<Target>,
    frame_times: Vec<Vec<f32>>,
    tag_spans: Vec<SpanEvent>,
    group_spans: Vec<SpanEvent>,
    duration: f32,
    rate: Option<f32>,
    summary: String,
    unavailable: Option<String>,
}

impl Gta6Timeline {
    /// `model` is the skeleton picked and each played animation's binding to it, by its key;
    /// `why_not_posed` says why playing poses nothing, when it does not; `waiting` are the keys
    /// of the sets no open body stores.
    pub(super) fn new(
        clip: Option<&Clip>,
        playing: &Playing,
        model: Option<(&Skeleton, &[(u32, Binding)])>,
        why_not_posed: Option<String>,
        waiting: &[u32],
        expanded: &dyn Fn(RowId) -> bool,
    ) -> Self {
        let duration = playing.duration;
        let tag_spans: Vec<SpanEvent> = clip
            .into_iter()
            .flat_map(|c| &c.tags)
            .flat_map(|tag| {
                let [start, end] = tag.phases.unwrap_or([0.0; 2]);
                // A tag whose start passes its end wraps over the loop point: two spans.
                let parts = match (tag.phases, start <= end) {
                    (None, _) => vec![],
                    (Some(_), true) => vec![(start, end)],
                    (Some(_), false) => vec![(start, 1.0), (0.0, end)],
                };
                parts.into_iter().map(move |(a, b)| SpanEvent {
                    range: TimeRange::new(a * duration, b * duration),
                    colour: TAG_SPAN,
                    label: Some(super::named(tag.name)),
                })
            })
            .collect();

        let mut rows = Vec::new();
        let mut targets = Vec::new();
        if !tag_spans.is_empty() {
            rows.push(
                ChannelRow::group(
                    RowId::new(KIND_TAGS, 0, 0),
                    tr!(
                        "Tags ({count})",
                        count = clip.map_or(0, |c| c
                            .tags
                            .iter()
                            .filter(|t| t.phases.is_some())
                            .count())
                    ),
                )
                .with_colour(TAG_SPAN),
            );
            targets.push(Target::Tags);
        }
        let starts = group_starts(&playing.groups);
        let group_spans: Vec<SpanEvent> = (playing.groups.iter().zip(&starts))
            .enumerate()
            .filter(|_| !playing.one)
            .map(|(g, (group, start))| SpanEvent {
                range: TimeRange::new(*start, start + group.duration),
                colour: theme::TEXT_SECONDARY.get(),
                label: Some(tr!("Group {group}", group = g)),
            })
            .collect();
        let waits = RowBadge {
            text: tr!("waits").to_string(),
            colour: theme::WARNING.get(),
            tooltip: tr!(
                "This layer's animation is in one of the sets {sets}, which another body stores. Open that body from the scan of its archive and the layer plays.",
                sets = keys(waiting)
            ),
        };
        for (g, span) in group_spans.iter().enumerate() {
            let row_id = RowId::new(KIND_GROUP, g as u32, 0);
            let open = expanded(row_id);
            rows.push(
                ChannelRow::group(row_id, span.label.clone().unwrap_or_default())
                    .with_colour(theme::TEXT_PRIMARY.get())
                    .expandable(true, open),
            );
            targets.push(Target::Group(g));
            if !open {
                continue;
            }
            for (l, layer) in playing.layers.iter().enumerate() {
                let Some((_, rate)) = layer.group.filter(|(lg, _)| *lg == g) else {
                    continue;
                };
                let fps = layer.animation.rate().unwrap_or(0.0);
                // The rate first: a narrow label column cuts its end, and the rate tells layers apart.
                rows.push(
                    ChannelRow::group(
                        RowId::new(KIND_LAYER, g as u32, l as u16),
                        tr!(
                            "rate {rate} · {animation} · {fps} fps",
                            animation = &layer.animation.name,
                            fps = format!("{fps:.0}"),
                            rate = rate,
                        ),
                    )
                    .with_depth(1)
                    .with_kind(RowKind::Track),
                );
                targets.push(Target::Keys(l));
            }
            for (w, &(_, key)) in playing
                .waiting
                .iter()
                .enumerate()
                .filter(|(_, (wg, _))| *wg == g)
            {
                rows.push(
                    ChannelRow::group(
                        RowId::new(KIND_WAITING, g as u32, w as u16),
                        format!("hash_{key:08X}"),
                    )
                    .with_depth(1)
                    .with_kind(RowKind::Track)
                    .with_badge(Some(waits.clone())),
                );
                targets.push(Target::Nothing);
            }
        }

        // ⚠ Looked up once each, not searched: an animation may carry thousands of DOFs.
        let mut why_skipped: HashMap<(usize, usize), &str> = HashMap::new();
        let mut bone_names: HashMap<u32, &str> = HashMap::new();
        if let Some((skeleton, bindings)) = model {
            for (l, layer) in playing.layers.iter().enumerate() {
                let binding = bindings.iter().find(|(key, _)| *key == layer.animation.key);
                for s in binding.map_or(&[][..], |(_, b)| &b.skipped) {
                    let (dof, why) = match *s {
                        Skipped::NoSuchBone { dof, .. } => (dof, tr!("The model has no bone whose name hashes to this DOF's id, so it poses nothing.")),
                        Skipped::NotABoneTrack { dof, .. } => (dof, tr!("This track is no bone's rotation, translation or scale, the only tracks laid out, so it poses nothing.")),
                    };
                    why_skipped.entry((l, dof)).or_insert(why);
                }
            }
            for bone in &skeleton.bones {
                bone_names.entry(bone.hash).or_insert(&bone.name);
            }
        }
        let skipped = |key: (usize, usize)| why_skipped.get(&key).map(|why| why.to_string());
        let badge = |why: String| RowBadge {
            text: tr!("not posed").to_string(),
            colour: theme::WARNING.get(),
            tooltip: why,
        };
        let id_name = |id: u32| {
            (bone_names.get(&id)).map_or_else(|| format!("hash_{id:08X}"), |name| name.to_string())
        };
        // Families first, then every other id in rising order; a group clip's layers drive
        // disjoint DOFs, so one id's DOFs may come from several layers.
        let mut by_row: BTreeMap<(u8, u32), Vec<(usize, usize)>> = BTreeMap::new();
        for (l, layer) in playing.layers.iter().enumerate() {
            for (d, dof) in layer.animation.dofs.iter().enumerate() {
                let name = dof.track_name().unwrap_or("");
                let family = FAMILIES.iter().find(|(word, _)| name.starts_with(word));
                let key = family.map_or((u8::MAX, dof.id), |(_, f)| (*f, 0));
                by_row.entry(key).or_default().push((l, d));
            }
        }
        for ((family, id), dofs) in by_row {
            let in_family = family != u8::MAX;
            let (row_id, name) = match family {
                0 => (RowId::new(KIND_FAMILY, 0, 0), tr!("Mover").to_string()),
                1 => (RowId::new(KIND_FAMILY, 1, 0), tr!("Camera").to_string()),
                2 => (RowId::new(KIND_FAMILY, 2, 0), tr!("Face").to_string()),
                3 => (RowId::new(KIND_FAMILY, 3, 0), tr!("Generic").to_string()),
                _ => (RowId::new(KIND_ID, id, 0), id_name(id)),
            };
            let open = expanded(row_id);
            let all_skipped = dofs.iter().all(|key| skipped(*key).is_some());
            rows.push(
                ChannelRow::group(row_id, name)
                    .with_colour(theme::TEXT_PRIMARY.get())
                    .expandable(true, open)
                    .with_badge(all_skipped.then(|| skipped(dofs[0])).flatten().map(badge)),
            );
            targets.push(Target::Keys(dofs[0].0));
            if !open {
                continue;
            }
            for (l, d) in dofs {
                let layer = &playing.layers[l];
                let dof = &layer.animation.dofs[d];
                let mut label = dof.track_label();
                if in_family {
                    label = format!("{label} · {}", id_name(dof.id));
                }
                let value = layer.time.and_then(|t| layer.animation.sample(d, t).ok());
                if let Some(value) = value {
                    let shown: Vec<String> =
                        value.as_slice().iter().map(|v| format!("{v:.3}")).collect();
                    label = format!("{label} = {}", shown.join(", "));
                }
                let track = dof.track_name().unwrap_or("");
                let colour = if track.ends_with("Translation") {
                    TRANSLATION
                } else if track.ends_with("Rotation") {
                    ROTATION
                } else if track.ends_with("Scale") {
                    SCALE
                } else {
                    egui::Color32::GRAY
                };
                rows.push(
                    ChannelRow::group(RowId::new(KIND_DOF, dof.id, d as u16), label)
                        .with_depth(1)
                        .with_kind(RowKind::Track)
                        .with_colour(colour)
                        .with_badge(skipped((l, d)).map(badge)),
                );
                targets.push(Target::Keys(l));
            }
        }
        let dofs: usize = playing.layers.iter().map(|l| l.animation.dofs.len()).sum();
        let summary = match playing.layers.as_slice() {
            [layer] if playing.one => tr!(
                "{dofs} DOFs · {frames} frames · {blocks} blocks",
                dofs = dofs,
                frames = layer.animation.frames,
                blocks = layer.animation.blocks.len(),
            ),
            _ => tr_n!(
                "{count} group played one after another · {layers} layers · {dofs} DOFs",
                "{count} groups played one after another · {layers} layers · {dofs} DOFs",
                playing.groups.len(),
                layers = playing.layers.len(),
                dofs = dofs,
            ),
        };
        // A layer's frames per second of the timeline: its animation's rate times its own.
        let rate = (playing.layers.iter())
            .filter_map(|l| Some(l.animation.rate()? * l.group.map_or(1.0, |(_, r)| r)))
            .filter(|r| r.is_finite() && *r > 0.0)
            .reduce(f32::max);
        Self {
            rows,
            targets,
            frame_times: playing
                .layers
                .iter()
                .map(|l| l.frame_times.clone())
                .collect(),
            tag_spans,
            group_spans,
            duration,
            rate,
            summary,
            unavailable: why_not_posed,
        }
    }

    fn frame_keys(&self, layer: usize, view: TimeRange, out: &mut RowEvents) {
        for &time in self.frame_times.get(layer).into_iter().flatten() {
            if view.contains(time) {
                out.keys.push(KeyEvent {
                    time,
                    shape: KeyShape::Diamond,
                    colour: theme::TEXT_SECONDARY.get(),
                    selected: false,
                });
            }
        }
        out.total_keys = out.keys.len();
    }
}

impl TimelineSource for Gta6Timeline {
    fn bounds(&self) -> TimeRange {
        TimeRange::from_duration(self.duration)
    }

    fn rows(&self) -> &[ChannelRow] {
        &self.rows
    }

    fn row_events(&self, row: usize, view: TimeRange, out: &mut RowEvents) {
        out.clear();
        let overlaps = |s: &&SpanEvent| s.range.end >= view.start && s.range.start <= view.end;
        match self.targets.get(row) {
            Some(Target::Tags) => out
                .spans
                .extend(self.tag_spans.iter().filter(overlaps).cloned()),
            Some(Target::Group(g)) => out
                .spans
                .extend(self.group_spans.get(*g).filter(overlaps).cloned()),
            Some(Target::Keys(layer)) => self.frame_keys(*layer, view, out),
            Some(Target::Nothing) | None => {}
        }
    }

    fn read_only_reason(&self) -> Option<&str> {
        Some(tr!(
            "a key of a GTA VI animation is not dragged on the timeline: Insert key and Clear key set the selected tracks at the playhead, and Edit, in the Inspector, sets any key or a clip's window."
        ))
    }

    fn playback_unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    fn frame_rate(&self) -> Option<f32> {
        self.rate
    }

    fn summary(&self) -> String {
        self.summary.clone()
    }
}

#[cfg(test)]
mod tests {
    use or_io::gta6::anim::{AnimSet, Dof, RootKind};
    use or_io::gta6::hash::name_hash32;

    use super::*;

    fn hash(name: &str) -> u32 {
        name_hash32(name.as_bytes())
    }

    fn animation(name: &str, frames: u16, fps: f32, tracks: &[(u32, &str)]) -> Animation {
        Animation {
            name: name.to_string(),
            key: hash(name),
            frames,
            frames_per_chunk: 255,
            duration: f32::from(frames - 1) / fps,
            dofs: (tracks.iter())
                .map(|(id, track)| Dof {
                    id: *id,
                    track: hash(track),
                    kind: 4,
                })
                .collect(),
            ..Animation::default()
        }
    }

    fn resource(
        root: RootKind,
        key: u32,
        animations: Vec<Animation>,
        clips: Vec<Clip>,
    ) -> AnimResource {
        AnimResource {
            root,
            root_word: 0,
            name: String::new(),
            key,
            block_map: None,
            word_at_60: 0,
            word_at_98: None,
            sets: vec![AnimSet {
                name: String::new(),
                key,
                animations,
            }],
            external_sets: Vec::new(),
            animation_keys: Vec::new(),
            clips,
            motions: Vec::new(),
            matchers: Vec::new(),
            dictionaries: Vec::new(),
            end_region: None,
            sections: None,
            edited: false,
        }
    }

    fn clip(play: ClipPlay) -> Clip {
        Clip {
            name: "pack:/a.clip".into(),
            key: 0,
            play,
            properties: Vec::new(),
            tags: Vec::new(),
        }
    }

    fn at(animation: usize) -> AnimAt {
        AnimAt { set: 0, animation }
    }

    fn labels(timeline: &Gta6Timeline) -> Vec<(u8, String)> {
        let rows = timeline.rows().iter();
        rows.map(|r| (r.depth, r.label.clone())).collect()
    }

    fn timeline_of(playing: &Playing, waiting: &[u32]) -> Gta6Timeline {
        Gta6Timeline::new(None, playing, None, None, waiting, &|_| true)
    }

    #[test]
    fn mover_camera_face_and_generic_tracks_have_rows_of_their_own_and_an_unknown_word_keeps_its_word(
    ) {
        let walk = animation(
            "walk",
            2,
            30.0,
            &[
                (7, "notATrackName"),
                (0, "moverAbsoluteTranslation"),
                (5, "facialTranslation"),
                (0, "cameraFieldOfView"),
                (9, "genericControl"),
                (0, "firstPersonCameraWeight"),
            ],
        );
        let resource = resource(RootKind::AnimationSet, 0, vec![walk], Vec::new());
        let playing = Playing::new(&resource, Choice::Animation(at(0)), &[], 0.0);
        let timeline = timeline_of(&playing, &[]);
        let unknown = format!("hash_{:08X}", hash("notATrackName"));
        let row = |depth: u8, label: &str| (depth, label.to_string());
        assert_eq!(
            labels(&timeline),
            [
                row(0, "Mover"),
                row(1, "moverAbsoluteTranslation · hash_00000000"),
                row(0, "Camera"),
                row(1, "cameraFieldOfView · hash_00000000"),
                row(1, "firstPersonCameraWeight · hash_00000000"),
                row(0, "Face"),
                row(1, "facialTranslation · hash_00000005"),
                row(0, "Generic"),
                row(1, "genericControl · hash_00000009"),
                row(0, "hash_00000007"),
                row(1, &unknown),
            ]
        );
    }

    fn entry(animation: Option<usize>, key: u32, start: f32, end: f32, rate: f32) -> ClipEntry {
        ClipEntry {
            animation: animation.map(at),
            key,
            start,
            end,
            rate,
            ..ClipEntry::default()
        }
    }

    #[test]
    fn a_group_clip_shows_its_groups_in_sequence_and_the_layers_each_plays_at_its_rate() {
        let fast = animation("fast", 46, 30.0, &[(1, "boneRotation")]);
        let slow = animation("slow", 13, 15.0, &[(2, "boneTranslation")]);
        let groups = vec![
            ClipGroup {
                duration: 0.5,
                entries: vec![entry(Some(0), 0, 0.0, 0.5, 1.0)],
            },
            ClipGroup {
                duration: 1.2,
                entries: vec![
                    entry(Some(0), 0, 0.3, 1.5, 1.0),
                    entry(Some(1), 0, 0.1, 0.7, 0.5),
                ],
            },
        ];
        let play = ClipPlay::Groups { total: 1.7, groups };
        let resource = resource(
            RootKind::ClipDictionary,
            0,
            vec![fast, slow],
            vec![clip(play)],
        );
        let playing = Playing::new(&resource, Choice::Clip(0), &[], 1.0);
        assert!((playing.duration - 1.7).abs() < 1e-6);
        // At 1.0 s the second group plays, 0.5 s into its span.
        let times: Vec<Option<f32>> = playing.layers.iter().map(|l| l.time).collect();
        assert_eq!(times[0], None);
        assert!((times[1].unwrap() - 0.8).abs() < 1e-6);
        assert!((times[2].unwrap() - 0.35).abs() < 1e-6);

        let timeline = timeline_of(&playing, &[]);
        let shown = labels(&timeline);
        let row = |depth: u8, label: &str| (depth, label.to_string());
        assert_eq!(
            shown[..5],
            [
                row(0, "Group 0"),
                row(1, "rate 1 · fast · 30 fps"),
                row(0, "Group 1"),
                row(1, "rate 1 · fast · 30 fps"),
                row(1, "rate 0.5 · slow · 15 fps"),
            ]
        );
        let mut events = RowEvents::default();
        timeline.row_events(2, TimeRange::new(0.0, 2.0), &mut events);
        let span = events.spans[0].range;
        assert!((span.start - 0.5).abs() < 1e-6 && (span.end - 1.7).abs() < 1e-6);
        // The slow layer's frames 2 to 10 lie in its window, two clip seconds per animation second.
        timeline.row_events(4, TimeRange::new(0.0, 2.0), &mut events);
        assert_eq!(events.keys.len(), 9);
        assert!((events.keys[0].time - (0.5 + (2.0 / 15.0 - 0.1) / 0.5)).abs() < 1e-5);
        assert_eq!(timeline.frame_rate(), Some(30.0));
    }

    #[test]
    fn a_clip_of_one_animation_plays_its_window_at_its_rate() {
        let walk = animation("walk", 31, 32.0, &[(1, "boneRotation")]);
        let play = ClipPlay::One {
            animation: at(0),
            key: hash("walk"),
            start: 0.25,
            end: 0.75,
            rate: 2.0,
        };
        let resource = resource(RootKind::ClipDictionary, 0, vec![walk], vec![clip(play)]);
        let playing = Playing::new(&resource, Choice::Clip(0), &[], 0.125);
        assert!(
            (playing.duration - 0.25).abs() < 1e-6,
            "{}",
            playing.duration
        );
        let layer = &playing.layers[0];
        assert!((layer.time.unwrap() - 0.5).abs() < 1e-6);
        assert!((layer.start - 0.25).abs() < 1e-6);
        // Frames 8 to 24 of 32 fps lie in the window, each half its animation time on the clip's.
        assert_eq!(layer.frame_times.len(), 17);
        assert!((layer.frame_times[0] - 0.0).abs() < 1e-5);
        assert!((layer.frame_times[16] - 0.25).abs() < 1e-5);
        let timeline = timeline_of(&playing, &[]);
        assert!(!labels(&timeline)
            .iter()
            .any(|(_, l)| l.starts_with("Group")));
        assert_eq!(timeline.frame_rate(), Some(64.0));
        // Its keys are edited under the inspector's Edit, which saves them, so it never says they are not written.
        let why = timeline
            .read_only_reason()
            .expect("the timeline moves no key");
        assert!(
            why.contains("Edit") && !why.contains("does not write"),
            "{why}"
        );
    }

    #[test]
    fn an_entry_of_a_set_another_body_stores_plays_once_that_body_is_open_and_waits_by_key_until() {
        let local = animation("local", 31, 30.0, &[(1, "boneRotation")]);
        let far = animation("far", 16, 15.0, &[(2, "boneTranslation")]);
        let groups = vec![ClipGroup {
            duration: 1.0,
            entries: vec![
                entry(Some(0), hash("local"), 0.0, 1.0, 1.0),
                entry(None, hash("far"), 0.0, 1.0, 1.0),
            ],
        }];
        let mut resource = resource(
            RootKind::ClipDictionary,
            hash("dict"),
            vec![local],
            vec![clip(ClipPlay::Groups { total: 1.0, groups })],
        );
        resource.external_sets = vec![hash("dict-1")];

        let playing = Playing::new(&resource, Choice::Clip(0), &[], 0.5);
        assert_eq!(playing.layers.len(), 1);
        assert_eq!(playing.waiting, [(0, hash("far"))]);
        let waiting = waiting_sets(&resource, &[]);
        assert_eq!(waiting, [hash("dict-1")]);
        let timeline = timeline_of(&playing, &waiting);
        let row = (timeline.rows().iter()).find(|r| r.label == format!("hash_{:08X}", hash("far")));
        let badge = row.and_then(|r| r.badge.as_ref()).expect("a waiting row");
        assert!(
            badge
                .tooltip
                .contains(&format!("hash_{:08X}", hash("dict-1"))),
            "{}",
            badge.tooltip
        );

        let set = Arc::new(resource_set(far));
        let external = [set];
        let playing = Playing::new(&resource, Choice::Clip(0), &external, 0.5);
        let names: Vec<&str> = playing
            .layers
            .iter()
            .map(|l| l.animation.name.as_str())
            .collect();
        assert_eq!(names, ["local", "far"]);
        assert!(playing.waiting.is_empty());
        assert!(waiting_sets(&resource, &external).is_empty());
    }

    fn resource_set(animation: Animation) -> AnimResource {
        resource(
            RootKind::AnimationSet,
            hash("dict-1"),
            vec![animation],
            Vec::new(),
        )
    }
}
