//! What the painter reads of a cloth and changes in it, as plain functions on the owned value.
//!
//! The painter edits what a level stores. A per-vertex run a cloth leaves empty is not created: the
//! engine reads an absent run as its own default, which nothing here measured, so a stored value
//! and an invented one would look alike in the file.

use or_io::gta5::{CharacterCloth, CharacterClothController, PgDictionary, PhVerletCloth};

pub type Dictionary = PgDictionary<CharacterCloth>;

/// How many simulated levels a cloth's controller holds.
pub const LODS: usize = 4;

/// The cloth at `entry`, if the dictionary holds one there.
pub fn cloth(dict: &Dictionary, entry: usize) -> Option<&CharacterCloth> {
    dict.entries.get(entry)?.as_deref()
}

fn controller(cloth: &CharacterCloth) -> Option<&CharacterClothController> {
    cloth.cloth_controller.as_deref()
}

/// One simulated level of a cloth.
pub fn level(cloth: &CharacterCloth, lod: usize) -> Option<&PhVerletCloth> {
    controller(cloth)?.base.cloth().get(lod).copied()?.as_deref()
}

/// The positions of a level's simulated vertices, which the viewport draws the cloth's mesh from.
pub fn positions(cloth: &CharacterCloth, lod: usize) -> Vec<[f32; 3]> {
    level(cloth, lod).map_or_else(Vec::new, |level| {
        let verts = &level.cloth_data.vertex_positions;
        verts.iter().map(|v| [v.x, v.y, v.z]).collect()
    })
}

/// A level's per-vertex runs as stored; a run the cloth leaves empty is `None`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Runs<'a> {
    pub weight: Option<&'a [f32]>,
    pub inflation: Option<&'a [f32]>,
    pub pin_radius: Option<&'a [f32]>,
}

pub fn runs(cloth: &CharacterCloth, lod: usize) -> Runs<'_> {
    let Some(bridge) = controller(cloth).and_then(|c| c.base.bridge.as_deref()) else {
        return Runs::default();
    };
    fn stored(run: Option<&Vec<f32>>) -> Option<&[f32]> {
        run.map(Vec::as_slice).filter(|run| !run.is_empty())
    }
    Runs {
        weight: stored(bridge.vertex_weight(lod)),
        inflation: stored(bridge.inflation_scale(lod)),
        pin_radius: stored(bridge.pin_radius(lod)),
    }
}

/// Whether the simulation may pin vertex `vertex`.
pub fn pinnable(cloth: &CharacterCloth, vertex: usize) -> bool {
    controller(cloth)
        .and_then(|c| c.base.bridge.as_deref())
        .is_some_and(|b| b.is_pinnable(vertex))
}

/// The smallest and largest value of a run, none for an empty one.
pub fn range(run: &[f32]) -> Option<(f32, f32)> {
    let mut finite = run.iter().copied().filter(|v| v.is_finite());
    let first = finite.next()?;
    Some(finite.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
}

/// What one paint stroke sets on the selected vertices of a level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    Weight(f32),
    Inflation(f32),
    /// The first set of a level's pin radii, which is the run `pin_radius(lod, vertex)` reads.
    PinRadius(f32),
    Pinnable(bool),
    /// The weight runs from `from` at the selection's low end of `axis` to `to` at its high end.
    WeightGradient { axis: usize, from: f32, to: f32 },
}

