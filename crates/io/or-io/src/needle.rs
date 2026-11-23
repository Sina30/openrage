//! Finding a byte pattern in a file, and showing where it was.

/// How a pattern was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedleKind {
    /// Bytes, from a hex string.
    Binary,
    /// ASCII, one byte per character.
    Text,
    /// UTF-16 little-endian, which is how RAGE stores wide strings.
    Utf16,
}

/// A byte pattern to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Needle {
    bytes: Vec<u8>,
    kind: NeedleKind,
    case_sensitive: bool,
}

impl Needle {
    /// ⚠ Case insensitivity is **ASCII-only**, because the comparison is
    /// byte-wise against a file whose encoding is not known. A pattern with
    /// a non-ASCII letter in it is matched exactly whatever this says, and
    /// [`is_case_insensitive`](Needle::is_case_insensitive) reports what the
    /// needle will actually do rather than what was asked for.
    pub fn text(pattern: &str, case_sensitive: bool) -> Self {
        Self {
            bytes: pattern.as_bytes().to_vec(),
            kind: NeedleKind::Text,
            case_sensitive,
        }
    }

    pub fn utf16(pattern: &str, case_sensitive: bool) -> Self {
        let bytes = pattern
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>();
        Self {
            bytes,
            kind: NeedleKind::Utf16,
            case_sensitive,
        }
    }

    /// Parse a hex string. Whitespace, `,`, `-` and a leading `0x` are ignored.
    pub fn hex(pattern: &str) -> Result<Self, String> {
        let cleaned: String = pattern
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X")
            .chars()
            .filter(|c| !c.is_whitespace() && *c != ',' && *c != '-')
            .collect();
        if cleaned.is_empty() {
            return Err("a hex pattern needs at least one byte".into());
        }
        if !cleaned.len().is_multiple_of(2) {
            return Err(format!(
                "a hex pattern needs an even number of digits; got {}",
                cleaned.len()
            ));
        }
        let mut bytes = Vec::with_capacity(cleaned.len() / 2);
        let digits: Vec<char> = cleaned.chars().collect();
        for pair in digits.chunks(2) {
            let s: String = pair.iter().collect();
            bytes.push(u8::from_str_radix(&s, 16).map_err(|_| format!("`{s}` is not a hex byte"))?);
        }
        Ok(Self {
            bytes,
            kind: NeedleKind::Binary,
            case_sensitive: true,
        })
    }

