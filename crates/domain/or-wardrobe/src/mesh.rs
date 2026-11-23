//! The mesh doctor over a clothing pack: levels of detail, a rig that is mended, weights that are cleaned and left turned to right, each one step of Undo.
//!
//! ⚠ **The doctor makes sources.** A pack holds only sources, so what an operation makes is a new drawables source: the
//! model is compiled as a build compiles it, changed as `or-interchange` changes a drawable, and extracted again, and the
//! garment names the new source. The source it was is not touched, and Undo names it again. The geometry the doctor adds
//! (a level of detail) is part of the new source's glb and base, never a compiled file of the pack.
//!
//! ⚠ **It keeps what the drawable has.** A level of detail copies the high level's shaders and vertex declarations, a mirror
//! and a weight cleanup write the geometries they read, and none makes a material, a layout or a bone.

use std::path::{Path, PathBuf};

use or_interchange::doctor::levels::{add_level, missing, LevelRequest};
use or_interchange::doctor::mirror::mirror_drawable;
use or_interchange::doctor::{rig, through_scene, uv};
use or_interchange::extract::Extracted;
use or_interchange::import::Imported;
use or_interchange::{drawable_scene, Bone, ExtractOptions, LodSelection, LodSlot, Scene};
use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{GtaDrawable, PgDictionary, RmcDrawable};
use or_source::asset::{How, Source};
use or_source::models::{self, Dictionary, Extraction, Rules};
use serde::Serialize;

use crate::edit::{garment_mut, Copying, Error, Revision, Watch};
use crate::facts::{is_source, stays_inside};
use crate::import::protected;
use crate::intake::Scratch;
use crate::model::Wardrobe;
use crate::names::{Component, Slot};
use crate::textures;

pub(crate) mod embedded;
mod materials;
mod uvcrop;

/// What the doctor is asked to do to a garment's models.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Make these levels of detail (1 to 3, with the share of the high level's triangles each keeps).
    Levels {
        levels: Vec<(usize, f32)>,
        /// Make again a level the model has; off, it is left as it is.
        replace: bool,
    },
    /// Mend the groups of the rig no skeleton knows, and then clean the weights.
    Rig,
    /// Drop the weights that are noise, make the rest add up and give a vertex with none the skin of its neighbour.
    Weights,
    /// Turn left and right over, with the skin.
    Mirror,
    /// Move the texture coordinates into the part of the texture they use, and cut every texture the garment samples to that part.
    UvCrop,
    /// Optimise the textures embedded in the models: their format, their mips and their size.
    Embedded(or_source::optimise::Options),
    /// State the constants a shader declares that a material lacks.
    Materials,
}

impl Op {
    /// The words a button and a Log line use.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Levels { .. } => "Add levels of detail",
            Self::Rig => "Repair rig",
            Self::Weights => "Clean weights",
            Self::Mirror => "Mirror",
            Self::UvCrop => "Crop texture coordinates",
            Self::Embedded(_) => "Optimise embedded textures",
            Self::Materials => "Complete materials",
        }
    }
}

/// What a level of detail holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Count {
    pub triangles: usize,
    pub vertices: usize,
}

/// The four levels of detail of a model, high to very low.
pub type Levels = [Count; 4];

/// Why a model was not changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub file: String,
    pub why: String,
}

/// What one model came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelReport {
    pub file: String,
    /// The source the garment names now.
    pub into: String,
    pub before: Levels,
    pub after: Levels,
    pub lines: Vec<String>,
    /// How many embedded textures the operation changed, for the one that works on them.
    pub textures: usize,
}

/// What an operation did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub op: String,
    pub models: Vec<ModelReport>,
    pub refused: Vec<Refusal>,
    /// What else the operation did or why it did nothing, as lines.
    pub notes: Vec<String>,
}

