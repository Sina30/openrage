//! Planar 4:2:0 pictures, and their conversion to the RGBA a `Frame` holds.

/// One plane of a picture: `height` rows of `width` bytes, each row `stride` bytes apart.
#[derive(Debug, Clone, Copy)]
pub struct Plane<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

impl<'a> Plane<'a> {
    pub fn row(&self, y: usize) -> &'a [u8] {
        &self.data[y * self.stride..][..self.width]
    }

    /// The plane's bytes with no gap between rows.
    pub fn packed(&self) -> Vec<u8> {
        (0..self.height)
            .flat_map(|y| self.row(y).iter().copied())
            .collect()
    }
}

/// A picture in planar Y, U and V at half resolution each way, with an optional alpha plane.
#[derive(Debug, Clone, Copy)]
pub struct Yuv420<'a> {
    pub width: usize,
    pub height: usize,
    /// Whether Y spans 0 to 255 rather than the 16 to 235 of broadcast video.
    pub full_range: bool,
    pub y: Plane<'a>,
    pub u: Plane<'a>,
    pub v: Plane<'a>,
    pub alpha: Option<Plane<'a>>,
}

/// Fixed-point (8 bits) coefficients of ITU-R BT.601: the matrix a decoder of this era assumes.
struct Matrix {
    /// What is taken off Y before it scales.
    y_offset: i32,
    y_scale: i32,
    v_to_r: i32,
    u_to_g: i32,
    v_to_g: i32,
    u_to_b: i32,
}

const LIMITED: Matrix = Matrix {
    y_offset: 16,
    y_scale: 298,
    v_to_r: 409,
    u_to_g: -100,
    v_to_g: -208,
    u_to_b: 516,
};

const FULL: Matrix = Matrix {
    y_offset: 0,
    y_scale: 256,
    v_to_r: 359,
    u_to_g: -88,
    v_to_g: -183,
    u_to_b: 454,
};

fn clamp8(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

impl Yuv420<'_> {
    /// The picture as `width` × `height` RGBA, chroma upsampled with the 3:1 triangle filter
    /// that centres each chroma sample between its four pixels.
    pub fn to_rgba(&self) -> Vec<u8> {
        let (w, h) = (self.width, self.height);
        let m = if self.full_range { &FULL } else { &LIMITED };
        let mut out = vec![0u8; w * h * 4];
        let (cw, ch) = (self.u.width.max(1), self.u.height.max(1));
        let mut u_row = vec![0i32; cw];
        let mut v_row = vec![0i32; cw];
        for y in 0..h {
            // Two chroma rows weighted 3:1, the nearer first: 4 times the vertical blend.
            let near = (y / 2).min(ch - 1);
            let far = if y % 2 == 0 {
                near.saturating_sub(1)
            } else {
                (near + 1).min(ch - 1)
            };
            let (u_near, u_far) = (self.u.row(near), self.u.row(far));
            let (v_near, v_far) = (self.v.row(near), self.v.row(far));
            for x in 0..cw {
                u_row[x] = 3 * i32::from(u_near[x]) + i32::from(u_far[x]);
                v_row[x] = 3 * i32::from(v_near[x]) + i32::from(v_far[x]);
            }
            let luma = self.y.row(y);
            let alpha = self.alpha.map(|a| a.row(y));
            let pixels = &mut out[y * w * 4..(y + 1) * w * 4];
            // A chroma sample stands over two pixels: the left blends with the sample before it
            // and the right with the one after, three parts of its own to one.
            for cx in 0..cw {
                let beside = [cx.saturating_sub(1), (cx + 1).min(cw - 1)];
                for (side, &other) in beside.iter().enumerate() {
                    let x = 2 * cx + side;
                    if x >= w {
                        break;
                    }
                    // 16 times the 2D blend of the four nearest chroma samples.
                    let u = ((3 * u_row[cx] + u_row[other] + 8) >> 4) - 128;
                    let v = ((3 * v_row[cx] + v_row[other] + 8) >> 4) - 128;
                    let scaled = m.y_scale * (i32::from(luma[x]) - m.y_offset) + 128;
                    let pixel = &mut pixels[x * 4..x * 4 + 4];
                    pixel[0] = clamp8((scaled + m.v_to_r * v) >> 8);
                    pixel[1] = clamp8((scaled + m.u_to_g * u + m.v_to_g * v) >> 8);
                    pixel[2] = clamp8((scaled + m.u_to_b * u) >> 8);
                    pixel[3] = alpha.map_or(255, |a| a[x]);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of one colour at the given size, converted.
    fn flat(width: usize, height: usize, (y, u, v): (u8, u8, u8), full_range: bool) -> Vec<u8> {
        let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
        let (yp, up, vp) = (vec![y; width * height], vec![u; cw * ch], vec![v; cw * ch]);
        let plane = |data, w, h| Plane {
            data,
            stride: w,
            width: w,
            height: h,
        };
        Yuv420 {
            width,
            height,
            full_range,
            y: plane(&yp, width, height),
            u: plane(&up, cw, ch),
            v: plane(&vp, cw, ch),
            alpha: None,
        }
        .to_rgba()
    }

    #[test]
    fn broadcast_black_white_and_grey_convert_to_the_full_range_ends() {
        assert_eq!(flat(3, 3, (16, 128, 128), false)[..4], [0, 0, 0, 255]);
        assert_eq!(
            flat(3, 3, (235, 128, 128), false)[..4],
            [255, 255, 255, 255]
        );
        let grey = flat(5, 3, (126, 128, 128), false);
        assert!(grey
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[2] == 128));
        assert_eq!(flat(2, 2, (77, 128, 128), true)[..4], [77, 77, 77, 255]);
    }

    #[test]
    fn red_and_blue_chroma_move_the_matching_channel_and_odd_sizes_convert_whole() {
        let red = flat(7, 5, (81, 90, 240), false);
        assert_eq!(red.len(), 7 * 5 * 4);
        let p = &red[..4];
        assert!(p[0] > 240 && p[1] < 20 && p[2] < 20, "{p:?}");
        let blue = flat(7, 5, (41, 240, 110), false);
        let p = &blue[blue.len() - 4..];
        assert!(p[2] > 230 && p[0] < 30, "{p:?}");
    }

    #[test]
    fn chroma_blends_three_to_one_between_neighbouring_samples() {
        // Luma 128 everywhere and a row of chroma 0, 255, 0, 255: each pixel takes three parts of
        // its own chroma sample and one of the next one over on its side, and the ends their own.
        let y = [128u8; 8];
        let u = [0u8, 255, 0, 255];
        let v = [128u8; 4];
        let picture = Yuv420 {
            width: 8,
            height: 1,
            full_range: true,
            y: Plane {
                data: &y,
                stride: 8,
                width: 8,
                height: 1,
            },
            u: Plane {
                data: &u,
                stride: 4,
                width: 4,
                height: 1,
            },
            v: Plane {
                data: &v,
                stride: 4,
                width: 4,
                height: 1,
            },
            alpha: None,
        };
        let blue = |u: i32| ((256 * 128 + 128 + 454 * (u - 128)) >> 8).clamp(0, 255) as u8;
        let blended = [0, 64, 191, 191, 64, 64, 191, 255];
        let want: Vec<u8> = blended.into_iter().map(blue).collect();
        let got: Vec<u8> = picture
            .to_rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[2])
            .collect();
        assert_eq!(got, want);
    }
}
