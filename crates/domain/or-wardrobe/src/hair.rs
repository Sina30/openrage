//! What a hat does to the hair under it, and how tall a hair stands: two values stated as expressions, named for what a screen shows.
//!
//! Like a heel they are no field of the engine's but two statements together: a modifier of the
//! drawable's component info or prop metadata, and an entry in the creature metadata that drives
//! one bone with it. The staged creature metadata drives `BONETAG_HAIR_SCALE` (13201) with the first
//! modifier of a head prop (583 entries, every one of expression 0), and `BONETAG_HAIR_HEIGHT`
//! (48472) with the third modifier of a hair drawable (86 entries, every one of expression 2).
//! `the_fields_a_garment_names_are_the_ones_the_staged_documents_state` holds both to the corpus.
//!
//! ⚠ What the value means in the game is not measured beyond its range: the staged hats state -0.5
//! most often (-1.2 to -0.1 in all) and the hair they cover is drawn smaller. Hide hair is the preset
//! [`HIDES`], the lowest round number the staged hats state; hide ears has no flag or value of its own in
//! the engine's tables, so there is no control for it.

use crate::edit::Error;
use crate::model::{Expression, Garment};
use crate::names::{Component, Prop, Slot};

/// The expression a hat's hair scale is stated as, which is also the index of the modifier that holds its value.
pub const SCALE_EXPRESSION: u32 = 0;
/// `BONETAG_HAIR_SCALE`.
pub const SCALE_BONE: u16 = 13201;
/// The expression a hair's height is stated as, which is also the index of the modifier that holds its value.
pub const HEIGHT_EXPRESSION: u32 = 2;
/// `BONETAG_HAIR_HEIGHT`.
pub const HEIGHT_BONE: u16 = 48472;

/// The most either value is taken to be, either way: the staged hair scales run from -1.2 to -0.1 and the staged heights from 0.18 to 1.9.
pub const LARGEST: f32 = 2.0;

/// The hair scale the Hide hair preset writes, and the one at or below which the preview takes the ped's own hair off.
///
/// ⚠ A convention measured on staged hats and not verified in the game: staged hats state exactly this scale, counted in `the_fields_a_garment_names_are_the_ones_the_staged_documents_state`.
pub const HIDES: f32 = -1.0;

/// The track every staged expression of these two states.
const TRACK: u8 = 33;

fn expression(index: u32, bone: u16) -> Expression {
    Expression {
        index,
        tracks: vec![TRACK],
        ids: vec![bone],
        types: vec![2],
        components: vec![1],
    }
}

fn value(garment: &Garment, index: u32) -> Option<f32> {
    garment
        .meta
        .ped_xml_expression_mods
        .or(garment.meta.expression_mods)
        .and_then(|mods| mods.get(index as usize).copied())
        .filter(|v| *v != 0.0)
}

fn set(garment: &mut Garment, index: u32, bone: u16, value: Option<f32>) -> Result<(), Error> {
    let value = value.filter(|v| *v != 0.0);
    if let Some(v) = value {
        if !v.is_finite() || v.abs() > LARGEST {
            return Err(Error::Refused(format!(
                "a hair value is a number from -{LARGEST} to {LARGEST}, and {v} is not"
            )));
        }
    }
    let field = match garment.slot {
        Some(Slot::Prop(_)) => &mut garment.meta.expression_mods,
        _ => &mut garment.meta.ped_xml_expression_mods,
    };
    let mut mods = field.unwrap_or([0.0; 5]);
    mods[index as usize] = value.unwrap_or(0.0);
    *field = (mods != [0.0; 5]).then_some(mods);
    garment.expressions.retain(|e| e.index != index);
    if value.is_some() {
        garment.expressions.push(expression(index, bone));
    }
    Ok(())
}

/// The hair scale a head prop states, none where it states none.
pub fn scale_of(garment: &Garment) -> Option<f32> {
    match garment.slot {
        Some(Slot::Prop(Prop::Head)) => value(garment, SCALE_EXPRESSION),
        _ => None,
    }
}

