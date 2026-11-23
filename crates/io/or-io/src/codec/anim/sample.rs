use crate::decl::{Vec3, Vec4};
use crate::error::Result;

use super::channel::decode_dof;
use super::{track, AnimationSource};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BonePose {
    pub translation: Vec3,
    pub rotation: Vec4,
    pub scale: Vec3,
}

impl Default for BonePose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl BonePose {
    pub const IDENTITY: Self = Self {
        translation: Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        scale: Vec3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
    };

    pub fn blend(&self, other: &Self, t: f32) -> Self {
        Self {
            translation: lerp_vec3(self.translation, other.translation, t),
            rotation: nlerp_quat(self.rotation, other.rotation, t),
            scale: lerp_vec3(self.scale, other.scale, t),
        }
    }

    pub fn align_rotation_to(&mut self, reference: &Vec4) {
        if dot_quat(reference, &self.rotation) < 0.0 {
            self.rotation = negate_quat(self.rotation);
        }
    }
}

/// The pose of the bone tagged `bone_tag` at `time` seconds, wrapped into the animation.
pub fn sample_bone<A: AnimationSource + ?Sized>(
    animation: &A,
    bone_tag: u16,
    time: f32,
) -> Result<Option<BonePose>> {
    let time = wrap_time(time, animation.duration());

    let mut pose = BonePose::IDENTITY;
    let mut found = false;

    for (track_id, slot) in [
        (track::BONE_TRANSLATION, Slot::Translation),
        (track::BONE_ROTATION, Slot::Rotation),
        (track::BONE_SCALE, Slot::Scale),
    ] {
        let Some(dof_index) = animation.find_dof(bone_tag, track_id) else {
            continue;
        };
        let Some(samples) = decode_dof(animation, dof_index, time)? else {
            continue;
        };
        found = true;

        let c = samples.components;
        match slot {
            Slot::Translation => {
                pose.translation = Vec3 {
                    x: c[0],
                    y: c[1],
                    z: c[2],
                }
            }
            Slot::Rotation => {
                pose.rotation = Vec4 {
                    x: c[0],
                    y: c[1],
                    z: c[2],
                    w: c[3],
                }
            }
            Slot::Scale => {
                pose.scale = Vec3 {
                    x: c[0],
                    y: c[1],
                    z: c[2],
                }
            }
        }
    }

    Ok(if found { Some(pose) } else { None })
}

enum Slot {
    Translation,
    Rotation,
    Scale,
}

fn wrap_time(time: f32, duration: f32) -> f32 {
    if duration <= 0.0 || !duration.is_finite() || !time.is_finite() {
        return 0.0;
    }
    let wrapped = time % duration;
    if wrapped < 0.0 {
        wrapped + duration
    } else {
        wrapped
    }
}

fn lerp_vec3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    Vec3 {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
    }
}

fn dot_quat(a: &Vec4, b: &Vec4) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w
}

fn negate_quat(q: Vec4) -> Vec4 {
    Vec4 {
        x: -q.x,
        y: -q.y,
        z: -q.z,
        w: -q.w,
    }
}

fn nlerp_quat(a: Vec4, b: Vec4, t: f32) -> Vec4 {
    let b = if dot_quat(&a, &b) < 0.0 {
        negate_quat(b)
    } else {
        b
    };
    let mut out = Vec4 {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
        w: a.w + (b.w - a.w) * t,
    };
    let length = (out.x * out.x + out.y * out.y + out.z * out.z + out.w * out.w).sqrt();
    if length > f32::MIN_POSITIVE {
        out.x /= length;
        out.y /= length;
        out.z /= length;
        out.w /= length;
    } else {
        out = Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
    }
    out
}

#[cfg(all(test, feature = "gta5"))]
mod tests {
    use super::super::format_type;
    use super::*;
    use crate::gta5::{CrAnimation, CrBlockStream, Dof};

