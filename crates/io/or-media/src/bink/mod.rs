//! Bink 1: video and audio decoders ported from FFmpeg, with the tables and pixel arithmetic they share.
//!
//! Origin: OpenRage's own. This file only declares the modules, and each of them names its own origin.

pub mod audio;
mod dsp;
mod tables;
pub mod video;
