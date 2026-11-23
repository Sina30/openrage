//! A GTA VI material's parameters as its effect declares them, for the material editor: one added
//! from the effect's own list, a changed effect given its parameters, a value reset to the
//! effect's default.
//!
//! ⚠ A drawable holds only the members of the effect's buffers whose slot has bit 8 set, each at
//! the byte offset the effect states, in the buffer its buffer's rank among those names; the
//! engine fills the other buffers, so their members are never laid out.

use or_io::gta5::shaders::sgd5::{constant_units, drawable_parameter};
use or_io::gta5::shaders::shader_defs::{
    ShaderDef, ShaderParamDef, ShaderParamKind, ShaderParamValue,
};
use or_io::gta6::headerless::{Parameter, ParameterValue, Shader};

/// Where a declared constant lies, its buffer, byte offset and words, or why that is not known.
type Place = Result<(u8, u16, usize), &'static str>;

/// The hash a drawable's record of `def` states: its semantic's, else its name's.
pub(crate) fn record_hash(def: &ShaderParamDef) -> u32 {
    def.semantic_hash.unwrap_or(def.name_hash)
}

/// The words one constant of `def` holds; `None` for an array or a type of no fixed size.
fn units(def: &ShaderParamDef) -> Option<usize> {
    let code = match def.kind {
        ShaderParamKind::Bool => 0x00,
        ShaderParamKind::Int => 0x01,
        ShaderParamKind::Float => 0x09,
        ShaderParamKind::Float2 => 0x0A,
        ShaderParamKind::Float3 => 0x0B,
        ShaderParamKind::Float4 => 0x0C,
        ShaderParamKind::Float4x4 => 0x0E,
        ShaderParamKind::Unknown(code) => code,
        _ => return None,
    };
    // No buffer a drawable holds has an array, so what a record of one states is not measured.
    (def.array_len <= 1).then(|| constant_units(code)).flatten()
}

/// The words `def`'s default sets, where it sets `units` of them.
fn default_words(def: &ShaderParamDef, units: usize) -> Option<Vec<u32>> {
    let words: Vec<u32> = match def.default.as_ref()? {
        ShaderParamValue::Floats(f) => f.iter().map(|x| x.to_bits()).collect(),
        ShaderParamValue::Uints(w) => w.clone(),
    };
    (words.len() == units).then_some(words)
}

/// The default `effect` sets for the parameter a record of `hash` names, where it is `len` words.
pub(crate) fn declared_default(effect: &ShaderDef, hash: u32, len: usize) -> Option<Vec<u32>> {
    let def = drawable_parameter(effect, hash)?;
    default_words(def, units(def)?).filter(|w| w.len() == len)
}

/// Each constant of `effect` a drawable can hold, in the table's order, with its place.
fn places(effect: &ShaderDef) -> Vec<(&ShaderParamDef, Place)> {
    (effect.parameters.iter())
        .filter_map(|def| {
            let (buffer, offset) = def.drawable_place?;
            let place = units(def).map(|units| (buffer, offset, units));
            Some((
                def,
                place.ok_or(tr!("it is an array or of a type whose size is not known")),
            ))
        })
        .collect()
}

/// Each declared texture of `effect`, in its table's order.
pub(crate) fn declared_textures(effect: &ShaderDef) -> impl Iterator<Item = &ShaderParamDef> {
    (effect.parameters.iter()).filter(|d| d.kind == ShaderParamKind::Texture)
}

/// Each parameter `effect` declares that a drawable's record can hold and `shader` does not: its
/// record's hash, its name, and whether it is a texture.
pub(crate) fn missing(shader: &Shader, effect: &ShaderDef) -> Vec<(u32, String, bool)> {
    let held = |hash: u32| shader.parameters.iter().any(|p| p.hash == hash);
    let textures = declared_textures(effect).map(|d| (d, true));
    let constants = (effect.parameters.iter())
        .filter(|d| d.drawable_place.is_some() && units(d).is_some())
        .map(|d| (d, false));
    (textures.chain(constants))
        .filter(|(d, _)| !held(record_hash(d)))
        .map(|(d, texture)| (record_hash(d), d.name.clone(), texture))
        .collect()
}

