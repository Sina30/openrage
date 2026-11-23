//! Completing the materials of a model: the constants a shader declares that a material lacks are stated, at the values the shader declares.
//!
//! ⚠ **Only constants are added.** A material that lacks a sampler uses the shader's own default for it, and a sampler entry
//! states a binding of its own, empty, which is not that default; a sampler the material lacks is left, and the report says so.
//! The shader database is the one the process has loaded, and a model of Enhanced, whose materials are laid out another way, is
//! left as it is.

use or_io::decl::File;
use or_io::decl::Gen8;
use or_io::gta5::gen8::GrcInstanceDataEntry;
use or_io::gta5::shaders::shader_defs;
use or_io::gta5::{GtaDrawable, PgDictionary};

use super::{add, levels_of, name_of, Levels, Made};

/// The most entries a material holds, which its count field can express.
const MOST: usize = u8::MAX as usize;

/// `bytes` with every constant the shader database declares for a material added to it; unchanged where it declares none, and `Err` where no database is loaded.
pub(super) fn complete(bytes: &[u8]) -> Result<Made, String> {
    if !shader_defs::is_installed() {
        return Err("the shader database is not loaded, so no shader says what its materials lack: read an install first".into());
    }
    let mut dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).map_err(|e| {
        format!("it is not a model of Legacy, whose materials are the ones that are written: {e}")
    })?;
    let several = dictionary.entries.iter().flatten().count() > 1;
    let mut made = Made {
        bytes: Vec::new(),
        lines: Vec::new(),
        before: Levels::default(),
        after: Levels::default(),
        changed: false,
    };
    for (slot, entry) in dictionary.entries.iter_mut().enumerate() {
        let Some(entry) = entry.as_deref_mut() else {
            continue;
        };
        let at = name_of(slot, &entry.base);
        add(&mut made.before, &levels_of(&entry.base));
        let Some(group) = entry.base.shader_group.as_deref_mut() else {
            continue;
        };
        for (index, shader) in group.shaders.iter_mut().enumerate() {
            let Some(declared) = shader_defs::parameters_of(shader.basis_hash.get()) else {
                continue;
            };
            let (mut added, mut samplers) = (Vec::new(), Vec::new());
            for def in declared {
                if shader.entry(def.name_hash).is_some() {
                    continue;
                }
                if def.is_binding() {
                    samplers.push(def.name);
                } else if shader.entries.len() >= MOST {
                    return Err(format!(
                        "shader {index} of {at} holds the most entries a material can, so {} is not added",
                        def.name
                    ));
                } else {
                    shader.insert_entry(GrcInstanceDataEntry::declared(&def));
                    added.push(def.name);
                }
            }
            let prefix = if several {
                format!("{at}: ")
            } else {
                String::new()
            };
            if !added.is_empty() {
                made.changed = true;
                made.lines.push(format!(
                    "{prefix}shader {index} states {}, at the values the shader declares",
                    added.join(", ")
                ));
            }
            if !samplers.is_empty() {
                made.lines.push(format!(
                    "{prefix}shader {index} lacks the samplers {}, which are left: a sampler entry states an empty binding and not the shader's default",
                    samplers.join(", ")
                ));
            }
        }
    }
    made.after = made.before;
    if made.changed {
        made.bytes = dictionary
            .to_bytes()
            .map_err(|e| format!("the model would not save, and the game would misread it: {e}"))?;
    }
    Ok(made)
}
