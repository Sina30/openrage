#[cfg(feature = "gta5")]
use super::super::TextureSource;
use super::*;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            let n = chunk.len();
            chunk.copy_from_slice(&bytes[..n]);
        }
    }
}

#[derive(Default, Debug)]
struct Delta {
    max: [u8; 4],
    sum: [u64; 4],
    count: u64,
}

impl Delta {
    fn accumulate(&mut self, a: &RgbaImage, b: &RgbaImage) {
        for (pa, pb) in a
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.data.as_chunks::<4>().0.iter())
        {
            for c in 0..4 {
                let d = pa[c].abs_diff(pb[c]);
                if d > self.max[c] {
                    self.max[c] = d;
                }
                self.sum[c] += d as u64;
            }
            self.count += 1;
        }
    }

    fn mean(&self, channel: usize) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum[channel] as f64 / self.count as f64
        }
    }

    fn max_rgb(&self) -> u8 {
        self.max[0].max(self.max[1]).max(self.max[2])
    }
}

fn random_blocks(rng: &mut Rng, width: u32, height: u32, block_bytes: usize) -> Vec<u8> {
    let blocks = (width.div_ceil(4) * height.div_ceil(4)) as usize;
    let mut data = vec![0u8; blocks * block_bytes];
    rng.fill(&mut data);
    data
}

fn dxtex_bc1_palette(c0: u16, c1: u16, only_opaque: bool) -> [[u8; 4]; 4] {
    let expand = |c: u16| -> [f32; 3] {
        let r = ((c >> 11) & 0x1F) as f32;
        let g = ((c >> 5) & 0x3F) as f32;
        let b = (c & 0x1F) as f32;
        [r * 255.0 / 31.0, g * 255.0 / 63.0, b * 255.0 / 31.0]
    };
    let q = |v: f32| -> u8 { (v + 0.5).clamp(0.0, 255.0) as u8 };

    let e0 = expand(c0);
    let e1 = expand(c1);
    let solid = |e: [f32; 3]| [q(e[0]), q(e[1]), q(e[2]), 255u8];

    if c0 > c1 || only_opaque {
        let lerp = |t: f32| {
            [
                q(e0[0] + (e1[0] - e0[0]) * t),
                q(e0[1] + (e1[1] - e0[1]) * t),
                q(e0[2] + (e1[2] - e0[2]) * t),
                255,
            ]
        };
        [solid(e0), solid(e1), lerp(1.0 / 3.0), lerp(2.0 / 3.0)]
    } else {
        let mid = [
            q((e0[0] + e1[0]) * 0.5),
            q((e0[1] + e1[1]) * 0.5),
            q((e0[2] + e1[2]) * 0.5),
            255,
        ];
        [solid(e0), solid(e1), mid, [0, 0, 0, 0]]
    }
}

fn dxtex_bc1(data: &[u8], width: u32, height: u32, only_opaque: bool) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    let blocks_x = width.div_ceil(4);
    for by in 0..height.div_ceil(4) {
        for bx in 0..blocks_x {
            let off = ((by * blocks_x + bx) as usize) * 8;
            let block = &data[off..off + 8];
            let c0 = u16::from_le_bytes([block[0], block[1]]);
            let c1 = u16::from_le_bytes([block[2], block[3]]);
            let palette = dxtex_bc1_palette(c0, c1, only_opaque);
            let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x < width && y < height {
                        let i = (py * 4 + px) * 2;
                        image.set_pixel(x, y, palette[((indices >> i) & 3) as usize]);
                    }
                }
            }
        }
    }
    image
}

#[test]
fn gate3_bcdec_is_closer_to_directxtex_than_the_old_decoder() {
    let mut rng = Rng::new(0xB01D_FACE);
    let data = random_blocks(&mut rng, 64, 64, 8);

    let reference = dxtex_bc1(&data, 64, 64, false);
    let current = decode_texture_data(GrcTextureFormat::DXT1, 64, 64, &data).unwrap();
    let old = super::legacy::decode(GrcTextureFormat::DXT1, 64, 64, &data).unwrap();

    let mut new_delta = Delta::default();
    new_delta.accumulate(&current, &reference);
    let mut old_delta = Delta::default();
    old_delta.accumulate(&old, &reference);

    let new_mean = (0..3).map(|c| new_delta.mean(c)).sum::<f64>() / 3.0;
    let old_mean = (0..3).map(|c| old_delta.mean(c)).sum::<f64>() / 3.0;

    println!(
        "BC1 vs DirectXTex reference: bcdec_rs mean |d| = {new_mean:.4} (max {}), \
         pre-WS3 mean |d| = {old_mean:.4} (max {})",
        new_delta.max_rgb(),
        old_delta.max_rgb()
    );

    assert!(
        new_mean <= old_mean,
        "the new backend drifted further from the reference than the decoder it replaced \
         ({new_mean:.4} vs {old_mean:.4}) — the migration is not an improvement"
    );
    assert!(
        new_delta.max_rgb() <= 1,
        "bcdec_rs differs from the reference by {} LSB, expected at most 1",
        new_delta.max_rgb()
    );
}

fn exact_value_palette(a0: u8, a1: u8) -> [u8; 8] {
    let (x, y) = (a0 as f32, a1 as f32);
    let q = |v: f32| (v + 0.5) as u8;
    let mut p = [0u8; 8];
    p[0] = a0;
    p[1] = a1;
    if a0 > a1 {
        for (i, slot) in p.iter_mut().enumerate().skip(2) {
            let w = (8 - i) as f32;
            *slot = q((w * x + (7.0 - w) * y) / 7.0);
        }
    } else {
        for (i, slot) in p.iter_mut().enumerate().take(6).skip(2) {
            let w = (6 - i) as f32;
            *slot = q((w * x + (5.0 - w) * y) / 5.0);
        }
        p[6] = 0;
        p[7] = 255;
    }
    p
}

