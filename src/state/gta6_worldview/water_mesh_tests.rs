use super::*;
use or_io::gta6::water::height::Heights;
use or_io::gta6::water::GridAtlas;

fn quad(min: [i16; 2], max: [i16; 2], kind: u8) -> WaterQuad {
    WaterQuad {
        min,
        max,
        alpha: [26; 4],
        z: 0.0,
        kind,
        ocean: false,
        invisible: false,
        limited_depth: false,
        no_stencil: false,
    }
}

type Mesh = (Vec<MeshVertex>, Vec<u32>);

fn plain(quads: &[WaterQuad], eye: [f32; 2]) -> Mesh {
    mesh(quads, Vec3::new(eye[0], eye[1], 0.0), 10_000.0, None, None)
}

fn triangles(mesh: &Mesh) -> impl Iterator<Item = [Vec3; 3]> + '_ {
    (mesh.1.chunks(3)).map(|t| [t[0], t[1], t[2]].map(|k| Vec3::from(mesh.0[k as usize].position)))
}

/// The area the mesh covers from above, each of its triangles counter-clockwise.
fn area(mesh: &Mesh) -> f32 {
    triangles(mesh)
        .map(|[a, b, c]| {
            let twice = (b - a).cross(c - a).z;
            assert!(twice > 0.0, "a triangle faces down: {a} {b} {c}");
            twice / 2.0
        })
        .sum()
}

fn covers(mesh: &Mesh, x: f32, y: f32) -> bool {
    let p = Vec3::new(x, y, 0.0);
    triangles(mesh).any(|[a, b, c]| {
        let left = |from: Vec3, to: Vec3| (to - from).cross(p - from).z >= 0.0;
        left(a, b) && left(b, c) && left(c, a)
    })
}

#[test]
fn each_cut_and_its_mirror_tile_the_square_and_leave_out_the_corner_their_type_names() {
    let square = |kind| plain(&[quad([0, 0], [10, 10], kind)], [5.0, 5.0]);
    // The type, its mirror, and a point by the corner it leaves out.
    for (kind, mirror, (x, y)) in [
        (1, 3, (9.0, 9.0)),
        (2, 4, (9.0, 1.0)),
        (3, 1, (1.0, 1.0)),
        (4, 2, (1.0, 9.0)),
    ] {
        let (cut, other) = (square(kind), square(mirror));
        assert_eq!(cut.0.len(), 3, "a square of one cell is one triangle");
        assert!((area(&cut) - 50.0).abs() < 1e-3);
        assert!(!covers(&cut, x, y) && covers(&other, x, y), "type {kind}");
        for j in 0..10 {
            for i in 0..10 {
                let (px, py) = (i as f32 + 0.37, j as f32 + 0.61);
                assert!(
                    covers(&cut, px, py) != covers(&other, px, py),
                    "type {kind} at {px}, {py}"
                );
            }
        }
    }
}

#[test]
fn a_cut_quad_of_many_cells_meets_its_mirror_at_the_same_points_of_the_diagonal() {
    let big = |kind| plain(&[quad([0, 0], [100, 100], kind)], [50.0, 50.0]);
    let on_diagonal = |mesh: &Mesh| {
        let mut points: Vec<[u32; 2]> = (mesh.0.iter())
            .filter(|v| (v.position[0] + v.position[1] - 100.0).abs() < 1e-3)
            .map(|v| [v.position[0].to_bits(), v.position[1].to_bits()])
            .collect();
        points.sort_unstable();
        points.dedup();
        points
    };
    let (cut, mirror) = (big(1), big(3));
    assert!((area(&cut) + area(&mirror) - 10_000.0).abs() < 0.1);
    assert!(on_diagonal(&cut).len() > 2);
    assert_eq!(on_diagonal(&cut), on_diagonal(&mirror));
}

#[test]
fn a_quad_is_cut_along_one_grid_so_its_neighbour_shares_the_points_of_their_edge() {
    let (below, above) = (quad([0, 0], [100, 50], 0), quad([0, 50], [100, 100], 0));
    let edge = |mesh: &Mesh| {
        let mut xs: Vec<u32> = (mesh.0.iter())
            .filter(|v| v.position[1] == 50.0)
            .map(|v| v.position[0].to_bits())
            .collect();
        xs.sort_unstable();
        xs.dedup();
        xs
    };
    let near = plain(&[below], [50.0, 50.0]);
    assert!((area(&near) - 5_000.0).abs() < 0.1);
    let xs = edge(&near);
    assert_eq!(xs.len(), 8, "every 16 m and the two corners");
    assert_eq!(xs, edge(&plain(&[above], [50.0, 50.0])));
    // Far from the camera the cells are coarse, and the quad keeps its area.
    let far = plain(&[below], [5_000.0, 0.0]);
    assert!((area(&far) - 5_000.0).abs() < 0.1);
    assert_eq!(edge(&far).len(), 3);
}

