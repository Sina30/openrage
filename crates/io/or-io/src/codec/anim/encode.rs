use crate::error::{IoError, Result};

use super::channel::{ChannelFormat, FORMAT_COUNT};
use super::packed::MASK_TABLE;
use super::{format_type, track, AnimationSink, BlockSource, BLOCK_HEADER_SIZE};

pub const CONSTANT_EPSILON: f32 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncodePolicy {
    #[default]
    Lossless,
    Quantize {
        bits: u32,
    },
}

impl EncodePolicy {
    pub fn quantize_bits(&self) -> Option<u32> {
        match self {
            Self::Lossless => None,
            Self::Quantize { bits } => Some((*bits).clamp(2, 31)),
        }
    }
}

/// One DOF's samples, a column per component, and the format type they hold.
#[derive(Debug, Clone, Default)]
pub struct DofSampleTable {
    pub format: u8,
    pub values: Vec<Vec<f32>>,
}

impl DofSampleTable {
    pub fn from_frames(format: u8, frames: &[[f32; 4]]) -> Self {
        let components = format_type::component_count(format).min(4);
        let mut values = vec![Vec::with_capacity(frames.len()); components];
        for frame in frames {
            for (component, slot) in values.iter_mut().enumerate() {
                slot.push(frame[component]);
            }
        }
        Self { format, values }
    }

    pub fn num_frames(&self) -> usize {
        self.values.first().map(|v| v.len()).unwrap_or(0)
    }

    fn is_rectangular(&self) -> bool {
        let frames = self.num_frames();
        self.values.iter().all(|v| v.len() == frames)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PlannedChannel {
    ConstantQuat {
        dof: u32,
        xyz: [f32; 3],
    },
    ConstantVec {
        dof: u32,
        xyz: [f32; 3],
    },
    Constant {
        dof: u32,
        component: u8,
        value: f32,
    },
    Raw {
        dof: u32,
        component: u8,
        values: Vec<f32>,
    },
    Quantize {
        dof: u32,
        component: u8,
        bits: u32,
        scale: f32,
        offset: f32,
        quantized: Vec<u32>,
    },
    Reconstruct {
        dof: u32,
        component: u8,
    },
}

impl PlannedChannel {
    fn format(&self) -> ChannelFormat {
        match self {
            Self::ConstantQuat { .. } => ChannelFormat::ConstantQuat,
            Self::ConstantVec { .. } => ChannelFormat::ConstantVec,
            Self::Constant { .. } => ChannelFormat::Constant,
            Self::Raw { .. } => ChannelFormat::Raw,
            Self::Quantize { .. } => ChannelFormat::Quantize,
            Self::Reconstruct { .. } => ChannelFormat::Reconstruct,
        }
    }

    fn directory_index(&self) -> u16 {
        let (dof, component) = match self {
            Self::ConstantQuat { dof, .. } | Self::ConstantVec { dof, .. } => (*dof, 0),
            Self::Constant { dof, component, .. }
            | Self::Raw { dof, component, .. }
            | Self::Quantize { dof, component, .. }
            | Self::Reconstruct { dof, component } => (*dof, *component),
        };
        ((dof as u16) << 2) | (component as u16 & 3)
    }
}

#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    bits: Vec<bool>,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bit_len(&self) -> usize {
        self.bits.len()
    }

    pub fn pad_to(&mut self, bit: usize) {
        while self.bits.len() < bit {
            self.bits.push(false);
        }
    }

    pub fn push_bits(&mut self, value: u32, bits: u32) {
        let bits = bits.min(32);
        for i in 0..bits {
            self.bits.push((value >> i) & 1 == 1);
        }
    }

    pub fn push_rice(&mut self, value: i32, divisor: u32) {
        let divisor = divisor.min(32);
        let magnitude = value.unsigned_abs();
        let remainder = magnitude & MASK_TABLE[divisor as usize];
        let quotient = magnitude >> divisor;
        self.push_bits(remainder, divisor);
        for _ in 0..quotient {
            self.bits.push(false);
        }
        self.bits.push(true);
        if magnitude != 0 {
            self.bits.push(value < 0);
        }
    }

    pub fn into_words(self, guard_words: usize) -> Vec<u32> {
        let mut words = vec![0u32; self.bits.len().div_ceil(32) + guard_words];
        for (index, &bit) in self.bits.iter().enumerate() {
            if bit {
                words[index / 32] |= 1 << (index % 32);
            }
        }
        words
    }

    pub fn into_bytes(self, guard_words: usize) -> Vec<u8> {
        self.into_words(guard_words)
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect()
    }
}

/// A block stream the encoder wrote; every header field it leaves out is zero.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EncodedBlock {
    pub constant_size: u32,
    /// The root-motion reference table's offset, counted from the start of the header.
    pub channel_offset: u32,
    pub num_frames: u16,
    pub frame_size: u16,
    pub segment_size: u8,
    pub data: Vec<u8>,
}

