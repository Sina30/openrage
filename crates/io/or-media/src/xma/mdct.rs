//! The inverse MDCT the WMA Pro family uses, as a DCT-IV run through a complex FFT of half its length.
//!
//! The transform is OpenRage's own code, and the only FFmpeg in this file is the test's reference:
//! `ff_tx_mdct_naive_inv` of `libavutil/tx_template.c`, tag `n7.0` (commit
//! `083443d67cb159ce469e5d902346b8d0c2cd1c93`, LGPL-2.1-or-later, Copyright (c) Lynne), which defines
//! what FFmpeg's inverse MDCT computes: `n` coefficients in, `n` samples out, the half of the full
//! inverse transform that a windowed overlap-add needs.

use std::f64::consts::PI;

type Complex = (f32, f32);

fn mul((ar, ai): Complex, (br, bi): Complex) -> Complex {
    (ar * br - ai * bi, ar * bi + ai * br)
}

/// One transform size, with its twiddles built once.
pub(super) struct Imdct {
    n: usize,
    scale: f32,
    /// `exp(-i pi m / n)`, applied before the FFT.
    pre: Vec<Complex>,
    /// `exp(-i pi (4k + 1) / 4n)`, applied after it.
    post: Vec<Complex>,
    /// `exp(-2 pi i j / (n / 2))` for the first half of the FFT's circle.
    fft: Vec<Complex>,
    reversed: Vec<usize>,
}

fn cis(angle: f64) -> Complex {
    (angle.cos() as f32, angle.sin() as f32)
}

impl Imdct {
    /// A transform of `n` coefficients, a power of two of at least 4, whose output is multiplied by `scale`.
    pub fn new(n: usize, scale: f32) -> Self {
        assert!(n >= 4 && n.is_power_of_two(), "{n} is not an MDCT size");
        let m = n / 2;
        let bits = m.trailing_zeros();
        Self {
            n,
            scale,
            pre: (0..m).map(|k| cis(-PI * k as f64 / n as f64)).collect(),
            post: (0..m)
                .map(|k| cis(-PI * (4 * k + 1) as f64 / (4 * n) as f64))
                .collect(),
            fft: (0..m / 2)
                .map(|j| cis(-2.0 * PI * j as f64 / m as f64))
                .collect(),
            reversed: (0..m)
                .map(|i| i.reverse_bits() >> (usize::BITS - bits))
                .collect(),
        }
    }

    /// `out[k] = scale * sum_j input[j] cos(pi (2j + 1) (2 (n - 1 - k) + 1) / 4n)`.
    pub fn process(&self, input: &[f32], out: &mut [f32]) {
        let (n, m) = (self.n, self.n / 2);
        let mut z = vec![(0.0f32, 0.0f32); m];
        for k in 0..m {
            z[self.reversed[k]] = mul((input[2 * k], input[n - 1 - 2 * k]), self.pre[k]);
        }
        let mut half = 1;
        while half < m {
            let step = m / (2 * half);
            for start in (0..m).step_by(2 * half) {
                for j in 0..half {
                    let t = mul(z[start + j + half], self.fft[j * step]);
                    let u = z[start + j];
                    z[start + j] = (u.0 + t.0, u.1 + t.1);
                    z[start + j + half] = (u.0 - t.0, u.1 - t.1);
                }
            }
            half *= 2;
        }
        for k in 0..m {
            let (re, im) = mul(z[k], self.post[k]);
            out[n - 1 - 2 * k] = self.scale * re;
            out[2 * k] = -self.scale * im;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ff_tx_mdct_naive_inv` in FFmpeg's `tx_template.c`, term for term.
    fn naive(input: &[f32], scale: f64) -> Vec<f64> {
        let len2 = input.len();
        let len = len2 / 2;
        let phase = PI / (4.0 * len2 as f64);
        let mut out = vec![0.0; len2];
        for i in 0..len {
            let (mut sum_d, mut sum_u) = (0.0, 0.0);
            let i_d = phase * (4 * len - 2 * i - 1) as f64;
            let i_u = phase * (3 * len2 + 2 * i + 1) as f64;
            for (j, &x) in input.iter().enumerate() {
                let a = (2 * j + 1) as f64;
                sum_d += (a * i_d).cos() * f64::from(x);
                sum_u += (a * i_u).cos() * f64::from(x);
            }
            out[i] = sum_d * scale;
            out[i + len] = -sum_u * scale;
        }
        out
    }

    #[test]
    fn the_fast_transform_is_the_definition_at_every_size_the_codec_uses() {
        let mut seed = 12345u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 23) as f32 - 1.0
        };
        for n in [4, 8, 64, 128, 256, 512, 1024] {
            let input: Vec<f32> = (0..n).map(|_| next() * 1000.0).collect();
            let scale = 2.0 / (n as f64 * 32768.0);
            let want = naive(&input, scale);
            let mut got = vec![0.0f32; n];
            Imdct::new(n, scale as f32).process(&input, &mut got);
            let worst = want
                .iter()
                .zip(&got)
                .map(|(w, g)| (w - f64::from(*g)).abs())
                .fold(0.0, f64::max);
            let peak = want.iter().fold(0.0f64, |p, w| p.max(w.abs()));
            assert!(worst <= peak * 1e-5, "n = {n}: off by {worst} of {peak}");
        }
    }

    #[test]
    fn a_transform_that_ignored_the_order_would_fail_the_check_above() {
        let input: Vec<f32> = (0..16).map(|i| (i as f32 * 0.7).sin()).collect();
        let want = naive(&input, 1.0);
        let mut got = vec![0.0f32; 16];
        Imdct::new(16, 1.0).process(&input, &mut got);
        got.reverse();
        let worst = want
            .iter()
            .zip(&got)
            .map(|(w, g)| (w - f64::from(*g)).abs())
            .fold(0.0, f64::max);
        assert!(worst > 0.1, "a reversed output is a different transform");
    }
}
