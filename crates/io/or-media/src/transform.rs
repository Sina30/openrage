//! The two inverse transforms Bink audio decodes with, over one radix-2 complex FFT.
//!
//! Origin: written for OpenRage to the contract of FFmpeg's `av_tx` DCT-III and complex-to-real
//! DFT (libavutil, n7.0); no code is taken from it. Sizes are powers of two. They run in double
//! precision, so what they return differs from the reference's single-precision result by rounding
//! only.

use std::f64::consts::PI;

/// A complex FFT of a power-of-two size.
pub(crate) struct Fft {
    n: usize,
    rev: Vec<u32>,
    /// `e^(+2 pi i k / n)` for `k < n / 2`.
    cos: Vec<f64>,
    sin: Vec<f64>,
}

impl Fft {
    pub(crate) fn new(n: usize) -> Self {
        assert!(n.is_power_of_two(), "an FFT size is a power of two");
        let bits = n.trailing_zeros();
        let rev = (0..n as u32)
            .map(|i| {
                if bits == 0 {
                    0
                } else {
                    i.reverse_bits() >> (32 - bits)
                }
            })
            .collect();
        let step = 2.0 * PI / n as f64;
        Self {
            n,
            rev,
            cos: (0..n / 2).map(|k| (step * k as f64).cos()).collect(),
            sin: (0..n / 2).map(|k| (step * k as f64).sin()).collect(),
        }
    }

    /// `x[k] = sum over j of x[j] e^(+2 pi i j k / n)`, in place and not divided by `n`.
    pub(crate) fn inverse(&self, re: &mut [f64], im: &mut [f64]) {
        let n = self.n;
        for (i, &rev) in self.rev.iter().enumerate().take(n) {
            let j = rev as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let stride = n / len;
            for start in (0..n).step_by(len) {
                for j in 0..half {
                    let (wr, wi) = (self.cos[j * stride], self.sin[j * stride]);
                    let (a, b) = (start + j, start + j + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

/// `y[n] = x[0] + sum over k >= 1 of x[k] cos(pi k (2n + 1) / (2 N))`, the DCT-III of `N` values.
pub(crate) struct Dct3 {
    n: usize,
    fft: Fft,
    /// `e^(i pi k / (2 N))` for `k < N`.
    twiddle: Vec<(f64, f64)>,
}

impl Dct3 {
    pub(crate) fn new(n: usize) -> Self {
        let step = PI / (2 * n) as f64;
        Self {
            n,
            fft: Fft::new(2 * n),
            twiddle: (0..n)
                .map(|k| ((step * k as f64).cos(), (step * k as f64).sin()))
                .collect(),
        }
    }

    /// Transforms `x` (`N` values) into `out` (`N` values).
    pub(crate) fn apply(&self, x: &[f32], out: &mut [f32]) {
        let n = self.n;
        let mut re = vec![0.0f64; 2 * n];
        let mut im = vec![0.0f64; 2 * n];
        for k in 0..n {
            let (c, s) = self.twiddle[k];
            let v = f64::from(x[k]);
            re[k] = v * c;
            im[k] = v * s;
        }
        self.fft.inverse(&mut re, &mut im);
        for (o, r) in out.iter_mut().zip(&re) {
            *o = *r as f32;
        }
    }
}

/// The inverse real DFT of `N` outputs from `N / 2 + 1` complex bins, as `N + 2` interleaved
/// values: `y[n] = scale * sum over k of X[k] e^(+2 pi i k n / N)` with `X` Hermitian-extended.
pub(crate) struct Rdft {
    n: usize,
    fft: Fft,
}

impl Rdft {
    pub(crate) fn new(n: usize) -> Self {
        Self {
            n,
            fft: Fft::new(n),
        }
    }

    pub(crate) fn apply(&self, bins: &[f32], scale: f32, out: &mut [f32]) {
        let n = self.n;
        let mut re = vec![0.0f64; n];
        let mut im = vec![0.0f64; n];
        let (dc, nyquist) = (f64::from(bins[0]), f64::from(bins[n]));
        re[0] = dc;
        re[n / 2] = nyquist;
        for k in 1..n / 2 {
            let (r, i) = (f64::from(bins[2 * k]), f64::from(bins[2 * k + 1]));
            re[k] = r;
            im[k] = i;
            re[n - k] = r;
            im[n - k] = -i;
        }
        self.fft.inverse(&mut re, &mut im);
        for (o, r) in out.iter_mut().zip(&re) {
            *o = (*r * f64::from(scale)) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed, unremarkable signal: no symmetry for a wrong transform to hide behind.
    fn signal(n: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect()
    }

    #[test]
    fn the_fft_matches_the_defining_sum_in_both_a_small_and_a_large_size() {
        for n in [1usize, 2, 8, 64] {
            let (xr, xi) = (signal(n, 1), signal(n, 2));
            let (mut re, mut im): (Vec<f64>, Vec<f64>) = (
                xr.iter().map(|&v| f64::from(v)).collect(),
                xi.iter().map(|&v| f64::from(v)).collect(),
            );
            Fft::new(n).inverse(&mut re, &mut im);
            for k in 0..n {
                let (mut sr, mut si) = (0.0, 0.0);
                for j in 0..n {
                    let a = 2.0 * PI * (j * k) as f64 / n as f64;
                    sr += f64::from(xr[j]) * a.cos() - f64::from(xi[j]) * a.sin();
                    si += f64::from(xr[j]) * a.sin() + f64::from(xi[j]) * a.cos();
                }
                assert!(
                    (re[k] - sr).abs() < 1e-9 && (im[k] - si).abs() < 1e-9,
                    "n {n} bin {k}"
                );
            }
        }
    }

    #[test]
    fn the_dct3_matches_its_definition() {
        for n in [2usize, 16, 64] {
            let x = signal(n, 3);
            let mut got = vec![0.0f32; n];
            Dct3::new(n).apply(&x, &mut got);
            for (i, g) in got.iter().enumerate() {
                let mut want = f64::from(x[0]);
                for (k, &v) in x.iter().enumerate().skip(1) {
                    want +=
                        f64::from(v) * (PI * k as f64 * (2 * i + 1) as f64 / (2 * n) as f64).cos();
                }
                assert!(
                    (f64::from(*g) - want).abs() < 1e-5,
                    "n {n} output {i}: {g} against {want}"
                );
            }
        }
    }

    #[test]
    fn the_inverse_rdft_matches_its_definition() {
        for n in [4usize, 16, 64] {
            let mut bins = signal(n + 2, 4);
            bins[1] = 0.0; // the DC bin's imaginary part is not an input
            bins[n + 1] = 0.0;
            let mut got = vec![0.0f32; n];
            Rdft::new(n).apply(&bins, 0.5, &mut got);
            for (i, g) in got.iter().enumerate() {
                let mut want =
                    f64::from(bins[0]) + f64::from(bins[n]) * if i % 2 == 0 { 1.0 } else { -1.0 };
                for k in 1..n / 2 {
                    let a = 2.0 * PI * (k * i) as f64 / n as f64;
                    want += 2.0
                        * (f64::from(bins[2 * k]) * a.cos() - f64::from(bins[2 * k + 1]) * a.sin());
                }
                assert!(
                    (f64::from(*g) - 0.5 * want).abs() < 1e-5,
                    "n {n} output {i}: {g} against {}",
                    0.5 * want
                );
            }
        }
    }
}
