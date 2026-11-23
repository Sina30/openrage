use crate::error::{IoError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MipFilter {
    #[default]
    Box,
    Lanczos3,
}

impl MipFilter {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "box" | "average" => Some(Self::Box),
            "lanczos" | "lanczos3" => Some(Self::Lanczos3),
            _ => None,
        }
    }
}

pub fn max_mip_levels(width: u32, height: u32) -> u32 {
    let largest = width.max(height).max(1);
    32 - largest.leading_zeros()
}

/// How a texture's colour relates to its alpha down the mip chain.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AlphaRule {
    /// The colour of the image is already multiplied by its alpha, and every mip is stored so.
    pub premultiplied: bool,
    /// The alpha a cutout is tested against: every mip keeps the share of texels that reach it at the top level.
    pub coverage: Option<f32>,
}

impl AlphaRule {
    /// Whether this is the rule a chain follows when none is stated.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

pub fn generate_mip_chain(
    width: u32,
    height: u32,
    rgba: &[u8],
    levels: Option<u32>,
    filter: MipFilter,
    srgb: bool,
) -> Result<Vec<Vec<u8>>> {
    generate_mip_chain_with(
        width,
        height,
        rgba,
        levels,
        filter,
        srgb,
        AlphaRule::default(),
    )
}

/// A mip chain whose alpha follows `alpha`; the default rule is [`generate_mip_chain`]'s.
pub fn generate_mip_chain_with(
    width: u32,
    height: u32,
    rgba: &[u8],
    levels: Option<u32>,
    filter: MipFilter,
    srgb: bool,
    alpha: AlphaRule,
) -> Result<Vec<Vec<u8>>> {
    if let Some(cutoff) = alpha.coverage {
        if !(cutoff > 0.0 && cutoff <= 1.0) {
            return Err(IoError::InvalidData {
                context: format!("an alpha cutoff of {cutoff} is not above 0 and at most 1"),
            });
        }
    }
    if width == 0 || height == 0 {
        return Err(IoError::InvalidData {
            context: format!("cannot build a mip chain for a {width}x{height} image"),
        });
    }
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(IoError::InvalidData {
            context: format!(
                "mip chain input is {} bytes, expected {} for a {}x{} RGBA8 image",
                rgba.len(),
                expected,
                width,
                height
            ),
        });
    }

    let full = max_mip_levels(width, height);
    let count = levels.unwrap_or(full).clamp(1, full);

    let mut chain = Vec::with_capacity(count as usize);
    chain.push(rgba.to_vec());
    if count == 1 {
        return Ok(chain);
    }

    // ⚠ The chain is always made from straight colour weighted by alpha, so that a texel that is not there adds no colour; a
    // premultiplied image is taken out of its alpha first and put back into it at every level.
    let straight = alpha.premultiplied.then(|| unpremultiplied(rgba));
    let mut src = to_linear_premultiplied(straight.as_deref().unwrap_or(rgba), srgb);
    let (mut sw, mut sh) = (width, height);
    let reached = alpha.coverage.map(|cutoff| {
        let bar = alpha_bar(cutoff);
        let top = rgba.as_chunks::<4>().0.iter();
        (cutoff, top.filter(|t| t[3] >= bar).count())
    });

    for level in 1..count {
        let dw = (width >> level).max(1);
        let dh = (height >> level).max(1);
        let dst = match filter {
            MipFilter::Box => box_downsample(&src, sw, sh, dw, dh),
            MipFilter::Lanczos3 => lanczos3_resample(&src, sw, sh, dw, dh),
        };
        let mut shown = dst.clone();
        if let Some((cutoff, texels)) = reached {
            let share = texels as f64 / (rgba.len() / 4) as f64;
            keep_coverage(&mut shown, cutoff, share);
        }
        let mut bytes = from_linear_premultiplied(&shown, srgb);
        if alpha.premultiplied {
            bytes = premultiplied(&bytes);
        }
        chain.push(bytes);
        src = dst;
        sw = dw;
        sh = dh;
    }

