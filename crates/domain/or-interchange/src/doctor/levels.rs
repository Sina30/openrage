//! A level of detail a drawable lacks, made from its highest.
//!
//! ⚠ **The level is the high level's own, cut down.** It has the high level's models, geometries, shaders and vertex
//! declarations, so it draws with the same materials and reads the same channels, and only each geometry's vertices and
//! triangles are fewer. The high level is not touched. A level is never given a material or a layout of its own.

use or_io::error::{IoError, Result};
use or_io::gta5::RmcDrawable;

use super::decimate::decimate;
use crate::extract::{drawable_scene, ExtractOptions, Extracted, LodSelection};
use crate::import::{replace_geometry_named, ImportNames, ImportOptions, Imported};
use crate::scene::LodSlot;

/// The names of the four levels of detail, by their number.
pub const LEVELS: [&str; 4] = ["High", "Medium", "Low", "Very low"];

/// What a level is asked to be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelRequest {
    /// 1 to 3: the level to make.
    pub level: usize,
    /// The share of the high level's triangles the level keeps, above 0 and no more than 1.
    pub ratio: f32,
    /// Make the level again where the drawable has one; off, a level that is there is refused.
    pub replace: bool,
}

/// One geometry of the level and what became of it.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryCut {
    /// `model0/geometry1`.
    pub at: String,
    pub triangles_before: usize,
    pub triangles_after: usize,
    pub target: usize,
    /// Whether it holds no more than its target.
    pub reached: bool,
}

/// What making a level did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LevelReport {
    pub level: usize,
    pub ratio: f32,
    pub high_triangles: usize,
    pub high_vertices: usize,
    pub triangles: usize,
    pub vertices: usize,
    pub cuts: Vec<GeometryCut>,
    /// The largest distance, in the drawable's units, a collapse put a surface off by.
    pub error: f32,
    /// Why no level was made, where the decimator could take so little out that a level of that cost is none.
    pub skipped: Option<String>,
}

impl LevelReport {
    /// How many geometries could not be cut as far as asked.
    pub fn unreached(&self) -> usize {
        self.cuts.iter().filter(|c| !c.reached).count()
    }

    /// The report as lines a person reads.
    pub fn lines(&self) -> Vec<String> {
        let share = if self.high_triangles == 0 {
            0.0
        } else {
            100.0 * self.triangles as f32 / self.high_triangles as f32
        };
        if let Some(why) = &self.skipped {
            return vec![why.clone()];
        }
        let mut lines = vec![format!(
            "{} level: {} triangles from {} ({share:.0} percent of the high level, {:.0} asked), {} vertices from {}",
            LEVELS[self.level.min(3)],
            self.triangles,
            self.high_triangles,
            100.0 * self.ratio,
            self.vertices,
            self.high_vertices,
        )];
        for cut in self.cuts.iter().filter(|c| !c.reached) {
            lines.push(format!(
                "{}: {} triangles, not the {} asked: no collapse that keeps its seams and borders was left",
                cut.at, cut.triangles_after, cut.target
            ));
        }
        lines
    }
}

fn refuse<T>(context: impl Into<String>) -> Result<T> {
    Err(IoError::InvalidData {
        context: context.into(),
    })
}

/// The levels from the second on that `drawable` holds no model for.
pub fn missing<T: or_io::gta5::DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<usize> {
    (1..4)
        .filter(|level| drawable.lod_group.lods[*level].models.is_empty())
        .collect()
}

