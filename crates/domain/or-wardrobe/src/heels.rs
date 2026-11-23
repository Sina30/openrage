//! Heels: the value a feet drawable states in expression 4, named for what a screen shows.
//!
//! A heel is no field of the engine's. The staged variation metadata gives 174 feet drawables a
//! value in the fifth `pedXml_expressionMods` and nothing in the other four, and the staged creature
//! metadata gives the feet drawables that state expression 4 one shape for it (199 of 206 alike);
//! a heel is those two statements together. What the value is for the game, a weight or a height in
//! some unit, is not measured: the preview raises the ped by a tenth of it, as other tools draw it
//! (`dress::METRES_PER_HEEL_UNIT`).

use crate::edit::Error;
use crate::model::{Expression, Garment};
use crate::names::{Component, Slot};

/// The expression a heel is stated as, which is also the index of the modifier that holds its value.
pub const EXPRESSION: u32 = 4;

/// The most any value is taken to be, either way: the tools that offer a value stop at 1.5 and 1.7.
pub const LARGEST: f32 = 2.0;

/// The value a feet garment states for expression 4, none where it states none.
pub fn of(garment: &Garment) -> Option<f32> {
    if garment.slot != Some(Slot::Component(Component::Feet)) {
        return None;
    }
    garment
        .meta
        .ped_xml_expression_mods
        .and_then(|mods| mods.get(EXPRESSION as usize).copied())
        .filter(|value| *value != 0.0)
}

/// The expression a heel adds to the creature metadata, in the shape the staged feet drawables state.
pub fn expression() -> Expression {
    Expression {
        index: EXPRESSION,
        tracks: vec![33],
        ids: vec![28462],
        types: vec![2],
        components: vec![1],
    }
}

/// State `value` for the feet garment, or take its heels off with none; nothing changes where it is refused.
pub fn set(garment: &mut Garment, value: Option<f32>) -> Result<(), Error> {
    if garment.slot != Some(Slot::Component(Component::Feet)) {
        return Err(Error::Refused("only a feet drawable has heels".to_string()));
    }
    let value = value.filter(|v| *v != 0.0);
    if let Some(v) = value {
        if !v.is_finite() || v.abs() > LARGEST {
            return Err(Error::Refused(format!(
                "a heel value is a number from -{LARGEST} to {LARGEST}, and {v} is not"
            )));
        }
    }
    let mut mods = garment.meta.ped_xml_expression_mods.unwrap_or([0.0; 5]);
    mods[EXPRESSION as usize] = value.unwrap_or(0.0);
    garment.meta.ped_xml_expression_mods = (mods != [0.0; 5]).then_some(mods);
    garment.expressions.retain(|e| e.index != EXPRESSION);
    if value.is_some() {
        garment.expressions.push(expression());
    }
    Ok(())
}
