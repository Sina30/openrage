//! A clip's fields written in place: a group entry's window, its rate and a three-point warp list
//! following from it, and a clip's name with the key it hashes to; a name its field cannot hold
//! takes text placed after every object.

use std::collections::BTreeSet;

use super::super::{AnimAt, AnimError, AnimResource, ClipPlay, Section, ENTRY_LEN, GROUP_LEN};
use super::{section_of, VIRTUAL_BASE};
use crate::container::rpf::Rpf9Sections;
use crate::decl::Aligned;
use crate::gta6::hash::name_hash32;

const WARP_POINT: usize = 16;

fn unwritten(why: impl Into<String>) -> AnimError {
    AnimError::Unwritten(why.into())
}

fn put(out: &mut [u8], at: usize, bytes: &[u8]) {
    out[at..at + bytes.len()].copy_from_slice(bytes);
}

/// Where an entry is: its clip, by its place in the dictionary, its group and its place there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntryAt {
    pub clip: usize,
    pub group: usize,
    pub entry: usize,
}

/// The window `start .. end` of the entry `at` names, its clip at `clip_at`, written into `out`
/// and `edited`: the rate the window and the group's duration give, and a three-point warp list
/// as the window gives it.
pub(super) fn entry_window(
    s: &Section,
    out: &mut [u8],
    edited: &mut AnimResource,
    clip_at: usize,
    at: EntryAt,
    [start, end]: [f32; 2],
) -> Result<(), AnimError> {
    if s.u32("clip", clip_at + 0x20)? != 2 {
        return Err(unwritten(
            "only a clip of groups has entries; a clip of one animation states one window",
        ));
    }
    let group_at = *(s
        .array("clip group list", clip_at + 0x68, GROUP_LEN)?
        .get(at.group))
    .ok_or_else(|| unwritten("the clip has no such group"))?;
    let entry_at = *(s
        .array("clip entry list", group_at, ENTRY_LEN)?
        .get(at.entry))
    .ok_or_else(|| unwritten("the group has no such entry"))?;
    let duration = s.f32("clip group", group_at + 0x10)?;
    if !(start.is_finite() && end.is_finite() && end > start && duration > 0.0) {
        return Err(unwritten(
            "an entry's window runs from a finite start to a later end, in a group of some duration",
        ));
    }
    let window = end - start;
    let rate = window / duration;
    let unresolved = || unwritten("the clip's groups do not resolve, so it is not written");
    let animation = match edited.clips.get(at.clip).map(|c| &c.play) {
        Some(ClipPlay::Groups { groups, .. }) => (groups.get(at.group))
            .and_then(|g| g.entries.get(at.entry))
            .map(|e| e.animation),
        _ => None,
    }
    .ok_or_else(unresolved)?;
    let points = s.array("warp point list", entry_at + 0x10, WARP_POINT)?;
    let fps = animation.and_then(|a| frame_rate(&*edited, a));
    let warp = match points.len() {
        0 => Vec::new(),
        3 => {
            let fps = fps.ok_or_else(|| {
                unwritten("its animation is in another body or has one frame, so the frame rate its warp list follows from is not known")
            })?;
            let (d, w) = (f64::from(duration), f64::from(window));
            let middle = [(d * fps).floor() / fps, d * (w - 1.0 / fps) / w];
            vec![[0.0, 0.0], middle.map(|v| v as f32), [duration, duration]]
        }
        n => {
            return Err(unwritten(format!(
                "its warp list of {n} points follows no known rule, so its window is not changed"
            )))
        }
    };
    for (point, [x, y]) in points.iter().zip(&warp) {
        put(out, *point, &x.to_le_bytes());
        put(out, point + 4, &y.to_le_bytes());
    }
    for (field, value) in [(0x28, start), (0x2C, end), (0x30, rate)] {
        put(out, entry_at + field, &value.to_le_bytes());
    }
    let play = edited.clips.get_mut(at.clip).map(|c| &mut c.play);
    if let Some(ClipPlay::Groups { groups, .. }) = play {
        if let Some(held) = (groups.get_mut(at.group)).and_then(|g| g.entries.get_mut(at.entry)) {
            (held.start, held.end, held.rate) = (start, end, rate);
            if !warp.is_empty() {
                held.warp_points = warp;
            }
        }
    }
    Ok(())
}

/// The frames a second of the animation at `at`, rounded as a warp list's rule takes them.
fn frame_rate(resource: &AnimResource, at: AnimAt) -> Option<f64> {
    let animation = resource.sets.get(at.set)?.animations.get(at.animation)?;
    Some(f64::from(animation.rate()?).round()).filter(|f| *f > 0.0)
}

/// The 64-bit key a clip named `name` takes in a dictionary of key `root_key`: its leaf's hash
/// low, and its folder's hash high where it has one, else the dictionary's key.
pub(super) fn clip_key(name: &str, root_key: u32) -> u64 {
    let base = name.strip_prefix("pack:/").unwrap_or(name);
    let base = base.strip_suffix(".clip").unwrap_or(base);
    let (folder, leaf) = base.rsplit_once('/').unwrap_or(("", base));
    let high = if folder.is_empty() {
        root_key
    } else {
        name_hash32(folder.as_bytes())
    };
    u64::from(high) << 32 | u64::from(name_hash32(leaf.as_bytes()))
}

