use std::io::{Read, Write};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;

use crate::error::{IoError, Result};

pub fn dat_decompress(compressed: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(compressed);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| IoError::DecompressionFailed(e.to_string()))?;
    Ok(decompressed)
}

/// Inflate no more than `limit` bytes, the length the container states (RSC7
/// in its page flags, an RPF entry in its size), and refuse a payload that
/// would inflate past it.
pub fn dat_decompress_bounded(compressed: &[u8], limit: usize) -> Result<Vec<u8>> {
    let decoder = DeflateDecoder::new(compressed);
    let mut decompressed = Vec::new();
    let ceiling = limit.saturating_add(1) as u64;
    decoder
        .take(ceiling)
        .read_to_end(&mut decompressed)
        .map_err(|e| IoError::DecompressionFailed(e.to_string()))?;
    if decompressed.len() > limit {
        return Err(IoError::DecompressionFailed(format!(
            "payload inflates past the {limit} bytes it declares; \
             refusing to keep expanding it"
        )));
    }
    Ok(decompressed)
}

const LEGACY_PAYLOAD_PREFIX: usize = 8;

/// Decompress an RSC7 payload whose length the page flags state, preferring
/// the attempt that produces exactly `expected_len` bytes.
///
/// ⚠ A DEFLATE stream carries no length, so a decode from the wrong offset can
/// *succeed* short: 817 of an expected 1,236,992 bytes for
/// `des_smash_root_merge.ydr`, 128 of 98,304 for `des_hosp_ceil.ycd`. The
/// length check is what makes trying more than one offset safe.
///
/// 12 `.ydr` at version 164 and 4 `.ycd` at version 43 in a retail Legacy
/// install put 8 opaque bytes ahead of the stream (not a length: the values
/// exceed the payloads); each inflates to its stated size from offset 8, and
/// none from 0, 4 or 16.
///
/// Tried in order: raw DEFLATE, accepted on an exact match; after the 8-byte
/// prefix, accepted only on an exact match, since otherwise nothing tells it
/// from garbage that happened to inflate; stored uncompressed when the payload
/// is already `expected_len`; and last whatever the first attempt produced, so
/// nothing [`dat_decompress`] accepts is refused here. The length prefers an attempt and never gates one, because a resource built
/// with zero page flags states a length of 0 while carrying a real payload.
pub fn dat_decompress_expecting(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    let inflates_from_the_start = match dat_decompress(compressed) {
        Ok(data) if data.len() == expected_len => return Ok(data),
        Ok(_) => true,
        Err(_) => false,
    };

    if expected_len > 0 && compressed.len() > LEGACY_PAYLOAD_PREFIX {
        if let Ok(data) = dat_decompress(&compressed[LEGACY_PAYLOAD_PREFIX..]) {
            if data.len() == expected_len {
                tracing::debug!(
                    prefix_bytes = LEGACY_PAYLOAD_PREFIX,
                    expected_len,
                    "RSC7 payload carried a prefix ahead of its DEFLATE stream"
                );
                return Ok(data);
            }
        }
    }

    if compressed.len() == expected_len && expected_len > 0 {
        tracing::debug!(expected_len, "RSC7 payload is stored uncompressed");
        return Ok(compressed.to_vec());
    }

    if inflates_from_the_start {
        let data = dat_decompress(compressed)?;
        tracing::debug!(
            inflated = data.len(),
            expected_len,
            "RSC7 payload inflated to a length the page flags do not predict"
        );
        return Ok(data);
    }

    Err(IoError::DecompressionFailed(format!(
        "the {} payload bytes are not raw DEFLATE; also tried skipping an \
         {LEGACY_PAYLOAD_PREFIX}-byte prefix and reading them as stored-uncompressed, and \
         neither produced the {expected_len} bytes the page flags declare",
        compressed.len()
    )))
}

pub fn dat_compress(data: &[u8], level: Compression) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), level);
    encoder
        .write_all(data)
        .map_err(|e| IoError::CompressionFailed(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| IoError::CompressionFailed(e.to_string()))
}

