//! The flags a drawable or a prop states, named by the engine's `ePedCompFlags` and kept as the number they are.
//!
//! A component states them in `flags` (32 bits). A prop states them twice: `flags` (16 bits) and
//! `propFlags` (32 bits), and the game reads `propFlags` when it is not zero and `flags` otherwise,
//! so [`word`] is that one and [`set_word`] writes the field the game reads.
//!
//! ⚠ A name is offered only for a bit some staged document states ([`MEASURED`], held to the corpus
//! by `the_fields_a_garment_names_are_the_ones_the_staged_documents_state`): the other bits keep the
//! name the engine gives them in [`Flag::name`] and are shown as a number, and an unknown bit
//! survives every edit.

use crate::edit::Error;
use crate::model::Garment;
use crate::names::Slot;

macro_rules! flags {
    ($($variant:ident = $bit:literal, $name:literal, $label:literal, $hint:literal;)*) => {
        /// One of the engine's `ePedCompFlags`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Flag {
            $($variant,)*
        }

        impl Flag {
            pub const ALL: [Flag; [$(Flag::$variant),*].len()] = [$(Flag::$variant),*];

            /// The bit it is, counted from 0.
            pub fn bit(self) -> u32 {
                match self {
                    $(Flag::$variant => $bit,)*
                }
            }

            /// The number with only its bit set.
            pub fn mask(self) -> u32 {
                1 << self.bit()
            }

            /// The name the engine gives the flag.
            pub fn name(self) -> &'static str {
                match self {
                    $(Flag::$variant => $name,)*
                }
            }

            /// The words a screen shows for it.
            pub fn label(self) -> &'static str {
                match self {
                    $(Flag::$variant => $label,)*
                }
            }

            /// What the game does with it, in a sentence.
            pub fn hint(self) -> &'static str {
                match self {
                    $(Flag::$variant => $hint,)*
                }
            }
        }
    };
}

flags! {
    Bulky = 0, "PV_FLAG_BULKY", "Bulky", "Bulky clothes are kept off the peds the game puts in cars";
    Job = 1, "PV_FLAG_JOB", "Job", "The game picks it for peds that have a job";
    Sunny = 2, "PV_FLAG_SUNNY", "Sunny weather", "The game picks it for peds in sunny weather";
    Wet = 3, "PV_FLAG_WET", "Wet weather", "The game picks it for peds in the rain";
    Cold = 4, "PV_FLAG_COLD", "Cold", "The game picks it for peds in cold weather";
    NotInCar = 5, "PV_FLAG_NOT_IN_CAR", "Remove in vehicle", "The game takes it off when the ped gets into a vehicle";
    BikeOnly = 6, "PV_FLAG_BIKE_ONLY", "Bike only", "The game picks it only for peds on a bike";
    NotIndoors = 7, "PV_FLAG_NOT_INDOORS", "Not indoors", "Kept off peds indoors";
    FireRetardent = 8, "PV_FLAG_FIRE_RETARDENT", "Fire retardant", "Protects against fire";
    Armoured = 9, "PV_FLAG_ARMOURED", "Armoured", "Counts as body armour";
    LightlyArmoured = 10, "PV_FLAG_LIGHTLY_ARMOURED", "Lightly armoured", "Counts as light body armour";
    HighDetail = 11, "PV_FLAG_HIGH_DETAIL", "High detail", "The engine's high detail flag";
    DefaultHelmet = 12, "PV_FLAG_DEFAULT_HELMET", "Helmet", "The helmet a ped puts on for a bike";
    RandomHelmet = 13, "PV_FLAG_RANDOM_HELMET", "Random helmet", "A helmet the game picks at random";
    ScriptHelmet = 14, "PV_FLAG_SCRIPT_HELMET", "Script helmet", "A helmet a script gives";
    FlightHelmet = 15, "PV_FLAG_FLIGHT_HELMET", "Flight helmet", "The helmet for a helicopter or a plane";
    HideInFirstPerson = 16, "PV_FLAG_HIDE_IN_FIRST_PERSON", "Hide in first person", "Not drawn in the first person view";
    UsePhysicsHat2 = 17, "PV_FLAG_USE_PHYSICS_HAT_2", "Second hat physics", "A hat that uses the second set of hat physics";
    PilotHelmet = 18, "PV_FLAG_PILOT_HELMET", "Pilot helmet", "The helmet for a pilot";
    WetMoreWet = 19, "PV_FLAG_WET_MORE_WET", "Looks wetter when wet", "Shows more wet when the ped is wet";
    WetLessWet = 20, "PV_FLAG_WET_LESS_WET", "Looks drier when wet", "Shows less wet when the ped is wet";
}

/// The bits some staged document states, which are the ones a screen offers by name.
pub const MEASURED: u32 = 0x000F_F777;

impl Flag {
    /// Whether a staged document states the bit.
    pub fn measured(self) -> bool {
        MEASURED & self.mask() != 0
    }

    pub fn from_bit(bit: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.bit() == bit)
    }
}

/// The flags the game reads for the garment: a component's `flags`, and a prop's `propFlags` where it states any, else its `flags`.
pub fn word(garment: &Garment) -> u32 {
    let meta = &garment.meta;
    match garment.slot {
        Some(Slot::Prop(_)) => match meta.prop_flags.unwrap_or(0) {
            0 => meta.flags.map_or(0, |f| f & 0xFFFF),
            wide => wide,
        },
        _ => meta.flags.unwrap_or(0),
    }
}

