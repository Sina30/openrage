//! A folder an external editor saves into, bound to a slot of a garment's texture: diffuse, specular or normal.
//!
//! What the editor saves there is put into the texture's source as the picture of that slot, and the live recook of the source does the
//! rest, so the preview follows the save. A binding is a fact about this machine (a folder that is on it), so it is kept in the pack's
//! private folder, which ignores itself, and never in `wardrobe.json`.
//!
//! ⚠ **The picture goes into the source and not into a copy of it**, as a picture an editor saves into the source's own folder does. It is
//! not a step of Undo; the editor's file is left where it is, so the picture can be put back from there.

use std::path::{Path, PathBuf};

use or_source::asset::{unsafe_file_name, Source};
use or_source::json::canonical;
use or_source::maps::{companion, Kind};
use or_source::textures::{self, ColourSpace, Encode, Entry, DIR, RULES};
use serde::{Deserialize, Serialize};

use crate::backup::{make_private, private_dir};
use crate::edit::Error;
use crate::facts::{source_kind, stays_inside};
use crate::model::Wardrobe;

const FILE: &str = "slots.json";

/// The slot of a texture a folder is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapSlot {
    Diffuse,
    Specular,
    Normal,
}

impl MapSlot {
    pub const ALL: [MapSlot; 3] = [Self::Diffuse, Self::Specular, Self::Normal];

    pub fn label(self) -> &'static str {
        match self {
            Self::Diffuse => "Diffuse",
            Self::Specular => "Specular",
            Self::Normal => "Normal",
        }
    }

    /// The word a command line says it by.
    pub fn word(self) -> &'static str {
        match self {
            Self::Diffuse => "diffuse",
            Self::Specular => "specular",
            Self::Normal => "normal",
        }
    }

    pub fn of_word(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|s| s.word().eq_ignore_ascii_case(word))
    }
}

/// A folder bound to a slot of the texture `texture` (counted among the textures of the garment) of the garment `garment`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// The folder, as the machine spells it.
    pub folder: String,
    pub garment: String,
    pub texture: usize,
    pub slot: MapSlot,
}

/// Where the source of a bound texture is and what the slot's texture is called in it.
struct Target {
    /// The source, inside the pack.
    source: String,
    /// The name of the texture of the slot.
    texture: String,
}

/// What putting a picture into a slot did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Delivered {
    /// The source, inside the pack.
    pub source: String,
    pub texture: String,
    /// The picture, inside the source's `textures/` folder.
    pub file: String,
}

