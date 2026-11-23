//! The codecs a PC `.awc` stream is stored in, and the decode to PCM16.

/// How a stream's samples are stored. The byte is `AwcFormatChunk::codec`.
///
/// ⚠ **Which value an install uses is an edition difference rather than a
/// format one.** Measured by `openrage-cli awc
/// census` over both retail installs: Legacy is 218,378 ADPCM, 23,387 PCM16
/// and 8 XMA of 253,932 streams; Enhanced is 207,436 MP3, 23,711 PCM16 and
/// only 11,347 ADPCM of 254,731. A value this does not name round-trips as
/// [`Codec::Unknown`] rather than being read as the nearest one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Signed 16-bit little-endian, one sample per two bytes.
    Pcm16,
    /// IMA ADPCM in 2048-byte blocks.
    ImaAdpcm,
    /// MPEG-1 Layer III. Identified by the frame header its payload starts
    /// with, and confirmed by [`mpeg_frame_rate`] agreeing with the rate the
    /// format chunk declares.
    Mp3,
    /// Xbox Media Audio, which has no decode here: its 2,048-byte packets
    /// are structure the install test measures, and the coding inside each
    /// frame needs tables no permissively licensed source carries.
    Xma,
    /// Opus (RFC 6716), GTA VI's one codec: 20 ms frames at 48 kHz, which nothing here decodes.
    Opus,
    Unknown(u8),
}

impl Codec {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0 => Codec::Pcm16,
            4 => Codec::ImaAdpcm,
            5 => Codec::Xma,
            7 => Codec::Mp3,
            11 => Codec::Opus,
            other => Codec::Unknown(other),
        }
    }

    pub fn as_byte(self) -> u8 {
        match self {
            Codec::Pcm16 => 0,
            Codec::ImaAdpcm => 4,
            Codec::Xma => 5,
            Codec::Mp3 => 7,
            Codec::Opus => 11,
            Codec::Unknown(other) => other,
        }
    }

    pub fn label(self) -> String {
        match self {
            Codec::Pcm16 => "PCM16".to_string(),
            Codec::ImaAdpcm => "ADPCM".to_string(),
            Codec::Mp3 => "MP3".to_string(),
            Codec::Xma => "XMA".to_string(),
            Codec::Opus => "Opus".to_string(),
            Codec::Unknown(other) => format!("unknown({other})"),
        }
    }
}

/// The sampling rate an MPEG audio frame header declares, in Hz.
///
/// Calling codec 7 MP3 is a claim; `openrage-cli awc census` tests it by how
/// often this rate agrees with the one the format chunk declares.
pub fn mpeg_frame_rate(data: &[u8]) -> Option<u32> {
    if data.len() < 4 || data[0] != 0xFF || data[1] & 0xE0 != 0xE0 {
        return None;
    }
    // MPEG version: 11 is MPEG-1, 10 is MPEG-2, 00 is MPEG-2.5, 01 reserved.
    let base = match (data[1] >> 3) & 3 {
        0b11 => 1,
        0b10 => 2,
        0b00 => 4,
        _ => return None,
    };
    let rate = match (data[2] >> 2) & 3 {
        0b00 => 44100,
        0b01 => 48000,
        0b10 => 32000,
        _ => return None,
    };
    Some(rate / base)
}

/// Bytes of one ADPCM block, header included.
pub const ADPCM_BLOCK_BYTES: usize = 2048;

/// Bytes of block header before the nibble run: step index, one unused byte,
/// then the initial predictor as an `i16`.
pub const ADPCM_BLOCK_HEADER_BYTES: usize = 4;

/// Nibble-payload bytes per block, so `2 * this` samples per block.
pub const ADPCM_BLOCK_PAYLOAD_BYTES: usize = ADPCM_BLOCK_BYTES - ADPCM_BLOCK_HEADER_BYTES;

const IMA_INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

const IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// What the decoder does with the running predictor between samples.
///
/// ⚠ Stock IMA ADPCM clamps the accumulator itself; the reference C#
/// implementation for this format clamps only on output and lets the
/// accumulator run past `i16`.
/// The two agree until a stream saturates and then diverge for the rest of
/// the block, so one of them is wrong about what the game plays.
/// `an_adpcm_stream_reproduces_the_peaks_its_own_encoder_recorded` decides it
/// against the peaks Rockstar's encoder wrote into the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accumulator {
    /// Clamp the predictor to `i16` range every sample.
    Clamped,
    /// Keep the predictor unclamped and clamp only what is emitted.
    Unclamped,
}

