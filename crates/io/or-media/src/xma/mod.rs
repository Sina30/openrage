//! XMA, the Xbox 360's audio codec, decoded in OpenRage's own Rust.
//!
//! The coding is the WMA Pro family's, ported from FFmpeg's `wmaprodec.c` ([`decoder`]) with its
//! tables ([`tables`]); every file there says where it came from and under which licence. XMA1 and
//! XMA2 differ in their container headers, not in the 2,048-byte packets a stream is made of, so one
//! decode serves both.
//!
//! The range of a stream [`open`] keeps follows `xma_decode_packet` in FFmpeg's
//! `libavcodec/wmaprodec.c`, tag `n7.0` (commit `083443d67cb159ce469e5d902346b8d0c2cd1c93`,
//! LGPL-2.1-or-later, Copyright (c) 2007 Baptiste Coudurier, Benjamin Larsson, Ulion and (c) 2008 -
//! 2011 Sascha Sommer, Benjamin Larsson): the 64-sample delay and the trim arithmetic. The rest of
//! this file is OpenRage's own.

mod bits;
mod decoder;
mod mdct;
mod tables;
mod vlc;

use crate::{AudioInfo, AudioStream, MediaError, Pcm, Samples};
use decoder::{Decoded, Decoder, PACKET_BYTES, SAMPLES_PER_FRAME};

/// Samples the decoder's output trails its input by, which FFmpeg's `xma2` decoder drops from the start.
const DECODER_DELAY: usize = 64;

/// The samples of one channel a player hears, as FFmpeg's `xma2` decoder keeps them.
///
/// The first frame only primes the overlap and is dropped whole, and the stream's headers state
/// how much more to drop: 512 at the start of every stream measured, which FFmpeg takes out of the
/// end instead of the start, and the stream's last count at the end. The last frame's overlap
/// follows it, in a flush frame whose second half is silence.
fn heard(decoded: &Decoded, channel: usize) -> Vec<f32> {
    let frames = &decoded.frames[channel];
    let mut samples = frames[SAMPLES_PER_FRAME.min(frames.len())..].to_vec();
    samples.extend_from_slice(&decoded.tail[channel]);
    samples.resize(samples.len() + SAMPLES_PER_FRAME / 2, 0.0);
    let (trim_start, trim_end) = decoded.trim;
    let cut = (trim_start + trim_end)
        .saturating_sub(128 + DECODER_DELAY)
        .min(samples.len());
    samples.truncate(samples.len() - cut);
    samples.drain(..DECODER_DELAY.min(samples.len()));
    samples
}

/// A stream's whole packets decoded, interleaved, and as many samples as the stream states.
///
/// What the headers trim away of the stated count is silence at the end, so the count a container
/// states is the count that comes out, as it is for every other codec.
pub(crate) fn open(info: &AudioInfo, data: &[u8]) -> Result<Box<dyn AudioStream>, MediaError> {
    if data.is_empty() || !data.len().is_multiple_of(PACKET_BYTES) {
        return Err(MediaError::Decode(format!(
            "this stream is XMA and its {} bytes are not whole {PACKET_BYTES}-byte packets",
            data.len()
        )));
    }
    let channels = usize::from(info.channels);
    let decoded = Decoder::decode(channels, info.sample_rate, data)?;
    let held = decoded.frames[0].len();
    let wanted = usize::try_from(info.frames)
        .ok()
        .filter(|&frames| frames <= held)
        .ok_or_else(|| {
            MediaError::Decode(format!(
                "this stream is XMA and states {} samples, which its packets do not hold",
                info.frames
            ))
        })?;
    let heard: Vec<Vec<f32>> = (0..channels).map(|c| heard(&decoded, c)).collect();
    let mut samples = Vec::with_capacity(wanted * channels);
    for i in 0..wanted {
        samples.extend(heard.iter().map(|c| c.get(i).copied().unwrap_or(0.0)));
    }
    Ok(Box::new(Pcm::new(Samples::F32(samples), info.channels)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Codec;

    fn info(frames: u64) -> AudioInfo {
        AudioInfo {
            codec: Codec::Xma,
            channels: 1,
            sample_rate: 32000,
            frames,
        }
    }

    #[test]
    fn what_is_heard_drops_the_first_frame_the_delay_and_what_the_end_trims() {
        let frames = vec![(0..3 * 512).map(|i| i as f32).collect()];
        let tail = vec![vec![-1.0; 256]];
        // 1,024 samples of the last two frames, 256 of overlap and 256 of silence follow the first frame.
        let decoded = Decoded {
            frames: frames.clone(),
            tail: tail.clone(),
            trim: (512, 0),
        };
        // Trims of 512 and 0 cut their sum less 192 from the end: 320 samples.
        let got = heard(&decoded, 0);
        assert_eq!(got.len(), 1536 - 320 - 64);
        assert_eq!((got[0], got[959], got[960]), (576.0, 1535.0, -1.0));
        assert_eq!(
            got[got.len() - 1],
            -1.0,
            "192 samples of the overlap are kept"
        );

        let decoded = Decoded {
            frames,
            tail,
            trim: (512, 384),
        };
        let got = heard(&decoded, 0);
        assert_eq!(got.len(), 1536 - 704 - 64);
        assert_eq!(got[got.len() - 1], 1343.0, "the overlap is trimmed away");
    }

    #[test]
    fn a_stream_of_other_than_whole_packets_is_refused_by_name() {
        for data in [&[][..], &[0u8; 100][..], &[0u8; 2049][..]] {
            let Err(why) = open(&info(512), data) else {
                panic!("{} bytes are not packets", data.len());
            };
            assert!(why.to_string().contains("XMA"), "{why}");
        }
    }

    #[test]
    fn bytes_that_are_not_a_stream_are_an_error_and_never_a_panic() {
        let mut seed = 7u32;
        for round in 0..300 {
            let packets = 1 + round % 4;
            let data: Vec<u8> = (0..packets * PACKET_BYTES)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 24) as u8
                })
                .collect();
            let _ = open(&info(512), &data);
        }
        for fill in [0x00u8, 0xFF] {
            let _ = open(&info(512), &vec![fill; PACKET_BYTES * 3]);
        }
    }
}
