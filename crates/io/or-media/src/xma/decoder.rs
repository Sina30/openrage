//! The WMA Pro frame decoder as XMA uses it, ported from FFmpeg.
//!
//! Origin: FFmpeg, `libavcodec/wmaprodec.c`, tag `n7.0` (commit
//! `083443d67cb159ce469e5d902346b8d0c2cd1c93`), with `ff_wma_run_level_decode` and
//! `ff_wma_get_large_val` from `libavcodec/wma.c` and the vector windowing of
//! `libavutil/float_dsp.c`. Licence: LGPL-2.1-or-later.
//! Copyright (c) 2007 Baptiste Coudurier, Benjamin Larsson, Ulion
//! Copyright (c) 2008 - 2011 Sascha Sommer, Benjamin Larsson
//!
//! What is ported is one XMA stream of one or two channels: the 2,048-byte packet header, the bit
//! reservoir a frame spanning packets is assembled in, and each frame's subframes (scale factors,
//! coefficients, channel transform, inverse MDCT, windowing). XMA fixes the WMA Pro flags at `0x10d6`
//! and 512 samples a frame, so what those flags select is written as constants, and what only a
//! plain WMA Pro stream or more than two channels needs is left out. Where FFmpeg marks the stream
//! lost and carries on, this returns an error.

use super::bits::{BitReader, Reservoir};
use super::mdct::Imdct;
use super::tables::{
    COEF0_LENS, COEF0_LEVEL, COEF0_RUN, COEF0_SYMS, COEF1_LEVEL, COEF1_RUN, COEF1_TABLE,
    CRITICAL_FREQ, SCALE_RL_LEVEL, SCALE_RL_RUN, SCALE_RL_TABLE, SCALE_TABLE, VEC1_TABLE,
    VEC2_TABLE, VEC4_LENS, VEC4_SYMS,
};
use super::vlc::Vlc;
use crate::MediaError;

/// Samples a frame decodes to, for every XMA stream.
pub(super) const SAMPLES_PER_FRAME: usize = 512;
/// Bytes of one packet.
pub(super) const PACKET_BYTES: usize = 2048;

const MAX_CHANNELS: usize = 2;
const MAX_SUBFRAMES: usize = 32;
const MAX_BANDS: usize = 29;
/// Bits of a frame's length prefix, a packet's bits being 2^15.
const LOG2_FRAME_SIZE: u32 = 15;
const MAX_FRAMESIZE: usize = 32768;
/// Subframe sizes a frame splits into: 512, 256 and 128 samples.
const BLOCK_SIZES: usize = 3;
const MIN_SAMPLES_PER_SUBFRAME: usize = SAMPLES_PER_FRAME >> (BLOCK_SIZES - 1);
/// Bits that say how a subframe's length is a fraction of the frame's.
const SUBFRAME_LEN_BITS: u32 = 2;
const BITS_PER_SAMPLE: u32 = 16;

/// A fraction of a frame a channel's coefficients or output are taken from, as an offset into `out`.
#[derive(Clone)]
struct Channel {
    prev_block_len: usize,
    transmit_coefs: bool,
    num_subframes: usize,
    subframe_len: [usize; MAX_SUBFRAMES],
    cur_subframe: usize,
    decoded_samples: usize,
    grouped: bool,
    quant_step: i32,
    reuse_sf: bool,
    scale_factor_step: i32,
    max_scale_factor: i32,
    saved_scale_factors: [[i32; MAX_BANDS]; 2],
    scale_factor_idx: usize,
    /// Which of `saved_scale_factors` the subframe being decoded reads.
    scale_factors: usize,
    table_idx: usize,
    /// Where the subframe's coefficients, and then its output, start in `out`.
    coeffs: usize,
    num_vec_coeffs: usize,
    out: Vec<f32>,
}

impl Channel {
    fn new() -> Self {
        Self {
            prev_block_len: SAMPLES_PER_FRAME,
            transmit_coefs: false,
            num_subframes: 0,
            subframe_len: [0; MAX_SUBFRAMES],
            cur_subframe: 0,
            decoded_samples: 0,
            grouped: false,
            quant_step: 0,
            reuse_sf: false,
            scale_factor_step: 0,
            max_scale_factor: 0,
            saved_scale_factors: [[0; MAX_BANDS]; 2],
            scale_factor_idx: 0,
            scale_factors: 0,
            table_idx: 0,
            coeffs: 0,
            num_vec_coeffs: 0,
            out: vec![0.0; SAMPLES_PER_FRAME + SAMPLES_PER_FRAME / 2],
        }
    }
}

/// Channels of the subframe being decoded whose coefficients are transformed together.
#[derive(Clone, Copy, Default)]
struct ChannelGroup {
    num_channels: usize,
    transform: bool,
    transform_band: [bool; MAX_BANDS],
    channels: [usize; MAX_CHANNELS],
}

