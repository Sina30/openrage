use super::*;

#[test]
fn the_sphere_is_a_unit_one_cut_into_the_triangles_the_cuts_make_and_every_one_faces_outward() {
    let corners = sphere();
    assert_eq!(corners.len(), 8 * 4usize.pow(CUTS) * 3);
    for triangle in corners.as_chunks::<3>().0 {
        for corner in triangle {
            assert!((corner.length() - 1.0).abs() < 1e-5, "{corner:?}");
        }
        let normal = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
        let middle = (triangle[0] + triangle[1] + triangle[2]) / 3.0;
        assert!(normal.dot(middle) > 0.0, "{triangle:?} faces inward");
    }
}
