//! PCM16 as a `.wav`, for exporting one stream, and a `.wav` read back as PCM16 for replacing one.

/// Wrap PCM16 little-endian samples in a canonical 44-byte RIFF header.
pub fn wav_from_pcm16(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits = 16u16;
    let block_align = channels * bits / 8;
    let byte_rate = sample_rate * block_align as u32;

    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // WAVE_FORMAT_PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// PCM16 channels as one interleaved buffer, each padded with silence to the longest.
pub fn interleave_pcm16(channels: &[Vec<u8>]) -> Vec<u8> {
    let longest = channels.iter().map(Vec::len).max().unwrap_or(0);
    let mut pcm = Vec::with_capacity(longest * channels.len());
    for at in (0..longest).step_by(2) {
        for channel in channels {
            pcm.extend_from_slice(channel.get(at..at + 2).unwrap_or(&[0, 0]));
        }
    }
    pcm
}

/// PCM16 samples, sample rate and channel count from a RIFF WAVE file.
pub fn read_wav(bytes: &[u8]) -> Result<(Vec<u8>, u32, u16), String> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("not a RIFF WAVE file".to_string());
    }
    let u16_at = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let mut format: Option<&[u8]> = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or("a WAV chunk runs past the end of the file")?;
        match &bytes[at..at + 4] {
            b"fmt " if len >= 16 => format = Some(body),
            b"data" => {
                let format = format.ok_or("the WAV's data comes before its format")?;
                let tag = u16_at(format, 0);
                let channels = u16_at(format, 2);
                let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
                let bits = u16_at(format, 14);
                let pcm = tag == 1 || (tag == 0xFFFE && format.get(24..26) == Some(&[1, 0]));
                if !pcm || bits != 16 || channels == 0 {
                    return Err(format!(
                        "the WAV is {bits}-bit, format {tag:#06x}: save it as 16-bit PCM"
                    ));
                }
                let frame = usize::from(channels) * 2;
                return Ok((body[..body.len() / frame * frame].to_vec(), rate, channels));
            }
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    Err("the WAV has no data chunk".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_interleave_a_sample_at_a_time_and_the_short_one_is_padded() {
        let (a, b) = (vec![1, 0, 2, 0, 3, 0], vec![9, 0]);
        assert_eq!(
            interleave_pcm16(&[a, b]),
            vec![1, 0, 9, 0, 2, 0, 0, 0, 3, 0, 0, 0]
        );
        assert!(interleave_pcm16(&[]).is_empty());
    }

    #[test]
    fn a_wav_reads_back_what_the_writer_wrapped() {
        let pcm: Vec<u8> = (0..40u8).collect();
        assert_eq!(
            read_wav(&wav_from_pcm16(&pcm, 44100, 2)),
            Ok((pcm, 44100, 2))
        );
    }

    #[test]
    fn a_wav_that_is_not_16_bit_pcm_is_refused_by_what_it_is() {
        let mut float = wav_from_pcm16(&[0; 8], 48000, 1);
        float[20..22].copy_from_slice(&3u16.to_le_bytes());
        float[34..36].copy_from_slice(&32u16.to_le_bytes());
        let why = read_wav(&float).expect_err("float");
        assert!(
            why.contains("32-bit") && why.contains("16-bit PCM"),
            "{why}"
        );
        assert!(read_wav(b"RIFF\0\0\0\0WAVE").is_err());
        assert!(read_wav(b"not a wav").is_err());
    }

    /// Every field is recomputed from the arguments rather than restated, so
    /// this fails if the header stops describing the payload.
    #[test]
    fn the_header_describes_the_payload_it_wraps() {
        let pcm: Vec<u8> = (0..800u32).map(|i| i as u8).collect();
        let wav = wav_from_pcm16(&pcm, 48000, 1);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + pcm.len());

        let u32_at = |o: usize| u32::from_le_bytes(wav[o..o + 4].try_into().unwrap());
        let u16_at = |o: usize| u16::from_le_bytes(wav[o..o + 2].try_into().unwrap());
        assert_eq!(u32_at(4) as usize, wav.len() - 8);
        assert_eq!(u32_at(40) as usize, pcm.len());
        assert_eq!(u16_at(22), 1);
        assert_eq!(u32_at(24), 48000);
        assert_eq!(u32_at(28), 48000 * 2, "byte rate is rate * block align");
        assert_eq!(u16_at(32), 2, "block align is channels * 16 / 8");
        assert_eq!(&wav[44..], &pcm[..]);
    }

    #[test]
    fn stereo_doubles_the_block_align_and_the_byte_rate() {
        let wav = wav_from_pcm16(&[0u8; 16], 22050, 2);
        let u32_at = |o: usize| u32::from_le_bytes(wav[o..o + 4].try_into().unwrap());
        let u16_at = |o: usize| u16::from_le_bytes(wav[o..o + 2].try_into().unwrap());
        assert_eq!(u16_at(32), 4);
        assert_eq!(u32_at(28), 22050 * 4);
    }
}