fn bc4_palette_probe(a0: u8, a1: u8) -> [u8; 8] {
    let mut block = [0u8; 8];
    block[0] = a0;
    block[1] = a1;
    let mut bits: u64 = 0;
    for i in 0..16u64 {
        bits |= (i % 8) << (i * 3);
    }
    block[2..8].copy_from_slice(&bits.to_le_bytes()[..6]);
    block
}

#[test]
fn gate3_bcdec_rounds_the_alpha_palette_correctly() {
    let mut ours_error = 0u32;
    let mut legacy_error = 0u32;

    for (a0, a1) in [(255u8, 0u8), (200, 100), (17, 240), (128, 127), (3, 2)] {
        let block = bc4_palette_probe(a0, a1);
        let expected = exact_value_palette(a0, a1);

        let ours = decode_texture_data(GrcTextureFormat::DXT5A, 4, 4, &block).unwrap();
        let old = super::legacy::decode(GrcTextureFormat::DXT5A, 4, 4, &block).unwrap();

        for i in 0..16usize {
            let want = expected[i % 8];
            let (x, y) = ((i % 4) as u32, (i / 4) as u32);
            ours_error += ours.get_pixel(x, y)[0].abs_diff(want) as u32;
            legacy_error += old.get_pixel(x, y)[0].abs_diff(want) as u32;
        }
    }

    println!("alpha palette total |error|: bcdec_rs {ours_error}, pre-WS3 {legacy_error}");
    assert_eq!(
        ours_error, 0,
        "bcdec_rs should reproduce the exact palette; off by {ours_error} in total"
    );
    assert!(
        legacy_error > 0,
        "the frozen reference was expected to truncate — if it no longer does, \
         VALUE_BUDGET can drop to 0"
    );
}

const RGB_BUDGET: u8 = 2;

const VALUE_BUDGET: u8 = 1;

#[test]
fn gate1_bc1_stays_within_budget() {
    let mut rng = Rng::new(0x1111_1111);
    let data = random_blocks(&mut rng, 64, 64, 8);

    let old = super::legacy::decode(GrcTextureFormat::DXT1, 64, 64, &data).unwrap();
    let new = decode_texture_data(GrcTextureFormat::DXT1, 64, 64, &data).unwrap();

    let mut delta = Delta::default();
    delta.accumulate(&old, &new);
    println!(
        "BC1 delta: max RGB {} mean {:.4}/{:.4}/{:.4}, alpha max {}",
        delta.max_rgb(),
        delta.mean(0),
        delta.mean(1),
        delta.mean(2),
        delta.max[3]
    );

    assert!(
        delta.max_rgb() <= RGB_BUDGET,
        "BC1 RGB moved by {} LSB, budget is {RGB_BUDGET}",
        delta.max_rgb()
    );
    assert_eq!(
        delta.max[3], 0,
        "BC1 punch-through alpha changed — cutout geometry would render solid"
    );
}

#[test]
fn gate1_bc2_stays_within_budget() {
    let mut rng = Rng::new(0x2222_2222);
    let data = random_blocks(&mut rng, 64, 64, 16);

    let old = super::legacy::decode(GrcTextureFormat::DXT3, 64, 64, &data).unwrap();
    let new = decode_texture_data(GrcTextureFormat::DXT3, 64, 64, &data).unwrap();

    let mut delta = Delta::default();
    delta.accumulate(&old, &new);
    println!(
        "BC2 delta: max RGB {} alpha {}",
        delta.max_rgb(),
        delta.max[3]
    );

    assert!(delta.max_rgb() <= RGB_BUDGET, "BC2 RGB moved too far");
    assert_eq!(delta.max[3], 0, "BC2 explicit alpha must be bit-exact");
}

#[test]
fn gate1_bc3_stays_within_budget() {
    let mut rng = Rng::new(0x3333_3333);
    let data = random_blocks(&mut rng, 64, 64, 16);

    let old = super::legacy::decode(GrcTextureFormat::DXT5, 64, 64, &data).unwrap();
    let new = decode_texture_data(GrcTextureFormat::DXT5, 64, 64, &data).unwrap();

    let mut delta = Delta::default();
    delta.accumulate(&old, &new);
    println!(
        "BC3 delta: max RGB {} alpha {}",
        delta.max_rgb(),
        delta.max[3]
    );

    assert!(delta.max_rgb() <= RGB_BUDGET, "BC3 RGB moved too far");
    assert!(
        delta.max[3] <= VALUE_BUDGET,
        "BC3 alpha moved by {} LSB, budget is {VALUE_BUDGET}",
        delta.max[3]
    );
}

#[test]
fn gate1_bc4_value_holds_only_placement_moved() {
    let mut rng = Rng::new(0x4444_4444);
    let data = random_blocks(&mut rng, 64, 64, 8);

    let old = super::legacy::decode(GrcTextureFormat::DXT5A, 64, 64, &data).unwrap();
    let new = decode_texture_data(GrcTextureFormat::DXT5A, 64, 64, &data).unwrap();

    for (o, n) in old
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(new.data.as_chunks::<4>().0.iter())
    {
        assert!(
            o[0].abs_diff(n[0]) <= VALUE_BUDGET,
            "BC4 value moved from {} to {}",
            o[0],
            n[0]
        );
        assert_eq!([n[1], n[2], n[3]], [0, 0, 255], "BC4 should be red-only");
        assert_eq!(o[1], o[0], "frozen reference should have broadcast grey");
    }
}

