//! A text document: its characters, and the encoding and line endings it is written back in.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Latin1,
}

impl TextEncoding {
    pub fn label(&self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf8Bom => "UTF-8 BOM",
            TextEncoding::Utf16Le => "UTF-16 LE",
            TextEncoding::Utf16Be => "UTF-16 BE",
            TextEncoding::Latin1 => "Latin-1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
    Mixed,
}

impl LineEnding {
    pub fn label(&self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
            LineEnding::Mixed => "Mixed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextDocument {
    pub text: ropey::Rope,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
}

impl TextDocument {
    pub fn empty() -> Self {
        Self {
            text: ropey::Rope::new(),
            encoding: TextEncoding::Utf8,
            line_ending: LineEnding::Lf,
        }
    }

    /// `bytes` decoded, or an error naming `path` when it is a `.dat` whose bytes are binary.
    pub fn open(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let dat = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dat"));
        let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
        if dat && !utf16 && bytes.contains(&0) {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            return Err(format!(
                "{name} is a binary table, not text, and OpenRage has no reader for it"
            ));
        }
        Ok(Self::decode(bytes))
    }

    pub fn decode(bytes: &[u8]) -> Self {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            let raw = std::str::from_utf8(&bytes[3..])
                .map(|s| s.to_string())
                .unwrap_or_else(|_| decode_latin1(&bytes[3..]));
            let (text, line_ending) = build_rope(&raw);
            return Self {
                text,
                encoding: TextEncoding::Utf8Bom,
                line_ending,
            };
        }
        if bytes.starts_with(&[0xFF, 0xFE]) {
            let raw = decode_utf16_le(&bytes[2..]);
            let (text, line_ending) = build_rope(&raw);
            return Self {
                text,
                encoding: TextEncoding::Utf16Le,
                line_ending,
            };
        }
        if bytes.starts_with(&[0xFE, 0xFF]) {
            let raw = decode_utf16_be(&bytes[2..]);
            let (text, line_ending) = build_rope(&raw);
            return Self {
                text,
                encoding: TextEncoding::Utf16Be,
                line_ending,
            };
        }
        match std::str::from_utf8(bytes) {
            Ok(s) => {
                let (text, line_ending) = build_rope(s);
                Self {
                    text,
                    encoding: TextEncoding::Utf8,
                    line_ending,
                }
            }
            Err(_) => {
                let raw = decode_latin1(bytes);
                let (text, line_ending) = build_rope(&raw);
                Self {
                    text,
                    encoding: TextEncoding::Latin1,
                    line_ending,
                }
            }
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        match self.encoding {
            TextEncoding::Utf8 => encode_chunks(&self.text, self.line_ending, None),
            TextEncoding::Utf8Bom => {
                encode_chunks(&self.text, self.line_ending, Some(&[0xEF, 0xBB, 0xBF]))
            }
            TextEncoding::Utf16Le => {
                let mut out: Vec<u8> = Vec::with_capacity(2 + self.text.len_bytes() * 2);
                out.extend_from_slice(&[0xFF, 0xFE]);
                emit_with_line_endings_str(&self.text, self.line_ending, |s| {
                    for unit in s.encode_utf16() {
                        out.extend_from_slice(&unit.to_le_bytes());
                    }
                });
                out
            }
            TextEncoding::Utf16Be => {
                let mut out: Vec<u8> = Vec::with_capacity(2 + self.text.len_bytes() * 2);
                out.extend_from_slice(&[0xFE, 0xFF]);
                emit_with_line_endings_str(&self.text, self.line_ending, |s| {
                    for unit in s.encode_utf16() {
                        out.extend_from_slice(&unit.to_be_bytes());
                    }
                });
                out
            }
            TextEncoding::Latin1 => {
                let mut out: Vec<u8> = Vec::with_capacity(self.text.len_bytes());
                emit_with_line_endings_str(&self.text, self.line_ending, |s| {
                    for c in s.chars() {
                        out.push(if (c as u32) < 0x100 { c as u8 } else { b'?' });
                    }
                });
                out
            }
        }
    }
}

fn emit_with_line_endings_str(
    rope: &ropey::Rope,
    line_ending: LineEnding,
    mut sink: impl FnMut(&str),
) {
    match line_ending {
        LineEnding::Crlf => {
            for line in rope.lines() {
                let s: std::borrow::Cow<str> = line.into();
                if let Some(stripped) = s.strip_suffix('\n') {
                    sink(stripped);
                    sink("\r\n");
                } else {
                    sink(&s);
                }
            }
        }
        LineEnding::Lf | LineEnding::Mixed => {
            for chunk in rope.chunks() {
                sink(chunk);
            }
        }
    }
}

fn encode_chunks(rope: &ropey::Rope, line_ending: LineEnding, prefix: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.map_or(0, |p| p.len()) + rope.len_bytes());
    if let Some(p) = prefix {
        out.extend_from_slice(p);
    }
    emit_with_line_endings_str(rope, line_ending, |s| out.extend_from_slice(s.as_bytes()));
    out
}

fn build_rope(text: &str) -> (ropey::Rope, LineEnding) {
    let (normalized, line_ending) = normalize_line_endings(text);
    (ropey::Rope::from_str(&normalized), line_ending)
}

fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn decode_utf16_le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn decode_utf16_be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn normalize_line_endings(input: &str) -> (String, LineEnding) {
    let mut has_crlf = false;
    let mut has_lone_lf = false;
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                has_crlf = true;
                i += 2;
            }
            b'\n' => {
                has_lone_lf = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    let line_ending = match (has_crlf, has_lone_lf) {
        (true, true) => LineEnding::Mixed,
        (true, false) => LineEnding::Crlf,
        _ => LineEnding::Lf,
    };
    let normalized = if has_crlf {
        input.replace("\r\n", "\n")
    } else {
        input.to_string()
    };
    (normalized, line_ending)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope_to_string(doc: &TextDocument) -> String {
        doc.text.to_string()
    }

    #[test]
    fn text_decode_plain_utf8_lf() {
        let doc = TextDocument::decode(b"hello\nworld");
        assert_eq!(doc.encoding, TextEncoding::Utf8);
        assert_eq!(doc.line_ending, LineEnding::Lf);
        assert_eq!(rope_to_string(&doc), "hello\nworld");
    }

    #[test]
    fn text_decode_utf8_bom_crlf() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"foo\r\nbar\r\n");
        let doc = TextDocument::decode(&bytes);
        assert_eq!(doc.encoding, TextEncoding::Utf8Bom);
        assert_eq!(doc.line_ending, LineEnding::Crlf);
        assert_eq!(rope_to_string(&doc), "foo\nbar\n");
    }

    #[test]
    fn text_decode_utf16_le_bom() {
        // "ab\n" little-endian: a=0x61, b=0x62, lf=0x0A.
        let bytes = [0xFF, 0xFE, 0x61, 0x00, 0x62, 0x00, 0x0A, 0x00];
        let doc = TextDocument::decode(&bytes);
        assert_eq!(doc.encoding, TextEncoding::Utf16Le);
        assert_eq!(rope_to_string(&doc), "ab\n");
    }

    #[test]
    fn text_decode_utf16_be_bom() {
        let bytes = [0xFE, 0xFF, 0x00, 0x61, 0x00, 0x62, 0x00, 0x0A];
        let doc = TextDocument::decode(&bytes);
        assert_eq!(doc.encoding, TextEncoding::Utf16Be);
        assert_eq!(rope_to_string(&doc), "ab\n");
    }

    #[test]
    fn text_decode_latin1_fallback() {
        // 0xE9 is 'é' in Latin-1 / Windows-1252 but invalid as a
        // standalone UTF-8 byte, so the decoder falls back.
        let bytes = b"caf\xE9";
        let doc = TextDocument::decode(bytes);
        assert_eq!(doc.encoding, TextEncoding::Latin1);
        assert_eq!(rope_to_string(&doc), "café");
    }

    #[test]
    fn text_decode_mixed_line_endings() {
        let doc = TextDocument::decode(b"a\r\nb\nc\r\n");
        assert_eq!(doc.line_ending, LineEnding::Mixed);
        assert_eq!(rope_to_string(&doc), "a\nb\nc\n");
    }

    #[test]
    fn text_roundtrip_utf8_lf() {
        let bytes = b"line1\nline2\n";
        let doc = TextDocument::decode(bytes);
        assert_eq!(doc.encode(), bytes);
    }

    #[test]
    fn text_roundtrip_utf8_bom_crlf() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<root>\r\n  <child/>\r\n</root>");
        let doc = TextDocument::decode(&bytes);
        assert_eq!(doc.encode(), bytes);
    }

    #[test]
    fn text_roundtrip_utf16_le_bom() {
        let bytes = vec![0xFF, 0xFE, 0x61, 0x00, 0x62, 0x00, 0x0A, 0x00, 0x63, 0x00];
        let doc = TextDocument::decode(&bytes);
        assert_eq!(doc.encode(), bytes);
    }

    #[test]
    fn text_roundtrip_latin1() {
        let bytes = b"caf\xE9 latte".to_vec();
        let doc = TextDocument::decode(&bytes);
        assert_eq!(doc.encode(), bytes);
    }
}
