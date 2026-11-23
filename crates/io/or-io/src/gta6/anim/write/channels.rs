//! A key written in place into a channel of sections 0 to 4 of a block, so no size changes: a
//! constant takes the value at every frame of its block, a raw channel at the frame, and a
//! quantised one at the frame in its own bits, its scale and offset set afresh over its values
//! when the value lies off its range.

use std::collections::BTreeSet;
use std::ops::Range;

use super::super::{constants, directory, Animation, DELTA_SECTION};
use super::KeyEdit;
use crate::codec::anim::channel::{ChannelFormat, ChannelWalk};
use crate::gta5::CrBlockStream;

/// `stream` with `edit` made in place where its channel is in sections 0 to 4, and the edit left
/// for section 10's encoder where its channel is there; a layout the reader refuses is left for
/// that encoder to refuse by name.
pub(super) fn apply(
    mut stream: CrBlockStream,
    dofs: usize,
    edit: Option<KeyEdit>,
) -> Result<(CrBlockStream, Option<KeyEdit>), String> {
    let Some(edit) = edit else {
        return Ok((stream, None));
    };
    let (Ok(dir), Ok(mut words)) = (directory(&stream, dofs), constants(&stream)) else {
        return Ok((stream, Some(edit)));
    };
    if dir.runs[DELTA_SECTION].contains(&edit.index) {
        return Ok((stream, Some(edit)));
    }
    if !edit.value.is_finite() {
        return Err(format!("the value {} is not finite", edit.value));
    }
    let (dof, component) = (edit.index >> 2, usize::from(edit.index & 3));
    let (mut constant, mut bit) = (0usize, 0usize);
    for (section, run) in dir.runs.iter().enumerate().take(DELTA_SECTION) {
        for &index in run {
            // A constant rotation's w follows from x, y and z, so it is held by no channel.
            let covers = match section {
                0 | 1 => index == dof << 2 && component < 3,
                _ => index == edit.index,
            };
            if covers {
                match section {
                    0..=2 => {
                        let at = constant + if section == 2 { 0 } else { component };
                        word(&words, at)?;
                        words[at] = edit.value.to_bits();
                    }
                    3 => {
                        let value = u64::from(edit.value.to_bits());
                        put_frame(&mut stream, edit.frame, bit, 32, value)?;
                    }
                    4 => quantised(&mut stream, &mut words, constant, bit, edit)?,
                    5 => {
                        return Err("its channel is in section 5, whose frames index a table of values, so no key is written into it".into());
                    }
                    _ => {
                        return Err(format!(
                            "its channel is in section {section}, which follows from the other components"
                        ));
                    }
                }
                let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
                stream.data[..bytes.len()].copy_from_slice(&bytes);
                return Ok((stream, None));
            }
            match section {
                0 | 1 => constant += 3,
                2 => constant += 1,
                3 => bit += 32,
                4 => {
                    bit += (word(&words, constant)? & 31) as usize;
                    constant += 3;
                }
                5 => {
                    bit += word(&words, constant)? as usize;
                    constant += 5 + word(&words, constant + 2)? as usize;
                }
                _ => {}
            }
        }
    }
    Err(format!(
        "it has no channel of component {component} of DOF {dof}"
    ))
}

fn word(words: &[u32], at: usize) -> Result<u32, String> {
    (words.get(at).copied())
        .ok_or_else(|| format!("its channels read constant word {at} of {}", words.len()))
}

/// The quantised channel whose three words start at `constant` and whose steps lie at `bit` of
/// every frame, its step at `edit.frame` set to `edit.value`.
fn quantised(
    stream: &mut CrBlockStream,
    words: &mut [u32],
    constant: usize,
    bit: usize,
    edit: KeyEdit,
) -> Result<(), String> {
    let width = (word(words, constant)? & 31) as usize;
    let scale = f32::from_bits(word(words, constant + 1)?);
    let offset = f32::from_bits(word(words, constant + 2)?);
    if width == 0 {
        return Err(
            "its quantised channel holds no bit a frame, so it takes one value at every frame"
                .into(),
        );
    }
    let top = (1u64 << width) - 1;
    let step = ((f64::from(edit.value) - f64::from(offset)) / f64::from(scale)).round();
    if scale > 0.0 && (0.0..=top as f64).contains(&step) {
        return put_frame(stream, edit.frame, bit, width, step as u64);
    }
    let frames = usize::from(stream.num_frames);
    let mut values = Vec::with_capacity(frames);
    for frame in 0..frames {
        let q = get_frame(stream, frame, bit, width)?;
        values.push(f64::from(scale) * q as f64 + f64::from(offset));
    }
    *values
        .get_mut(edit.frame)
        .ok_or("the block holds no such frame")? = f64::from(edit.value);
    let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (scale, offset) = (((hi - lo) / top as f64) as f32, lo as f32);
    if !(scale.is_finite() && offset.is_finite()) {
        return Err(format!("its range {lo} to {hi} has no finite step"));
    }
    for (frame, v) in values.into_iter().enumerate() {
        let q = if scale > 0.0 {
            ((v - f64::from(offset)) / f64::from(scale))
                .round()
                .clamp(0.0, top as f64)
        } else {
            0.0
        };
        put_frame(stream, frame, bit, width, q as u64)?;
    }
    words[constant + 1] = scale.to_bits();
    words[constant + 2] = offset.to_bits();
    Ok(())
}

