//! UV crop over a garment: the texture coordinates of its models move into the part of the texture they use, and every texture they sample is cut to that part.
//!
//! ⚠ **The textures are cropped with the coordinates, or not at all.** A garment's colour textures are the pack's sources and the
//! normal and specular maps of a ped component are embedded in the model; the coordinates of every model of the garment, at every
//! level of detail, read all of them, so a crop that missed one would draw it wrongly. The window is the smallest aligned
//! power-of-two part of the texture that holds the coordinates, so each texture's crop is a copy of whole blocks
//! ([`or_source::crop`]); a crop that would keep more than half of the texture is not made.

use std::path::Path;

use or_interchange::doctor::through_scene;
use or_interchange::doctor::uv::{self, UvBox, Window};
use or_interchange::extract::Extracted;
use or_interchange::import::Imported;
use or_interchange::{ExtractOptions, LodSelection, SceneBuilder};
use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{GtaDrawable, PgDictionary, RmcDrawable};
use or_source::asset::Source;
use or_source::textures::{self, Extraction};

use super::{join, levels_of, load, models_of, source_of, ModelReport, Op, Refusal, Report};
use crate::edit::{garment_mut, Copying, Error, Revision, Watch};
use crate::facts::is_source;
use crate::intake::Scratch;
use crate::model::Wardrobe;
use crate::textures as pack_textures;

/// The most of a texture a crop may keep: a crop of more is not worth the picture it asks a person to take on trust.
pub const MOST_KEPT: f32 = 0.5;

/// The box the first texture coordinates of the drawables of `bytes` fill, with why it is none where it is.
fn used_in(bytes: &[u8]) -> Result<Option<UvBox>, String> {
    let seal = Rsc7::peek(bytes).map_err(|e| format!("it is not a resource: {e}"))?;
    let format = or_io::gta5::formats::format("ydd").ok_or("no .ydd format is declared")?;
    let target = format
        .target_of_version(seal.version())
        .map_err(|e| e.to_string())?;
    if target.is_gen9() {
        used::<Gen9>(bytes)
    } else {
        used::<Gen8>(bytes)
    }
}

fn used<T: Extracted>(bytes: &[u8]) -> Result<Option<UvBox>, String>
where
    PgDictionary<GtaDrawable<T>>: File,
{
    let dictionary =
        PgDictionary::<GtaDrawable<T>>::from_bytes(bytes).map_err(|e| e.to_string())?;
    let mut found: Option<UvBox> = None;
    for entry in dictionary.entries.iter().flatten() {
        let mut builder = SceneBuilder::new(&entry.base.name);
        builder.push(
            &entry.base.name,
            &entry.base,
            &ExtractOptions {
                lods: LodSelection::All,
                textures: false,
            },
        );
        if let Some(b) = uv::used(&builder.finish()) {
            let all = found.get_or_insert(b);
            for k in 0..2 {
                all.min[k] = all.min[k].min(b.min[k]);
                all.max[k] = all.max[k].max(b.max[k]);
            }
        }
    }
    Ok(found)
}

