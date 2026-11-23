//! What a source or a stream refuses, in words the app shows as they are.

use crate::Codec;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    /// The table has no row for the codec.
    #[error("this is {}, which OpenRage has no decoder for", .0.label())]
    NoDecoder(Codec),
    #[error("there is no sound in this")]
    NoAudio,
    #[error("there is no picture in this")]
    NoVideo,
    #[error("this cannot seek")]
    Unseekable,
    /// The container is not readable as it stands, or lacks what reading it needs.
    #[error("{0}")]
    Container(String),
    /// A decoder, or the route to one, did not produce what the source states.
    #[error("{0}")]
    Decode(String),
    /// An encoder, or the route to one, refused what it was given to encode.
    #[error("{0}")]
    Encode(String),
}
