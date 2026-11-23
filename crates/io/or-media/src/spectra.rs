//! GTA VI's codec 2: a mono signal stored as overlapping 2,048-point spectra, decoded and encoded.
//!
//! Block `b` holds bins 0 to 1,023 of the real FFT of samples `(b − 1)·1,024` to `(b + 1)·1,024`;
//! the Nyquist bin is not stored, so each block's inverse is off by `a·(−1)^n`, recovered from the
//! silence before sample 0 and from each overlap.

use or_io::awc::codec::peak_window_count;
use or_io::awc::{AwcFile, Codec as AwcCodec};

use crate::transform::{Fft, Rdft};
use crate::{AudioInfo, AudioStream, MediaError, Pcm, Samples};

/// Samples of one block's window.
const WINDOW: usize = 2048;

/// Samples from one block's window to the next.
const HOP: usize = 1024;

/// Samples of one peak window.
const PEAK_WINDOW: usize = 4096;

/// The loudest window's peak word: a stream states its peaks as if its loudest sample were 26,028.
const LOUDEST_PEAK: f64 = 52_056.0;

/// Opens a codec-2 stream's data as the samples `info` states, as `f32`.
pub(crate) fn open(info: &AudioInfo, data: &[u8]) -> Result<Box<dyn AudioStream>, MediaError> {
    if info.channels != 1 {
        return Err(MediaError::Decode(format!(
            "a codec 2 stream is mono, and this one states {} channels",
            info.channels
        )));
    }
    let samples = usize::try_from(info.frames)
        .map_err(|_| MediaError::Decode("the stream states more samples than fit".into()))?;
    Ok(Box::new(Pcm::new(Samples::F32(decode(data, samples)?), 1)))
}

/// `(−1)^n`.
fn sign(n: usize) -> f64 {
    if n.is_multiple_of(2) {
        1.0
    } else {
        -1.0
    }
}

/// The mean of `x[n]·(−1)^n` over one hop.
fn alternating(x: impl Iterator<Item = f64>) -> f64 {
    x.enumerate().map(|(n, v)| v * sign(n)).sum::<f64>() / HOP as f64
}

/// The first `samples` of the signal `data`'s blocks hold, each block's Nyquist term restored.
pub(crate) fn decode(data: &[u8], samples: usize) -> Result<Vec<f32>, MediaError> {
    let blocks = samples.div_ceil(HOP);
    let need = blocks.checked_mul(4 * WINDOW);
    let Some(stored) = need.and_then(|need| data.get(..need)) else {
        return Err(MediaError::Decode(format!(
            "a codec 2 stream of {samples} samples takes {blocks} blocks of 8,192 bytes, and its \
             data holds {} bytes",
            data.len()
        )));
    };
    let rdft = Rdft::new(WINDOW);
    // Bins 0 to 1,023 as stored, and the Nyquist bin, not stored, left zero.
    let mut bins = vec![0.0f32; WINDOW + 2];
    let mut this = vec![0.0f32; WINDOW];
    let mut before = vec![0.0f32; WINDOW];
    let mut a = 0.0f64;
    let mut out = Vec::with_capacity(blocks * HOP);
    for (b, block) in stored.as_chunks::<{ 4 * WINDOW }>().0.iter().enumerate() {
        for (bin, float) in bins.iter_mut().zip(block.as_chunks::<4>().0) {
            *bin = f32::from_le_bytes(*float);
        }
        rdft.apply(&bins, 1.0 / WINDOW as f32, &mut this);
        let wide = |x: &f32| f64::from(*x);
        a = if b == 0 {
            // Block 0's first half is the silence before sample 0.
            -alternating(this[..HOP].iter().map(wide))
        } else {
            let overlap = before[HOP..].iter().zip(&this[..HOP]);
            a + alternating(overlap.map(|(p, t)| wide(p) - wide(t)))
        };
        let second = this[HOP..].iter().enumerate();
        out.extend(second.map(|(n, t)| (wide(t) + a * sign(n)) as f32));
        std::mem::swap(&mut before, &mut this);
    }
    out.truncate(samples);
    Ok(out)
}

/// The signal blocks of `signal`, from block 0 to block `ceil(len / 1,024)`, as `f32` bytes.
pub(crate) fn encode(signal: &[f64]) -> Vec<u8> {
    let blocks = signal.len().div_ceil(HOP) + 1;
    let fft = Fft::new(WINDOW);
    let mut out = Vec::with_capacity(blocks * 4 * WINDOW);
    let (mut re, mut im) = (vec![0.0f64; WINDOW], vec![0.0f64; WINDOW]);
    for b in 0..blocks {
        for (n, (r, i)) in re.iter_mut().zip(im.iter_mut()).enumerate() {
            let at = (b * HOP + n).checked_sub(HOP);
            (*r, *i) = (
                at.and_then(|at| signal.get(at)).copied().unwrap_or(0.0),
                0.0,
            );
        }
        // The transform runs `e^(+i)`, so the forward FFT of a real window is its conjugate.
        fft.inverse(&mut re, &mut im);
        for (bin, (r, i)) in re.iter().zip(&im).take(HOP).enumerate() {
            let imaginary = if bin == 0 { 0.0 } else { -i };
            out.extend((*r as f32).to_le_bytes());
            out.extend((imaginary as f32).to_le_bytes());
        }
    }
    out
}

