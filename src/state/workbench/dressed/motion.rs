//! The clip the dressed ped plays: its skeleton posed by a clip of the install or of a file, through the animation machinery of the Animation workspace.
//!
//! ⚠ **Presets are found, never assumed.** The dictionary a preset comes from is the one the engine
//! names as the default movement of a ped; an install without it, or without the clip in it, does
//! not offer the preset. A clip that drives few of the skeleton's bones is for another rig and is refused
//! with the figures, as the Animation workspace refuses it.

use std::sync::Arc;

use glam::Mat4;
use or_anim::{clip, BindingReport, ClipPlayer, Pose, Skeleton, Transport};
use or_io::gta5::CrClipDictionary;
use or_wardrobe::Ped;

/// The speeds the playback offers.
pub const SPEEDS: [f32; 5] = [0.25, 0.5, 1.0, 1.5, 2.0];

/// A clip of the engine's default movement dictionary, by the name its clips go by.
pub struct Preset {
    pub label: &'static str,
    pub clip: &'static str,
}

pub const PRESETS: [Preset; 4] = [
    Preset {
        label: tr_key!("Idle"),
        clip: "idle",
    },
    Preset {
        label: tr_key!("Walk"),
        clip: "walk",
    },
    Preset {
        label: tr_key!("Run"),
        clip: "run",
    },
    Preset {
        label: tr_key!("Sprint"),
        clip: "sprint",
    },
];

/// The file the default movement of `ped` is in: `move_m@generic` and `move_f@generic`, the clip sets the engine names for a ped model that states none.
pub fn dictionary_file(ped: Ped) -> String {
    format!("move_{}@generic.ycd", ped.letter())
}

/// The key of the clip called `name` in `dictionary`: the hash of the name, or a clip whose own name ends in it.
pub fn find(dictionary: &CrClipDictionary, name: &str) -> Option<u32> {
    let hashed = or_io::hash::string_hash_str(name);
    clip::clips(dictionary)
        .find(|(key, c)| {
            let own = clip::header(c).name.data.to_ascii_lowercase();
            let own = own.trim_end_matches(".clip");
            *key == hashed || own.rsplit(['/', ':', '\\']).next() == Some(name)
        })
        .map(|(key, _)| key)
}

/// Every clip of `dictionary` with the name it goes by, in the order of the dictionary's keys.
pub fn clips_of(dictionary: &CrClipDictionary) -> Vec<(u32, String)> {
    let mut all: Vec<(u32, String)> = clip::clips(dictionary)
        .map(|(key, c)| {
            let name = &clip::header(c).name.data;
            let name = match name.is_empty() {
                true => format!("0x{key:08X}"),
                false => name
                    .rsplit(['/', ':', '\\'])
                    .next()
                    .unwrap_or(name)
                    .to_string(),
            };
            (key, name)
        })
        .collect();
    all.sort_by_key(|a| a.1.to_ascii_lowercase());
    all
}

/// A clip bound to a skeleton and played: the pose it gives at the time the transport says.
pub struct Motion {
    /// What the clip is called on screen.
    pub label: String,
    /// The playhead, its rate the speed and its loop the clip's own flag.
    pub transport: Transport,
    pub dictionary: Arc<CrClipDictionary>,
    pub key: u32,
    pub report: BindingReport,
    player: ClipPlayer,
    skeleton: Skeleton,
    pose: Pose,
    world: Vec<Mat4>,
    skin: Vec<Mat4>,
    revision: u64,
}

impl Motion {
    /// Bind the clip `key` of `dictionary` to `skeleton`, posed at its first frame, or say why it does not fit.
    pub fn bind(
        label: String,
        dictionary: Arc<CrClipDictionary>,
        key: u32,
        skeleton: &Skeleton,
    ) -> Result<Self, String> {
        let clip = clip::clip(&dictionary, key).ok_or(tr!("the dictionary has no such clip"))?;
        let player = ClipPlayer::bind(skeleton, &dictionary, clip);
        let report = player.report().clone();
        if !report.looks_compatible() {
            return Err(tr!("{label} drives {resolved} of its {clip_tags} bones on this ped, so it is for another rig", label = label, resolved = report.resolved, clip_tags = report.clip_tags()));
        }
        let mut transport = Transport::default();
        player.configure_transport(&mut transport);
        let mut motion = Self {
            label,
            transport,
            dictionary,
            key,
            report,
            player,
            skeleton: skeleton.clone(),
            pose: Pose::bind(skeleton),
            world: Vec::new(),
            skin: Vec::new(),
            revision: 0,
        };
        motion.evaluate();
        Ok(motion)
    }

