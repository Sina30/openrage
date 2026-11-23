//! GTA VI's Opus streams decoded and encoded, at 48 kHz in 20 ms frames; the only file that calls
//! the `ropus` codec, so another codec replaces it here alone.
//!
//! ⚠ A decode drops the first [`PRE_SKIP`] samples of each channel as the encoder's delay, which
//! is inferred, not stated by the container, and an encode is refused unless its delay is the same.

use or_io::awc::ogg::PRE_SKIP;
use or_io::awc::opus::{split_packets, OpusStream, OPUS_FRAME};
use or_io::awc::replace::{loudness_windows, opus_peak_windows, LOUDNESS_STEP, LOUDNESS_WINDOW};
use or_io::awc::{AwcChunkKind, AwcFile};
use ropus::{
    Application, Bitrate, Channels, DecodeMode, Decoder, Encoder, ForceChannels, FrameDuration,
};

use crate::{AudioInfo, AudioStream, MediaError, Pcm, Samples};

mod blocks;

/// The rate every Opus stream decodes and encodes at.
const RATE: u32 = 48_000;

/// The longest Opus packet a 2,048-byte packet holds beside its `u16` length.
const LONGEST_PACKET: usize = 2046;

/// Samples of each channel an encode adds past the last, as every shipped stream holds 1,920 to
/// 2,879 past it: the encoder's delay and the last frame's padding.
const PADDING: usize = 1920;

/// The bit rates a replaced stream is encoded between, in bits per second.
const BITRATES: (u64, u64) = (6_000, 510_000);

/// Sample frames one peak window covers.
const PEAK_FRAMES: usize = 4096;

/// The level, in LUFS, of a window at or below BS.1770's absolute gate, silence included.
const GATE_LUFS: f64 = -70.0;

/// BS.1770's K-weighting at 48 kHz, its high shelf then its high pass, as `[b0, b1, b2, a1, a2]`.
const SHELF: [f64; 5] = [
    1.53512485958697,
    -2.69169618940638,
    1.19839281085285,
    -1.69065929318241,
    0.73248077421585,
];
const HIGH_PASS: [f64; 5] = [1.0, -2.0, 1.0, -1.99004745483398, 0.99007225036621];

/// How a stream is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Bits per second.
    pub bitrate: u32,
    /// Speech coding, SILK, rather than music coding, CELT or hybrid.
    pub voice: bool,
}

impl Settings {
    /// What a stream's packets show it was encoded with: its coding and its mean bit rate.
    pub fn of(stream: &OpusStream) -> Self {
        // TOC configurations 0 to 11 are SILK alone.
        let voice = (stream.packets.iter()).all(|p| p.first().is_some_and(|toc| toc >> 3 < 12));
        let bytes: u64 = stream.packets.iter().map(|p| p.len() as u64).sum();
        // 50 frames of 20 ms make a second.
        let mean = bytes * 8 * 50 / (stream.packets.len() as u64).max(1);
        Self {
            bitrate: mean.clamp(BITRATES.0, BITRATES.1) as u32,
            voice,
        }
    }
}

fn layout(channels: u8) -> Result<Channels, MediaError> {
    match channels {
        1 => Ok(Channels::Mono),
        2 => Ok(Channels::Stereo),
        n => Err(MediaError::Decode(format!(
            "an Opus stream of {n} channels is neither mono nor stereo"
        ))),
    }
}

/// Told how many of a work's steps are done, of how many; `false` stops the work with an error.
pub type Watch<'a> = &'a dyn Fn(u64, u64) -> bool;

fn stopped() -> MediaError {
    MediaError::Decode("the work was stopped".to_string())
}

/// A tick per step of a work of `total` steps, each told to `watch`.
pub(crate) fn ticks<'a>(watch: Watch<'a>, total: u64) -> impl Fn() -> bool + 'a {
    let done = std::cell::Cell::new(0u64);
    move || {
        done.set(done.get() + 1);
        watch(done.get(), total)
    }
}

