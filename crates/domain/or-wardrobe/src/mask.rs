//! The bits of a drawable's `propMask` and of a prop's `renderFlags`, named by the engine and kept as the numbers they are.
//!
//! The game builds the name of a component's files from the mask: it asks for the `r` form of a
//! model and its textures when [`Property::RaceTex`] is set, the `m` form when [`Property::MatchPrev`]
//! is, and the `u` form otherwise. So a garment's skin and its mask state one fact twice, and
//! [`default_for`] is the mask a garment that states none gets.

use crate::edit::Error;
use crate::model::Garment;
use crate::names::{Skin, Slot};

macro_rules! bits {
    ($(#[$doc:meta])* $enum:ident { $($variant:ident = $bit:literal, $name:literal, $label:literal;)* }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $enum {
            $($variant,)*
        }

        impl $enum {
            pub const ALL: &'static [$enum] = &[$($enum::$variant),*];

            /// The bit it is, counted from 0.
            pub fn bit(self) -> u8 {
                match self {
                    $($enum::$variant => $bit,)*
                }
            }

            /// The number with only its bit set.
            pub fn mask(self) -> u8 {
                1 << self.bit()
            }

            /// The name the engine gives it.
            pub fn name(self) -> &'static str {
                match self {
                    $($enum::$variant => $name,)*
                }
            }

            /// The words a screen shows for it.
            pub fn label(self) -> &'static str {
                match self {
                    $($enum::$variant => $label,)*
                }
            }
        }
    };
}

bits! {
    /// One of the engine's `ePVDrawblProperty`, a bit of a component drawable's `propMask`.
    Property {
        Active = 0, "PV_DRAWBL_ACTIVE", "Has a drawable";
        Alpha = 1, "PV_DRAWBL_ALPHA", "Has transparent parts, drawn last";
        Decal = 2, "PV_DRAWBL_DECAL", "Uses the decal pass";
        Cutout = 3, "PV_DRAWBL_CUTOUT", "Uses the cutout pass";
        RaceTex = 4, "PV_DRAWBL_RACE_TEX", "Textures follow the ped's skin";
        MatchPrev = 5, "PV_DRAWBL_MATCH_PREV", "Matches the drawable before it";
        Palette = 6, "PV_DRAWBL_PALETTE", "Has a palette";
        ProxyTex = 7, "PV_DRAWBL_PROXY_TEX", "Is a proxy texture";
    }
}

bits! {
    /// One of the engine's `ePropRenderFlags`, a bit of a prop's `renderFlags`.
    RenderFlag {
        Alpha = 0, "PRF_ALPHA", "Has transparent parts";
        Decal = 1, "PRF_DECAL", "Uses the decal pass";
        Cutout = 2, "PRF_CUTOUT", "Uses the cutout pass";
    }
}

/// The bits of `propMask` some staged document states, which are all eight.
pub const PROPERTIES_MEASURED: u8 = 0xFF;

/// The bits of `renderFlags` some staged document states: the decal pass is named by the engine and no staged prop uses it.
pub const RENDER_MEASURED: u8 = 0b101;

/// The mask a garment of `skin` states when it states none: it has a drawable, and one of the race kind says so.
pub fn default_for(skin: Skin) -> u8 {
    match skin {
        Skin::Universal => Property::Active.mask(),
        Skin::Race => Property::Active.mask() | Property::RaceTex.mask(),
    }
}

/// Give the garment `skin`, and the skin bit of the mask it states the value that skin implies: the game builds the names of its files from the mask.
pub fn follow_skin(garment: &mut Garment, skin: Skin) {
    garment.skin = skin;
    if let Some(stated) = garment.meta.prop_mask {
        let bit = Property::RaceTex.mask();
        let made = match skin {
            Skin::Race => stated | bit,
            Skin::Universal => stated & !bit,
        };
        garment.meta.prop_mask = (made != default_for(skin)).then_some(made);
    }
}

