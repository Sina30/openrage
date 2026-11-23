//! A multi-channel Opus source encoded anew from a WAV: a channel the WAV leaves as it decodes
//! keeps its packets, and each other one is encoded with the settings its packets show.

use or_io::awc::opus::{OpusStream, OPUS_FRAME};
use or_io::awc::AwcFile;

use super::{decode_ticked, encode_ticked, ticks, Settings, Watch, PADDING, RATE};
use crate::MediaError;

/// `file` with multi-channel Opus source `stream` encoded from a WAV's PCM16 bytes, whose
/// channels are the source's in order, a stereo channel as two.
pub(super) fn replaced(
    file: &AwcFile,
    stream: usize,
    pcm: &[u8],
    rate: u32,
    channels: u16,
    watch: Watch<'_>,
) -> Result<AwcFile, MediaError> {
    let refuse = |why: String| Err(MediaError::Encode(why));
    let old = (file.opus_streams(stream)).map_err(|e| MediaError::Encode(e.to_string()))?;
    if old.is_empty() {
        return refuse(format!("stream {stream} holds no Opus packet"));
    }
    if rate != RATE {
        return refuse(format!(
            "the WAV is {rate} Hz and this Opus stream is {RATE} Hz, and OpenRage resamples \
             nothing: save it at {RATE} Hz"
        ));
    }
    let width: usize = old.iter().map(|c| usize::from(c.channels)).sum();
    if usize::from(channels) != width {
        return refuse(format!(
            "the WAV has {channels} channel(s) and this source's {} channels hold {width}: save \
             it with {width}",
            old.len()
        ));
    }
    let pcm: Vec<i16> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    let Ok(samples) = u32::try_from(pcm.len() / width) else {
        return refuse("the WAV holds more samples than a stream states".to_string());
    };
    if samples == 0 {
        return refuse("the WAV holds no samples".to_string());
    }
    // Each old channel decoded to compare, and each channel encoded.
    let encoded = (samples as usize + PADDING).div_ceil(OPUS_FRAME as usize) as u64;
    let steps: u64 = old.iter().map(|c| c.packets.len() as u64 + encoded).sum();
    let tick = ticks(watch, steps);
    let mut new = Vec::with_capacity(old.len());
    let mut at = 0;
    for group in &old {
        let k = usize::from(group.channels);
        let own: Vec<i16> = (pcm.chunks_exact(width))
            .flat_map(|frame| frame[at..at + k].iter().copied())
            .collect();
        at += k;
        let kept = samples == group.samples && decode_ticked(group, &tick)? == own;
        let packets = if kept {
            group.packets.clone()
        } else {
            encode_ticked(&own, group.channels, Settings::of(group), &tick)?
        };
        new.push(OpusStream {
            id: group.id,
            channels: group.channels,
            samples,
            packets,
        });
    }
    let edited = file.with_opus_channels_replaced(stream, &new);
    edited.map_err(|e| MediaError::Encode(e.to_string()))
}
