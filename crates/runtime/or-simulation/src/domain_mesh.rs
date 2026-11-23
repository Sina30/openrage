//! The surface of a model a mesh domain emits from: its triangles, each picked by its share of
//! the surface's area.

use glam::Vec3;

/// A model's triangles in the frame of the domain that names it.
#[derive(Debug)]
pub struct DomainMesh {
    triangles: Vec<[Vec3; 3]>,
    /// Each triangle's area added to the areas of those before it.
    areas: Vec<f32>,
}

impl DomainMesh {
    /// `triangles` as a surface to emit from; `None` where they cover no finite area.
    pub fn new(triangles: Vec<[Vec3; 3]>) -> Option<Self> {
        let mut total = 0.0_f32;
        let areas: Vec<f32> = (triangles.iter())
            .map(|[a, b, c]| {
                total += 0.5 * (*b - *a).cross(*c - *a).length();
                total
            })
            .collect();
        (total.is_finite() && total > 0.0).then_some(Self { triangles, areas })
    }

    /// The point of the surface `uvw` picks: `u` the triangle by area, `v` and `w` the place on it.
    pub fn sample(&self, [u, v, w]: [f32; 3]) -> Vec3 {
        let total = self.areas.last().copied().unwrap_or_default();
        let picked = self.areas.partition_point(|&area| area < u * total);
        let last = self.triangles.len().saturating_sub(1);
        let Some([a, b, c]) = self.triangles.get(picked.min(last)).copied() else {
            return Vec3::ZERO;
        };
        // A point past the triangle's long side is folded back onto it.
        let (v, w) = match v + w > 1.0 {
            true => (1.0 - v, 1.0 - w),
            false => (v, w),
        };
        a + (b - a) * v + (c - a) * w
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::Mat4;

    use super::*;
    use crate::domain_sample::DomainInst;

    /// Whether `p` lies on the triangle `t`, within a small distance.
    fn on(t: [Vec3; 3], p: Vec3) -> bool {
        let [a, b, c] = t;
        let n = (b - a).cross(c - a);
        let inside = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|(from, to)| (*to - *from).cross(p - *from).dot(n) >= -1e-4);
        inside && (p - a).dot(n.normalize()).abs() < 1e-4
    }

    #[test]
    fn every_sample_lies_on_a_triangle_picked_by_its_share_of_the_area() {
        // A triangle of area 3 and one of area 1, apart.
        let big = [
            Vec3::ZERO,
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        ];
        let small = [
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(12.0, 0.0, 0.0),
            Vec3::new(10.0, 1.0, 0.0),
        ];
        let mesh = DomainMesh::new(vec![big, small]).expect("a surface");
        let steps = 8;
        for i in 0..=steps {
            for j in 0..=steps {
                for k in 0..=steps {
                    let uvw = [i, j, k].map(|n| n as f32 / steps as f32);
                    let p = mesh.sample(uvw);
                    let expected = match uvw[0] <= 0.75 {
                        true => big,
                        false => small,
                    };
                    assert!(on(expected, p), "{uvw:?} gave {p:?}");
                }
            }
        }
    }

    #[test]
    fn triangles_of_no_area_are_no_surface() {
        let point = [Vec3::ONE; 3];
        assert!(DomainMesh::new(vec![point]).is_none());
        assert!(DomainMesh::new(Vec::new()).is_none());
    }

    #[test]
    fn a_domain_with_a_mesh_places_its_samples_on_the_surface_in_its_frame() {
        let t = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let to = Vec3::new(5.0, 6.0, 7.0);
        let domain = DomainInst {
            mesh: DomainMesh::new(vec![t]).map(Arc::new),
            local_to_world: Mat4::from_translation(to),
            ..Default::default()
        };
        let p = domain.sample_position([0.5, 0.25, 0.25]);
        assert!(on(t.map(|corner| corner + to), p), "{p:?}");
        // With no mesh the domain samples its shape, a box by default.
        let boxed = DomainInst::default().sample_position([1.0, 1.0, 1.0]);
        assert_eq!(boxed, Vec3::splat(0.5));
    }

    #[test]
    fn a_target_or_attractor_with_a_mesh_aims_at_its_surface() {
        let t = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let surface = DomainMesh::new(vec![t]).map(Arc::new);
        let to = Vec3::new(5.0, 6.0, 7.0);
        let domain = DomainInst {
            mesh: surface.clone(),
            local_to_world: Mat4::from_translation(to),
            ..Default::default()
        };
        let uvw = [0.5, 0.25, 0.25];
        let local = surface.as_deref().expect("a surface").sample(uvw);
        // A direction is the surface's point as a vector, which a translation leaves alone.
        assert_eq!(domain.sample_direction(uvw), local);
        assert_eq!(domain.surface_point(uvw), Some(local + to));
        assert_eq!(DomainInst::default().surface_point(uvw), None);
    }
}