/// Decode `samples` samples of IMA ADPCM into PCM16 little-endian bytes.
///
/// Output is always `2 * samples` bytes: a stream whose payload runs out
/// early is zero-filled to its declared length rather than truncated, so
/// `samples` and the buffer length never disagree.
pub fn decode_adpcm(data: &[u8], samples: usize, accumulator: Accumulator) -> Vec<u8> {
    let mut out = vec![0u8; samples * 2];
    let mut written = 0usize;
    let mut at = 0usize;

    while at + ADPCM_BLOCK_HEADER_BYTES <= data.len() && written < samples {
        let mut step_index = (data[at] as i32).clamp(0, 88);
        let mut predictor = i16::from_le_bytes([data[at + 2], data[at + 3]]) as i32;
        at += ADPCM_BLOCK_HEADER_BYTES;

        let block_end = (at + ADPCM_BLOCK_PAYLOAD_BYTES).min(data.len());
        while at < block_end && written < samples {
            let byte = data[at];
            at += 1;
            for nibble in [byte & 0x0F, byte >> 4] {
                let step = IMA_STEP_TABLE[step_index as usize];
                let mut diff = (((((nibble & 7) as i32) << 1) + 1) * step) >> 3;
                if nibble & 8 != 0 {
                    diff = -diff;
                }
                predictor += diff;
                if accumulator == Accumulator::Clamped {
                    predictor = predictor.clamp(-32768, 32767);
                }
                step_index = (step_index + IMA_INDEX_TABLE[nibble as usize]).clamp(0, 88);

                if written < samples {
                    let sample = predictor.clamp(-32768, 32767) as i16;
                    out[written * 2..written * 2 + 2].copy_from_slice(&sample.to_le_bytes());
                    written += 1;
                }
            }
        }
    }

    out
}

/// IMA ADPCM blocks for PCM16 little-endian samples, laid out as [`decode_adpcm`] reads them.
///
/// ⚠ **Each sample takes the nibble whose decode lands nearest it, found by
/// running the decoder's own arithmetic over all sixteen**, so the decode is
/// the encoder's model and not an approximation of it. A nibble that would
/// carry the predictor past `i16` is never chosen, which makes both
/// [`Accumulator`] models decode the result identically. Each block's header
/// carries the step index and predictor the previous block ended on; the
/// first starts at step index 0 from predictor 0. The payload is padded to a
/// whole number of words, as every shipped stream's is.
///
/// ⚠ **Re-encoding a shipped stream's decode is close, not exact.** The
/// game's encoder starts each block from a header of its own choosing, near
/// the block's first sample rather than where the previous block ended, so
/// no block after the first starts from the state this one carries
/// (`a_shipped_adpcm_stream_reencodes_within_adpcm_error`). Re-encoding this
/// encoder's own output is exact.
pub fn encode_adpcm(pcm: &[u8]) -> Vec<u8> {
    let samples: Vec<i32> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as i32)
        .collect();
    let mut out = Vec::new();
    let (mut predictor, mut step_index) = (0i32, 0i32);
    for block in samples.chunks(ADPCM_BLOCK_PAYLOAD_BYTES * 2) {
        out.extend_from_slice(&[step_index as u8, 0]);
        out.extend_from_slice(&(predictor as i16).to_le_bytes());
        for pair in block.chunks(2) {
            let mut byte = 0u8;
            for (half, &target) in pair.iter().enumerate() {
                let step = IMA_STEP_TABLE[step_index as usize];
                let decoded = |nibble: u8| {
                    let diff = (((((nibble & 7) as i32) << 1) + 1) * step) >> 3;
                    let next = predictor + if nibble & 8 != 0 { -diff } else { diff };
                    // The nibble toward zero always stays inside, so one does.
                    let cost = if (-32768..=32767).contains(&next) {
                        (next - target).abs()
                    } else {
                        i32::MAX
                    };
                    (cost, nibble, next)
                };
                let (_, nibble, next) =
                    (1..16).map(decoded).fold(
                        decoded(0),
                        |best, c| {
                            if c.0 < best.0 {
                                c
                            } else {
                                best
                            }
                        },
                    );
                predictor = next;
                step_index = (step_index + IMA_INDEX_TABLE[nibble as usize]).clamp(0, 88);
                byte |= nibble << (4 * half);
            }
            out.push(byte);
        }
    }
    out.resize(out.len().next_multiple_of(4), 0);
    out
}

/// Every frame of an MP3 run through `decoder`, as PCM16 little-endian bytes.
///
/// ⚠ **The decoder is the caller's so it can outlive the run.** A
/// multi-channel stream's channel is one MP3 stream cut into blocks: its
/// first frame in a block takes the bit reservoir and the overlap from the
/// previous block's last, and a fresh decoder per block jumps at every join.
///
/// `None` on a corrupt frame, a frame that needs reservoir bytes the decoder
/// never saw, or a stereo frame, which no `.awc` stream is.
pub(crate) fn decode_mp3_frames(decoder: &mut nanomp3::Decoder, data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut frame = [0i16; nanomp3::MAX_SAMPLES_PER_FRAME];
    let mut at = 0;
    while at < data.len() {
        let (used, result) = decoder.decode(&data[at..], &mut frame);
        at += used;
        match result {
            Ok(info) if info.channels == nanomp3::Channels::Mono => {
                for sample in &frame[..info.samples_produced] {
                    out.extend_from_slice(&sample.to_le_bytes());
                }
            }
            Ok(_) => return None,
            // Bytes that hold no frame were consumed; nothing is left once
            // none were.
            Err(nanomp3::DecodeError::NoFrame) if used > 0 => {}
            Err(nanomp3::DecodeError::NoFrame) => break,
            Err(_) => return None,
        }
    }
    Some(out)
}