#[test]
fn gate1_bc5_values_hold_only_blue_moved() {
    let mut rng = Rng::new(0x5555_5555);
    let data = random_blocks(&mut rng, 64, 64, 16);

    let old = super::legacy::decode(GrcTextureFormat::DXN, 64, 64, &data).unwrap();
    let new = decode_texture_data(GrcTextureFormat::DXN, 64, 64, &data).unwrap();

    for (o, n) in old
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(new.data.as_chunks::<4>().0.iter())
    {
        assert!(
            o[0].abs_diff(n[0]) <= VALUE_BUDGET && o[1].abs_diff(n[1]) <= VALUE_BUDGET,
            "BC5 R/G moved from {:?} to {:?}",
            [o[0], o[1]],
            [n[0], n[1]]
        );
        assert_eq!([n[2], n[3]], [0, 255], "BC5 blue should be 0, not 128");
        assert_eq!(o[2], 128, "frozen reference should have written blue = 128");
    }
}

#[test]
fn gate1_partial_blocks_match_the_frozen_reference() {
    for (w, h) in [(1, 1), (3, 5), (7, 4), (4, 7), (13, 9), (17, 33)] {
        let mut rng = Rng::new(0x0EDE_0000u64.wrapping_add(w as u64));
        let data = random_blocks(&mut rng, w, h, 16);

        let old = super::legacy::decode(GrcTextureFormat::DXT5, w, h, &data).unwrap();
        let new = decode_texture_data(GrcTextureFormat::DXT5, w, h, &data).unwrap();

        let mut delta = Delta::default();
        delta.accumulate(&old, &new);
        assert!(
            delta.max_rgb() <= RGB_BUDGET && delta.max[3] <= VALUE_BUDGET,
            "{w}x{h}: partial-block handling diverged (max rgb {}, alpha {})",
            delta.max_rgb(),
            delta.max[3]
        );
        assert_eq!(new.data.len(), (w as usize) * (h as usize) * 4);
    }
}

fn oracle(
    decoder: impl Fn(&[u8], usize, usize, &mut [u32]) -> core::result::Result<(), &'static str>,
    data: &[u8],
    width: u32,
    height: u32,
) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    decode_packed_bgra(data, width, height, &mut image, "oracle", decoder).unwrap();
    image
}

#[test]
fn gate2_bc1_matches_the_independent_oracle() {
    let mut rng = Rng::new(0xA1A1_A1A1);
    let data = random_blocks(&mut rng, 64, 64, 8);

    let theirs = oracle(texture2ddecoder::decode_bc1a, &data, 64, 64);
    let ours = decode_texture_data(GrcTextureFormat::DXT1, 64, 64, &data).unwrap();

    let mut delta = Delta::default();
    delta.accumulate(&ours, &theirs);
    println!("BC1 vs texture2ddecoder: max RGB {}", delta.max_rgb());

    assert!(delta.max_rgb() <= RGB_BUDGET);
    assert_eq!(delta.max[3], 0, "alpha must agree exactly");
}

#[test]
fn gate2_bc7_matches_the_independent_oracle() {
    let mut rng = Rng::new(0xB7B7_B7B7);
    let data = random_blocks(&mut rng, 128, 128, 16);

    let theirs = oracle(texture2ddecoder::decode_bc7, &data, 128, 128);
    let ours = decode_texture_data(GrcTextureFormat::BC7, 128, 128, &data).unwrap();

    assert_eq!(
        ours.data, theirs.data,
        "BC7 decoders disagree; one of them is wrong"
    );
}

#[test]
fn gate2_every_bc7_mode_matches_the_oracle() {
    for mode in 0..8u32 {
        let mut rng = Rng::new(0x0BC7_0000 | mode as u64);
        let mut data = vec![0u8; 16 * 16];
        for block in data.chunks_mut(16) {
            rng.fill(block);
            let clear_low = !(((1u32 << (mode + 1)) - 1) as u8);
            block[0] = (block[0] & clear_low) | (1u8 << mode);
        }

        let theirs = oracle(texture2ddecoder::decode_bc7, &data, 16, 16);
        let ours = decode_texture_data(GrcTextureFormat::BC7, 16, 16, &data).unwrap();
        assert_eq!(ours.data, theirs.data, "BC7 mode {mode} decoders disagree");
    }
}

#[test]
fn gate2_bc4_and_bc5_match_the_independent_oracle() {
    let mut rng = Rng::new(0xC4C5_C4C5);

    let bc4 = random_blocks(&mut rng, 32, 32, 8);
    let theirs = oracle(texture2ddecoder::decode_bc4, &bc4, 32, 32);
    let ours = decode_texture_data(GrcTextureFormat::DXT5A, 32, 32, &bc4).unwrap();
    for (o, t) in ours
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(theirs.data.as_chunks::<4>().0.iter())
    {
        assert!(
            o[0].abs_diff(t[0]) <= VALUE_BUDGET,
            "BC4 red channel disagrees: {} vs {}",
            o[0],
            t[0]
        );
    }

    let bc5 = random_blocks(&mut rng, 32, 32, 16);
    let theirs = oracle(texture2ddecoder::decode_bc5, &bc5, 32, 32);
    let ours = decode_texture_data(GrcTextureFormat::DXN, 32, 32, &bc5).unwrap();
    for (o, t) in ours
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(theirs.data.as_chunks::<4>().0.iter())
    {
        assert!(
            o[0].abs_diff(t[0]) <= VALUE_BUDGET && o[1].abs_diff(t[1]) <= VALUE_BUDGET,
            "BC5 red/green disagree: {:?} vs {:?}",
            [o[0], o[1]],
            [t[0], t[1]]
        );
    }
}