/// A stream's samples, interleaved: the pre-skip dropped and cut to the samples it states.
pub fn decode(stream: &OpusStream) -> Result<Vec<i16>, MediaError> {
    decode_ticked(stream, &|| true)
}

/// [`decode`], `tick` called after each packet; its `false` stops the decode.
pub(crate) fn decode_ticked(
    stream: &OpusStream,
    tick: &dyn Fn() -> bool,
) -> Result<Vec<i16>, MediaError> {
    let k = usize::from(stream.channels);
    let mut decoder = Decoder::new(RATE, layout(stream.channels)?)
        .map_err(|e| MediaError::Decode(format!("the Opus decoder did not start: {e}")))?;
    let frame = OPUS_FRAME as usize * k;
    let skip = usize::from(PRE_SKIP) * k;
    let want = stream.samples as usize * k;
    if stream.packets.len().saturating_mul(frame) < skip + want {
        return Err(MediaError::Decode(format!(
            "{} Opus frames hold fewer than the {} samples the stream states and the {PRE_SKIP} \
             the decode drops",
            stream.packets.len(),
            stream.samples
        )));
    }
    // Grown by what decodes, never sized by a count the file states.
    let mut out = Vec::new();
    let mut buffer = vec![0i16; frame];
    for packet in &stream.packets {
        if out.len() >= skip + want {
            break;
        }
        if packet.is_empty() {
            return Err(MediaError::Decode(
                "an Opus packet of no bytes has no frame to decode".to_string(),
            ));
        }
        let decoded = decoder
            .decode(packet, &mut buffer, DecodeMode::Normal)
            .map_err(|e| MediaError::Decode(format!("an Opus packet does not decode: {e}")))?;
        if decoded != OPUS_FRAME as usize {
            return Err(MediaError::Decode(format!(
                "an Opus packet decodes to {decoded} samples, not one 20 ms frame"
            )));
        }
        out.extend_from_slice(&buffer);
        if !tick() {
            return Err(stopped());
        }
    }
    out.drain(..skip);
    out.truncate(want);
    Ok(out)
}

/// Interleaved 48 kHz samples as 20 ms Opus packets, padded with silence as shipped streams are.
pub fn encode(pcm: &[i16], channels: u8, settings: Settings) -> Result<Vec<Vec<u8>>, MediaError> {
    encode_ticked(pcm, channels, settings, &|| true)
}

/// [`encode`], `tick` called after each frame; its `false` stops the encode.
fn encode_ticked(
    pcm: &[i16],
    channels: u8,
    settings: Settings,
    tick: &dyn Fn() -> bool,
) -> Result<Vec<Vec<u8>>, MediaError> {
    let k = usize::from(channels);
    let layout = layout(channels)?;
    if pcm.is_empty() || !pcm.len().is_multiple_of(k) {
        return Err(MediaError::Encode(format!(
            "{} samples are not whole frames of {k} channel(s)",
            pcm.len()
        )));
    }
    let application = if settings.voice {
        Application::Voip
    } else {
        Application::Audio
    };
    let mut builder = Encoder::builder(RATE, layout, application)
        .bitrate(Bitrate::Bits(settings.bitrate))
        .complexity(10)
        .vbr(true)
        .dtx(false)
        .frame_duration(FrameDuration::Ms20);
    if k == 2 {
        // Every frame of a stereo stream is coded stereo, which its TOC byte states.
        builder = builder.force_channels(ForceChannels::Stereo);
    }
    let mut encoder = builder
        .build()
        .map_err(|e| MediaError::Encode(format!("the Opus encoder did not start: {e}")))?;
    if encoder.lookahead() != u32::from(PRE_SKIP) {
        return Err(MediaError::Encode(format!(
            "the Opus encoder delays its output by {} samples, and a decode drops {PRE_SKIP}",
            encoder.lookahead()
        )));
    }
    let frame = OPUS_FRAME as usize * k;
    let frames = (pcm.len() / k + PADDING).div_ceil(OPUS_FRAME as usize);
    let mut padded = pcm.to_vec();
    padded.resize(frames * frame, 0);
    let mut packet = [0u8; LONGEST_PACKET];
    padded
        .chunks_exact(frame)
        .map(|samples| {
            let n = encoder
                .encode(samples, &mut packet)
                .map_err(|e| MediaError::Encode(format!("a frame does not encode: {e}")))?;
            if !tick() {
                return Err(stopped());
            }
            Ok(packet[..n].to_vec())
        })
        .collect()
}