    Ok(chain)
}

/// The lowest 8-bit alpha that reaches `cutoff`.
fn alpha_bar(cutoff: f32) -> u8 {
    (cutoff * 255.0).ceil().clamp(1.0, 255.0) as u8
}

/// `rgba` with its colour divided by its alpha; a texel with no alpha has no colour to give back.
fn unpremultiplied(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for texel in out.as_chunks_mut::<4>().0 {
        let a = u32::from(texel[3]);
        for c in &mut texel[..3] {
            *c = (u32::from(*c) * 255 + a / 2)
                .checked_div(a)
                .map_or(0, |v| v.min(255) as u8);
        }
    }
    out
}

/// `rgba` with its colour multiplied by its alpha.
fn premultiplied(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for texel in out.as_chunks_mut::<4>().0 {
        let a = u32::from(texel[3]);
        for c in &mut texel[..3] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
    out
}

/// Scale the alpha of `level`, which holds colour already multiplied by it, so that `share` of its texels reach `cutoff`, with the
/// straight colour of each texel kept: a mip of a cutout that has lost its thin edges to the average gets them back.
fn keep_coverage(level: &mut [f32], cutoff: f32, share: f64) {
    let texels = level.len() / 4;
    let wanted = (share * texels as f64).round() as usize;
    if wanted == 0 || wanted >= texels {
        return;
    }
    let mut alphas: Vec<f32> = level.as_chunks::<4>().0.iter().map(|t| t[3]).collect();
    alphas.sort_unstable_by(|a, b| b.total_cmp(a));
    let edge = alphas[wanted - 1];
    if edge <= 0.0 {
        return;
    }
    // The 8-bit alpha that reaches the cutoff is the one that rounds up to it.
    let needed = (f32::from(alpha_bar(cutoff)) - 0.5) / 255.0;
    let scale = needed / edge * (1.0 + 1e-4);
    for t in level.as_chunks_mut::<4>().0 {
        let a = t[3];
        if a <= 0.0 {
            continue;
        }
        let scaled = (a * scale).min(1.0);
        let by = scaled / a;
        for c in &mut t[..3] {
            *c *= by;
        }
        t[3] = scaled;
    }
}

/// `rgba`, 8 bits a channel and `from` wide and high, resampled to `to` in the linear light the mips are made in.
///
/// ⚠ A side that grows is always Lanczos-resampled, since a box filter only averages and would repeat texels; `filter` is how a
/// side that shrinks does.
pub fn resize_rgba(
    rgba: &[u8],
    from: (u32, u32),
    to: (u32, u32),
    filter: MipFilter,
    srgb: bool,
) -> Result<Vec<u8>> {
    let ((sw, sh), (dw, dh)) = (from, to);
    if [sw, sh, dw, dh].contains(&0) {
        return Err(IoError::InvalidData {
            context: format!("cannot resize a {sw}x{sh} image to {dw}x{dh}"),
        });
    }
    let expected = (sw as usize) * (sh as usize) * 4;
    if rgba.len() != expected {
        return Err(IoError::InvalidData {
            context: format!(
                "resize input is {} bytes, expected {expected} for a {sw}x{sh} RGBA8 image",
                rgba.len()
            ),
        });
    }
    if from == to {
        return Ok(rgba.to_vec());
    }
    let src = to_linear_premultiplied(rgba, srgb);
    let grows = dw > sw || dh > sh;
    let dst = match (filter, grows) {
        (MipFilter::Box, false) => box_downsample(&src, sw, sh, dw, dh),
        _ => lanczos3_resample(&src, sw, sh, dw, dh),
    };
    Ok(from_linear_premultiplied(&dst, srgb))
}

fn to_linear_premultiplied(rgba: &[u8], srgb: bool) -> Vec<f32> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.as_chunks::<4>().0.iter() {
        let a = px[3] as f32 / 255.0;
        for &c in &px[..3] {
            let linear = if srgb {
                SRGB_TO_LINEAR[c as usize]
            } else {
                c as f32 / 255.0
            };
            out.push(linear * a);
        }
        out.push(a);
    }
    out
}