impl BlockSource for EncodedBlock {
    fn data(&self) -> &[u8] {
        &self.data
    }
    fn compact_size(&self) -> u32 {
        0
    }
    fn constant_size(&self) -> u32 {
        self.constant_size
    }
    fn num_frames(&self) -> u16 {
        self.num_frames
    }
    fn frame_size(&self) -> u16 {
        self.frame_size
    }
    fn segment_size(&self) -> u8 {
        self.segment_size
    }
}

pub fn encode_block(
    tables: &[DofSampleTable],
    num_frames: u16,
    policy: EncodePolicy,
) -> Result<EncodedBlock> {
    for (index, table) in tables.iter().enumerate() {
        if !table.is_rectangular() {
            return Err(IoError::SerializationFailed {
                offset: 0,
                message: format!(
                    "DOF {index} sample table is ragged: components have different frame counts"
                ),
            });
        }
        if table.num_frames() != usize::from(num_frames) {
            return Err(IoError::SerializationFailed {
                offset: 0,
                message: format!(
                    "DOF {index} has {} frames but the block declares {num_frames}",
                    table.num_frames()
                ),
            });
        }
    }

    let planned = plan_channels(tables, policy);
    assemble(&planned, num_frames)
}

fn plan_channels(tables: &[DofSampleTable], policy: EncodePolicy) -> Vec<PlannedChannel> {
    let mut out = Vec::new();
    for (dof_index, table) in tables.iter().enumerate() {
        let dof = dof_index as u32;
        let components = table.values.len();

        let quaternion = table.format == format_type::QUATERNION && components == 4;
        let reconstructible = quaternion && is_reconstructible(&table.values);

        let encodable = if reconstructible { 3 } else { components };

        if encodable == 3 && (0..3).all(|c| is_constant(&table.values[c])) {
            let xyz = [
                table.values[0].first().copied().unwrap_or(0.0),
                table.values[1].first().copied().unwrap_or(0.0),
                table.values[2].first().copied().unwrap_or(0.0),
            ];
            if quaternion {
                out.push(PlannedChannel::ConstantQuat { dof, xyz });
                continue;
            }
            out.push(PlannedChannel::ConstantVec { dof, xyz });
            continue;
        }

        for (component, values) in table.values.iter().enumerate().take(encodable) {
            let component = component as u8;
            if is_constant(values) {
                out.push(PlannedChannel::Constant {
                    dof,
                    component,
                    value: values.first().copied().unwrap_or(0.0),
                });
                continue;
            }
            match plan_quantize(dof, component, values, policy) {
                Some(channel) => out.push(channel),
                None => out.push(PlannedChannel::Raw {
                    dof,
                    component,
                    values: values.clone(),
                }),
            }
        }

        if reconstructible {
            out.push(PlannedChannel::Reconstruct { dof, component: 3 });
        }
    }
    out
}

fn plan_quantize(
    dof: u32,
    component: u8,
    values: &[f32],
    policy: EncodePolicy,
) -> Option<PlannedChannel> {
    let bits = policy.quantize_bits()?;
    if values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let min = values.iter().copied().fold(f32::INFINITY, f32::min);
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let range = max - min;
    if !range.is_finite() || range <= 0.0 {
        return None;
    }

    let steps = MASK_TABLE[bits as usize] as f32;
    let scale = range / steps;
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let quantized: Vec<u32> = values
        .iter()
        .map(|value| {
            let normalized = ((value - min) / scale).round();
            normalized.clamp(0.0, steps) as u32
        })
        .collect();

    Some(PlannedChannel::Quantize {
        dof,
        component,
        bits,
        scale,
        offset: min,
        quantized,
    })
}

fn is_constant(values: &[f32]) -> bool {
    let Some(first) = values.first().copied() else {
        return true;
    };
    if !first.is_finite() {
        return false;
    }
    values
        .iter()
        .all(|v| v.is_finite() && (*v - first).abs() <= CONSTANT_EPSILON)
}

const MIN_RECONSTRUCTIBLE_W: f32 = 0.05;

fn is_reconstructible(values: &[Vec<f32>]) -> bool {
    let Some(w) = values.get(3) else {
        return false;
    };
    if w.is_empty() {
        return false;
    }
    for (frame, w) in w.iter().enumerate() {
        if !w.is_finite() || *w < MIN_RECONSTRUCTIBLE_W {
            return false;
        }
        let x = values[0].get(frame).copied().unwrap_or(0.0);
        let y = values[1].get(frame).copied().unwrap_or(0.0);
        let z = values[2].get(frame).copied().unwrap_or(0.0);
        let derived = super::channel::reconstruct_component(x, y, z);
        if (derived - *w).abs() > 1e-4 {
            return false;
        }
    }
    true
}