impl Report {
    fn new(op: &Op) -> Self {
        Self {
            op: op.label().to_string(),
            models: Vec::new(),
            refused: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Whether any model was changed.
    pub fn changed(&self) -> bool {
        !self.models.is_empty()
    }

    /// The report as lines a person reads: each model with what it did, then what was refused.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for model in &self.models {
            lines.push(format!("{} is now {}", model.file, model.into));
            lines.push(format!(
                "  triangles by level: {} -> {}",
                shown(&model.before),
                shown(&model.after)
            ));
            lines.extend(model.lines.iter().map(|l| format!("  {l}")));
        }
        lines.extend(self.notes.iter().cloned());
        lines.extend(
            self.refused
                .iter()
                .map(|r| format!("refused {}: {}", r.file, r.why)),
        );
        lines
    }

    /// One sentence for a toast.
    pub fn sentence(&self) -> String {
        match (self.models.len(), self.refused.len()) {
            (0, 0) if !self.notes.is_empty() => format!("{}: {}", self.op, self.notes[0]),
            (0, 0) => format!("{}: nothing to do", self.op),
            (0, r) => format!("{}: {r} model(s) refused, none changed", self.op),
            (n, 0) => format!("{}: {n} model(s) changed", self.op),
            (n, r) => format!("{}: {n} model(s) changed, {r} refused", self.op),
        }
    }
}