/// `samples` samples of what a single-channel MP3 stream's frames decode to.
///
/// ⚠ **The count is the frames' less the last frame's padding, and the start
/// is not trimmed.** Measured over Enhanced, 213,874 of 213,875 streams decode
/// to between 0 and 1,151 samples more than they state, and the stated peaks
/// fit the decode from sample 0 far better than from the decoder's
/// 529-sample delay (`every_enhanced_mp3_stream_decodes_to_its_stated_sample_count`).
/// The one other decodes to 57 fewer and is padded with silence, as a short
/// ADPCM payload is. A whole frame more or fewer is a refusal: the payload and
/// the format chunk disagree.
fn decode_mp3(data: &[u8], samples: usize) -> Option<Vec<u8>> {
    let mut pcm = decode_mp3_frames(&mut nanomp3::Decoder::new(), data)?;
    if (pcm.len() / 2).abs_diff(samples) >= MP3_FRAME_SAMPLES {
        return None;
    }
    pcm.resize(samples * 2, 0);
    Some(pcm)
}

/// Samples per channel in an MPEG-1 Layer III frame, the only MP3 frame an
/// `.awc` measured so far holds.
const MP3_FRAME_SAMPLES: usize = 1152;

/// Samples [`encode_mp3`]'s encoder and [`decode_mp3`]'s decoder together delay a signal by.
///
/// ⚠ Measured by an impulse (`the_mp3_encoders_delay_is_the_one_the_lead_is_cut_by`), not stated by
/// the encoder: a different encoder release moves it, and that test fails when it does.
const MP3_DELAY: usize = 1057;

/// Zeros fed ahead of the audio, so the frame the encoder primes with holds only silence and
/// dropping it takes no audio, and the one after it has a silent half to overlap into.
///
/// ⚠ **The least that keeps the first 576 samples clean is 700 of them.** Dropping the priming frame
/// with a shorter lead loses the overlap the first granule needs, and it comes back as the signal's
/// own size in distortion: 95 zeros, which would start the audio at sample 0, left the first
/// granule at 7,038 rms of a 7,492 rms signal.
const MP3_LEAD: usize = 700;

/// Samples an encoded stream decodes to before its audio begins.
pub const MP3_START: usize = MP3_LEAD + MP3_DELAY - MP3_FRAME_SAMPLES;

/// An MP3 stream as `.awc` stores one: its frames end to end, each frame's byte length, and the samples they decode to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mp3Stream {
    pub data: Vec<u8>,
    pub frame_sizes: Vec<u16>,
    pub samples: usize,
}

/// PCM16 mono at 32 or 48 kHz as the MPEG-1 Layer III frames every shipped Enhanced stream is.
///
/// ⚠ **The shape is measured over all 213,875 single-channel MP3 streams of Enhanced and 16,422,938
/// frames**: MPEG-1, mono, no CRC, no padding, at 32 or 48 kHz, every frame self-contained
/// (`main_data_begin` 0, so no frame borrows bytes the one before it holds), each frame at its own
/// bitrate between 32 and 320 kbps averaging 82.5, and the stream's samples fewer than its frames hold
/// by under one frame. This writes that shape, with the encoder's variable-bitrate mode at `kbps`
/// on average, and refuses a frame outside it rather than store one the game may not read.
///
/// The encoder's information frame and its priming frame are dropped, as the shipped streams' priming
/// frames are (their decode runs 47 samples ahead of the same stream's ADPCM), and the audio begins
/// [`MP3_START`] samples in.
pub fn encode_mp3(pcm: &[u8], rate: u32, kbps: f32) -> Result<Mp3Stream, String> {
    let samples = pcm.len() / 2;
    if samples == 0 {
        return Err("the WAV holds no samples".to_string());
    }
    if !matches!(rate, 32000 | 48000) {
        return Err(format!(
            "this MP3 stream is {rate} Hz, and OpenRage encodes MP3 only at 32000 or 48000 Hz, \
             the rates every shipped stream's frames are at"
        ));
    }
    let stated = samples + MP3_START;
    let kept = stated.div_ceil(MP3_FRAME_SAMPLES);
    let mut input = vec![0u8; MP3_LEAD * 2];
    input.extend_from_slice(&pcm[..samples * 2]);
    input.resize((kept + 1) * MP3_FRAME_SAMPLES * 2, 0);

    let mut encoder = rusty_mp3::Mp3Encoder::new(rusty_mp3::Mp3EncoderConfig {
        bitrate_kbps: 0,
        vbr_quality: Some(kbps),
    });
    encoder
        .push_pcm_s16le(&input, 1, rate)
        .map_err(|e| format!("the MP3 encoder refused the samples: {e:?}"))?;
    encoder.finish();
    let mut packets = Vec::new();
    while let Ok(packet) = encoder.next_packet() {
        packets.push(packet);
    }
    if packets.len() != kept + 2 {
        return Err(format!(
            "the MP3 encoder wrote {} frames where {} were expected",
            packets.len(),
            kept + 2
        ));
    }

    let (mut data, mut frame_sizes) = (Vec::new(), Vec::with_capacity(kept));
    for frame in &packets[2..] {
        let reservoir = frame
            .get(4..6)
            .map(|b| (usize::from(b[0]) << 1) | usize::from(b[1] >> 7));
        let bitrate = frame
            .get(2)
            .map(|b| MPEG1_LAYER3_KBPS[usize::from(*b >> 4)])
            .filter(|kbps| *kbps > 0);
        let whole = bitrate.map(|kbps| 144_000 * kbps as usize / rate as usize);
        let shipped = frame.get(..4).is_some_and(|h| {
            // MPEG-1, Layer III, no CRC; no padding; mono.
            h[0] == 0xFF && h[1] == 0xFB && (h[2] >> 1) & 1 == 0 && h[3] >> 6 == 3
        });
        if !shipped || reservoir != Some(0) || whole != Some(frame.len()) {
            return Err(
                "the MP3 encoder wrote a frame the shipped streams have none like: one that is \
                 not mono MPEG-1, or is padded, or borrows bytes from the frame before it"
                    .to_string(),
            );
        }
        frame_sizes.push(frame.len() as u16);
        data.extend_from_slice(frame);
    }
    Ok(Mp3Stream {
        data,
        frame_sizes,
        samples: stated,
    })
}