fn assemble(planned: &[PlannedChannel], num_frames: u16) -> Result<EncodedBlock> {
    let mut sections: Vec<Vec<&PlannedChannel>> = vec![Vec::new(); FORMAT_COUNT];
    for channel in planned {
        sections[channel.format() as usize].push(channel);
    }

    let mut constants: Vec<u32> = Vec::new();
    let mut frame_bits_total: u32 = 0;

    for section in &sections {
        for channel in section {
            match channel {
                PlannedChannel::ConstantQuat { xyz, .. }
                | PlannedChannel::ConstantVec { xyz, .. } => {
                    constants.extend(xyz.iter().map(|v| v.to_bits()));
                }
                PlannedChannel::Constant { value, .. } => {
                    constants.push(value.to_bits());
                }
                PlannedChannel::Raw { .. } => {
                    frame_bits_total += 32;
                }
                PlannedChannel::Quantize {
                    bits,
                    scale,
                    offset,
                    ..
                } => {
                    constants.push(*bits);
                    constants.push(scale.to_bits());
                    constants.push(offset.to_bits());
                    frame_bits_total += *bits;
                }
                PlannedChannel::Reconstruct { .. } => {}
            }
        }
    }

    let frame_size_bytes = (frame_bits_total as usize).div_ceil(8);
    let mut variable = BitWriter::new();
    if frame_size_bytes > 0 {
        for frame in 0..usize::from(num_frames) {
            let frame_base = frame * frame_size_bytes * 8;
            let mut bit_address = 0u32;
            for section in &sections {
                for channel in section {
                    match channel {
                        PlannedChannel::Raw { values, .. } => {
                            variable.pad_to(frame_base + bit_address as usize);
                            let value = values.get(frame).copied().unwrap_or(0.0);
                            variable.push_bits(value.to_bits(), 32);
                            bit_address += 32;
                        }
                        PlannedChannel::Quantize {
                            bits, quantized, ..
                        } => {
                            variable.pad_to(frame_base + bit_address as usize);
                            let value = quantized.get(frame).copied().unwrap_or(0);
                            variable.push_bits(value, *bits);
                            bit_address += *bits;
                        }
                        _ => {}
                    }
                }
            }
            variable.pad_to(frame_base + frame_size_bytes * 8);
        }
    }

    let mut data: Vec<u8> = Vec::new();
    for word in &constants {
        data.extend_from_slice(&word.to_le_bytes());
    }
    let constant_size = data.len() as u32;

    let variable_bytes = variable.into_bytes(0);
    let expected = frame_size_bytes * usize::from(num_frames);
    data.extend_from_slice(&variable_bytes[..variable_bytes.len().min(expected)]);
    data.resize(constant_size as usize + expected, 0);

    for section in &sections {
        let count = u16::try_from(section.len()).map_err(|_| IoError::SerializationFailed {
            offset: 0,
            message: format!("{} channels in one section exceeds u16", section.len()),
        })?;
        data.extend_from_slice(&count.to_le_bytes());
    }
    for section in &sections {
        for channel in section {
            data.extend_from_slice(&channel.directory_index().to_le_bytes());
        }
        let padded = (section.len() + 3) & !3;
        for _ in section.len()..padded {
            data.extend_from_slice(&0u16.to_le_bytes());
        }
    }

    // `channel_offset` locates the root-motion reference table, counted from the
    // start of the block **including its 0x20 header** — it is not the channel
    // directory's offset, which is derived. This encoder emits no mover refs, and
    // every real block with none says so by pointing the field exactly at the end
    // of the payload: **7,040 of 48,411** corpus blocks, every one of which also
    // has `num_mover_channels == 0`. Pointed at the directory instead, a reader
    // following the offset finds the tail of the last frame plus the whole
    // directory as a root-motion table — 1,335 of 1,335 re-encoded corpus blocks
    // claimed a table they do not have — and on a block whose directory begins
    // before offset 0x20 (an all-constant animation with a few channels) the
    // offset is not block-relative at all.
    let channel_offset = data.len() as u32 + BLOCK_HEADER_SIZE as u32;

    Ok(EncodedBlock {
        constant_size,
        channel_offset,
        num_frames,
        frame_size: u16::try_from(frame_size_bytes).map_err(|_| IoError::SerializationFailed {
            offset: 0,
            message: format!("frame stride {frame_size_bytes} bytes exceeds u16"),
        })?,
        segment_size: 64,
        data,
    })
}

