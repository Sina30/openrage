use or_io::decl::{Keyed, Open};
use or_io::gta5::{
    Animation, AtMap, AtMapEntry, CrAnimDictionary, CrAnimation, CrClip, CrClipAnimation,
    CrClipAnimations, CrClipAny, CrClipDictionary, Dof, EClipType, Mat34V, Vec4V,
};

pub fn v4(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
    Vec4V {
        x,
        y,
        z,
        w,
        ..Vec4V::default()
    }
}

pub fn m34(col0: Vec4V, col1: Vec4V, col2: Vec4V, col3: Vec4V) -> Mat34V {
    Mat34V {
        col0,
        col1,
        col2,
        col3,
        ..Mat34V::default()
    }
}

pub fn dof(id: u16, r#type: u8, track: u8) -> Dof {
    Dof {
        id,
        r#type,
        track,
        ..Dof::default()
    }
}

/// A map holding every entry in one bucket.
pub fn one_bucket<T: or_io::decl::Owned>(entries: Vec<(u32, T)>) -> AtMap<T>
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

/// A clip playing the animation keyed `animation` over `[start, end]` at `rate`.
pub fn single_clip(name: &str, animation: u32, start: f32, end: f32, rate: f32) -> CrClipAny {
    CrClipAny::CrClipAnimation(CrClipAnimation {
        base: CrClip {
            r#type: Open::new(EClipType::Animation),
            name: or_io::gta5::AtString::new(name),
            ..CrClip::default()
        },
        animation: Keyed::to(animation),
        start_time: start,
        end_time: end,
        rate,
        ..CrClipAnimation::default()
    })
}

/// A dictionary of these clips and animations, each map in one bucket.
pub fn dictionary(
    clips: Vec<(u32, CrClipAny)>,
    animations: Vec<(u32, CrAnimation)>,
) -> CrClipDictionary {
    CrClipDictionary {
        anim_dictionary: Some(Box::new(CrAnimDictionary {
            animations: one_bucket(animations),
            ..CrAnimDictionary::default()
        })),
        clips: one_bucket(clips),
        ..CrClipDictionary::default()
    }
}

/// A clip playing each `(animation, start, end, rate)` in order, or all at once.
pub fn list_clip(
    played: &[(u32, f32, f32, f32)],
    total_duration: f32,
    parallel: bool,
) -> CrClipAny {
    CrClipAny::CrClipAnimations(CrClipAnimations {
        base: CrClip {
            r#type: Open::new(EClipType::Animations),
            ..CrClip::default()
        },
        animations: played
            .iter()
            .map(|&(key, start_time, end_time, rate)| Animation {
                start_time,
                end_time,
                rate,
                animation: Keyed::to(key),
                ..Animation::default()
            })
            .collect(),
        total_duration,
        parallel: or_io::decl::Bool8::new(parallel),
        ..CrClipAnimations::default()
    })
}
