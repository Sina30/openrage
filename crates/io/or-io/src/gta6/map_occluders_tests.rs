use super::super::lights::tests::{array, block, model_of, ptr, put, put_floats};
use super::*;

const VERTICES: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 2.0],
    [0.0, 1.0, 2.0],
];

/// Two boxes, block 2: one as it stands, one stated from z 9 down to 5 and turned a quarter about
/// z; and one model, block 3, of the four vertices and two triangles of block 4.
fn map_model() -> RbmModel {
    let mut root = vec![0u8; MAP_DATA_SIZE];
    array(&mut root, 0x110, ptr(2, 0), 2);
    array(&mut root, 0x120, ptr(3, 0), 1);
    let mut boxes = vec![0u8; 2 * BOX_SIZE];
    put_floats(&mut boxes, 0x00, &[1.0, 2.0, 3.0, 1.0, 4.0, 6.0, 8.0, 1.0]);
    put_floats(&mut boxes, 0x20, &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    put_floats(&mut boxes, 0x48, &[1.0]);
    let turned = &mut boxes[BOX_SIZE..];
    put_floats(turned, 0x00, &[10.0, 20.0, 9.0, 1.0, 12.0, 23.0, 5.0, 1.0]);
    put_floats(turned, 0x20, &[0.0, 1.0, 0.0, 0.0, -1.0, 0.0]);
    put_floats(turned, 0x48, &[1.0]);
    let mut occluder = vec![0u8; MODEL_SIZE];
    put_floats(&mut occluder, 0x00, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0]);
    put(&mut occluder, 0x20, &54u32.to_le_bytes());
    put(&mut occluder, 0x28, &ptr(4, 0).to_le_bytes());
    put(&mut occluder, 0x30, &[48, 0, 2, 0, 4, 0, 0, 0]);
    let mut data = vec![0u8; 54];
    put_floats(&mut data, 0, VERTICES.as_flattened());
    put(&mut data, 48, &[0, 1, 2, 0, 2, 3]);
    model_of(
        super::super::LAYOUTS.iter().chain(LAYOUTS),
        1,
        vec![
            block(C_MAP_DATA, root),
            block(BOX_OCCLUDER, boxes),
            block(OCCLUDE_MODEL, occluder),
            block(BYTES, data),
        ],
    )
}

/// The corners of the box `low` to `high`, each placed by `place`.
fn corners(low: [f32; 3], high: [f32; 3], place: fn([f32; 3]) -> [f32; 3]) -> BoxOccluder {
    BoxOccluder {
        corners: std::array::from_fn(|k| {
            place(std::array::from_fn(|axis| match k >> axis & 1 {
                0 => low[axis],
                _ => high[axis],
            }))
        }),
    }
}

#[test]
fn a_map_reads_its_boxes_and_models_in_the_world() {
    let read = read(&map_model()).expect("it reads");
    let expected = MapOccluders {
        boxes: vec![
            corners([1.0, 2.0, 3.0], [4.0, 6.0, 8.0], |p| p),
            // A quarter turn about z: the box's x runs along the world's y.
            corners([10.0, 20.0, 5.0], [12.0, 23.0, 9.0], |[x, y, z]| [-y, x, z]),
        ],
        models: vec![OccludeModel {
            bounds: Aabb {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 2.0],
            },
            flags: 4,
            vertices: VERTICES.to_vec(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }],
    };
    assert_eq!(read, expected);
    let triangles: Vec<_> = read.triangles().collect();
    assert_eq!(triangles.len(), 2 * 12 + 2);
    assert_eq!(triangles[25], [VERTICES[0], VERTICES[2], VERTICES[3]]);
}

#[test]
fn a_map_with_no_occluder_reads_as_none() {
    let mut m = map_model();
    m.data_blocks[0].data = vec![0u8; MAP_DATA_SIZE];
    assert_eq!(read(&m).expect("it reads"), MapOccluders::default());
}

#[test]
fn a_box_s_triangles_cover_each_face_twice_and_face_out() {
    let corner = |k: u8| [0, 1, 2].map(|axis| f32::from(k >> axis & 1));
    let mut faces = [[0u8; 2]; 3];
    for t in BOX_TRIANGLES {
        let [a, b, c] = t.map(corner);
        let (u, v) = (
            [0, 1, 2].map(|i| b[i] - a[i]),
            [0, 1, 2].map(|i| c[i] - a[i]),
        );
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let axis = (0..3).find(|i| normal[*i] != 0.0).expect("a triangle");
        // The triangle lies in the face its normal points out of.
        assert!([a, b, c].iter().all(|p| p[axis] == a[axis]), "{t:?}");
        assert_eq!(normal[axis] > 0.0, a[axis] == 1.0, "{t:?}");
        faces[axis][a[axis] as usize] += 1;
    }
    assert_eq!(faces, [[2; 2]; 3]);
}

type Patch = Box<dyn Fn(&mut RbmModel)>;

fn set(block: usize, at: usize, bytes: Vec<u8>) -> Patch {
    Box::new(move |m: &mut RbmModel| put(&mut m.data_blocks[block - 1].data, at, &bytes))
}

#[test]
fn what_breaks_a_measured_rule_is_refused_with_the_rule() {
    let cases: Vec<(Patch, &str)> = vec![
        (
            set(2, 0x20, 2.0f32.to_le_bytes().to_vec()),
            "not three unit axes",
        ),
        (
            set(3, 0x00, 5.0f32.to_le_bytes().to_vec()),
            "bmin is over its bmax",
        ),
        (set(3, 0x20, vec![55, 0, 0, 0]), "dataSize 55 is not"),
        (set(3, 0x30, vec![47]), "dataSize 54 is not"),
        (set(4, 48, vec![4]), "a vertex past its 4 vertices"),
        (set(4, 0, f32::NAN.to_le_bytes().to_vec()), "is not finite"),
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[3].data_type = 0x15),
            "its verts name no block",
        ),
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[3].data.truncate(53)),
            "its verts name no block",
        ),
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[1].data_type = OCCLUDE_MODEL),
            "do not fit a block of their type",
        ),
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[0].data_type = C_MAP_TYPES),
            "it is a CMapTypes",
        ),
        (
            Box::new(|m: &mut RbmModel| {
                m.classes
                    .get_mut(&OCCLUDE_MODEL)
                    .expect("declared")
                    .data_size = 0x48;
            }),
            "OccludeModel is 0x48 bytes",
        ),
    ];
    for (patch, want) in cases {
        let mut m = map_model();
        patch(&mut m);
        let err = read(&m).expect_err(want).to_string();
        assert!(err.contains(want), "{want}: {err}");
    }
}

#[test]
fn every_truncated_block_is_an_error_and_never_a_panic() {
    let whole = map_model();
    for b in 0..whole.data_blocks.len() {
        for len in 0..whole.data_blocks[b].data.len() {
            let mut m = whole.clone();
            m.data_blocks[b].data.truncate(len);
            assert!(read(&m).is_err(), "block {b} cut to {len} bytes still read");
        }
    }
}

#[test]
fn every_changed_byte_is_read_or_refused_and_never_a_panic() {
    crate::gta6::mutated_tests::every_block_change(&map_model(), |m| {
        let _ = read(m);
    });
}
