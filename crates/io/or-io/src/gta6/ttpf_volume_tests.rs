//! Volumes and a top level cut into 4 MiB rows, in packs built here from the documented layout.

use super::super::write::Start;
use super::*;

/// A volume tile's element-index bits, lowest first, for each element size, as the layout states them.
const VOLUME_1: &str = "x0 x1 z0 y0 z1 y1 x2 z2 y2 x3 z3 y3";
const VOLUME_2: &str = "x0 z0 y0 z1 y1 x1 z2 y2 x2 z3 y3";
const VOLUME_4: &str = "x0 y0 z0 y1 x1 z1 y2 x2 z2 y3";
const VOLUME_8: &str = "x0 z0 y0 x1 z1 y1 x2 z2 y2";
const VOLUME_16: &str = "z0 y0 x0 z1 y1 x1 z2 y2";

/// A volume's tail slots, largest level first.
const TAIL_3D: [usize; 5] = [0x800, 0x300, 0x200, 0x100, 0];

/// A volume tile's sides in elements.
fn sides(pattern: &str) -> [usize; 3] {
    let count = |axis| {
        pattern
            .split(' ')
            .filter(|bit| bit.starts_with(axis))
            .count()
    };
    [1 << count('x'), 1 << count('y'), 1 << count('z')]
}

/// Element (`x`, `y`, `z`)'s index inside its volume tile under `pattern`.
fn index3(pattern: &str, [x, y, z]: [usize; 3]) -> usize {
    (pattern.split(' ').enumerate())
        .map(|(i, bit)| {
            let coordinate = match &bit[..1] {
                "x" => x,
                "y" => y,
                _ => z,
            };
            ((coordinate >> bit[1..].parse::<usize>().expect("a bit number")) & 1) << i
        })
        .sum()
}

/// One level of a volume: its size in elements and its elements, x fastest, then y, then z.
struct Level3 {
    size: [usize; 3],
    elements: Vec<u8>,
}

/// A level of distinct, non-zero elements.
fn level3(size: [usize; 3], element: usize, seed: usize) -> Level3 {
    let count = size.iter().product::<usize>() * element;
    let elements = (0..count)
        .map(|i| (1 + (i * 7 + seed * 31) % 251) as u8)
        .collect();
    Level3 { size, elements }
}

/// Writes `level` into `memory` in tiles from `base`, row by row, its z-slabs `slab` bytes apart.
fn place3(
    memory: &mut [u8],
    level: &Level3,
    base: usize,
    slab: usize,
    pattern: &str,
    element: usize,
) {
    let [tile_w, tile_h, tile_d] = sides(pattern);
    let [width, height, depth] = level.size;
    for z in 0..depth {
        for y in 0..height {
            for x in 0..width {
                let tile_at = (y / tile_h) * width.div_ceil(tile_w) + x / tile_w;
                let at = base
                    + z / tile_d * slab
                    + tile_at * 0x1000
                    + index3(pattern, [x, y, z]) * element;
                let from = ((z * height + y) * width + x) * element;
                memory[at..at + element].copy_from_slice(&level.elements[from..from + element]);
            }
        }
    }
}

/// What a volume reads as: each slice s holds slice `s >> m` of every level m.
fn slices(levels: &[Level3], depth: usize, element: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for slice in 0..depth {
        for (m, level) in levels.iter().enumerate() {
            let [width, height, d] = level.size;
            let plane = width * height * element;
            out.extend(&level.elements[(slice >> m).min(d - 1) * plane..][..plane]);
        }
    }
    out
}

fn volume(code: u16, pixels: [u32; 3], mips: u8, rows: Vec<Vec<u8>>) -> Spec {
    let bodies = rows.into_iter().map(Body::Zlib).collect();
    let mut spec = spec(pixels[0], pixels[1], code, 1, mips, bodies);
    (spec.dimension, spec.slices) = (3, pixels[2] as u16);
    spec
}

fn saved(pack: &TexturePack) -> Vec<u8> {
    let mut bytes = Vec::new();
    pack.save_to(&mut bytes).expect("the pack saves");
    bytes
}

/// Saves `pack`, reopens it and checks every texture reads as it did and a second save is the first.
fn saves_to_a_fixed_point(pack: &TexturePack) {
    let once = saved(pack);
    let (_dir, again) = open(&once);
    let again = again.expect("the saved pack opens");
    assert_eq!(again.entries, pack.entries);
    for index in 0..pack.entries.len() {
        assert_eq!(
            again.texture(index).expect("reads after a save"),
            pack.texture(index).expect("reads"),
            "texture {index}"
        );
    }
    assert!(
        saved(&again) == once,
        "saving a saved pack is byte-identical"
    );
}

#[test]
fn a_one_level_volume_of_each_element_size_reads_back_slice_by_slice_through_its_padding() {
    // Every side is padded to whole tiles: each volume is 2 by 2 by 2 tiles in two z-slabs.
    let cases: [(u16, &str, usize, [u32; 3], u32); 5] = [
        (0x1E8, VOLUME_1, 1, [20, 18, 17], 1),
        (0x1B0, VOLUME_2, 2, [12, 20, 18], 1),
        (0x0E0, VOLUME_4, 4, [10, 18, 9], 1),
        (0x050, VOLUME_8, 8, [9, 9, 9], 1),
        (0x298, VOLUME_16, 16, [20, 36, 9], 4),
    ];
    for (n, (code, pattern, element, pixels, side)) in cases.into_iter().enumerate() {
        let size = [
            pixels[0].div_ceil(side) as usize,
            pixels[1].div_ceil(side) as usize,
            pixels[2] as usize,
        ];
        let level = level3(size, element, n);
        let mut memory = vec![0; 0x8000];
        place3(&mut memory, &level, 0, 0x4000, pattern, element);
        let (_dir, pack) = pack_of(&[volume(code, pixels, 1, vec![memory])]);
        let texture = pack
            .texture(0)
            .unwrap_or_else(|e| panic!("case {n} ({code:#05x}) reads: {e}"));
        assert_eq!((texture.faces, texture.levels), (pixels[2], 1), "case {n}");
        assert!(
            texture.pixels == slices(&[level], size[2], element),
            "case {n} reads back the elements written"
        );
        assert!(texture
            .face(pixels[2] - 1)
            .is_some_and(|slice| slice.faces == 1));
        saves_to_a_fixed_point(&pack);
    }
}

