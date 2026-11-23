//! Decoded sound and picture, pulled from a source one block or one frame at a time.

use std::ops::Range;

use crate::MediaError;

/// Interleaved samples, in whichever type the decoder produced them.
#[derive(Debug, Clone, PartialEq)]
pub enum Samples {
    I16(Vec<i16>),
    /// Between -1.0 and 1.0.
    F32(Vec<f32>),
}

impl Samples {
    pub fn len(&self) -> usize {
        match self {
            Samples::I16(samples) => samples.len(),
            Samples::F32(samples) => samples.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The samples as `i16`, a float one rounded and clamped.
    pub fn into_i16(self) -> Vec<i16> {
        match self {
            Samples::I16(samples) => samples,
            Samples::F32(samples) => samples
                .into_iter()
                .map(|s| (s * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
                .collect(),
        }
    }

    /// The samples as `f32`, an `i16` one scaled by 1 / 32768.
    pub fn into_f32(self) -> Vec<f32> {
        match self {
            Samples::I16(samples) => samples
                .into_iter()
                .map(|s| f32::from(s) / 32768.0)
                .collect(),
            Samples::F32(samples) => samples,
        }
    }

    fn range(&self, range: Range<usize>) -> Samples {
        match self {
            Samples::I16(samples) => Samples::I16(samples[range].to_vec()),
            Samples::F32(samples) => Samples::F32(samples[range].to_vec()),
        }
    }
}

/// A sound track, pulled in blocks of interleaved samples.
pub trait AudioStream: Send {
    /// The next block, or `None` once the track has ended.
    fn read(&mut self) -> Result<Option<Samples>, MediaError>;

    /// Moves to sample frame `frame`, one sample of every channel; a format that cannot says so.
    fn seek(&mut self, _frame: u64) -> Result<(), MediaError> {
        Err(MediaError::Unseekable)
    }

    /// Every sample still to come, as `i16`.
    fn read_all_i16(&mut self) -> Result<Vec<i16>, MediaError> {
        let mut all = Vec::new();
        while let Some(block) = self.read()? {
            all.extend(block.into_i16());
        }
        Ok(all)
    }
}

/// One picture: `width` × `height` pixels of RGBA, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A picture track, pulled a frame at a time.
pub trait VideoStream: Send {
    /// The next picture, or `None` once the track has ended.
    fn next_frame(&mut self) -> Result<Option<Frame>, MediaError>;

    /// Pictures per second the frames come at, as numerator and denominator; zero where unknown.
    fn frame_rate(&self) -> (u32, u32);

    /// Moves to frame `frame`; a format that cannot says so.
    fn seek(&mut self, _frame: u64) -> Result<(), MediaError> {
        Err(MediaError::Unseekable)
    }
}

/// Sample frames a [`Pcm`] hands over per read.
const BLOCK_FRAMES: usize = 4096;

/// Samples already decoded, read back in blocks: what a decoder with no streaming of its own returns.
#[derive(Debug, Clone)]
pub struct Pcm {
    samples: Samples,
    channels: usize,
    at: usize,
}

impl Pcm {
    pub fn new(samples: Samples, channels: u16) -> Self {
        Self {
            samples,
            channels: usize::from(channels.max(1)),
            at: 0,
        }
    }
}

impl AudioStream for Pcm {
    fn read(&mut self) -> Result<Option<Samples>, MediaError> {
        let end = (self.at + BLOCK_FRAMES * self.channels).min(self.samples.len());
        if self.at >= end {
            return Ok(None);
        }
        let block = self.samples.range(self.at..end);
        self.at = end;
        Ok(Some(block))
    }

    fn seek(&mut self, frame: u64) -> Result<(), MediaError> {
        let at = usize::try_from(frame).map_or(usize::MAX, |f| f.saturating_mul(self.channels));
        self.at = at.min(self.samples.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_i16_sample_survives_a_trip_through_f32() {
        let all: Vec<i16> = (i16::MIN..=i16::MAX).collect();
        let back = Samples::F32(Samples::I16(all.clone()).into_f32()).into_i16();
        assert_eq!(back, all);
    }

    #[test]
    fn a_float_outside_full_scale_clamps_instead_of_wrapping() {
        let got = Samples::F32(vec![2.0, -2.0, 1.0, -1.0, 0.0]).into_i16();
        assert_eq!(got, vec![32767, -32768, 32767, -32768, 0]);
    }

    #[test]
    fn a_buffer_is_read_in_blocks_of_whole_frames_and_seeks_by_frame() {
        let channels = 2;
        let frames = BLOCK_FRAMES * 2 + 10;
        let all: Vec<i16> = (0..frames * channels).map(|i| i as i16).collect();
        let mut pcm = Pcm::new(Samples::I16(all.clone()), channels as u16);

        let first = pcm.read().expect("a block").expect("not the end");
        assert_eq!(
            first.len(),
            BLOCK_FRAMES * channels,
            "a block is whole frames"
        );
        assert_eq!(pcm.read_all_i16().expect("the rest"), all[first.len()..]);
        assert_eq!(pcm.read().expect("the end"), None);

        pcm.seek(BLOCK_FRAMES as u64 + 3).expect("a buffer seeks");
        assert_eq!(
            pcm.read_all_i16().expect("from the frame"),
            all[(BLOCK_FRAMES + 3) * channels..]
        );
        pcm.seek(u64::MAX).expect("past the end is the end");
        assert_eq!(pcm.read().expect("the end"), None);
    }

    #[test]
    fn a_stream_that_does_not_say_how_to_seek_cannot() {
        struct Never;
        impl VideoStream for Never {
            fn next_frame(&mut self) -> Result<Option<Frame>, MediaError> {
                Ok(None)
            }
            fn frame_rate(&self) -> (u32, u32) {
                (0, 0)
            }
        }
        assert_eq!(Never.seek(0), Err(MediaError::Unseekable));
    }
}