#[test]
fn gate2_bc2_bc3_oracle_exclusion_is_earned() {
    // c0 = 0x0000 (black), c1 = 0xF800 (red), so c0 < c1 and the
    // BC1 rule would read index 3 as transparent black; inside BC3 the colour
    // block is always four-colour, so index 3 is two thirds of red.
    let mut block = [0u8; 16];
    block[..8].fill(0xFF);
    block[8..10].copy_from_slice(&0x0000u16.to_le_bytes());
    block[10..12].copy_from_slice(&0xF800u16.to_le_bytes());
    block[12..16].fill(0xFF);

    let ours = decode_texture_data(GrcTextureFormat::DXT5, 4, 4, &block).unwrap();
    let theirs = oracle(texture2ddecoder::decode_bc3, &block, 4, 4);

    let ours_px = ours.get_pixel(0, 0);
    assert!(
        ours_px[0] > 140 && ours_px[0] < 190,
        "expected 2/3 of full red, got {ours_px:?}"
    );

    let theirs_px = theirs.get_pixel(0, 0);
    assert_eq!(
        [theirs_px[0], theirs_px[1], theirs_px[2]],
        [0, 0, 0],
        "texture2ddecoder no longer diverges on BC3 — re-evaluate whether it \
         can rejoin the oracle set"
    );
}

