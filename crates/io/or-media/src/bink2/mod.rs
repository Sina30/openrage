//! Bink 2: the picture decoder ported from NihAV, with the tables and pixel arithmetic it shares and the
//! in-loop filter derived from RAD's `bink2w64.dll` (see `filter.rs`).

mod dsp;
mod filter;
mod tables;
pub mod video;