/// GTA V's peak windows of `signal`, scaled so its loudest sample states 52,056; `None` if silent.
fn peaks(signal: &[f64]) -> Option<Vec<u16>> {
    let top = |x: &[f64]| x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let loudest = top(signal);
    if loudest == 0.0 {
        return None;
    }
    let windows = 1 + peak_window_count(signal.len());
    let peak = |w: usize| {
        let window = &signal[w * PEAK_WINDOW..((w + 1) * PEAK_WINDOW).min(signal.len())];
        (LOUDEST_PEAK * top(window) / loudest).round().min(65_535.0) as u16
    };
    Some((0..windows).map(peak).collect())
}

/// `file` with codec-2 stream `stream` storing a WAV's mono PCM16 bytes as its signal.
pub fn replaced(
    file: &AwcFile,
    stream: usize,
    pcm: &[u8],
    rate: u32,
    channels: u16,
) -> Result<AwcFile, MediaError> {
    let refuse = |why: String| Err(MediaError::Encode(why));
    let Some(s) = file.streams.get(stream) else {
        return refuse(format!("there is no stream {stream}"));
    };
    let format = (s.coded_format()).map_err(|e| MediaError::Encode(e.to_string()))?;
    let Some(format) = format.filter(|f| f.codec == AwcCodec::SPECTRA) else {
        return refuse(format!("stream {stream} is not a codec 2 stream"));
    };
    let stream_rate = u32::from(format.stated_rate);
    if rate != stream_rate {
        return refuse(format!(
            "the WAV is {rate} Hz and this codec 2 stream is {stream_rate} Hz, and OpenRage \
             resamples nothing: save it at {stream_rate} Hz"
        ));
    }
    if channels != 1 {
        return refuse(format!(
            "the WAV has {channels} channels and this stream has one: save it as mono"
        ));
    }
    let signal: Vec<f64> = (pcm.as_chunks::<2>().0.iter())
        .map(|b| f64::from(i16::from_le_bytes(*b)) / 32_768.0)
        .collect();
    let Ok(samples) = u32::try_from(signal.len()) else {
        return refuse("the WAV holds more samples than a stream states".to_string());
    };
    let Some(peaks) = peaks(&signal) else {
        return refuse(
            "the WAV is silent, and a codec 2 stream states its peaks against its loudest sample"
                .to_string(),
        );
    };
    let edited = file.with_spectra_replaced(stream, &encode(&signal), samples, &peaks);
    edited.map_err(|e| MediaError::Encode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decaying signal of `samples` whose every other sample flips sign: a large Nyquist term in
    /// every block, which a decode that does not restore it gets wrong.
    fn ringing(samples: usize) -> Vec<f64> {
        let at = |n: usize| {
            let t = n as f64;
            0.6 * sign(n) * (-t / 3_000.0).exp() + 0.2 * (t / 37.0).sin()
        };
        (0..samples).map(at).collect()
    }

    #[test]
    fn a_signal_encoded_decodes_to_itself_with_its_nyquist_terms_restored() {
        for samples in [1, 1_000, 1_024, 5_000] {
            let signal = ringing(samples);
            let blocks = encode(&signal);
            assert_eq!(blocks.len(), (samples.div_ceil(HOP) + 1) * 4 * WINDOW);
            for block in blocks.chunks(4 * WINDOW) {
                assert_eq!(block[4..8], [0; 4], "bin 0 states no imaginary part");
            }
            let decoded = decode(&blocks, samples).expect("it decodes");
            assert_eq!(decoded.len(), samples);
            let worst = (signal.iter().zip(&decoded))
                .map(|(s, d)| (s - f64::from(*d)).abs())
                .fold(0.0, f64::max);
            assert!(worst < 1e-5, "{samples} samples: off by {worst}");
        }
    }

    #[test]
    fn a_decode_that_left_the_nyquist_bin_out_would_be_wrong() {
        // The control: block 1's own inverse, unrestored, is far from samples 0 to 1,023 it holds.
        let signal = ringing(3_000);
        let blocks = encode(&signal);
        let mut bins = vec![0.0f32; WINDOW + 2];
        let block = &blocks[4 * WINDOW..8 * WINDOW];
        for (bin, float) in bins.iter_mut().zip(block.as_chunks::<4>().0) {
            *bin = f32::from_le_bytes(*float);
        }
        let mut raw = vec![0.0f32; WINDOW];
        Rdft::new(WINDOW).apply(&bins, 1.0 / WINDOW as f32, &mut raw);
        let worst = (signal.iter().zip(&raw[..HOP]))
            .map(|(s, r)| (s - f64::from(*r)).abs())
            .fold(0.0, f64::max);
        assert!(worst > 1e-3, "off by only {worst}");
    }

    #[test]
    fn short_data_and_a_stereo_record_are_refused_not_read_past() {
        let blocks = encode(&ringing(2_000));
        assert!(decode(&blocks[..4 * WINDOW], 2_000).is_err());
        assert!(decode(&blocks, usize::MAX).is_err());
        let info = AudioInfo {
            codec: crate::Codec::Spectra,
            channels: 2,
            sample_rate: 48_000,
            frames: 10,
        };
        assert!(open(&info, &blocks).is_err());
    }

    #[test]
    fn the_peaks_are_the_windows_scaled_to_the_loudest_sample() {
        let mut signal = vec![0.0; 9_000];
        (signal[100], signal[5_000]) = (-0.5, 0.25);
        assert_eq!(peaks(&signal), Some(vec![52_056, 26_028]));
        assert_eq!(peaks(&[0.0; 10]), None);
        assert_eq!(peaks(&[0.1; 10]).map(|p| p.len()), Some(1));
    }
}
