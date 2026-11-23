//! A file no reader opens, kept as its bytes for a read-only hex view.

/// Leading bytes OpenRage recognises, and what each names.
const MAGICS: &[(&[u8], &str)] = &[
    (b"RSC7", "RAGE resource"),
    (b"7FPR", "RPF7 archive"),
    (b"PSIN", "PSO metadata"),
    (b"RBF0", "RBF0 metadata"),
    (b"ADAT", "AWC audio container"),
    (b"TADA", "AWC audio container (big-endian)"),
    (b"KB2", "Bink 2 video"),
    (b"BIK", "Bink video"),
    (b"DDS ", "DDS texture"),
    (b"GFX", "Scaleform GFX movie"),
    (b"CFX", "Scaleform GFX movie (compressed)"),
    (b"2TXG", "GXT2 text table"),
    (b"rgxe", "FXC shader effect"),
    (b"FWS", "SWF movie"),
    (b"CWS", "SWF movie (compressed)"),
    (b"\x89PNG", "PNG image"),
    (b"OggS", "Ogg stream"),
    (b"RIFF", "RIFF container"),
    (b"PK\x03\x04", "ZIP archive"),
    (b"MZ", "Windows executable"),
];

/// Bytes per row of the view.
const HEX_ROW: usize = 16;

#[derive(Debug, Clone)]
pub struct HexDocument {
    pub bytes: Vec<u8>,
    /// Why no richer view opened this file.
    pub banner: String,
}

impl HexDocument {
    /// What the file's leading bytes name, when OpenRage knows them.
    pub fn magic(&self) -> Option<&'static str> {
        MAGICS
            .iter()
            .find(|(magic, _)| self.bytes.starts_with(magic))
            .map(|(_, name)| *name)
    }

    pub fn rows(&self) -> usize {
        self.bytes.len().div_ceil(HEX_ROW)
    }

    /// Row `i` as offset, hex and printable columns.
    pub fn row(&self, i: usize) -> String {
        let start = (i * HEX_ROW).min(self.bytes.len());
        let end = (start + HEX_ROW).min(self.bytes.len());
        crate::state::entry_preview::hex_line(start, &self.bytes[start..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(bytes: &[u8]) -> HexDocument {
        HexDocument {
            bytes: bytes.to_vec(),
            banner: String::new(),
        }
    }

    #[test]
    fn the_magics_or_io_declares_are_named() {
        use or_io::container::rpf::RPF7_MAGIC;
        use or_io::gta5::GXT2_MAGIC;
        assert_eq!(of(&RPF7_MAGIC.to_le_bytes()).magic(), Some("RPF7 archive"));
        assert_eq!(
            of(&GXT2_MAGIC.to_le_bytes()).magic(),
            Some("GXT2 text table")
        );
        assert_eq!(of(b"KB2j\0\0").magic(), Some("Bink 2 video"));
        assert_eq!(of(b"\x01\x02\x03\x04").magic(), None);
        assert_eq!(of(b"").magic(), None);
    }

    #[test]
    fn a_row_is_its_offset_and_the_last_row_is_short() {
        let doc = of(&[0xAB; 20]);
        assert_eq!(doc.rows(), 2);
        assert!(
            doc.row(1).starts_with("00000010  AB AB AB AB  "),
            "{}",
            doc.row(1)
        );
        assert_eq!(of(b"").rows(), 0);
    }
}