/// The `propMask` the garment's drawable states.
pub fn properties(garment: &Garment) -> u8 {
    garment
        .meta
        .prop_mask
        .unwrap_or_else(|| default_for(garment.skin))
}

pub fn has_property(garment: &Garment, property: Property) -> bool {
    properties(garment) & property.mask() != 0
}

/// The form of its files the garment's mask asks the game for: `m` with the match bit, `r` with the skin bit, `u` otherwise.
pub fn form_asked(garment: &Garment) -> char {
    match (
        has_property(garment, Property::MatchPrev),
        has_property(garment, Property::RaceTex),
    ) {
        (true, _) => 'm',
        (_, true) => 'r',
        _ => 'u',
    }
}

/// The properties a screen offers for a component: all of them, but the match bit only where the mask states it, because the pack ships the `u` or `r` form of its skin and never `m`, so ticking it could only stop the build, and a stated one must be able to come off.
pub fn offered(garment: &Garment) -> Vec<Property> {
    (Property::ALL.iter().copied())
        .filter(|p| *p != Property::MatchPrev || has_property(garment, *p))
        .collect()
}

fn component(garment: &Garment, what: &str) -> Result<(), Error> {
    match garment.slot {
        Some(Slot::Component(_)) => Ok(()),
        _ => Err(Error::Refused(format!("only a component has {what}"))),
    }
}

fn prop(garment: &Garment, what: &str) -> Result<(), Error> {
    match garment.slot {
        Some(Slot::Prop(_)) => Ok(()),
        _ => Err(Error::Refused(format!("only a prop has {what}"))),
    }
}

/// State exactly `mask` as the garment's `propMask`; the mask it would get anyway is not stated.
pub fn set_properties(garment: &mut Garment, mask: u8) -> Result<(), Error> {
    component(garment, "a propMask")?;
    garment.meta.prop_mask = (mask != default_for(garment.skin)).then_some(mask);
    Ok(())
}

/// State `property` or take it off, leaving every other bit as it was.
pub fn set_property(garment: &mut Garment, property: Property, on: bool) -> Result<(), Error> {
    let now = properties(garment);
    set_properties(
        garment,
        match on {
            true => now | property.mask(),
            false => now & !property.mask(),
        },
    )
}

/// The `renderFlags` the garment's prop states.
pub fn render_flags(garment: &Garment) -> u8 {
    garment.meta.render_flags.unwrap_or(0)
}

pub fn has_render(garment: &Garment, flag: RenderFlag) -> bool {
    render_flags(garment) & flag.mask() != 0
}

/// State exactly `bits` as the prop's `renderFlags`. The field holds three bits, so a bit past them is refused.
pub fn set_render_flags(garment: &mut Garment, bits: u8) -> Result<(), Error> {
    prop(garment, "renderFlags")?;
    if bits > 0b111 {
        return Err(Error::Refused(format!(
            "renderFlags has three bits, and {bits} states more"
        )));
    }
    garment.meta.render_flags = (bits != 0).then_some(bits);
    Ok(())
}

pub fn set_render(garment: &mut Garment, flag: RenderFlag, on: bool) -> Result<(), Error> {
    let now = render_flags(garment);
    set_render_flags(
        garment,
        match on {
            true => now | flag.mask(),
            false => now & !flag.mask(),
        },
    )
}

