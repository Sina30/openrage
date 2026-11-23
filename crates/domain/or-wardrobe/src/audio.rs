//! The audio a drawable states, and the ids the game's own clothing states for each kind of drawable.
//!
//! A drawable states an audio id (`pedXml_audioID`) and a component a second (`pedXml_audioID2`), and a prop
//! one (`audioId`). They are hashes of names this tree does not hold, so a preset is a hash and the number of
//! drawables of its slot that state it, and a name or a `hash_XXXXXXXX` is what a garment states.
//!
//! ⚠ The table is the staged documents' and nobody's choice: for each slot, the ids that at least a hundredth
//! of its drawables state (and twenty), most used first, the ones that state nothing (`NONE`) left out.
//! `the_fields_a_garment_names_are_the_ones_the_staged_documents_state` recomputes it from the corpus.

use crate::edit::Error;
use crate::model::Garment;
use crate::names::{Component, Slot};

/// Which of a component's two audio ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    First,
    Second,
}

/// What the staged documents state for one slot: how many drawables, and the ids with their uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presets {
    pub slot: Component,
    pub drawables: u32,
    pub first: &'static [(u32, u32)],
    pub second: &'static [(u32, u32)],
}

/// Every slot whose staged drawables state an id other than `NONE`.
pub const PRESETS: &[Presets] = &[
    Presets {
        slot: Component::Uppr,
        drawables: 6883,
        first: &[
            (0x49BC7916, 675),
            (0x0A859662, 301),
            (0xEEE67B89, 268),
            (0xBE1C20F3, 230),
            (0x3A674E94, 214),
            (0xEA7D5CB0, 180),
            (0x6F81BF4C, 131),
            (0x41F3FBCF, 106),
        ],
        second: &[],
    },
    Presets {
        slot: Component::Lowr,
        drawables: 5126,
        first: &[
            (0x4404BB2A, 1328),
            (0xF8F42403, 688),
            (0xC4C7D0C4, 566),
            (0xDBB1AAB9, 472),
            (0x34684E19, 193),
            (0x2A51D428, 176),
            (0xC650AE7F, 126),
            (0x51B1AE8B, 108),
        ],
        second: &[
            (0xC6396A50, 407),
            (0x3D857DBD, 358),
            (0xBEBB4F46, 228),
            (0x7735E17F, 163),
            (0x7ACB75CC, 86),
            (0x571BA0CC, 76),
            (0xEA2B54C0, 55),
        ],
    },
    Presets {
        slot: Component::Hand,
        drawables: 2331,
        first: &[(0xD1675D16, 215), (0xF78B58FB, 96)],
        second: &[],
    },
    Presets {
        slot: Component::Feet,
        drawables: 2949,
        first: &[
            (0xC6396A50, 741),
            (0x3D857DBD, 593),
            (0xBEBB4F46, 223),
            (0xBE6F6E26, 196),
            (0xDEC15E40, 124),
            (0x7ACB75CC, 95),
            (0x7735E17F, 91),
            (0x571BA0CC, 82),
        ],
        second: &[],
    },
    Presets {
        slot: Component::Accs,
        drawables: 4636,
        first: &[
            (0x5A54FD27, 121),
            (0x49BC7916, 120),
            (0x318603AE, 73),
            (0xF65A3B99, 70),
            (0x335ED08A, 61),
        ],
        second: &[],
    },
    Presets {
        slot: Component::Task,
        drawables: 1283,
        first: &[(0xF78B58FB, 38), (0xF65A3B99, 30), (0xF31E6566, 26)],
        second: &[],
    },
    Presets {
        slot: Component::Jbib,
        drawables: 7114,
        first: &[
            (0xEEE67B89, 1029),
            (0x49BC7916, 984),
            (0xBE1C20F3, 865),
            (0x41F3FBCF, 644),
            (0x0A859662, 444),
            (0x3A674E94, 377),
            (0xABECA02C, 312),
            (0x318603AE, 274),
        ],
        second: &[],
    },
];

/// The presets of a slot, none where the staged documents state only `NONE` for it.
pub fn presets(slot: Slot) -> Option<&'static Presets> {
    match slot {
        Slot::Component(c) => PRESETS.iter().find(|p| p.slot == c),
        Slot::Prop(_) => None,
    }
}