/// The levels from the second on that are copies of the highest: each draws at least `share` of the vertices the high level does.
///
/// ⚠ A copy adds its whole cost and saves none: a hair of 225,104 triangles whose three levels all drew about 393,000 vertices was
/// three times the data, needed a heap of more than 64 MiB (so a page of 128 MiB, which the game refuses) and drew no faster
/// from afar. Removing the copies (`RmcDrawable::remove_lods`) leaves the high level, which every level was.
pub fn copies<T: Extracted>(drawable: &RmcDrawable<T>, share: f32) -> Vec<usize> {
    let scene = drawable_scene(
        &drawable.name,
        drawable,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    let mut vertices = [0usize; 4];
    for primitive in scene.primitives() {
        if let LodSlot::Level(level) = primitive.lod {
            if let Some(count) = vertices.get_mut(level as usize) {
                *count += primitive.positions.len();
            }
        }
    }
    (1..4)
        .filter(|l| vertices[*l] > 0 && vertices[*l] as f32 >= share * vertices[0] as f32)
        .collect()
}

/// `drawable` with level `request.level` made from its highest, on a clone.
///
/// Refused, naming why: no high level, a level there already unless the request replaces it, a ratio outside (0, 1], and a
/// drawable whose levels switch at distances of its own, since where the new level takes over is then a choice (a ped
/// component's levels are chosen by the ped, and every distance of one is 9998).
pub fn add_level<T: Imported>(
    drawable: &RmcDrawable<T>,
    request: &LevelRequest,
) -> Result<(RmcDrawable<T>, LevelReport)> {
    let level = request.level;
    if !(1..4).contains(&level) {
        return refuse(format!(
            "level {level} is not one of the 1 to 3 made from the high level"
        ));
    }
    if !(request.ratio > 0.0 && request.ratio <= 1.0) {
        return refuse(format!(
            "a ratio of {} is not above 0 and at most 1",
            request.ratio
        ));
    }
    if drawable.lod_group.lods[0].models.is_empty() {
        return refuse("the drawable has no high level to make another from");
    }
    if !drawable.lod_group.lods[level].models.is_empty() && !request.replace {
        return refuse(format!(
            "the drawable has a {} level already",
            LEVELS[level].to_ascii_lowercase()
        ));
    }
    if let Some(odd) = (drawable.lod_group.lod_distances.iter()).find(|d| **d != 9998.0) {
        return refuse(format!(
            "the drawable's levels switch at distances of its own ({odd}), so where a new level takes \
             over is a choice this does not make"
        ));
    }

    let scene = drawable_scene(
        &drawable.name,
        drawable,
        &ExtractOptions {
            lods: LodSelection::Highest,
            textures: false,
        },
    );
    let mut brought = scene.clone();
    let mut report = LevelReport {
        level,
        ratio: request.ratio,
        ..LevelReport::default()
    };
    let mut worst = 0.0f32;
    for mesh in brought.objects.iter_mut().flat_map(|o| o.meshes.iter_mut()) {
        mesh.lod = LodSlot::Level(level as u8);
        mesh.name = format!(
            "{}_model{}",
            mesh.lod.label(),
            mesh.name.rsplit("_model").next().unwrap_or("0")
        );
        for primitive in &mut mesh.primitives {
            let cut = decimate(primitive, request.ratio);
            report.high_triangles += cut.triangles_before;
            report.high_vertices += cut.vertices_before;
            report.triangles += cut.triangles_after;
            report.vertices += cut.vertices_after;
            worst = worst.max(cut.error);
            report.cuts.push(GeometryCut {
                at: format!("model{}/geometry{}", primitive.model, primitive.geometry),
                triangles_before: cut.triangles_before,
                triangles_after: cut.triangles_after,
                target: cut.target,
                reached: cut.reached,
            });
            let (model, geometry) = (primitive.model, primitive.geometry);
            *primitive = cut.primitive;
            primitive.lod = LodSlot::Level(level as u8);
            primitive.model = model;
            primitive.geometry = geometry;
        }
    }
    report.error = worst;

    // A level that costs nearly what the High one does is no level: where the decimator took out less than a quarter of what was
    // asked, as it does of a strip of hair cards, none is made, and the report says why.
    let asked = 1.0 - request.ratio;
    let taken = 1.0 - report.triangles as f32 / report.high_triangles.max(1) as f32;
    if asked > 0.0 && taken < asked / 4.0 {
        report.skipped = Some(format!(
            "the {} level was not made: {:.0} percent of the High level's triangles could be taken out of {:.0} asked, since no collapse that keeps its seams and borders was left",
            LEVELS[level].to_ascii_lowercase(),
            taken * 100.0,
            asked * 100.0
        ));
        return Ok((drawable.clone(), report));
    }

    let mut built = drawable.clone();
    let high = (*built.lod_group.lods[0]).clone();
    *built.lod_group.lods[level] = high;
    if built.lod_group.bucket_masks[level] == 0 || request.replace {
        built.lod_group.bucket_masks[level] = built.lod_group.bucket_masks[0];
    }
    let options = ImportOptions {
        fill_missing: false,
        keep_skinning: true,
    };
    let (made, _) = replace_geometry_named(&built, &brought, &options, &ImportNames::default())?;
    Ok((made, report))
}
