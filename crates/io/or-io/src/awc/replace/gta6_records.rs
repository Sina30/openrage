//! GTA VI's single streams of a 32-byte format record replaced: PCM16 from a WAV's samples, and
//! codec 2 from the blocks of a new signal.

use super::{rate_field, refuse, Refused};
use crate::awc::codec::{peak_envelope, peak_window_count};
use crate::awc::gta6_codecs::{
    spectra_blocks, spectra_zero_block, SPECTRA_BLOCK_FLOATS, SPECTRA_HOP,
};
use crate::awc::{AwcChunkKind, AwcFile, Codec};

impl AwcFile {
    /// Stream `stream`, a GTA VI PCM16 stream, storing mono PCM16 `pcm` at `rate` instead.
    pub(super) fn replaced_record(
        &self,
        stream: usize,
        pcm: &[u8],
        rate: u32,
        channels: u16,
    ) -> Result<AwcFile, Refused> {
        let s = &self.streams[stream];
        let Some(chunk) = s.chunk(AwcChunkKind::OpusFormat) else {
            return refuse(format!("stream {stream} has no format to replace it by"));
        };
        let format = match s.coded_format() {
            Ok(Some(format)) if format.codec == Codec::Pcm16 => format,
            Ok(Some(format)) if format.codec == Codec::Xma => {
                return refuse(
                    "this stream is XMA and OpenRage has no XMA encoder: only a PCM16 stream of \
                     this kind is replaced"
                        .to_string(),
                );
            }
            Ok(_) => {
                return refuse(format!(
                    "stream {stream} is not stored as PCM16, and only a PCM16 stream of this \
                     kind is replaced from samples"
                ));
            }
            Err(e) => return refuse(e.to_string()),
        };
        if channels != 1 {
            return refuse(format!(
                "the WAV has {channels} channels and this stream has one: save it as mono"
            ));
        }
        let rate = rate_field(rate)?;
        let Ok(samples) = u32::try_from(pcm.len() / 2) else {
            return refuse("the WAV holds more samples than a stream states".to_string());
        };
        if samples == 0 {
            return refuse("the WAV holds no samples".to_string());
        }
        let pcm = &pcm[..2 * samples as usize];
        let envelope = peak_envelope(pcm, 1 + peak_window_count(samples as usize));
        // A loop point is kept only inside the new samples; 0x16 and 0x18 follow it.
        let looped = u32::try_from(format.loop_point)
            .ok()
            .filter(|l| *l < samples);
        let mut record = chunk.payload.clone();
        record[0x00..0x04].copy_from_slice(&samples.to_le_bytes());
        record[0x04..0x08].copy_from_slice(&looped.map_or(-1, |l| l as i32).to_le_bytes());
        record[0x08..0x0A].copy_from_slice(&rate.to_le_bytes());
        record[0x0C..0x10].copy_from_slice(&samples.to_le_bytes());
        record[0x10..0x16].fill(0);
        record[0x16..0x18].copy_from_slice(&looped.map_or(0xFFFF, |l| l as u16).to_le_bytes());
        let last = looped.map_or(0xFFFF, |_| (samples - 1) as u16);
        record[0x18..0x1A].copy_from_slice(&last.to_le_bytes());
        record[0x1A..0x1C].copy_from_slice(&envelope[0].to_le_bytes());
        self.with_record_replaced(stream, record, &envelope[1..], pcm.to_vec())
    }

