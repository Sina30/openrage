//! `metadata.dat`: the file the game hands the Social Club library by path
//! at start-up. It sits loose in `x64/` and `update/x64/`, identical across
//! both editions, and holds the title's localized activity and achievement
//! text for that library.
//!
//! Only the header is measured: a big-endian word, `META` and a build
//! timestamp. The body's layout (a hash-sorted index ahead of the strings)
//! is not, so it is kept as its stated bytes.

use crate::error::{IoError, Result};

const MAGIC: &[u8; 4] = b"META";
const BUILT_LEN: usize = 19;
const HEADER_LEN: usize = 4 + 4 + BUILT_LEN;

/// One `metadata.dat`: its header and the body as stated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocialClubMetadata {
    /// The big-endian word ahead of `META`: 1 in every shipped file; its meaning is not measured.
    pub format_word: u32,
    /// The build time, `YYYY-MM-DDTHH:MM:SS`.
    pub built: String,
    /// Everything after the header, whose layout is not measured.
    pub body: Vec<u8>,
}

impl SocialClubMetadata {
    /// Reads a whole `metadata.dat`, refusing one without the `META` header.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let header = bytes
            .get(..HEADER_LEN)
            .ok_or_else(|| refuse(format!("{} bytes is shorter than the header", bytes.len())))?;
        if &header[4..8] != MAGIC {
            return Err(refuse("the magic is not `META`".into()));
        }
        let built = std::str::from_utf8(&header[8..])
            .ok()
            .filter(|t| is_timestamp(t))
            .ok_or_else(|| refuse("the build time is not `YYYY-MM-DDTHH:MM:SS`".into()))?;
        Ok(Self {
            format_word: u32::from_be_bytes(header[..4].try_into().expect("four bytes")),
            built: built.to_string(),
            body: bytes[HEADER_LEN..].to_vec(),
        })
    }

    /// Writes the header and the body back.
    pub fn save(&self) -> Result<Vec<u8>> {
        if !is_timestamp(&self.built) {
            return Err(refuse(format!(
                "the build time {:?} is not `YYYY-MM-DDTHH:MM:SS`",
                self.built
            )));
        }
        let mut out = Vec::with_capacity(HEADER_LEN + self.body.len());
        out.extend_from_slice(&self.format_word.to_be_bytes());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(self.built.as_bytes());
        out.extend_from_slice(&self.body);
        Ok(out)
    }
}

fn is_timestamp(text: &str) -> bool {
    text.len() == BUILT_LEN
        && text.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            10 => b == b'T',
            13 | 16 => b == b':',
            _ => b.is_ascii_digit(),
        })
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("metadata.dat: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_reads_and_the_body_is_kept_as_stated() {
        let bytes = b"\0\0\0\x01META2024-02-01T15:54:51\x02\x04\x04\0".to_vec();
        let metadata = SocialClubMetadata::open(&bytes).expect("opens");
        assert_eq!(metadata.format_word, 1);
        assert_eq!(metadata.built, "2024-02-01T15:54:51");
        assert_eq!(metadata.body, [2, 4, 4, 0]);
        assert_eq!(metadata.save().expect("saves"), bytes);

        let mut wrong = bytes.clone();
        wrong[4] = b'X';
        assert!(SocialClubMetadata::open(&wrong).is_err());
        wrong = bytes.clone();
        wrong[18] = b'x';
        assert!(SocialClubMetadata::open(&wrong).is_err());
        assert!(SocialClubMetadata::open(&bytes[..20]).is_err());
    }
}
