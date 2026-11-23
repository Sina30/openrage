//! What a source states about its sound and its picture, before any of it is decoded.

use std::time::Duration;

use crate::Codec;

/// A sound track as its container states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioInfo {
    pub codec: Codec,
    pub channels: u16,
    /// Samples per second of each channel.
    pub sample_rate: u32,
    /// Samples of each channel.
    pub frames: u64,
}

/// A picture track as its container states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoInfo {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    /// Pictures per second as numerator and denominator, zero where the container states none.
    pub frame_rate: (u32, u32),
    pub frames: u64,
}

impl VideoInfo {
    /// Pictures per second, or `None` when no usable rate is stated.
    pub fn fps(&self) -> Option<f64> {
        let (numerator, denominator) = self.frame_rate;
        (numerator != 0 && denominator != 0).then(|| f64::from(numerator) / f64::from(denominator))
    }
}

/// Every track a source holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MediaInfo {
    pub audio: Option<AudioInfo>,
    pub video: Option<VideoInfo>,
}

impl MediaInfo {
    /// The longest track's length, or `None` when no track states a rate to measure it by.
    pub fn duration(&self) -> Option<Duration> {
        let sound = self
            .audio
            .filter(|a| a.sample_rate != 0)
            .map(|a| a.frames as f64 / f64::from(a.sample_rate));
        let picture = self
            .video
            .and_then(|v| v.fps().map(|fps| v.frames as f64 / fps));
        let seconds = match (sound, picture) {
            (Some(s), Some(p)) => s.max(p),
            (s, p) => s.or(p)?,
        };
        Duration::try_from_secs_f64(seconds).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(frames: u64, sample_rate: u32) -> AudioInfo {
        AudioInfo {
            codec: Codec::Pcm16,
            channels: 1,
            sample_rate,
            frames,
        }
    }

    fn picture(frames: u64, frame_rate: (u32, u32)) -> VideoInfo {
        VideoInfo {
            codec: Codec::Bink1,
            width: 640,
            height: 360,
            frame_rate,
            frames,
        }
    }

    #[test]
    fn a_duration_is_the_longest_track_by_its_own_rate() {
        let both = MediaInfo {
            audio: Some(sound(48_000, 24_000)),
            video: Some(picture(90, (30, 1))),
        };
        assert_eq!(both.duration(), Some(Duration::from_secs(3)));
        let only_sound = MediaInfo {
            audio: Some(sound(48_000, 24_000)),
            video: None,
        };
        assert_eq!(only_sound.duration(), Some(Duration::from_secs(2)));
        assert_eq!(MediaInfo::default().duration(), None);
    }

    #[test]
    fn a_track_with_no_rate_has_no_duration() {
        let none = MediaInfo {
            audio: Some(sound(10, 0)),
            video: Some(picture(10, (30, 0))),
        };
        assert_eq!(none.duration(), None);
        assert_eq!(none.video.and_then(|v| v.fps()), None);
        assert_eq!(picture(1, (30_000, 1_001)).fps(), Some(30_000.0 / 1_001.0));
    }
}
