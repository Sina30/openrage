//! Review mode: what differs between two versions of a pack, read-only, as the garments, textures and settings added, changed and removed.
//!
//! A version is `wardrobe.json` as a write left it ([`crate::backup`]). The review holds the two versions side by side and
//! changes neither, and a garment is followed by its stable id, so a rename, a renumbering or a move is a change of that
//! garment and not a removal and an addition. A texture is followed by its place among the garment's textures.

use std::path::Path;

use crate::backup::read_version;
use crate::changes::{garment_lines, lines_between, settings_lines};
use crate::edit::Error;
use crate::model::{Garment, Wardrobe};
use crate::rules::name_of;

/// How a garment or a texture differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Added,
    Changed,
    Removed,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Added => "added",
            Kind::Changed => "changed",
            Kind::Removed => "removed",
        }
    }
}

/// A texture of a garment that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// Its place among the garment's textures, from 1.
    pub number: usize,
    pub kind: Kind,
    /// The file it names, as the newer version names it, else the older.
    pub file: Option<String>,
    /// For a texture that changed, what differs.
    pub lines: Vec<String>,
}

/// A garment that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GarmentChange {
    pub id: String,
    /// What a list calls it, as the newer version has it, else the older.
    pub name: String,
    pub kind: Kind,
    /// For a garment that changed, what differs but its textures.
    pub lines: Vec<String>,
    pub textures: Vec<Texture>,
}

/// What differs between two versions of a pack.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Review {
    pub garments: Vec<GarmentChange>,
    /// What the pack states beyond its garments that differs, one line each.
    pub settings: Vec<String>,
}

fn texture_file(v: &crate::model::Variation) -> Option<String> {
    v.texture.clone()
}

fn textures_between(before: &Garment, after: &Garment) -> Vec<Texture> {
    let (x, y) = (&before.variations, &after.variations);
    (0..x.len().max(y.len()))
        .filter_map(|at| {
            let number = at + 1;
            match (x.get(at), y.get(at)) {
                (Some(p), Some(q)) => {
                    let lines = lines_between(p, q);
                    (!lines.is_empty()).then(|| Texture {
                        number,
                        kind: Kind::Changed,
                        file: texture_file(q).or_else(|| texture_file(p)),
                        lines,
                    })
                }
                (Some(p), None) => Some(Texture {
                    number,
                    kind: Kind::Removed,
                    file: texture_file(p),
                    lines: Vec::new(),
                }),
                (None, Some(q)) => Some(Texture {
                    number,
                    kind: Kind::Added,
                    file: texture_file(q),
                    lines: Vec::new(),
                }),
                (None, None) => None,
            }
        })
        .collect()
}

/// The garment without its textures, which are compared on their own.
fn without_textures(g: &Garment) -> Garment {
    let mut bare = g.clone();
    bare.variations.clear();
    bare
}

/// What differs between `before` and `after`.
pub fn between(before: &Wardrobe, after: &Wardrobe) -> Review {
    let mut review = Review::default();
    for now in &after.garments {
        match before.garment(&now.id) {
            None => review.garments.push(GarmentChange {
                id: now.id.clone(),
                name: name_of(now),
                kind: Kind::Added,
                lines: Vec::new(),
                textures: textures_between(&Garment::new(), now),
            }),
            Some(then) => {
                let lines = garment_lines(&without_textures(then), &without_textures(now));
                let textures = textures_between(then, now);
                if !lines.is_empty() || !textures.is_empty() {
                    review.garments.push(GarmentChange {
                        id: now.id.clone(),
                        name: name_of(now),
                        kind: Kind::Changed,
                        lines,
                        textures,
                    });
                }
            }
        }
    }
    for then in &before.garments {
        if after.garment(&then.id).is_none() {
            review.garments.push(GarmentChange {
                id: then.id.clone(),
                name: name_of(then),
                kind: Kind::Removed,
                lines: Vec::new(),
                textures: textures_between(then, &Garment::new()),
            });
        }
    }
    review.settings = settings_lines(before, after);
    review
}

/// The pack as version `n` held it, or as it is now where `n` is none.
fn side(folder: &Path, n: Option<u64>) -> Result<Wardrobe, Error> {
    match n {
        Some(n) => read_version(folder, n),
        None => Ok(Wardrobe::read(folder)?),
    }
}

/// What differs between version `from` of the pack in `folder` and version `to`; a side that is none is the pack as it is now.
pub fn of_versions(folder: &Path, from: Option<u64>, to: Option<u64>) -> Result<Review, Error> {
    Ok(between(&side(folder, from)?, &side(folder, to)?))
}

impl Review {
    /// Whether the two versions state the same.
    pub fn is_empty(&self) -> bool {
        self.garments.is_empty() && self.settings.is_empty()
    }

    fn garments_of(&self, kind: Kind) -> usize {
        self.garments.iter().filter(|g| g.kind == kind).count()
    }

    fn textures_of(&self, kind: Kind) -> usize {
        (self.garments.iter())
            .flat_map(|g| &g.textures)
            .filter(|t| t.kind == kind)
            .count()
    }

    /// The review in a sentence: how many garments, textures and settings were added, changed and removed.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "The two versions state the same".to_string();
        }
        let of = |what: &str, number: &dyn Fn(Kind) -> usize| {
            let parts: Vec<String> = [Kind::Added, Kind::Changed, Kind::Removed]
                .into_iter()
                .filter(|&k| number(k) > 0)
                .map(|k| format!("{} {}", number(k), k.word()))
                .collect();
            (!parts.is_empty()).then(|| format!("{what}: {}", parts.join(", ")))
        };
        let mut parts: Vec<String> = Vec::new();
        parts.extend(of("garments", &|k| self.garments_of(k)));
        parts.extend(of("textures", &|k| self.textures_of(k)));
        if !self.settings.is_empty() {
            parts.push(format!("settings: {} changed", self.settings.len()));
        }
        parts.join("; ")
    }

    /// The whole review as lines of text, for a report or a file: a heading per garment, its changes under it, then the settings.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for g in &self.garments {
            out.push(format!("{} {}", g.name, g.kind.word()));
            out.extend(g.lines.iter().map(|l| format!("  {l}")));
            for t in &g.textures {
                let file = t.file.as_deref().unwrap_or("empty texture");
                out.push(format!("  texture {} {}: {file}", t.number, t.kind.word()));
                out.extend(t.lines.iter().map(|l| format!("    {l}")));
            }
        }
        if !self.settings.is_empty() {
            out.push("Settings".to_string());
            out.extend(self.settings.iter().map(|l| format!("  {l}")));
        }
        out
    }
}
