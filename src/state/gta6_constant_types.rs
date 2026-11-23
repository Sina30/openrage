//! How a GTA VI drawable's constant reads where its effect declares type 0x03, a table of 16
//! entries, or type 0x00 or 0x0F, which no drawable holds.

use or_io::gta5::shaders::sgd5::remap_table;
use or_io::gta5::shaders::shader_defs::{ShaderParamDef, ShaderParamKind};

/// Whether `def` declares type 0x03.
pub(super) fn is_remap(def: &ShaderParamDef) -> bool {
    def.kind == ShaderParamKind::Unknown(3)
}

/// Three words of type 0x03 as their 16 entries: an entry that maps to itself by its index, a
/// remapped one as `index→value`, and either followed by `:flag` where its flag is set.
pub(super) fn remap_shown(words: &[u32]) -> Option<String> {
    let table = remap_table(<[u32; 3]>::try_from(words).ok()?);
    let entries: Vec<String> = (table.iter().enumerate())
        .map(|(i, &(value, flag))| {
            let entry = match usize::from(value) == i {
                true => i.to_string(),
                false => format!("{i}→{value}"),
            };
            match flag {
                0 => entry,
                _ => format!("{entry}:{flag}"),
            }
        })
        .collect();
    Some(tr!("16 entries {entries}", entries = entries.join(" ")))
}

/// The words of a constant whose effect declares type 0x00 or 0x0F, in hex, with the declaration
/// refused as stale because no drawable holds either type; `None` for any other declaration.
pub(super) fn stale(words: &[u32], def: &ShaderParamDef) -> Option<String> {
    let code = match def.kind {
        ShaderParamKind::Bool => 0x00,
        ShaderParamKind::Unknown(0x0F) => 0x0F,
        _ => return None,
    };
    let values: Vec<String> = words.iter().map(|w| format!("{w:#010X}")).collect();
    Some(tr!(
        "{values}; its effect declares type {code}, which no drawable holds, so that declaration is refused as stale",
        values = values.join(", "),
        code = format!("{code:#04X}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta5::shaders::sgd5::{is_identity_remap, remap_table};

    fn def(kind: ShaderParamKind) -> ShaderParamDef {
        ShaderParamDef {
            name: "hash_01234567".to_string(),
            name_hash: 0x0123_4567,
            kind,
            array_len: 1,
            register: 0,
            cbuffer: None,
            drawable_place: None,
            semantic: None,
            semantic_hash: None,
            ui_name: None,
            default: None,
            preset_default: None,
        }
    }

    #[test]
    fn a_remap_shows_each_entry_by_its_index_and_a_moved_one_with_its_value_and_flag() {
        // Entry 0 maps to 3 with flag 1, and entry 15 keeps its value with flag 2.
        let mut words = [0u32; 3];
        for i in 0..16u32 {
            let (value, flag) = match i {
                0 => (3, 1),
                15 => (15, 2),
                i => (i, 0),
            };
            words[0] |= (value & 3) << (2 * i);
            words[1] |= (value >> 2) << (2 * i);
            words[2] |= flag << (2 * i);
        }
        assert!(!is_identity_remap(&words));
        assert_eq!(remap_table(words)[0], (3, 1));
        let shown = remap_shown(&words).expect("three words");
        assert!(shown.contains("0→3:1 1 2 3 "), "{shown}");
        assert!(shown.ends_with(" 14 15:2"), "{shown}");
        assert_eq!(remap_shown(&words[..2]), None);
        assert!(is_remap(&def(ShaderParamKind::Unknown(3))));
        assert!(!is_remap(&def(ShaderParamKind::Unknown(5))));
    }

    #[test]
    fn a_boolean_or_structure_declaration_is_refused_as_stale_and_any_other_is_not() {
        let line = stale(&[1, 0xAB], &def(ShaderParamKind::Unknown(0x0F))).expect("stale");
        assert!(line.starts_with("0x00000001, 0x000000AB;"), "{line}");
        assert!(line.contains("type 0x0F"), "{line}");
        assert!(stale(&[1], &def(ShaderParamKind::Bool)).is_some());
        assert_eq!(stale(&[1], &def(ShaderParamKind::Float)), None);
    }
}