/// The bitrate, in kbps, a Layer III MPEG-1 frame header's index states.
const MPEG1_LAYER3_KBPS: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];

/// PCM16 bytes for a stream, whatever it is stored as.
///
/// A `Pcm16` stream is already PCM, so this is a copy; an unknown codec has
/// no decode and yields `None` rather than the bytes reinterpreted as PCM.
pub fn decode(
    data: &[u8],
    samples: usize,
    codec: Codec,
    accumulator: Accumulator,
) -> Option<Vec<u8>> {
    match codec {
        Codec::Pcm16 => {
            let want = samples * 2;
            let mut out = vec![0u8; want];
            let take = want.min(data.len());
            out[..take].copy_from_slice(&data[..take]);
            Some(out)
        }
        Codec::ImaAdpcm => Some(decode_adpcm(data, samples, accumulator)),
        Codec::Mp3 => decode_mp3(data, samples),
        Codec::Xma | Codec::Opus | Codec::Unknown(_) => None,
    }
}

/// The peak envelope of a PCM16 buffer, in the encoder's own units.
///
/// One entry per 4,096 samples, each `min(2 * |sample|, 65535)` over the
/// window. Entry 0 is what the format chunk's optional trailing word holds
/// and entries 1.. are what the `peak` chunk holds, which is what makes this
/// an oracle a decoder can be checked against.
pub fn peak_envelope(pcm: &[u8], windows: usize) -> Vec<u16> {
    const WINDOW: usize = 4096;
    (0..windows)
        .map(|w| {
            let base = w * WINDOW;
            (0..WINDOW)
                .map(|i| {
                    let at = (base + i) * 2;
                    // The reference reads a sample only while `at + 2` is
                    // strictly inside the buffer, so the last whole sample of
                    // an exactly-sized buffer counts as zero. Matching that
                    // matters: it is the encoder's definition, not ours.
                    if at + 2 >= pcm.len() {
                        return 0u16;
                    }
                    let sample = i16::from_le_bytes([pcm[at], pcm[at + 1]]);
                    ((sample as i32).unsigned_abs() * 2).min(65535) as u16
                })
                .max()
                .unwrap_or(0)
        })
        .collect()
}