/// Whether the garment states `flag`.
pub fn has(garment: &Garment, flag: Flag) -> bool {
    word(garment) & flag.mask() != 0
}

/// State exactly `word` for the garment, in the field the game reads: a prop that states `propFlags` keeps them there, and one that fits the 16 bits of `flags` and states no `propFlags` stays in `flags`.
pub fn set_word(garment: &mut Garment, word: u32) -> Result<(), Error> {
    let nonzero = |w: u32| (w != 0).then_some(w);
    match garment.slot {
        Some(Slot::Prop(_)) => {
            let wide = garment.meta.prop_flags.unwrap_or(0) != 0;
            if wide || word > 0xFFFF {
                garment.meta.prop_flags = nonzero(word);
                // ⚠ With no `propFlags` the game falls back to `flags`, so a word of nothing leaves nothing behind.
                if word == 0 {
                    garment.meta.flags = None;
                }
            } else {
                garment.meta.flags = nonzero(word);
            }
        }
        _ => garment.meta.flags = nonzero(word),
    }
    Ok(())
}

/// State `flag` or take it off, leaving every other bit as it was.
pub fn set(garment: &mut Garment, flag: Flag, on: bool) -> Result<(), Error> {
    let now = word(garment);
    set_word(
        garment,
        match on {
            true => now | flag.mask(),
            false => now & !flag.mask(),
        },
    )
}

/// The bits of `word` that no screen offers by name, kept as a number.
pub fn unnamed(word: u32) -> u32 {
    word & !MEASURED
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{Component, Prop};

    fn component() -> Garment {
        let mut g = Garment::new();
        g.slot = Some(Slot::Component(Component::Jbib));
        g
    }

    fn prop() -> Garment {
        let mut g = Garment::new();
        g.slot = Some(Slot::Prop(Prop::Head));
        g
    }

    #[test]
    fn each_flag_is_the_bit_the_engine_numbers_it_by() {
        let bits: Vec<u32> = Flag::ALL.iter().map(|f| f.bit()).collect();
        assert_eq!(bits, (0..21).collect::<Vec<u32>>());
        for flag in Flag::ALL {
            assert_eq!(flag.mask(), 1 << flag.bit());
            assert_eq!(Flag::from_bit(flag.bit()), Some(flag));
        }
        assert_eq!(Flag::from_bit(21), None);
    }

    #[test]
    fn a_flag_round_trips_its_bit_and_leaves_every_other_bit_alone() {
        for flag in Flag::ALL {
            let mut g = component();
            // An unknown bit, which a screen does not name.
            g.meta.flags = Some(1 << 30);
            set(&mut g, flag, true).unwrap();
            assert!(has(&g, flag), "{}", flag.name());
            assert_eq!(word(&g), (1 << 30) | flag.mask());
            set(&mut g, flag, false).unwrap();
            assert!(!has(&g, flag));
            assert_eq!(g.meta.flags, Some(1 << 30), "the unknown bit survived");
        }
    }

    #[test]
    fn a_prop_states_the_wide_word_when_it_has_one_and_the_narrow_one_otherwise() {
        let mut g = prop();
        g.meta.flags = Some(Flag::Armoured.mask());
        assert!(has(&g, Flag::Armoured));
        // With `propFlags` stated the game reads those, and `flags` is no longer the answer.
        g.meta.prop_flags = Some(Flag::UsePhysicsHat2.mask());
        assert!(has(&g, Flag::UsePhysicsHat2));
        assert!(!has(&g, Flag::Armoured));
        set(&mut g, Flag::Bulky, true).unwrap();
        assert_eq!(
            g.meta.prop_flags,
            Some(Flag::UsePhysicsHat2.mask() | Flag::Bulky.mask())
        );
        assert_eq!(
            g.meta.flags,
            Some(Flag::Armoured.mask()),
            "flags is not touched"
        );
    }

    #[test]
    fn a_prop_with_no_wide_word_keeps_a_narrow_one_in_flags_and_goes_wide_when_it_must() {
        let mut g = prop();
        set(&mut g, Flag::DefaultHelmet, true).unwrap();
        assert_eq!(g.meta.flags, Some(Flag::DefaultHelmet.mask()));
        assert_eq!(g.meta.prop_flags, None);
        set(&mut g, Flag::HideInFirstPerson, true).unwrap();
        assert_eq!(
            g.meta.prop_flags,
            Some(Flag::DefaultHelmet.mask() | Flag::HideInFirstPerson.mask()),
            "bit 16 does not fit the 16 bits of flags"
        );
        set(&mut g, Flag::DefaultHelmet, false).unwrap();
        set(&mut g, Flag::HideInFirstPerson, false).unwrap();
        assert_eq!((g.meta.flags, g.meta.prop_flags), (None, None));
    }

    #[test]
    fn a_bit_the_staged_documents_never_state_is_not_offered_by_name() {
        assert!(Flag::Armoured.measured() && Flag::UsePhysicsHat2.measured());
        for flag in [
            Flag::Wet,
            Flag::NotIndoors,
            Flag::HighDetail,
            Flag::WetLessWet,
        ] {
            assert!(!flag.measured(), "{}", flag.name());
        }
        assert_eq!(
            unnamed(Flag::Wet.mask() | Flag::Bulky.mask() | 1 << 25),
            Flag::Wet.mask() | 1 << 25
        );
    }
}
