pub mod parser;

use std::path::PathBuf;
use std::sync::Arc;

pub use parser::{
    build_palette_entries, parse_material_fx, parse_materials_dat, ParsedMaterialRow,
};

#[derive(Debug, Clone, Copy)]
pub struct MaterialPaletteEntry {
    pub name: &'static str,
    pub fx_group: &'static str,
    pub rgb: [u8; 3],
}

/// The tint for a material the palette does not name, including every
/// material before an install has been read.
///
/// ⚠ OpenRage's own neutral grey, chosen so an unloaded palette reads as
/// "no data" rather than as a colour some table assigned. Nothing ships a
/// palette: the entries come only from the user's install.
pub const MATERIAL_PALETTE_DEFAULT_RGB: [u8; 3] = [128, 128, 128];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialPaletteSource {
    NotLoaded,
    GtaInstall { install_root: PathBuf },
}

#[derive(Debug, Clone)]
pub struct MaterialPalette {
    entries: PaletteEntries,
    source: MaterialPaletteSource,
}

#[derive(Debug, Clone)]
enum PaletteEntries {
    Static(&'static [MaterialPaletteEntry]),
    Owned(Arc<[MaterialPaletteEntry]>),
}

impl PaletteEntries {
    fn as_slice(&self) -> &[MaterialPaletteEntry] {
        match self {
            PaletteEntries::Static(s) => s,
            PaletteEntries::Owned(o) => o,
        }
    }
}

impl MaterialPalette {
    /// The palette before an install has been read: no entries, so every
    /// material takes [`MATERIAL_PALETTE_DEFAULT_RGB`].
    pub const fn empty() -> &'static MaterialPalette {
        &EMPTY_PALETTE
    }

    pub fn from_runtime(
        entries: Vec<MaterialPaletteEntry>,
        source: MaterialPaletteSource,
    ) -> MaterialPalette {
        MaterialPalette {
            entries: PaletteEntries::Owned(Arc::from(entries)),
            source,
        }
    }

    pub fn source(&self) -> &MaterialPaletteSource {
        &self.source
    }

    pub fn is_loaded(&self) -> bool {
        !matches!(self.source, MaterialPaletteSource::NotLoaded)
    }

    pub fn entries(&self) -> &[MaterialPaletteEntry] {
        self.entries.as_slice()
    }

    pub fn len(&self) -> usize {
        self.entries.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.as_slice().is_empty()
    }

    #[inline]
    pub fn rgb_for(&self, material_type: u8) -> [u8; 3] {
        let idx = material_type as usize;
        let entries = self.entries.as_slice();
        if idx < entries.len() {
            entries[idx].rgb
        } else {
            MATERIAL_PALETTE_DEFAULT_RGB
        }
    }

    #[inline]
    pub fn name_for(&self, material_type: u8) -> Option<&str> {
        let idx = material_type as usize;
        let entries = self.entries.as_slice();
        entries.get(idx).map(|e| e.name)
    }
}

static EMPTY_PALETTE: MaterialPalette = MaterialPalette {
    entries: PaletteEntries::Static(&[]),
    source: MaterialPaletteSource::NotLoaded,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unloaded_palette_names_nothing_and_tints_everything_neutral() {
        let palette = MaterialPalette::empty();
        assert!(!palette.is_loaded());
        assert!(palette.is_empty());
        for material in [0u8, 1, 7, 255] {
            assert_eq!(palette.rgb_for(material), MATERIAL_PALETTE_DEFAULT_RGB);
            assert_eq!(palette.name_for(material), None);
        }
    }

    #[test]
    fn from_runtime_preserves_source_and_entries() {
        let entries = vec![
            MaterialPaletteEntry {
                name: "TEST_A",
                fx_group: "FX_A",
                rgb: [10, 20, 30],
            },
            MaterialPaletteEntry {
                name: "TEST_B",
                fx_group: "FX_B",
                rgb: [40, 50, 60],
            },
        ];
        let palette = MaterialPalette::from_runtime(
            entries,
            MaterialPaletteSource::GtaInstall {
                install_root: PathBuf::from("/tmp/gta"),
            },
        );
        assert!(palette.is_loaded());
        assert_eq!(palette.len(), 2);
        assert_eq!(palette.rgb_for(0), [10, 20, 30]);
        assert_eq!(palette.rgb_for(1), [40, 50, 60]);
        assert_eq!(palette.rgb_for(2), MATERIAL_PALETTE_DEFAULT_RGB);
        assert_eq!(palette.name_for(0), Some("TEST_A"));
        assert_eq!(palette.name_for(99), None);
        match palette.source() {
            MaterialPaletteSource::GtaInstall { install_root } => {
                assert_eq!(install_root, &PathBuf::from("/tmp/gta"));
            }
            _ => panic!("expected GtaInstall source"),
        }
    }
}