#[test]
fn a_quads_opacity_is_its_corners_from_south_west_round_to_north_west() {
    let corners = WaterQuad {
        alpha: [0, 255, 255, 0],
        ..quad([0, 0], [10, 10], 0)
    };
    let (vertices, indices) = plain(&[corners], [5.0, 5.0]);
    assert_eq!((vertices.len(), indices), (4, vec![0, 1, 2, 0, 2, 3]));
    let alpha = |x: f32, y: f32| {
        let at = (vertices.iter()).find(|v| v.position[..2] == [x, y]);
        at.expect("a corner").colour[3]
    };
    assert_eq!(
        [alpha(0.0, 0.0), alpha(10.0, 0.0), alpha(10.0, 10.0), alpha(0.0, 10.0)],
        [0.0, 1.0, 1.0, 0.0]
    );
    assert!(vertices.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    assert_eq!(vertices[2].uv, [10.0, 10.0]);
    assert_eq!(vertices[0].colour[..3], TINT);
}

#[test]
fn a_quad_out_of_reach_a_hidden_one_a_deleted_one_and_one_of_no_area_are_left_out() {
    let far = quad([1000, 1000], [1100, 1100], 0);
    let hidden = WaterQuad {
        invisible: true,
        ..quad([0, 0], [1, 1], 0)
    };
    let deleted = quad([0, 0], [1, 1], 5);
    let near = quad([50, 0], [60, 10], 0);
    let (eye, reach) = (Vec3::ZERO, 100.0);
    let (vertices, indices) = mesh(&[far, hidden, deleted, near], eye, reach, None, None);
    assert_eq!((vertices.len(), indices.len()), (4, 6));
    assert_eq!(vertices[0].position, [50.0, 0.0, 0.0]);
    // A camera over a quad is at no distance from it, however high the reach is counted from.
    let over = Vec3::new(1050.0, 1050.0, 0.0);
    assert!(!mesh(&[far], over, 1.0, None, None).0.is_empty());
    let reversed = [quad([10, 0], [0, 10], 0), quad([0, 10], [10, 0], 0)];
    assert!(mesh(&reversed, eye, reach, None, None).0.is_empty());
}

fn medium(extinction: [f32; 3], albedo: [f32; 3]) -> Medium {
    Medium { extinction, albedo }
}

#[test]
fn water_over_dry_ground_is_not_drawn_and_deeper_water_hides_more_of_the_ground() {
    let m = medium([1.0, 0.5, 0.25], [1.0; 3]);
    assert_eq!(shade(0.1, Some(-1.0), Some(&m))[3], 0.0);
    assert_eq!(shade(0.1, Some(0.0), Some(&m))[3], 0.0);
    let (shallow, deep) = (shade(0.1, Some(2.0), Some(&m)), shade(0.1, Some(8.0), Some(&m)));
    assert!(0.1 < shallow[3] && shallow[3] < deep[3] && deep[3] < 1.0);
    // Red is taken out first, so more of the water's own red shows than of its blue.
    assert!(shallow[0] > shallow[2]);
    // Where the depth is not known only the quad's own opacity is drawn.
    assert!((shade(0.1, None, Some(&m))[3] - 0.1).abs() < 1e-6);
    let fading = shade(0.1, Some(0.5), Some(&m))[3];
    assert!(0.0 < fading && fading < shade(0.1, Some(1.0), Some(&m))[3]);
    assert_eq!(shade(0.3, None, None), [TINT[0], TINT[1], TINT[2], 0.3]);
}

#[test]
fn the_medium_is_the_days_extinction_and_its_scattering_over_it() {
    let vars: [(&str, f32); 7] = [
        ("water_clip_extinction_r", 2.0),
        ("water_clip_extinction_g", 1.0),
        ("water_clip_extinction_b", 1.0),
        ("water_clip_extinction_scale", 2.0),
        ("water_clip_scattering_r", 1.0),
        ("water_clip_scattering_g", 1.0),
        ("water_clip_scattering_b", 0.5),
    ];
    let get = |name: &str| vars.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
    let m = Medium::of(get).expect("a medium");
    assert_eq!(m.extinction, [4.0, 2.0, 2.0]);
    assert_eq!(m.albedo, [0.25, 0.5, 0.25]);
    assert_eq!(Medium::of(|_| None), None);
    assert_eq!(Medium::of(|_| Some(0.0)), None);
}

#[test]
fn the_ground_under_a_quad_dries_the_part_of_it_that_stands_over_land() {
    // A grid of two 16 m tiles about (0, 0): ten metres down to the west, ten up to the east.
    let atlas = GridAtlas {
        size: [32.0, 16.0],
        tile_size: 16.0,
        ..GridAtlas::default()
    };
    let fallback = Heights::new(2, 1, vec![32358, 33178]).expect("2 by 1");
    let ground = WaterGround::new(&atlas, fallback).expect("fits");
    let m = medium([1.0; 3], [0.1; 3]);
    let eye = Vec3::ZERO;
    let (vertices, _) = mesh(&[quad([-15, -7], [15, 7], 0)], eye, 100.0, Some(&ground), Some(&m));
    assert!(!vertices.is_empty());
    for v in &vertices {
        let (x, alpha) = (v.position[0], v.colour[3]);
        assert!(if x < 0.0 { alpha > 0.5 } else { alpha == 0.0 }, "at {x}: {alpha}");
    }
}
