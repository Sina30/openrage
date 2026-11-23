//! The decimator cuts a geometry to the ratio asked and invents nothing: every vertex it keeps is a vertex of the source.

mod common;

use common::{farthest_from_surface, sheet, sphere};
use or_interchange::doctor::decimate::{decimate, target_of, FLOOR, KEEP_WHOLE};
use or_interchange::Primitive;

/// Whether vertex `v` of `kept` is a vertex of `source` in every channel, bit for bit.
fn is_a_vertex_of(kept: &Primitive, v: usize, source: &Primitive) -> bool {
    (0..source.positions.len()).any(|s| {
        kept.positions[v] == source.positions[s]
            && kept.normals[v] == source.normals[s]
            && kept.tangents[v] == source.tangents[s]
            && kept.uvs.iter().zip(&source.uvs).all(|(k, o)| k[v] == o[s])
            && kept
                .colours
                .iter()
                .zip(&source.colours)
                .all(|(k, o)| k[v] == o[s])
            && kept.joints[v] == source.joints[s]
            && kept.weights[v] == source.weights[s]
    })
}

#[test]
fn a_sphere_is_cut_to_the_ratio_asked_and_keeps_its_texture_seam_and_its_skin() {
    let source = sphere(24, 48);
    let triangles = source.indices.len() / 3;
    for ratio in [0.5f32, 0.25, 0.1] {
        let cut = decimate(&source, ratio);
        let target = target_of(triangles, ratio);
        assert_eq!(cut.target, target);
        assert!(
            cut.reached,
            "ratio {ratio}: stopped at {}",
            cut.triangles_after
        );
        // The last collapse takes two triangles, so the result is the target or one under it.
        assert!(
            (target - 1..=target).contains(&cut.triangles_after),
            "ratio {ratio}: {} triangles for a target of {target}",
            cut.triangles_after
        );
        assert_eq!(cut.primitive.indices.len() / 3, cut.triangles_after);
        assert_eq!(cut.primitive.positions.len(), cut.vertices_after);
        assert!(cut.vertices_after < cut.vertices_before);

        let kept = &cut.primitive;
        let count = kept.positions.len();
        assert!(kept.indices.iter().all(|i| (*i as usize) < count));
        assert!(kept
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .all(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]));
        // Nothing is invented: each vertex is one the source had, in every channel.
        for v in 0..count {
            assert!(
                is_a_vertex_of(kept, v, &source),
                "ratio {ratio}: vertex {v} is new"
            );
        }
        // The channel set is the source's.
        assert_eq!(kept.uvs.len(), source.uvs.len());
        assert_eq!(kept.colours.len(), source.colours.len());
        assert_eq!(kept.uv_channels, source.uv_channels);
        // No triangle reaches across the seam, where the texture runs from 1 back to 0.
        let widest = kept
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let us = [0, 1, 2].map(|k| kept.uvs[0][t[k] as usize][0]);
                us.iter().cloned().fold(f32::MIN, f32::max)
                    - us.iter().cloned().fold(f32::MAX, f32::min)
            })
            .fold(0.0, f32::max);
        assert!(
            widest < 0.5,
            "ratio {ratio}: a triangle spans {widest} of the texture"
        );
        // The skin of every kept vertex sums to a whole.
        assert!(kept
            .weights
            .iter()
            .all(|w| (w.iter().sum::<f32>() - 1.0).abs() < 1e-6));
        // The surface stays near where it was.
        let off = farthest_from_surface(&source, kept);
        let bound = match ratio {
            r if r >= 0.5 => 0.05,
            r if r >= 0.25 => 0.1,
            _ => 0.25,
        };
        assert!(
            off < bound,
            "ratio {ratio}: the source lies {off} from the result"
        );
    }
}

