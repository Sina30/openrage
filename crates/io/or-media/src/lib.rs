//! Decodes every audio and video format OpenRage meets, behind one interface.
//!
//! A [`Source`] states what it holds ([`MediaInfo`]) and opens its tracks as an [`AudioStream`] of
//! interleaved samples or a [`VideoStream`] of RGBA frames. The codec a track is stored in picks a
//! row of the [`Decoders`] table, so a codec added later is one row, and a codec with no row is
//! refused by name while its info still reads. Containers are [`awc`] and [`bik`], [`bink`] holds
//! the Bink 1 picture and Bink audio decoders, and [`bink2`] the Bink 2 picture decoder.
//!
//! Decoders are OpenRage's own Rust, ported from open implementations where that is the fastest
//! correct route. A ported file keeps a header naming its origin, upstream path, revision and
//! licence, and this crate's `Cargo.toml` states the strictest licence it holds.

pub mod awc;
pub mod bik;
pub mod bink;
pub mod bink2;
mod bits;
mod codec;
mod decoders;
mod error;
mod info;
pub mod opus;
pub mod spectra;
mod stream;
mod transform;
mod xma;
mod yuv;

use std::sync::Arc;

pub use codec::Codec;
pub use decoders::{Decoder, Decoders, OpenAudio, OpenVideo};
pub use error::MediaError;
pub use info::{AudioInfo, MediaInfo, VideoInfo};
pub use stream::{AudioStream, Frame, Pcm, Samples, VideoStream};
pub use yuv::{Plane, Yuv420};

/// A file, or one stream of one, that holds sound or picture.
pub trait Source: Send + Sync {
    fn info(&self) -> &MediaInfo;

    /// The sound track, decoded through the row `decoders` holds for its codec.
    fn audio(&self, _decoders: &Decoders) -> Result<Box<dyn AudioStream>, MediaError> {
        Err(MediaError::NoAudio)
    }

    /// The picture track, decoded through the row `decoders` holds for its codec.
    fn video(&self, _decoders: &Decoders) -> Result<Box<dyn VideoStream>, MediaError> {
        Err(MediaError::NoVideo)
    }
}

/// Opens a file that is one source: a movie. An `.awc` holds many, so [`awc::open`] takes the one wanted.
pub fn open(bytes: impl Into<Arc<[u8]>>) -> Result<Box<dyn Source>, MediaError> {
    let bytes = bytes.into();
    if bik::is_bink(&bytes) {
        return bik::open(bytes);
    }
    Err(MediaError::Container(
        "OpenRage reads no media container that starts like this".to_string(),
    ))
}