/// Whether the hat states a hair scale at or below [`HIDES`].
pub fn hides_hair(garment: &Garment) -> bool {
    scale_of(garment).is_some_and(|scale| scale <= HIDES)
}

/// State `value` as the hair scale of the hat, or take it off with none; nothing changes where it is refused.
pub fn set_scale(garment: &mut Garment, value: Option<f32>) -> Result<(), Error> {
    if garment.slot != Some(Slot::Prop(Prop::Head)) {
        return Err(Error::Refused("only a head prop has a hair scale".into()));
    }
    set(garment, SCALE_EXPRESSION, SCALE_BONE, value)
}

/// The height a hair drawable states, none where it states none.
pub fn height_of(garment: &Garment) -> Option<f32> {
    match garment.slot {
        Some(Slot::Component(Component::Hair)) => value(garment, HEIGHT_EXPRESSION),
        _ => None,
    }
}

/// State `value` as the height of the hair, or take it off with none; nothing changes where it is refused.
pub fn set_height(garment: &mut Garment, value: Option<f32>) -> Result<(), Error> {
    if garment.slot != Some(Slot::Component(Component::Hair)) {
        return Err(Error::Refused("only a hair drawable has a height".into()));
    }
    set(garment, HEIGHT_EXPRESSION, HEIGHT_BONE, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hat() -> Garment {
        let mut g = Garment::new();
        g.slot = Some(Slot::Prop(Prop::Head));
        g
    }

    fn hair() -> Garment {
        let mut g = Garment::new();
        g.slot = Some(Slot::Component(Component::Hair));
        g
    }

    #[test]
    fn a_hats_scale_is_a_prop_modifier_and_the_expression_that_drives_the_hair() {
        let mut g = hat();
        assert_eq!(scale_of(&g), None);
        set_scale(&mut g, Some(-0.5)).unwrap();
        assert_eq!(g.meta.expression_mods, Some([-0.5, 0.0, 0.0, 0.0, 0.0]));
        assert_eq!(
            g.meta.ped_xml_expression_mods, None,
            "a prop's are not a component's"
        );
        assert_eq!(g.expressions, vec![expression(0, SCALE_BONE)]);
        assert_eq!(scale_of(&g), Some(-0.5));
        set_scale(&mut g, None).unwrap();
        assert_eq!(g.meta, Default::default());
        assert!(g.expressions.is_empty());
    }

    #[test]
    fn a_hairs_height_is_a_component_modifier_and_a_value_out_of_range_is_refused() {
        let mut g = hair();
        set_height(&mut g, Some(0.19)).unwrap();
        assert_eq!(
            g.meta.ped_xml_expression_mods,
            Some([0.0, 0.0, 0.19, 0.0, 0.0])
        );
        assert_eq!(height_of(&g), Some(0.19));
        assert!(set_height(&mut g, Some(2.5)).is_err());
        assert!(set_height(&mut g, Some(f32::NAN)).is_err());
        assert_eq!(height_of(&g), Some(0.19), "a refusal changes nothing");
        assert!(set_scale(&mut g, Some(-0.5)).is_err(), "a hair is no hat");
        assert!(
            set_height(&mut hat(), Some(0.1)).is_err(),
            "a hat has no hair height"
        );
    }

    #[test]
    fn the_other_modifiers_and_expressions_of_a_garment_survive() {
        let mut g = hat();
        g.meta.expression_mods = Some([0.0, 0.28, 0.0, 0.0, 0.0]);
        g.expressions.push(expression(1, 1921));
        set_scale(&mut g, Some(-1.0)).unwrap();
        set_scale(&mut g, None).unwrap();
        assert_eq!(g.meta.expression_mods, Some([0.0, 0.28, 0.0, 0.0, 0.0]));
        assert_eq!(g.expressions, vec![expression(1, 1921)]);
    }
}
