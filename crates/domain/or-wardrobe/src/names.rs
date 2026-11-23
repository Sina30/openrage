//! The peds, slots and file names of a freemode clothing pack: what a name is made of and what it is called.
//!
//! The names are the engine's through the corpus. A component slot is the word the game's own file
//! names carry (`jbib` in `mp_m_freemode_01_mp_m_unity_clothes^jbib_012_u.ydd`), numbered as
//! `ePedVarComp` numbers it; a prop slot is the anchor `eAnchorPoints` numbers, in the file name's
//! `p_` form. `every_slot_is_the_number_the_engine_states` holds both to the enums the staged
//! documents state.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A freemode ped, the only kind of ped a Wardrobe dresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ped {
    Male,
    Female,
}

impl Ped {
    pub const ALL: [Ped; 2] = [Ped::Male, Ped::Female];

    /// The other freemode ped, which a garment is copied to.
    pub fn other(self) -> Self {
        match self {
            Ped::Male => Ped::Female,
            Ped::Female => Ped::Male,
        }
    }

    /// The model name the game knows the ped by.
    pub fn model(self) -> &'static str {
        match self {
            Ped::Male => "mp_m_freemode_01",
            Ped::Female => "mp_f_freemode_01",
        }
    }

    /// The letter the collection names of a pack carry for the ped.
    pub fn letter(self) -> char {
        match self {
            Ped::Male => 'm',
            Ped::Female => 'f',
        }
    }

    pub fn from_model(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|ped| ped.model().eq_ignore_ascii_case(text))
    }
}

/// One of the twelve component slots of a ped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Component {
    Head,
    Berd,
    Hair,
    Uppr,
    Lowr,
    Hand,
    Feet,
    Teef,
    Accs,
    Task,
    Decl,
    Jbib,
}

impl Component {
    pub const ALL: [Component; 12] = [
        Component::Head,
        Component::Berd,
        Component::Hair,
        Component::Uppr,
        Component::Lowr,
        Component::Hand,
        Component::Feet,
        Component::Teef,
        Component::Accs,
        Component::Task,
        Component::Decl,
        Component::Jbib,
    ];

    /// The number `ePedVarComp` gives the slot, which is its index in `availComp`.
    pub fn id(self) -> u8 {
        self as u8
    }

    /// The word the game's file names carry.
    pub fn name(self) -> &'static str {
        match self {
            Component::Head => "head",
            Component::Berd => "berd",
            Component::Hair => "hair",
            Component::Uppr => "uppr",
            Component::Lowr => "lowr",
            Component::Hand => "hand",
            Component::Feet => "feet",
            Component::Teef => "teef",
            Component::Accs => "accs",
            Component::Task => "task",
            Component::Decl => "decl",
            Component::Jbib => "jbib",
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        Self::ALL.get(usize::from(id)).copied()
    }
}

/// One of the prop slots of a ped, named by the anchor a prop attaches at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Prop {
    Head,
    Eyes,
    Ears,
    Mouth,
    LeftHand,
    RightHand,
    LeftWrist,
    RightWrist,
    Hip,
}

impl Prop {
    pub const ALL: [Prop; 9] = [
        Prop::Head,
        Prop::Eyes,
        Prop::Ears,
        Prop::Mouth,
        Prop::LeftHand,
        Prop::RightHand,
        Prop::LeftWrist,
        Prop::RightWrist,
        Prop::Hip,
    ];

    /// The number `eAnchorPoints` gives the anchor.
    pub fn anchor(self) -> u8 {
        self as u8
    }

    /// The word the game's file names carry, `p_` first.
    pub fn name(self) -> &'static str {
        match self {
            Prop::Head => "p_head",
            Prop::Eyes => "p_eyes",
            Prop::Ears => "p_ears",
            Prop::Mouth => "p_mouth",
            Prop::LeftHand => "p_lhand",
            Prop::RightHand => "p_rhand",
            Prop::LeftWrist => "p_lwrist",
            Prop::RightWrist => "p_rwrist",
            Prop::Hip => "p_lhip",
        }
    }

    /// The prop a `p_` name says: the engine's word, or `p_hip`, which OpenRage spelled the hip with before and still reads.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = if name == "p_hip" { "p_lhip" } else { name };
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The bone of the ped's skeleton a prop of the anchor is stored at, by its name.
    ///
    /// ⚠ A wrist's is the forearm's roll bone and not the hand's: a watch drawn at the hand's sat across the knuckles.
    pub fn bone(self) -> &'static str {
        match self {
            Prop::Head | Prop::Eyes | Prop::Ears | Prop::Mouth => "SKEL_Head",
            Prop::LeftHand => "PH_L_Hand",
            Prop::RightHand => "PH_R_Hand",
            Prop::LeftWrist => "RB_L_ForeArmRoll",
            Prop::RightWrist => "RB_R_ForeArmRoll",
            Prop::Hip => "SKEL_Pelvis",
        }
    }

    pub fn from_anchor(anchor: u8) -> Option<Self> {
        Self::ALL.get(usize::from(anchor)).copied()
    }
}

/// A slot a garment fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Component(Component),
    Prop(Prop),
}