/// The text a garment states a preset by.
pub fn text_of(hash: u32) -> String {
    format!("hash_{hash:08X}")
}

/// The audio id the garment states, `NONE` where it states none.
pub fn get(garment: &Garment, which: Which) -> String {
    let meta = &garment.meta;
    let stated = match (garment.slot, which) {
        (Some(Slot::Prop(_)), Which::First) => &meta.audio_id,
        (Some(Slot::Prop(_)), Which::Second) => &None,
        (_, Which::First) => &meta.ped_xml_audio_id,
        (_, Which::Second) => &meta.ped_xml_audio_id2,
    };
    stated.clone().unwrap_or_else(|| "NONE".to_string())
}

/// State `id` as the audio of the garment, or `NONE` with none.
///
/// An id is a name the game hashes (letters, digits and underscores) or `hash_` and eight hexadecimal digits; anything else is refused.
pub fn set(garment: &mut Garment, which: Which, id: Option<&str>) -> Result<(), Error> {
    let id = id
        .map(str::trim)
        .filter(|i| !i.is_empty() && !i.eq_ignore_ascii_case("none"));
    if let Some(i) = id {
        let name = i.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !name {
            return Err(Error::Refused(format!(
                "an audio id is a name of letters, digits and underscores, or `hash_` and eight hexadecimal digits, and {i:?} is neither"
            )));
        }
    }
    let id = id.map(str::to_string);
    let meta = &mut garment.meta;
    match (garment.slot, which) {
        (Some(Slot::Prop(_)), Which::First) => meta.audio_id = id,
        (Some(Slot::Prop(_)), Which::Second) => {
            return Err(Error::Refused("a prop states one audio id".into()))
        }
        (_, Which::First) => meta.ped_xml_audio_id = id,
        (_, Which::Second) => meta.ped_xml_audio_id2 = id,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::Prop;

    fn feet() -> Garment {
        let mut g = Garment::new();
        g.slot = Some(Slot::Component(Component::Feet));
        g
    }

    #[test]
    fn a_preset_is_stated_as_the_hash_it_is_and_none_states_nothing() {
        let mut g = feet();
        assert_eq!(get(&g, Which::First), "NONE");
        let preset = presets(Slot::Component(Component::Feet)).expect("feet have presets");
        let hash = preset.first[0].0;
        set(&mut g, Which::First, Some(&text_of(hash))).unwrap();
        assert_eq!(g.meta.ped_xml_audio_id, Some(text_of(hash)));
        assert_eq!(crate::metadata::hashed(&get(&g, Which::First)).hash, hash);
        set(&mut g, Which::First, Some("none")).unwrap();
        assert_eq!(g.meta.ped_xml_audio_id, None);
        set(&mut g, Which::Second, Some("SHOE_SOFT")).unwrap();
        assert_eq!(g.meta.ped_xml_audio_id2.as_deref(), Some("SHOE_SOFT"));
    }

    #[test]
    fn an_id_that_is_no_name_is_refused_and_a_prop_has_one() {
        let mut g = feet();
        assert!(set(&mut g, Which::First, Some("two words")).is_err());
        assert!(set(&mut g, Which::First, Some("a/b")).is_err());
        assert_eq!(g.meta.ped_xml_audio_id, None);
        let mut p = Garment::new();
        p.slot = Some(Slot::Prop(Prop::Eyes));
        set(&mut p, Which::First, Some("GLASSES")).unwrap();
        assert_eq!(p.meta.audio_id.as_deref(), Some("GLASSES"));
        assert!(set(&mut p, Which::Second, Some("X")).is_err());
        assert_eq!(presets(Slot::Prop(Prop::Eyes)), None);
    }

    #[test]
    fn the_table_is_ordered_by_use_and_counts_no_more_than_the_slot_has() {
        for preset in PRESETS {
            for list in [preset.first, preset.second] {
                assert!(list.windows(2).all(|w| w[0].1 >= w[1].1));
                assert!(list.iter().all(|(_, uses)| *uses <= preset.drawables));
            }
        }
    }
}
