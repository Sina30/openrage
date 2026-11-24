use super::*;
use crate::meta::binary::pso::table_form::tests::{body, member, pointer, Class};

fn quad(min: [i16; 2], max: [i16; 2], z: f32, kind: u8) -> [u8; QUAD_SIZE] {
    let mut q = [0u8; QUAD_SIZE];
    for (k, v) in [min[0], min[1], max[0], max[1]].into_iter().enumerate() {
        q[2 * k..2 * k + 2].copy_from_slice(&v.to_le_bytes());
    }
    q[8..12].copy_from_slice(&[26, 30, 40, 50]);
    q[0x18..0x1C].copy_from_slice(&z.to_le_bytes());
    q[0x1C] = kind;
    q[0x26] = 1;
    q
}

fn classes(quad_members: &[(u32, u32, u8)]) -> Vec<Class> {
    vec![
        Class {
            name: WATER_DATA,
            size: ROOT_SIZE as u32,
            members: vec![
                member(0x100, 0, 0x05, 0, WATER_QUAD),
                member(lit(b"WaterQuads"), QUADS as u32, ARRAY, 0, 0),
            ],
        },
        Class {
            name: WATER_QUAD,
            size: QUAD_SIZE as u32,
            members: (quad_members.iter())
                .map(|&(name, offset, code)| member(name, offset, code, 0, 0))
                .collect(),
        },
    ]
}

fn document(quads: &[[u8; QUAD_SIZE]], count: u16, quad_members: &[(u32, u32, u8)]) -> Vec<u8> {
    let mut root = vec![0u8; ROOT_SIZE];
    root[QUADS..QUADS + 8].copy_from_slice(&pointer(2, 0).to_le_bytes());
    root[QUADS + 8..QUADS + 10].copy_from_slice(&count.to_le_bytes());
    root[QUADS + 10..QUADS + 12].copy_from_slice(&count.to_le_bytes());
    let block = quads.concat();
    body(
        &classes(quad_members),
        &[(WATER_DATA, root), (WATER_QUAD, block)],
        1,
    )
}

#[test]
fn the_quads_of_a_document_are_read_with_their_heights_and_corners() {
    let bytes = document(
        &[
            quad([-10, -20], [30, 40], 0.3, 0),
            quad([5, 6], [7, 8], 28.5, 3),
        ],
        2,
        QUAD,
    );
    let water = WaterData::open(&bytes).expect("a water document");
    assert_eq!(water.quads.len(), 2);
    let first = water.quads[0];
    assert_eq!((first.min, first.max), ([-10, -20], [30, 40]));
    assert_eq!(
        (first.z, first.kind, first.alpha),
        (0.3, 0, [26, 30, 40, 50])
    );
    assert!(first.ocean && !first.invisible && !first.limited_depth && !first.no_stencil);
    assert_eq!((water.quads[1].z, water.quads[1].kind), (28.5, 3));
}

#[test]
fn a_count_past_the_block_and_a_height_that_is_no_number_are_refused() {
    let one = [quad([0, 0], [1, 1], 0.0, 0)];
    let past = WaterData::open(&document(&one, 2, QUAD));
    assert!(matches!(past, Err(WaterError::Broken(_))), "{past:?}");
    let nan = WaterData::open(&document(&[quad([0, 0], [1, 1], f32::NAN, 0)], 1, QUAD));
    assert!(matches!(nan, Err(WaterError::Broken(_))), "{nan:?}");
}

#[test]
fn a_body_that_declares_a_quad_another_way_is_refused_and_another_root_is_not_water() {
    let mut moved = QUAD.to_vec();
    moved[8].1 = 0x14;
    let read = WaterData::open(&document(&[quad([0, 0], [1, 1], 1.0, 0)], 1, &moved));
    assert!(
        matches!(
            read,
            Err(WaterError::Layout {
                class: "WaterQuad",
                offset: 0x18,
                ..
            })
        ),
        "{read:?}"
    );
    let other = body(&classes(QUAD), &[(WATER_QUAD, vec![0; QUAD_SIZE])], 1);
    let read = WaterData::open(&other);
    assert!(
        matches!(read, Err(WaterError::NotWater(WATER_QUAD))),
        "{read:?}"
    );
    assert!(matches!(
        WaterData::open(b"nothing"),
        Err(WaterError::Model(_))
    ));
}

#[test]
fn an_empty_document_has_no_quad() {
    let mut root = vec![0u8; ROOT_SIZE];
    root[QUADS..QUADS + 8].fill(0);
    let bytes = body(&classes(QUAD), &[(WATER_DATA, root)], 1);
    assert_eq!(WaterData::open(&bytes).expect("read"), WaterData::default());
}