    /// Play or pause; a clip that has run to its end plays again from its start.
    pub fn toggle(&mut self) {
        let at_end = self.transport.time >= self.transport.range.end;
        if !self.transport.playing && at_end {
            self.transport.seek(self.transport.range.start);
            self.transport.take_seek();
            self.evaluate();
        }
        self.transport.toggle();
    }

    /// Advance the playhead by `dt` seconds and pose the skeleton there; a paused clip is not posed again.
    pub fn tick(&mut self, dt: f32) {
        let before = self.transport.time;
        self.transport.tick(dt);
        let _ = self.transport.take_seek();
        if self.transport.time != before {
            self.evaluate();
        }
    }

    /// Pose the skeleton at the playhead, after it was moved by hand.
    pub fn evaluate(&mut self) {
        self.player
            .evaluate_into(&self.skeleton, self.transport.time, &mut self.pose);
        self.pose
            .pose_into(&self.skeleton, &mut self.world, &mut self.skin);
        self.revision += 1;
    }

    /// Names the pose held, which moves whenever the pose does.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The matrices that skin a mesh to the pose.
    pub fn skin(&self) -> &[Mat4] {
        &self.skin
    }

    /// Where each bone is in the pose.
    pub fn world(&self) -> &[Mat4] {
        &self.world
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use or_io::codec::anim::{encode_block, format_type, track, DofSampleTable, EncodePolicy};
    use or_io::decl::{Keyed, Open};
    use or_io::gta5::{
        AtMap, AtMapEntry, AtString, CrAnimDictionary, CrAnimation, CrBoneData, CrClip,
        CrClipAnimation, CrClipAny, CrSkeletonData, Dof, EClipType, Vec4V,
    };

    const FRAMES: u16 = 12;
    const ANIMATION: u32 = 0x1000;

    fn v4(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
        Vec4V {
            x,
            y,
            z,
            w,
            ..Vec4V::default()
        }
    }

    fn bone(index: u16, tag: u16, parent: i16, translation: [f32; 3]) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(translation[0], translation[1], translation[2], 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("SKEL_{index}"),
            index,
            bone_id: tag,
            ..CrBoneData::default()
        }
    }

    /// Two bones, the second a metre from the first along x.
    pub(crate) fn skeleton() -> Skeleton {
        Skeleton::build(&CrSkeletonData {
            parent_indices: vec![-1, 0],
            bones: vec![
                bone(0, 100, -1, [0.0, 0.0, 0.0]),
                bone(1, 200, 0, [1.0, 0.0, 0.0]),
            ],
            ..CrSkeletonData::default()
        })
    }

    fn map<T: or_io::decl::Owned>(entries: Vec<(u32, T)>) -> AtMap<T>
    where
        or_io::decl::Raw<T>: or_io::decl::Record,
    {
        let head = entries.into_iter().rev().fold(None, |next, (key, data)| {
            Some(Box::new(AtMapEntry {
                key,
                data: Some(Box::new(data)),
                next,
                ..AtMapEntry::default()
            }))
        });
        AtMap {
            hash: vec![head],
            ..AtMap::default()
        }
    }

    /// The first bone moves along x by a tenth of a metre a frame, over a dictionary of clips each keyed by the hash of its name.
    pub(crate) fn dictionary(names: &[&str], tag: u16) -> CrClipDictionary {
        let frames: Vec<[f32; 4]> = (0..FRAMES)
            .map(|i| [i as f32 * 0.1, 0.0, 0.0, 0.0])
            .collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let block = encode_block(&[table], FRAMES, EncodePolicy::Lossless).expect("encode");
        let animation = CrAnimation {
            num_frames: FRAMES,
            frames_per_chunk: 127,
            duration: 1.1,
            name: "walk".into(),
            dofs: vec![Dof {
                id: tag,
                r#type: format_type::VECTOR3,
                track: track::BONE_TRANSLATION,
                ..Dof::default()
            }],
            max_block_size: block.data.len() as u32,
            blocks: vec![Some(Box::new(block.into()))],
            ..CrAnimation::default()
        };
        let clips = names
            .iter()
            .map(|name| {
                let clip = CrClipAnimation {
                    base: CrClip {
                        r#type: Open::new(EClipType::Animation),
                        name: AtString::new(name),
                        ..CrClip::default()
                    },
                    animation: Keyed::to(ANIMATION),
                    start_time: 0.0,
                    end_time: animation.duration,
                    rate: 1.0,
                    ..CrClipAnimation::default()
                };
                (
                    or_io::hash::string_hash_str(name),
                    CrClipAny::CrClipAnimation(clip),
                )
            })
            .collect();
        CrClipDictionary {
            anim_dictionary: Some(Box::new(CrAnimDictionary {
                animations: map(vec![(ANIMATION, animation)]),
                ..CrAnimDictionary::default()
            })),
            clips: map(clips),
            ..CrClipDictionary::default()
        }
    }

