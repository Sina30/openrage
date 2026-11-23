use super::*;

/// Two triangles in the drawable's box, of weights one and three.
const TRIANGLES: [[[f32; 3]; 3]; 2] = [
    [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    [[0.5, 0.5, 0.0], [1.0, 0.25, 0.0], [0.25, 1.0, 0.0]],
];
const WEIGHTS: [f32; 2] = [1.0, 3.0];
const CODE: u32 = 0x2A6A_F408;

/// The triangle at bucket `j` of `n` triangles whose cumulative shares are `c`.
fn bucket(c: &[f32], j: usize, n: usize) -> u32 {
    let x = j.saturating_sub(1) as f32 / (4 * n + 1) as f32;
    (0..n).rev().find(|t| c[*t] <= x).unwrap_or(0) as u32
}

/// A record of one entry of [`TRIANGLES`] of `weights`, its tables derived from them; where it lies.
fn record_into(img: &mut Image, weights: [f32; 2]) -> usize {
    let n = TRIANGLES.len();
    let a = img.alloc(0x70 * n);
    for (t, (corners, w)) in TRIANGLES.iter().zip(weights).enumerate() {
        for (k, p) in corners.iter().enumerate() {
            img.vec3(a + 0x70 * t + 16 * k, *p);
            img.put(a + 0x70 * t + 16 * k + 12, &(1.0f32 / 3.0).to_le_bytes());
            img.vec3(a + 0x70 * t + 0x30 + 16 * k, [0.0, 0.0, 1.0]);
        }
        for l in 0..4 {
            img.put(a + 0x70 * t + 0x60 + 4 * l, &w.to_le_bytes());
        }
    }
    let sum: f32 = weights.iter().sum();
    let mut c = vec![0f32];
    for w in weights {
        c.push(c.last().copied().unwrap_or_default() + w / sum);
    }
    let floats = |img: &mut Image, values: &[f32]| {
        let at = img.alloc(4 * values.len());
        for (k, x) in values.iter().enumerate() {
            img.put(at + 4 * k, &x.to_le_bytes());
        }
        at
    };
    let (c_at, d_at) = match sum {
        0.0 => (floats(img, &[f32::NAN; 3]), floats(img, &[0.0; 2])),
        _ => {
            let d: Vec<f32> = c.windows(2).map(|s| 1.0 / (s[1] - s[0])).collect();
            (floats(img, &c), floats(img, &d))
        }
    };
    let e_at = img.alloc(4 * (4 * n + 1));
    for j in 0..=4 * n {
        let b = if sum == 0.0 {
            u32::MAX
        } else {
            bucket(&c, j, n)
        };
        img.u32(e_at + 4 * j, b);
    }
    let f = img.alloc(0xC0);
    let entry = img.alloc(0xB0);
    img.u32(entry + 0x10, 1);
    for (slot, at, count) in [
        (0x18, a, n),
        (0x38, c_at, n + 1),
        (0x48, d_at, n),
        (0x58, e_at, 4 * n + 1),
    ] {
        img.ptr(entry + slot, at);
        img.u32(entry + slot + 8, count as u32);
        img.u32(entry + slot + 0xC, count as u32);
    }
    img.ptr(entry + 0x68, f);
    for (o, x) in [
        (0x80, 0.0f32),
        (0x84, 0.0),
        (0x88, 0.0),
        (0x90, 1.0),
        (0x94, 1.0),
        (0x98, 0.0),
    ] {
        img.put(entry + o, &x.to_le_bytes());
    }
    img.put(entry + 0x9C, &(1.0f32 / 3.0).to_le_bytes());
    img.put(entry + 0x8C, &(1.0f32 / 3.0).to_le_bytes());
    img.u32(entry + 0xA0, 0x101);
    let codes = img.alloc(4);
    img.u32(codes, CODE);
    let entries = img.pointers(&[entry]);
    let record = img.alloc(0x40);
    img.u32(record + 0x18, 1);
    img.ptr(record + 0x20, codes);
    img.ptr(record + 0x30, entries);
    for o in [0x28, 0x2A, 0x38, 0x3A] {
        img.u16(record + o, 1);
    }
    record
}

fn with_record(weights: [f32; 2]) -> (Image, Vec<u8>, usize) {
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let record = record_into(&mut img, weights);
    img.ptr(parts.root + 0xD0, record);
    (img, phys, record)
}

fn surfaces_of(img: &Image, phys: &[u8]) -> Result<Vec<Surface>> {
    let d =
        read_drawable(&img.bytes, Some(phys)).expect("the drawable reads whatever its 0xD0 holds");
    *d.embedded.expect("a record at 0xD0")
}

#[test]
fn the_record_at_0xd0_reads_to_its_surfaces_triangles_and_weights() {
    let (img, phys, _) = with_record(WEIGHTS);
    let surfaces = surfaces_of(&img, &phys).expect("the surfaces read");
    assert_eq!(
        surfaces,
        [Surface {
            code: CODE,
            triangles: TRIANGLES.to_vec(),
            weights: WEIGHTS.to_vec(),
            neighbours: false,
            word: 0x101,
        }]
    );
    let d = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    assert_eq!(
        d.levels[0].as_ref().map(Vec::len),
        Some(1),
        "its level is its own"
    );
}

#[test]
fn surfaces_whose_tables_the_weights_do_not_give_are_refused_and_the_drawable_still_reads() {
    // Shares stated for weights one and three, where the triangles state three and one.
    let (mut img, phys, record) = with_record(WEIGHTS);
    let entry = u64_at(
        &img.bytes,
        u64_at(&img.bytes, record + 0x30) as usize & 0x0FFF_FFFF,
    );
    let a = u64_at(&img.bytes, (entry as usize & 0x0FFF_FFFF) + 0x18) as usize & 0x0FFF_FFFF;
    for (t, w) in [(0, 3.0f32), (1, 1.0)] {
        for l in 0..4 {
            img.put(a + 0x70 * t + 0x60 + 4 * l, &w.to_le_bytes());
        }
    }
    assert!(matches!(
        surfaces_of(&img, &phys),
        Err(HeaderlessError::Surfaces(_))
    ));

    // A reference count other than one.
    let (mut img, phys, record) = with_record(WEIGHTS);
    img.u32(record + 0x18, 2);
    assert!(matches!(
        surfaces_of(&img, &phys),
        Err(HeaderlessError::Surfaces(_))
    ));

    // A word at 0xD0 that is no pointer is no record; the drawable's root is the section's start.
    let (mut img, phys, _) = with_record(WEIGHTS);
    img.u64(0xD0, 0x4000_0000_3F80_0000);
    let d = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    assert!(d.embedded.is_none());
}

#[test]
fn an_entry_of_no_weight_holds_nan_shares_and_empty_buckets() {
    let (img, phys, _) = with_record([0.0, 0.0]);
    let surfaces = surfaces_of(&img, &phys).expect("the surfaces read");
    assert_eq!(surfaces[0].weights, [0.0, 0.0]);
}