/// How firmly a prop stays on: `0` comes off whenever the ped is hurt and `255` never does (the game's chance of a knock-off is one minus this over 255).
pub fn set_stickyness(garment: &mut Garment, value: Option<u8>) -> Result<(), Error> {
    prop(garment, "stickyness")?;
    garment.meta.stickyness = value.filter(|v| *v != 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{Component, Prop};

    fn component_garment(skin: Skin) -> Garment {
        let mut g = Garment::new();
        g.slot = Some(Slot::Component(Component::Uppr));
        g.skin = skin;
        g
    }

    fn prop_garment() -> Garment {
        let mut g = Garment::new();
        g.slot = Some(Slot::Prop(Prop::Eyes));
        g
    }

    #[test]
    fn each_bit_is_the_number_the_engine_gives_it() {
        let bits: Vec<u8> = Property::ALL.iter().map(|p| p.bit()).collect();
        assert_eq!(bits, (0..8).collect::<Vec<u8>>());
        let render: Vec<u8> = RenderFlag::ALL.iter().map(|p| p.bit()).collect();
        assert_eq!(render, (0..3).collect::<Vec<u8>>());
        assert_eq!(
            Property::ALL.iter().fold(0, |m, p| m | p.mask()),
            PROPERTIES_MEASURED
        );
    }

    #[test]
    fn the_mask_a_garment_states_none_for_follows_its_skin() {
        assert_eq!(default_for(Skin::Universal), 1);
        assert_eq!(default_for(Skin::Race), 1 | 16);
        let mut g = component_garment(Skin::Race);
        assert!(has_property(&g, Property::RaceTex));
        g.skin = Skin::Universal;
        assert!(!has_property(&g, Property::RaceTex));
    }

    #[test]
    fn a_skin_chosen_after_a_mask_was_stated_moves_the_skin_bit_with_it_and_leaves_the_other_bits()
    {
        let mut g = component_garment(Skin::Race);
        g.meta.prop_mask = Some(1 | 16 | 2);
        follow_skin(&mut g, Skin::Universal);
        assert_eq!((g.skin, g.meta.prop_mask), (Skin::Universal, Some(1 | 2)));
        follow_skin(&mut g, Skin::Race);
        assert_eq!(g.meta.prop_mask, Some(1 | 16 | 2));
        // A mask that is the one the skin implies is not stated at all, and one never stated stays so.
        g.meta.prop_mask = Some(1 | 16);
        follow_skin(&mut g, Skin::Race);
        assert_eq!(g.meta.prop_mask, None);
        g.meta.prop_mask = None;
        follow_skin(&mut g, Skin::Universal);
        assert_eq!((g.skin, g.meta.prop_mask), (Skin::Universal, None));
    }

    #[test]
    fn a_bit_round_trips_and_the_mask_that_is_the_default_is_not_stated() {
        let mut g = component_garment(Skin::Universal);
        set_property(&mut g, Property::Cutout, true).unwrap();
        assert_eq!(g.meta.prop_mask, Some(1 | 8));
        assert!(has_property(&g, Property::Cutout));
        set_property(&mut g, Property::Cutout, false).unwrap();
        assert_eq!(g.meta.prop_mask, None, "back to what it would get anyway");
        // An unknown-to-us combination survives an edit of another bit.
        g.meta.prop_mask = Some(0b1000_0001);
        set_property(&mut g, Property::Alpha, true).unwrap();
        assert_eq!(g.meta.prop_mask, Some(0b1000_0011));
    }

    #[test]
    fn a_prop_has_render_flags_and_stickyness_and_a_component_has_neither() {
        let mut p = prop_garment();
        set_render(&mut p, RenderFlag::Alpha, true).unwrap();
        set_render(&mut p, RenderFlag::Cutout, true).unwrap();
        assert_eq!(p.meta.render_flags, Some(0b101));
        assert!(set_render_flags(&mut p, 0b1000).is_err());
        set_stickyness(&mut p, Some(100)).unwrap();
        assert_eq!(p.meta.stickyness, Some(100));
        set_stickyness(&mut p, Some(0)).unwrap();
        assert_eq!(p.meta.stickyness, None);
        let mut c = component_garment(Skin::Universal);
        assert!(set_render(&mut c, RenderFlag::Alpha, true).is_err());
        assert!(set_stickyness(&mut c, Some(1)).is_err());
        assert!(set_property(&mut p, Property::Alpha, true).is_err());
    }
}
