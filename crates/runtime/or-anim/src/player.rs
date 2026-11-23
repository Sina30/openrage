// Quaternion hemisphere history keeps scrubbing continuous (PrepareSlerp-style).
use crate::binding::{BindingReport, BoneBinding, ClipBinding};
use crate::pose::{BoneTransform, Pose};
use crate::skeleton::Skeleton;
use glam::{Quat, Vec3};
use or_io::codec::anim::{DofDecoder, DofSamples};
use or_io::gta5::{CrAnimation, CrClipAny, CrClipDictionary};
use or_transport::Transport;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineMode {
    Single,
    Parallel,
    Sequential,
}

#[derive(Debug, Clone)]
struct Track {
    animation: std::sync::Arc<CrAnimation>,
    binding: ClipBinding,
    start_time: f32,
    end_time: f32,
    rate: f32,
}

impl Track {
    fn animation_time(&self, clip_time: f32) -> f32 {
        let rate = if self.rate.is_finite() && self.rate != 0.0 {
            self.rate
        } else {
            1.0
        };
        let raw = self.start_time + clip_time * rate;
        let upper = if self.end_time > self.start_time {
            self.end_time
        } else {
            self.animation.duration
        };
        raw.clamp(self.start_time.min(upper), upper)
    }

    fn clip_duration(&self) -> f32 {
        let window = (self.end_time - self.start_time).max(0.0);
        let rate = self.rate.abs();
        if rate > f32::EPSILON {
            window / rate
        } else {
            window
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClipPlayer {
    tracks: Vec<Track>,
    combine: CombineModeOrDefault,
    duration: f32,
    looped: bool,
    report: BindingReport,
    rotation_history: Vec<Option<Quat>>,
    frame_rate: Option<f32>,
    frame_count: u16,
    last_errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CombineModeOrDefault(CombineMode);
impl Default for CombineModeOrDefault {
    fn default() -> Self {
        Self(CombineMode::Single)
    }
}

impl ClipPlayer {
    /// ⚠ Copies each animation the clip plays, so the player outlives an edit to the dictionary.
    pub fn bind(skeleton: &Skeleton, dictionary: &CrClipDictionary, clip: &CrClipAny) -> Self {
        let tracks: Vec<Track> = crate::clip::played(dictionary, clip)
            .into_iter()
            .map(|played| Track {
                binding: ClipBinding::build(skeleton, played.animation),
                animation: std::sync::Arc::new(played.animation.clone()),
                start_time: played.start_time,
                end_time: resolve_end(played.end_time, played.animation),
                rate: played.rate,
            })
            .collect();

        let combine = match clip {
            CrClipAny::CrClipAnimations(list) if tracks.len() > 1 => {
                if list.parallel.get() {
                    CombineMode::Parallel
                } else {
                    CombineMode::Sequential
                }
            }
            _ => CombineMode::Single,
        };

        let bindings: Vec<ClipBinding> = tracks.iter().map(|t| t.binding.clone()).collect();
        let report = ClipBinding::merged_report(&bindings);

        let duration = clip_duration(clip, &tracks, combine);
        let primary = tracks.first().map(|t| t.animation.as_ref());
        let frame_count = primary.map(|a| a.num_frames).unwrap_or(0);
        let frame_rate = primary.and_then(frame_rate_of);

        Self {
            tracks,
            combine: CombineModeOrDefault(combine),
            duration,
            looped: crate::clip::is_looped(clip),
            report,
            rotation_history: vec![None; skeleton.bone_count()],
            frame_rate,
            frame_count,
            last_errors: Vec::new(),
        }
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn is_looped(&self) -> bool {
        self.looped
    }

    pub fn report(&self) -> &BindingReport {
        &self.report
    }

    pub fn combine_mode(&self) -> CombineMode {
        self.combine.0
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn frame_rate(&self) -> Option<f32> {
        self.frame_rate
    }

    pub fn frame_count(&self) -> u16 {
        self.frame_count
    }

    pub fn last_errors(&self) -> &[String] {
        &self.last_errors
    }

    pub fn configure_transport(&self, transport: &mut Transport) {
        transport.set_duration(self.duration.max(0.0));
        transport.set_looping(self.looped);
        transport.frame_rate = self.frame_rate;
    }

    pub fn reset_history(&mut self) {
        self.rotation_history.fill(None);
    }

    pub fn evaluate(&mut self, skeleton: &Skeleton, clip_time: f32) -> Pose {
        let mut pose = Pose::bind(skeleton);
        self.evaluate_into(skeleton, clip_time, &mut pose);
        pose
    }

    pub fn evaluate_into(&mut self, skeleton: &Skeleton, clip_time: f32, pose: &mut Pose) {
        pose.reset_to_bind(skeleton);
        self.last_errors.clear();
        if self.tracks.is_empty() {
            return;
        }
        if self.rotation_history.len() != skeleton.bone_count() {
            self.rotation_history = vec![None; skeleton.bone_count()];
        }

        let clip_time = self.wrap(clip_time);

        match self.combine.0 {
            CombineMode::Single => {
                let (animation, binding, time) = {
                    let track = &self.tracks[0];
                    (
                        track.animation.clone(),
                        track.binding.clone(),
                        track.animation_time(clip_time),
                    )
                };
                self.apply_track(&animation, &binding, time, pose, 1.0);
            }
            CombineMode::Parallel => {
                let snapshot: Vec<(std::sync::Arc<CrAnimation>, ClipBinding, f32)> = self
                    .tracks
                    .iter()
                    .map(|t| {
                        (
                            t.animation.clone(),
                            t.binding.clone(),
                            t.animation_time(clip_time),
                        )
                    })
                    .collect();
                for (index, (animation, binding, time)) in snapshot.iter().enumerate() {
                    let weight = 1.0 / (index as f32 + 1.0);
                    self.apply_track(animation, binding, *time, pose, weight);
                }
            }
            CombineMode::Sequential => {
                if let Some((animation, binding, time)) = self.sequential_track(clip_time) {
                    self.apply_track(&animation, &binding, time, pose, 1.0);
                }
            }
        }
    }

    fn sequential_track(
        &self,
        clip_time: f32,
    ) -> Option<(std::sync::Arc<CrAnimation>, ClipBinding, f32)> {
        let mut cursor = 0.0f32;
        for track in &self.tracks {
            let length = track.clip_duration();
            if clip_time < cursor + length || length <= 0.0 {
                return Some((
                    track.animation.clone(),
                    track.binding.clone(),
                    track.animation_time(clip_time - cursor),
                ));
            }
            cursor += length;
        }
        self.tracks.last().map(|track| {
            (
                track.animation.clone(),
                track.binding.clone(),
                track.animation_time(track.clip_duration()),
            )
        })
    }

    fn apply_track(
        &mut self,
        animation: &CrAnimation,
        binding: &ClipBinding,
        time: f32,
        pose: &mut Pose,
        weight: f32,
    ) {
        // One decoder for the track: every sample below is at one time, so
        // in one block, and the block is walked once rather than once a DOF.
        let mut decoder = DofDecoder::new(animation);
        for bone in &binding.bones {
            let sampled = self.sample_bone(&mut decoder, bone, time, pose);
            let Some(sampled) = sampled else { continue };
            let blended = if weight >= 1.0 {
                sampled
            } else {
                pose.local(bone.bone)
                    .copied()
                    .unwrap_or(BoneTransform::IDENTITY)
                    .blend(&sampled, weight)
            };
            pose.set_local(bone.bone, blended);
        }

        if let Some(dof) = binding.mover_translation {
            if let Some(samples) = self.decode(&mut decoder, dof, time) {
                pose.root_motion.translation = Vec3::new(
                    samples.components[0],
                    samples.components[1],
                    samples.components[2],
                );
            }
        }
        if let Some(dof) = binding.mover_rotation {
            if let Some(samples) = self.decode(&mut decoder, dof, time) {
                let c = samples.components;
                let q = Quat::from_xyzw(c[0], c[1], c[2], c[3]);
                if q.length_squared() > 1e-12 {
                    pose.root_motion.rotation = q.normalize();
                }
            }
        }
    }

    fn sample_bone(
        &mut self,
        decoder: &mut DofDecoder<'_, CrAnimation>,
        bone: &BoneBinding,
        time: f32,
        pose: &Pose,
    ) -> Option<BoneTransform> {
        let current = pose
            .local(bone.bone)
            .copied()
            .unwrap_or(BoneTransform::IDENTITY);
        let mut out = current;
        let mut touched = false;

        if let Some(dof) = bone.translation {
            if let Some(s) = self.decode(decoder, dof, time) {
                out.translation = Vec3::new(s.components[0], s.components[1], s.components[2]);
                touched = true;
            }
        }
        if let Some(dof) = bone.scale {
            if let Some(s) = self.decode(decoder, dof, time) {
                out.scale = Vec3::new(s.components[0], s.components[1], s.components[2]);
                touched = true;
            }
        }
        if let Some(dof) = bone.rotation {
            if let Some(s) = self.decode(decoder, dof, time) {
                let c = s.components;
                let mut q = Quat::from_xyzw(c[0], c[1], c[2], c[3]);
                if q.length_squared() < 1e-12 {
                    q = Quat::IDENTITY;
                } else {
                    q = q.normalize();
                }
                let slot = self.rotation_history.get_mut(bone.bone as usize);
                if let Some(slot) = slot {
                    if let Some(previous) = *slot {
                        // Flip to stay on the same hemisphere as the previous sample.
                        if previous.dot(q) < 0.0 {
                            q = -q;
                        }
                    }
                    *slot = Some(q);
                }
                out.rotation = q;
                touched = true;
            }
        }

        if touched {
            Some(out)
        } else {
            None
        }
    }

    fn decode(
        &mut self,
        decoder: &mut DofDecoder<'_, CrAnimation>,
        dof: u32,
        time: f32,
    ) -> Option<DofSamples> {
        match decoder.decode(dof as usize, time) {
            Ok(samples) => samples,
            Err(error) => {
                let message = error.to_string();
                if !self.last_errors.contains(&message) {
                    self.last_errors.push(message);
                }
                None
            }
        }
    }

    fn wrap(&self, clip_time: f32) -> f32 {
        if !clip_time.is_finite() {
            return 0.0;
        }
        if self.duration <= 0.0 {
            return 0.0;
        }
        if self.looped {
            let wrapped = clip_time % self.duration;
            if wrapped < 0.0 {
                wrapped + self.duration
            } else {
                wrapped
            }
        } else {
            clip_time.clamp(0.0, self.duration)
        }
    }
}

fn resolve_end(end_time: f32, animation: &CrAnimation) -> f32 {
    if end_time > 0.0 && end_time.is_finite() {
        end_time
    } else {
        animation.duration
    }
}

fn clip_duration(clip: &CrClipAny, tracks: &[Track], combine: CombineMode) -> f32 {
    let declared = crate::clip::duration(clip);
    if declared > 0.0 && declared.is_finite() {
        return declared;
    }
    match combine {
        CombineMode::Sequential => tracks.iter().map(|t| t.clip_duration()).sum(),
        _ => tracks
            .iter()
            .map(|t| t.clip_duration())
            .fold(0.0f32, f32::max),
    }
}

/// Frames per second over the clip's stated duration; `None` for fewer than two frames or no duration.
pub fn frame_rate_of(animation: &CrAnimation) -> Option<f32> {
    if animation.num_frames < 2 || animation.duration <= 0.0 || !animation.duration.is_finite() {
        return None;
    }
    Some((animation.num_frames - 1) as f32 / animation.duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{dictionary, dof, list_clip, v4};
    use or_io::codec::anim::{format_type, track, ChannelFormat};
    use or_io::gta5::{CrBlockStream, CrBoneData, CrSkeletonData};
    const FORMAT_COUNT: usize = 9;
    fn bone(index: u16, tag: u16, parent: i16, translation: [f32; 3]) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(translation[0], translation[1], translation[2], 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("BONE_{index}"),
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
            bones: vec![
                bone(0, 100, -1, [0.0, 0.0, 0.0]),
                bone(1, 200, 0, [1.0, 0.0, 0.0]),
            ],
            ..CrSkeletonData::default()
        })
    }

    fn constant_vec_block(value: [f32; 3]) -> CrBlockStream {
        let mut data = Vec::new();
        for v in value {
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

    fn animation_with_translation(
        tag: u16,
        value: [f32; 3],
        duration: f32,
        frames: u16,
    ) -> CrAnimation {
        CrAnimation {
            num_frames: frames,
            frames_per_chunk: 127,
            duration,
            dofs: vec![dof(tag, format_type::VECTOR3, track::BONE_TRANSLATION)],
            blocks: vec![Some(Box::new(constant_vec_block(value)))],
            ..CrAnimation::default()
        }
    }

    /// A clip and the dictionary holding the animations it plays.
    struct TestClip {
        dictionary: CrClipDictionary,
        clip: CrClipAny,
    }

    fn play(skeleton: &Skeleton, clip: &TestClip) -> ClipPlayer {
        ClipPlayer::bind(skeleton, &clip.dictionary, &clip.clip)
    }

    fn single_clip(animation: CrAnimation, start: f32, end: f32, rate: f32) -> TestClip {
        TestClip {
            dictionary: dictionary(Vec::new(), vec![(1, animation)]),
            clip: crate::testing::single_clip("test", 1, start, end, rate),
        }
    }

    fn list(animations: Vec<CrAnimation>, total_duration: f32, parallel: bool) -> TestClip {
        let played: Vec<(u32, f32, f32, f32)> = (1..=animations.len() as u32)
            .map(|key| (key, 0.0, 1.0, 1.0))
            .collect();
        TestClip {
            clip: list_clip(&played, total_duration, parallel),
            dictionary: dictionary(Vec::new(), (1..).zip(animations).collect()),
        }
    }

    #[test]
    fn evaluates_a_driven_bone_and_leaves_others_at_bind() {
        let skeleton = skeleton();
        let clip = single_clip(
            animation_with_translation(100, [4.0, 5.0, 6.0], 1.0, 2),
            0.0,
            1.0,
            1.0,
        );
        let mut player = play(&skeleton, &clip);
        let pose = player.evaluate(&skeleton, 0.5);

        assert_eq!(pose.local(0).unwrap().translation, Vec3::new(4.0, 5.0, 6.0));
        assert_eq!(
            pose.local(1).unwrap().translation,
            skeleton.bones[1].bind_translation
        );
    }

    #[test]
    fn an_empty_clip_evaluates_to_the_bind_pose() {
        let skeleton = skeleton();
        let clip = TestClip {
            dictionary: CrClipDictionary::default(),
            clip: CrClipAny::Base(Default::default()),
        };
        let mut player = play(&skeleton, &clip);
        assert!(player.is_empty());
        let pose = player.evaluate(&skeleton, 0.25);
        for (i, local) in pose.locals().iter().enumerate() {
            assert_eq!(local.translation, skeleton.bones[i].bind_translation);
        }
    }

    #[test]
    fn a_clip_naming_an_animation_the_dictionary_lacks_binds_nothing() {
        let skeleton = skeleton();
        let mut clip = single_clip(
            animation_with_translation(100, [4.0, 5.0, 6.0], 1.0, 2),
            0.0,
            1.0,
            1.0,
        );
        clip.clip = crate::testing::single_clip("test", 2, 0.0, 1.0, 1.0);
        assert!(play(&skeleton, &clip).is_empty());
    }

    #[test]
    fn frame_rate_uses_frames_minus_one() {
        let skeleton = skeleton();
        let clip = single_clip(
            animation_with_translation(100, [0.0; 3], 1.0, 31),
            0.0,
            1.0,
            1.0,
        );
        let player = play(&skeleton, &clip);
        assert_eq!(player.frame_count(), 31);
        let fps = player.frame_rate().expect("frame rate");
        assert!((fps - 30.0).abs() < 1e-4, "fps was {fps}");
    }

    #[test]
    fn a_single_frame_animation_has_no_frame_rate() {
        let skeleton = skeleton();
        let clip = single_clip(
            animation_with_translation(100, [0.0; 3], 1.0, 1),
            0.0,
            1.0,
            1.0,
        );
        assert!(play(&skeleton, &clip).frame_rate().is_none());
    }

    #[test]
    fn configure_transport_copies_duration_loop_and_frame_rate() {
        let skeleton = skeleton();
        let mut clip = single_clip(
            animation_with_translation(100, [0.0; 3], 2.0, 61),
            0.0,
            2.0,
            1.0,
        );
        crate::clip::set_looped(&mut clip.clip, true);

        let player = play(&skeleton, &clip);
        let mut transport = Transport::default();
        player.configure_transport(&mut transport);

        assert!((transport.bounds.end - 2.0).abs() < 1e-6);
        assert!(transport.is_looping());
        assert!((transport.frame_rate.unwrap() - 30.0).abs() < 1e-4);
    }

    #[test]
    fn clip_window_offsets_and_scales_animation_time() {
        let animation = animation_with_translation(100, [0.0; 3], 4.0, 5);
        let skeleton = skeleton();
        let clip = single_clip(animation, 1.0, 3.0, 2.0);
        let player = play(&skeleton, &clip);
        let track = &player.tracks[0];

        assert!((track.animation_time(0.0) - 1.0).abs() < 1e-6);
        assert!((track.animation_time(0.5) - 2.0).abs() < 1e-6);
        assert!((track.animation_time(5.0) - 3.0).abs() < 1e-6);
        assert!((track.clip_duration() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_zero_end_time_falls_back_to_the_animation_duration() {
        let skeleton = skeleton();
        let clip = single_clip(
            animation_with_translation(100, [0.0; 3], 3.0, 4),
            0.0,
            0.0,
            1.0,
        );
        let player = play(&skeleton, &clip);
        assert!((player.tracks[0].end_time - 3.0).abs() < 1e-6);
        assert!((player.duration() - 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_zero_rate_is_treated_as_realtime() {
        let skeleton = skeleton();
        let clip = single_clip(
            animation_with_translation(100, [0.0; 3], 2.0, 3),
            0.0,
            2.0,
            0.0,
        );
        let player = play(&skeleton, &clip);
        assert!((player.tracks[0].animation_time(1.0) - 1.0).abs() < 1e-6);
        assert!(player.duration().is_finite() && player.duration() > 0.0);
    }

    #[test]
    fn looping_wraps_time_and_clamping_does_not() {
        let skeleton = skeleton();
        let animation = animation_with_translation(100, [0.0; 3], 2.0, 3);

        let mut looped = single_clip(animation.clone(), 0.0, 2.0, 1.0);
        crate::clip::set_looped(&mut looped.clip, true);
        let player = play(&skeleton, &looped);
        assert!((player.wrap(2.5) - 0.5).abs() < 1e-6);
        assert!((player.wrap(-0.5) - 1.5).abs() < 1e-6);

        let once = single_clip(animation, 0.0, 2.0, 1.0);
        let player = play(&skeleton, &once);
        assert!((player.wrap(2.5) - 2.0).abs() < 1e-6);
        assert!((player.wrap(-0.5) - 0.0).abs() < 1e-6);
        assert_eq!(player.wrap(f32::NAN), 0.0);
    }

    #[test]
    fn root_motion_is_read_from_the_mover_track() {
        let skeleton = skeleton();
        let animation = CrAnimation {
            num_frames: 2,
            frames_per_chunk: 127,
            duration: 1.0,
            dofs: vec![dof(0, format_type::VECTOR3, track::MOVER_TRANSLATION)],
            blocks: vec![Some(Box::new(constant_vec_block([7.0, 8.0, 9.0])))],
            ..CrAnimation::default()
        };
        let clip = single_clip(animation, 0.0, 1.0, 1.0);
        let mut player = play(&skeleton, &clip);
        assert!(player.report().has_root_motion);

        let pose = player.evaluate(&skeleton, 0.0);
        assert_eq!(pose.root_motion.translation, Vec3::new(7.0, 8.0, 9.0));
    }

    #[test]
    fn parallel_clips_average_their_tracks() {
        let skeleton = skeleton();
        let clip = list(
            vec![
                animation_with_translation(100, [0.0, 0.0, 0.0], 1.0, 2),
                animation_with_translation(100, [4.0, 0.0, 0.0], 1.0, 2),
            ],
            1.0,
            true,
        );
        let mut player = play(&skeleton, &clip);
        assert_eq!(player.combine_mode(), CombineMode::Parallel);
        assert_eq!(player.track_count(), 2);

        let pose = player.evaluate(&skeleton, 0.0);
        assert!((pose.local(0).unwrap().translation.x - 2.0).abs() < 1e-5);
    }

    #[test]
    fn sequential_clips_pick_the_track_covering_the_time() {
        let skeleton = skeleton();
        let clip = list(
            vec![
                animation_with_translation(100, [1.0, 0.0, 0.0], 1.0, 2),
                animation_with_translation(100, [9.0, 0.0, 0.0], 1.0, 2),
            ],
            2.0,
            false,
        );
        let mut player = play(&skeleton, &clip);
        assert_eq!(player.combine_mode(), CombineMode::Sequential);

        assert!(
            (player
                .evaluate(&skeleton, 0.5)
                .local(0)
                .unwrap()
                .translation
                .x
                - 1.0)
                .abs()
                < 1e-5
        );
        assert!(
            (player
                .evaluate(&skeleton, 1.5)
                .local(0)
                .unwrap()
                .translation
                .x
                - 9.0)
                .abs()
                < 1e-5
        );
        assert!(
            (player
                .evaluate(&skeleton, 9.0)
                .local(0)
                .unwrap()
                .translation
                .x
                - 9.0)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn a_one_entry_animations_list_is_treated_as_single() {
        let skeleton = skeleton();
        let clip = list(
            vec![animation_with_translation(100, [3.0, 0.0, 0.0], 1.0, 2)],
            1.0,
            true,
        );
        let player = play(&skeleton, &clip);
        assert_eq!(player.combine_mode(), CombineMode::Single);
    }

    #[test]
    fn a_compact_animation_reports_the_decode_failure_and_poses_bind() {
        let skeleton = skeleton();
        let mut block = constant_vec_block([5.0, 0.0, 0.0]);
        block.compact_size = 64;
        let animation = CrAnimation {
            num_frames: 2,
            frames_per_chunk: 127,
            duration: 1.0,
            dofs: vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            blocks: vec![Some(Box::new(block))],
            ..CrAnimation::default()
        };
        let clip = single_clip(animation, 0.0, 1.0, 1.0);
        let mut player = play(&skeleton, &clip);
        assert!(player.report().compact);

        let pose = player.evaluate(&skeleton, 0.0);
        assert_eq!(
            pose.local(0).unwrap().translation,
            skeleton.bones[0].bind_translation
        );
        assert!(!player.last_errors().is_empty());
    }

    #[test]
    fn history_reset_clears_hemisphere_state() {
        let skeleton = skeleton();
        let clip = single_clip(
            animation_with_translation(100, [0.0; 3], 1.0, 2),
            0.0,
            1.0,
            1.0,
        );
        let mut player = play(&skeleton, &clip);
        let _ = player.evaluate(&skeleton, 0.0);
        player.reset_history();
        assert!(player.rotation_history.iter().all(|h| h.is_none()));
    }

    #[test]
    fn evaluating_twice_reuses_the_pose_buffer() {
        let skeleton = skeleton();
        let clip = single_clip(
            animation_with_translation(100, [1.0, 2.0, 3.0], 1.0, 2),
            0.0,
            1.0,
            1.0,
        );
        let mut player = play(&skeleton, &clip);
        let mut pose = Pose::bind(&skeleton);
        player.evaluate_into(&skeleton, 0.0, &mut pose);
        player.evaluate_into(&skeleton, 0.5, &mut pose);
        assert_eq!(pose.bone_count(), skeleton.bone_count());
        assert_eq!(pose.local(0).unwrap().translation, Vec3::new(1.0, 2.0, 3.0));
    }
}
