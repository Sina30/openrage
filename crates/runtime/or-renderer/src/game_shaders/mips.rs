//! The levels of a texture below its first, which a texture seen small is read from, so that it shows its average and not a texel it happened to land on.

use rayon::prelude::*;

/// One level: its width, its height and its 8-bit RGBA.
pub type Level = (u32, u32, Vec<u8>);

/// How many levels a texture of this size has, the first included.
pub fn count(width: u32, height: u32) -> u32 {
    width.max(height).max(1).ilog2() + 1
}

fn decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_to_linear(value: u8) -> f32 {
    decode(f32::from(value) / 255.0)
}

fn linear_to_srgb(linear: f32) -> u8 {
    let l = linear.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// The sRGB bytes in linear light, and the linear values at which a byte rounds up to the next.
struct Srgb {
    linear: [f32; 256],
    steps: [f32; 255],
}

impl Srgb {
    fn new() -> Self {
        Self {
            linear: std::array::from_fn(|i| srgb_to_linear(i as u8)),
            steps: std::array::from_fn(|i| decode((i as f32 + 0.5) / 255.0)),
        }
    }

    /// The byte of a linear value: a search among the steps, and the exact formula where a value lies too near a step for the search to be sure of the side.
    fn encode(&self, linear: f32) -> u8 {
        let l = linear.clamp(0.0, 1.0);
        let at = self.steps.partition_point(|step| *step <= l);
        let near = |step: Option<&f32>| step.is_some_and(|s| (l - s).abs() < 1e-6);
        if near(self.steps.get(at)) || near(at.checked_sub(1).and_then(|i| self.steps.get(i))) {
            return linear_to_srgb(l);
        }
        at as u8
    }
}

/// The levels below the first of `rgba`, `width` by `height`, each half the size of the last and each texel the average of the four it covers; colour is averaged in linear light where the texture is `srgb`, and alpha is a coverage and always averaged as it is.
pub fn chain(width: u32, height: u32, rgba: &[u8], srgb: bool) -> Vec<Level> {
    let table = srgb.then(Srgb::new);
    let mut levels: Vec<Level> = Vec::new();
    let (mut w, mut h) = (width.max(1), height.max(1));
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let next = {
            let source = levels.last().map_or(rgba, |level| level.2.as_slice());
            let mut next = vec![0u8; (nw * nh * 4) as usize];
            // A row of the new level reads two of the last and nothing else, so the rows are made at once
            next.par_chunks_mut((nw * 4) as usize)
                .enumerate()
                .for_each(|(y, out)| {
                    let y = y as u32;
                    // The rows the new ones cover, the last repeated where a side has one
                    let row = |dy: u32| {
                        let at = ((2 * y + dy).min(h - 1) * w * 4) as usize;
                        &source[at..at + (w * 4) as usize]
                    };
                    let (top, bottom) = (row(0), row(1));
                    for x in 0..nw {
                        let left = ((2 * x).min(w - 1) * 4) as usize;
                        let right = ((2 * x + 1).min(w - 1) * 4) as usize;
                        for channel in 0..4 {
                            let four = [
                                top[left + channel],
                                top[right + channel],
                                bottom[left + channel],
                                bottom[right + channel],
                            ];
                            out[x as usize * 4 + channel] = match &table {
                                Some(table) if channel < 3 => table.encode(
                                    four.iter()
                                        .map(|v| table.linear[usize::from(*v)])
                                        .sum::<f32>()
                                        / 4.0,
                                ),
                                _ => {
                                    let sum: f32 = four.iter().map(|v| f32::from(*v) / 255.0).sum();
                                    (sum / 4.0 * 255.0).round() as u8
                                }
                            };
                        }
                    }
                });
            next
        };
        levels.push((nw, nh, next));
        (w, h) = (nw, nh);
    }
    levels
}

#[cfg(test)]
mod tests;