#[cfg(feature = "gta5")]
fn corpus_root() -> std::path::PathBuf {
    std::env::var("OPENRAGE_TEXTURE_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| or_test_support::fixtures_root(env!("CARGO_MANIFEST_DIR")))
}

#[test]
#[cfg(feature = "gta5")]
fn gate4_real_textures_stay_within_budget() {
    let root = corpus_root();

    if !root.exists() {
        or_test_support::skip_corpus(
            "texture corpus decode",
            format_args!("no texture corpus at {}", root.display()),
        );
        return;
    }

    let mut textures_seen = 0usize;
    let mut checked = 0usize;
    let mut worst_rgb = 0u8;
    let mut worst_value = 0u8;
    let mut offenders = Vec::new();

    for path in collect_files(&root) {
        let Some(textures) = load_textures(&path) else {
            continue;
        };

        for texture in textures {
            if !matches!(
                texture.format,
                GrcTextureFormat::DXT1
                    | GrcTextureFormat::DXT3
                    | GrcTextureFormat::DXT5
                    | GrcTextureFormat::DXT5A
                    | GrcTextureFormat::DXN
            ) {
                continue;
            }
            textures_seen += 1;

            for level in 0..texture.stored_mip_count() {
                let Some((w, h)) = texture.mip_dimensions(level) else {
                    continue;
                };
                let Some(data) = texture.mip_level_bytes(level) else {
                    continue;
                };
                let (Ok(new), Some(old)) = (
                    decode_texture_data(texture.format, w, h, data),
                    super::legacy::decode(texture.format, w, h, data),
                ) else {
                    continue;
                };

                let mut delta = Delta::default();
                delta.accumulate(&old, &new);
                checked += 1;

                let (rgb, value) = match texture.format {
                    GrcTextureFormat::DXT5A => (0, delta.max[0]),
                    GrcTextureFormat::DXN => (0, delta.max[0].max(delta.max[1])),
                    _ => (delta.max_rgb(), delta.max[3]),
                };
                worst_rgb = worst_rgb.max(rgb);
                worst_value = worst_value.max(value);

                if rgb > RGB_BUDGET || value > VALUE_BUDGET {
                    offenders.push(format!(
                        "{}:{} ({:?} mip {level} {w}x{h}) rgb {rgb} value {value}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        texture.name,
                        texture.format
                    ));
                }
            }
        }
    }

    println!(
        "gate 4: {textures_seen} real textures ({checked} mip levels) compared from {}; \
         worst RGB delta {worst_rgb}, worst single-channel delta {worst_value}",
        root.display()
    );

    if checked == 0 {
        or_test_support::skip_corpus(
            "block-compressed texture decode",
            "corpus contained no block-compressed textures",
        );
        return;
    }
    assert!(
        offenders.is_empty(),
        "{} texture(s) exceeded the delta budget:\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
}

#[test]
#[cfg(feature = "gta5")]
fn gate4_real_bc7_matches_the_independent_oracle() {
    let root = corpus_root();
    if !root.exists() {
        or_test_support::skip_corpus(
            "texture corpus decode",
            format_args!("no texture corpus at {}", root.display()),
        );
        return;
    }

    let mut textures_seen = 0usize;
    let mut checked = 0usize;
    let mut disagreements = Vec::new();

    for path in collect_files(&root) {
        let Some(textures) = load_textures(&path) else {
            continue;
        };

        for texture in textures {
            if texture.format != GrcTextureFormat::BC7 {
                continue;
            }
            textures_seen += 1;

            for level in 0..texture.stored_mip_count() {
                let Some((w, h)) = texture.mip_dimensions(level) else {
                    continue;
                };
                let Some(data) = texture.mip_level_bytes(level) else {
                    continue;
                };
                let Ok(ours) = decode_texture_data(GrcTextureFormat::BC7, w, h, data) else {
                    continue;
                };

                checked += 1;
                let theirs = oracle(texture2ddecoder::decode_bc7, data, w, h);
                if ours.data != theirs.data {
                    let differing = ours
                        .data
                        .iter()
                        .zip(&theirs.data)
                        .filter(|(a, b)| a != b)
                        .count();
                    disagreements.push(format!(
                        "{}:{} (mip {level} {w}x{h}) {differing} byte(s) differ",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        texture.name
                    ));
                }
            }
        }
    }

    println!(
        "gate 4 (BC7): {textures_seen} shipped BC7 textures ({checked} mip levels) \
         cross-checked against texture2ddecoder from {}",
        root.display()
    );

    if checked == 0 {
        or_test_support::skip_corpus("BC7 texture decode", "corpus contained no BC7 textures");
        return;
    }
    assert!(
        disagreements.is_empty(),
        "{} shipped BC7 texture(s) disagree with the oracle:\n  {}",
        disagreements.len(),
        disagreements.join("\n  ")
    );
}

#[cfg(feature = "gta5")]
fn collect_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    or_test_support::Corpus::with_root(root, "ytd")
        .all_targets()
        .find()
}

/// A shipped texture's stored mips, as [`TextureSource`] states them.
#[cfg(feature = "gta5")]
struct Shipped {
    name: String,
    format: GrcTextureFormat,
    size: (u32, u32),
    levels: u32,
    payload: Vec<u8>,
}

#[cfg(feature = "gta5")]
impl Shipped {
    fn of(name: &str, texture: &dyn TextureSource) -> Self {
        Self {
            name: name.to_string(),
            format: texture.pixel_format(),
            size: texture.stored_size(),
            levels: texture.stored_levels(),
            payload: texture.payload().to_vec(),
        }
    }

    fn stored_mip_count(&self) -> u32 {
        self.levels
    }

    fn mip_dimensions(&self, level: u32) -> Option<(u32, u32)> {
        (level < self.levels).then(|| self.format.mip_dimensions(self.size.0, self.size.1, level))
    }

    fn mip_level_bytes(&self, level: u32) -> Option<&[u8]> {
        let (w, h) = self.size;
        (level < self.levels)
            .then(|| self.payload.get(self.format.mip_level_range(w, h, level)))
            .flatten()
    }
}

/// Every texture of a `.ytd` of either target, opened owned.
#[cfg(feature = "gta5")]
fn load_textures(path: &std::path::Path) -> Option<Vec<Shipped>> {
    Some(match crate::open(path).ok()? {
        crate::Asset::TextureDict(dict) => dict
            .entries
            .iter()
            .flatten()
            .map(|t| Shipped::of(&t.name, &**t))
            .collect(),
        #[cfg(feature = "gen9")]
        crate::Asset::TextureDictGen9(dict) => dict
            .entries
            .iter()
            .flatten()
            .map(|t| Shipped::of(&t.name, &**t))
            .collect(),
        _ => return None,
    })
}

#[test]
#[cfg(feature = "gta5")]
fn gate5_visual_dump() {
    if std::env::var("OPENRAGE_TEXTURE_DUMP").is_err() {
        return;
    }

    let root = corpus_root();
    let Some(workspace) = or_test_support::workspace_root(env!("CARGO_MANIFEST_DIR")) else {
        return;
    };
    let out = workspace.join("target/ws3-visual");
    if !root.exists() || std::fs::create_dir_all(&out).is_err() {
        return;
    }

    let mut written = 0;
    for path in collect_files(&root) {
        let Some(textures) = load_textures(&path) else {
            continue;
        };
        for texture in textures.iter().take(4) {
            let Some((w, h)) = texture.mip_dimensions(0) else {
                continue;
            };
            let Some(data) = texture.mip_level_bytes(0) else {
                continue;
            };
            let Ok(new) = decode_texture_data(texture.format, w, h, data) else {
                continue;
            };
            let stem = format!("{:?}_{}", texture.format, sanitize(&texture.name));

            let view = ChannelView::preview_default(texture.format);
            let _ = super::super::save_png(
                out.join(format!("{stem}.after.png")),
                &new.clone().with_channel_view(view),
            );
            if let Some(old) = super::legacy::decode(texture.format, w, h, data) {
                let _ = super::super::save_png(out.join(format!("{stem}.before.png")), &old);
            }
            written += 1;
        }
        if written > 40 {
            break;
        }
    }
    println!(
        "gate 5: wrote {written} before/after pairs to {}",
        out.display()
    );
}

#[cfg(feature = "gta5")]
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[test]
fn bc7_mode5_decodes_a_known_colour() {
    let mut bits = BitWriter::default();
    bits.push(1 << 5, 6);
    bits.push(0, 2);
    for _ in 0..3 {
        bits.push(0x7F, 7);
        bits.push(0x7F, 7);
    }
    bits.push(0xFF, 8);
    bits.push(0xFF, 8);

    let block = bits.finish();
    let image = decode_texture_data(GrcTextureFormat::BC7, 4, 4, &block).unwrap();

    for y in 0..4 {
        for x in 0..4 {
            let p = image.get_pixel(x, y);
            assert_eq!(
                p,
                [255, 255, 255, 255],
                "BC7 mode 5 flat white block decoded as {p:?} at ({x},{y})"
            );
        }
    }
}

#[test]
fn bc7_is_no_longer_a_placeholder() {
    let mut rng = Rng::new(0xDEAD_BC77);
    let data = random_blocks(&mut rng, 16, 16, 16);
    let image = decode_texture_data(GrcTextureFormat::BC7, 16, 16, &data).unwrap();

    let mut distinct = std::collections::HashSet::new();
    for px in image.data.as_chunks::<4>().0.iter() {
        distinct.insert([px[0], px[1], px[2], px[3]]);
    }
    assert!(
        distinct.len() > 32,
        "only {} distinct pixels — this looks like the old placeholder",
        distinct.len()
    );
}

#[test]
fn bc7_reserved_mode_is_transparent_black() {
    let block = [0u8; 16];
    let image = decode_texture_data(GrcTextureFormat::BC7, 4, 4, &block).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(image.get_pixel(x, y), [0, 0, 0, 0]);
        }
    }
}

#[derive(Default)]
struct BitWriter {
    bits: Vec<bool>,
}