/// The table's row: a single stream's data chunk, decoded to the samples its record states.
pub fn open(info: &AudioInfo, data: &[u8]) -> Result<Box<dyn AudioStream>, MediaError> {
    let (Ok(channels), Ok(samples)) = (u8::try_from(info.channels), u32::try_from(info.frames))
    else {
        return Err(MediaError::Decode(format!(
            "an Opus stream of {} channels and {} samples is not one the container states",
            info.channels, info.frames
        )));
    };
    let packets = split_packets(data, channels).map_err(|e| MediaError::Decode(e.to_string()))?;
    let pcm = decode(&OpusStream {
        id: 0,
        channels,
        samples,
        packets,
    })?;
    Ok(Box::new(Pcm::new(Samples::I16(pcm), info.channels)))
}

/// `file` with Opus stream `stream`, single or multi-channel, encoded anew from a WAV's PCM16 bytes.
pub fn replaced(
    file: &AwcFile,
    stream: usize,
    pcm: &[u8],
    rate: u32,
    channels: u16,
) -> Result<AwcFile, MediaError> {
    replaced_watched(file, stream, pcm, rate, channels, &|_, _| true)
}

/// [`replaced`], `watch` told of each frame encoded and each packet decoded.
pub fn replaced_watched(
    file: &AwcFile,
    stream: usize,
    pcm: &[u8],
    rate: u32,
    channels: u16,
    watch: Watch<'_>,
) -> Result<AwcFile, MediaError> {
    let refuse = |why: String| Err(MediaError::Encode(why));
    let Some(s) = file.streams.get(stream) else {
        return refuse(format!("there is no stream {stream}"));
    };
    let Some(format) = (s.opus_format()).map_err(|e| MediaError::Encode(e.to_string()))? else {
        return blocks::replaced(file, stream, pcm, rate, channels, watch);
    };
    if rate != RATE {
        return refuse(format!(
            "the WAV is {rate} Hz and this Opus stream is {RATE} Hz, and OpenRage resamples \
             nothing: save it at {RATE} Hz"
        ));
    }
    let k = format.channels;
    if channels != u16::from(k) {
        return refuse(format!(
            "the WAV has {channels} channel(s) and this stream has {k}: save it with {k}"
        ));
    }
    let pcm: Vec<i16> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    let Ok(samples) = u32::try_from(pcm.len() / usize::from(k)) else {
        return refuse("the WAV holds more samples than a stream states".to_string());
    };
    if samples == 0 {
        return refuse("the WAV holds no samples".to_string());
    }
    let old = (file.opus_streams(stream)).map_err(|e| MediaError::Encode(e.to_string()))?;
    let Some(old) = old.first() else {
        return refuse(format!("stream {stream} holds no Opus packet"));
    };
    let loudness = s.chunk(AwcChunkKind::Loudness);
    // The frames encoded, the new packets decoded, and the old ones a loudness chunk decodes.
    let frames = (samples as usize + PADDING).div_ceil(OPUS_FRAME as usize) as u64;
    let old_packets = loudness.map_or(0, |_| old.packets.len() as u64);
    let tick = ticks(watch, 2 * frames + old_packets);
    let new = OpusStream {
        id: old.id,
        channels: k,
        samples,
        packets: encode_ticked(&pcm, k, Settings::of(old), &tick)?,
    };
    let heard = decode_ticked(&new, &tick)?;
    let loudness = match loudness {
        Some(chunk) => levels(&chunk.payload, old, &heard, k, samples, &tick)?,
        None => Vec::new(),
    };
    let windows = peaks(&heard, k, samples);
    let edited = file.with_opus_replaced(stream, &new, &windows, &loudness);
    edited.map_err(|e| MediaError::Encode(e.to_string()))
}