fn check_name(name: &str) -> Result<(), AnimError> {
    let printable = name.bytes().all(|b| (32..127).contains(&b));
    if name.is_empty() || name.len() >= usize::from(u16::MAX) || !printable {
        return Err(unwritten(
            "a clip's name is printable ASCII text shorter than 65,535 bytes",
        ));
    }
    Ok(())
}

/// `name`'s text placed after every object, as a grown DOF list is, and clip `clip`'s name field
/// in `out` pointed at it, where the field `s` holds at `clip_at` cannot hold that name.
pub(super) fn make_room(
    s: &Section,
    resource: &AnimResource,
    sections: &Rpf9Sections,
    out: &mut Vec<u8>,
    clip: usize,
    clip_at: usize,
    name: &str,
) -> Result<(), AnimError> {
    check_name(name)?;
    let field = s.bytes("clip name", clip_at + 0x28, 16)?;
    let room = match field[15] & 0x80 != 0 {
        true => 15,
        false => usize::from(s.u16("clip name", clip_at + 0x30)?),
    };
    if name.len() <= room {
        return Ok(());
    }
    let mut text = name.as_bytes().to_vec();
    text.push(0);
    let text_at = super::relayout::place_list(s, resource, sections, out, &text)?;
    // A growth that gave the block map more slots moved what follows them, the clip perhaps.
    let placed = Aligned::from_bytes(out);
    let clip_at = *(section_of(&placed).clip_offsets(resource)?.get(clip))
        .ok_or_else(|| unwritten("the resource has no such clip"))?;
    let n = name.len() as u16;
    put(
        out,
        clip_at + 0x28,
        &(VIRTUAL_BASE + text_at as u64).to_le_bytes(),
    );
    put(out, clip_at + 0x30, &n.to_le_bytes());
    put(out, clip_at + 0x32, &(n + 1).to_le_bytes());
    put(out, clip_at + 0x34, &1u32.to_le_bytes());
    Ok(())
}

/// Clip `clip`, at `clip_at`, renamed `name` in `out`: its text where the old one lay, its key, its
/// map entry moved to the bucket the key falls in, and the root's word at 0x6C as its clips'
/// sources give it.
pub(super) fn rename(
    s: &Section,
    out: &mut [u8],
    resource: &AnimResource,
    clip: usize,
    clip_at: usize,
    name: &str,
) -> Result<(), AnimError> {
    check_name(name)?;
    let key = clip_key(name, resource.key);
    let old =
        (resource.clips.get(clip)).ok_or_else(|| unwritten("the resource has no such clip"))?;
    let old = old.key;
    if (resource.clips.iter().enumerate()).any(|(i, c)| i != clip && c.key == key) {
        return Err(unwritten(
            "another clip of the dictionary has the key that name hashes to",
        ));
    }
    let grows =
        || unwritten("the name is longer than its field holds, and no room was made for it");
    if s.bytes("clip name", clip_at + 0x28, 16)?[15] & 0x80 != 0 {
        if name.len() > 15 {
            return Err(grows());
        }
        let mut inline = [0u8; 16];
        inline[..name.len()].copy_from_slice(name.as_bytes());
        inline[15] = 0x80 | name.len() as u8;
        put(out, clip_at + 0x28, &inline);
    } else {
        let text = s.target("clip name", clip_at + 0x28)?;
        let len = usize::from(s.u16("clip name", clip_at + 0x30)?);
        s.bytes("clip name", text, len)?;
        if name.len() > len {
            return Err(grows());
        }
        out[text..text + len].fill(0);
        put(out, text, name.as_bytes());
        let n = name.len() as u16;
        put(out, clip_at + 0x30, &n.to_le_bytes());
        put(out, clip_at + 0x32, &n.wrapping_add(1).to_le_bytes());
    }
    put(out, clip_at + 0x58, &key.to_le_bytes());
    let buckets = s.target("clip map", 0x48)?;
    let slots = u64::from(s.u32("clip map", 0x50)?);
    let used = s.u32("clip map", 0x58)? as usize;
    if slots == 0 {
        return Err(unwritten("the clip map has no bucket"));
    }
    let bucket = |key: u64| buckets + 8 * (key % slots) as usize;
    let (from, to) = (bucket(old), bucket(key));
    // The entry naming the clip, and the word that points at it: its bucket or an entry's next.
    let (mut link, mut next, mut walked) = (from, s.pointer("clip map", from)?, 0);
    let entry = loop {
        let at = next.ok_or_else(|| unwritten("the clip map's chain does not hold the clip"))?;
        if walked > used {
            return Err(unwritten("the clip map's chain is longer than its entries"));
        }
        if s.pointer("clip map", at + 8)? == Some(clip_at) {
            break at;
        }
        (link, next, walked) = (at + 0x10, s.pointer("clip map", at + 0x10)?, walked + 1);
    };
    put(out, entry, &key.to_le_bytes());
    if from != to {
        let after = s.u64("clip map", entry + 0x10)?;
        put(out, link, &after.to_le_bytes());
        let head = s.u64("clip map", to)?;
        put(out, entry + 0x10, &head.to_le_bytes());
        put(out, to, &(VIRTUAL_BASE + entry as u64).to_le_bytes());
    }
    let sources: BTreeSet<u64> = (resource.clips.iter().enumerate())
        .map(|(i, c)| (if i == clip { key } else { c.key }) >> 32)
        .collect();
    let word: u32 = if sources.len() > 1 { 3 } else { 2 };
    put(out, 0x6C, &word.to_le_bytes());
    Ok(())
}
