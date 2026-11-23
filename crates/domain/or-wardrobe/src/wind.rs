//! The wind scale of a garment's cloth: the value each cloth of a `.yld` states, and the one the build writes in its place.
//!
//! A cloth file has no source form, so it stays the file it was linked as, and the scale a person chose is the
//! garment's own statement in `wardrobe.json`. The build reads the linked file, states the scale in every cloth
//! of it and ships that, so a change of wind is an edit with Undo that touches no file, and a build reads what it
//! shipped back and says when it does not hold the scale ([`crate::verify`]).
//!
//! ⚠ What the scale does in the game is not measured: the layout of `CharacterClothController` states one
//! `m_WindScale` and its default is 1.0, and nothing here says more.

use or_io::decl::File;
use or_io::gta5::cloth_dict::CharacterCloth;
use or_io::gta5::shared::PgDictionary;

use crate::edit::Error;
use crate::model::Garment;
use crate::names::Slot;

/// The wind scale a controller states when nobody changed it: the default the layout of the controller documents.
pub const DEFAULT: f32 = 1.0;

/// The largest scale a person may state, either way.
///
/// ⚠ A sanity bound against a mistyped number and no value of the engine's: the shipped cloths all state the one
/// value [`SHIPPED_MOST`], so nothing measured says where a scale stops making sense.
pub const LARGEST: f32 = 100.0;

/// The largest wind scale a shipped `.yld` states: all 58 controllers of the shipped cloths state exactly this
/// (`the_wind_of_every_shipped_cloth_is_read_and_stated_again`).
pub const SHIPPED_MOST: f32 = 1.0;

type Dictionary = PgDictionary<CharacterCloth>;

/// The wind scale of each cloth of the dictionary in `bytes` that has a controller, in the order of its entries.
pub fn stated(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let dictionary = Dictionary::from_bytes(bytes).map_err(|e| e.to_string())?;
    Ok(dictionary
        .entries
        .iter()
        .flatten()
        .filter_map(|cloth| cloth.cloth_controller.as_ref())
        .map(|controller| controller.wind_scale)
        .collect())
}

/// `bytes` with `scale` stated as the wind scale of every cloth that has a controller.
pub fn with_scale(bytes: &[u8], scale: f32) -> Result<Vec<u8>, String> {
    let mut dictionary = Dictionary::from_bytes(bytes).map_err(|e| e.to_string())?;
    let mut held = 0usize;
    for cloth in dictionary.entries.iter_mut().flatten() {
        if let Some(controller) = cloth.cloth_controller.as_mut() {
            controller.wind_scale = scale;
            held += 1;
        }
    }
    if held == 0 {
        return Err("it holds no cloth with a controller to state a wind in".into());
    }
    dictionary.to_bytes().map_err(|e| e.to_string())
}

/// State `scale` as the wind scale of the cloth of `garment`, or take the statement back with none so that the cloth ships as its file is.
///
/// Refused for a number that is not a scale, and for a garment that has no cloth to state it in; nothing changes then.
pub fn set(garment: &mut Garment, scale: Option<f32>) -> Result<(), Error> {
    if let Some(scale) = scale {
        if !scale.is_finite() || scale.abs() > LARGEST {
            return Err(Error::Refused(format!(
                "a wind scale is a number from -{LARGEST} to {LARGEST}, and {scale} is not"
            )));
        }
        if garment.slot.is_some_and(Slot::is_prop) {
            return Err(Error::Refused(
                "a prop owns no cloth to state a wind in".into(),
            ));
        }
        if garment.cloth.is_none() {
            return Err(Error::Refused(
                "the garment has no cloth linked, so there is nothing to state a wind in".into(),
            ));
        }
    }
    garment.wind_scale = scale;
    Ok(())
}

/// The wind scale each cloth states of the file `cloth`, a path inside the pack in `folder`, which a reference leads away from; why not where the file is not here or does not read.
pub fn stated_by(folder: &std::path::Path, cloth: &str) -> Result<Vec<f32>, String> {
    use crate::follow::{follow, Led};
    let path = match follow(folder, cloth) {
        Led::Own => folder.join(cloth.replace('/', std::path::MAIN_SEPARATOR_STR)),
        Led::To { at, .. } => at,
        Led::Id(id) => return Err(format!("{id} is only an id, and the file is in no project")),
        Led::Nowhere { why, .. } => return Err(why),
    };
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    stated(&bytes)
}