fn from_linear_premultiplied(buf: &[f32], srgb: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len());
    for px in buf.as_chunks::<4>().0.iter() {
        let a = px[3].clamp(0.0, 1.0);
        let inv_a = if a > 0.0 { 1.0 / a } else { 0.0 };
        for &c in &px[..3] {
            let straight = (c * inv_a).clamp(0.0, 1.0);
            out.push(if srgb {
                linear_to_srgb_u8(straight)
            } else {
                unorm_to_u8(straight)
            });
        }
        out.push(unorm_to_u8(a));
    }
    out
}

fn box_downsample(src: &[f32], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<f32> {
    let x_ratio = sw as f32 / dw as f32;
    let y_ratio = sh as f32 / dh as f32;
    let mut out = vec![0.0f32; (dw as usize) * (dh as usize) * 4];

    for dy in 0..dh {
        let y0 = (dy as f32 * y_ratio).floor() as u32;
        let y1 = (((dy + 1) as f32 * y_ratio).ceil() as u32)
            .min(sh)
            .max(y0 + 1);
        for dx in 0..dw {
            let x0 = (dx as f32 * x_ratio).floor() as u32;
            let x1 = (((dx + 1) as f32 * x_ratio).ceil() as u32)
                .min(sw)
                .max(x0 + 1);

            let mut acc = [0.0f32; 4];
            let mut n = 0.0f32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * sw + sx) * 4) as usize;
                    for (a, s) in acc.iter_mut().zip(&src[i..i + 4]) {
                        *a += *s;
                    }
                    n += 1.0;
                }
            }

            let o = ((dy * dw + dx) * 4) as usize;
            for (dst, a) in out[o..o + 4].iter_mut().zip(acc) {
                *dst = a / n;
            }
        }
    }

    out
}