/// Paint `selection` of level `lod` and say how many vertices changed, or why none could.
pub fn paint(
    cloth: &mut CharacterCloth,
    lod: usize,
    selection: &[bool],
    paint: Paint,
) -> Result<usize, String> {
    let at = positions(cloth, lod);
    if at.is_empty() {
        return Err(format!("level {lod} holds no vertices"));
    }
    let chosen: Vec<usize> = (0..at.len())
        .filter(|v| selection.get(*v).copied().unwrap_or(false))
        .collect();
    if chosen.is_empty() {
        return Err("no vertex is selected".to_string());
    }
    let bridge = cloth
        .cloth_controller
        .as_deref_mut()
        .and_then(|c| c.base.bridge.as_deref_mut())
        .ok_or("the cloth has no bridge, which holds the per-vertex values")?;
    let set = |run: Option<&mut Vec<f32>>, what: &str, value: &dyn Fn(usize) -> f32| {
        let run = run
            .filter(|run| !run.is_empty())
            .ok_or_else(|| format!("level {lod} stores no {what}, and the painter edits what is stored"))?;
        let mut changed = 0;
        let held = run.len();
        for &v in chosen.iter().filter(|v| **v < held) {
            run[v] = value(v);
            changed += 1;
        }
        Ok::<usize, String>(changed)
    };
    match paint {
        Paint::Weight(w) => set(bridge.vertex_weight_mut(lod), "vertex weights", &|_| w),
        Paint::Inflation(s) => set(bridge.inflation_scale_mut(lod), "inflation scales", &|_| s),
        Paint::PinRadius(r) => set(bridge.pin_radius_mut(lod), "pin radii", &|_| r),
        Paint::WeightGradient { axis, from, to } => {
            let axis = axis.min(2);
            let (lo, hi) = range(&chosen.iter().map(|v| at[*v][axis]).collect::<Vec<_>>())
                .ok_or("the selection has no finite position")?;
            let along = |v: usize| {
                let t = if hi > lo { (at[v][axis] - lo) / (hi - lo) } else { 0.0 };
                from + (to - from) * t
            };
            set(bridge.vertex_weight_mut(lod), "vertex weights", &along)
        }
        Paint::Pinnable(on) => {
            let list = &mut bridge.pinnable_list;
            let mut changed = 0;
            for &v in &chosen {
                let (word, bit) = (v / 32, 1u32 << (v % 32));
                if v >= usize::from(list.bit_size) || word >= list.bits.len() {
                    continue;
                }
                if on {
                    list.bits[word] |= bit;
                } else {
                    list.bits[word] &= !bit;
                }
                changed += 1;
            }
            if changed == 0 {
                return Err("the pinnable list holds none of the selected vertices".to_string());
            }
            Ok(changed)
        }
    }
}

/// Which vertices lie in the box whose corners are `lo` and `hi`, each a fraction of the level's extent.
pub fn in_region(at: &[[f32; 3]], lo: [f32; 3], hi: [f32; 3]) -> Vec<bool> {
    let axis = |a: usize| range(&at.iter().map(|p| p[a]).collect::<Vec<_>>());
    let extent: Vec<Option<(f32, f32)>> = (0..3).map(axis).collect();
    at.iter()
        .map(|p| {
            (0..3).all(|a| match extent[a] {
                Some((min, max)) => {
                    let span = max - min;
                    let (from, to) = (min + lo[a] * span, min + hi[a] * span);
                    // ⚠ A flat axis holds every vertex at its one value, whatever the fractions say.
                    span <= 0.0 || (p[a] >= from - f32::EPSILON && p[a] <= to + f32::EPSILON)
                }
                None => false,
            })
        })
        .collect()
}

/// `0-20, 25` as the vertices it names, or the first word that is no index.
pub fn indices(text: &str, count: usize) -> Result<Vec<usize>, String> {
    let mut found = Vec::new();
    for word in text.split([',', ' ']).filter(|w| !w.is_empty()) {
        let bad = || format!("`{word}` is not an index or a range such as 3-9");
        let (from, to) = match word.split_once('-') {
            Some((a, b)) => (a.parse::<usize>().map_err(|_| bad())?, b.parse::<usize>().map_err(|_| bad())?),
            None => {
                let one = word.parse::<usize>().map_err(|_| bad())?;
                (one, one)
            }
        };
        if from > to || to >= count {
            return Err(format!("`{word}` is outside the {count} vertices of the level"));
        }
        found.extend(from..=to);
    }
    Ok(found)
}