/// The peak of each window of 4,096 sample frames from window 0: min(2·|s|, 65,535) over every
/// channel.
fn peaks(pcm: &[i16], channels: u8, samples: u32) -> Vec<u16> {
    let span = PEAK_FRAMES * usize::from(channels.max(1));
    (0..opus_peak_windows(samples))
        .map(|w| {
            let window = &pcm[(w * span).min(pcm.len())..((w + 1) * span).min(pcm.len())];
            let doubled = window.iter().map(|s| 2 * u32::from(s.unsigned_abs()));
            doubled.max().unwrap_or(0).min(u32::from(u16::MAX)) as u16
        })
        .collect()
}

/// The new decode's loudness records, each BS.1770's level plus how far the old records sat from
/// the old decode's.
fn levels(
    payload: &[u8],
    old: &OpusStream,
    heard: &[i16],
    channels: u8,
    samples: u32,
    tick: &dyn Fn() -> bool,
) -> Result<Vec<f32>, MediaError> {
    let (records, rest) = payload.as_chunks::<8>();
    if !rest.is_empty() {
        return Err(MediaError::Encode(format!(
            "the loudness chunk's {} bytes are not whole 8-byte records",
            payload.len()
        )));
    }
    let at: Vec<u32> = (records.iter())
        .map(|r| u32::from_le_bytes([r[0], r[1], r[2], r[3]]))
        .collect();
    let measured = momentary(&decode_ticked(old, tick)?, old.channels, &at);
    let mut offsets: Vec<f64> = (records.iter().zip(measured))
        .map(|(r, level)| f64::from(f32::from_le_bytes([r[4], r[5], r[6], r[7]])) - level)
        .filter(|offset| offset.is_finite())
        .collect();
    offsets.sort_by(f64::total_cmp);
    // Inferred: each stream's records sit a distance of their own from BS.1770, which the median
    // carries over; a stream with no record carries none.
    let offset = offsets.get(offsets.len() / 2).copied().unwrap_or(0.0);
    let starts: Vec<u32> = (0..loudness_windows(samples) as u32)
        .map(|i| i * LOUDNESS_STEP)
        .collect();
    let levels = momentary(heard, channels, &starts).into_iter();
    Ok(levels.map(|level| (level + offset) as f32).collect())
}

/// `x` through one biquad section from rest.
fn biquad(x: &[f64], f: &[f64; 5]) -> Vec<f64> {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    x.iter()
        .map(|&x0| {
            let y0 = f[0] * x0 + f[1] * x1 + f[2] * x2 - f[3] * y1 - f[4] * y2;
            (x2, x1, y2, y1) = (x1, x0, y1, y0);
            y0
        })
        .collect()
}

