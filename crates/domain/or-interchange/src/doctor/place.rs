//! A mesh moved from one frame to another: a ped component (authored in the ped's frame, where the head is at (0, 0.003, 0.642)) becomes a prop (authored in the frame of the bone it hangs from).
//!
//! ⚠ The move is rigid: every position is carried to `R^T (p - origin)` where `origin` is where the bone stands and the columns of `R` are the bone's axes in the world, normals and the first three lanes of a tangent are turned by
//! `R^T`, and the boxes of the drawable follow. The triangles keep their winding (a rotation does not turn them over).

use or_io::error::Result;
use or_io::gta5::RmcDrawable;

use super::through_scene;
use crate::import::Imported;
use crate::scene::Scene;

/// A frame: where it stands and which way its axes point (`axes[row][column]`, the columns being the axes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: [f32; 3],
    pub axes: [[f32; 3]; 3],
}

impl Frame {
    /// `R^T v`: a direction carried into the frame.
    fn direction(&self, v: [f32; 3]) -> [f32; 3] {
        let a = &self.axes;
        [
            a[0][0] * v[0] + a[1][0] * v[1] + a[2][0] * v[2],
            a[0][1] * v[0] + a[1][1] * v[1] + a[2][1] * v[2],
            a[0][2] * v[0] + a[1][2] * v[1] + a[2][2] * v[2],
        ]
    }

    /// A point carried into the frame.
    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        self.direction([
            p[0] - self.origin[0],
            p[1] - self.origin[1],
            p[2] - self.origin[2],
        ])
    }
}

/// What a placement did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaceReport {
    pub vertices: usize,
    /// The box of everything, in the new frame.
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// Every mesh of `scene` carried into `frame`.
pub fn place_scene(scene: &mut Scene, frame: &Frame) -> Result<PlaceReport> {
    let mut report = PlaceReport {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
        ..PlaceReport::default()
    };
    for object in &mut scene.objects {
        for primitive in object
            .meshes
            .iter_mut()
            .flat_map(|m| m.primitives.iter_mut())
        {
            for p in &mut primitive.positions {
                *p = frame.point(*p);
                for k in 0..3 {
                    report.min[k] = report.min[k].min(p[k]);
                    report.max[k] = report.max[k].max(p[k]);
                }
            }
            report.vertices += primitive.positions.len();
            for n in &mut primitive.normals {
                *n = frame.direction(*n);
            }
            for t in &mut primitive.tangents {
                let v = frame.direction([t[0], t[1], t[2]]);
                *t = [v[0], v[1], v[2], t[3]];
            }
        }
    }
    Ok(report)
}

/// `drawable` carried into `frame` on a clone, every level of detail, with its bounding box and sphere.
pub fn place_drawable<T: Imported>(
    drawable: &RmcDrawable<T>,
    frame: &Frame,
) -> Result<(RmcDrawable<T>, PlaceReport)> {
    let (mut made, report) = through_scene(drawable, None, |scene| place_scene(scene, frame))?;
    if report.vertices > 0 {
        let b = &mut made.lod_group.bounds_box;
        (b.min.x, b.min.y, b.min.z) = (report.min[0], report.min[1], report.min[2]);
        (b.max.x, b.max.y, b.max.z) = (report.max[0], report.max[1], report.max[2]);
        let centre = [
            (report.min[0] + report.max[0]) / 2.0,
            (report.min[1] + report.max[1]) / 2.0,
            (report.min[2] + report.max[2]) / 2.0,
        ];
        let radius = (0..3)
            .map(|k| ((report.max[k] - report.min[k]) / 2.0).powi(2))
            .sum::<f32>()
            .sqrt();
        let s = &mut made.lod_group.bounds_sphere.center_and_radius;
        (s.x, s.y, s.z, s.w) = (centre[0], centre[1], centre[2], radius);
    }
    Ok((made, report))
}