struct Vlcs {
    scale: Vlc,
    scale_rl: Vlc,
    coef: [Vlc; 2],
    vec4: Vlc,
    vec2: Vlc,
    vec1: Vlc,
}

impl Vlcs {
    fn new() -> Option<Self> {
        let pairs = |table: &[(u8, u8)]| -> (Vec<u8>, Vec<u32>) {
            (
                table.iter().map(|e| e.1).collect(),
                table.iter().map(|e| u32::from(e.0)).collect(),
            )
        };
        let from_pairs = |table: &[(u8, u8)], offset: i32| {
            let (lens, syms) = pairs(table);
            Vlc::from_lengths(lens, syms, offset)
        };
        Some(Self {
            scale: from_pairs(&SCALE_TABLE, -60)?,
            scale_rl: from_pairs(&SCALE_RL_TABLE, 0)?,
            coef: [
                Vlc::from_lengths(COEF0_LENS, COEF0_SYMS.iter().map(|&s| u32::from(s)), 0)?,
                from_pairs(&COEF1_TABLE, 0)?,
            ],
            vec4: Vlc::from_lengths(VEC4_LENS, VEC4_SYMS.iter().map(|&s| u32::from(s)), -1)?,
            vec2: from_pairs(&VEC2_TABLE, -1)?,
            vec1: from_pairs(&VEC1_TABLE, 0)?,
        })
    }
}

/// Everything decoded: every frame's samples per channel, the last frame's overlap included.
pub(super) struct Decoded {
    /// One buffer of samples per channel, 512 for each frame decoded.
    pub frames: Vec<Vec<f32>>,
    /// The overlap the last frame leaves, 256 samples of each channel, which a next frame would finish.
    pub tail: Vec<Vec<f32>>,
    /// Samples the frame headers say to drop at the start and at the end, the last nonzero values stated.
    pub trim: (usize, usize),
}

pub(super) struct Decoder {
    nb_channels: usize,
    vlcs: Vlcs,
    /// Sine windows by `log2` of their length.
    windows: Vec<Vec<f32>>,
    /// Inverse MDCTs of 128, 256 and 512 coefficients.
    imdct: Vec<Imdct>,
    num_sfb: [usize; BLOCK_SIZES],
    sfb_offsets: [[usize; MAX_BANDS]; BLOCK_SIZES],
    sf_offsets: [[[usize; MAX_BANDS]; BLOCK_SIZES]; BLOCK_SIZES],

    reservoir: Reservoir,
    packet_loss: bool,

    channel: [Channel; MAX_CHANNELS],
    chgroup: ChannelGroup,
    num_chgroups: usize,
    subframe_len: usize,
    channels_for_cur_subframe: usize,
    channel_indexes_for_cur_subframe: [usize; MAX_CHANNELS],
    parsed_all_subframes: bool,
    num_bands: usize,
    table_idx: usize,
    esc_len: u32,
    transmit_num_vec_coeffs: bool,
    tmp: Vec<f32>,
    frames: Vec<Vec<f32>>,
    trim: (usize, usize),
}

/// The sample rate the scale factor bands are laid out for, which XMA rounds up to one of four.
fn band_rate(sample_rate: u32) -> usize {
    match sample_rate {
        r if r > 44100 => 48000,
        r if r > 32000 => 44100,
        r if r > 24000 => 32000,
        _ => 24000,
    }
}

fn bad(what: &str) -> MediaError {
    MediaError::Decode(format!("this XMA stream is broken: {what}"))
}

/// `ff_wma_get_large_val`.
fn get_large_val(gb: &mut BitReader<'_>) -> u32 {
    let mut bits = 8;
    if gb.bit() {
        bits += 8;
        if gb.bit() {
            bits += 8;
            if gb.bit() {
                bits += 7;
            }
        }
    }
    gb.get(bits)
}

impl Decoder {
    pub fn new(channels: usize, sample_rate: u32) -> Result<Self, MediaError> {
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err(MediaError::Decode(format!(
                "an XMA stream holds one or two channels, and this states {channels}"
            )));
        }
        if sample_rate == 0 {
            return Err(MediaError::Decode(
                "this XMA stream states a sample rate of 0".to_string(),
            ));
        }
        let vlcs = Vlcs::new()
            .ok_or_else(|| MediaError::Decode("the XMA code tables do not form codes".into()))?;

