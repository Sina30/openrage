//! A clip dictionary's clips and animations found by key, and what each clip
//! plays: a clip names its animations by the animation map's key.

use or_io::codec::anim::{format_type, track, BlockSource, COMPACT};
use or_io::gta5::{AtMap, AtMapEntry, CrAnimation, CrClip, CrClipAny, CrClipDictionary};

/// A clip's `flags` bits.
pub mod flags {
    pub const IS_LOOPED: u8 = 1 << 0;
    pub const IS_RESOURCE: u8 = 1 << 1;
}

/// One animation a clip plays, over a window of it at a rate.
#[derive(Debug, Clone, Copy)]
pub struct Played<'a> {
    pub animation: &'a CrAnimation,
    pub start_time: f32,
    pub end_time: f32,
    pub rate: f32,
}

fn entry<T>(map: &AtMap<T>, key: u32) -> Option<&T>
where
    T: or_io::gta5::Owned,
    or_io::decl::Raw<T>: or_io::decl::Record,
{
    map.entries().find(|e| e.key == key)?.data.as_deref()
}

fn entry_mut<T>(map: &mut AtMap<T>, key: u32) -> Option<&mut T>
where
    T: or_io::gta5::Owned,
    or_io::decl::Raw<T>: or_io::decl::Record,
{
    for head in map.hash.iter_mut() {
        let mut next: Option<&mut AtMapEntry<T>> = head.as_deref_mut();
        while let Some(entry) = next {
            if entry.key == key {
                return entry.data.as_deref_mut();
            }
            next = entry.next.as_deref_mut();
        }
    }
    None
}

/// Every clip, keyed, bucket by bucket.
pub fn clips(dictionary: &CrClipDictionary) -> impl Iterator<Item = (u32, &CrClipAny)> {
    dictionary
        .clips
        .entries()
        .filter_map(|e| Some((e.key, e.data.as_deref()?)))
}

/// The clip keyed `key`.
pub fn clip(dictionary: &CrClipDictionary, key: u32) -> Option<&CrClipAny> {
    entry(&dictionary.clips, key)
}

/// The clip keyed `key`, to edit.
pub fn clip_mut(dictionary: &mut CrClipDictionary, key: u32) -> Option<&mut CrClipAny> {
    entry_mut(&mut dictionary.clips, key)
}

/// The animation keyed `key`.
pub fn animation(dictionary: &CrClipDictionary, key: u32) -> Option<&CrAnimation> {
    entry(&dictionary.anim_dictionary.as_deref()?.animations, key)
}

/// The animation keyed `key`, to edit; every clip playing it plays the edit.
pub fn animation_mut(dictionary: &mut CrClipDictionary, key: u32) -> Option<&mut CrAnimation> {
    entry_mut(
        &mut dictionary.anim_dictionary.as_deref_mut()?.animations,
        key,
    )
}

/// What every clip layout starts with.
pub fn header(clip: &CrClipAny) -> &CrClip {
    match clip {
        CrClipAny::CrClipAnimation(c) => &c.base,
        CrClipAny::CrClipAnimations(c) => &c.base,
        CrClipAny::Base(c) => c,
    }
}

/// What every clip layout starts with, to edit.
pub fn header_mut(clip: &mut CrClipAny) -> &mut CrClip {
    match clip {
        CrClipAny::CrClipAnimation(c) => &mut c.base,
        CrClipAny::CrClipAnimations(c) => &mut c.base,
        CrClipAny::Base(c) => c,
    }
}

pub fn is_looped(clip: &CrClipAny) -> bool {
    header(clip).flags & flags::IS_LOOPED != 0
}

pub fn set_looped(clip: &mut CrClipAny, looped: bool) {
    let header = header_mut(clip);
    if looped {
        header.flags |= flags::IS_LOOPED;
    } else {
        header.flags &= !flags::IS_LOOPED;
    }
}

/// The length a clip states: its window for one animation, its total for several.
pub fn duration(clip: &CrClipAny) -> f32 {
    match clip {
        CrClipAny::CrClipAnimation(c) => (c.end_time - c.start_time).max(0.0),
        CrClipAny::CrClipAnimations(c) => c.total_duration,
        CrClipAny::Base(_) => 0.0,
    }
}

/// The animations a clip plays that the dictionary holds, in order.
pub fn played<'a>(dictionary: &'a CrClipDictionary, clip: &CrClipAny) -> Vec<Played<'a>> {
    let windows: Vec<(Option<u32>, f32, f32, f32)> = match clip {
        CrClipAny::CrClipAnimation(c) => vec![(c.animation.key, c.start_time, c.end_time, c.rate)],
        CrClipAny::CrClipAnimations(c) => c
            .animations
            .iter()
            .map(|a| (a.animation.key, a.start_time, a.end_time, a.rate))
            .collect(),
        CrClipAny::Base(_) => Vec::new(),
    };
    windows
        .into_iter()
        .filter_map(|(key, start_time, end_time, rate)| {
            Some(Played {
                animation: animation(dictionary, key?)?,
                start_time,
                end_time,
                rate,
            })
        })
        .collect()
}

/// The first animation a clip plays that the dictionary holds.
pub fn primary<'a>(dictionary: &'a CrClipDictionary, clip: &CrClipAny) -> Option<&'a CrAnimation> {
    played(dictionary, clip).first().map(|p| p.animation)
}

/// The bone tags an animation's bone tracks drive, in first-seen order.
pub fn bone_ids(animation: &CrAnimation) -> Vec<u16> {
    let mut ids = Vec::new();
    for dof in &animation.dofs {
        let bone = matches!(
            dof.track,
            track::BONE_TRANSLATION | track::BONE_ROTATION | track::BONE_SCALE
        );
        if bone && !ids.contains(&dof.id) {
            ids.push(dof.id);
        }
    }
    ids
}

/// Whether the animation, or any of its blocks, is compressed.
pub fn is_compact(animation: &CrAnimation) -> bool {
    animation.flags & COMPACT != 0 || animation.blocks.iter().flatten().any(|b| b.is_compact())
}

/// What a DOF's track drives, as a panel names it.
pub fn track_name(track: u8) -> &'static str {
    match track {
        track::BONE_TRANSLATION => "bone translation",
        track::BONE_ROTATION => "bone rotation",
        track::BONE_SCALE => "bone scale",
        3 => "bone constraint",
        track::VISIBILITY => "visibility",
        track::MOVER_TRANSLATION => "mover translation",
        track::MOVER_ROTATION => "mover rotation",
        0x80.. => "project-specific",
        _ => "rage-reserved",
    }
}

/// What a DOF's value is, as a panel names it.
pub fn format_name(format: u8) -> &'static str {
    match format {
        format_type::VECTOR3 => "vector3",
        format_type::QUATERNION => "quaternion",
        format_type::FLOAT => "float",
        _ => "unknown",
    }
}