    /// A clip that plays on `ped` on a skeleton of a few bones, for a screen test that has no install to read them from.
    pub(crate) fn playing_on(dressed: &mut super::super::Dressed, ped: Ped) {
        dressed.show_skeleton(ped, skeleton());
        let dictionary = Arc::new(dictionary(&["walk", "run"], 100));
        let key = find(&dictionary, "run").expect("the run clip");
        dressed.bind_clip(ped, "Run".into(), dictionary, key, true);
    }

    #[test]
    fn a_preset_is_found_by_the_hash_of_its_name_or_the_name_a_clip_goes_by_and_a_clip_that_is_missing_is_not_offered(
    ) {
        let dictionary = dictionary(&["idle", "walk", "pack:/move/run.clip"], 100);
        for preset in &PRESETS[..2] {
            let key = find(&dictionary, preset.clip).expect(preset.label);
            assert_eq!(key, or_io::hash::string_hash_str(preset.clip));
        }
        // A clip whose key is not the hash of its name is found by the name inside it.
        assert!(find(&dictionary, "run").is_some());
        assert_eq!(find(&dictionary, "sprint"), None);
        let names: Vec<String> = clips_of(&dictionary).into_iter().map(|c| c.1).collect();
        assert_eq!(names, ["idle", "run.clip", "walk"]);
        assert_eq!(dictionary_file(Ped::Male), "move_m@generic.ycd");
        assert_eq!(dictionary_file(Ped::Female), "move_f@generic.ycd");
    }

    #[test]
    fn a_clip_poses_the_skeleton_as_time_moves_at_the_speed_asked_and_a_paused_clip_does_not() {
        let skeleton = skeleton();
        let dictionary = Arc::new(dictionary(&["walk"], 100));
        let key = find(&dictionary, "walk").unwrap();
        let mut motion = Motion::bind("Walk".into(), dictionary, key, &skeleton).unwrap();
        let first = motion.revision();
        assert_eq!(motion.world().len(), 2);
        let at_start = motion.world()[0].w_axis.x;

        motion.tick(0.5);
        assert_eq!(motion.revision(), first, "not playing: not posed again");
        motion.transport.play();
        motion.tick(0.5);
        let half = motion.world()[0].w_axis.x;
        assert!(half > at_start, "{half} {at_start}");
        assert!(motion.revision() > first);

        // Twice as fast reaches twice as far in the same time.
        let (mut slow, mut fast) = (
            Motion::bind("a".into(), motion.dictionary.clone(), key, &skeleton).unwrap(),
            Motion::bind("b".into(), motion.dictionary.clone(), key, &skeleton).unwrap(),
        );
        fast.transport.rate = 2.0;
        for m in [&mut slow, &mut fast] {
            m.transport.play();
            m.tick(0.25);
        }
        assert!((fast.transport.time - 2.0 * slow.transport.time).abs() < 1e-5);
        assert!(fast.world()[0].w_axis.x > slow.world()[0].w_axis.x);
    }

    #[test]
    fn a_clip_that_ran_to_its_end_plays_again_from_its_start() {
        let skeleton = skeleton();
        let dictionary = Arc::new(dictionary(&["walk"], 100));
        let key = find(&dictionary, "walk").unwrap();
        let mut motion = Motion::bind("Walk".into(), dictionary, key, &skeleton).unwrap();
        motion.transport.set_looping(false);
        motion.transport.play();
        motion.tick(5.0);
        assert!(!motion.transport.playing, "it stopped at its end");
        let end = motion.transport.time;
        assert!(end > 0.0);

        motion.toggle();
        assert!(motion.transport.playing);
        assert_eq!(motion.transport.time, 0.0, "from its start");
        motion.toggle();
        assert!(!motion.transport.playing, "and a playing clip is paused");
        assert_eq!(motion.transport.time, 0.0);
    }

    #[test]
    fn a_clip_for_another_rig_is_refused_with_the_figures() {
        let skeleton = skeleton();
        // The clip drives a bone tag the skeleton lacks.
        let dictionary = Arc::new(dictionary(&["walk"], 999));
        let key = find(&dictionary, "walk").unwrap();
        let why = Motion::bind("Walk".into(), dictionary, key, &skeleton)
            .err()
            .expect("refused");
        assert!(why.contains("drives 0 of its 1 bones"), "{why}");
    }
}
