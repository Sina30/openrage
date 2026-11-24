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