    /// This container with codec-2 stream `stream` holding `signal`'s blocks, `samples`, `peaks`.
    pub fn with_spectra_replaced(
        &self,
        stream: usize,
        signal: &[u8],
        samples: u32,
        peaks: &[u16],
    ) -> Result<AwcFile, Refused> {
        // ⚠ `signal` and `peaks` come from the caller's transform of the new samples: this crate
        // has no FFT. The last block's table is carried, which holds only while the count does.
        let Some(s) = self.streams.get(stream) else {
            return refuse("no such stream".to_string());
        };
        let found = (
            s.coded_format(),
            s.chunk(AwcChunkKind::OpusFormat),
            s.chunk(AwcChunkKind::Data),
        );
        let (Ok(Some(format)), Some(chunk), Some(data)) = found else {
            return refuse(format!(
                "stream {stream} has no codec 2 record to replace it by"
            ));
        };
        if format.codec != Codec::SPECTRA {
            return refuse(format!(
                "stream {stream} has no codec 2 record to replace it by"
            ));
        }
        let block = 4 * SPECTRA_BLOCK_FLOATS;
        let held = u64::from(samples).div_ceil(SPECTRA_HOP) + 1;
        if samples == 0 || signal.len() as u64 != block as u64 * held {
            return refuse(format!(
                "{} bytes of blocks were given for {samples} samples, which take {held} blocks of \
                 {block} bytes",
                signal.len()
            ));
        }
        let stored = spectra_blocks(format.samples_per_channel.into());
        let wanted = spectra_blocks(samples.into());
        if stored != wanted {
            let most = (stored - 3) * SPECTRA_HOP;
            let least = if stored == spectra_blocks(1) {
                1
            } else {
                most - SPECTRA_HOP + 1
            };
            return refuse(format!(
                "{samples} samples take {wanted} codec 2 blocks and this stream stores {stored}, \
                 and the 15 bin and block pairs its last block holds are known only for the count \
                 it was read with, as no rule found yet gives them for another: give it {least} to \
                 {most} samples"
            ));
        }
        if peaks.len() != 1 + peak_window_count(samples as usize) {
            return refuse(format!(
                "{} peak windows were given for {samples} samples, which state {}",
                peaks.len(),
                1 + peak_window_count(samples as usize)
            ));
        }
        // The stored data is `stored` whole blocks, as reading the record checked.
        let trailer = &data.payload[data.payload.len() - block..];
        let zero = spectra_zero_block();
        let mut payload = signal.to_vec();
        for _ in held + 1..wanted {
            payload.extend_from_slice(&zero);
        }
        payload.extend_from_slice(trailer);
        let mut record = chunk.payload.clone();
        record[0x04..0x08].copy_from_slice(&(-1i32).to_le_bytes());
        record[0x0C..0x10].copy_from_slice(&samples.to_le_bytes());
        record[0x16..0x1A].fill(0xFF);
        record[0x1A..0x1C].copy_from_slice(&peaks[0].to_le_bytes());
        let edited = self.with_record_replaced(stream, record, &peaks[1..], payload)?;
        // What is written reads back as codec 2, a signal block's bin 0 included.
        (edited.streams[stream].coded_format()).map_err(|e| Refused(e.to_string()))?;
        Ok(edited)
    }