/// `1,200 / 400 / 60 / -` for the triangles of the levels a model has.
pub fn shown(levels: &Levels) -> String {
    levels
        .iter()
        .map(|c| {
            if c.triangles == 0 {
                "-".to_string()
            } else {
                c.triangles.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

/// What a look at one model found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelExam {
    pub file: String,
    pub levels: Levels,
    /// The levels from the second on that it holds no model for.
    pub missing: Vec<usize>,
    /// What is wrong with the rig and the weights, as sentences.
    pub rig: Vec<String>,
    /// What a look at the heel found, for a pair of shoes.
    pub heel: Option<String>,
    /// The part of the texture the model uses, as a share of it, where its coordinates are in the texture.
    pub uv_used: Option<f32>,
    /// What a crop of the texture would keep, where there is a part of it worth the crop.
    pub uv_crop: Option<String>,
    /// Why it was not looked at.
    pub unreadable: Option<String>,
}

/// What a look at a garment's models found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Examination {
    pub models: Vec<ModelExam>,
}

impl ModelExam {
    /// The model by the part of its file name that tells it apart: `jbib_000_u` for `models/mp_m_pack^jbib_000_u.drawables.src`.
    pub fn short_name(&self) -> &str {
        let leaf = self.file.rsplit('/').next().unwrap_or(&self.file);
        let tail = leaf.rsplit('^').next().unwrap_or(leaf);
        tail.split('.').next().unwrap_or(tail)
    }

    /// The triangles of each level in words: `High 1,133 · Medium 346 · Low 71 · Very low none`.
    pub fn levels_in_words(&self) -> String {
        let grouped = |n: usize| {
            let digits = n.to_string();
            let mut out = String::new();
            for (at, digit) in digits.chars().enumerate() {
                if at > 0 && (digits.len() - at).is_multiple_of(3) {
                    out.push(',');
                }
                out.push(digit);
            }
            out
        };
        or_interchange::doctor::levels::LEVELS
            .iter()
            .zip(&self.levels)
            .map(|(name, c)| match c.triangles {
                0 => format!("{name} none"),
                n => format!("{name} {}", grouped(n)),
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// What was found of this model as lines a person reads, the first naming it and the levels it holds.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("{}: {}", self.short_name(), self.levels_in_words())];
        if let Some(why) = &self.unreadable {
            lines.push(format!("  not looked at: {why}"));
        }
        if !self.missing.is_empty() {
            let names: Vec<&str> = (self.missing.iter())
                .map(|l| or_interchange::doctor::levels::LEVELS[*l])
                .collect();
            lines.push(format!(
                "  no model for the {} level of detail",
                names.join(" and ").to_ascii_lowercase()
            ));
        }
        lines.extend(self.rig.iter().map(|r| format!("  {r}")));
        if let Some(heel) = &self.heel {
            lines.push(format!("  heel: {heel}"));
        }
        if let Some(used) = self.uv_used {
            lines.push(format!(
                "  the texture coordinates use {:.0} percent of the texture",
                used * 100.0
            ));
        }
        if let Some(crop) = &self.uv_crop {
            lines.push(format!("  a crop would keep {crop}"));
        }
        lines
    }
}

impl Examination {
    /// The findings as lines a person reads.
    pub fn lines(&self) -> Vec<String> {
        self.models.iter().flat_map(ModelExam::lines).collect()
    }
}

fn refused(why: impl ToString) -> Error {
    Error::Refused(why.to_string())
}

fn join(problems: &[or_source::Problem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// A model of the pack, compiled as a build compiles it.
struct Loaded {
    /// The compiled model, or why it did not compile: a rig no skeleton knows is mended from the glb and not from this.
    bytes: Result<Vec<u8>, String>,
    stem: String,
    /// The folder of the source it came from.
    source: Option<PathBuf>,
}

fn load(folder: &Path, rel: &str) -> Result<Loaded, String> {
    if !stays_inside(rel) || or_project::reference::is_reference(rel) {
        return Err(
            "it leads to a file that is not in this pack: make it a source of the pack first"
                .into(),
        );
    }
    let at = folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if at.is_dir() {
        let source = Source::open(&at).map_err(|p| p.to_string())?;
        if source.manifest.kind != models::DRAWABLES {
            return Err(format!(
                "it is a {} source, and the doctor works on a drawables source",
                source.manifest.kind
            ));
        }
        let bytes = models::compile(&source)
            .map(|c| c.bytes)
            .map_err(|p| join(&p));
        return Ok(Loaded {
            bytes,
            stem: source.manifest.name,
            source: Some(at),
        });
    }
    if !at.is_file() || !rel.to_ascii_lowercase().ends_with(".ydd") {
        return Err("it is not a .ydd file or a drawables source that is there".into());
    }
    if let Some(why) = protected(&at) {
        return Err(why);
    }
    let bytes = std::fs::read(&at).map_err(|e| format!("cannot be read: {e}"))?;
    let stem = at
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    Ok(Loaded {
        bytes: Ok(bytes),
        stem,
        source: None,
    })
}

fn levels_of<T: Extracted>(drawable: &RmcDrawable<T>) -> Levels {
    let scene = drawable_scene(
        &drawable.name,
        drawable,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    let mut out = Levels::default();
    for p in scene.primitives() {
        if let LodSlot::Level(l) = p.lod {
            out[usize::from(l).min(3)].triangles += p.triangle_count();
            out[usize::from(l).min(3)].vertices += p.vertex_count();
        }
    }
    out
}

fn add(into: &mut Levels, more: &Levels) {
    for (a, b) in into.iter_mut().zip(more) {
        a.triangles += b.triangles;
        a.vertices += b.vertices;
    }
}

/// What changing a model's bytes came to.
struct Made {
    bytes: Vec<u8>,
    lines: Vec<String>,
    before: Levels,
    after: Levels,
    changed: bool,
}

fn name_of<T: Extracted>(slot: usize, drawable: &RmcDrawable<T>) -> String {
    match drawable.name.as_str() {
        "" => format!("drawable {slot}"),
        name => name.to_string(),
    }
}

/// `bytes` with `op` done to every drawable of the dictionary.
fn apply(bytes: &[u8], op: &Op, skeleton: Option<&[Bone]>) -> Result<Made, String> {
    if matches!(op, Op::Materials) {
        return materials::complete(bytes);
    }
    let seal = Rsc7::peek(bytes).map_err(|e| format!("it is not a resource: {e}"))?;
    let format = or_io::gta5::formats::format("ydd").ok_or("no .ydd format is declared")?;
    let target = format
        .target_of_version(seal.version())
        .map_err(|e| e.to_string())?;
    if target.is_gen9() {
        apply_in::<Gen9>(bytes, op, skeleton)
    } else {
        apply_in::<Gen8>(bytes, op, skeleton)
    }
}

fn apply_in<T: Imported>(bytes: &[u8], op: &Op, skeleton: Option<&[Bone]>) -> Result<Made, String>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    let mut dictionary =
        PgDictionary::<GtaDrawable<T>>::from_bytes(bytes).map_err(|e| e.to_string())?;
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
        let said = |why: String| format!("{at}: {why}");
        let mut notes = Vec::new();
        let base = match op {
            Op::Levels { levels, replace } => {
                let mut current = entry.base.clone();
                for (level, ratio) in levels {
                    if !(1..4).contains(level) {
                        return Err(said(format!(
                            "level {level} is not one of the 1 to 3 made from the High level"
                        )));
                    }
                    if !current.lod_group.lods[*level].models.is_empty() && !*replace {
                        continue;
                    }
                    let request = LevelRequest {
                        level: *level,
                        ratio: *ratio,
                        replace: *replace,
                    };
                    let (next, report) =
                        add_level(&current, &request).map_err(|e| said(e.to_string()))?;
                    notes.extend(report.lines());
                    if report.skipped.is_some() {
                        continue;
                    }
                    current = next;
                    made.changed = true;
                }
                current
            }
            Op::Mirror => {
                let (next, report) =
                    mirror_drawable(&entry.base, skeleton).map_err(|e| said(e.to_string()))?;
                notes.extend(report.lines());
                made.changed = true;
                next
            }
            Op::UvCrop => {
                return Err(
                    "a crop is made over the whole garment, which a model alone does not state"
                        .into(),
                )
            }
            Op::Embedded(_) | Op::Materials => {
                return Err("this is not an operation on one drawable".into())
            }
            Op::Rig | Op::Weights => {
                let (next, cleaned) = through_scene(&entry.base, skeleton, |scene| {
                    let mut total = rig::Cleaned::default();
                    for p in scene
                        .objects
                        .iter_mut()
                        .flat_map(|o| o.meshes.iter_mut())
                        .flat_map(|m| m.primitives.iter_mut())
                    {
                        let one = rig::clean_weights(p, rig::FAINT);
                        total.faint += one.faint;
                        total.renormalised += one.renormalised;
                        total.unweighted += one.unweighted;
                    }
                    Ok(total)
                })
                .map_err(|e| said(e.to_string()))?;
                if cleaned.is_empty() {
                    entry.base.clone()
                } else {
                    notes.extend(cleaned.lines());
                    made.changed = true;
                    next
                }
            }
        };
        made.lines.extend(
            notes
                .into_iter()
                .map(|n| if several { format!("{at}: {n}") } else { n }),
        );
        entry.base = base;
        add(&mut made.after, &levels_of(&entry.base));
    }
    if made.changed {
        made.bytes = dictionary
            .to_bytes()
            .map_err(|e| format!("the model would not save, and the game would misread it: {e}"))?;
    }
    Ok(made)
}

/// The new source for `bytes`, made in the scratch folder and put in the pack's `models/`.
fn source_of(
    copying: &mut Copying,
    scratch: &mut Scratch,
    stem: &str,
    bytes: &[u8],
    skeleton: Option<&[Bone]>,
    op: &Op,
    lines: &[String],
) -> Result<String, String> {
    let parent = scratch.next().map_err(|e| e.to_string())?;
    let extract = |skeleton: Option<&[Bone]>| {
        models::extract_drawables(
            bytes,
            stem,
            &parent,
            &Extraction {
                from_game: false,
                virtual_path: None,
                textures: false,
                skeleton,
            },
        )
    };
    // A skeleton that does not fit the palette is no reason to refuse the file: the source then keeps the weights it has.
    let folder = match extract(skeleton) {
        Ok(folder) => folder,
        Err(_) if skeleton.is_some() => {
            let _ = std::fs::remove_dir_all(&parent);
            std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
            extract(None).map_err(|p| join(&p))?
        }
        Err(p) => return Err(join(&p)),
    };
    let refuse = |p: or_source::Problem| p.to_string();
    let mut source = Source::open(&folder).map_err(refuse)?;
    source.manifest.origin.how = How::Imported;
    source.manifest.origin.extra.insert(
        "doctored".into(),
        serde_json::json!({ "op": op.label(), "lines": lines }),
    );
    source.write_manifest().map_err(refuse)?;
    copying
        .put_tree_in("models", &folder, false)
        .map_err(|e| e.to_string())
}

/// Copy the folder `from` to `to`, files and folders.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The drawables a source holds: each one's folder, its rules and its glb.
fn entries_of(source: &Path) -> Result<Vec<(PathBuf, Rules)>, String> {
    let text = std::fs::read_to_string(source.join(models::DICTIONARY))
        .map_err(|e| format!("{}: {e}", models::DICTIONARY))?;
    let list: Dictionary =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", models::DICTIONARY))?;
    list.entries
        .into_iter()
        .map(|entry| {
            let dir = source.join(models::DIR).join(&entry.folder);
            let rules = std::fs::read_to_string(dir.join(models::RULES))
                .map_err(|e| format!("{}: {e}", models::RULES))
                .and_then(|t| {
                    serde_json::from_str::<Rules>(&t).map_err(|e| format!("{}: {e}", models::RULES))
                })?;
            Ok((dir, rules))
        })
        .collect()
}

/// What is wrong with the names of the bones a source's glbs state, and what repairing them says, with the glbs written repaired into `into` when it is given.
///
/// A glb that states no skin, as a ped component's does until it is extracted beside the ped's skeleton, has no names to check; its weights are the base's and the compiled model is judged for them.
fn rig_of_glbs(source: &Path, into: Option<&Path>) -> Result<(Vec<String>, Vec<String>), String> {
    let (mut findings, mut repairs) = (Vec::new(), Vec::new());
    for (dir, rules) in entries_of(source)? {
        if rules.skeleton.is_empty() {
            continue;
        }
        let glb = std::fs::read(dir.join(models::MODEL))
            .map_err(|e| format!("{}: {e}", models::MODEL))?;
        let mut scene: Scene = or_interchange::gltf::read::load(Path::new(models::MODEL), &glb)
            .map_err(|e| e.to_string())?;
        let found = rig::check(&scene, &rules.skeleton);
        let names: Vec<&rig::Finding> = found
            .iter()
            .filter(|f| {
                matches!(
                    f.defect,
                    rig::Defect::UnnamedGroup | rig::Defect::UnknownBone | rig::Defect::Misspelt
                )
            })
            .collect();
        if names.is_empty() {
            continue;
        }
        findings.extend(names.iter().map(|f| f.sentence()));
        if let Some(into) = into {
            repairs.extend(rig::repair(&mut scene, &rules.skeleton));
            let written = or_interchange::gltf::write(
                &scene,
                &source
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                or_interchange::gltf::Container::Glb,
            )
            .primary;
            let relative = dir.strip_prefix(source).map_err(|e| e.to_string())?;
            std::fs::write(into.join(relative).join(models::MODEL), written)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok((findings, repairs))
}

/// The paths of a garment's models: its own and its alternates, once each.
fn models_of(wardrobe: &Wardrobe, id: &str) -> Result<(Vec<String>, bool), Error> {
    let garment = wardrobe
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let mut paths: Vec<String> = Vec::new();
    for model in garment.model.iter().chain(garment.alternates.iter()) {
        if !paths.contains(model) {
            paths.push(model.clone());
        }
    }
    let feet = garment.slot == Some(Slot::Component(Component::Feet));
    Ok((paths, feet))
}

/// Look at the models of garment `id` of the pack in `folder`: their levels, their rig and weights, a shoe's heel and the texture they use. Nothing is written.
pub fn examine(folder: &Path, id: &str, skeleton: Option<&[Bone]>) -> Result<Examination, Error> {
    let wardrobe = Wardrobe::read(folder)?;
    let (paths, feet) = models_of(&wardrobe, id)?;
    if paths.is_empty() {
        return Err(refused("the garment holds no model to look at"));
    }
    let mut out = Vec::new();
    for rel in paths {
        let mut exam = ModelExam {
            file: rel.clone(),
            levels: Levels::default(),
            missing: Vec::new(),
            rig: Vec::new(),
            heel: None,
            uv_used: None,
            uv_crop: None,
            unreadable: None,
        };
        match load(folder, &rel) {
            Ok(loaded) => {
                if let Err(why) = look(&loaded, &mut exam, skeleton, feet) {
                    exam.unreadable = Some(why);
                }
            }
            Err(why) => exam.unreadable = Some(why),
        }
        out.push(exam);
    }
    Ok(Examination { models: out })
}

fn look(
    loaded: &Loaded,
    exam: &mut ModelExam,
    skeleton: Option<&[Bone]>,
    feet: bool,
) -> Result<(), String> {
    if let Some(source) = &loaded.source {
        let (names, _) = rig_of_glbs(source, None)?;
        exam.rig.extend(names);
    }
    let bytes = loaded.bytes.as_ref().map_err(Clone::clone)?;
    let seal = Rsc7::peek(bytes).map_err(|e| format!("it is not a resource: {e}"))?;
    let format = or_io::gta5::formats::format("ydd").ok_or("no .ydd format is declared")?;
    let target = format
        .target_of_version(seal.version())
        .map_err(|e| e.to_string())?;
    if target.is_gen9() {
        look_in::<Gen9>(bytes, exam, skeleton, feet)
    } else {
        look_in::<Gen8>(bytes, exam, skeleton, feet)
    }
}

fn look_in<T: Imported>(
    bytes: &[u8],
    exam: &mut ModelExam,
    skeleton: Option<&[Bone]>,
    feet: bool,
) -> Result<(), String>
where
    PgDictionary<GtaDrawable<T>>: File,
{
    let dictionary =
        PgDictionary::<GtaDrawable<T>>::from_bytes(bytes).map_err(|e| e.to_string())?;
    let names: Vec<String> = skeleton
        .map(|b| b.iter().map(|b| b.name.clone()).collect())
        .unwrap_or_default();
    let mut absent: Option<Vec<usize>> = None;
    let (mut lo, mut hi, mut seen) = ([f32::MAX; 2], [f32::MIN; 2], false);
    for entry in dictionary.entries.iter().flatten() {
        let drawable = &entry.base;
        add(&mut exam.levels, &levels_of(drawable));
        // The game picks among three levels; the fourth, very low, is one no ped component of the install states.
        let here: Vec<usize> = missing(drawable).into_iter().filter(|l| *l < 3).collect();
        absent = Some(match absent {
            Some(m) => m.into_iter().filter(|l| here.contains(l)).collect(),
            None => here,
        });
        let mut builder = or_interchange::SceneBuilder::new(&drawable.name);
        if let Some(bones) = skeleton {
            builder = builder.with_reference_skeleton(bones.to_vec());
        }
        builder.push(
            &drawable.name,
            drawable,
            &ExtractOptions {
                lods: LodSelection::All,
                textures: false,
            },
        );
        let scene = builder.finish();
        let own: Vec<String> = scene
            .objects
            .iter()
            .find(|o| !o.bones.is_empty())
            .map(|o| o.bones.iter().map(|b| b.name.clone()).collect())
            .unwrap_or_default();
        let known = if own.is_empty() { &names } else { &own };
        for f in rig::check(&scene, known) {
            // The names are judged against the skeleton the scene was read with, so a mismatch there is the file's.
            exam.rig.push(f.sentence());
        }
        if feet {
            let heel = rig::heel_bones(&scene, known);
            exam.heel = Some(heel.sentence());
        }
        if let Some(b) = uv::used(&scene) {
            if b.inside_unit() {
                seen = true;
                for k in 0..2 {
                    lo[k] = lo[k].min(b.min[k]);
                    hi[k] = hi[k].max(b.max[k]);
                }
            } else {
                // Coordinates past the texture repeat it, and a crop would break the repeat.
                seen = false;
                exam.uv_used = None;
                break;
            }
        }
    }
    exam.missing = absent.unwrap_or_default();
    if seen {
        exam.uv_used = Some((hi[0] - lo[0]) * (hi[1] - lo[1]));
        let used = uv::UvBox { min: lo, max: hi };
        exam.uv_crop = uvcrop::window_for(&used)
            .ok()
            .map(|w| uvcrop::window_words(&w));
    }
    Ok(())
}

/// Run `op` over the models of garment `id` of the pack in `folder`: each becomes a new source the garment names, as one step of Undo.
///
/// Stopped by `watch`, or by a model the operation cannot be done to, the pack and its folder are as they were: a model refused for its own reason is listed in the report and the others are done.
pub fn run(
    folder: &Path,
    id: &str,
    op: &Op,
    skeleton: Option<&[Bone]>,
    watch: &Watch,
) -> Result<(Report, Option<Revision>), Error> {
    if matches!(op, Op::UvCrop) {
        return uvcrop::garment(folder, id, watch);
    }
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    if models_of(&wardrobe, id)?.0.is_empty() {
        return Err(refused("the garment holds no model"));
    }
    let mut report = Report::new(op);
    let mut copying = Copying::new(folder);
    let mut scratch = Scratch::default();
    if let Err(e) = run_in(
        &mut wardrobe,
        &mut copying,
        &mut scratch,
        id,
        op,
        skeleton,
        watch,
        &mut report,
    ) {
        copying.undo();
        return Err(e);
    }
    let revision = textures::finish(folder, before, &wardrobe, copying)?;
    Ok((report, revision))
}

/// Run `op` over the models of the garments `ids` (every garment with a model where none) of the pack in `folder`, as one step of Undo.
///
/// A model several garments name is made into one new source, which they all name; a garment with no model is passed over. The operations
/// that work on one drawable of a garment's own (a crop of the texture coordinates) are asked of a garment at a time, with [`run`].
pub fn run_many(
    folder: &Path,
    ids: Option<&[String]>,
    op: &Op,
    skeleton: Option<&[Bone]>,
    watch: &Watch,
) -> Result<(Report, Option<Revision>), Error> {
    if matches!(op, Op::UvCrop) {
        return Err(refused(
            "a crop is made over one garment, which names the texture its coordinates are for",
        ));
    }
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let mut copying = Copying::new(folder);
    let report = match run_many_in(&mut wardrobe, &mut copying, ids, op, skeleton, watch) {
        Ok(report) => report,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    let revision = textures::finish(folder, before, &wardrobe, copying)?;
    Ok((report, revision))
}

/// [`run_many`] over a pack already read and a copy already begun, for a pipeline that finishes both itself: the caller undoes `copying` on an error.
pub(crate) fn run_many_in(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    ids: Option<&[String]>,
    op: &Op,
    skeleton: Option<&[Bone]>,
    watch: &Watch,
) -> Result<Report, Error> {
    let wanted: Vec<String> = wardrobe
        .garments
        .iter()
        .filter(|g| g.model.is_some())
        .map(|g| g.id.clone())
        .filter(|id| ids.is_none_or(|ids| ids.iter().any(|i| i == id)))
        .collect();
    let mut report = Report::new(op);
    let mut scratch = Scratch::default();
    let mut made: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for id in &wanted {
        // A model an earlier garment already made a source of is that source for this garment too.
        let (paths, _) = models_of(wardrobe, id)?;
        let again: Vec<(&String, &String)> = paths
            .iter()
            .filter_map(|p| Some((p, made.get(p)?)))
            .collect();
        if !again.is_empty() {
            let g = garment_mut(wardrobe, id)?;
            for (rel, into) in &again {
                for model in g.model.iter_mut().chain(g.alternates.iter_mut()) {
                    if model == *rel {
                        *model = (*into).clone();
                    }
                }
            }
        }
        let fresh = report.models.len();
        run_in(
            wardrobe,
            copying,
            &mut scratch,
            id,
            op,
            skeleton,
            watch,
            &mut report,
        )?;
        for done in &report.models[fresh..] {
            made.insert(done.file.clone(), done.into.clone());
        }
    }
    Ok(report)
}

/// The models of garment `id` of `wardrobe` made into new sources by `op`, which the garment names afterwards; what was done and what was refused go to `report`.
#[allow(clippy::too_many_arguments)]
fn run_in(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    scratch: &mut Scratch,
    id: &str,
    op: &Op,
    skeleton: Option<&[Bone]>,
    watch: &Watch,
    report: &mut Report,
) -> Result<(), Error> {
    let folder = copying.folder;
    let (paths, _) = models_of(wardrobe, id)?;
    let total = paths.len() as u64;
    for (n, rel) in paths.iter().enumerate() {
        watch.check()?;
        watch.progress.reached(n as u64, total);
        watch.progress.now_on(rel.clone());
        let mut skipped = Vec::new();
        match one(copying, scratch, folder, rel, op, skeleton, &mut skipped) {
            Ok(Some(done)) => {
                let g = garment_mut(wardrobe, id)?;
                for model in g.model.iter_mut().chain(g.alternates.iter_mut()) {
                    if model == rel {
                        *model = done.into.clone();
                    }
                }
                report.models.push(done);
            }
            Ok(None) => report.notes.extend(skipped.into_iter().map(|n| {
                if paths.len() > 1 {
                    format!("{rel}: {n}")
                } else {
                    n
                }
            })),
            Err(why) => report.refused.push(Refusal {
                file: rel.clone(),
                why,
            }),
        }
    }
    watch.progress.reached(total, total);
    Ok(())
}

/// One model: its new source, or none where the operation had nothing to do to it, in which case `skipped` holds what the operation said of why.
fn one(
    copying: &mut Copying,
    scratch: &mut Scratch,
    folder: &Path,
    rel: &str,
    op: &Op,
    skeleton: Option<&[Bone]>,
    skipped: &mut Vec<String>,
) -> Result<Option<ModelReport>, String> {
    if is_source(rel) && !folder.join(rel).is_dir() {
        return Err("the source folder is not there".into());
    }
    let mut loaded = load(folder, rel)?;
    let mut lines = Vec::new();
    // A glb whose bones no skeleton knows is mended in a copy that is compiled and thrown away: the source it was is not touched.
    if matches!(op, Op::Rig) {
        if let Some(source) = loaded.source.clone() {
            let held = scratch.next().map_err(|e| e.to_string())?.join("repaired");
            copy_tree(&source, &held).map_err(|e| e.to_string())?;
            let (_, repairs) = rig_of_glbs(&source, Some(&held))?;
            if !repairs.is_empty() {
                let patched = Source::open(&held).map_err(|p| p.to_string())?;
                let compiled = models::compile(&patched).map_err(|p| join(&p))?;
                loaded.bytes = Ok(compiled.bytes);
                lines.extend(repairs);
            }
        }
    }
    let compiled = loaded.bytes.clone()?;
    let mut made = apply(&compiled, op, skeleton)?;
    made.changed |= !lines.is_empty();
    if !made.changed {
        skipped.extend(made.lines);
        return Ok(None);
    }
    if made.bytes.is_empty() {
        // Only the glbs were mended: the compiled bytes of the mended copy are the model.
        made.bytes = compiled;
    }
    lines.extend(made.lines);
    let into = source_of(
        copying,
        scratch,
        &loaded.stem,
        &made.bytes,
        skeleton,
        op,
        &lines,
    )?;
    Ok(Some(ModelReport {
        file: rel.to_string(),
        into,
        before: made.before,
        after: made.after,
        lines,
        textures: 0,
    }))
}