/// The bytes of frame `frame` of `stream`, with bits `bit .. bit + width` checked to lie in them.
fn frame_range(
    stream: &CrBlockStream,
    frame: usize,
    bit: usize,
    width: usize,
) -> Result<Range<usize>, String> {
    let size = usize::from(stream.frame_size);
    if frame >= usize::from(stream.num_frames) || bit + width > 8 * size {
        return Err(format!(
            "its bits {bit} to {} of frame {frame} lie past the block's {} frames of {size} bytes",
            bit + width,
            stream.num_frames
        ));
    }
    let at = stream.constant_size as usize + frame * size;
    (at + size <= stream.data.len())
        .then_some(at..at + size)
        .ok_or_else(|| "its frames run past its payload".to_string())
}

fn get_frame(
    stream: &CrBlockStream,
    frame: usize,
    bit: usize,
    width: usize,
) -> Result<u64, String> {
    let bytes = &stream.data[frame_range(stream, frame, bit, width)?];
    Ok((0..width).fold(0, |v, b| {
        let at = bit + b;
        v | (u64::from((bytes[at / 8] >> (at % 8)) & 1) << b)
    }))
}

fn put_frame(
    stream: &mut CrBlockStream,
    frame: usize,
    bit: usize,
    width: usize,
    value: u64,
) -> Result<(), String> {
    let range = frame_range(stream, frame, bit, width)?;
    let bytes = &mut stream.data[range];
    for b in 0..width {
        let (at, one) = (bit + b, ((value >> b) & 1) as u8);
        bytes[at / 8] = (bytes[at / 8] & !(1 << (at % 8))) | (one << (at % 8));
    }
    Ok(())
}

/// Whether a channel of `format` and `channel` holds component `component` as a value a key is
/// written to.
fn holds(format: ChannelFormat, channel: u8, component: u8) -> bool {
    match format {
        ChannelFormat::ConstantQuat | ChannelFormat::ConstantVec => component < 3,
        ChannelFormat::Constant
        | ChannelFormat::Raw
        | ChannelFormat::Quantize
        | ChannelFormat::Indirect => channel == component,
        _ => false,
    }
}

impl Animation {
    /// The `(DOF, component)` pairs a key is written to in some block, in order.
    pub fn key_components(&self) -> Vec<(usize, u8)> {
        let mut pairs = BTreeSet::new();
        for block in &self.blocks {
            let channels = ChannelWalk::new(&block.stream).and_then(|walk| walk.channels());
            for c in channels.unwrap_or_default() {
                let dof = c.dof_index as usize;
                let held = (0..4u8).filter(|k| holds(c.format, c.component, *k));
                pairs.extend(held.map(|k| (dof, k)));
            }
            pairs.extend(block.deltas.iter().map(|d| (d.dof as usize, d.component)));
        }
        pairs.into_iter().collect()
    }

    /// Component `component` of DOF `dof` at `frame`, as the channel a key is written to holds it.
    pub fn key_value(&self, dof: usize, component: u8, frame: u16) -> Option<f32> {
        if let Some(value) = self.delta_value(dof, component, frame) {
            return Some(value);
        }
        let chunk = usize::from(self.frames_per_chunk.max(1));
        let block = (usize::from(frame) / chunk).min(self.blocks.len().checked_sub(1)?);
        let at = usize::from(frame).saturating_sub(block * chunk);
        let walk = ChannelWalk::new(&self.blocks[block].stream).ok()?;
        let channel = (walk.channels_for_dof(dof as u32).ok()?.into_iter())
            .find(|c| holds(c.format, c.component, component))?;
        let samples = walk.evaluate_channels(&[channel], at, 0.0, 4).ok()?;
        samples.components.get(usize::from(component)).copied()
    }

    /// The place in DOF `dof`'s rotation that `frame`'s block rebuilds from the three it holds.
    // The held components shift past that place; none where no channel of the block rebuilds one.
    pub fn rebuilt_component(&self, dof: usize, frame: u16) -> Option<u8> {
        let chunk = usize::from(self.frames_per_chunk.max(1));
        let block = (usize::from(frame) / chunk).min(self.blocks.len().checked_sub(1)?);
        let walk = ChannelWalk::new(&self.blocks[block].stream).ok()?;
        (walk.channels_for_dof(dof as u32).ok()?.into_iter())
            .find(|c| c.format == ChannelFormat::Reconstruct)
            .map(|c| c.component)
    }
}