    /// This container with stream `stream`'s format record, the peaks after window 0 when it keeps
    /// a peak chunk, and its data set.
    fn with_record_replaced(
        &self,
        stream: usize,
        record: Vec<u8>,
        peaks: &[u16],
        data: Vec<u8>,
    ) -> Result<AwcFile, Refused> {
        let mut edited = self.clone();
        let mut set = |kind, payload| {
            edited
                .set_chunk_payload(stream, kind, payload)
                .map_err(|e| Refused(e.to_string()))
        };
        set(AwcChunkKind::OpusFormat, record)?;
        if self.streams[stream].chunk(AwcChunkKind::Peak).is_some() {
            set(
                AwcChunkKind::Peak,
                peaks.iter().flat_map(|p| p.to_le_bytes()).collect(),
            )?;
        }
        set(AwcChunkKind::Data, data)?;
        Ok(edited)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::gta6_codecs::tests::spectra_stream;
    use super::super::super::opus::tests::{container, Chunks};
    use super::*;
    use crate::awc::AwcKey;

    /// A PCM16 record of `pcm` at 44,100 Hz by the rules a writer follows, looped at `loop_point`.
    fn pcm_record(pcm: &[u8], loop_point: Option<u32>) -> Vec<u8> {
        let samples = (pcm.len() / 2) as u32;
        let peaks = peak_envelope(pcm, 1 + peak_window_count(samples as usize));
        let mut r = vec![0u8; 32];
        r[0x00..0x04].copy_from_slice(&samples.to_le_bytes());
        r[0x04..0x08].copy_from_slice(&loop_point.map_or(-1, |l| l as i32).to_le_bytes());
        r[0x08..0x0A].copy_from_slice(&44_100u16.to_le_bytes());
        r[0x0A..0x0C].copy_from_slice(&(-3i16).to_le_bytes());
        r[0x0C..0x10].copy_from_slice(&samples.to_le_bytes());
        r[0x16..0x18].copy_from_slice(&loop_point.map_or(0xFFFF, |l| l as u16).to_le_bytes());
        let last = loop_point.map_or(0xFFFF, |_| (samples - 1) as u16);
        r[0x18..0x1A].copy_from_slice(&last.to_le_bytes());
        r[0x1A..0x1C].copy_from_slice(&peaks[0].to_le_bytes());
        (r[0x1C], r[0x1D]) = (0, 1);
        r
    }

    fn tone(samples: usize, step: i32) -> Vec<u8> {
        (0..samples as i32)
            .map(|i| ((i * step) % 20_000 - 10_000) as i16)
            .flat_map(i16::to_le_bytes)
            .collect()
    }

    fn pcm_stream(pcm: &[u8], loop_point: Option<u32>) -> Chunks {
        let samples = pcm.len() / 2;
        let peaks = peak_envelope(pcm, 1 + peak_window_count(samples));
        let peak: Vec<u8> = peaks[1..].iter().flat_map(|p| p.to_le_bytes()).collect();
        let mut chunks = vec![(0x55, pcm.to_vec()), (0x76, pcm_record(pcm, loop_point))];
        if !peak.is_empty() {
            chunks.push((0x36, peak));
        }
        chunks
    }

    fn parse(chunks: Chunks) -> (Vec<u8>, AwcFile) {
        let bytes = container(0xFF21, &[0, 0], &[(0x0C01, chunks)]);
        let awc = AwcFile::parse(&bytes, AwcKey::default()).expect("the container parses");
        (bytes, awc)
    }

    #[test]
    fn a_pcm16_stream_replaced_by_its_own_samples_saves_byte_identical() {
        for looped in [None, Some(7)] {
            let pcm = tone(9_000, 37);
            let (bytes, awc) = parse(pcm_stream(&pcm, looped));
            let edited = (awc.with_stream_replaced(0, &pcm, 44_100, 1, AwcKey::default()))
                .expect("replaced");
            assert_eq!(edited.to_bytes(AwcKey::default()).expect("written"), bytes);
        }
    }

    #[test]
    fn a_pcm16_stream_replaced_reads_back_its_samples_rate_peaks_and_loop() {
        let (_, awc) = parse(pcm_stream(&tone(9_000, 37), Some(7)));
        for (samples, loop_kept) in [(5_000, Some(7u32)), (5, None)] {
            let pcm = tone(samples, 91);
            let edited = (awc.with_stream_replaced(0, &pcm, 48_000, 1, AwcKey::default()))
                .expect("replaced");
            let bytes = edited.to_bytes(AwcKey::default()).expect("written");
            let read = AwcFile::parse(&bytes, AwcKey::default()).expect("it reads back");
            let s = &read.streams[0];
            let format = s.coded_format().expect("read").expect("PCM16");
            let n = samples as u32;
            assert_eq!(
                (
                    format.samples,
                    format.samples_per_channel,
                    format.stated_rate
                ),
                (n, n, 48_000)
            );
            let words = loop_kept.map_or((0xFFFF, 0xFFFF), |l| (l as u16, (n - 1) as u16));
            assert_eq!(
                (format.loop_point, format.word_16, format.word_18),
                (loop_kept.map_or(-1, |l| l as i32), words.0, words.1)
            );
            assert_eq!(format.word_0a, -3);
            let peaks = peak_envelope(&pcm, 1 + peak_window_count(samples));
            assert_eq!(format.peak, peaks[0]);
            assert_eq!(s.peak_envelope(), Some(peaks[1..].to_vec()));
            let data = read.stream_data(s, AwcKey::default()).expect("data");
            assert_eq!(data, pcm);
        }
    }

    #[test]
    fn a_pcm16_stream_takes_only_mono_samples_and_an_xma_one_is_refused() {
        let pcm = tone(300, 5);
        let (_, awc) = parse(pcm_stream(&pcm, None));
        fn refused(awc: &AwcFile, pcm: &[u8], channels: u16) -> String {
            let why = awc.with_stream_replaced(0, pcm, 44_100, channels, AwcKey::default());
            why.expect_err("refused").to_string()
        }
        assert!(refused(&awc, &pcm, 2).contains("save it as mono"));
        assert!(refused(&awc, &[], 1).contains("holds no samples"));
        let mut xma = pcm_stream(&pcm, None);
        xma[0].1 = vec![0; 2048];
        xma[0].1[0] = 1 << 2;
        xma[1].1[0x00..0x04].copy_from_slice(&512u32.to_le_bytes());
        xma[1].1[0x0C..0x10].copy_from_slice(&300u32.to_le_bytes());
        xma[1].1[0x1C] = 5;
        let (_, awc) = parse(xma);
        let why = awc.with_stream_replaced(0, &pcm, 44_100, 1, AwcKey::default());
        assert!(why
            .expect_err("refused")
            .to_string()
            .contains("no XMA encoder"));
    }

    /// `blocks` signal blocks of made-up floats, bin 0's imaginary part zero.
    fn signal_blocks(blocks: usize) -> Vec<u8> {
        (0..blocks * SPECTRA_BLOCK_FLOATS)
            .map(|f| match f % SPECTRA_BLOCK_FLOATS {
                1 => 0.0f32,
                at => (at as f32 * 0.37).sin() / (1 + f / SPECTRA_BLOCK_FLOATS) as f32,
            })
            .flat_map(f32::to_le_bytes)
            .collect()
    }

    #[test]
    fn a_codec_2_stream_replaced_by_its_own_blocks_saves_byte_identical() {
        let (bytes, awc) = parse(spectra_stream(400));
        let data = &awc.streams[0]
            .chunk(AwcChunkKind::Data)
            .expect("data")
            .payload;
        let own = &data[..2 * 4 * SPECTRA_BLOCK_FLOATS];
        let edited = awc
            .with_spectra_replaced(0, own, 400, &[900])
            .expect("replaced");
        assert_eq!(edited.to_bytes(AwcKey::default()).expect("written"), bytes);
    }

    #[test]
    fn a_codec_2_stream_replaced_reads_back_its_blocks_samples_and_peak() {
        let (_, awc) = parse(spectra_stream(400));
        let old = awc.streams[0]
            .chunk(AwcChunkKind::Data)
            .expect("data")
            .payload
            .clone();
        let block = 4 * SPECTRA_BLOCK_FLOATS;
        let signal = signal_blocks(3);
        let edited = (awc.with_spectra_replaced(0, &signal, 2_000, &[52_056])).expect("replaced");
        let bytes = edited.to_bytes(AwcKey::default()).expect("written");
        let read = AwcFile::parse(&bytes, AwcKey::default()).expect("it reads back");
        let s = &read.streams[0];
        let format = s.coded_format().expect("read").expect("codec 2");
        assert_eq!((format.samples_per_channel, format.peak), (2_000, 52_056));
        assert_eq!(format.samples, old.len() as u32 / 4);
        let data = &s.chunk(AwcChunkKind::Data).expect("data").payload;
        assert_eq!(data.len(), old.len());
        assert_eq!(data[..signal.len()], signal[..]);
        assert_eq!(data[data.len() - block..], old[old.len() - block..]);
    }

    #[test]
    fn a_codec_2_signal_of_another_block_count_or_a_stated_bin_0_is_refused() {
        let (_, awc) = parse(spectra_stream(400));
        fn refused(awc: &AwcFile, signal: &[u8], samples: u32, peaks: &[u16]) -> String {
            let why = awc.with_spectra_replaced(0, signal, samples, peaks);
            why.expect_err("refused").to_string()
        }
        let why = refused(&awc, &signal_blocks(19), 17 * 1024 + 1, &[1, 1, 1, 1]);
        assert!(why.contains("give it 1 to 17408 samples"), "{why}");
        assert!(refused(&awc, &signal_blocks(2), 2_000, &[1]).contains("take 3 blocks"));
        assert!(refused(&awc, &signal_blocks(3), 2_000, &[1, 1]).contains("peak windows"));
        let mut stated = signal_blocks(3);
        stated[4 * SPECTRA_BLOCK_FLOATS + 4] = 1;
        let why = refused(&awc, &stated, 2_000, &[1]);
        assert!(why.contains("block 1 states an imaginary part"), "{why}");
        let (_, pcm) = parse(pcm_stream(&tone(300, 5), None));
        let why = pcm.with_spectra_replaced(0, &signal_blocks(2), 300, &[1]);
        assert!(why
            .expect_err("refused")
            .to_string()
            .contains("no codec 2 record"));
    }
}