fn lanczos3_resample(src: &[f32], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<f32> {
    let horizontal = resample_axis(src, sw, sh, dw, Axis::X);
    resample_axis(&horizontal, dw, sh, dh, Axis::Y)
}

enum Axis {
    X,
    Y,
}

fn resample_axis(src: &[f32], sw: u32, sh: u32, dst_len: u32, axis: Axis) -> Vec<f32> {
    const LOBES: f32 = 3.0;

    let src_len = match axis {
        Axis::X => sw,
        Axis::Y => sh,
    };
    let (dw, dh) = match axis {
        Axis::X => (dst_len, sh),
        Axis::Y => (sw, dst_len),
    };
    let mut out = vec![0.0f32; (dw as usize) * (dh as usize) * 4];

    let scale = src_len as f32 / dst_len as f32;
    let support = LOBES * scale.max(1.0);
    let inv_scale = 1.0 / scale.max(1.0);

    for d in 0..dst_len {
        let center = (d as f32 + 0.5) * scale;
        let first = ((center - support).floor().max(0.0)) as u32;
        let last = (((center + support).ceil()) as i64).min(src_len as i64) as u32;

        let mut weights = Vec::with_capacity((last - first) as usize);
        let mut total = 0.0f32;
        for s in first..last {
            let w = lanczos3(((s as f32 + 0.5) - center) * inv_scale);
            weights.push(w);
            total += w;
        }
        if total == 0.0 {
            continue;
        }

        let other_len = match axis {
            Axis::X => sh,
            Axis::Y => sw,
        };
        for o in 0..other_len {
            let mut acc = [0.0f32; 4];
            for (k, s) in (first..last).enumerate() {
                let (sx, sy) = match axis {
                    Axis::X => (s, o),
                    Axis::Y => (o, s),
                };
                let i = ((sy * sw + sx) * 4) as usize;
                let w = weights[k];
                for (a, v) in acc.iter_mut().zip(&src[i..i + 4]) {
                    *a += *v * w;
                }
            }
            let (dx, dy) = match axis {
                Axis::X => (d, o),
                Axis::Y => (o, d),
            };
            let oi = ((dy * dw + dx) * 4) as usize;
            for (dst, a) in out[oi..oi + 4].iter_mut().zip(acc) {
                *dst = (a / total).clamp(0.0, 1.0);
            }
        }
    }

    out
}

fn lanczos3(x: f32) -> f32 {
    const LOBES: f32 = 3.0;
    if x == 0.0 {
        return 1.0;
    }
    if x.abs() >= LOBES {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    (px.sin() / px) * ((px / LOBES).sin() / (px / LOBES))
}

#[inline]
fn unorm_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

pub(crate) fn linear_to_srgb_u8(linear: f32) -> u8 {
    let v = linear.clamp(0.0, 1.0);
    let hi = SRGB_TO_LINEAR.partition_point(|&t| t < v);
    if hi == 0 {
        return 0;
    }
    if hi >= SRGB_TO_LINEAR.len() {
        return 255;
    }
    let lo = hi - 1;
    if v - SRGB_TO_LINEAR[lo] <= SRGB_TO_LINEAR[hi] - v {
        lo as u8
    } else {
        hi as u8
    }
}

#[rustfmt::skip]
static SRGB_TO_LINEAR: [f32; 256] = [
    0.0, 0.000303527, 0.000607054, 0.000910581,
    0.001214108, 0.001517635, 0.001821162, 0.0021246888,
    0.002428216, 0.0027317428, 0.00303527, 0.0033465358,
    0.0036765074, 0.004024717, 0.004391442, 0.0047769533,
    0.0051815165, 0.0056053917, 0.006048833, 0.0065120906,
    0.00699541, 0.007499032, 0.008023193, 0.008568126,
    0.009134059, 0.009721218, 0.010329823, 0.010960094,
    0.011612245, 0.012286488, 0.0129830325, 0.013702083,
    0.014443844, 0.015208514, 0.015996294, 0.016807375,
    0.017641954, 0.01850022, 0.019382361, 0.020288562,
    0.02121901, 0.022173885, 0.023153367, 0.024157632,
    0.02518686, 0.026241222, 0.027320892, 0.02842604,
    0.029556835, 0.030713445, 0.031896032, 0.033104766,
    0.034339808, 0.035601314, 0.03688945, 0.038204372,
    0.039546236, 0.0409152, 0.04231141, 0.04373503,
    0.045186203, 0.046665087, 0.048171826, 0.049706567,
    0.051269457, 0.052860647, 0.054480277, 0.05612849,
    0.05780543, 0.059511237, 0.061246052, 0.063010015,
    0.064803265, 0.06662594, 0.06847817, 0.070360094,
    0.07227185, 0.07421357, 0.07618538, 0.07818742,
    0.08021982, 0.08228271, 0.08437621, 0.08650046,
    0.08865558, 0.09084171, 0.093058966, 0.09530747,
    0.09758735, 0.099898726, 0.10224173, 0.104616486,
    0.107023105, 0.10946171, 0.11193243, 0.114435375,
    0.116970666, 0.11953843, 0.122138776, 0.12477182,
    0.12743768, 0.13013647, 0.13286832, 0.13563333,
    0.13843161, 0.14126329, 0.14412847, 0.14702727,
    0.14995979, 0.15292615, 0.15592647, 0.15896083,
    0.16202937, 0.1651322, 0.1682694, 0.17144111,
    0.1746474, 0.17788842, 0.18116425, 0.18447499,
    0.18782078, 0.19120169, 0.19461784, 0.19806932,
    0.20155625, 0.20507874, 0.20863687, 0.21223076,
    0.2158605, 0.2195262, 0.22322796, 0.22696587,
    0.23074006, 0.23455058, 0.23839757, 0.24228112,
    0.24620132, 0.25015828, 0.2541521, 0.25818285,
    0.26225066, 0.2663556, 0.2704978, 0.2746773,
    0.27889428, 0.28314874, 0.28744084, 0.29177064,
    0.29613826, 0.30054379, 0.3049873, 0.30946892,
    0.31398872, 0.31854677, 0.3231432, 0.3277781,
    0.33245152, 0.33716363, 0.34191442, 0.34670407,
    0.3515326, 0.35640013, 0.3613068, 0.3662526,
    0.3712377, 0.37626213, 0.38132602, 0.38642943,
    0.39157248, 0.39675522, 0.40197778, 0.4072402,
    0.4125426, 0.41788507, 0.42326766, 0.4286905,
    0.43415365, 0.43965718, 0.4452012, 0.4507858,
    0.45641103, 0.462077, 0.4677838, 0.47353148,
    0.47932017, 0.48514995, 0.49102086, 0.49693298,
    0.5028865, 0.50888133, 0.5149177, 0.52099556,
    0.5271151, 0.5332764, 0.5394795, 0.54572445,
    0.55201143, 0.5583404, 0.5647115, 0.57112485,
    0.57758045, 0.58407843, 0.59061885, 0.59720176,
    0.60382736, 0.61049557, 0.6172066, 0.6239604,
    0.63075715, 0.63759685, 0.6444797, 0.65140563,
    0.65837485, 0.6653873, 0.67244315, 0.6795425,
    0.6866853, 0.69387174, 0.7011019, 0.70837575,
    0.7156935, 0.7230551, 0.73046076, 0.7379104,
    0.7454042, 0.7529422, 0.7605245, 0.76815116,
    0.7758222, 0.7835378, 0.7912979, 0.7991027,
    0.80695224, 0.8148466, 0.82278574, 0.8307699,
    0.838799, 0.8468732, 0.8549926, 0.8631572,
    0.8713671, 0.8796224, 0.8879231, 0.8962694,
    0.9046612, 0.91309863, 0.92158186, 0.9301109,
    0.9386857, 0.9473065, 0.9559733, 0.9646863,
    0.9734453, 0.9822506, 0.9911021, 1.0,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, px: [u8; 4]) -> Vec<u8> {
        px.iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect()
    }

    #[test]
    fn a_resize_keeps_a_solid_colour_in_either_direction_and_is_the_mip_when_it_halves() {
        let rgba = solid(8, 8, [200, 100, 50, 255]);
        for (to, filter) in [
            ((4, 4), MipFilter::Box),
            ((16, 16), MipFilter::Box),
            ((5, 3), MipFilter::Lanczos3),
        ] {
            let out = resize_rgba(&rgba, (8, 8), to, filter, true).unwrap();
            assert_eq!(out.len(), (to.0 * to.1 * 4) as usize);
            for px in out.as_chunks::<4>().0 {
                assert_eq!(*px, [200, 100, 50, 255], "{to:?}");
            }
        }
        // A halving by the box is the first mip of the chain.
        let ramp: Vec<u8> = (0..8 * 8)
            .flat_map(|i| [(i * 4) as u8, (i * 3) as u8, i as u8, 255])
            .collect();
        let chain = generate_mip_chain(8, 8, &ramp, Some(2), MipFilter::Box, true).unwrap();
        assert_eq!(
            resize_rgba(&ramp, (8, 8), (4, 4), MipFilter::Box, true).unwrap(),
            chain[1]
        );
        assert_eq!(
            resize_rgba(&ramp, (8, 8), (8, 8), MipFilter::Box, true).unwrap(),
            ramp
        );
        assert!(resize_rgba(&ramp, (8, 8), (0, 4), MipFilter::Box, true).is_err());
        assert!(resize_rgba(&ramp[1..], (8, 8), (4, 4), MipFilter::Box, true).is_err());
    }

    #[test]
    fn max_levels_matches_full_chain() {
        assert_eq!(max_mip_levels(1, 1), 1);
        assert_eq!(max_mip_levels(2, 2), 2);
        assert_eq!(max_mip_levels(256, 256), 9);
        assert_eq!(max_mip_levels(256, 64), 9);
        assert_eq!(max_mip_levels(1024, 1024), 11);
    }

    #[test]
    fn chain_halves_each_level() {
        let rgba = solid(64, 32, [10, 20, 30, 255]);
        let chain = generate_mip_chain(64, 32, &rgba, None, MipFilter::Box, false).unwrap();
        assert_eq!(chain.len(), 7);
        let expected = [(64, 32), (32, 16), (16, 8), (8, 4), (4, 2), (2, 1), (1, 1)];
        for (level, (w, h)) in expected.iter().enumerate() {
            assert_eq!(
                chain[level].len(),
                (w * h * 4) as usize,
                "level {level} has the wrong byte count"
            );
        }
    }

    #[test]
    fn level_zero_is_untouched() {
        let rgba = solid(8, 8, [1, 2, 3, 4]);
        let chain = generate_mip_chain(8, 8, &rgba, None, MipFilter::Lanczos3, true).unwrap();
        assert_eq!(chain[0], rgba);
    }

    #[test]
    fn level_count_is_clamped_not_rejected() {
        let rgba = solid(4, 4, [0, 0, 0, 255]);
        let chain = generate_mip_chain(4, 4, &rgba, Some(99), MipFilter::Box, false).unwrap();
        assert_eq!(chain.len(), 3);

        let single = generate_mip_chain(4, 4, &rgba, Some(1), MipFilter::Box, false).unwrap();
        assert_eq!(single.len(), 1);
    }

    #[test]
    fn solid_colour_survives_every_level() {
        for srgb in [false, true] {
            for filter in [MipFilter::Box, MipFilter::Lanczos3] {
                let rgba = solid(32, 32, [200, 100, 50, 255]);
                let chain = generate_mip_chain(32, 32, &rgba, None, filter, srgb).unwrap();
                for (level, data) in chain.iter().enumerate() {
                    for px in data.as_chunks::<4>().0.iter() {
                        assert_eq!(
                            *px,
                            [200, 100, 50, 255],
                            "level {level} drifted (srgb={srgb}, filter={filter:?})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn srgb_average_is_brighter_than_naive_average() {
        let mut rgba = vec![0u8; 2 * 2 * 4];
        for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let v = if i % 2 == 0 { 0 } else { 255 };
            px.copy_from_slice(&[v, v, v, 255]);
        }
        let chain = generate_mip_chain(2, 2, &rgba, None, MipFilter::Box, true).unwrap();
        let level1 = &chain[1];
        assert!(
            level1[0] > 180 && level1[0] < 195,
            "linear-space average should land near 188, got {}",
            level1[0]
        );

        let naive = generate_mip_chain(2, 2, &rgba, None, MipFilter::Box, false).unwrap();
        assert_eq!(naive[1][0], 128);
    }

    #[test]
    fn transparent_texels_do_not_bleed_colour() {
        let rgba: Vec<u8> = [
            [255u8, 0, 0, 255],
            [0, 255, 0, 0],
            [255, 0, 0, 255],
            [0, 255, 0, 0],
        ]
        .concat();
        let chain = generate_mip_chain(2, 2, &rgba, None, MipFilter::Box, false).unwrap();
        let px = &chain[1];
        assert_eq!(px[0], 255, "red channel should survive");
        assert_eq!(px[1], 0, "transparent green must not bleed in");
        assert_eq!(px[3], 128, "alpha should be the mean coverage");
    }

    /// Leaves: a tenth of the texels are solid and the rest are a faint haze, so that a box filter takes the share that is solid down to almost none.
    fn leaves(side: u32) -> Vec<u8> {
        (0..side * side)
            .flat_map(|i| {
                let h = i.wrapping_mul(2_654_435_761).rotate_left(13) ^ i.wrapping_mul(40_503);
                let alpha = if h % 10 == 0 {
                    200 + (h / 10 % 56) as u8
                } else {
                    (h / 7 % 61) as u8
                };
                [200, 120, 40, alpha]
            })
            .collect()
    }

    /// The share of texels of `level` whose alpha reaches `cutoff`.
    fn reaching(level: &[u8], cutoff: f32) -> f32 {
        let bar = (cutoff * 255.0).ceil() as u8;
        let texels = level.as_chunks::<4>().0;
        texels.iter().filter(|t| t[3] >= bar).count() as f32 / texels.len() as f32
    }

    #[test]
    fn a_cutout_keeps_its_coverage_down_the_chain_only_when_the_rule_says_so() {
        let image = leaves(64);
        let plain = generate_mip_chain(64, 64, &image, Some(4), MipFilter::Box, false).unwrap();
        let rule = AlphaRule {
            premultiplied: false,
            coverage: Some(0.5),
        };
        let kept =
            generate_mip_chain_with(64, 64, &image, Some(4), MipFilter::Box, false, rule).unwrap();
        let top = reaching(&image, 0.5);
        assert_eq!(kept[0], image, "the top level is the artist's");
        let lost = reaching(&plain[2], 0.5);
        assert!(
            lost < top / 2.0,
            "without the rule the leaves are mostly gone by the third level: {lost} of {top}"
        );
        for (level, pixels) in kept.iter().enumerate() {
            let share = reaching(pixels, 0.5);
            assert!(
                (share - top).abs() <= 0.05,
                "level {level} reaches {share} of its texels, the top level {top}"
            );
        }
    }

    #[test]
    fn a_premultiplied_chain_stays_premultiplied_and_a_straight_one_does_not_darken_the_edge() {
        // Half of the texels are fully transparent black, as a premultiplied export writes them; the rest are red at half alpha.
        let image: Vec<u8> = (0..16 * 16)
            .flat_map(|i| {
                if i % 2 == 0 {
                    [0, 0, 0, 0]
                } else {
                    [100, 0, 0, 200]
                }
            })
            .collect();
        let rule = AlphaRule {
            premultiplied: true,
            coverage: None,
        };
        let chain =
            generate_mip_chain_with(16, 16, &image, Some(3), MipFilter::Box, false, rule).unwrap();
        for (level, pixels) in chain.iter().enumerate() {
            for t in pixels.as_chunks::<4>().0 {
                assert!(
                    t[0] <= t[3],
                    "level {level}: colour {} is above alpha {}, so it is not premultiplied",
                    t[0],
                    t[3]
                );
            }
        }
        // The colour over the alpha is the same red at every level: nothing was multiplied twice.
        let straight = |t: &[u8]| f32::from(t[0]) / f32::from(t[3]);
        let top = straight(&chain[0].as_chunks::<4>().0[1]);
        let below = straight(&chain[2].as_chunks::<4>().0[0]);
        assert!(
            (top - below).abs() < 0.03,
            "straight colour {top} became {below}"
        );
    }

    #[test]
    fn srgb_transfer_round_trips_every_code_value() {
        for code in 0u16..=255 {
            let linear = SRGB_TO_LINEAR[code as usize];
            assert_eq!(
                linear_to_srgb_u8(linear),
                code as u8,
                "sRGB round-trip failed at code {code}"
            );
        }
    }

    #[test]
    fn srgb_table_is_monotonic() {
        for pair in SRGB_TO_LINEAR.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    #[test]
    fn generation_is_reproducible() {
        let rgba: Vec<u8> = (0..64 * 64 * 4).map(|i| (i * 37 % 256) as u8).collect();
        for filter in [MipFilter::Box, MipFilter::Lanczos3] {
            let a = generate_mip_chain(64, 64, &rgba, None, filter, true).unwrap();
            let b = generate_mip_chain(64, 64, &rgba, None, filter, true).unwrap();
            assert_eq!(a, b, "{filter:?} is not reproducible");
        }
    }

    #[test]
    fn size_mismatch_is_rejected() {
        let rgba = vec![0u8; 10];
        let err = generate_mip_chain(4, 4, &rgba, None, MipFilter::Box, false);
        assert!(err.is_err());
    }

    #[test]
    fn filter_names_parse() {
        assert_eq!(MipFilter::parse("box"), Some(MipFilter::Box));
        assert_eq!(MipFilter::parse("Lanczos3"), Some(MipFilter::Lanczos3));
        assert_eq!(MipFilter::parse("bogus"), None);
    }
}