impl BitWriter {
    fn push(&mut self, value: u32, count: u32) {
        for i in 0..count {
            self.bits.push((value >> i) & 1 == 1);
        }
    }

    fn finish(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (i, bit) in self.bits.iter().enumerate().take(128) {
            if *bit {
                out[i / 8] |= 1 << (i % 8);
            }
        }
        out
    }
}

#[test]
fn truncated_block_data_is_an_error_not_black_pixels() {
    for (format, block_bytes) in [
        (GrcTextureFormat::DXT1, 8),
        (GrcTextureFormat::DXT3, 16),
        (GrcTextureFormat::DXT5, 16),
        (GrcTextureFormat::DXT5A, 8),
        (GrcTextureFormat::DXN, 16),
        (GrcTextureFormat::BC7, 16),
    ] {
        let data = vec![0u8; block_bytes * 3];
        let err = decode_texture_data(format, 8, 8, &data).unwrap_err();
        assert!(
            matches!(err, IoError::InsufficientData { .. }),
            "{format:?} accepted truncated data: {err:?}"
        );
    }
}

#[test]
fn absurd_dimensions_are_refused_before_allocating() {
    for (w, h) in [
        (MAX_DECODE_DIMENSION + 1, 4),
        (4, MAX_DECODE_DIMENSION + 1),
        (u32::MAX, u32::MAX),
    ] {
        let err = decode_texture_data(GrcTextureFormat::DXT1, w, h, &[]).unwrap_err();
        assert!(
            matches!(err, IoError::InvalidData { .. }),
            "{w}x{h} was not refused: {err:?}"
        );
        let err = decode_texture_hdr(GrcTextureFormat::BC6, w, h, &[]).unwrap_err();
        assert!(
            matches!(err, IoError::InvalidData { .. }),
            "{w}x{h} was not refused on the HDR path: {err:?}"
        );
    }
}

#[test]
fn the_dimension_bound_admits_every_real_texture_size() {
    let err = decode_texture_data(
        GrcTextureFormat::DXT1,
        MAX_DECODE_DIMENSION,
        MAX_DECODE_DIMENSION,
        &[],
    )
    .unwrap_err();
    assert!(
        matches!(err, IoError::InsufficientData { .. }),
        "the limit itself must pass the dimension check and fail on data: {err:?}"
    );
}

#[test]
fn truncated_uncompressed_data_is_an_error() {
    let err = decode_texture_data(GrcTextureFormat::A8R8G8B8, 4, 4, &[0u8; 16]).unwrap_err();
    assert!(matches!(err, IoError::InsufficientData { .. }));
}

#[test]
fn no_format_falls_through_to_the_magenta_placeholder() {
    use super::super::format::DecodeSupport;
    use GrcTextureFormat as F;

    let all = [
        F::None,
        F::R5G6B5,
        F::A8R8G8B8,
        F::R16F,
        F::R32F,
        F::A2B10G10R10,
        F::A2B10G10R10ATI,
        F::A16B16G16R16F,
        F::G16R16,
        F::G16R16F,
        F::A32B32G32R32F,
        F::A16B16G16R16FNoExpand,
        F::A16B16G16R16,
        F::L8,
        F::L16,
        F::G8R8,
        F::A1R5G5B5,
        F::D24S8,
        F::A4R4G4B4,
        F::G32R32F,
        F::D16,
        F::D32F,
        F::X8R8G8B8,
        F::Null,
        F::X24G8,
        F::A8,
        F::R11G11B10F,
        F::D32FS8,
        F::X32S8,
        F::DXT1,
        F::DXT3,
        F::DXT5,
        F::DXT5A,
        F::DXN,
        F::BC6,
        F::BC7,
        F::A8B8G8R8Snorm,
        F::A8B8G8R8,
        F::Etc2Rgb,
        F::Etc2Rgba,
        F::EacR11,
        F::EacRg11,
    ];

    for format in all {
        assert_ne!(
            format.decode_support(),
            DecodeSupport::Missing,
            "{format:?} has no decoder"
        );

        let data = vec![0x80u8; 4096];
        let result = decode_texture_data(format, 8, 8, &data);

        match format.decode_support() {
            DecodeSupport::Exact | DecodeSupport::Tonemapped => {
                let image = result.unwrap_or_else(|e| panic!("{format:?} failed to decode: {e}"));
                assert!(
                    !image
                        .data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| *p == [255, 0, 255, 255]),
                    "{format:?} produced the magenta placeholder"
                );
            }
            DecodeSupport::NotSampleable | DecodeSupport::Empty => {
                assert!(result.is_err(), "{format:?} should refuse to decode");
            }
            DecodeSupport::Missing => unreachable!(),
        }
    }
}

#[test]
fn depth_formats_explain_themselves() {
    let err = decode_texture_data(GrcTextureFormat::D24S8, 4, 4, &[0u8; 4096]).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("depth") && message.contains("not a sampleable image"),
        "unhelpful error: {message}"
    );
}

