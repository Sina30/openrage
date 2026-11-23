//! Normal and specular maps made from a diffuse texture, written as new pictures of a textures source.
//!
//! ⚠ **Nothing is written over what is there.** A map is a new picture beside the diffuse, with an entry of its own in
//! `textures.json` that states it linear, and a map the source already has is left and said so. [`plan`] reads names and writes
//! nothing; [`apply`] writes into the folder it is given, so a caller that must keep the folder as it was runs it on a copy.
//!
//! ⚠ **The maps are estimates.** A picture holds colour and not shape, so the normal map reads the brightness of the diffuse as
//! a height and the specular map reads it as a measure of shine; an artist's own maps are better and replace these by name.

use std::path::Path;

use or_io::codec::texture::{save_png, RgbaImage};
use serde::Serialize;

use crate::asset::Source;
use crate::json::canonical;
use crate::textures::{
    inputs_of, is_texture_file, ColourSpace, Encode, Entry, Inputs, Rules, DIR, KIND, RULES,
};
use crate::Problem;

/// The format a new map is stored in: opaque, and half the memory of a map with alpha.
const FORMAT: &str = "DXT1";

/// A kind of map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Normal,
    Specular,
}

impl Kind {
    /// What a person calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal map",
            Self::Specular => "specular map",
        }
    }

    /// The word the game's own names carry for it.
    fn word(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Specular => "spec",
        }
    }

    /// The suffix a name with no diffuse word in it takes.
    fn suffix(self) -> &'static str {
        match self {
            Self::Normal => "_n",
            Self::Specular => "_s",
        }
    }
}

/// What may be made.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub normal: bool,
    pub specular: bool,
    /// How steeply the brightness of the diffuse tilts the normal: the tilt of a texel is this many times its slope in brightness.
    pub strength: f32,
    /// The textures to make maps of, by name, whatever their names say. With none, every texture whose name has the word `diff`.
    pub only: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            normal: true,
            specular: true,
            strength: 2.0,
            only: Vec::new(),
        }
    }
}

impl Options {
    fn kinds(&self) -> Vec<Kind> {
        let mut kinds = Vec::new();
        if self.normal {
            kinds.push(Kind::Normal);
        }
        if self.specular {
            kinds.push(Kind::Specular);
        }
        kinds
    }
}

/// A map to be made, or made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Made {
    pub kind: Kind,
    /// The name of the new texture.
    pub texture: String,
    /// The picture that holds it, inside the source's `textures/` folder.
    pub file: String,
}

/// The maps made from one diffuse texture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Generated {
    pub diffuse: String,
    pub maps: Vec<Made>,
}

/// A texture, or one map, that was not made, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Left {
    pub texture: String,
    pub why: String,
}

/// What a request makes of a source, and what it leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub generated: Vec<Generated>,
    pub left: Vec<Left>,
}

impl Plan {
    /// How many maps are made.
    pub fn count(&self) -> usize {
        self.generated.iter().map(|g| g.maps.len()).sum()
    }
}

/// The name of the map of `kind` for the texture `diffuse`: the `diff` word of a game name becomes `normal` or `spec`, and a name
/// without one takes `_n` or `_s`.
pub fn companion(diffuse: &str, kind: Kind) -> String {
    let words: Vec<&str> = diffuse.split('_').collect();
    match words.iter().position(|w| w.eq_ignore_ascii_case("diff")) {
        Some(at) => words
            .iter()
            .enumerate()
            .map(|(i, w)| if i == at { kind.word() } else { w })
            .collect::<Vec<_>>()
            .join("_"),
        None => format!("{diffuse}{}", kind.suffix()),
    }
}

/// Whether the name of a texture says it is a diffuse.
fn is_diffuse(name: &str) -> bool {
    name.split('_').any(|w| w.eq_ignore_ascii_case("diff"))
}

/// The textures of the source at `dir` that maps are made of, and the maps that would be made; nothing is written.
pub fn plan(dir: &Path, options: &Options) -> Result<Plan, Vec<Problem>> {
    let source = Source::open(dir).map_err(|p| vec![p])?;
    if source.manifest.kind != KIND {
        return Err(vec![Problem::new(format!(
            "this is a {} source, and maps are made in a textures source",
            source.manifest.kind
        ))]);
    }
    let held = inputs_of(&source)?;
    let named = |name: &str| held.iter().any(|i| i.texture.eq_ignore_ascii_case(name));
    let kinds = options.kinds();
    let mut plan = Plan::default();
    for input in &held {
        let chosen = options
            .only
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&input.texture));
        if !(chosen || options.only.is_empty() && is_diffuse(&input.texture)) {
            if options.only.is_empty() && !already_a_map(&input.texture) {
                plan.left.push(Left {
                    texture: input.texture.clone(),
                    why: "its name has no diff word, so it is not taken for a diffuse; name it in the request to make maps of it".to_string(),
                });
            }
            continue;
        }
        let mut maps = Vec::new();
        for kind in &kinds {
            let texture = companion(&input.texture, *kind);
            if named(&texture) {
                plan.left.push(Left {
                    texture: input.texture.clone(),
                    why: format!(
                        "a {} of its name, {texture}, is already in the source",
                        kind.label()
                    ),
                });
                continue;
            }
            maps.push(Made {
                kind: *kind,
                file: format!("{texture}.png"),
                texture,
            });
        }
        if !maps.is_empty() {
            plan.generated.push(Generated {
                diffuse: input.texture.clone(),
                maps,
            });
        }
    }
    for name in &options.only {
        if !named(name) {
            plan.left.push(Left {
                texture: name.clone(),
                why: "it is not a texture of the source".to_string(),
            });
        }
    }
    Ok(plan)
}