#[test]
fn a_geometry_of_eight_triangles_or_fewer_is_copied_as_it_is() {
    let mut small = sheet(2);
    assert_eq!(small.indices.len() / 3, 8);
    assert_eq!(KEEP_WHOLE, 8);
    let cut = decimate(&small, 0.1);
    assert_eq!(cut.primitive.indices, small.indices);
    assert_eq!(cut.primitive.positions, small.positions);
    small.indices.truncate(3);
    assert_eq!(decimate(&small, 0.1).triangles_after, 1);
}

#[test]
fn no_geometry_is_cut_below_the_floor() {
    let cut = decimate(&sphere(24, 48), 0.001);
    assert!(cut.target >= FLOOR);
    assert!(
        cut.triangles_after >= FLOOR - 1,
        "{} triangles",
        cut.triangles_after
    );
}

#[test]
fn a_ratio_of_one_changes_nothing() {
    let source = sphere(12, 24);
    let cut = decimate(&source, 1.0);
    assert_eq!(cut.triangles_after, cut.triangles_before);
    assert_eq!(cut.primitive.indices, source.indices);
    assert!(cut.reached);
}

#[test]
fn a_border_only_slides_along_itself() {
    let source = sheet(16);
    let cut = decimate(&source, 0.2);
    assert!(
        cut.reached,
        "stopped at {} of {}",
        cut.triangles_after, cut.target
    );
    let kept = &cut.primitive;
    // Every vertex still lies in the sheet's plane, and every vertex that was on the border still does.
    assert!(kept.positions.iter().all(|p| p[2] == 0.0));
    let on_border = |p: &[f32; 3]| p[0] == 0.0 || p[0] == 1.0 || p[1] == 0.0 || p[1] == 1.0;
    // The four corners are the border's ends and stay.
    for corner in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
        assert!(
            kept.positions
                .iter()
                .any(|p| p[0] == corner[0] && p[1] == corner[1]),
            "the corner {corner:?} was collapsed"
        );
    }
    // The sheet is still the unit square: its area is the same, so nothing was pulled off the border.
    let area: f32 = kept
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| kept.positions[t[k] as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2.0
        })
        .sum();
    assert!((area - 1.0).abs() < 1e-4, "the sheet covers {area}");
    assert!(kept.positions.iter().filter(|p| on_border(p)).count() >= 4);
}

#[test]
fn a_surface_that_two_faces_cover_with_one_set_of_edges_is_left_alone() {
    // A flat sheet drawn twice, once facing each way, shares every edge among four triangles: no collapse of it is safe.
    let mut doubled = sheet(6);
    let once = doubled.clone();
    let offset = doubled.positions.len() as u32;
    doubled.positions.extend(once.positions.iter());
    doubled
        .normals
        .extend(once.normals.iter().map(|n| [-n[0], -n[1], -n[2]]));
    doubled.uvs[0].extend(once.uvs[0].iter());
    doubled.indices.extend(
        once.indices
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|t| [t[0] + offset, t[2] + offset, t[1] + offset]),
    );
    let triangles = doubled.indices.len() / 3;
    let cut = decimate(&doubled, 0.5);
    assert!(!cut.reached);
    assert_eq!(cut.triangles_after, triangles);
}

#[test]
fn a_texture_seam_is_never_torn() {
    let source = common::sheet_with_a_seam(16);
    assert_eq!(common::cracks(&source), 0);
    let cut = decimate(&source, 0.2);
    assert!(
        cut.reached,
        "stopped at {} of {}",
        cut.triangles_after, cut.target
    );
    assert_eq!(common::cracks(&cut.primitive), 0, "a crack opened");
    // The texture still jumps by a quarter across the seam and by nothing else: no triangle spans more than the jump and its own width.
    let kept = &cut.primitive;
    let widest = kept
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let us = [0, 1, 2].map(|k| kept.uvs[0][t[k] as usize][0]);
            us.iter().cloned().fold(f32::MIN, f32::max)
                - us.iter().cloned().fold(f32::MAX, f32::min)
        })
        .fold(0.0, f32::max);
    assert!(widest <= 0.5, "a triangle spans {widest} of the texture");
}