#[test]
fn packed_16bit_formats_decode_primaries() {
    let data: Vec<u8> = [0xF800u16, 0x07E0, 0x001F, 0xFFFF]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let image = decode_texture_data(GrcTextureFormat::R5G6B5, 4, 1, &data).unwrap();
    assert_eq!(image.get_pixel(0, 0), [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(1, 0), [0, 255, 0, 255]);
    assert_eq!(image.get_pixel(2, 0), [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(3, 0), [255, 255, 255, 255]);

    let data: Vec<u8> = [0x7C00u16, 0xFC00]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let image = decode_texture_data(GrcTextureFormat::A1R5G5B5, 2, 1, &data).unwrap();
    assert_eq!(image.get_pixel(0, 0), [255, 0, 0, 0], "alpha bit clear");
    assert_eq!(image.get_pixel(1, 0), [255, 0, 0, 255], "alpha bit set");

    let data = 0x8F00u16.to_le_bytes();
    let image = decode_texture_data(GrcTextureFormat::A4R4G4B4, 1, 1, &data).unwrap();
    assert_eq!(image.get_pixel(0, 0), [255, 0, 0, 136]);
}

#[test]
fn x8r8g8b8_forces_opaque() {
    let data = [10u8, 20, 30, 0];
    let image = decode_texture_data(GrcTextureFormat::X8R8G8B8, 1, 1, &data).unwrap();
    assert_eq!(image.get_pixel(0, 0), [30, 20, 10, 255]);
}

#[test]
fn a8_roundtrips_through_encode() {
    let data: Vec<u8> = (0..16).map(|i| (i * 17) as u8).collect();
    let image = decode_texture_data(GrcTextureFormat::A8, 4, 4, &data).unwrap();
    assert_eq!(image.get_pixel(0, 0), [0, 0, 0, 0]);
    assert_eq!(image.get_pixel(1, 0), [0, 0, 0, 17]);

    let reencoded = encode_texture_data(GrcTextureFormat::A8, 4, 4, &image).unwrap();
    assert_eq!(reencoded, data, "A8 must survive decode -> encode");
}

#[test]
fn l8_roundtrips_through_encode() {
    let data: Vec<u8> = (0..16).map(|i| (i * 17) as u8).collect();
    let image = decode_texture_data(GrcTextureFormat::L8, 4, 4, &data).unwrap();
    let reencoded = encode_texture_data(GrcTextureFormat::L8, 4, 4, &image).unwrap();
    assert_eq!(reencoded, data, "L8 must survive decode -> encode");
}

#[test]
fn half_float_roundtrips_through_the_bit_converter() {
    use or_io::{f32_to_half, half_to_f32};
    for value in [0.0f32, 1.0, 0.5, 2.0, -1.0, 65504.0, 1.0 / 3.0, 1e-5] {
        let back = half_to_f32(f32_to_half(value));
        let tolerance = value.abs() * 1e-3 + 1e-7;
        assert!(
            (back - value).abs() <= tolerance,
            "f16 round-trip lost {value}: got {back}"
        );
    }
}

#[test]
fn rgba16f_decodes_at_full_precision() {
    use or_io::f32_to_half;
    let values = [4.0f32, 0.5, 0.25, 1.0];
    let data: Vec<u8> = values
        .iter()
        .flat_map(|v| f32_to_half(*v).to_le_bytes())
        .collect();

    let hdr = decode_texture_hdr(GrcTextureFormat::A16B16G16R16F, 1, 1, &data).unwrap();
    assert_eq!(hdr.data[0], 4.0, "highlight was clamped during decode");
    assert_eq!(hdr.data[1], 0.5);

    let preview = hdr.tonemap(Tonemap::ClampSrgb);
    assert_eq!(preview.get_pixel(0, 0)[0], 255);
    assert!(preview.is_srgb, "tone-mapped output is sRGB-encoded");

    let reinhard = hdr.tonemap(Tonemap::Reinhard);
    assert!(reinhard.get_pixel(0, 0)[0] < 255);
}

#[test]
fn bc6h_decodes_to_real_floats() {
    let data = vec![0u8; 16];
    let hdr = decode_texture_hdr(GrcTextureFormat::BC6, 4, 4, &data).unwrap();
    assert_eq!(hdr.data.len(), 4 * 4 * 4);
    assert!(hdr.data.iter().all(|v| v.is_finite()));
    for px in hdr.data.as_chunks::<4>().0.iter() {
        assert_eq!(px[3], 1.0);
    }
}

#[test]
fn hdr_decode_rejects_integer_formats() {
    let err = decode_texture_hdr(GrcTextureFormat::DXT1, 4, 4, &[0u8; 64]).unwrap_err();
    assert!(err.to_string().contains("not an HDR format"));
}

#[test]
fn channel_view_defaults_suit_the_format() {
    use GrcTextureFormat as F;
    assert_eq!(
        ChannelView::preview_default(F::DXT5A),
        ChannelView::Grey(Channel::R)
    );
    assert_eq!(
        ChannelView::preview_default(F::DXN),
        ChannelView::NormalMapRg
    );
    assert_eq!(ChannelView::preview_default(F::A8), ChannelView::AlphaOnly);
    assert_eq!(ChannelView::preview_default(F::DXT1), ChannelView::Rgba);
}

#[test]
fn grey_view_restores_the_old_bc4_preview() {
    let mut rng = Rng::new(0x9999);
    let data = random_blocks(&mut rng, 16, 16, 8);

    let old = super::legacy::decode(GrcTextureFormat::DXT5A, 16, 16, &data).unwrap();
    let new = decode_texture_data(GrcTextureFormat::DXT5A, 16, 16, &data)
        .unwrap()
        .with_channel_view(ChannelView::Grey(Channel::R));

    let mut delta = Delta::default();
    delta.accumulate(&old, &new);
    assert!(
        delta.max_rgb() <= VALUE_BUDGET && delta.max[3] == 0,
        "grey view diverged from the old preview by {}",
        delta.max_rgb()
    );
}

#[test]
fn normal_map_view_reconstructs_z() {
    let mut image = RgbaImage::new(1, 1);
    image.set_pixel(0, 0, [128, 128, 0, 255]);
    image.apply_channel_view(ChannelView::NormalMapRg);
    let p = image.get_pixel(0, 0);
    assert_eq!([p[0], p[1]], [128, 128], "R/G must be preserved");
    assert!(
        p[2] > 250,
        "flat normal should reconstruct z near 1.0: {p:?}"
    );
}

#[test]
fn rgba_view_is_a_no_op() {
    let mut rng = Rng::new(0x4242);
    let mut data = vec![0u8; 64];
    rng.fill(&mut data);
    let image = RgbaImage {
        width: 4,
        height: 4,
        data: data.clone(),
        is_srgb: false,
    };
    assert_eq!(image.with_channel_view(ChannelView::Rgba).data, data);
}

#[test]
fn etc2_rgb_decodes_known_block() {
    let block: [u8; 8] = [0xA0, 0x78, 0x50, 0x00, 0x00, 0x00, 0x00, 0x00];
    let image = decode_texture_data(GrcTextureFormat::Etc2Rgb, 4, 4, &block).unwrap();
    assert_eq!(image.width, 4);
    assert_eq!(image.height, 4);
    assert_eq!(image.data.len(), 4 * 4 * 4);
    for y in 0..4 {
        for x in 0..4 {
            let p = image.get_pixel(x, y);
            assert_eq!(p[3], 255, "ETC2 RGB pixel {x},{y} should be opaque: {p:?}");
        }
    }
}

#[test]
fn bc4_decodes_exact_ramp() {
    let block: [u8; 8] = [0xFF, 0x00, 0x88, 0xC6, 0xFA, 0x88, 0xC6, 0xFA];
    let image = decode_texture_data(GrcTextureFormat::DXT5A, 4, 4, &block).unwrap();
    let expected_rows = [
        [255u8, 0, 219, 182],
        [146, 109, 73, 36],
        [255, 0, 219, 182],
        [146, 109, 73, 36],
    ];
    for (y, row) in expected_rows.iter().enumerate() {
        for (x, expected) in row.iter().enumerate() {
            let p = image.get_pixel(x as u32, y as u32);
            assert_eq!(
                p,
                [*expected, 0, 0, 255],
                "pixel ({x}, {y}) expected red {expected}, got {p:?}"
            );
        }
    }
}

#[test]
fn bc4_decodes_known_block() {
    let block: [u8; 8] = [200, 100, 0, 0, 0, 0, 0, 0];
    let image = decode_texture_data(GrcTextureFormat::DXT5A, 4, 4, &block).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(image.get_pixel(x, y), [200, 0, 0, 255]);
        }
    }
}