/// What a crop of one model's embedded textures came to, as lines.
fn embedded<T: Imported>(
    drawable: &mut RmcDrawable<T>,
    window: &Window,
    scratch: &mut Scratch,
) -> Result<Vec<String>, String>
where
    PgDictionary<T::Texture>: File,
{
    let Some(group) = drawable.shader_group.as_deref_mut() else {
        return Ok(Vec::new());
    };
    let Some(dictionary) = group.texture_dictionary.as_deref() else {
        return Ok(Vec::new());
    };
    if dictionary.entries.iter().flatten().next().is_none() {
        return Ok(Vec::new());
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
    let report = or_source::crop::crop(&folder, window, &|| false).map_err(|p| join(&p))?;
    let compiled = textures::compile(&Source::open(&folder).map_err(|p| p.to_string())?)
        .map_err(|p| join(&p))?;
    let cropped = PgDictionary::<T::Texture>::from_bytes(&compiled.bytes)
        .map_err(|e| format!("the cropped embedded textures do not read back: {e}"))?;
    group.texture_dictionary = Some(Box::new(cropped));
    let mut lines: Vec<String> = report
        .cropped
        .iter()
        .map(|c| format!("embedded {}: {} -> {}", c.texture, c.was, c.now))
        .collect();
    lines.extend(
        report
            .left
            .iter()
            .map(|l| format!("embedded {} was left: {}", l.texture, l.why)),
    );
    Ok(lines)
}

/// `bytes` with the coordinates of every drawable moved into `window` and its embedded textures cropped to it.
fn cropped(
    bytes: &[u8],
    window: &Window,
    scratch: &mut Scratch,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let seal = Rsc7::peek(bytes).map_err(|e| format!("it is not a resource: {e}"))?;
    let format = or_io::gta5::formats::format("ydd").ok_or("no .ydd format is declared")?;
    let target = format
        .target_of_version(seal.version())
        .map_err(|e| e.to_string())?;
    if target.is_gen9() {
        cropped_in::<Gen9>(bytes, window, scratch)
    } else {
        cropped_in::<Gen8>(bytes, window, scratch)
    }
}

fn cropped_in<T: Imported>(
    bytes: &[u8],
    window: &Window,
    scratch: &mut Scratch,
) -> Result<(Vec<u8>, Vec<String>), String>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    PgDictionary<T::Texture>: File,
{
    let mut dictionary =
        PgDictionary::<GtaDrawable<T>>::from_bytes(bytes).map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    for entry in dictionary.entries.iter_mut().flatten() {
        let (mut base, ()) = through_scene(&entry.base, None, |scene| {
            window.apply(scene);
            Ok(())
        })
        .map_err(|e| e.to_string())?;
        lines.extend(embedded(&mut base, window, scratch)?);
        entry.base = base;
    }
    let bytes = dictionary
        .to_bytes()
        .map_err(|e| format!("the model would not save, and the game would misread it: {e}"))?;
    Ok((bytes, lines))
}

/// What the window says, in words.
pub(super) fn window_words(window: &Window) -> String {
    let part = |a: &or_interchange::doctor::uv::Axis| match a.power {
        0 => "all".to_string(),
        p => format!("1/{}", 1u32 << p),
    };
    format!(
        "{} of the width by {} of the height, {:.1} percent of the texture",
        part(&window.x),
        part(&window.y),
        window.share() * 100.0
    )
}

/// The window for the part of the texture `used` fills, or why there is none to crop to.
pub(super) fn window_for(used: &UvBox) -> Result<Window, String> {
    if !used.inside_unit() {
        return Err("the coordinates reach past the texture, which a texture that repeats is drawn with, and a crop would break the repeat".into());
    }
    let share = used.covers();
    let window = Window::holding(used).ok_or_else(|| {
        format!(
            "the coordinates use {:.0} percent of the texture, and no aligned power-of-two part of it holds them",
            share * 100.0
        )
    })?;
    if window.share() > MOST_KEPT {
        return Err(format!(
            "the smallest part that holds the coordinates is {}, and a crop keeps at most half",
            window_words(&window)
        ));
    }
    Ok(window)
}

/// Crop the models and the textures of garment `id` of the pack in `folder` to the part of the texture its coordinates use, as one step of Undo.
pub(super) fn garment(
    folder: &Path,
    id: &str,
    watch: &Watch,
) -> Result<(Report, Option<Revision>), Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let (paths, _) = models_of(&wardrobe, id)?;
    let held = before
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let mut sources: Vec<String> = Vec::new();
    for variation in &held.variations {
        if let Some(texture) = &variation.texture {
            if !sources.contains(texture) {
                sources.push(texture.clone());
            }
        }
    }
    let mut report = Report {
        op: Op::UvCrop.label().to_string(),
        models: Vec::new(),
        refused: Vec::new(),
        notes: Vec::new(),
    };
    let refuse = |report: &mut Report,
                  file: &str,
                  why: String|
     -> Result<(Report, Option<Revision>), Error> {
        report.refused.push(Refusal {
            file: file.to_string(),
            why,
        });
        Ok((report.clone(), None))
    };

    // The coordinates of every model decide the window, so each is read before any is changed.
    let mut loaded = Vec::new();
    let mut union: Option<UvBox> = None;
    for rel in &paths {
        watch.check()?;
        let one = match load(folder, rel).and_then(|l| l.bytes.clone().map(|b| (l, b))) {
            Ok(one) => one,
            Err(why) => return refuse(&mut report, rel, why),
        };
        match used_in(&one.1) {
            Ok(Some(b)) => {
                let all = union.get_or_insert(b);
                for k in 0..2 {
                    all.min[k] = all.min[k].min(b.min[k]);
                    all.max[k] = all.max[k].max(b.max[k]);
                }
            }
            Ok(None) => return refuse(&mut report, rel, "it states no texture coordinates".into()),
            Err(why) => return refuse(&mut report, rel, why),
        }
        loaded.push((rel.clone(), one.0, one.1));
    }
    let Some(union) = union else {
        return refuse(&mut report, id, "the garment holds no model".into());
    };
    let window = match window_for(&union) {
        Ok(window) => window,
        Err(why) => {
            report.notes.push(format!("nothing was cropped: {why}"));
            return Ok((report, None));
        }
    };
    for source in &sources {
        if !is_source(source) || !folder.join(source).is_dir() {
            return refuse(
                &mut report,
                source,
                "the texture is not a source folder of the pack, so it cannot be cropped with the model: make it a source first".into(),
            );
        }
    }

    let mut copying = Copying::new(folder);
    let mut scratch = Scratch::default();
    let done: Result<(), String> = (|| {
        let mut moved: std::collections::BTreeMap<String, String> = Default::default();
        for source in &sources {
            watch.check().map_err(|e| e.to_string())?;
            let copy = copying
                .duplicate_source(source)
                .map_err(|e| e.to_string())?;
            let cut =
                or_source::crop::crop(&folder.join(&copy), &window, &|| watch.cancel.is_set())
                    .map_err(|p| format!("{source}: {}", join(&p)))?;
            report.notes.extend(
                cut.cropped
                    .iter()
                    .map(|c| format!("{}: {} -> {}", c.texture, c.was, c.now)),
            );
            report.notes.extend(
                cut.left
                    .iter()
                    .map(|l| format!("{} was left: {}", l.texture, l.why)),
            );
            moved.insert(source.clone(), copy);
        }
        for (rel, one, bytes) in &loaded {
            watch.check().map_err(|e| e.to_string())?;
            let levels = levels_of_bytes(bytes);
            let (made, mut lines) =
                cropped(bytes, &window, &mut scratch).map_err(|e| format!("{rel}: {e}"))?;
            lines.insert(
                0,
                format!("the coordinates moved into {}", window_words(&window)),
            );
            let into = source_of(
                &mut copying,
                &mut scratch,
                &one.stem,
                &made,
                None,
                &Op::UvCrop,
                &lines,
            )
            .map_err(|e| format!("{rel}: {e}"))?;
            report.models.push(ModelReport {
                file: rel.clone(),
                into,
                before: levels,
                after: levels,
                lines,
                textures: 0,
            });
        }
        let g = garment_mut(&mut wardrobe, id).map_err(|e| e.to_string())?;
        for done in &report.models {
            for model in g.model.iter_mut().chain(g.alternates.iter_mut()) {
                if *model == done.file {
                    *model = done.into.clone();
                }
            }
        }
        for variation in &mut g.variations {
            if let Some(copy) = variation.texture.as_ref().and_then(|t| moved.get(t)) {
                variation.texture = Some(copy.clone());
            }
        }
        Ok(())
    })();
    if let Err(why) = done {
        copying.undo();
        return Err(match watch.cancel.is_set() {
            true => Error::Stopped,
            false => Error::Refused(why),
        });
    }
    let revision = pack_textures::finish(folder, before, &wardrobe, copying)?;
    Ok((report, revision))
}

/// The triangles of each level of the drawables of `bytes`, which a crop leaves as they are.
pub(super) fn levels_of_bytes(bytes: &[u8]) -> super::Levels {
    fn count<T: Extracted>(bytes: &[u8]) -> super::Levels
    where
        PgDictionary<GtaDrawable<T>>: File,
    {
        let mut out = super::Levels::default();
        if let Ok(dictionary) = PgDictionary::<GtaDrawable<T>>::from_bytes(bytes) {
            for entry in dictionary.entries.iter().flatten() {
                super::add(&mut out, &levels_of(&entry.base));
            }
        }
        out
    }
    let gen9 = Rsc7::peek(bytes)
        .ok()
        .and_then(|seal| {
            or_io::gta5::formats::format("ydd")
                .and_then(|f| f.target_of_version(seal.version()).ok())
        })
        .is_some_and(|t| t.is_gen9());
    if gen9 {
        count::<Gen9>(bytes)
    } else {
        count::<Gen8>(bytes)
    }
}