#[test]
fn a_multi_level_volume_holds_its_chain_in_each_z_slab_and_its_small_levels_at_the_3d_slots() {
    // R8 32×32×32 in six levels: a z-slab is the tail tile, level 1's tile and level 0's four.
    let r8: Vec<Level3> = (0..6)
        .map(|m| level3([32 >> m, 32 >> m, 32 >> m], 1, 10 + m))
        .collect();
    let mut memory = vec![0; 2 * 0x6000];
    place3(&mut memory, &r8[0], 0x2000, 0x6000, VOLUME_1, 1);
    place3(&mut memory, &r8[1], 0x1000, 0x6000, VOLUME_1, 1);
    for (level, slot) in r8[2..].iter().zip(TAIL_3D) {
        place3(&mut memory, level, slot, 0x6000, VOLUME_1, 1);
    }
    let rows = memory.chunks(0x6000).map(<[u8]>::to_vec).collect();
    let r8_volume = volume(0x1E8, [32, 32, 32], 6, rows);

    // BC5 32×32×16 in six levels: level 0's two tiles, then five tail levels, slot 0 the last.
    let bc5: Vec<Level3> = (0..6)
        .map(|m| {
            let blocks = (32usize >> m).div_ceil(4);
            level3([blocks, blocks, (16 >> m).max(1)], 16, 20 + m)
        })
        .collect();
    let mut memory = vec![0; 2 * 0x3000];
    place3(&mut memory, &bc5[0], 0x1000, 0x3000, VOLUME_16, 16);
    for (level, slot) in bc5[1..].iter().zip(TAIL_3D) {
        place3(&mut memory, level, slot, 0x3000, VOLUME_16, 16);
    }
    let bc5_volume = volume(0x298, [32, 32, 16], 6, vec![memory]);

    let (_dir, pack) = pack_of(&[r8_volume, bc5_volume]);
    for (index, levels, depth, element) in [(0, &r8, 32, 1), (1, &bc5, 16, 16)] {
        let texture = pack.texture(index).expect("the volume reads");
        assert_eq!(
            (texture.faces, texture.levels),
            (depth as u32, 6),
            "volume {index}"
        );
        assert!(
            texture.pixels == slices(levels, depth, element),
            "volume {index} reads back every level written"
        );
    }
    saves_to_a_fixed_point(&pack);
}

#[test]
fn a_volume_whose_range_is_not_its_z_slabs_or_whose_tile_mode_is_not_measured_is_refused() {
    let level = level3([8, 8, 8], 8, 1);
    let mut memory = vec![0; 0x1000];
    place3(&mut memory, &level, 0, 0x1000, VOLUME_8, 8);
    let mut long = memory.clone();
    long.resize(0x2000, 0);
    let mut moded = volume(0x050, [8, 8, 8], 1, vec![memory]);
    moded.tile_mode = 2;
    let (_dir, pack) = pack_of(&[volume(0x050, [8, 8, 8], 1, vec![long]), moded]);
    assert!(matches!(pack.texture(0), Err(TtpfError::Malformed(_))));
    assert!(matches!(
        pack.check(1),
        Err(TtpfError::Volume {
            index: 1,
            tile_mode: 2
        })
    ));
}

#[test]
fn a_top_level_over_4_mib_joins_its_4_mib_rows_and_replaces_to_a_fixed_point() {
    // BC1 4096×4096 in 64 KiB tiles: level 1's 2 MiB row, then level 0's 8 MiB as two rows.
    let (top, next) = (blocks(1024, 1024, 8, 3), blocks(512, 512, 8, 4));
    let tiled = tile(&top, 1024, 1024, 8, TILES_64K_8);
    let rows = [
        tile(&next, 512, 512, 8, TILES_64K_8),
        tiled[..4 << 20].to_vec(),
        tiled[4 << 20..].to_vec(),
    ];
    let split = spec(4096, 4096, 0x238, 0, 2, rows.map(Body::Stored).into());
    let (_dir, pack) = pack_of(&[split]);
    let texture = pack.texture(0).expect("the split top level reads");
    assert_eq!(texture.levels, 2);
    assert!(texture.pixels == [top, next].concat(), "the two rows join");

    let starts = pack.starts();
    assert_eq!(
        (starts[2], starts[3]),
        (Start::Forcible, Start::Aligned),
        "the first row of the top level is the forcible one"
    );

    let mut replaced = pack.clone();
    let levels = [blocks(1024, 1024, 8, 5), blocks(512, 512, 8, 6)];
    replaced.replace_levels(0, &levels).expect("replaces");
    let (_dir, reopened) = open(&saved(&replaced));
    let reopened = reopened.expect("the saved pack opens");
    let read = reopened.texture(0).expect("reads after a save");
    assert!(
        read.pixels == levels.concat(),
        "reads back what was written"
    );
    saves_to_a_fixed_point(&reopened);
    saves_to_a_fixed_point(&pack);
}