impl Slot {
    /// Every slot, components first, in the order a plan lists them.
    pub fn all() -> impl Iterator<Item = Slot> {
        Component::ALL
            .into_iter()
            .map(Slot::Component)
            .chain(Prop::ALL.into_iter().map(Slot::Prop))
    }

    pub fn name(self) -> &'static str {
        match self {
            Slot::Component(c) => c.name(),
            Slot::Prop(p) => p.name(),
        }
    }

    pub fn is_prop(self) -> bool {
        matches!(self, Slot::Prop(_))
    }
}

impl FromStr for Slot {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let lower = text.to_ascii_lowercase();
        Slot::all()
            .find(|slot| slot.name() == lower)
            .or_else(|| Prop::from_name(&lower).map(Slot::Prop))
            .ok_or_else(|| format!("`{text}` is not a slot of a freemode ped"))
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether a component drawable fits every skin tone (`u`) or has a texture for each (`r`).
///
/// ⚠ Every staged drawable states the texture id 0 for `u` and 1 for `r`, in each of its textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Skin {
    #[default]
    Universal,
    Race,
}

impl Skin {
    pub fn letter(self) -> char {
        match self {
            Skin::Universal => 'u',
            Skin::Race => 'r',
        }
    }

    /// The `texId` a texture of such a drawable states.
    pub fn tex_id(self) -> u8 {
        match self {
            Skin::Universal => 0,
            Skin::Race => 1,
        }
    }

    /// The word a texture's name ends with when nothing else says.
    pub fn tint(self) -> &'static str {
        match self {
            Skin::Universal => "uni",
            Skin::Race => "whi",
        }
    }
}

macro_rules! text_serde {
    ($($t:ty),*) => {$(
        impl Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    )*};
}

impl fmt::Display for Ped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.model())
    }
}

impl FromStr for Ped {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        Ped::from_model(text).ok_or_else(|| format!("`{text}` is not a freemode ped"))
    }
}

impl fmt::Display for Skin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.letter())
    }
}

impl FromStr for Skin {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "u" => Ok(Skin::Universal),
            "r" => Ok(Skin::Race),
            other => Err(format!("`{other}` is not a skin, which is `u` or `r`")),
        }
    }
}

text_serde!(Ped, Slot, Skin);

/// A name the file states, from the values the type has.
macro_rules! text_schema {
    ($t:ident, $what:literal, $values:expr) => {
        impl schemars::JsonSchema for $t {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($t).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                let values: Vec<&str> = $values;
                schemars::json_schema!({ "type": "string", "enum": values, "description": $what })
            }
        }
    };
}

text_schema!(
    Ped,
    "A freemode ped, by the model name the game knows it by",
    Ped::ALL.map(Ped::model).to_vec()
);
text_schema!(
    Slot,
    "A slot a garment fills: a component by its file-name word, or a prop by its anchor",
    Slot::all().map(Slot::name).collect()
);
text_schema!(
    Skin,
    "u for a drawable that fits every skin tone, r for one with a texture for each",
    vec!["u", "r"]
);

pub use or_project::content::folder::is_identifier;

/// The name of a pack made of `text`: lower case letters and digits with `_` between words, and `pack_` before a name that starts with a digit.
pub fn identifier_for(text: &str) -> String {
    let mut name = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('_') {
            name.push('_');
        }
    }
    let name = name.trim_end_matches('_');
    match name.chars().next() {
        None => "pack".to_string(),
        Some(c) if c.is_ascii_digit() => format!("pack_{name}"),
        Some(_) => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_parse_from_the_words_of_the_file_names_and_print_them_back() {
        for slot in Slot::all() {
            assert_eq!(slot.name().parse::<Slot>(), Ok(slot));
            assert_eq!(slot.to_string(), slot.name());
        }
        assert_eq!("JBIB".parse::<Slot>(), Ok(Slot::Component(Component::Jbib)));
        assert!("p_foot".parse::<Slot>().is_err());
    }

    #[test]
    fn a_name_people_type_becomes_a_pack_name_that_is_one() {
        for (typed, made) in [
            ("Unity Clothes", "unity_clothes"),
            ("  My-Pack!! ", "my_pack"),
            ("2024 drop", "pack_2024_drop"),
            ("ünï", "n"),
            ("", "pack"),
            ("---", "pack"),
        ] {
            assert_eq!(identifier_for(typed), made, "{typed:?}");
            assert!(is_identifier(&identifier_for(typed)), "{typed:?}");
        }
    }

    #[test]
    fn a_component_is_found_again_by_its_number() {
        for c in Component::ALL {
            assert_eq!(Component::from_id(c.id()), Some(c));
        }
        assert_eq!(Component::from_id(12), None);
        for p in Prop::ALL {
            assert_eq!(Prop::from_anchor(p.anchor()), Some(p));
        }
    }

    #[test]
    fn a_pack_name_is_lower_case_words() {
        for good in ["unity_clothes", "a1", "pack"] {
            assert!(is_identifier(good), "{good}");
        }
        for bad in ["", "1pack", "Pack", "my-pack", "a b", "_a", "ä"] {
            assert!(!is_identifier(bad), "{bad:?}");
        }
    }
}