/// The classes of a document that declares its border: its corners' heights and its segments.
fn bordered_classes() -> Vec<Class> {
    let mut classes = classes(QUAD);
    classes[0].members.push(member(
        lit(b"BorderInfo"),
        BORDER as u32,
        STRUCT,
        0,
        WATER_BORDERS,
    ));
    let corners = [
        lit(b"NorthEast"),
        lit(b"SouthEast"),
        lit(b"SouthWest"),
        lit(b"NorthWest"),
    ];
    let mut borders: Vec<_> = (corners.iter().zip(0u32..))
        .map(|(&name, k)| member(name, 4 * k, FLOAT, 0, 0))
        .collect();
    borders.push(member(
        lit(b"BorderSegments"),
        SEGMENTS as u32,
        FIXED_ARRAY,
        0,
        0,
    ));
    classes.push(Class {
        name: WATER_BORDERS,
        size: 212,
        members: borders,
    });
    classes.push(Class {
        name: BORDER_SEGMENT,
        size: SEGMENT_SIZE as u32,
        members: vec![
            member(lit(b"WhichSide"), 0, ENUM32, 0, 0),
            member(lit(b"RangeMin"), 4, FLOAT, 0, 0),
            member(lit(b"RangeMax"), 8, FLOAT, 0, 0),
            member(lit(b"Height"), 12, FLOAT, 0, 0),
        ],
    });
    classes
}

/// A root whose border's corners stand at 1 to 4 m and whose one segment is side 2 from -50 to 70 at 0.5 m.
fn bordered_root() -> Vec<u8> {
    let mut root = vec![0u8; ROOT_SIZE];
    for (k, v) in [1.0f32, 2.0, 3.0, 4.0].into_iter().enumerate() {
        root[BORDER + 4 * k..BORDER + 4 * k + 4].copy_from_slice(&v.to_le_bytes());
    }
    let segment = BORDER + SEGMENTS;
    root[segment..segment + 4].copy_from_slice(&2u32.to_le_bytes());
    for (k, v) in [-50.0f32, 70.0, 0.5].into_iter().enumerate() {
        root[segment + 4 + 4 * k..segment + 8 + 4 * k].copy_from_slice(&v.to_le_bytes());
    }
    let count = segment + SEGMENTS_HELD * SEGMENT_SIZE;
    root[count..count + 4].copy_from_slice(&1u32.to_le_bytes());
    root
}

#[test]
fn a_border_is_read_where_the_body_declares_it_and_none_where_it_does_not() {
    let root = bordered_root();
    let bordered = body(&bordered_classes(), &[(WATER_DATA, root.clone())], 1);
    let water = WaterData::open(&bordered).expect("read");
    let border = water.border.expect("a border");
    assert_eq!(border.corners, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        border.segments,
        vec![BorderSegment {
            side: 2,
            range: [-50.0, 70.0],
            height: 0.5
        }]
    );
    let plain = WaterData::open(&body(&self::classes(QUAD), &[(WATER_DATA, root)], 1));
    assert_eq!(
        plain.expect("read").border,
        None,
        "a body that does not declare it"
    );
}

#[test]
fn a_grid_atlas_states_its_grid_and_the_names_of_its_tiles() {
    let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<GridAtlasInfo>
	<type>GS_TYPE_2D_TEXTURE_ATLAS</type>
	<clientSystemName>Heights</clientSystemName>
	<tileTextureNamePrefix>heights-</tileTextureNamePrefix>
	<tileTextureNameSuffix>heights</tileTextureNameSuffix>
	<globalFallbackTextureName>heights_all</globalFallbackTextureName>
	<origin x="-100.0" y="200.0" z="0.0" />
	<width value="1024" />
	<height value="2048" />
	<tileSize value="512" />
	<streamRadius value="300.0" />
	<sectorOverrides><Item><sectorName>other</sectorName><width value="7" /></Item></sectorOverrides>
</GridAtlasInfo>"#;
    let atlas = GridAtlas::parse(text).expect("an atlas");
    assert_eq!(atlas.client, "Heights");
    assert_eq!(
        (atlas.tile_prefix.as_str(), atlas.tile_suffix.as_str()),
        ("heights-", "heights")
    );
    assert_eq!(atlas.fallback, "heights_all");
    assert_eq!(
        (atlas.origin, atlas.size),
        ([-100.0, 200.0], [1024.0, 2048.0])
    );
    assert_eq!((atlas.tile_size, atlas.stream_radius), (512.0, 300.0));
    assert!(matches!(
        GridAtlas::parse("<Other/>"),
        Err(WaterError::NotAnAtlas(_))
    ));
    assert!(matches!(
        GridAtlas::parse("<GridAtlasInfo></GridAtlasInfo>"),
        Err(WaterError::NotAnAtlas(_))
    ));
}

#[test]
fn every_changed_byte_is_read_or_refused_and_never_a_panic() {
    let bytes = document(
        &[
            quad([-10, -20], [30, 40], 0.3, 0),
            quad([5, 6], [7, 8], 28.5, 3),
        ],
        2,
        QUAD,
    );
    crate::gta6::mutated_tests::every_change(&bytes, |bytes| {
        let _ = WaterData::open(bytes);
    });
    // The border is read only from a body that declares it.
    let bordered = body(&bordered_classes(), &[(WATER_DATA, bordered_root())], 1);
    crate::gta6::mutated_tests::every_change(&bordered, |bytes| {
        let _ = WaterData::open(bytes);
    });
}