/// Replaces every block of `animation` with one encoded from `sampler(dof, frame)`.
pub fn reencode_animation<A: AnimationSink + ?Sized>(
    animation: &mut A,
    policy: EncodePolicy,
    mut sampler: impl FnMut(usize, usize) -> [f32; 4],
) -> Result<()> {
    let num_frames = animation.num_frames();
    if num_frames == 0 {
        return Err(IoError::SerializationFailed {
            offset: 0,
            message: "cannot encode an animation with no frames".to_string(),
        });
    }
    if animation.dof_count() == 0 {
        return Err(IoError::SerializationFailed {
            offset: 0,
            message: "cannot encode an animation with no DOFs".to_string(),
        });
    }

    let formats: Vec<u8> = (0..animation.dof_count())
        .map(|dof| animation.dof_format(dof).unwrap_or(format_type::FLOAT))
        .collect();
    let block_count = animation.calc_num_blocks();
    let chunk = usize::from(animation.frames_per_chunk().max(1));
    let mut blocks = Vec::with_capacity(block_count);

    for block_index in 0..block_count {
        let frames_in_block = animation.frames_in_block(block_index);
        let first_frame = block_index * chunk;

        let tables: Vec<DofSampleTable> = formats
            .iter()
            .enumerate()
            .map(|(dof_index, &format)| {
                let frames: Vec<[f32; 4]> = (0..frames_in_block)
                    .map(|local| {
                        let global = (first_frame + local).min(usize::from(num_frames) - 1);
                        sampler(dof_index, global)
                    })
                    .collect();
                DofSampleTable::from_frames(format, &frames)
            })
            .collect();

        let frames = u16::try_from(frames_in_block).map_err(|_| IoError::SerializationFailed {
            offset: 0,
            message: format!("{frames_in_block} frames in one block exceeds u16"),
        })?;
        blocks.push(encode_block(&tables, frames, policy)?);
    }

    let max_block_size = blocks
        .iter()
        .map(|b| b.data.len() as u32)
        .max()
        .unwrap_or(0);
    animation.store_blocks(blocks, max_block_size);
    Ok(())
}

/// The format type a bone or mover DOF on `track_id` holds.
pub fn format_of_track(track_id: u8) -> u8 {
    match track_id {
        track::BONE_ROTATION | track::MOVER_ROTATION => format_type::QUATERNION,
        track::BONE_TRANSLATION | track::BONE_SCALE | track::MOVER_TRANSLATION => {
            format_type::VECTOR3
        }
        _ => format_type::FLOAT,
    }
}

#[cfg(all(test, feature = "gta5"))]
mod tests {
    use super::super::{decode_dof, AnimationSource, ChannelWalk, COMPACT};
    use super::*;
    use crate::gta5::{CrAnimation, Dof};

    fn decode_all(animation: &CrAnimation, dof: usize) -> Vec<[f32; 4]> {
        (0..animation.num_frames)
            .map(|frame| {
                let time = animation.internal_frame_to_time(frame as f32);
                decode_dof(animation, dof, time)
                    .expect("decode")
                    .expect("samples")
                    .components
            })
            .collect()
    }

    fn animation_with(block: EncodedBlock, dofs: Vec<Dof>, frames: u16) -> CrAnimation {
        CrAnimation {
            num_frames: frames,
            frames_per_chunk: 127,
            duration: 1.0,
            dofs,
            max_block_size: block.data.len() as u32,
            blocks: vec![Some(Box::new(block.into()))],
            ..CrAnimation::default()
        }
    }

    fn dof(id: u16, format: u8, track: u8) -> Dof {
        Dof {
            id,
            r#type: format,
            track,
            ..Dof::default()
        }
    }

    fn vector_dof(tag: u16) -> Dof {
        dof(tag, format_type::VECTOR3, track::BONE_TRANSLATION)
    }

    fn quat_dof(tag: u16) -> Dof {
        dof(tag, format_type::QUATERNION, track::BONE_ROTATION)
    }

    #[test]
    fn bit_writer_is_the_inverse_of_packed_array() {
        use super::super::packed::PackedArray;
        for bits in [1u32, 3, 7, 8, 11, 16, 17, 31, 32] {
            let values: Vec<u32> = (0..40u32)
                .map(|i| i.wrapping_mul(2_654_435_761) & MASK_TABLE[bits as usize])
                .collect();
            let mut writer = BitWriter::new();
            for value in &values {
                writer.push_bits(*value, bits);
            }
            let words = writer.into_words(1);
            let array = PackedArray::new(&words);
            for (index, expected) in values.iter().enumerate() {
                assert_eq!(
                    array.get(index as u32 * bits, bits).expect("read"),
                    *expected,
                    "bits = {bits}, index = {index}"
                );
            }
        }
    }

    #[test]
    fn rice_round_trips_through_packed_sequence() {
        use super::super::packed::PackedSequence;
        for divisor in [1u32, 2, 4, 6] {
            let values: Vec<i32> = vec![0, 1, -1, 5, -5, 17, -17, 63, -63];
            let mut writer = BitWriter::new();
            for value in &values {
                writer.push_rice(*value, divisor);
            }
            let words = writer.into_words(2);
            let mut sequence = PackedSequence::new(&words, 0);
            for expected in &values {
                assert_eq!(
                    sequence.next_value(divisor).expect("decode"),
                    *expected,
                    "divisor = {divisor}"
                );
            }
        }
    }

    #[test]
    fn rice_encoding_of_zero_carries_no_sign_bit() {
        let mut with_zero = BitWriter::new();
        with_zero.push_rice(0, 3);
        assert_eq!(with_zero.bit_len(), 4);

        let mut with_one = BitWriter::new();
        with_one.push_rice(1, 3);
        assert_eq!(with_one.bit_len(), 5);
    }

