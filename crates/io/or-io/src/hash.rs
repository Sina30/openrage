//! RAGE's string hashes: Bob Jenkins'
//! [one-at-a-time hash](http://burtleburtle.net/bob/hash/doobs.html).
//!
//! # Example
//!
//! ```
//! use or_io::hash::string_hash;
//!
//! // Runtime hash
//! let hash = string_hash(b"my_effect_name");
//!
//! // Compile-time hash
//! const MY_HASH: u32 = or_io::hash::string_hash(b"my_effect_name");
//! ```

/// `A`–`Z` to `a`–`z` and `\` to `/`; every other byte to itself.
#[rustfmt::skip]
const NORMALIZE_CASE_AND_SLASH: [u8; 256] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
    32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
    64, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
    112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122,
    91, 47, 93, 94, 95,
    96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
    112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122,
    123, 124, 125, 126, 127,
    128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143,
    144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159,
    160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175,
    176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191,
    192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207,
    208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223,
    224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239,
    240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255,
];

#[inline]
pub const fn finalize_hash(partial_hash: u32) -> u32 {
    let mut key = partial_hash;
    key = key.wrapping_add(key << 3);
    key ^= key >> 11;
    key = key.wrapping_add(key << 15);
    key
}

#[inline]
const fn hash_step(key: u32, byte: u8) -> u32 {
    let mut k = key.wrapping_add(byte as u32);
    k = k.wrapping_add(k << 10);
    k ^= k >> 6;
    k
}

#[inline]
const fn normalize_byte(byte: u8) -> u8 {
    NORMALIZE_CASE_AND_SLASH[byte as usize]
}

/// `atStringHash`: folds case, turns `\` into `/`, and hashes only what is
/// between the quotes when the string starts with one.
///
/// # Example
///
/// ```
/// use or_io::hash::string_hash;
///
/// // Runtime
/// let hash = string_hash(b"MyEffect");
/// assert_eq!(hash, string_hash(b"myeffect")); // Case insensitive
///
/// // Compile-time
/// const HASH: u32 = string_hash(b"MyEffect");
/// ```
#[inline]
pub const fn string_hash(bytes: &[u8]) -> u32 {
    finalize_hash(partial_string_hash(bytes, 0))
}

#[inline]
pub const fn partial_string_hash(bytes: &[u8], init_value: u32) -> u32 {
    let mut key = init_value;
    let len = bytes.len();

    let quotes = len > 0 && bytes[0] == b'"';
    let mut i = if quotes { 1 } else { 0 };

    while i < len {
        let byte = bytes[i];

        if quotes && byte == b'"' {
            break;
        }

        key = hash_step(key, normalize_byte(byte));
        i += 1;
    }

    key
}

/// [`string_hash`] with no case, slash or quote normalization.
///
/// # Example
///
/// ```
/// use or_io::hash::{literal_string_hash, string_hash};
///
/// let literal = literal_string_hash(b"MyEffect");
/// let normalized = string_hash(b"MyEffect");
/// assert_ne!(literal, normalized); // Different because of case
/// ```
#[inline]
pub const fn literal_string_hash(bytes: &[u8]) -> u32 {
    finalize_hash(partial_literal_hash(bytes, 0))
}

#[inline]
pub const fn partial_literal_hash(bytes: &[u8], init_value: u32) -> u32 {
    let mut key = init_value;
    let mut i = 0;
    while i < bytes.len() {
        key = hash_step(key, bytes[i]);
        i += 1;
    }
    key
}

#[inline]
pub fn string_hash_str(s: &str) -> u32 {
    string_hash(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_insensitive() {
        assert_eq!(string_hash(b"Hello"), string_hash(b"hello"));
        assert_eq!(string_hash(b"HELLO"), string_hash(b"hello"));
        assert_eq!(string_hash(b"HeLLo"), string_hash(b"hello"));
    }

    #[test]
    fn test_slash_normalization() {
        assert_eq!(string_hash(b"path\\to\\file"), string_hash(b"path/to/file"));
        assert_eq!(
            string_hash(b"C:\\Users\\Test"),
            string_hash(b"C:/Users/Test")
        );
    }

    #[test]
    fn test_quoted_strings() {
        assert_eq!(string_hash(b"\"hello\""), string_hash(b"hello"));
        assert_eq!(string_hash(b"\"test\"extra"), string_hash(b"test"));
    }

    #[test]
    fn test_literal_hash_differs() {
        assert_ne!(literal_string_hash(b"Hello"), literal_string_hash(b"hello"));
        assert_ne!(
            literal_string_hash(b"path\\file"),
            literal_string_hash(b"path/file")
        );
    }
}