        let rate = band_rate(sample_rate);
        let mut sfb_offsets = [[0usize; MAX_BANDS]; BLOCK_SIZES];
        let mut num_sfb = [0usize; BLOCK_SIZES];
        for i in 0..BLOCK_SIZES {
            let subframe_len = SAMPLES_PER_FRAME >> i;
            let mut band = 1;
            let mut x = 0;
            while x < MAX_BANDS - 1 && sfb_offsets[i][band - 1] < subframe_len {
                let offset = ((subframe_len * 2 * usize::from(CRITICAL_FREQ[x])) / rate + 2) & !3;
                if offset > sfb_offsets[i][band - 1] {
                    sfb_offsets[i][band] = offset;
                    band += 1;
                }
                if offset >= subframe_len {
                    break;
                }
                x += 1;
            }
            sfb_offsets[i][band - 1] = subframe_len;
            num_sfb[i] = band - 1;
            if num_sfb[i] == 0 {
                return Err(bad("no scale factor bands"));
            }
        }

        let mut sf_offsets = [[[0usize; MAX_BANDS]; BLOCK_SIZES]; BLOCK_SIZES];
        for i in 0..BLOCK_SIZES {
            for b in 0..num_sfb[i] {
                let offset = ((sfb_offsets[i][b] + sfb_offsets[i][b + 1] - 1) << i) >> 1;
                for x in 0..BLOCK_SIZES {
                    let mut v = 0;
                    while v + 1 < MAX_BANDS && (sfb_offsets[x][v + 1] << x) < offset {
                        v += 1;
                    }
                    sf_offsets[i][x][b] = v;
                }
            }
        }

        let windows = (0..=SAMPLES_PER_FRAME.ilog2() as usize)
            .map(|log2| {
                let n = 1usize << log2;
                (0..n)
                    .map(|i| {
                        (((i as f64 + 0.5) * (std::f64::consts::PI / (2.0 * n as f64))) as f32)
                            .sin()
                    })
                    .collect()
            })
            .collect();
        let imdct = (0..BLOCK_SIZES)
            .map(|i| {
                let n = MIN_SAMPLES_PER_SUBFRAME << i;
                Imdct::new(
                    n,
                    (2.0 / n as f64 / f64::from(1u32 << (BITS_PER_SAMPLE - 1))) as f32,
                )
            })
            .collect();

