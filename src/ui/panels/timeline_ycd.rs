use std::collections::BTreeMap;

use egui::Color32;
use or_anim::clip::track_name;
use or_anim::Skeleton;
use or_io::codec::anim::track;
use or_io::codec::anim::ChannelFormat;
use or_io::codec::anim::ChannelWalk;
use or_io::gta5::{CrAnimation, CrClipAny, CrClipDictionary};
use or_transport::TimeRange;

use crate::ui::panel_state::PanelState;
use crate::ui::theme;
use crate::ui::timeline;
use crate::ui::timeline::{
    ChannelRow, KeyEvent, KeyShape, RowBadge, RowEvents, RowId, RowKind, SpanEvent, TimelineSource,
};

#[derive(Default)]
pub struct AnimTimelineState(pub timeline::TimelineState);

impl PanelState for AnimTimelineState {
    const KEY: &'static str = "timeline-ycd";
}

pub fn draw(ui: &mut egui::Ui, state: &mut crate::state::AppState) {
    use crate::state::DocPayload;

    let Some(doc_id) = state.active_document else {
        timeline::draw_placeholder(ui, "Open a clip dictionary (.ycd) to see its timeline");
        return;
    };
    if !matches!(
        state.document(doc_id).map(|d| &d.payload),
        Some(DocPayload::Resource(or_io::Asset::Clips(_)))
    ) {
        timeline::draw_placeholder(ui, "Timeline available when a clip dictionary is active");
        return;
    }

    let source = {
        let Some(binding) = state.anim.binding.as_ref() else {
            let message = match state.preview_scene.clip {
                None => format!(
                    "Select a clip in {} to play it",
                    state
                        .document(doc_id)
                        .map(|d| d.file_name())
                        .unwrap_or_default()
                ),
                Some(_) => "Pick a skeleton in the Preview tab to play this clip".to_string(),
            };
            timeline::draw_placeholder(ui, &message);
            return;
        };
        let clip_selection = state.preview_scene.clip;
        let dictionary = clip_selection
            .and_then(|clip| state.document(clip.document))
            .and_then(|doc| match &doc.payload {
                DocPayload::Resource(or_io::Asset::Clips(dictionary)) => Some(dictionary),
                _ => None,
            });
        let found = clip_selection
            .zip(dictionary)
            .and_then(|(selection, dictionary)| {
                Some((
                    dictionary,
                    or_anim::clip::clip(dictionary, selection.clip_key)?,
                ))
            });
        let Some((dictionary, clip)) = found else {
            timeline::draw_placeholder(ui, "The selected clip is no longer in the dictionary");
            return;
        };

        let expanded_rows = state
            .panels
            .peek::<AnimTimelineState>()
            .map(|s| s.0.rows.clone())
            .unwrap_or_default();
        YcdTimelineSource::new(dictionary, clip, &binding.skeleton, &|id| {
            expanded_rows.is_expanded(id)
        })
    };

    let clip_subject = state
        .preview_scene
        .clip
        .map(|c| (c.document << 32) ^ u64::from(c.clip_key));

    let mut tl = state.panels.take::<AnimTimelineState>();
    if let Some(selection) = clip_subject {
        tl.0.ensure_subject(selection);
    }
    let mut transport = state.anim_transport;
    let output = timeline::draw(ui, &source, &mut transport, &mut tl.0);
    state.panels.put(tl);
    state.anim_transport = transport;

    if let Some(time) = output.seek_to {
        state.anim_transport.time = state.anim_transport.range.clamp(time);
        state.anim_transport.seek_pending = None;
    }

    draw_key_controls(ui, state, &source);
}

fn insert_key(
    state: &mut crate::state::AppState,
    bone: u16,
    track: or_anim::EditTrack,
    time: f32,
) -> bool {
    let Some(frame) = state.anim.frame_at(time) else {
        return false;
    };
    let Some(local) = state.anim.pose.local(bone).copied() else {
        return false;
    };
    let value = match track {
        or_anim::EditTrack::Translation => or_anim::EditValue::Vector(local.translation),
        or_anim::EditTrack::Scale => or_anim::EditValue::Vector(local.scale),
        or_anim::EditTrack::Rotation => or_anim::EditValue::Rotation(local.rotation),
    };
    state
        .anim_edits
        .set(or_anim::EditKey { bone, track, frame }, value)
}