    #[test]
    fn guard_words_are_appended() {
        let mut writer = BitWriter::new();
        writer.push_bits(0xFFFF_FFFF, 32);
        assert_eq!(writer.clone().into_words(0).len(), 1);
        assert_eq!(writer.clone().into_words(1).len(), 2);
        assert_eq!(writer.into_bytes(2).len(), 12);
    }

    #[test]
    fn padding_moves_the_cursor_without_writing() {
        let mut writer = BitWriter::new();
        writer.pad_to(40);
        writer.push_bits(1, 1);
        assert_eq!(writer.bit_len(), 41);
        let words = writer.into_words(0);
        assert_eq!(words[0], 0);
        assert_eq!(words[1], 1 << 8);
    }

    #[test]
    fn a_flat_vector_channel_becomes_a_constant_vector() {
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &[[1.0, 2.0, 3.0, 0.0]; 8]);
        let planned = plan_channels(&[table], EncodePolicy::Lossless);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].format(), ChannelFormat::ConstantVec);
    }

    #[test]
    fn a_moving_vector_channel_becomes_raw_per_component() {
        let frames: Vec<[f32; 4]> = (0..8).map(|i| [i as f32, 2.0, 3.0, 0.0]).collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let planned = plan_channels(&[table], EncodePolicy::Lossless);
        assert_eq!(planned.len(), 3);
        assert_eq!(planned[0].format(), ChannelFormat::Raw);
        assert_eq!(planned[1].format(), ChannelFormat::Constant);
        assert_eq!(planned[2].format(), ChannelFormat::Constant);
    }

    #[test]
    fn the_quantize_policy_replaces_raw() {
        let frames: Vec<[f32; 4]> = (0..8).map(|i| [i as f32, 0.0, 0.0, 0.0]).collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let planned = plan_channels(&[table], EncodePolicy::Quantize { bits: 12 });
        assert_eq!(planned[0].format(), ChannelFormat::Quantize);
    }

    #[test]
    fn a_non_finite_channel_falls_back_to_raw_even_under_quantize() {
        let frames = vec![
            [0.0, 0.0, 0.0, 0.0],
            [f32::NAN, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ];
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let planned = plan_channels(&[table], EncodePolicy::Quantize { bits: 12 });
        assert_eq!(planned[0].format(), ChannelFormat::Raw);
    }

    #[test]
    fn quantize_bits_are_clamped_to_what_the_format_can_store() {
        assert_eq!(EncodePolicy::Quantize { bits: 0 }.quantize_bits(), Some(2));
        assert_eq!(
            EncodePolicy::Quantize { bits: 99 }.quantize_bits(),
            Some(31)
        );
        assert_eq!(
            EncodePolicy::Quantize { bits: 12 }.quantize_bits(),
            Some(12)
        );
        assert_eq!(EncodePolicy::Lossless.quantize_bits(), None);
    }

    #[test]
    fn a_negative_w_quaternion_is_not_reconstructible() {
        let positive = vec![vec![0.0], vec![0.0], vec![0.0], vec![1.0]];
        assert!(is_reconstructible(&positive));

        let negative = vec![vec![0.0], vec![0.0], vec![0.0], vec![-1.0]];
        assert!(!is_reconstructible(&negative));
    }

    #[test]
    fn a_near_zero_w_quaternion_is_not_reconstructible() {
        let w = 1.2e-6f32;
        let x = (1.0f32 - w * w).sqrt();
        let values = vec![vec![x], vec![0.0], vec![0.0], vec![w]];
        assert!(
            !is_reconstructible(&values),
            "reconstruct was accepted at w = {w}, where sqrt has no precision"
        );

        let w = 0.2f32;
        let x = (1.0f32 - w * w).sqrt();
        let values = vec![vec![x], vec![0.0], vec![0.0], vec![w]];
        assert!(is_reconstructible(&values));
    }

    #[test]
    fn a_near_singular_quaternion_round_trips_via_raw() {
        let frames: Vec<[f32; 4]> = (0..8)
            .map(|i| {
                let w = 1e-6 + i as f32 * 4.0 * CONSTANT_EPSILON;
                let x = (1.0f32 - w * w).sqrt();
                [x, 0.0, 0.0, w]
            })
            .collect();
        let table = DofSampleTable::from_frames(format_type::QUATERNION, &frames);
        let planned = plan_channels(std::slice::from_ref(&table), EncodePolicy::Lossless);
        assert!(!planned
            .iter()
            .any(|c| c.format() == ChannelFormat::Reconstruct));

        let block = encode_block(&[table], 8, EncodePolicy::Lossless).expect("encode");
        let animation = animation_with(block, vec![quat_dof(200)], 8);
        for (index, (got, want)) in decode_all(&animation, 0)
            .iter()
            .zip(frames.iter())
            .enumerate()
        {
            assert!(
                (got[3] - want[3]).abs() < 1e-9,
                "frame {index}: w {} vs {}",
                got[3],
                want[3]
            );
        }
    }

    #[test]
    fn a_quaternion_varying_within_epsilon_becomes_a_constant_channel() {
        let frames: Vec<[f32; 4]> = (0..8)
            .map(|i| {
                let w = 1e-6 + i as f32 * 1e-7;
                let x = (1.0f32 - w * w).sqrt();
                [x, 0.0, 0.0, w]
            })
            .collect();

        let table = DofSampleTable::from_frames(format_type::QUATERNION, &frames);
        assert!(
            is_constant(&table.values[3]),
            "a spread of 7e-7 is inside the epsilon"
        );

        let block = encode_block(&[table], 8, EncodePolicy::Lossless).expect("encode");
        let animation = animation_with(block, vec![quat_dof(200)], 8);

        let decoded = decode_all(&animation, 0);
        let first = decoded[0][3];
        assert!(decoded.iter().all(|f| f[3] == first));
        assert!((first - frames[0][3]).abs() <= CONSTANT_EPSILON);
    }

    #[test]
    fn a_non_unit_quaternion_is_not_reconstructible() {
        let values = vec![vec![0.0], vec![0.0], vec![0.0], vec![0.5]];
        assert!(!is_reconstructible(&values));
    }

    #[test]
    fn constant_detection_rejects_non_finite_values() {
        assert!(is_constant(&[1.0, 1.0, 1.0]));
        assert!(is_constant(&[]));
        assert!(!is_constant(&[f32::NAN, f32::NAN]));
        assert!(!is_constant(&[1.0, 2.0]));
        assert!(is_constant(&[1.0, 1.0 + CONSTANT_EPSILON * 0.5]));
    }

    #[test]
    fn a_constant_vector_round_trips_exactly() {
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &[[1.5, -2.5, 3.5, 0.0]; 4]);
        let block = encode_block(&[table], 4, EncodePolicy::Lossless).expect("encode");
        let animation = animation_with(block, vec![vector_dof(100)], 4);

        for frame in decode_all(&animation, 0) {
            assert!((frame[0] - 1.5).abs() < 1e-6, "{frame:?}");
            assert!((frame[1] + 2.5).abs() < 1e-6, "{frame:?}");
            assert!((frame[2] - 3.5).abs() < 1e-6, "{frame:?}");
        }
    }

    #[test]
    fn a_raw_channel_round_trips_exactly() {
        let frames: Vec<[f32; 4]> = (0..16)
            .map(|i| [i as f32 * 0.125, -(i as f32), 7.0, 0.0])
            .collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let block = encode_block(&[table], 16, EncodePolicy::Lossless).expect("encode");
        let animation = animation_with(block, vec![vector_dof(100)], 16);

        let decoded = decode_all(&animation, 0);
        assert_eq!(decoded.len(), 16);
        for (index, (got, want)) in decoded.iter().zip(frames.iter()).enumerate() {
            assert!(
                (got[0] - want[0]).abs() < 1e-6,
                "frame {index}: x {got:?} vs {want:?}"
            );
            assert!((got[1] - want[1]).abs() < 1e-6, "frame {index}: y");
            assert!((got[2] - want[2]).abs() < 1e-6, "frame {index}: z");
        }
    }

    #[test]
    fn a_quantized_channel_round_trips_within_its_step() {
        let bits = 10u32;
        let frames: Vec<[f32; 4]> = (0..32)
            .map(|i| [(i as f32) * 0.5 - 8.0, 0.0, 0.0, 0.0])
            .collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let block = encode_block(&[table], 32, EncodePolicy::Quantize { bits }).expect("encode");
        let animation = animation_with(block, vec![vector_dof(100)], 32);

        let range = 0.5 * 31.0;
        let step = range / MASK_TABLE[bits as usize] as f32;
        for (index, (got, want)) in decode_all(&animation, 0)
            .iter()
            .zip(frames.iter())
            .enumerate()
        {
            assert!(
                (got[0] - want[0]).abs() <= step,
                "frame {index}: {} vs {} (step {step})",
                got[0],
                want[0]
            );
        }
    }

    #[test]
    fn a_reconstructed_quaternion_round_trips() {
        let frames: Vec<[f32; 4]> = (0..8)
            .map(|i| {
                let angle = i as f32 * 0.1;
                let (s, c) = (angle * 0.5).sin_cos();
                [s, 0.0, 0.0, c]
            })
            .collect();
        let table = DofSampleTable::from_frames(format_type::QUATERNION, &frames);
        let block = encode_block(&[table], 8, EncodePolicy::Lossless).expect("encode");
        let animation = animation_with(block, vec![quat_dof(200)], 8);

        for (index, (got, want)) in decode_all(&animation, 0)
            .iter()
            .zip(frames.iter())
            .enumerate()
        {
            assert!((got[0] - want[0]).abs() < 1e-5, "frame {index}: x");
            assert!(
                (got[3] - want[3]).abs() < 1e-3,
                "frame {index}: w {} vs {}",
                got[3],
                want[3]
            );
        }
    }

    #[test]
    fn a_constant_quaternion_round_trips() {
        let frames = vec![[0.0f32, 0.0, 0.0, 1.0]; 4];
        let table = DofSampleTable::from_frames(format_type::QUATERNION, &frames);
        let planned = plan_channels(std::slice::from_ref(&table), EncodePolicy::Lossless);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].format(), ChannelFormat::ConstantQuat);

        let block = encode_block(&[table], 4, EncodePolicy::Lossless).expect("encode");
        let animation = animation_with(block, vec![quat_dof(200)], 4);
        for frame in decode_all(&animation, 0) {
            assert!((frame[3] - 1.0).abs() < 1e-6, "{frame:?}");
        }
    }

    #[test]
    fn multiple_dofs_in_one_block_all_decode() {
        let moving: Vec<[f32; 4]> = (0..8)
            .map(|i| [i as f32, i as f32 * 2.0, 0.0, 0.0])
            .collect();
        let tables = vec![
            DofSampleTable::from_frames(format_type::VECTOR3, &moving),
            DofSampleTable::from_frames(format_type::VECTOR3, &[[9.0, 8.0, 7.0, 0.0]; 8]),
            DofSampleTable::from_frames(format_type::VECTOR3, &moving),
        ];
        let dofs = vec![
            vector_dof(100),
            vector_dof(200),
            dof(300, format_type::VECTOR3, track::BONE_SCALE),
        ];
        let block = encode_block(&tables, 8, EncodePolicy::Lossless).expect("encode");
        let animation = animation_with(block, dofs, 8);

        let first = decode_all(&animation, 0);
        assert!((first[4][0] - 4.0).abs() < 1e-5, "{:?}", first[4]);
        let second = decode_all(&animation, 1);
        assert!((second[0][0] - 9.0).abs() < 1e-6);
        assert!((second[7][2] - 7.0).abs() < 1e-6);
        let third = decode_all(&animation, 2);
        assert!((third[4][1] - 8.0).abs() < 1e-5, "{:?}", third[4]);
    }

    #[test]
    fn mixed_raw_and_quantized_channels_coexist() {
        let clean: Vec<[f32; 4]> = (0..8).map(|i| [i as f32, i as f32, 0.0, 0.0]).collect();
        let dirty: Vec<[f32; 4]> = (0..8)
            .map(|i| [if i == 3 { f32::NAN } else { i as f32 }, 0.0, 0.0, 0.0])
            .collect();
        let tables = vec![
            DofSampleTable::from_frames(format_type::VECTOR3, &clean),
            DofSampleTable::from_frames(format_type::VECTOR3, &dirty),
        ];
        let dofs = vec![vector_dof(100), vector_dof(200)];
        let block = encode_block(&tables, 8, EncodePolicy::Quantize { bits: 12 }).expect("encode");
        let animation = animation_with(block, dofs, 8);

        let first = decode_all(&animation, 0);
        assert!((first[5][0] - 5.0).abs() < 0.05, "{:?}", first[5]);
        let second = decode_all(&animation, 1);
        assert!((second[5][0] - 5.0).abs() < 1e-6, "{:?}", second[5]);
    }

    #[test]
    fn directory_index_arrays_are_padded_to_four_entries() {
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &[[1.0, 2.0, 3.0, 0.0]; 2]);
        let block = encode_block(&[table], 2, EncodePolicy::Lossless).expect("encode");

        let directory = &block.data[block.channel_directory_offset()..];
        let counts: Vec<u16> = directory[..FORMAT_COUNT * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(counts[ChannelFormat::ConstantVec as usize], 1);

        let mut expected_u16s = FORMAT_COUNT;
        for count in &counts {
            expected_u16s += (usize::from(*count) + 3) & !3;
        }
        assert_eq!(directory.len(), expected_u16s * 2);
    }

    #[test]
    fn a_directory_index_packs_the_dof_and_component() {
        let channel = PlannedChannel::Raw {
            dof: 5,
            component: 2,
            values: vec![0.0],
        };
        assert_eq!(channel.directory_index(), (5 << 2) | 2);
        let constant = PlannedChannel::ConstantVec {
            dof: 7,
            xyz: [0.0; 3],
        };
        assert_eq!(constant.directory_index(), 7 << 2);
    }

    #[test]
    fn the_encoded_block_is_walkable_by_the_decoder() {
        let frames: Vec<[f32; 4]> = (0..6).map(|i| [i as f32, 1.0, 2.0, 0.0]).collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let block = encode_block(&[table], 6, EncodePolicy::Lossless).expect("encode");

        let walk = ChannelWalk::new(&block).expect("walk");
        let channels = walk.channels().expect("channels");
        assert_eq!(channels.len(), 3);
        assert!(channels
            .iter()
            .any(|c| c.format == ChannelFormat::Raw && c.component == 0));
    }

    #[test]
    fn frame_size_covers_every_per_frame_channel() {
        let frames: Vec<[f32; 4]> = (0..4)
            .map(|i| [i as f32, i as f32 + 1.0, i as f32 + 2.0, 0.0])
            .collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let block = encode_block(&[table], 4, EncodePolicy::Lossless).expect("encode");
        assert_eq!(block.frame_size, 12);
    }

    #[test]
    fn a_fully_constant_block_has_no_variable_section() {
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &[[1.0, 2.0, 3.0, 0.0]; 4]);
        let block = encode_block(&[table], 4, EncodePolicy::Lossless).expect("encode");
        assert_eq!(block.frame_size, 0);
        assert_eq!(block.constant_size, 12);
    }

    #[test]
    fn encoded_blocks_are_never_marked_compact() {
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &[[1.0, 2.0, 3.0, 0.0]; 2]);
        let block = encode_block(&[table], 2, EncodePolicy::Lossless).expect("encode");
        assert!(!block.is_compact());
        assert_eq!(block.compact_size(), 0);
    }

    #[test]
    fn a_ragged_sample_table_is_rejected() {
        let table = DofSampleTable {
            format: format_type::VECTOR3,
            values: vec![vec![0.0, 1.0], vec![0.0], vec![0.0, 1.0]],
        };
        assert!(encode_block(&[table], 2, EncodePolicy::Lossless).is_err());
    }

    #[test]
    fn a_frame_count_mismatch_is_rejected() {
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &[[0.0; 4]; 3]);
        let err = encode_block(&[table], 8, EncodePolicy::Lossless).unwrap_err();
        assert!(format!("{err}").contains("frames"));
    }

    #[test]
    fn an_empty_block_encodes_to_a_directory_only_payload() {
        let block = encode_block(&[], 2, EncodePolicy::Lossless).expect("encode");
        assert_eq!(block.constant_size, 0);
        assert_eq!(block.frame_size, 0);
        let walk = ChannelWalk::new(&block).expect("walk");
        assert!(walk.channels().expect("channels").is_empty());
    }

    #[test]
    fn reencoding_an_animation_replaces_every_block() {
        let mut animation = CrAnimation {
            num_frames: 300,
            frames_per_chunk: 100,
            duration: 5.0,
            dofs: vec![vector_dof(100), quat_dof(200)],
            ..CrAnimation::default()
        };
        let expected_blocks = animation.calc_num_blocks();
        assert!(expected_blocks > 1, "test needs multiple blocks");

        reencode_animation(&mut animation, EncodePolicy::Lossless, |dof, frame| {
            if dof == 0 {
                [frame as f32 * 0.01, 1.0, 2.0, 0.0]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            }
        })
        .expect("reencode");

        assert_eq!(animation.blocks.len(), expected_blocks);
        assert!(animation.max_block_size > 0);
        assert_eq!(
            animation.max_block_size,
            animation
                .blocks
                .iter()
                .flatten()
                .map(|b| b.data.len() as u32)
                .max()
                .unwrap()
        );
        assert_eq!(animation.flags & COMPACT, 0);

        for frame in [0u16, 50, 150, 299] {
            let time = animation.internal_frame_to_time(frame as f32);
            let samples = decode_dof(&animation, 0, time)
                .expect("decode")
                .expect("samples");
            let expected = frame as f32 * 0.01;
            assert!(
                (samples.components[0] - expected).abs() < 0.02,
                "frame {frame}: {} vs {expected}",
                samples.components[0]
            );
        }
    }

    #[test]
    fn reencoding_refuses_a_degenerate_animation() {
        let mut no_frames = CrAnimation {
            num_frames: 0,
            dofs: vec![vector_dof(100)],
            ..CrAnimation::default()
        };
        assert!(
            reencode_animation(&mut no_frames, EncodePolicy::Lossless, |_, _| [0.0; 4]).is_err()
        );

        let mut no_dofs = CrAnimation {
            num_frames: 10,
            frames_per_chunk: 127,
            ..CrAnimation::default()
        };
        assert!(reencode_animation(&mut no_dofs, EncodePolicy::Lossless, |_, _| [0.0; 4]).is_err());
    }

    #[test]
    fn a_track_picks_its_format() {
        assert_eq!(
            format_of_track(track::BONE_ROTATION),
            format_type::QUATERNION
        );
        assert_eq!(
            format_of_track(track::BONE_TRANSLATION),
            format_type::VECTOR3
        );
        assert_eq!(
            format_of_track(track::MOVER_ROTATION),
            format_type::QUATERNION
        );
        assert_eq!(format_of_track(track::VISIBILITY), format_type::FLOAT);
    }

    #[test]
    fn encoding_is_deterministic() {
        let frames: Vec<[f32; 4]> = (0..12).map(|i| [i as f32, 1.0, 2.0, 0.0]).collect();
        let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
        let first =
            encode_block(std::slice::from_ref(&table), 12, EncodePolicy::Lossless).expect("encode");
        let second = encode_block(&[table], 12, EncodePolicy::Lossless).expect("encode");
        assert_eq!(first.data, second.data);
        assert_eq!(first.frame_size, second.frame_size);
        assert_eq!(first.constant_size, second.constant_size);
    }
}
