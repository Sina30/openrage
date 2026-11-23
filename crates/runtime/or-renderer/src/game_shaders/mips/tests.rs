use super::*;

fn grey(width: u32, height: u32, value: u8) -> Vec<u8> {
    [value, value, value, 255].repeat((width * height) as usize)
}

#[test]
fn a_texture_has_a_level_for_each_halving_down_to_one_texel() {
    for (w, h, levels) in [
        (1, 1, 1),
        (2, 1, 2),
        (4, 4, 3),
        (5, 3, 3),
        (256, 64, 9),
        (1024, 1024, 11),
    ] {
        assert_eq!(count(w, h), levels, "{w}x{h}");
        assert_eq!(
            chain(w, h, &grey(w, h, 9), false).len() as u32 + 1,
            levels,
            "{w}x{h}"
        );
    }
}

#[test]
fn each_level_is_half_the_size_of_the_last_and_stops_at_one() {
    let sizes: Vec<(u32, u32)> = chain(8, 2, &grey(8, 2, 0), false)
        .iter()
        .map(|(w, h, data)| {
            assert_eq!(data.len() as u32, w * h * 4);
            (*w, *h)
        })
        .collect();
    assert_eq!(sizes, [(4, 1), (2, 1), (1, 1)]);
    let odd: Vec<(u32, u32)> = chain(5, 3, &grey(5, 3, 0), false)
        .iter()
        .map(|(w, h, _)| (*w, *h))
        .collect();
    assert_eq!(odd, [(2, 1), (1, 1)]);
    assert!(chain(1, 1, &grey(1, 1, 0), false).is_empty());
}

#[test]
fn a_texel_is_the_average_of_the_four_it_covers() {
    // Two rows of two: 0, 100 / 200, 40 average to 85
    let rgba = [
        [0, 0, 0, 0],
        [100, 100, 100, 100],
        [200, 200, 200, 200],
        [40, 40, 40, 40],
    ]
    .concat();
    let levels = chain(2, 2, &rgba, false);
    assert_eq!(levels, [(1, 1, vec![85, 85, 85, 85])]);
}

#[test]
fn colour_is_averaged_in_linear_light_when_the_texture_is_srgb_and_alpha_as_it_is() {
    // Black and white side by side: a linear half is 188 in sRGB, and a half of the alpha is 128
    let rgba = [[0, 0, 0, 0], [255, 255, 255, 255]].concat();
    let plain = chain(2, 1, &rgba, false);
    assert_eq!(plain[0].2, [128, 128, 128, 128]);
    let srgb = chain(2, 1, &rgba, true);
    assert_eq!(srgb[0].2, [188, 188, 188, 128]);
}

#[test]
fn a_side_of_one_texel_repeats_it_rather_than_reading_past_it() {
    // Two wide and one high: the row below does not exist, so the average is of the row there is
    let rgba = [[10, 20, 30, 40], [30, 40, 50, 60]].concat();
    assert_eq!(chain(2, 1, &rgba, false)[0].2, [20, 30, 40, 50]);
}

#[test]
fn a_flat_texture_stays_flat_at_every_level() {
    for srgb in [false, true] {
        for (_, _, data) in chain(16, 16, &grey(16, 16, 77), srgb) {
            assert!(data.chunks(4).all(|t| t == [77, 77, 77, 255]), "{srgb}");
        }
    }
}

/// The average of four texels written out plainly, one texel and one channel at a time.
fn plain_chain(width: u32, height: u32, rgba: &[u8], srgb: bool) -> Vec<Level> {
    let mut levels: Vec<Level> = Vec::new();
    let (mut w, mut h) = (width.max(1), height.max(1));
    let mut source = rgba.to_vec();
    let linear: Vec<f32> = (0..=255).map(srgb_to_linear).collect();
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::new();
        for y in 0..nh {
            for x in 0..nw {
                let xs = [(2 * x).min(w - 1), (2 * x + 1).min(w - 1)];
                let ys = [(2 * y).min(h - 1), (2 * y + 1).min(h - 1)];
                for channel in 0..4 {
                    let mut sum = 0.0;
                    for sy in ys {
                        for sx in xs {
                            let value = source[((sy * w + sx) * 4) as usize + channel];
                            sum += if srgb && channel < 3 {
                                linear[usize::from(value)]
                            } else {
                                f32::from(value) / 255.0
                            };
                        }
                    }
                    let mean = sum / 4.0;
                    next.push(if srgb && channel < 3 {
                        linear_to_srgb(mean)
                    } else {
                        (mean * 255.0).round() as u8
                    });
                }
            }
        }
        levels.push((nw, nh, next.clone()));
        (w, h, source) = (nw, nh, next);
    }
    levels
}

#[test]
fn the_levels_are_what_averaging_each_texel_plainly_gives_for_any_size_and_either_colour_space() {
    let mut state = 0x2545_f491_u32;
    let mut random = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 24) as u8
    };
    for (w, h) in [
        (1, 1),
        (1, 7),
        (9, 1),
        (2, 2),
        (5, 3),
        (16, 16),
        (37, 20),
        (64, 33),
    ] {
        let rgba: Vec<u8> = (0..w * h * 4).map(|_| random()).collect();
        for srgb in [false, true] {
            assert_eq!(
                chain(w, h, &rgba, srgb),
                plain_chain(w, h, &rgba, srgb),
                "{w}x{h} {srgb}"
            );
        }
    }
    // Every byte of colour as a flat texture, so each step of the encoding is read
    for value in 0..=255u8 {
        let flat = [value, value, value, value].repeat(4);
        assert_eq!(
            chain(2, 2, &flat, true),
            plain_chain(2, 2, &flat, true),
            "{value}"
        );
    }
}
