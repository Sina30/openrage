//! The optimiser over the textures embedded in a garment's models: their format, their mips and their size, written into a new source of the model.
//!
//! ⚠ **A ped component's normal and specular maps are embedded in its model**, so an optimiser that reads only the textures sources
//! of a pack never reaches them. The dictionary a drawable carries is taken out as a textures source, optimised as
//! [`or_source::optimise`] optimises any, and put back; the geometry, the shaders and every other word of the model are the
//! model's own. The model is a new source and the source it was is not touched, as in every operation of the doctor.

use or_interchange::import::Imported;
use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{GtaDrawable, PgDictionary, RmcDrawable};
use or_source::asset::Source;
use or_source::optimise::{self, Options};
use or_source::textures::{self, Extraction};
use std::collections::BTreeMap;

use super::uvcrop::levels_of_bytes;
use super::{join, load, models_of, source_of, ModelReport, Op, Refusal};
use crate::edit::{garment_mut, Copying, Error, Watch};
use crate::intake::Scratch;
use crate::model::Wardrobe;

/// What the optimiser did to the embedded textures of one model.
struct Embedded {
    bytes: Vec<u8>,
    lines: Vec<String>,
    /// How many textures changed.
    changed: usize,
}

/// The textures embedded in `drawable`, optimised in place; the lines that say what became of each.
fn drawable<T: Imported>(
    drawable: &mut RmcDrawable<T>,
    options: &Options,
    scratch: &mut Scratch,
    at: &str,
) -> Result<(Vec<String>, usize), String>
where
    PgDictionary<T::Texture>: File,
{
    let Some(group) = drawable.shader_group.as_deref_mut() else {
        return Ok((Vec::new(), 0));
    };
    let Some(dictionary) = group.texture_dictionary.as_deref() else {
        return Ok((Vec::new(), 0));
    };
    if dictionary.entries.iter().flatten().next().is_none() {
        return Ok((Vec::new(), 0));
    }
    let bytes = dictionary
        .to_bytes()
        .map_err(|e| format!("the embedded textures would not be written out: {e}"))?;
    let parent = scratch.next().map_err(|e| e.to_string())?;
    let folder = textures::extract(
        &bytes,
        "embedded",
        &parent,
        &Extraction {
            from_game: false,
            virtual_path: None,
        },
    )
    .map_err(|p| join(&p))?;
    let plan = optimise::apply(&folder, options, &|| false).map_err(|p| join(&p))?;
    let mut lines: Vec<String> = plan
        .changes
        .iter()
        .map(|c| format!("{at}embedded {c}"))
        .collect();
    lines.extend(
        plan.left
            .iter()
            .map(|l| format!("{at}embedded {} was left: {}", l.texture, l.why)),
    );
    if plan.changes.is_empty() {
        return Ok((lines, 0));
    }
    let compiled = textures::compile(&Source::open(&folder).map_err(|p| p.to_string())?)
        .map_err(|p| join(&p))?;
    let back = PgDictionary::<T::Texture>::from_bytes(&compiled.bytes)
        .map_err(|e| format!("the optimised embedded textures do not read back: {e}"))?;
    group.texture_dictionary = Some(Box::new(back));
    Ok((lines, plan.changes.len()))
}

fn optimised_in<T: Imported>(
    bytes: &[u8],
    options: &Options,
    scratch: &mut Scratch,
) -> Result<Embedded, String>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    PgDictionary<T::Texture>: File,
{
    let mut dictionary =
        PgDictionary::<GtaDrawable<T>>::from_bytes(bytes).map_err(|e| e.to_string())?;
    let several = dictionary.entries.iter().flatten().count() > 1;
    let (mut lines, mut changed) = (Vec::new(), 0);
    for entry in dictionary.entries.iter_mut().flatten() {
        let at = match several {
            true => format!("{}: ", entry.base.name),
            false => String::new(),
        };
        let (said, n) = drawable(&mut entry.base, options, scratch, &at)?;
        lines.extend(said);
        changed += n;
    }
    let bytes = match changed {
        0 => Vec::new(),
        _ => dictionary
            .to_bytes()
            .map_err(|e| format!("the model would not save, and the game would misread it: {e}"))?,
    };
    Ok(Embedded {
        bytes,
        lines,
        changed,
    })
}

/// `bytes` with the textures embedded in every drawable optimised as `options` say; the bytes are empty where nothing changed.
fn optimised(bytes: &[u8], options: &Options, scratch: &mut Scratch) -> Result<Embedded, String> {
    let seal = Rsc7::peek(bytes).map_err(|e| format!("it is not a resource: {e}"))?;
    let format = or_io::gta5::formats::format("ydd").ok_or("no .ydd format is declared")?;
    let target = format
        .target_of_version(seal.version())
        .map_err(|e| e.to_string())?;
    if target.is_gen9() {
        optimised_in::<Gen9>(bytes, options, scratch)
    } else {
        optimised_in::<Gen8>(bytes, options, scratch)
    }
}

/// What the optimiser did to the embedded textures of a garment's models.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reached {
    pub models: Vec<ModelReport>,
    pub refused: Vec<Refusal>,
}

/// Optimise the textures embedded in the models of garment `id` of `wardrobe`, each model becoming a new source the garment names.
///
/// `done` remembers the new source each model became, so that a model several garments name is changed once. A model with nothing
/// to change, one that is not there and one that is a compiled file with no source form are left, and the ones that are refused say why.
pub(crate) fn garment(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    scratch: &mut Scratch,
    id: &str,
    options: &Options,
    done: &mut BTreeMap<String, Option<String>>,
    watch: &Watch,
) -> Result<Reached, Error> {
    let (paths, _) = models_of(wardrobe, id)?;
    let mut out = Reached::default();
    for rel in paths {
        watch.check()?;
        let into = match done.get(&rel) {
            Some(known) => known.clone(),
            None => {
                let made = one(copying, scratch, &rel, options);
                let into = match made {
                    Ok(Some(report)) => {
                        let into = report.into.clone();
                        out.models.push(report);
                        Some(into)
                    }
                    Ok(None) => None,
                    Err(why) => {
                        out.refused.push(Refusal {
                            file: rel.clone(),
                            why,
                        });
                        None
                    }
                };
                done.insert(rel.clone(), into.clone());
                into
            }
        };
        if let Some(into) = into {
            let g = garment_mut(wardrobe, id)?;
            for model in g.model.iter_mut().chain(g.alternates.iter_mut()) {
                if *model == rel {
                    *model = into.clone();
                }
            }
        }
    }
    Ok(out)
}

fn one(
    copying: &mut Copying,
    scratch: &mut Scratch,
    rel: &str,
    options: &Options,
) -> Result<Option<ModelReport>, String> {
    // A model that is not there, or is not a model the doctor works on, is not an optimisation's to refuse.
    let loaded = match load(copying.folder, rel) {
        Ok(loaded) => loaded,
        Err(_) => return Ok(None),
    };
    let compiled = loaded.bytes.clone()?;
    let made = optimised(&compiled, options, scratch)?;
    if made.changed == 0 {
        return Ok(None);
    }
    let levels = levels_of_bytes(&compiled);
    let into = source_of(
        copying,
        scratch,
        &loaded.stem,
        &made.bytes,
        None,
        &Op::Embedded(*options),
        &made.lines,
    )?;
    Ok(Some(ModelReport {
        file: rel.to_string(),
        into,
        before: levels,
        after: levels,
        lines: made.lines,
        textures: made.changed,
    }))
}