/// The bindings the pack has.
pub fn bindings(pack: &Path) -> Vec<Binding> {
    std::fs::read_to_string(private_dir(pack).join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn keep(pack: &Path, list: &[Binding]) -> Result<(), Error> {
    let dir =
        make_private(pack).map_err(|e| Error::Io(private_dir(pack).display().to_string(), e))?;
    let text = serde_json::to_string_pretty(list).map_err(|e| Error::Refused(e.to_string()))?;
    std::fs::write(dir.join(FILE), text)
        .map_err(|e| Error::Io(dir.join(FILE).display().to_string(), e))
}

/// The source and the texture a binding stands for, or why it stands for none.
fn target(pack: &Path, binding: &Binding) -> Result<Target, Error> {
    let wardrobe = Wardrobe::read(pack)?;
    let garment = wardrobe
        .garment(&binding.garment)
        .ok_or_else(|| Error::NoSuchGarment(binding.garment.clone()))?;
    let held = garment.variations.len();
    let rel = garment
        .variations
        .get(binding.texture)
        .and_then(|v| v.texture.clone())
        .ok_or_else(|| {
            Error::Refused(format!(
                "the garment has {held} texture(s) and none is the number {}",
                binding.texture + 1
            ))
        })?;
    let at = pack.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !stays_inside(&rel) || or_project::reference::is_reference(&rel) {
        return Err(Error::Refused(format!(
            "{rel} leads out of this pack, and a picture is put into a source of the pack"
        )));
    }
    if source_kind(&rel) != Some(textures::KIND) || !at.is_dir() {
        return Err(Error::Refused(format!(
            "{rel} is a compiled file and not a textures source: put a source in its place (Replace texture takes the same .ytd and makes one), and a picture can be put into that"
        )));
    }
    let source = Source::open(&at).map_err(|p| Error::Refused(p.to_string()))?;
    let held = textures::inputs_of(&source).map_err(|p| Error::Refused(join(&p)))?;
    // The diffuse of a variation is the first texture of its dictionary, in the order the compile writes them.
    let diffuse = held
        .iter()
        .find(|i| !is_map(&i.texture))
        .or_else(|| held.first())
        .map(|i| i.texture.clone())
        .unwrap_or_else(|| binding_name(&at));
    let texture = match binding.slot {
        MapSlot::Diffuse => diffuse,
        MapSlot::Specular => companion(&diffuse, Kind::Specular),
        MapSlot::Normal => companion(&diffuse, Kind::Normal),
    };
    // The compile refuses such a name, but a picture is put in before any compile runs.
    if let Some(bad) = unsafe_file_name(&texture) {
        return Err(Error::Refused(format!(
            "the source names a texture a picture cannot be put in under: {bad}"
        )));
    }
    Ok(Target {
        source: rel,
        texture,
    })
}

/// The name of an empty source, which its first picture takes.
fn binding_name(source: &Path) -> String {
    Source::open(source).map_or_else(|_| "texture".to_string(), |s| s.manifest.name)
}

/// Whether a texture's name says it is a map and not a diffuse.
fn is_map(name: &str) -> bool {
    name.split('_').any(|w| {
        ["normal", "spec", "bump", "n", "s"]
            .iter()
            .any(|m| w.eq_ignore_ascii_case(m))
    })
}

fn join(problems: &[or_source::Problem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Bind a folder to a slot; one that cannot hold a picture is refused with why, and a second binding of the same slot of the same texture replaces the first.
pub fn bind(pack: &Path, binding: Binding) -> Result<(), Error> {
    target(pack, &binding)?;
    let mut all = bindings(pack);
    all.retain(|b| {
        (b.garment.as_str(), b.texture, b.slot)
            != (binding.garment.as_str(), binding.texture, binding.slot)
    });
    all.push(binding);
    keep(pack, &all)
}

/// Let go of a binding.
pub fn unbind(pack: &Path, binding: &Binding) -> Result<(), Error> {
    let mut all = bindings(pack);
    all.retain(|b| b != binding);
    keep(pack, &all)
}

/// The newest picture the editor saved in the folder, none where it saved none.
pub fn newest(binding: &Binding) -> Option<PathBuf> {
    std::fs::read_dir(&binding.folder)
        .ok()?
        .flatten()
        .filter(|e| {
            e.path().is_file() && textures::is_texture_file(&e.file_name().to_string_lossy())
        })
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| !x.eq_ignore_ascii_case("dds"))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(at, _)| *at)
        .map(|(_, path)| path)
}

/// Put `file`, a picture the editor saved, into the slot `binding` stands for.
///
/// The source gets the picture under the slot's name and no other picture of that name, and a copy that fails leaves the picture that was
/// there; a map the source has not got yet is stated linear in its `textures.json`, and a picture of 16 bits a channel is given the
/// window that keeps its range, which a picture of 8 bits under the same name takes back.
pub fn deliver(pack: &Path, binding: &Binding, file: &Path) -> Result<Delivered, Error> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !textures::is_texture_file(&name) || extension == "dds" {
        return Err(Error::Refused(format!(
            "{name} is not a picture a texture is made from (PNG, TGA, TIFF, BMP, JPG, PSD, HDR or EXR)"
        )));
    }
    let Target { source, texture } = target(pack, binding)?;
    let dir = pack.join(source.replace('/', std::path::MAIN_SEPARATOR_STR));
    let folder = dir.join(DIR);
    let put = format!("{texture}.{extension}");
    let part = folder.join(format!(".{put}.part"));
    std::fs::copy(file, &part)
        .and_then(|_| std::fs::rename(&part, folder.join(&put)))
        .map_err(|e| {
            let _ = std::fs::remove_file(&part);
            Error::Io(file.display().to_string(), e)
        })?;
    // One picture of a name: the one before it, whatever its format, goes, and only once the new one is in.
    for old in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
        let other = old.file_name().to_string_lossy().into_owned();
        let stem = Path::new(&other)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if stem.eq_ignore_ascii_case(&texture)
            && other != put
            && !other.to_ascii_lowercase().ends_with(".dds")
            && textures::is_texture_file(&other)
        {
            let _ = std::fs::remove_file(old.path());
        }
    }
    state_rules(&dir, &texture, &put, binding.slot)?;
    Ok(Delivered {
        source,
        texture,
        file: put,
    })
}

/// A map the source does not hold yet is stated linear, and a picture of 16 bits a channel is given its window.
fn state_rules(dir: &Path, texture: &str, file: &str, slot: MapSlot) -> Result<(), Error> {
    let said = textures::state_depth_rules(dir, &[file.to_string()]).map_err(Error::Refused)?;
    let _ = said;
    if slot == MapSlot::Diffuse {
        return Ok(());
    }
    let text = std::fs::read_to_string(dir.join(RULES))
        .map_err(|e| Error::Io(dir.join(RULES).display().to_string(), e))?;
    let mut rules = serde_json::from_str::<textures::Rules>(&text)
        .map_err(|e| Error::Refused(format!("{RULES}: {e}")))?;
    if rules
        .textures
        .iter()
        .any(|e| e.name.eq_ignore_ascii_case(texture))
    {
        return Ok(());
    }
    rules.textures.push(Entry {
        name: texture.to_string(),
        source: None,
        pack: None,
        header: None,
        encode: Some(Encode {
            format: Some("DXT1".to_string()),
            colour_space: Some(ColourSpace::Linear),
            ..Encode::default()
        }),
    });
    let text = canonical(&rules).map_err(|e| Error::Refused(e.to_string()))?;
    std::fs::write(dir.join(RULES), text)
        .map_err(|e| Error::Io(dir.join(RULES).display().to_string(), e))
}