/// How many `peak` chunk entries a stream of `samples` samples carries.
pub fn peak_window_count(samples: usize) -> usize {
    samples.saturating_sub(4096) / 4096
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_codec_byte_the_table_does_not_name_stays_a_number() {
        assert_eq!(Codec::from_byte(0), Codec::Pcm16);
        assert_eq!(Codec::from_byte(4), Codec::ImaAdpcm);
        assert_eq!(Codec::from_byte(5), Codec::Xma);
        assert_eq!(Codec::from_byte(7), Codec::Mp3);
        assert_eq!(Codec::from_byte(3), Codec::Unknown(3));
        for byte in [0u8, 1, 3, 4, 5, 7, 255] {
            assert_eq!(Codec::from_byte(byte).as_byte(), byte);
        }
        assert!(Codec::from_byte(3).label().contains('3'));
        assert_eq!(Codec::from_byte(5).label(), "XMA");
    }

    /// The rate is decoded from the header's own fields rather than looked
    /// up in a table of observed values, so it holds for the MPEG-2 and
    /// MPEG-2.5 headers this corpus does not contain.
    #[test]
    fn an_mpeg_frame_header_states_a_rate_the_container_can_be_checked_against() {
        // MPEG-1 Layer III, no CRC, 128 kbit, 32 kHz, mono — the header
        // every Enhanced codec-7 payload measured so far begins with.
        assert_eq!(mpeg_frame_rate(&[0xFF, 0xFB, 0x88, 0xC4]), Some(32000));
        // The version bits divide the base rate: MPEG-2 halves it and
        // MPEG-2.5 quarters it.
        assert_eq!(mpeg_frame_rate(&[0xFF, 0xF3, 0x88, 0xC4]), Some(16000));
        assert_eq!(mpeg_frame_rate(&[0xFF, 0xE3, 0x88, 0xC4]), Some(8000));
        assert_eq!(mpeg_frame_rate(&[0xFF, 0xFB, 0x80, 0xC4]), Some(44100));
        assert_eq!(mpeg_frame_rate(&[0xFF, 0xFB, 0x84, 0xC4]), Some(48000));

        // Anything without the sync word, a reserved version, or a reserved
        // rate is not a frame header and must not be guessed at.
        assert_eq!(mpeg_frame_rate(&[0xFF, 0xFB, 0x8C, 0xC4]), None);
        assert_eq!(mpeg_frame_rate(&[0xFF, 0xEB, 0x88, 0xC4]), None);
        assert_eq!(mpeg_frame_rate(&[0x00, 0xFB, 0x88, 0xC4]), None);
        assert_eq!(mpeg_frame_rate(&[0xFF]), None);
    }

    /// ⚠ An unknown codec must not silently read as PCM. The bytes are not
    /// samples and a viewer that played them would produce noise at full
    /// scale.
    #[test]
    fn an_unknown_codec_refuses_rather_than_reinterpreting_the_bytes() {
        assert!(decode(&[0u8; 64], 32, Codec::Unknown(3), Accumulator::Clamped).is_none());
        assert!(decode(&[0u8; 2048], 512, Codec::Xma, Accumulator::Clamped).is_none());
        assert!(decode(&[0u8; 64], 32, Codec::Pcm16, Accumulator::Clamped).is_some());
    }

    /// `frames` MPEG-1 Layer III frames, mono, 32 kHz at 32 kbit: 144 bytes
    /// each, whose zeroed side information decodes to silence.
    fn silent_mp3(frames: usize) -> Vec<u8> {
        let mut frame = [0u8; 144];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x18, 0xC4]);
        frame.repeat(frames)
    }

    /// A stream decodes to exactly the samples it states while that is within
    /// a frame of what its frames hold. A whole frame more or fewer is a
    /// payload the format chunk does not describe, and is refused rather
    /// than padded or cut.
    #[test]
    fn an_mp3_stream_decodes_to_its_stated_count_and_refuses_one_it_does_not_hold() {
        let mp3 = silent_mp3(4);
        let decode = |samples| decode(&mp3, samples, Codec::Mp3, Accumulator::Clamped);
        let frames = 4 * MP3_FRAME_SAMPLES;
        for samples in [
            frames - MP3_FRAME_SAMPLES + 1,
            frames - 100,
            frames,
            frames + 57,
        ] {
            let pcm = decode(samples).expect("within a frame of the payload");
            assert_eq!(pcm.len(), samples * 2);
            assert!(
                pcm.iter().all(|b| *b == 0),
                "zeroed side information is silence"
            );
        }
        assert!(
            decode(frames + MP3_FRAME_SAMPLES).is_none(),
            "a whole frame missing"
        );
        assert!(
            decode(frames - MP3_FRAME_SAMPLES).is_none(),
            "a whole frame left over"
        );
    }

    /// One decoder carried across two runs decodes every frame of each, which
    /// is what lets a multi-channel stream's blocks share one per channel.
    #[test]
    fn an_mp3_decoder_carried_across_runs_decodes_every_frame_of_each() {
        let mut decoder = nanomp3::Decoder::new();
        let first = decode_mp3_frames(&mut decoder, &silent_mp3(3)).expect("first run");
        let second = decode_mp3_frames(&mut decoder, &silent_mp3(2)).expect("second run");
        assert_eq!(first.len(), 3 * MP3_FRAME_SAMPLES * 2);
        assert_eq!(second.len(), 2 * MP3_FRAME_SAMPLES * 2);

        // A stereo frame is not an `.awc` stream: mode bits 00 rather than 11.
        let mut stereo = silent_mp3(2);
        stereo[3] = 0x04;
        stereo[144 + 3] = 0x04;
        assert!(decode_mp3_frames(&mut nanomp3::Decoder::new(), &stereo).is_none());
    }

    /// ⚠ **The step table has no closed form, so this checks its shape
    /// rather than recomputing it.** Every "it is the previous one times
    /// 1.1" rule was tried against the published table and every one of them
    /// misses: rounding down disagrees at entry 8, rounding up at 36 of 88,
    /// and `round(7 · 1.1ⁱ)` at 85. The table *is* the specification.
    ///
    /// What is checkable is the shape a digit slip would break — a dropped
    /// or doubled digit moves an entry outside the band the whole table
    /// sits in. The decode itself is checked against the corpus by
    /// `an_adpcm_stream_reproduces_the_peaks_its_own_encoder_recorded`,
    /// which is where a subtler error would show.
    #[test]
    fn the_step_table_keeps_the_shape_a_transcription_slip_would_break() {
        assert_eq!(IMA_STEP_TABLE.len(), 89);
        assert_eq!(IMA_STEP_TABLE[0], 7);
        assert_eq!(IMA_STEP_TABLE[88], i16::MAX as i32);
        for (i, pair) in IMA_STEP_TABLE.windows(2).enumerate() {
            let ratio = pair[1] as f64 / pair[0] as f64;
            assert!(
                (1.0..=1.15).contains(&ratio),
                "entry {} to {} grows by {ratio}, outside the table's band",
                i,
                i + 1
            );
        }
        assert_eq!(IMA_INDEX_TABLE.len(), 16);
        // The index table is symmetric about the sign bit: nibble `n` and
        // nibble `n | 8` move the step index the same way.
        for n in 0..8 {
            assert_eq!(IMA_INDEX_TABLE[n], IMA_INDEX_TABLE[n + 8]);
        }
    }

    /// The decode of an encoded stream is the WAV's audio from [`MP3_START`], and the first 576
    /// samples are as clean as the rest, which a priming frame dropped without a lead would not be.
    #[test]
    fn an_encoded_mp3_stream_decodes_to_the_samples_it_was_given() {
        for rate in [32000u32, 48000] {
            let samples = 9000;
            let pcm = pcm16((0..samples).map(|i| {
                let t = i as f64 / f64::from(rate);
                (9000.0 * (std::f64::consts::TAU * 440.0 * t).sin()
                    + 5000.0 * (std::f64::consts::TAU * 1230.0 * t).sin()) as i32
            }));
            let mp3 = encode_mp3(&pcm, rate, 96.0).expect("encodes");
            assert_eq!(mp3.samples, samples + MP3_START);
            assert_eq!(
                mp3.frame_sizes.len(),
                mp3.samples.div_ceil(MP3_FRAME_SAMPLES)
            );
            assert_eq!(
                mp3.frame_sizes
                    .iter()
                    .map(|s| usize::from(*s))
                    .sum::<usize>(),
                mp3.data.len()
            );
            assert_eq!(mpeg_frame_rate(&mp3.data), Some(rate));

            let back = decode(&mp3.data, mp3.samples, Codec::Mp3, Accumulator::Clamped)
                .expect("the stream decodes to its stated count");
            let rms = |range: std::ops::Range<usize>| {
                let squares: f64 = range
                    .map(|j| {
                        let want = if j >= MP3_START {
                            sample_at(&pcm, j - MP3_START)
                        } else {
                            0
                        };
                        f64::from(sample_at(&back, j) - want).powi(2)
                    })
                    .sum();
                (squares / 576.0).sqrt()
            };
            // The signal's own rms is about 7,500; the codec's share of it is small and the
            // same in the first granule of the audio as in any other.
            for (name, error) in [
                ("the lead-in", rms(0..576)),
                (
                    "the first granule of audio",
                    rms(MP3_START..MP3_START + 576),
                ),
                ("the second", rms(MP3_START + 576..MP3_START + 1152)),
                ("the middle", rms(4000..4576)),
            ] {
                assert!(error < 400.0, "{rate} Hz: {name} is off by {error:.0} rms");
            }
        }
    }

    /// ⚠ The shift that makes a dropped priming frame clean is measured, so an encoder release
    /// that delays the signal differently fails here rather than in a stream's first granule.
    #[test]
    fn the_mp3_encoders_delay_is_the_one_the_lead_is_cut_by() {
        let mut pcm = vec![0u8; 20_000 * 2];
        pcm[5000 * 2..5000 * 2 + 2].copy_from_slice(&20_000i16.to_le_bytes());
        let mp3 = encode_mp3(&pcm, 32000, 96.0).expect("encodes");
        let back =
            decode(&mp3.data, mp3.samples, Codec::Mp3, Accumulator::Clamped).expect("decodes");
        let loudest = (0..back.len() / 2)
            .max_by_key(|i| sample_at(&back, *i).abs())
            .expect("samples");
        assert_eq!(loudest, 5000 + MP3_START);
    }

    /// Every frame is the shape the shipped streams' are: one a refusal names is one the game may not read.
    #[test]
    fn an_encoded_mp3_stream_is_the_shape_the_shipped_ones_are() {
        let pcm = pcm16((0..30_000).map(|i| ((i * 37) % 6000) - 3000));
        let mp3 = encode_mp3(&pcm, 48000, 96.0).expect("encodes");
        let mut at = 0;
        for size in &mp3.frame_sizes {
            let frame = &mp3.data[at..at + usize::from(*size)];
            assert_eq!(
                (frame[0], frame[1]),
                (0xFF, 0xFB),
                "MPEG-1 Layer III, no CRC"
            );
            assert_eq!((frame[2] >> 1) & 1, 0, "no padding");
            assert_eq!(frame[3] >> 6, 3, "mono");
            assert_eq!(
                (usize::from(frame[4]) << 1) | usize::from(frame[5] >> 7),
                0,
                "no frame borrows from the one before"
            );
            at += usize::from(*size);
        }
        assert_eq!(at, mp3.data.len());
        // The samples are fewer than the frames hold, by under one frame.
        assert!((0..MP3_FRAME_SAMPLES)
            .contains(&(mp3.frame_sizes.len() * MP3_FRAME_SAMPLES - mp3.samples)));
    }

    #[test]
    fn an_mp3_encode_it_cannot_make_is_refused_by_what_it_is() {
        assert!(encode_mp3(&[], 32000, 96.0)
            .expect_err("empty")
            .contains("no samples"));
        let why = encode_mp3(&[0; 400], 44100, 96.0).expect_err("44.1 kHz");
        assert!(
            why.contains("44100 Hz") && why.contains("32000 or 48000"),
            "{why}"
        );
    }

    /// A block is 2,048 bytes: four of header and 2,044 of nibbles, so
    /// 4,088 samples. Decoding must consume exactly that before it reads the
    /// next block header.
    #[test]
    fn a_block_yields_the_samples_its_size_implies() {
        assert_eq!(ADPCM_BLOCK_PAYLOAD_BYTES, 2044);
        // Two blocks of pure zero nibbles: every sample equals the block's
        // initial predictor, so the block boundary is visible in the output.
        let mut data = vec![0u8; ADPCM_BLOCK_BYTES * 2];
        data[2..4].copy_from_slice(&1000i16.to_le_bytes());
        data[ADPCM_BLOCK_BYTES + 2..ADPCM_BLOCK_BYTES + 4]
            .copy_from_slice(&(-2000i16).to_le_bytes());

        let samples = ADPCM_BLOCK_PAYLOAD_BYTES * 2 * 2;
        let pcm = decode_adpcm(&data, samples, Accumulator::Clamped);
        let at = |i: usize| i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]);

        // With step index 0 the smallest step is 7 and `(1 * 7) >> 3` is
        // zero, so a run of zero nibbles holds the predictor exactly at the
        // block's initial value — which makes the boundary unambiguous.
        assert_eq!(at(0), 1000, "first block starts from its own predictor");
        let first_block_samples = ADPCM_BLOCK_PAYLOAD_BYTES * 2;
        assert_eq!(at(first_block_samples - 1), 1000);
        assert_eq!(
            at(first_block_samples),
            -2000,
            "the second block restarts from its own predictor, not the first's"
        );
        assert_eq!(pcm.len(), samples * 2);
    }

    /// A payload shorter than the declared sample count is zero-filled, so
    /// the buffer length and the count never disagree downstream.
    #[test]
    fn a_short_payload_is_padded_rather_than_shortening_the_stream() {
        let pcm = decode_adpcm(&[0u8; 8], 1000, Accumulator::Clamped);
        assert_eq!(pcm.len(), 2000);
    }

    /// ⚠ The two accumulator models are only distinguishable once a stream
    /// saturates. A test on quiet data would pass for both and prove
    /// nothing, so this one drives the predictor past `i16` deliberately.
    #[test]
    fn the_two_accumulator_models_disagree_once_a_stream_saturates() {
        let mut data = vec![0u8; ADPCM_BLOCK_BYTES];
        data[0] = 88; // Largest step, so saturation arrives immediately.
        data[2..4].copy_from_slice(&32000i16.to_le_bytes());
        // Nibble 7 is the largest positive step; 0x77 is two of them. Both
        // models clip their *output* here, so nothing is visible yet.
        for byte in data.iter_mut().skip(ADPCM_BLOCK_HEADER_BYTES).take(16) {
            *byte = 0x77;
        }
        // Nibble 0xF is the largest negative one. A clamped accumulator
        // descends from 32,767; an unclamped one descends from far above it
        // and stays clipped for several more samples. That difference is
        // what the peak envelope over a real corpus can see.
        for byte in data.iter_mut().skip(ADPCM_BLOCK_HEADER_BYTES + 16).take(16) {
            *byte = 0xFF;
        }
        let samples = 512;
        let clamped = decode_adpcm(&data, samples, Accumulator::Clamped);
        let unclamped = decode_adpcm(&data, samples, Accumulator::Unclamped);
        assert_ne!(clamped, unclamped);
    }

    fn pcm16(samples: impl IntoIterator<Item = i32>) -> Vec<u8> {
        samples
            .into_iter()
            .flat_map(|s| (s as i16).to_le_bytes())
            .collect()
    }

    fn sample_at(pcm: &[u8], i: usize) -> i32 {
        i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]) as i32
    }

    /// A sine the quantiser can follow decodes back within a quarter of its
    /// largest step between samples: the levels a step sits a quarter-step
    /// apart, and the step that tracks the sine stays under twice its slope.
    /// The first samples, where the step climbs from the smallest, are left
    /// out; the stream crosses two block joins and ends on an odd sample.
    #[test]
    fn an_encoded_sine_decodes_back_within_quantisation_error() {
        let (rate, hz, amplitude) = (24000.0, 440.0, 8000.0);
        let samples = ADPCM_BLOCK_PAYLOAD_BYTES * 2 * 2 + 1001;
        let pcm = pcm16((0..samples).map(|i| {
            (amplitude * (std::f64::consts::TAU * hz * i as f64 / rate).sin()).round() as i32
        }));
        let back = decode_adpcm(&encode_adpcm(&pcm), samples, Accumulator::Clamped);
        let bound = std::f64::consts::TAU * hz * amplitude / rate / 4.0;
        let worst = (16..samples)
            .map(|i| (sample_at(&pcm, i) - sample_at(&back, i)).abs())
            .max()
            .unwrap_or(0);
        assert!(worst as f64 <= bound, "off by {worst}, over {bound:.0}");
    }

    /// Every sample of a decode is reachable from the state the encoder
    /// carries, so encoding a decode of the encoder's own output gives the
    /// same bytes back, white noise at full scale included.
    #[test]
    fn reencoding_the_encoders_own_decode_gives_the_same_bytes() {
        let mut seed = 1u32;
        let samples = ADPCM_BLOCK_PAYLOAD_BYTES * 2 * 3 + 7;
        let noise = pcm16((0..samples).map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) as i32 - 32768
        }));
        let encoded = encode_adpcm(&noise);
        let decoded = decode_adpcm(&encoded, samples, Accumulator::Clamped);
        assert_eq!(encode_adpcm(&decoded), encoded);
    }

    /// The encoder never carries the predictor past `i16`, so a stream that
    /// saturates decodes the same under both accumulator models, which
    /// `the_two_accumulator_models_disagree_once_a_stream_saturates` shows
    /// they do not for every stream.
    #[test]
    fn a_full_scale_square_decodes_the_same_under_both_accumulator_models() {
        let samples = 5000;
        let square = pcm16((0..samples).map(|i| if (i / 40) % 2 == 0 { 32767 } else { -32768 }));
        let encoded = encode_adpcm(&square);
        assert_eq!(
            decode_adpcm(&encoded, samples, Accumulator::Clamped),
            decode_adpcm(&encoded, samples, Accumulator::Unclamped)
        );
    }

    /// One sample past a whole block is a second header and one byte, padded
    /// to a word; the second header carries the predictor the first block
    /// ended on.
    #[test]
    fn an_encode_lays_its_blocks_out_as_the_decoder_reads_them() {
        assert!(encode_adpcm(&[]).is_empty());
        let block = ADPCM_BLOCK_PAYLOAD_BYTES * 2;
        let pcm = pcm16((0..=block).map(|i| (i % 300) as i32 * 50));
        let encoded = encode_adpcm(&pcm);
        assert_eq!(encoded.len(), ADPCM_BLOCK_BYTES + 8);
        let decoded = decode_adpcm(&encoded, block + 1, Accumulator::Clamped);
        let second = &encoded[ADPCM_BLOCK_BYTES..];
        assert_eq!(
            i16::from_le_bytes([second[2], second[3]]) as i32,
            sample_at(&decoded, block - 1)
        );
        assert_eq!((encoded[1], second[1]), (0, 0));
    }

    /// The envelope is the encoder's definition and not a plausible one:
    /// doubled magnitude, saturating at `u16::MAX`, over 4,096 samples.
    #[test]
    fn a_peak_window_is_the_doubled_magnitude_of_its_loudest_sample() {
        let mut pcm = vec![0u8; 4096 * 2 * 2];
        pcm[100..102].copy_from_slice(&(-1234i16).to_le_bytes());
        pcm[4096 * 2 + 40..4096 * 2 + 42].copy_from_slice(&30000i16.to_le_bytes());

        let envelope = peak_envelope(&pcm, 2);
        assert_eq!(envelope[0], 2468);
        assert_eq!(envelope[1], 60000);

        // Saturation rather than wraparound.
        let mut loud = vec![0u8; 4096 * 2];
        loud[0..2].copy_from_slice(&(-32768i16).to_le_bytes());
        assert_eq!(peak_envelope(&loud, 1)[0], 65535);
    }

    #[test]
    fn the_peak_chunk_holds_every_window_after_the_first() {
        assert_eq!(peak_window_count(0), 0);
        assert_eq!(peak_window_count(4096), 0);
        assert_eq!(peak_window_count(8192), 1);
        assert_eq!(peak_window_count(8191), 0);
        assert_eq!(peak_window_count(4096 * 10), 9);
    }
}