/// Whether a name is one a map of ours would carry, so that the textures a source already holds as maps are not listed as left.
fn already_a_map(name: &str) -> bool {
    name.split('_').any(|w| {
        ["normal", "spec", "bump", "n", "s"]
            .iter()
            .any(|m| w.eq_ignore_ascii_case(m))
    })
}

/// Make what [`plan`] says in the source at `dir`; `stop` is asked between textures and ends it with a [`Problem`], having written nothing.
pub fn apply(dir: &Path, options: &Options, stop: &dyn Fn() -> bool) -> Result<Plan, Vec<Problem>> {
    let plan = plan(dir, options)?;
    if plan.generated.is_empty() {
        return Ok(plan);
    }
    let source = Source::open(dir).map_err(|p| vec![p])?;
    let inputs = Inputs::read(&source)?;
    let mut made: Vec<(String, RgbaImage)> = Vec::new();
    for one in &plan.generated {
        if stop() {
            return Err(vec![Problem::new("stopped before it was done")]);
        }
        let pixels = inputs
            .pixels_of(&one.diffuse)
            .map_err(|why| vec![Problem::new(why).of_texture(&one.diffuse)])?;
        for map in &one.maps {
            let image = match map.kind {
                Kind::Normal => normal_from(&pixels, options.strength),
                Kind::Specular => specular_from(&pixels),
            };
            made.push((map.file.clone(), image));
        }
    }
    let rules_text = std::fs::read_to_string(dir.join(RULES))
        .map_err(|e| vec![Problem::new(format!("cannot read: {e}")).in_file(RULES)])?;
    let mut rules = serde_json::from_str::<Rules>(&rules_text)
        .map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;
    for map in plan.generated.iter().flat_map(|g| &g.maps) {
        rules.textures.push(Entry {
            name: map.texture.clone(),
            source: None,
            pack: None,
            header: None,
            encode: Some(Encode {
                format: Some(FORMAT.to_string()),
                colour_space: Some(ColourSpace::Linear),
                ..Encode::default()
            }),
        });
    }
    let text = canonical(&rules).map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;
    // The pictures first, so that a failure while writing them leaves the rules saying nothing of a picture that is not there.
    let mut written = Vec::new();
    for (file, image) in &made {
        let path = dir.join(DIR).join(file);
        if is_texture_file(file) && path.exists() {
            remove(&written);
            return Err(vec![Problem::new(
                "is there already and is not written over",
            )
            .in_file(format!("{DIR}/{file}"))]);
        }
        if let Err(e) = save_png(&path, image) {
            remove(&written);
            return Err(vec![
                Problem::new(format!("cannot write: {e}")).in_file(format!("{DIR}/{file}"))
            ]);
        }
        written.push(path);
    }
    if let Err(e) = std::fs::write(dir.join(RULES), text) {
        remove(&written);
        return Err(vec![
            Problem::new(format!("cannot write: {e}")).in_file(RULES)
        ]);
    }
    Ok(plan)
}

fn remove(paths: &[std::path::PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

/// The luminance of a texel, 0 to 1.
fn height_of(texel: &[u8]) -> f32 {
    (0.2126 * f32::from(texel[0]) + 0.7152 * f32::from(texel[1]) + 0.0722 * f32::from(texel[2]))
        / 255.0
}

/// A normal map of `image`, reading its brightness as a height: green points down the picture, as the game's own maps do.
///
/// The picture wraps at its edges, since a texture that repeats has no edge.
pub fn normal_from(image: &RgbaImage, strength: f32) -> RgbaImage {
    let (w, h) = (image.width as i64, image.height as i64);
    let height: Vec<f32> = image
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|t| height_of(t))
        .collect();
    let at = |x: i64, y: i64| height[(y.rem_euclid(h) * w + x.rem_euclid(w)) as usize];
    let mut out = RgbaImage::new(image.width, image.height);
    for y in 0..h {
        for x in 0..w {
            // The Sobel gradient over eight, which is the slope of the brightness across one texel.
            let gx = (at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x - 1, y)
                - at(x - 1, y + 1))
                / 8.0;
            let gy = (at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x, y - 1)
                - at(x + 1, y - 1))
                / 8.0;
            let (nx, ny, nz) = (-gx * strength, -gy * strength, 1.0f32);
            let length = (nx * nx + ny * ny + nz * nz).sqrt();
            let byte = |v: f32| ((v / length * 0.5 + 0.5) * 255.0 + 0.5) as u8;
            out.set_pixel(x as u32, y as u32, [byte(nx), byte(ny), byte(nz), 255]);
        }
    }
    out
}

/// A specular map of `image`: the shine rises with the brightness of the diffuse, up to 60 percent; the falloff and the Fresnel
/// term, which a picture says nothing of, are the same everywhere.
pub fn specular_from(image: &RgbaImage) -> RgbaImage {
    let mut out = RgbaImage::new(image.width, image.height);
    for (i, texel) in image.data.as_chunks::<4>().0.iter().enumerate() {
        let shine = (height_of(texel) * 0.6 * 255.0 + 0.5) as u8;
        out.set_pixel(
            i as u32 % image.width,
            i as u32 / image.width,
            [shine, 128, 64, 255],
        );
    }
    out
}