pub fn dat_compress_best(data: &[u8]) -> Result<Vec<u8>> {
    dat_compress(data, Compression::best())
}

pub fn dat_compress_default(data: &[u8]) -> Result<Vec<u8>> {
    dat_compress(data, Compression::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        let original = b"Hello, RAGE engine! This is a test of compression.";

        let compressed = dat_compress_best(original).unwrap();
        let decompressed = dat_decompress(&compressed).unwrap();

        assert_eq!(&decompressed, original);
    }

    #[test]
    fn test_empty_data() {
        let original: &[u8] = &[];

        let compressed = dat_compress_best(original).unwrap();
        let decompressed = dat_decompress(&compressed).unwrap();

        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_large_data() {
        let original: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();

        let compressed = dat_compress_best(&original).unwrap();
        let decompressed = dat_decompress(&compressed).unwrap();

        assert_eq!(decompressed, original);
        assert!(compressed.len() < original.len());
    }

    fn payload() -> Vec<u8> {
        (0..4096u32).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn expecting_accepts_an_ordinary_payload() {
        let original = payload();
        let compressed = dat_compress_best(&original).unwrap();
        let out = dat_decompress_expecting(&compressed, original.len()).unwrap();
        assert_eq!(out, original);
    }

    #[test]
    fn expecting_skips_the_eight_byte_prefix() {
        let original = payload();
        let mut prefixed = vec![0x5F, 0xB8, 0x2B, 0xEF, 0xFB, 0x76, 0x40, 0xA9];
        prefixed.extend_from_slice(&dat_compress_best(&original).unwrap());

        assert!(
            dat_decompress(&prefixed).is_err()
                || dat_decompress(&prefixed).unwrap().len() != original.len()
        );

        let out = dat_decompress_expecting(&prefixed, original.len()).unwrap();
        assert_eq!(out, original);
    }

    #[test]
    fn expecting_reads_a_stored_payload() {
        let original = payload();
        let out = dat_decompress_expecting(&original, original.len()).unwrap();
        assert_eq!(out, original);
    }

    #[test]
    fn expecting_falls_back_when_the_page_flags_disagree() {
        let original = payload();
        let compressed = dat_compress_best(&original).unwrap();
        let out = dat_decompress_expecting(&compressed, 0).unwrap();
        assert_eq!(
            out, original,
            "a zero expected length must not reject a payload that inflates"
        );
    }

    #[test]
    fn a_zero_expected_length_never_endorses_an_attempt() {
        for len in [16usize, 512, 4096] {
            let original = vec![0u8; len];
            let compressed = dat_compress_best(&original).unwrap();
            let out = dat_decompress_expecting(&compressed, 0).unwrap();
            assert_eq!(
                out.len(),
                len,
                "a {len}-byte payload came back as {} bytes",
                out.len()
            );
        }
    }

    #[test]
    fn expecting_does_not_accept_a_prefix_skip_it_cannot_confirm() {
        let original = payload();
        let mut prefixed = vec![0u8; 8];
        prefixed.extend_from_slice(&dat_compress_best(&original).unwrap());

        let out = dat_decompress_expecting(&prefixed, original.len() + 1);
        match out {
            Err(_) => {}
            Ok(data) => assert_ne!(
                data, original,
                "the prefix skip was accepted without a confirming length"
            ),
        }
    }

    #[test]
    fn expecting_names_what_it_tried_when_everything_fails() {
        let err = dat_decompress_expecting(&[0xFF; 64], 4096).unwrap_err();
        let message = err.to_string();
        for expected in ["8-byte prefix", "stored-uncompressed", "4096"] {
            assert!(
                message.contains(expected),
                "error should mention {expected:?}: {message}"
            );
        }
    }

    #[test]
    fn expecting_tolerates_a_payload_shorter_than_the_prefix() {
        for short in [&[][..], &[0x01][..], &[0x01, 0x02][..], &[0xFF; 7][..]] {
            let _ = dat_decompress_expecting(short, 4096);
            let _ = dat_decompress_expecting(short, 0);
            let _ = dat_decompress_expecting(short, short.len());
        }
    }
}
