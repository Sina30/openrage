use super::*;
use crate::gta6::ttpf::Format;

/// A grid of 4 by 2 tiles of 64 m about (0, 0), whose fallback is 16 m a texel.
fn atlas() -> GridAtlas {
    GridAtlas {
        origin: [0.0, 0.0],
        size: [256.0, 128.0],
        tile_size: 64.0,
        ..GridAtlas::default()
    }
}

/// A fallback texel's value: a ramp east and a steeper one south, so no two blocks are alike.
fn under(c: u32, r: u32) -> u16 {
    (30_000 + 37 * c + 911 * r) as u16
}

fn fallback() -> Heights {
    let values = (0..8).flat_map(|r| (0..16).map(move |c| under(c, r))).collect();
    Heights::new(16, 8, values).expect("16 by 8")
}

/// A 1 m tile of the block at `col`, `row`: the fallback there, give or take a little.
fn tile(col: u32, row: u32) -> Heights {
    let values = (0..64)
        .flat_map(|j| {
            (0..64).map(move |i| u32::from(under(col * 4 + i / 16, row * 4 + j / 16)) + (i + j) % 3)
        })
        .map(|v| v as u16)
        .collect();
    Heights::new(64, 64, values).expect("64 by 64")
}

#[test]
fn a_value_is_a_height_of_sixteen_hundred_metres_over_its_range_about_its_middle() {
    assert_eq!(height(32768), 0.0);
    assert_eq!(height(0), -800.0);
    assert!((height(65535) - 800.0).abs() < 0.03);
    assert!((height(32768 + 4096) - 100.0).abs() < 1e-3);
}

#[test]
fn the_grid_is_about_its_origin_and_its_rows_run_south() {
    let atlas = GridAtlas {
        origin: [-50.0, 20.0],
        size: [300.0, 600.0],
        ..atlas()
    };
    assert_eq!((atlas.west(), atlas.north()), (-200.0, 320.0));
    let ground = WaterGround::new(&self::atlas(), fallback()).expect("fits");
    // The north-west texel is the first; the one a texel east and two south is the fallback's (1, 2).
    assert_eq!(ground.ground(-127.0, 63.0), Some(height(under(0, 0))));
    assert_eq!(ground.ground(-128.0 + 17.0, 64.0 - 33.0), Some(height(under(1, 2))));
    assert_eq!(ground.ground(-129.0, 0.0), None);
    assert_eq!(ground.ground(0.0, 64.5), None);
    assert_eq!(ground.ground(128.0, 0.0), None);
}

#[test]
fn a_tile_is_laid_where_its_means_match_the_fallback_and_read_there_at_its_own_texels() {
    let mut ground = WaterGround::new(&atlas(), fallback()).expect("fits");
    assert_eq!(ground.add_tile(tile(2, 1)), Some((2, 1)));
    assert_eq!(ground.add_tile(tile(0, 0)), Some((0, 0)));
    assert_eq!(ground.tiles(), 2);
    // Ten metres east and twenty south into tile (2, 1): its texel (10, 20).
    let (x, y) = (-128.0 + 2.0 * 64.0 + 10.5, 64.0 - 64.0 - 20.5);
    let own = tile(2, 1).at(10, 20).expect("in the tile");
    assert_eq!(ground.ground(x, y), Some(height(own)));
    // A place no tile is laid on reads the fallback.
    assert_eq!(ground.ground(-128.0 + 64.0 + 1.0, 63.0), Some(height(under(4, 0))));
}

#[test]
fn a_flat_tile_is_not_laid_and_a_fallback_off_its_grid_is_refused() {
    let mut ground = WaterGround::new(&atlas(), fallback()).expect("fits");
    let flat = Heights::new(64, 64, vec![32768; 64 * 64]).expect("64 by 64");
    assert_eq!(ground.add_tile(flat), None);
    assert_eq!(ground.tiles(), 0);
    let short = Heights::new(16, 7, vec![0; 16 * 7]).expect("16 by 7");
    assert!(matches!(
        WaterGround::new(&atlas(), short),
        Err(WaterError::Ground(_))
    ));
    let no_tiles = GridAtlas {
        tile_size: 0.0,
        ..atlas()
    };
    assert!(WaterGround::new(&no_tiles, fallback()).is_err());
}

#[test]
fn a_map_shrinks_to_the_rounded_means_of_its_boxes() {
    let map = Heights::new(4, 2, vec![0, 2, 10, 10, 1, 2, 10, 11]).expect("4 by 2");
    let small = map.shrunk(2).expect("halves");
    assert_eq!(small.width(), 2);
    assert_eq!((small.at(0, 0), small.at(1, 0)), (Some(1), Some(10)));
    assert_eq!(small.at(0, 1), Some(2));
    assert!(map.shrunk(3).is_none());
    assert!(Heights::new(2, 2, vec![0; 3]).is_none());
}

#[test]
fn an_r16_texture_reads_as_its_values_and_another_format_does_not() {
    let r16 = Format::of_dxgi(56).expect("R16_UNORM");
    let texture = Texture {
        width: 2,
        height: 1,
        format: r16,
        levels: 1,
        faces: 1,
        pixels: vec![0x00, 0x80, 0x34, 0x12],
    };
    let heights = Heights::of(&texture).expect("R16");
    assert_eq!((heights.at(0, 0), heights.at(1, 0)), (Some(0x8000), Some(0x1234)));
    let other = Texture {
        format: Format::of_dxgi(61).expect("R8_UNORM"),
        ..texture
    };
    assert!(Heights::of(&other).is_none());
}