    pub fn kind(&self) -> NeedleKind {
        self.kind
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether this needle will actually fold case, which is not the same as
    /// whether case insensitivity was asked for.
    pub fn is_case_insensitive(&self) -> bool {
        !self.case_sensitive && self.bytes.iter().any(u8::is_ascii_alphabetic)
    }

    /// Offsets of every non-overlapping occurrence, up to `limit`.
    ///
    /// Non-overlapping because a hit is a place a user will be shown, and
    /// `aa` in `aaaa` is two places rather than three.
    pub fn find_in(&self, haystack: &[u8], limit: usize) -> Vec<usize> {
        let mut out = Vec::new();
        if self.bytes.is_empty() || haystack.len() < self.bytes.len() || limit == 0 {
            return out;
        }
        let mut at = 0usize;
        while at + self.bytes.len() <= haystack.len() {
            if self.matches_at(haystack, at) {
                out.push(at);
                if out.len() >= limit {
                    break;
                }
                at += self.bytes.len();
            } else {
                at += 1;
            }
        }
        out
    }

    fn matches_at(&self, haystack: &[u8], at: usize) -> bool {
        let window = &haystack[at..at + self.bytes.len()];
        if self.case_sensitive {
            return window == self.bytes;
        }
        window
            .iter()
            .zip(&self.bytes)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }
}

/// A `xxd`-style window around `at`, for showing a hit in context.
pub fn hex_preview(bytes: &[u8], at: usize, width: usize) -> String {
    if bytes.is_empty() || width == 0 {
        return String::new();
    }
    let start = at.saturating_sub(width / 2) / 16 * 16;
    let end = (start + width).min(bytes.len());
    let mut out = String::new();
    for (row, chunk) in bytes[start..end].chunks(16).enumerate() {
        let base = start + row * 16;
        out.push_str(&format!("{base:08X}  "));
        for i in 0..16 {
            match chunk.get(i) {
                Some(b) => out.push_str(&format!("{b:02X} ")),
                None => out.push_str("   "),
            }
            if i == 7 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        for b in chunk {
            out.push(if (0x20..0x7F).contains(b) {
                *b as char
            } else {
                '.'
            });
        }
        out.push_str("|\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_needle_finds_every_non_overlapping_hit() {
        let needle = Needle::text("ab", true);
        assert_eq!(needle.find_in(b"xxabyyabzz", 10), vec![2, 6]);
        assert_eq!(needle.find_in(b"aaaa", 10), Vec::<usize>::new());
        assert_eq!(Needle::text("aa", true).find_in(b"aaaa", 10), vec![0, 2]);
        assert_eq!(needle.find_in(b"nothing here", 10), Vec::<usize>::new());
    }

    #[test]
    fn a_limit_stops_the_search_rather_than_truncating_the_answer() {
        let needle = Needle::text("a", true);
        assert_eq!(needle.find_in(b"aaaaa", 2), vec![0, 1]);
        assert_eq!(needle.find_in(b"aaaaa", 0), Vec::<usize>::new());
    }

    #[test]
    fn case_folding_is_ascii_only_and_says_so() {
        let folded = Needle::text("RailGun", false);
        assert_eq!(folded.find_in(b"weapon_railgun.meta", 4), vec![7]);
        assert!(folded.is_case_insensitive());

        let exact = Needle::text("RailGun", true);
        assert_eq!(
            exact.find_in(b"weapon_railgun.meta", 4),
            Vec::<usize>::new()
        );
        assert!(!exact.is_case_insensitive());

        // ⚠ Asked for insensitivity, and there is no ASCII letter to fold,
        // so the needle reports what it will actually do.
        let digits = Needle::text("1234", false);
        assert!(!digits.is_case_insensitive());
    }

    #[test]
    fn a_hex_needle_accepts_what_a_user_would_paste() {
        let expected = vec![0x52u8, 0x50, 0x46, 0x37];
        for spelling in ["52504637", "52 50 46 37", "0x52504637", "52,50,46,37"] {
            let needle = Needle::hex(spelling).unwrap_or_else(|e| panic!("{spelling}: {e}"));
            assert_eq!(needle.find_in(&expected, 1), vec![0]);
            assert_eq!(needle.len(), 4);
            assert_eq!(needle.kind(), NeedleKind::Binary);
        }
    }

    #[test]
    fn a_hex_needle_that_is_not_hex_is_refused() {
        assert!(Needle::hex("").is_err());
        assert!(Needle::hex("   ").is_err());
        assert!(Needle::hex("525").is_err());
        assert!(Needle::hex("zz").is_err());
        assert!(Needle::hex("0x").is_err());
    }

    /// ⚠ RAGE stores plenty of strings as UTF-16, where a byte search for
    /// the ASCII spelling finds nothing.
    #[test]
    fn a_utf16_needle_finds_a_wide_string_an_ascii_one_misses() {
        let wide: Vec<u8> = "hello".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            Needle::text("hello", true).find_in(&wide, 4),
            Vec::<usize>::new()
        );
        assert_eq!(Needle::utf16("hello", true).find_in(&wide, 4), vec![0]);
        assert_eq!(Needle::utf16("HELLO", false).find_in(&wide, 4), vec![0]);
    }

    #[test]
    fn a_preview_shows_the_row_the_hit_is_on() {
        let bytes: Vec<u8> = (0..64u8).collect();
        let preview = hex_preview(&bytes, 0x12, 32);
        assert!(preview.starts_with("00000000  00 01 02"), "{preview}");
        assert!(preview.contains("|"), "{preview}");
        assert_eq!(preview.lines().count(), 2);

        // A short buffer renders what there is rather than reading past it.
        let short = hex_preview(b"abc", 1, 32);
        assert_eq!(short.lines().count(), 1);
        assert!(short.contains("|abc|"), "{short}");
        assert!(hex_preview(&[], 0, 32).is_empty());
        assert!(hex_preview(b"abc", 0, 0).is_empty());
    }
}