/// BS.1770 momentary loudness, in LUFS, of the 400 ms from each sample frame in `starts`; every
/// channel weighs 1, as left and right do.
fn momentary(pcm: &[i16], channels: u8, starts: &[u32]) -> Vec<f64> {
    let k = usize::from(channels.max(1));
    let frames = pcm.len() / k;
    // The K-weighted energy of every channel up to each sample frame.
    let mut energy = vec![0.0f64; frames + 1];
    for c in 0..k {
        let x: Vec<f64> = (pcm.iter().skip(c).step_by(k).take(frames))
            .map(|s| f64::from(*s) / 32768.0)
            .collect();
        let weighted = biquad(&biquad(&x, &SHELF), &HIGH_PASS);
        let mut sum = 0.0;
        for (t, y) in weighted.iter().enumerate() {
            sum += y * y;
            energy[t + 1] += sum;
        }
    }
    let window = LOUDNESS_WINDOW as usize;
    (starts.iter())
        .map(|&start| {
            let start = (start as usize).min(frames);
            let end = (start + window).min(frames);
            let mean = (energy[end] - energy[start]) / window as f64;
            if mean > 0.0 {
                (-0.691 + 10.0 * mean.log10()).max(GATE_LUFS)
            } else {
                GATE_LUFS
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use or_io::awc::AwcKey;

    use super::*;
    use crate::{awc, Codec, Decoders};

    /// A `hz` tone `frames` long, each channel at `amplitude` divided by one more than its index.
    fn tone(frames: usize, channels: u8, hz: f64, amplitude: f64) -> Vec<i16> {
        let mut out = Vec::with_capacity(frames * usize::from(channels));
        for t in 0..frames {
            let s = (TAU * hz * t as f64 / 48_000.0).sin();
            for c in 0..channels {
                out.push((amplitude * s / f64::from(c + 1)).round() as i16);
            }
        }
        out
    }

    /// Signal over error in dB, from 20 ms in to 20 ms before the end.
    fn snr(want: &[i16], got: &[i16], channels: u8) -> f64 {
        let edge = 960 * usize::from(channels);
        let (want, got) = (&want[edge..want.len() - edge], &got[edge..got.len() - edge]);
        let (mut signal, mut error) = (0.0, 0.0);
        for (w, g) in want.iter().zip(got) {
            let (w, g) = (f64::from(*w), f64::from(*g));
            signal += w * w;
            error += (w - g) * (w - g);
        }
        10.0 * (signal / f64::max(error, 1.0)).log10()
    }

    /// The floor a decode must pass: 9 dB above the decode left delayed by the pre-skip, which
    /// puts a 1 kHz tone, 6.5 periods late, half a period out.
    fn floor(want: &[i16], channels: u8) -> f64 {
        let mut late = vec![0; usize::from(PRE_SKIP) * usize::from(channels)];
        late.extend_from_slice(want);
        snr(want, &late[..want.len()], channels) + 9.0
    }

    fn le(pcm: &[i16]) -> Vec<u8> {
        pcm.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn a_tone_encoded_and_decoded_comes_back_above_the_floor_a_misaligned_tone_misses() {
        let cases = [
            (1u8, 48_000usize, 8_000u32, true),
            (1, 30_011, 64_000, false),
            (2, 48_000, 64_000, false),
            (2, 30_011, 24_000, true),
        ];
        for (channels, frames, bitrate, voice) in cases {
            let settings = Settings { bitrate, voice };
            let want = tone(frames, channels, 1000.0, 9000.0);
            let packets = encode(&want, channels, settings).expect("it encodes");
            assert_eq!(packets.len(), (frames + 1920).div_ceil(960));
            assert!(packets
                .iter()
                .all(|p| (1..=LONGEST_PACKET).contains(&p.len())));
            let stereo = packets.iter().all(|p| (p[0] & 4 != 0) == (channels == 2));
            let one_frame = packets.iter().all(|p| p[0] & 3 == 0);
            assert!(stereo && one_frame, "{settings:?}");
            let stream = OpusStream {
                id: 1,
                channels,
                samples: frames as u32,
                packets,
            };
            let got = decode(&stream).expect("it decodes");
            assert_eq!(got.len(), want.len());
            let (heard, floor) = (snr(&want, &got, channels), floor(&want, channels));
            assert!(
                heard > floor,
                "{settings:?}: {heard:.1} dB, floor {floor:.1} dB"
            );
        }
    }

    #[test]
    fn the_same_samples_encode_to_the_same_packets() {
        let settings = Settings {
            bitrate: 32_000,
            voice: false,
        };
        let pcm = tone(9_600, 2, 440.0, 6000.0);
        let first = encode(&pcm, 2, settings).expect("it encodes");
        assert_eq!(encode(&pcm, 2, settings).expect("it encodes"), first);
        let other = tone(9_600, 2, 660.0, 6000.0);
        assert_ne!(encode(&other, 2, settings).expect("it encodes"), first);
    }

    #[test]
    fn a_truncated_packet_or_stream_is_an_error_not_a_panic() {
        let settings = Settings {
            bitrate: 32_000,
            voice: false,
        };
        let packets = encode(&tone(500, 1, 440.0, 4000.0), 1, settings).expect("it encodes");
        let mut data = Vec::new();
        for p in &packets {
            data.extend((p.len() as u16).to_le_bytes());
            data.extend(p);
        }
        data.resize(2048, 0);
        let info = AudioInfo {
            codec: Codec::Opus,
            channels: 1,
            sample_rate: 48_000,
            frames: 500,
        };
        let played = open(&info, &data).and_then(|mut audio| audio.read_all_i16());
        assert_eq!(played.expect("it plays").len(), 500);
        let mut cut = data.clone();
        cut[0..2].copy_from_slice(&2047u16.to_le_bytes());
        for broken in [&cut[..], &data[..2047], &data[..3]] {
            assert!(open(&info, broken).is_err(), "{} bytes", broken.len());
        }
        let stream = |samples: u32, packets: Vec<Vec<u8>>| OpusStream {
            id: 1,
            channels: 1,
            samples,
            packets,
        };
        let held = 960 * packets.len() as u32 - u32::from(PRE_SKIP);
        assert!(decode(&stream(held, packets.clone())).is_ok());
        assert!(decode(&stream(held + 1, packets.clone())).is_err());
        let mut empty = packets.clone();
        empty[0].clear();
        assert!(decode(&stream(500, empty)).is_err());
        let three = OpusStream {
            channels: 3,
            ..stream(500, packets)
        };
        assert!(decode(&three).is_err());
    }

    #[test]
    fn a_window_peaks_at_twice_its_largest_sample_over_every_channel() {
        let mut mono = vec![0i16; 10_000];
        (mono[100], mono[5000], mono[9000]) = (300, -1000, i16::MIN);
        assert_eq!(peaks(&mono, 1, 10_000), [600, 2000]);
        let mut stereo = vec![0i16; 2 * 9_000];
        stereo[2 * 10 + 1] = 7;
        stereo[2 * 4100] = i16::MIN;
        stereo[2 * 4200 + 1] = 20_000;
        assert_eq!(peaks(&stereo, 2, 9_000), [14, 65_535]);
        assert_eq!(peaks(&mono[..100], 1, 100), [0]);
    }

    /// BS.1770 states that a full-scale 997 Hz sine reads −3.01 LUFS on one channel, and the same
    /// on two channels 3.01 more.
    #[test]
    fn a_full_scale_sine_reads_the_loudness_bs_1770_states() {
        for channels in [1u8, 2] {
            let sine = tone(48_000, channels, 997.0, 32_767.0);
            let same: Vec<i16> = match channels {
                1 => sine,
                _ => sine.chunks(2).flat_map(|f| [f[0], f[0]]).collect(),
            };
            let want = -3.0103 + 3.0103 * f64::from(channels - 1);
            for level in momentary(&same, channels, &[4_800, 24_000]) {
                assert!((level - want).abs() < 0.1, "{channels}: {level}");
            }
        }
        assert_eq!(momentary(&[0; 30_000], 1, &[0]), [GATE_LUFS]);
    }

    /// A plain container of `old` as one single Opus stream, its packets in one 2,048-byte packet,
    /// with a peak chunk and the loudness records `levels`.
    fn container(old: &OpusStream, levels: &[(u32, f32)]) -> Vec<u8> {
        let (k, samples) = (old.channels, old.samples);
        let mut data = Vec::new();
        for p in &old.packets {
            data.extend((p.len() as u16).to_le_bytes());
            data.extend(p);
        }
        assert!(data.len() <= 2048);
        data.resize(2048, 0);
        let mut record = vec![0u8; 32];
        record[0x00..0x04].copy_from_slice(&(samples * u32::from(k)).to_le_bytes());
        record[0x04..0x08].copy_from_slice(&(-1i32).to_le_bytes());
        record[0x08..0x0A].copy_from_slice(&48_000u16.to_le_bytes());
        record[0x0C..0x10].copy_from_slice(&samples.to_le_bytes());
        record[0x16..0x18].copy_from_slice(&0xFFFFu16.to_le_bytes());
        record[0x18..0x1A].copy_from_slice(&((samples - 1) as u16).to_le_bytes());
        (record[0x1C], record[0x1D]) = (11, k);
        let last = 960 * u32::from(k) * (old.packets.len() as u32 - 1);
        let seek: Vec<u8> = [0u32, last].iter().flat_map(|w| w.to_le_bytes()).collect();
        let loudness: Vec<u8> = (levels.iter())
            .flat_map(|(at, level)| at.to_le_bytes().into_iter().chain(level.to_le_bytes()))
            .collect();
        let chunks = [
            (0x76u64, record),
            (0x55, data),
            (0xA3, seek),
            (0x36, vec![0; 4]),
            (0xB2, loudness),
        ];
        let tables = 16 + 4 + chunks.len() * 8;
        let mut out = b"ADAT".to_vec();
        let words = [0xFF02_0001u32, 1, tables as u32, 5 << 29 | old.id];
        words.iter().for_each(|w| out.extend(w.to_le_bytes()));
        let mut at = tables;
        for (kind, payload) in &chunks {
            out.extend((kind << 56 | (payload.len() as u64) << 28 | at as u64).to_le_bytes());
            at += payload.len();
        }
        chunks.iter().for_each(|(_, payload)| out.extend(payload));
        out
    }

    #[test]
    fn a_stream_replaced_from_a_wav_plays_back_with_the_peaks_and_levels_of_its_decode() {
        for channels in [1u8, 2] {
            let settings = Settings {
                bitrate: 64_000,
                voice: false,
            };
            let old_pcm = tone(1000, channels, 1000.0, 9000.0);
            let old = OpusStream {
                id: 0x0ABC,
                channels,
                samples: 1000,
                packets: encode(&old_pcm, channels, settings).expect("it encodes"),
            };
            let stated = momentary(&decode(&old).expect("it decodes"), channels, &[0])[0] + 7.5;
            let bytes = container(&old, &[(0, stated as f32)]);
            let file = AwcFile::parse(&bytes, AwcKey::default()).expect("the container parses");

            let want = tone(30_011, channels, 1000.0, 9000.0);
            let wav = u16::from(channels);
            let edited = replaced(&file, 0, &le(&want), 48_000, wav).expect("replaced");
            let saved = edited.to_bytes(AwcKey::default()).expect("written");
            let read = AwcFile::parse(&saved, AwcKey::default()).expect("it reads back");
            let source = awc::open(&read, AwcKey::default(), 0).expect("it opens");
            let got = (source.audio(&Decoders::new()))
                .and_then(|mut audio| audio.read_all_i16())
                .expect("it plays");
            assert_eq!(got.len(), want.len());
            let (heard, floor) = (snr(&want, &got, channels), floor(&want, channels));
            assert!(
                heard > floor,
                "{channels}: {heard:.1} dB, floor {floor:.1} dB"
            );

            let s = &read.streams[0];
            let format = s.opus_format().expect("read").expect("present");
            let windows = peaks(&got, channels, 30_011);
            assert_eq!(
                (format.samples_per_channel, format.peak),
                (30_011, windows[0])
            );
            assert_eq!(s.peak_envelope(), Some(windows[1..].to_vec()));
            let records = &s.chunk(AwcChunkKind::Loudness).expect("kept").payload;
            let starts = [0, LOUDNESS_STEP, 2 * LOUDNESS_STEP];
            let levels = momentary(&got, channels, &starts);
            assert_eq!(records.len(), 8 * levels.len());
            for (record, (start, level)) in records.chunks(8).zip(starts.iter().zip(levels)) {
                assert_eq!(record[0..4], start.to_le_bytes());
                let stored = f32::from_le_bytes([record[4], record[5], record[6], record[7]]);
                assert!(
                    (f64::from(stored) - level - 7.5).abs() < 1e-3,
                    "{stored} {level}"
                );
            }

            let refused = |rate, wav| replaced(&file, 0, &le(&want), rate, wav).err();
            let why = refused(44_100, wav).expect("refused").to_string();
            assert!(why.contains("resamples nothing"), "{why}");
            let why = refused(48_000, 3).expect("refused").to_string();
            assert!(why.contains("save it with"), "{why}");
        }
    }
}