/// `shader` with the constant of `effect` whose record states `hash` added at the place the effect
/// states, holding the effect's default, else zeros.
pub(crate) fn with_declared(
    shader: &Shader,
    effect: &ShaderDef,
    hash: u32,
) -> Result<Shader, String> {
    if shader.parameters.iter().any(|p| p.hash == hash) {
        return Err(tr!("The shader already holds that parameter.").to_string());
    }
    let places = places(effect);
    let (def, place) = (places.iter())
        .find(|(d, _)| record_hash(d) == hash)
        .ok_or_else(|| tr!("The effect declares no constant of that hash.").to_string())?;
    let not_added = |why: &str| tr!("{name} is not added: {why}", name = def.name, why = why);
    let (buffer, offset, units) = (*place).map_err(not_added)?;
    // A shader holding a constant elsewhere than the effect places it is stale: its layout stays.
    for p in &shader.parameters {
        let ParameterValue::Constant {
            buffer: b,
            offset: o,
            ..
        } = &p.value
        else {
            continue;
        };
        let stated = (places.iter()).find(|(d, _)| record_hash(d) == p.hash);
        if stated.is_some_and(|(_, at)| at.map(|(b, o, _)| (b, o)) != Ok((*b, *o))) {
            return Err(not_added(tr!(
                "the shader holds a constant elsewhere than the effect places it"
            )));
        }
    }
    let words = default_words(def, units).unwrap_or_else(|| vec![0; units]);
    let mut out = shader.clone();
    let value = ParameterValue::Constant {
        buffer,
        offset,
        words,
    };
    out.parameters.push(Parameter { hash, value });
    Ok(out)
}

/// `shader` given `effect` and what it declares: each texture in the effect's order, keeping the
/// key of the slot of its hash, then each constant a drawable holds at the place the effect states,
/// keeping the words one of its hash and size holds, else the effect's default, else zeros.
pub(crate) fn declared_shader(shader: &Shader, effect: &ShaderDef) -> Result<Shader, String> {
    let held_key = |hash: u32| {
        shader.parameters.iter().find_map(|p| match p.value {
            ParameterValue::Texture { slot } if p.hash == hash => shader
                .texture_keys
                .get(usize::from(slot))
                .copied()
                .flatten(),
            _ => None,
        })
    };
    let too_many = || tr!("The effect declares more than 255 parameters.").to_string();
    let (mut parameters, mut texture_keys) = (Vec::new(), Vec::new());
    for def in declared_textures(effect) {
        let slot = u8::try_from(texture_keys.len()).map_err(|_| too_many())?;
        let hash = record_hash(def);
        let value = ParameterValue::Texture { slot };
        parameters.push(Parameter { hash, value });
        texture_keys.push(held_key(hash));
    }
    for (def, place) in places(effect) {
        let (buffer, offset, units) = place.map_err(|why| {
            tr!(
                "The effect's {name} is not laid out: {why}",
                name = def.name,
                why = why
            )
        })?;
        let hash = record_hash(def);
        let held = shader.parameters.iter().find_map(|p| match &p.value {
            ParameterValue::Constant { words, .. } if p.hash == hash && words.len() == units => {
                Some(words.clone())
            }
            _ => None,
        });
        let words = (held.or_else(|| default_words(def, units))).unwrap_or_else(|| vec![0; units]);
        let value = ParameterValue::Constant {
            buffer,
            offset,
            words,
        };
        parameters.push(Parameter { hash, value });
    }
    Ok(Shader {
        name_hash: effect.name_hash,
        texture_keys,
        parameters,
    })
}

#[cfg(test)]
#[path = "gta6_material_declared_tests.rs"]
mod tests;
