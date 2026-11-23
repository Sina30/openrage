//! Oodle, the compressor RDR2's RPF8 bodies default to: Kraken through a pure-Rust port, and LZB16 through
//! [`super::lzb16`], whichever the stream's own header names.
//!
//! ⚠ Writing is LZB16 only: no open Kraken encoder exists, and the game's decoder reads either by the
//! type in a stream's header.

use crate::error::{IoError, Result};

/// The most a body may inflate to, so a hostile row cannot ask for an unbounded buffer.
const LIMIT: usize = 1 << 30;

/// An Oodle stream of exactly `wanted` bytes, or an error naming what went wrong.
pub fn decompress(stream: &[u8], wanted: usize) -> Result<Vec<u8>> {
    if wanted > LIMIT {
        return Err(IoError::DecompressionFailed(format!(
            "an Oodle body that inflates to {wanted} bytes"
        )));
    }
    if super::lzb16::is_lzb16(stream) {
        return super::lzb16::decode(stream, wanted);
    }
    kraken(stream, wanted)
}

/// `data` as an Oodle stream the game reads.
pub fn compress(data: &[u8]) -> Vec<u8> {
    super::lzb16::encode(data)
}

#[cfg(feature = "oodle")]
fn kraken(stream: &[u8], wanted: usize) -> Result<Vec<u8>> {
    or_kraken::decode(stream, wanted)
        .map_err(|why| IoError::DecompressionFailed(format!("Oodle: {why}")))
}

#[cfg(not(feature = "oodle"))]
fn kraken(_stream: &[u8], _wanted: usize) -> Result<Vec<u8>> {
    Err(IoError::DecompressionFailed(
        "the body is Oodle Kraken and this build has no `oodle` feature; it can still be read as \
         stored and carried into a rebuilt archive"
            .into(),
    ))
}