        Ok(Self {
            nb_channels: channels,
            vlcs,
            windows,
            imdct,
            num_sfb,
            sfb_offsets,
            sf_offsets,
            reservoir: Reservoir::default(),
            packet_loss: true,
            channel: [Channel::new(), Channel::new()],
            chgroup: ChannelGroup::default(),
            num_chgroups: 0,
            subframe_len: 0,
            channels_for_cur_subframe: 0,
            channel_indexes_for_cur_subframe: [0; MAX_CHANNELS],
            parsed_all_subframes: false,
            num_bands: 0,
            table_idx: 0,
            esc_len: 0,
            transmit_num_vec_coeffs: false,
            tmp: vec![0.0; SAMPLES_PER_FRAME],
            frames: vec![Vec::new(); channels],
            trim: (0, 0),
        })
    }

    /// Decodes every packet of a stream, `data` being whole packets in order.
    pub fn decode(channels: usize, sample_rate: u32, data: &[u8]) -> Result<Decoded, MediaError> {
        let mut decoder = Self::new(channels, sample_rate)?;
        for packet in data.chunks(PACKET_BYTES) {
            decoder.decode_packet(packet)?;
        }
        let tail = decoder.channel[..channels]
            .iter()
            .map(|c| c.out[..SAMPLES_PER_FRAME / 2].to_vec())
            .collect();
        Ok(Decoded {
            frames: decoder.frames,
            tail,
            trim: decoder.trim,
        })
    }

    /// Subframe length: how many samples the next subframe of the frame holds.
    fn decode_subframe_length(
        &self,
        gb: &mut BitReader<'_>,
        offset: usize,
    ) -> Result<usize, MediaError> {
        if offset == SAMPLES_PER_FRAME - MIN_SAMPLES_PER_SUBFRAME {
            return Ok(MIN_SAMPLES_PER_SUBFRAME);
        }
        if gb.left() < 1 {
            return Err(bad("a subframe length is past the frame's end"));
        }
        // The bit says whether the subframe is of maximum length, as 4 subframes make it.
        let frame_len_shift = if gb.bit() {
            1 + gb.get(SUBFRAME_LEN_BITS - 1) as usize
        } else {
            0
        };
        let subframe_len = SAMPLES_PER_FRAME >> frame_len_shift;
        if !(MIN_SAMPLES_PER_SUBFRAME..=SAMPLES_PER_FRAME).contains(&subframe_len) {
            return Err(bad(&format!("subframe_len {subframe_len}")));
        }
        Ok(subframe_len)
    }

    /// How the frame splits into subframes in each channel.
    fn decode_tilehdr(&mut self, gb: &mut BitReader<'_>) -> Result<(), MediaError> {
        let nb = self.nb_channels;
        let mut num_samples = [0usize; MAX_CHANNELS];
        let mut contains_subframe = [false; MAX_CHANNELS];
        let mut channels_for_cur_subframe = nb;
        let mut min_channel_len = 0usize;

        for channel in self.channel.iter_mut().take(nb) {
            channel.num_subframes = 0;
        }
        // XMA frames of 4 subframes always state whether every channel splits alike.
        let fixed_channel_layout = gb.bit();

        loop {
            for c in 0..nb {
                contains_subframe[c] = num_samples[c] == min_channel_len
                    && (fixed_channel_layout
                        || channels_for_cur_subframe == 1
                        || min_channel_len == SAMPLES_PER_FRAME - MIN_SAMPLES_PER_SUBFRAME
                        || gb.bit());
            }
            let subframe_len = self.decode_subframe_length(gb, min_channel_len)?;

            min_channel_len += subframe_len;
            for c in 0..nb {
                let chan = &mut self.channel[c];
                if contains_subframe[c] {
                    if chan.num_subframes >= MAX_SUBFRAMES {
                        return Err(bad("num subframes > 31"));
                    }
                    chan.subframe_len[chan.num_subframes] = subframe_len;
                    num_samples[c] += subframe_len;
                    chan.num_subframes += 1;
                    if num_samples[c] > SAMPLES_PER_FRAME {
                        return Err(bad("channel len > samples_per_frame"));
                    }
                } else if num_samples[c] <= min_channel_len {
                    if num_samples[c] < min_channel_len {
                        channels_for_cur_subframe = 0;
                        min_channel_len = num_samples[c];
                    }
                    channels_for_cur_subframe += 1;
                }
            }
            if min_channel_len >= SAMPLES_PER_FRAME {
                return Ok(());
            }
        }
    }

    /// Which channels of the subframe are transformed together, and how.
    fn decode_channel_transform(&mut self, gb: &mut BitReader<'_>) -> Result<(), MediaError> {
        self.num_chgroups = 0;
        if self.nb_channels > 1 {
            if gb.bit() {
                return Err(bad("an unsupported channel transform bit"));
            }
            // At most two channels: one group takes every channel the subframe holds.
            let mut group = ChannelGroup {
                num_channels: self.channels_for_cur_subframe,
                ..ChannelGroup::default()
            };
            let mut taken = 0;
            for i in 0..self.channels_for_cur_subframe {
                let idx = self.channel_indexes_for_cur_subframe[i];
                if !self.channel[idx].grouped {
                    group.channels[taken] = idx;
                    taken += 1;
                }
                self.channel[idx].grouped = true;
            }
            if group.num_channels == 2 {
                if gb.bit() {
                    if gb.bit() {
                        return Err(bad("an unknown channel transform type"));
                    }
                } else {
                    group.transform = true;
                }
            }
            if group.transform {
                if !gb.bit() {
                    for band in group.transform_band.iter_mut().take(self.num_bands) {
                        *band = gb.bit();
                    }
                } else {
                    group.transform_band = [true; MAX_BANDS];
                }
            }
            self.chgroup = group;
            self.num_chgroups = usize::from(self.channels_for_cur_subframe > 0);
        }
        Ok(())
    }

    /// The vector-coded and then run-level-coded coefficients of channel `c`'s subframe.
    ///
    /// FFmpeg ignores a run-level error here, leaving what it decoded so far, and so does this.
    fn decode_coeffs(&mut self, gb: &mut BitReader<'_>, c: usize) {
        let vlctable = gb.get(1) as usize;
        let (run, level): (&[u16], &[u8]) = if vlctable == 1 {
            (&COEF1_RUN, &COEF1_LEVEL)
        } else {
            (&COEF0_RUN, &COEF0_LEVEL)
        };
        let subframe_len = self.subframe_len;
        let esc_len = self.esc_len;
        let transmit_num_vec_coeffs = self.transmit_num_vec_coeffs;
        let vlcs = &self.vlcs;
        let ci = &mut self.channel[c];
        let num_vec_coeffs = ci.num_vec_coeffs;
        let coeffs = &mut ci.out[ci.coeffs..ci.coeffs + subframe_len];

        let mut rl_mode = false;
        let mut cur_coeff = 0usize;
        let mut num_zeros = 0usize;

        while (transmit_num_vec_coeffs || !rl_mode) && cur_coeff + 3 < num_vec_coeffs {
            let mut vals = [0.0f32; 4];
            match vlcs.vec4.decode(gb).filter(|&i| i >= 0) {
                None => {
                    for i in (0..4).step_by(2) {
                        match vlcs.vec2.decode(gb).filter(|&i| i >= 0) {
                            None => {
                                let mut v = [0u32; 2];
                                for slot in &mut v {
                                    *slot = vlcs.vec1.decode(gb).unwrap_or(0) as u32;
                                    if *slot == VEC1_TABLE.len() as u32 - 1 {
                                        *slot += get_large_val(gb);
                                    }
                                }
                                vals[i] = v[0] as f32;
                                vals[i + 1] = v[1] as f32;
                            }
                            Some(idx) => {
                                vals[i] = (idx >> 4) as f32;
                                vals[i + 1] = (idx & 0xF) as f32;
                            }
                        }
                    }
                }
                Some(idx) => {
                    vals = [
                        (idx >> 12) as f32,
                        ((idx >> 8) & 0xF) as f32,
                        ((idx >> 4) & 0xF) as f32,
                        (idx & 0xF) as f32,
                    ];
                }
            }
            for v in vals {
                if v != 0.0 {
                    // A set bit is positive.
                    coeffs[cur_coeff] = if gb.bit() { v } else { -v };
                    num_zeros = 0;
                } else {
                    coeffs[cur_coeff] = 0.0;
                    // A run of subframe_len / 256 zeros switches to run-level coding.
                    num_zeros += 1;
                    rl_mode |= num_zeros > subframe_len >> 8;
                }
                cur_coeff += 1;
            }
        }

        if cur_coeff < subframe_len {
            coeffs[cur_coeff..].fill(0.0);
            let vlc = &vlcs.coef[vlctable];
            let mask = subframe_len - 1;
            let mut offset = cur_coeff;
            while offset < subframe_len {
                match vlc.decode(gb) {
                    Some(code) if code > 1 => {
                        let code = code as usize;
                        offset += usize::from(run[code]);
                        let v = f32::from(level[code]);
                        coeffs[offset & mask] = if gb.bit() { v } else { -v };
                    }
                    Some(1) => break,
                    // The escape: a value, then how far the run goes.
                    _ => {
                        let level = get_large_val(gb);
                        if gb.bit() {
                            if gb.bit() {
                                if gb.bit() {
                                    return;
                                }
                                offset += gb.get(esc_len) as usize + 4;
                            } else {
                                offset += gb.get(2) as usize + 1;
                            }
                        }
                        let v = level as f32;
                        coeffs[offset & mask] = if gb.bit() { v } else { -v };
                    }
                }
                offset += 1;
            }
        }
    }

    /// The scale factors of every channel of the subframe.
    fn decode_scale_factors(&mut self, gb: &mut BitReader<'_>) -> Result<(), MediaError> {
        for i in 0..self.channels_for_cur_subframe {
            let c = self.channel_indexes_for_cur_subframe[i];
            let num_bands = self.num_bands;
            let table_idx = self.table_idx;
            let sf_offsets = self.sf_offsets[table_idx];
            let vlcs = &self.vlcs;
            let ch = &mut self.channel[c];

            ch.scale_factors = 1 - ch.scale_factor_idx;
            let (dst, src) = (ch.scale_factors, ch.scale_factor_idx);

            // Resample the last transmitted factors to this subframe's band layout.
            if ch.reuse_sf {
                for (b, &from) in sf_offsets[ch.table_idx][..num_bands].iter().enumerate() {
                    ch.saved_scale_factors[dst][b] = ch.saved_scale_factors[src][from];
                }
            }

            if ch.cur_subframe == 0 || gb.bit() {
                if !ch.reuse_sf {
                    ch.scale_factor_step = gb.get(2) as i32 + 1;
                    let mut val = 45 / ch.scale_factor_step;
                    for slot in ch.saved_scale_factors[dst].iter_mut().take(num_bands) {
                        val += vlcs
                            .scale
                            .decode(gb)
                            .ok_or_else(|| bad("a scale factor code"))?;
                        *slot = val;
                    }
                } else {
                    let mut band = 0;
                    while band < num_bands {
                        let idx = vlcs
                            .scale_rl
                            .decode(gb)
                            .ok_or_else(|| bad("a scale factor run-level code"))?
                            as usize;
                        let (skip, val, sign);
                        if idx == 0 {
                            let code = gb.get(14);
                            val = (code >> 6) as i32;
                            sign = if code & 1 == 1 { 0 } else { -1 };
                            skip = ((code & 0x3f) >> 1) as usize;
                        } else if idx == 1 {
                            break;
                        } else {
                            skip = usize::from(SCALE_RL_RUN[idx]);
                            val = i32::from(SCALE_RL_LEVEL[idx]);
                            sign = if gb.bit() { 0 } else { -1 };
                        }
                        band += skip;
                        if band >= num_bands {
                            return Err(bad("invalid scale factor coding"));
                        }
                        ch.saved_scale_factors[dst][band] += (val ^ sign) - sign;
                        band += 1;
                    }
                }
                ch.scale_factor_idx = 1 - ch.scale_factor_idx;
                ch.table_idx = table_idx;
                ch.reuse_sf = true;
            }

            ch.max_scale_factor = ch.saved_scale_factors[dst][..num_bands]
                .iter()
                .copied()
                .max()
                .unwrap_or(0);
        }
        Ok(())
    }

    /// Undoes the channel transform of the subframe in the bands it is switched on for.
    fn inverse_channel_transform(&mut self) {
        let subframe_len = self.subframe_len;
        let nb = self.nb_channels;
        for _ in 0..self.num_chgroups {
            let group = self.chgroup;
            if !group.transform {
                continue;
            }
            // A pair of channels: the sum-and-difference matrix, as the XMA stream has no other.
            let matrix = [1.0f32, -1.0, 1.0, 1.0];
            let sfb = &self.sfb_offsets[self.table_idx];
            for (b, &on) in group.transform_band[..self.num_bands].iter().enumerate() {
                let end = sfb[b + 1].min(subframe_len);
                if on {
                    for y in sfb[b]..end {
                        let mut data = [0.0f32; MAX_CHANNELS];
                        for (j, d) in data.iter_mut().enumerate().take(group.num_channels) {
                            let ch = &self.channel[group.channels[j]];
                            *d = ch.out[ch.coeffs + y];
                        }
                        for k in 0..group.num_channels {
                            let mut sum = 0.0f32;
                            for (j, d) in data.iter().enumerate().take(group.num_channels) {
                                sum += *d * matrix[k * group.num_channels + j];
                            }
                            let ch = &mut self.channel[group.channels[k]];
                            let at = ch.coeffs + y;
                            ch.out[at] = sum;
                        }
                    }
                } else if nb == 2 {
                    for k in 0..2 {
                        let ch = &mut self.channel[group.channels[k]];
                        for y in sfb[b]..end {
                            let at = ch.coeffs + y;
                            ch.out[at] *= 181.0 / 128.0;
                        }
                    }
                }
            }
        }
    }

    /// Applies the sine window to the overlap of each channel's previous and current subframe.
    fn window(&mut self) {
        for i in 0..self.channels_for_cur_subframe {
            let c = self.channel_indexes_for_cur_subframe[i];
            let ch = &mut self.channel[c];
            let mut winlen = ch.prev_block_len;
            let mut start = ch.coeffs - (winlen >> 1);
            if self.subframe_len < winlen {
                start += (winlen - self.subframe_len) >> 1;
                winlen = self.subframe_len;
            }
            let window = &self.windows[winlen.ilog2() as usize];
            let len = winlen >> 1;
            // `vector_fmul_window`: the first `len` samples overlap the last `len` in reverse.
            for k in 0..len {
                let a = ch.out[start + k];
                let b = ch.out[start + 2 * len - 1 - k];
                let (wi, wj) = (window[k], window[2 * len - 1 - k]);
                ch.out[start + k] = a * wj - b * wi;
                ch.out[start + 2 * len - 1 - k] = a * wi + b * wj;
            }
            ch.prev_block_len = self.subframe_len;
        }
    }

    /// One subframe, the part of the frame the channel with the fewest decoded samples is next to decode.
    fn decode_subframe(&mut self, gb: &mut BitReader<'_>) -> Result<(), MediaError> {
        let nb = self.nb_channels;
        let mut offset = SAMPLES_PER_FRAME;
        let mut subframe_len = SAMPLES_PER_FRAME;
        let mut total_samples = (SAMPLES_PER_FRAME * nb) as isize;
        let mut transmit_coeffs = false;

        for ch in self.channel.iter_mut().take(nb) {
            ch.grouped = false;
            if offset > ch.decoded_samples {
                offset = ch.decoded_samples;
                subframe_len = ch.subframe_len.get(ch.cur_subframe).copied().unwrap_or(0);
            }
        }

        self.channels_for_cur_subframe = 0;
        for i in 0..nb {
            let ch = &mut self.channel[i];
            let cur = ch.subframe_len.get(ch.cur_subframe).copied().unwrap_or(0);
            total_samples -= ch.decoded_samples as isize;
            if offset == ch.decoded_samples && subframe_len == cur {
                total_samples -= cur as isize;
                ch.decoded_samples += cur;
                self.channel_indexes_for_cur_subframe[self.channels_for_cur_subframe] = i;
                self.channels_for_cur_subframe += 1;
            }
        }
        if total_samples == 0 {
            self.parsed_all_subframes = true;
        }
        if self.channels_for_cur_subframe == 0 || !subframe_len.is_power_of_two() {
            return Err(bad("a subframe no channel holds"));
        }

        self.table_idx = (SAMPLES_PER_FRAME / subframe_len).ilog2() as usize;
        if self.table_idx >= BLOCK_SIZES {
            return Err(bad("a subframe shorter than the frame allows"));
        }
        self.num_bands = self.num_sfb[self.table_idx];

        offset += SAMPLES_PER_FRAME >> 1;
        for i in 0..self.channels_for_cur_subframe {
            let c = self.channel_indexes_for_cur_subframe[i];
            self.channel[c].coeffs = offset;
        }
        self.subframe_len = subframe_len;
        self.esc_len = (subframe_len - 1).ilog2() + 1;

        // An extended header, which only says how many bits to skip.
        if gb.bit() {
            let mut num_fill_bits = gb.get(2) as usize;
            if num_fill_bits == 0 {
                let len = gb.get(4);
                num_fill_bits = gb.get_z(len) as usize + 1;
            }
            if gb.pos() + num_fill_bits > gb.len() {
                return Err(bad("an invalid number of fill bits"));
            }
            gb.skip(num_fill_bits);
        }
        if gb.bit() {
            return Err(bad("a reserved bit is set"));
        }

        self.decode_channel_transform(gb)?;

        for i in 0..self.channels_for_cur_subframe {
            let c = self.channel_indexes_for_cur_subframe[i];
            self.channel[c].transmit_coefs = gb.bit();
            transmit_coeffs |= self.channel[c].transmit_coefs;
        }

        if transmit_coeffs {
            let mut quant_step = ((90 * BITS_PER_SAMPLE) >> 4) as i32;

            self.transmit_num_vec_coeffs = gb.bit();
            if self.transmit_num_vec_coeffs {
                let num_bits = (subframe_len.div_ceil(4)).ilog2() + 1;
                for i in 0..self.channels_for_cur_subframe {
                    let c = self.channel_indexes_for_cur_subframe[i];
                    let num_vec_coeffs = (gb.get(num_bits) as usize) << 2;
                    if num_vec_coeffs > subframe_len {
                        return Err(bad(&format!(
                            "num_vec_coeffs {num_vec_coeffs} is too large"
                        )));
                    }
                    self.channel[c].num_vec_coeffs = num_vec_coeffs;
                }
            } else {
                for i in 0..self.channels_for_cur_subframe {
                    let c = self.channel_indexes_for_cur_subframe[i];
                    self.channel[c].num_vec_coeffs = subframe_len;
                }
            }

            let mut step = gb.get_signed(6);
            quant_step += step;
            if step == -32 || step == 31 {
                let sign = if step == 31 { 0 } else { -1 };
                let mut quant = 0;
                while gb.pos() + 5 < gb.len() {
                    step = gb.get(5) as i32;
                    if step != 31 {
                        break;
                    }
                    quant += 31;
                }
                quant_step += ((quant + step) ^ sign) - sign;
            }

            if self.channels_for_cur_subframe == 1 {
                let c = self.channel_indexes_for_cur_subframe[0];
                self.channel[c].quant_step = quant_step;
            } else {
                let modifier_len = gb.get(3);
                for i in 0..self.channels_for_cur_subframe {
                    let c = self.channel_indexes_for_cur_subframe[i];
                    self.channel[c].quant_step = quant_step;
                    if gb.bit() {
                        self.channel[c].quant_step += if modifier_len > 0 {
                            gb.get(modifier_len) as i32 + 1
                        } else {
                            1
                        };
                    }
                }
            }

            self.decode_scale_factors(gb)?;
        }

        for i in 0..self.channels_for_cur_subframe {
            let c = self.channel_indexes_for_cur_subframe[i];
            if self.channel[c].transmit_coefs && gb.pos() < gb.len() {
                self.decode_coeffs(gb, c);
            } else {
                let ch = &mut self.channel[c];
                ch.out[ch.coeffs..ch.coeffs + subframe_len].fill(0.0);
            }
        }

        if transmit_coeffs {
            self.inverse_channel_transform();
            let imdct = &self.imdct[(subframe_len / MIN_SAMPLES_PER_SUBFRAME).ilog2() as usize];
            for i in 0..self.channels_for_cur_subframe {
                let c = self.channel_indexes_for_cur_subframe[i];
                let ch = &mut self.channel[c];
                let sfb = &self.sfb_offsets[self.table_idx];
                // Inverse quantization and rescaling, band by band.
                for b in 0..self.num_bands {
                    let end = sfb[b + 1].min(subframe_len);
                    let exp = ch.quant_step
                        - (ch.max_scale_factor - ch.saved_scale_factors[ch.scale_factors][b])
                            * ch.scale_factor_step;
                    let quant = (std::f64::consts::LOG2_10 * (f64::from(exp) / 20.0)).exp2() as f32;
                    for k in sfb[b]..end {
                        self.tmp[k] = ch.out[ch.coeffs + k] * quant;
                    }
                }
                let at = ch.coeffs;
                imdct.process(
                    &self.tmp[..subframe_len],
                    &mut ch.out[at..at + subframe_len],
                );
            }
        }

        self.window();

        for i in 0..self.channels_for_cur_subframe {
            let c = self.channel_indexes_for_cur_subframe[i];
            let ch = &mut self.channel[c];
            if ch.cur_subframe >= ch.num_subframes {
                return Err(bad("a subframe past the channel's last"));
            }
            ch.cur_subframe += 1;
        }
        Ok(())
    }

    /// One frame from the bits `gb` holds, and whether another follows it in its packet.
    fn decode_frame(&mut self, gb: &mut BitReader<'_>) -> Result<bool, MediaError> {
        let nb = self.nb_channels;
        let len = gb.get(LOG2_FRAME_SIZE) as usize;

        self.decode_tilehdr(gb)?;

        // A post-processing transform, which XMA never applies and only says how to skip.
        if nb > 1 && gb.bit() && gb.bit() {
            gb.skip(4 * nb * nb);
        }
        // The dynamic range compression gain, which the decode does not use.
        gb.skip(8);
        // Samples to drop at the start and at the end of the stream, of which the last nonzero
        // value stated is kept, as FFmpeg's XMA wrapper keeps it.
        if gb.bit() {
            let bits = (SAMPLES_PER_FRAME * 2).ilog2();
            for slot in [&mut self.trim.0, &mut self.trim.1] {
                if gb.bit() {
                    let stated = gb.get(bits) as usize;
                    if stated != 0 {
                        *slot = stated;
                    }
                }
            }
        }

        self.parsed_all_subframes = false;
        for ch in &mut self.channel[..nb] {
            ch.decoded_samples = 0;
            ch.cur_subframe = 0;
            ch.reuse_sf = false;
        }
        while !self.parsed_all_subframes {
            self.decode_subframe(gb)?;
        }

        for (c, frames) in self.frames.iter_mut().enumerate() {
            frames.extend_from_slice(&self.channel[c].out[..SAMPLES_PER_FRAME]);
        }
        // The second half of the overlap is what the next frame's first half is windowed with.
        for ch in &mut self.channel[..nb] {
            ch.out.copy_within(
                SAMPLES_PER_FRAME..SAMPLES_PER_FRAME + SAMPLES_PER_FRAME / 2,
                0,
            );
        }

        if len != gb.pos() + 2 {
            return Err(bad(&format!(
                "a frame of {len} bits whose data ends at {}",
                gb.pos() + 2
            )));
        }
        gb.skip(len - gb.pos() - 1);
        Ok(gb.bit())
    }

    /// One 2,048-byte packet: the end of the frame the last one left unfinished, then its own frames.
    pub fn decode_packet(&mut self, packet: &[u8]) -> Result<(), MediaError> {
        let buf_bit_size = packet.len().min(PACKET_BYTES) * 8;
        let mut gb = BitReader::new(packet, buf_bit_size);

        // The header: frames or sequence number (not needed to decode), the bits of the previous
        // frame this packet carries, and the packets to skip to reach this stream's next.
        gb.skip(6);
        let mut num_bits_prev_frame = gb.get(LOG2_FRAME_SIZE) as usize;
        gb.skip(3 + 8);

        let mut packet_done = false;
        if num_bits_prev_frame > 0 {
            let remaining = buf_bit_size as isize - gb.pos() as isize;
            if num_bits_prev_frame as isize >= remaining {
                num_bits_prev_frame = remaining.max(0) as usize;
                packet_done = true;
            }
            if (self.reservoir.bits() + num_bits_prev_frame).div_ceil(8) > MAX_FRAMESIZE {
                return Err(bad("a frame longer than its buffer"));
            }
            self.reservoir.append(packet, gb.pos(), num_bits_prev_frame);
            gb.skip(num_bits_prev_frame);
            if !self.packet_loss {
                self.decode_saved_frame()?;
            }
        }
        if self.packet_loss {
            self.reservoir.clear();
            self.packet_loss = false;
        }

        while !packet_done {
            let remaining = buf_bit_size as isize - gb.pos() as isize;
            let frame_size = gb.peek(LOG2_FRAME_SIZE) as usize;
            if remaining > LOG2_FRAME_SIZE as isize
                && frame_size != 0
                && frame_size as isize <= remaining
            {
                self.reservoir.clear();
                self.reservoir.append(packet, gb.pos(), frame_size);
                gb.skip(frame_size);
                packet_done = !self.decode_saved_frame()?;
            } else {
                packet_done = true;
            }
        }

        // What is left starts a frame the next packet finishes.
        let remaining = buf_bit_size as isize - gb.pos() as isize;
        if remaining > 0 {
            self.reservoir.clear();
            self.reservoir.append(packet, gb.pos(), remaining as usize);
        }
        Ok(())
    }

    fn decode_saved_frame(&mut self) -> Result<bool, MediaError> {
        let reservoir = std::mem::take(&mut self.reservoir);
        let mut gb = reservoir.reader();
        let more = self.decode_frame(&mut gb);
        self.reservoir = reservoir;
        more
    }
}