#[test]
fn bc5_decodes_known_block() {
    let block: [u8; 16] = [200, 100, 0, 0, 0, 0, 0, 0, 100, 50, 0, 0, 0, 0, 0, 0];
    let image = decode_texture_data(GrcTextureFormat::DXN, 4, 4, &block).unwrap();
    assert_eq!(image.data.len(), 4 * 4 * 4);
    assert_eq!(image.get_pixel(2, 2), [200, 100, 0, 255]);
}

#[test]
fn bc7_srgb_flag_propagates() {
    struct Bc7(bool);
    impl super::super::TextureSource for Bc7 {
        fn pixel_format(&self) -> GrcTextureFormat {
            GrcTextureFormat::BC7
        }
        fn is_srgb(&self) -> bool {
            self.0
        }
        fn stored_size(&self) -> (u32, u32) {
            (4, 4)
        }
        fn stored_levels(&self) -> u32 {
            1
        }
        fn payload(&self) -> &[u8] {
            &[0; 16]
        }
    }
    let image = super::super::decode_level(&Bc7(true), 0).unwrap();
    assert!(image.is_srgb, "decode_level should propagate the sRGB flag");

    let linear = super::super::decode_level(&Bc7(false), 0).unwrap();
    assert!(
        !linear.is_srgb,
        "non-sRGB texture should NOT carry the flag"
    );
}

#[test]
fn eac_r11_decodes_known_block() {
    let block: [u8; 8] = [0x80, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let image = decode_texture_data(GrcTextureFormat::EacR11, 4, 4, &block).unwrap();
    assert_eq!(image.data.len(), 4 * 4 * 4);
    for y in 0..4 {
        for x in 0..4 {
            assert!(image.get_pixel(x, y)[3] > 0, "alpha should be non-zero");
        }
    }
}

#[test]
fn test_rgb565_conversion() {
    assert_eq!(rgb565_to_rgba(0xF800), [255, 0, 0, 255]);
    assert_eq!(rgb565_to_rgba(0x07E0), [0, 255, 0, 255]);
    assert_eq!(rgb565_to_rgba(0x001F), [0, 0, 255, 255]);
}

#[test]
fn test_decode_bgra_roundtrip() {
    let mut input = Vec::new();
    for i in 0..16 {
        input.push((i * 16) as u8);
        input.push((i * 8) as u8);
        input.push((i * 4) as u8);
        input.push(255u8);
    }

    let image = decode_texture_data(GrcTextureFormat::A8R8G8B8, 4, 4, &input).unwrap();
    let pixel = image.get_pixel(0, 0);
    assert_eq!(pixel[0], 0);
    assert_eq!(pixel[1], 0);
    assert_eq!(pixel[2], 0);
    assert_eq!(pixel[3], 255);
}

#[test]
fn test_encode_bgra_roundtrip() {
    let mut rgba = RgbaImage::new(2, 2);
    rgba.set_pixel(0, 0, [255, 128, 64, 255]);
    rgba.set_pixel(1, 0, [0, 255, 0, 128]);
    rgba.set_pixel(0, 1, [64, 64, 64, 255]);
    rgba.set_pixel(1, 1, [128, 128, 128, 0]);

    let encoded = encode_texture_data(GrcTextureFormat::A8R8G8B8, 2, 2, &rgba).unwrap();
    assert_eq!(encoded[0], 64);
    assert_eq!(encoded[1], 128);
    assert_eq!(encoded[2], 255);
    assert_eq!(encoded[3], 255);
}
