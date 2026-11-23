use std::collections::HashMap;

use super::{MaterialPaletteEntry, MATERIAL_PALETTE_DEFAULT_RGB};

pub fn parse_material_fx(text: &str) -> HashMap<String, [u8; 3]> {
    let mut out = HashMap::new();
    let mut in_table = false;
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with("MTLFX_TABLE_START") {
            in_table = true;
            continue;
        }
        if line.starts_with("MTLFX_TABLE_END") {
            break;
        }
        if !in_table {
            continue;
        }
        let mut parts = line.split(['\t', ' ']).filter(|s| !s.is_empty());
        let Some(name) = parts.next() else { continue };
        let Some(r) = parts.next().and_then(|s| s.parse::<u8>().ok()) else {
            continue;
        };
        let Some(g) = parts.next().and_then(|s| s.parse::<u8>().ok()) else {
            continue;
        };
        let Some(b) = parts.next().and_then(|s| s.parse::<u8>().ok()) else {
            continue;
        };
        out.insert(name.to_string(), [r, g, b]);
    }
    out
}

#[derive(Debug, Clone)]
pub struct ParsedMaterialRow {
    pub name: String,
    pub fx_group: String,
}

pub fn parse_materials_dat(text: &str) -> Vec<ParsedMaterialRow> {
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if line.len() < 20 {
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.split(['\t', ' ']).filter(|s| !s.is_empty());
        let Some(name) = parts.next() else { continue };
        let Some(_filter) = parts.next() else {
            continue;
        };
        let Some(fx_group) = parts.next() else {
            continue;
        };
        out.push(ParsedMaterialRow {
            name: name.to_string(),
            fx_group: fx_group.to_string(),
        });
    }
    out
}

pub fn build_palette_entries(
    materials_dat: &str,
    materialfx_dat: &str,
) -> Vec<MaterialPaletteEntry> {
    let fx = parse_material_fx(materialfx_dat);
    parse_materials_dat(materials_dat)
        .into_iter()
        .map(|row| {
            let rgb = fx
                .get(row.fx_group.as_str())
                .copied()
                .unwrap_or(MATERIAL_PALETTE_DEFAULT_RGB);
            MaterialPaletteEntry {
                name: leak_str(row.name),
                fx_group: leak_str(row.fx_group),
                rgb,
            }
        })
        .collect()
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MFX_FIXTURE: &str = "\
# header comment
MTLFX_TABLE_START
GENERIC\t\t255\t0\t255
ROCK 100 110 120
# in-table comment
WOOD\t\t10\t20\t30\t# extra columns ignored
MTLFX_TABLE_END
GARBAGE 1 2 3 4 5
";

    const MAT_FIXTURE: &str = "\
# Material        Filter     FXGroup    Disturb    Rumble     React      Friction   Elastic    Density    TyreGrip   WetGrip    TyreDrag   TopSpeedMult Soft       Noise      Penetrate  SeeThru    ShootThru  ShootFX    NoDecal    Porous     HeatsTyre  Material
DEFAULT           DEFAULT    GENERIC    NONE       0          NONE       1.0        0.5        1000.0     1.0        1.0        1.0        1.0          0.5        0.5        1.0        1          1          NONE       0          0          0          DEFAULT
ROCK              ROCK       ROCK       NONE       0          NONE       1.0        0.5        1000.0     1.0        1.0        1.0        1.0          0.5        0.5        1.0        1          1          NONE       0          0          0          ROCK
WOOD              WOOD       WOOD       NONE       0          NONE       1.0        0.5        1000.0     1.0        1.0        1.0        1.0          0.5        0.5        1.0        1          1          NONE       0          0          0          WOOD
TOO SHORT
# comment
";

    #[test]
    fn material_fx_parses_table_block_only() {
        let fx = parse_material_fx(MFX_FIXTURE);
        assert_eq!(fx.get("GENERIC"), Some(&[255, 0, 255]));
        assert_eq!(fx.get("ROCK"), Some(&[100, 110, 120]));
        assert_eq!(fx.get("WOOD"), Some(&[10, 20, 30]));
        // GARBAGE is past MTLFX_TABLE_END so it must not show up.
        assert!(!fx.contains_key("GARBAGE"));
    }

    #[test]
    fn materials_dat_parses_name_and_fx_group() {
        let mats = parse_materials_dat(MAT_FIXTURE);
        assert_eq!(mats.len(), 3);
        assert_eq!(mats[0].name, "DEFAULT");
        assert_eq!(mats[0].fx_group, "GENERIC");
        assert_eq!(mats[1].name, "ROCK");
        assert_eq!(mats[1].fx_group, "ROCK");
        assert_eq!(mats[2].name, "WOOD");
        assert_eq!(mats[2].fx_group, "WOOD");
    }

    #[test]
    fn build_palette_substitutes_default_rgb_for_missing_fx_group() {
        let mat_with_orphan = format!(
            "{MAT_FIXTURE}\
ORPHAN            ORPHAN     NEVER_HEARD_OF NONE 0 NONE 1.0 0.5 1000.0 1.0 1.0 1.0 1.0 0.5 0.5 1.0 1 1 NONE 0 0 0 ORPHAN
"
        );
        let entries = build_palette_entries(&mat_with_orphan, MFX_FIXTURE);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].rgb, [255, 0, 255]);
        assert_eq!(entries[1].rgb, [100, 110, 120]);
        assert_eq!(entries[2].rgb, [10, 20, 30]);
        assert_eq!(entries[3].name, "ORPHAN");
        assert_eq!(entries[3].rgb, MATERIAL_PALETTE_DEFAULT_RGB);
    }
}