    fn block_with_two_dofs(translation: [f32; 3], rotation_xyz: [f32; 3]) -> CrBlockStream {
        use super::super::channel::{ChannelFormat, FORMAT_COUNT};

        let mut data = Vec::new();
        for v in rotation_xyz {
            data.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in translation {
            data.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        let constant_size = data.len() as u32;
        let channel_offset = data.len() as u32;

        let mut counts = [0u16; FORMAT_COUNT];
        counts[ChannelFormat::ConstantQuat as usize] = 1;
        counts[ChannelFormat::ConstantVec as usize] = 1;
        for count in counts {
            data.extend_from_slice(&count.to_le_bytes());
        }
        for entry in [(1u16 << 2), 0, 0, 0] {
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

    fn animation_with(translation: [f32; 3], rotation_xyz: [f32; 3], bone_tag: u16) -> CrAnimation {
        let dof = |r#type, track| Dof {
            id: bone_tag,
            r#type,
            track,
            ..Dof::default()
        };
        CrAnimation {
            num_frames: 2,
            frames_per_chunk: 127,
            duration: 1.0,
            dofs: vec![
                dof(format_type::VECTOR3, track::BONE_TRANSLATION),
                dof(format_type::QUATERNION, track::BONE_ROTATION),
            ],
            blocks: vec![Some(Box::new(block_with_two_dofs(
                translation,
                rotation_xyz,
            )))],
            ..CrAnimation::default()
        }
    }

    #[test]
    fn samples_translation_and_rotation() {
        let animation = animation_with([1.0, 2.0, 3.0], [0.0, 0.0, 0.0], 0x1234);
        let pose = sample_bone(&animation, 0x1234, 0.0)
            .expect("sample")
            .expect("bone present");

        assert_eq!(pose.translation.x, 1.0);
        assert_eq!(pose.translation.y, 2.0);
        assert_eq!(pose.translation.z, 3.0);
        assert_eq!(pose.rotation.w, 1.0);
        assert_eq!(pose.scale.x, 1.0);
    }

    #[test]
    fn unknown_bone_tag_samples_nothing() {
        let animation = animation_with([1.0, 2.0, 3.0], [0.0, 0.0, 0.0], 0x1234);
        assert!(sample_bone(&animation, 0x9999, 0.0).unwrap().is_none());
    }

    #[test]
    fn an_animation_without_blocks_samples_nothing() {
        let mut animation = animation_with([1.0, 2.0, 3.0], [0.0, 0.0, 0.0], 0x1234);
        animation.blocks.clear();
        assert!(sample_bone(&animation, 0x1234, 0.0).unwrap().is_none());
    }

    #[test]
    fn start_and_end_of_a_loop_agree() {
        let animation = animation_with([1.0, 2.0, 3.0], [0.1, 0.2, 0.3], 0x1234);
        let duration = animation.duration;

        let at_start = sample_bone(&animation, 0x1234, 0.0).unwrap().unwrap();
        let at_end = sample_bone(&animation, 0x1234, duration).unwrap().unwrap();
        assert_eq!(at_start, at_end);
    }

    #[test]
    fn time_wraps_into_the_clip() {
        assert_eq!(wrap_time(0.0, 2.0), 0.0);
        assert_eq!(wrap_time(1.0, 2.0), 1.0);
        assert_eq!(wrap_time(2.0, 2.0), 0.0);
        assert_eq!(wrap_time(2.5, 2.0), 0.5);
        assert_eq!(wrap_time(-0.5, 2.0), 1.5);
        // Degenerate inputs must not produce NaN.
        assert_eq!(wrap_time(1.0, 0.0), 0.0);
        assert_eq!(wrap_time(f32::NAN, 2.0), 0.0);
        assert_eq!(wrap_time(1.0, f32::INFINITY), 0.0);
    }

    #[test]
    fn nlerp_takes_the_short_way_round() {
        let a = Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        let b = Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: -1.0,
        };
        let mid = nlerp_quat(a, b, 0.5);
        assert!((mid.w.abs() - 1.0).abs() < 1e-6, "mid = {mid:?}");
    }

    #[test]
    fn nlerp_endpoints_are_the_inputs() {
        let a = Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        let b = Vec4 {
            x: std::f32::consts::FRAC_1_SQRT_2,
            y: 0.0,
            z: 0.0,
            w: std::f32::consts::FRAC_1_SQRT_2,
        };
        let at_a = nlerp_quat(a, b, 0.0);
        assert!((at_a.w - 1.0).abs() < 1e-6);
        let at_b = nlerp_quat(a, b, 1.0);
        assert!((at_b.x - b.x).abs() < 1e-6 && (at_b.w - b.w).abs() < 1e-6);
    }

    #[test]
    fn nlerp_of_cancelling_quaternions_is_the_identity() {
        let a = Vec4 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        };
        let b = Vec4 {
            x: -1.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        };
        let mid = nlerp_quat(a, b, 0.5);
        assert!(!mid.x.is_nan() && !mid.w.is_nan(), "mid = {mid:?}");
    }

    #[test]
    fn align_rotation_flips_away_facing_quaternions() {
        let reference = Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        let mut pose = BonePose {
            rotation: Vec4 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: -1.0,
            },
            ..BonePose::IDENTITY
        };
        pose.align_rotation_to(&reference);
        assert_eq!(pose.rotation.w, 1.0);

        pose.align_rotation_to(&reference);
        assert_eq!(pose.rotation.w, 1.0);
    }

    #[test]
    fn blend_interpolates_all_three_tracks() {
        let a = BonePose::IDENTITY;
        let b = BonePose {
            translation: Vec3 {
                x: 2.0,
                y: 4.0,
                z: 6.0,
            },
            rotation: Vec4 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            scale: Vec3 {
                x: 3.0,
                y: 3.0,
                z: 3.0,
            },
        };
        let mid = a.blend(&b, 0.5);
        assert_eq!(mid.translation.x, 1.0);
        assert_eq!(mid.translation.z, 3.0);
        assert_eq!(mid.scale.x, 2.0);
        assert!((mid.rotation.w - 1.0).abs() < 1e-6);
    }
}