fn draw_key_controls(
    ui: &mut egui::Ui,
    state: &mut crate::state::AppState,
    source: &YcdTimelineSource,
) {
    let time = state.anim_transport.time;
    let Some(frame) = state.anim.frame_at(time) else {
        return;
    };
    let selected: Vec<(u16, or_anim::EditTrack)> = state
        .panels
        .get::<AnimTimelineState>()
        .0
        .selected_rows
        .iter()
        .filter_map(|row| source.bone_track_of(*row))
        .collect();

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("frame {frame}"))
                .small()
                .monospace()
                .color(theme::TEXT_SECONDARY),
        );

        ui.add_enabled_ui(!selected.is_empty(), |ui| {
            if ui
                .small_button("Key")
                .on_hover_text(
                    "Insert a key at the playhead on the selected rows, holding their \
                     current posed value",
                )
                .clicked()
            {
                let before = state.anim_edits.clone();
                let mut applied = false;
                for (bone, track) in &selected {
                    applied |= insert_key(state, *bone, *track, time);
                }
                if applied {
                    state.push_anim_edit("insert key", before);
                    state.anim.invalidate();
                }
            }
            if ui
                .small_button("Clear key")
                .on_hover_text("Remove the key at the playhead on the selected rows")
                .clicked()
            {
                let before = state.anim_edits.clone();
                let mut applied = false;
                for (bone, track) in &selected {
                    applied |= state.anim_edits.remove(&or_anim::EditKey {
                        bone: *bone,
                        track: *track,
                        frame,
                    });
                }
                if applied {
                    state.push_anim_edit("clear key", before);
                    state.anim.invalidate();
                }
            }
        });

        if selected.is_empty() {
            ui.label(
                egui::RichText::new("select a bone track to key it")
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
}

const KIND_BONE: u8 = 1;
const KIND_TRACK: u8 = 2;
const KIND_TAG: u8 = 3;

pub(crate) const TRANSLATION: Color32 = Color32::from_rgb(0xE0, 0x6C, 0x6C);
pub(crate) const ROTATION: Color32 = Color32::from_rgb(0x6C, 0xC8, 0x8A);
pub(crate) const SCALE: Color32 = Color32::from_rgb(0x72, 0x9E, 0xE0);
pub(crate) const TAG_SPAN: Color32 = Color32::from_rgb(0xC8, 0xA0, 0x50);

#[derive(Debug, Clone)]
struct TrackRow {
    bone: u16,
    track_id: u8,
    dof: u32,
    format: Option<ChannelFormat>,
    undecodable: bool,
}

#[derive(Debug, Clone)]
enum RowTarget {
    Bone { tracks: Vec<usize> },
    Track(usize),
    Tags,
}

pub struct YcdTimelineSource {
    rows: Vec<ChannelRow>,
    targets: Vec<RowTarget>,
    tracks: Vec<TrackRow>,
    frame_times: Vec<f32>,
    tag_spans: Vec<SpanEvent>,
    duration: f32,
    frame_rate: Option<f32>,
    summary: String,
    read_only_reason: Option<String>,
}

impl YcdTimelineSource {
    pub fn new(
        dictionary: &CrClipDictionary,
        clip: &CrClipAny,
        skeleton: &Skeleton,
        expanded: &dyn Fn(RowId) -> bool,
    ) -> Self {
        let animation = or_anim::clip::primary(dictionary, clip);
        let duration =
            or_anim::clip::duration(clip).max(animation.map(|a| a.duration).unwrap_or(0.0));

        let (tracks, frame_times, frame_rate) = match animation {
            Some(animation) => (
                collect_tracks(animation, skeleton),
                frame_times_of(animation, duration),
                or_anim::player::frame_rate_of(animation),
            ),
            None => (Vec::new(), Vec::new(), None),
        };

        let mut by_bone: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
        for (index, track) in tracks.iter().enumerate() {
            by_bone.entry(track.bone).or_default().push(index);
        }

        let mut rows = Vec::new();
        let mut targets = Vec::new();

        let tag_spans = collect_tag_spans(clip, duration);
        if !tag_spans.is_empty() {
            rows.push(
                ChannelRow::group(
                    RowId::new(KIND_TAG, 0, 0),
                    format!("Tags ({})", tag_spans.len()),
                )
                .with_colour(TAG_SPAN),
            );
            targets.push(RowTarget::Tags);
        }

        for (bone, track_indices) in by_bone {
            let name = skeleton
                .bone(bone)
                .map(|b| b.name.clone())
                .unwrap_or_else(|| format!("bone {bone}"));
            let bone_row_id = RowId::new(KIND_BONE, bone as u32, 0);
            let depth = skeleton.depth_of(bone).min(6) as u8;
            let is_expanded = expanded(bone_row_id);

            rows.push(
                ChannelRow::group(bone_row_id, name)
                    .with_depth(depth)
                    .with_colour(theme::TEXT_PRIMARY.get())
                    .expandable(true, is_expanded)
                    .with_badge(bone_badge(&tracks, &track_indices)),
            );
            targets.push(RowTarget::Bone {
                tracks: track_indices.clone(),
            });

            if !is_expanded {
                continue;
            }
            for index in track_indices {
                let track = &tracks[index];
                rows.push(
                    ChannelRow::group(
                        RowId::new(KIND_TRACK, bone as u32, index as u16),
                        track_name(track.track_id),
                    )
                    .with_depth(depth + 1)
                    .with_kind(RowKind::Track)
                    .with_colour(track_colour(track.track_id))
                    .with_badge(format_badge(track)),
                );
                targets.push(RowTarget::Track(index));
            }
        }

        let compact = animation.is_some_and(or_anim::clip::is_compact);
        let summary = match animation {
            Some(animation) => format!(
                "{} bones · {} DOFs · {} frames · {} blocks{}",
                by_bone_count(&tracks),
                animation.dofs.len(),
                animation.num_frames,
                animation.blocks.len(),
                if compact { " · compact" } else { "" }
            ),
            None => "no animation".to_string(),
        };

        // Stated up front: a view that looks editable and silently refuses
        // changes leaves no way to find out why.
        let read_only_reason = if animation.is_none() {
            Some("this clip references no animation".to_string())
        } else if compact {
            Some(
                "compact (compressed) blocks cannot be decoded, so their keys cannot be edited"
                    .to_string(),
            )
        } else {
            None
        };

        Self {
            rows,
            targets,
            tracks,
            frame_times,
            tag_spans,
            duration,
            frame_rate,
            summary,
            read_only_reason,
        }
    }

    pub fn bone_track_of(&self, row: RowId) -> Option<(u16, or_anim::EditTrack)> {
        match row.kind() {
            KIND_TRACK => {
                let track = self.tracks.get(row.secondary() as usize)?;
                Some((
                    track.bone,
                    or_anim::EditTrack::from_track_id(track.track_id)?,
                ))
            }
            KIND_BONE => {
                let bone = row.primary() as u16;
                let track = self.tracks.iter().find(|t| t.bone == bone)?;
                Some((bone, or_anim::EditTrack::from_track_id(track.track_id)?))
            }
            _ => None,
        }
    }

    fn track_keys(&self, track: &TrackRow, view: TimeRange, out: &mut RowEvents) {
        let colour = track_colour(track.track_id);
        if track.undecodable {
            out.keys.push(KeyEvent {
                time: view.start.max(0.0),
                shape: KeyShape::Cross,
                colour: theme::WARNING.get(),
                selected: false,
            });
            out.total_keys = 1;
            return;
        }

        match track.format {
            Some(
                ChannelFormat::Constant | ChannelFormat::ConstantVec | ChannelFormat::ConstantQuat,
            ) => {
                out.keys.push(KeyEvent {
                    time: 0.0,
                    shape: KeyShape::Square,
                    colour,
                    selected: false,
                });
                out.total_keys = 1;
            }
            format => {
                let shape = match format {
                    Some(ChannelFormat::Raw) => KeyShape::Circle,
                    _ => KeyShape::Diamond,
                };
                let mut count = 0usize;
                for time in &self.frame_times {
                    if *time < view.start || *time > view.end {
                        continue;
                    }
                    count += 1;
                    out.keys.push(KeyEvent {
                        time: *time,
                        shape,
                        colour,
                        selected: false,
                    });
                }
                out.total_keys = count;
            }
        }
    }
}

impl TimelineSource for YcdTimelineSource {
    fn bounds(&self) -> TimeRange {
        TimeRange::from_duration(self.duration.max(1e-3))
    }

    fn rows(&self) -> &[ChannelRow] {
        &self.rows
    }

    fn row_events(&self, row: usize, view: TimeRange, out: &mut RowEvents) {
        out.clear();
        let Some(target) = self.targets.get(row) else {
            return;
        };
        match target {
            RowTarget::Tags => {
                for span in &self.tag_spans {
                    if span.range.end < view.start || span.range.start > view.end {
                        continue;
                    }
                    out.spans.push(span.clone());
                }
            }
            RowTarget::Track(index) => {
                if let Some(track) = self.tracks.get(*index) {
                    self.track_keys(track, view, out);
                }
            }
            RowTarget::Bone { tracks } => {
                let mut merged = RowEvents::default();
                let mut total = 0usize;
                for index in tracks {
                    let Some(track) = self.tracks.get(*index) else {
                        continue;
                    };
                    merged.clear();
                    self.track_keys(track, view, &mut merged);
                    total += merged.total_keys;
                    if out.keys.is_empty() {
                        out.keys.append(&mut merged.keys);
                    }
                }
                out.total_keys = total;
            }
        }
    }

    fn editable(&self) -> bool {
        self.read_only_reason.is_none()
    }

    fn read_only_reason(&self) -> Option<&str> {
        self.read_only_reason.as_deref()
    }

    fn frame_rate(&self) -> Option<f32> {
        self.frame_rate
    }

    fn summary(&self) -> String {
        self.summary.clone()
    }
}

fn by_bone_count(tracks: &[TrackRow]) -> usize {
    let mut bones: Vec<u16> = tracks.iter().map(|t| t.bone).collect();
    bones.sort_unstable();
    bones.dedup();
    bones.len()
}

fn track_colour(track_id: u8) -> Color32 {
    match track_id {
        track::BONE_TRANSLATION => TRANSLATION,
        track::BONE_ROTATION => ROTATION,
        track::BONE_SCALE => SCALE,
        _ => Color32::GRAY,
    }
}

fn collect_tracks(animation: &CrAnimation, skeleton: &Skeleton) -> Vec<TrackRow> {
    let formats = first_block_formats(animation);

    let mut out = Vec::new();
    for (dof_index, dof) in animation.dofs.iter().enumerate() {
        if !matches!(
            dof.track,
            track::BONE_TRANSLATION | track::BONE_ROTATION | track::BONE_SCALE
        ) {
            continue;
        }
        let Some(bone) = skeleton.index_for_tag(dof.id) else {
            continue;
        };
        out.push(TrackRow {
            bone,
            track_id: dof.track,
            dof: dof_index as u32,
            format: formats
                .as_ref()
                .ok()
                .and_then(|map| map.get(&(dof_index as u32)).copied()),
            undecodable: formats.is_err(),
        });
    }
    out.sort_by_key(|t| (t.bone, t.track_id));
    out
}

fn first_block_formats(animation: &CrAnimation) -> Result<BTreeMap<u32, ChannelFormat>, ()> {
    let Some(block) = animation.blocks.first().and_then(|b| b.as_deref()) else {
        return Err(());
    };
    let channels = ChannelWalk::new(block)
        .and_then(|walk| walk.channels())
        .map_err(|_| ())?;
    let mut out = BTreeMap::new();
    for channel in channels {
        out.entry(channel.dof_index).or_insert(channel.format);
    }
    Ok(out)
}

fn frame_times_of(animation: &CrAnimation, duration: f32) -> Vec<f32> {
    if animation.num_frames < 2 || duration <= 0.0 {
        return vec![0.0];
    }
    let intervals = (animation.num_frames - 1) as f32;
    (0..animation.num_frames)
        .map(|frame| frame as f32 / intervals * duration)
        .collect()
}

fn collect_tag_spans(clip: &CrClipAny, duration: f32) -> Vec<SpanEvent> {
    let Some(tags) = or_anim::clip::header(clip).tags.as_deref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for tag in tags.tags.iter().flatten() {
        let start = tag.start.clamp(0.0, 1.0) * duration;
        let end = tag.end.clamp(0.0, 1.0) * duration;
        let label = Some(format!("0x{:08X}", tag.base.name.key));
        if tag.start <= tag.end {
            out.push(SpanEvent {
                range: TimeRange::new(start, end),
                colour: TAG_SPAN,
                label,
            });
        } else {
            out.push(SpanEvent {
                range: TimeRange::new(start, duration),
                colour: TAG_SPAN,
                label: label.clone(),
            });
            out.push(SpanEvent {
                range: TimeRange::new(0.0, end),
                colour: TAG_SPAN,
                label,
            });
        }
    }
    out
}

fn bone_badge(tracks: &[TrackRow], indices: &[usize]) -> Option<RowBadge> {
    let mut marks = String::with_capacity(3);
    for (track_id, mark) in [
        (track::BONE_TRANSLATION, 'T'),
        (track::BONE_ROTATION, 'R'),
        (track::BONE_SCALE, 'S'),
    ] {
        let present = indices
            .iter()
            .any(|i| tracks.get(*i).is_some_and(|t| t.track_id == track_id));
        marks.push(if present { mark } else { '·' });
    }
    Some(RowBadge {
        text: marks,
        colour: theme::TEXT_SECONDARY.get(),
        tooltip: "Which of translation / rotation / scale this clip drives".to_string(),
    })
}

fn format_badge(track: &TrackRow) -> Option<RowBadge> {
    if track.undecodable {
        return Some(RowBadge {
            text: "compact".to_string(),
            colour: theme::WARNING.get(),
            tooltip: "Compressed block: readable and writable, but not decodable without \
                      crAnimCache expansion"
                .to_string(),
        });
    }
    let format = track.format?;
    Some(RowBadge {
        text: format.engine_name().trim_start_matches('k').to_string(),
        colour: format_colour(format),
        tooltip: format!(
            "Channel encoding {} (DOF {}). {}",
            format.engine_name(),
            track.dof,
            format_explanation(format)
        ),
    })
}

pub(crate) fn format_colour(format: ChannelFormat) -> Color32 {
    match format {
        ChannelFormat::Constant | ChannelFormat::ConstantVec | ChannelFormat::ConstantQuat => {
            Color32::from_rgb(0x70, 0x90, 0xB0)
        }
        ChannelFormat::Raw => Color32::from_rgb(0xD0, 0x80, 0x60),
        ChannelFormat::Quantize | ChannelFormat::Indirect => Color32::from_rgb(0xB0, 0xA0, 0x60),
        ChannelFormat::Linear => Color32::from_rgb(0x80, 0xB0, 0x80),
        ChannelFormat::Reconstruct | ChannelFormat::Normalize => {
            Color32::from_rgb(0x90, 0x80, 0xB0)
        }
    }
}

fn format_explanation(format: ChannelFormat) -> &'static str {
    match format {
        ChannelFormat::ConstantQuat => "Three constant floats, fourth component reconstructed.",
        ChannelFormat::ConstantVec => "Three constant floats for the whole block.",
        ChannelFormat::Constant => "One constant float for the whole block.",
        ChannelFormat::Raw => "A full f32 per frame — the densest encoding.",
        ChannelFormat::Quantize => "A fixed-width integer per frame, scaled and offset.",
        ChannelFormat::Indirect => "A per-frame index into a shared value table.",
        ChannelFormat::Linear => "Segmented linear prediction with Rice-coded deltas.",
        ChannelFormat::Reconstruct => "Post-pass computing a quaternion's missing component.",
        ChannelFormat::Normalize => "Post-pass normalizing a quaternion.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::codec::anim::format_type;
    use or_io::decl::{Keyed, Open};
    use or_io::gta5::{
        AtMap, AtMapEntry, CrAnimDictionary, CrBlockStream, CrBoneData, CrClip, CrClipAnimation,
        CrSkeletonData, CrTag, CrTags, Dof, EClipType, Vec4V,
    };

    fn v4(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
        Vec4V {
            x,
            y,
            z,
            w,
            ..Vec4V::default()
        }
    }

    fn dof(id: u16, r#type: u8, track: u8) -> Dof {
        Dof {
            id,
            r#type,
            track,
            ..Dof::default()
        }
    }

    const FORMAT_COUNT: usize = 9;

    fn bone(index: u16, tag: u16, parent: i16) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(0.0, 0.0, 0.0, 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("SKEL_{index}"),
            dofs: 0,
            index,
            bone_id: tag,
            mirror_index: 0,
            ..CrBoneData::default()
        }
    }

    fn skeleton() -> Skeleton {
        Skeleton::build(&CrSkeletonData {
            parent_indices: vec![-1, 0],
            bones: vec![bone(0, 100, -1), bone(1, 200, 0)],
            ..CrSkeletonData::default()
        })
    }

    fn constant_vec_block() -> CrBlockStream {
        let mut data = Vec::new();
        for v in [1.0f32, 2.0, 3.0] {
            data.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        let constant_size = data.len() as u32;
        let channel_offset = data.len() as u32;
        let mut counts = [0u16; FORMAT_COUNT];
        counts[ChannelFormat::ConstantVec as usize] = 1;
        for count in counts {
            data.extend_from_slice(&count.to_le_bytes());
        }
        for entry in [0u16; 4] {
            data.extend_from_slice(&entry.to_le_bytes());
        }
        for entry in [0u16, 0, 0, 0] {
            data.extend_from_slice(&entry.to_le_bytes());
        }
        CrBlockStream {
            constant_size,
            channel_offset,
            num_frames: 2,
            frame_size: 0,
            segment_size: 64,
            data,
            ..CrBlockStream::default()
        }
    }

    /// A clip and the dictionary holding the animation it plays.
    struct TestClip {
        dictionary: CrClipDictionary,
        clip: CrClipAny,
    }

    fn source(clip: &TestClip, expanded: &dyn Fn(RowId) -> bool) -> YcdTimelineSource {
        YcdTimelineSource::new(&clip.dictionary, &clip.clip, &skeleton(), expanded)
    }

    fn walk(animation: Keyed<CrAnimation>) -> CrClipAny {
        CrClipAny::CrClipAnimation(CrClipAnimation {
            base: CrClip {
                r#type: Open::new(EClipType::Animation),
                name: or_io::gta5::AtString::new("walk"),
                ..CrClip::default()
            },
            animation,
            end_time: 2.0,
            rate: 1.0,
            ..CrClipAnimation::default()
        })
    }

    fn clip_with(dofs: Vec<Dof>, frames: u16, compact: bool) -> TestClip {
        const ANIMATION: u32 = 1;
        let mut block = constant_vec_block();
        if compact {
            block.compact_size = 32;
        }
        let animation = CrAnimation {
            num_frames: frames,
            frames_per_chunk: 127,
            duration: 2.0,
            dofs,
            blocks: vec![Some(Box::new(block))],
            ..CrAnimation::default()
        };
        let animations = AtMap {
            hash: vec![Some(Box::new(AtMapEntry {
                key: ANIMATION,
                data: Some(Box::new(animation)),
                ..AtMapEntry::default()
            }))],
            ..AtMap::default()
        };
        TestClip {
            dictionary: CrClipDictionary {
                anim_dictionary: Some(Box::new(CrAnimDictionary {
                    animations,
                    ..CrAnimDictionary::default()
                })),
                ..CrClipDictionary::default()
            },
            clip: walk(Keyed::to(ANIMATION)),
        }
    }

    fn tag(clip: &mut TestClip, start: f32, end: f32) {
        or_anim::clip::header_mut(&mut clip.clip).tags = Some(Box::new(CrTags {
            tags: vec![Some(Box::new(CrTag {
                start,
                end,
                ..CrTag::default()
            }))],
            ..CrTags::default()
        }));
    }

    fn collapsed(_: RowId) -> bool {
        false
    }
    fn all_expanded(_: RowId) -> bool {
        true
    }

    #[test]
    fn bones_are_one_row_each_when_collapsed() {
        let clip = clip_with(
            vec![
                dof(100, format_type::VECTOR3, track::BONE_TRANSLATION),
                dof(100, format_type::QUATERNION, track::BONE_ROTATION),
                dof(200, format_type::QUATERNION, track::BONE_ROTATION),
            ],
            5,
            false,
        );
        let source = source(&clip, &collapsed);
        assert_eq!(source.rows().len(), 2);
        assert!(source.rows().iter().all(|r| r.expandable));
        assert_eq!(source.rows()[0].label, "SKEL_0");
    }

    #[test]
    fn expanding_reveals_one_row_per_track() {
        let clip = clip_with(
            vec![
                dof(100, format_type::VECTOR3, track::BONE_TRANSLATION),
                dof(100, format_type::QUATERNION, track::BONE_ROTATION),
                dof(200, format_type::QUATERNION, track::BONE_ROTATION),
            ],
            5,
            false,
        );
        let source = source(&clip, &all_expanded);
        assert_eq!(source.rows().len(), 5);
        let track_rows = source
            .rows()
            .iter()
            .filter(|r| r.kind == RowKind::Track)
            .count();
        assert_eq!(track_rows, 3);
    }

    #[test]
    fn track_rows_are_ordered_translation_rotation_scale() {
        let clip = clip_with(
            vec![
                dof(100, format_type::VECTOR3, track::BONE_SCALE),
                dof(100, format_type::QUATERNION, track::BONE_ROTATION),
                dof(100, format_type::VECTOR3, track::BONE_TRANSLATION),
            ],
            5,
            false,
        );
        let source = source(&clip, &all_expanded);
        let labels: Vec<&str> = source
            .rows()
            .iter()
            .filter(|r| r.kind == RowKind::Track)
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![
                track_name(track::BONE_TRANSLATION),
                track_name(track::BONE_ROTATION),
                track_name(track::BONE_SCALE)
            ]
        );
    }

    #[test]
    fn unresolvable_bone_tags_produce_no_rows() {
        let clip = clip_with(
            vec![dof(9999, format_type::QUATERNION, track::BONE_ROTATION)],
            5,
            false,
        );
        let source = source(&clip, &collapsed);
        assert!(source.rows().is_empty());
    }

    #[test]
    fn mover_and_non_transform_tracks_are_excluded() {
        let clip = clip_with(
            vec![
                dof(0, format_type::VECTOR3, track::MOVER_TRANSLATION),
                dof(100, format_type::FLOAT, track::VISIBILITY),
            ],
            5,
            false,
        );
        let source = source(&clip, &collapsed);
        assert!(source.rows().is_empty());
    }

    #[test]
    fn a_constant_channel_emits_one_square_key() {
        let clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            60,
            false,
        );
        let source = source(&clip, &all_expanded);
        let track_row = source
            .rows()
            .iter()
            .position(|r| r.kind == RowKind::Track)
            .expect("a track row");

        let mut events = RowEvents::default();
        source.row_events(track_row, source.bounds(), &mut events);
        assert_eq!(events.keys.len(), 1);
        assert_eq!(events.keys[0].shape, KeyShape::Square);
        assert_eq!(events.total_keys, 1);
    }

    #[test]
    fn frame_times_span_the_duration_with_n_minus_one_intervals() {
        let clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            5,
            false,
        );
        let source = source(&clip, &collapsed);
        assert_eq!(source.frame_times.len(), 5);
        assert!((source.frame_times[0] - 0.0).abs() < 1e-6);
        assert!((source.frame_times[4] - 2.0).abs() < 1e-6);
        assert!((source.frame_rate().unwrap() - 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_single_frame_animation_has_one_frame_time_and_no_rate() {
        let clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            1,
            false,
        );
        let source = source(&clip, &collapsed);
        assert_eq!(source.frame_times, vec![0.0]);
        assert!(source.frame_rate().is_none());
    }

    #[test]
    fn a_compact_block_marks_its_tracks_undecodable() {
        let clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            10,
            true,
        );
        let source = source(&clip, &all_expanded);
        assert!(source.tracks.iter().all(|t| t.undecodable));

        let track_row = source
            .rows()
            .iter()
            .position(|r| r.kind == RowKind::Track)
            .expect("a track row");
        let mut events = RowEvents::default();
        source.row_events(track_row, source.bounds(), &mut events);
        assert_eq!(events.keys[0].shape, KeyShape::Cross);

        let badge = source.rows()[track_row].badge.as_ref().expect("badge");
        assert_eq!(badge.text, "compact");
        assert!(source.read_only_reason().unwrap().contains("compact"));
    }

    #[test]
    fn a_bone_row_badge_shows_which_tracks_drive_it() {
        let clip = clip_with(
            vec![
                dof(100, format_type::VECTOR3, track::BONE_TRANSLATION),
                dof(100, format_type::QUATERNION, track::BONE_ROTATION),
            ],
            5,
            false,
        );
        let source = source(&clip, &collapsed);
        let badge = source.rows()[0].badge.as_ref().expect("badge");
        assert_eq!(badge.text, "TR·");
    }

    #[test]
    fn a_collapsed_bone_row_still_shows_keys() {
        let clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            5,
            false,
        );
        let source = source(&clip, &collapsed);
        let mut events = RowEvents::default();
        source.row_events(0, source.bounds(), &mut events);
        assert!(!events.keys.is_empty(), "collapsed bone drew nothing");
    }

    #[test]
    fn clip_tags_become_a_span_row() {
        let mut clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            5,
            false,
        );
        tag(&mut clip, 0.25, 0.75);
        let source = source(&clip, &collapsed);
        assert!(source.rows()[0].label.starts_with("Tags"));

        let mut events = RowEvents::default();
        source.row_events(0, source.bounds(), &mut events);
        assert_eq!(events.spans.len(), 1);
        assert!((events.spans[0].range.start - 0.5).abs() < 1e-6);
        assert!((events.spans[0].range.end - 1.5).abs() < 1e-6);
    }

    #[test]
    fn a_wrapped_tag_becomes_two_spans() {
        let mut clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            5,
            false,
        );
        tag(&mut clip, 0.9, 0.1);
        let source = source(&clip, &collapsed);
        let mut events = RowEvents::default();
        source.row_events(0, source.bounds(), &mut events);
        assert_eq!(events.spans.len(), 2);
        assert!(events.spans.iter().all(|s| s.range.span() >= 0.0));
    }

    #[test]
    fn an_out_of_range_row_is_harmless() {
        let clip = clip_with(Vec::new(), 5, false);
        let source = source(&clip, &collapsed);
        let mut events = RowEvents::default();
        source.row_events(99, source.bounds(), &mut events);
        assert!(events.keys.is_empty() && events.spans.is_empty());
    }

    #[test]
    fn a_clip_without_an_animation_is_read_only_and_empty() {
        let clip = TestClip {
            dictionary: CrClipDictionary::default(),
            clip: walk(Keyed::default()),
        };
        let source = source(&clip, &collapsed);
        assert!(source.rows().is_empty());
        assert!(!source.editable());
        assert!(source
            .read_only_reason()
            .unwrap()
            .contains("references no animation"));
        assert!(source.bounds().span() > 0.0);
    }

    #[test]
    fn every_channel_format_has_an_explanation_and_a_colour() {
        for format in [
            ChannelFormat::ConstantQuat,
            ChannelFormat::ConstantVec,
            ChannelFormat::Constant,
            ChannelFormat::Raw,
            ChannelFormat::Quantize,
            ChannelFormat::Indirect,
            ChannelFormat::Linear,
            ChannelFormat::Reconstruct,
            ChannelFormat::Normalize,
        ] {
            assert!(!format_explanation(format).is_empty());
            assert_ne!(format_colour(format), Color32::TRANSPARENT);
        }
    }

    #[test]
    fn the_summary_names_the_shape_of_the_animation() {
        let clip = clip_with(
            vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            7,
            false,
        );
        let source = source(&clip, &collapsed);
        let summary = source.summary();
        assert!(summary.contains("1 bones"), "{summary}");
        assert!(summary.contains("7 frames"), "{summary}");
        assert!(summary.contains("1 blocks"), "{summary}");
    }
}
