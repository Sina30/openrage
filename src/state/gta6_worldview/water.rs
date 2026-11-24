//! The water of the GTA VI world: the level's quads, read once on a worker and held, and the mesh
//! of those around the camera.
//!
//! ⚠ The quads are the sea, the lakes and the rivers as level rectangles. The level's streamed
//! height map of water is not read, and a quad a corner is cut from is left out of the mesh.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use glam::Vec3;
use or_io::container::rpf::read_rpf9_body;
use or_io::gta6::water::{WaterData, WaterQuad};
use or_renderer::MeshVertex;

/// The archive of an install the water document is in, and the document's path in it.
const ARCHIVE: &str = "levels.rpf";
const ENTRY: &str = "levels/americas/water";
/// The bytes the water document is read to.
const BODY_LIMIT: u64 = 16 << 20;

type Read = Arc<OnceLock<Option<Arc<WaterData>>>>;

static HELD: Mutex<Vec<(PathBuf, Read)>> = Mutex::new(Vec::new());

/// The water document at `offset` of the GTA VI archive at `archive`.
pub fn read(archive: &Path, offset: u64) -> Result<WaterData, String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let (_, body) = read_rpf9_body(&file, offset, BODY_LIMIT).map_err(|e| e.to_string())?;
    WaterData::open(&body).map_err(|e| e.to_string())
}

/// The water of the install at `root`, read once on a worker and held: `None` until it is read, and where the install's table gives no such document or it does not read.
pub fn water(root: &Path) -> Option<Arc<WaterData>> {
    let archive = root.join(ARCHIVE);
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    let slot = match held.iter().find(|(path, _)| *path == archive) {
        Some((_, slot)) => Arc::clone(slot),
        None => {
            let slot = Read::default();
            held.push((archive.clone(), Arc::clone(&slot)));
            let filled = Arc::clone(&slot);
            std::thread::spawn(move || {
                let found = or_title::gta6_mount::table::entry_offset(&archive, ENTRY);
                let water = match found.map(|offset| read(&archive, offset)) {
                    Some(Ok(water)) => Some(Arc::new(water)),
                    Some(Err(why)) => {
                        tracing::warn!(archive = %archive.display(), %why, "GTA VI's water is not read");
                        None
                    }
                    None => {
                        tracing::info!(archive = %archive.display(), "the archive's table names no water document");
                        None
                    }
                };
                let _ = filled.set(water);
            });
            slot
        }
    };
    slot.get()?.clone()
}

/// The mesh of the whole quads of `quads` that are drawn and lie within `reach` metres of `eye`: four corners a quad, in the world, the surface up, the texture coordinate the place in metres and the opacity in the colour.
pub fn mesh(quads: &[WaterQuad], eye: Vec3, reach: f32) -> (Vec<MeshVertex>, Vec<u32>) {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for quad in quads.iter().filter(|q| q.kind == 0 && !q.invisible) {
        let (min, max) = (quad.min.map(f32::from), quad.max.map(f32::from));
        let nearest = Vec3::new(
            eye.x.clamp(min[0], max[0]),
            eye.y.clamp(min[1], max[1]),
            eye.z,
        );
        if nearest.distance(eye) > reach || min[0] >= max[0] || min[1] >= max[1] {
            continue;
        }
        let first = vertices.len() as u32;
        // ponytail: one opacity a quad, its first corner's; which corner each of the four is is not measured.
        let colour = [1.0, 1.0, 1.0, f32::from(quad.alpha[0]) / 255.0];
        for (x, y) in [
            (min[0], min[1]),
            (max[0], min[1]),
            (max[0], max[1]),
            (min[0], max[1]),
        ] {
            vertices.push(MeshVertex::new_coloured(
                [x, y, quad.z],
                [0.0, 0.0, 1.0],
                [x, y],
                colour,
            ));
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(min: [i16; 2], max: [i16; 2], z: f32) -> WaterQuad {
        WaterQuad {
            min,
            max,
            alpha: [51, 0, 0, 0],
            z,
            kind: 0,
            ocean: true,
            invisible: false,
            limited_depth: false,
            no_stencil: false,
        }
    }

    #[test]
    fn a_whole_quad_in_reach_is_two_triangles_facing_up_at_its_height() {
        let (vertices, indices) = mesh(&[quad([-10, -20], [30, 40], 0.5)], Vec3::ZERO, 100.0);
        assert_eq!((vertices.len(), indices), (4, vec![0, 1, 2, 0, 2, 3]));
        assert_eq!(vertices[0].position, [-10.0, -20.0, 0.5]);
        assert_eq!(vertices[2].position, [30.0, 40.0, 0.5]);
        assert!(vertices.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
        assert_eq!(vertices[2].uv, [30.0, 40.0]);
        assert_eq!(vertices[0].colour, [1.0, 1.0, 1.0, 0.2]);
        // Counter-clockwise from above, so that the surface faces up.
        let at = |k: usize| Vec3::from(vertices[k].position);
        assert!((at(1) - at(0)).cross(at(2) - at(0)).z > 0.0);
    }

    #[test]
    fn a_quad_out_of_reach_a_hidden_one_and_one_a_corner_is_cut_from_are_left_out() {
        let far = quad([1000, 1000], [1100, 1100], 0.0);
        let hidden = WaterQuad {
            invisible: true,
            ..quad([0, 0], [1, 1], 0.0)
        };
        let cut = WaterQuad {
            kind: 3,
            ..quad([0, 0], [1, 1], 0.0)
        };
        let near = quad([50, 0], [60, 10], 0.0);
        let (vertices, indices) = mesh(&[far, hidden, cut, near], Vec3::ZERO, 100.0);
        assert_eq!((vertices.len(), indices.len()), (4, 6));
        assert_eq!(vertices[0].position, [50.0, 0.0, 0.0]);
        // A camera over a quad is at no distance from it, however high the reach is counted from.
        assert_eq!(mesh(&[far], Vec3::new(1050.0, 1050.0, 0.0), 1.0).0.len(), 4);
    }

    #[test]
    fn a_document_that_is_not_there_is_an_error_and_not_a_panic() {
        assert!(read(Path::new("no such archive of water"), 0).is_err());
    }
}
