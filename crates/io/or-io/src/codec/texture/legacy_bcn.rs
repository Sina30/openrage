use super::super::format::GrcTextureFormat;
use super::RgbaImage;

pub fn decode(format: GrcTextureFormat, width: u32, height: u32, data: &[u8]) -> Option<RgbaImage> {
    let mut image = RgbaImage::new(width, height);
    match format {
        GrcTextureFormat::DXT1 => decode_bc1(data, width, height, &mut image),
        GrcTextureFormat::DXT3 => decode_bc2(data, width, height, &mut image),
        GrcTextureFormat::DXT5 => decode_bc3(data, width, height, &mut image),
        GrcTextureFormat::DXT5A => decode_bc4(data, width, height, &mut image),
        GrcTextureFormat::DXN => decode_bc5(data, width, height, &mut image),
        _ => return None,
    }
    Some(image)
}

fn decode_bc1(data: &[u8], width: u32, height: u32, image: &mut RgbaImage) {
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block_idx = (by * blocks_x + bx) as usize;
            let block_offset = block_idx * 8;

            if block_offset + 8 > data.len() {
                break;
            }

            let block = &data[block_offset..block_offset + 8];
            decode_bc1_block(block, bx * 4, by * 4, width, height, image);
        }
    }
}

fn decode_bc1_block(block: &[u8], x: u32, y: u32, width: u32, height: u32, image: &mut RgbaImage) {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);

    let color0 = rgb565_to_rgba(c0);
    let color1 = rgb565_to_rgba(c1);

    let colors = if c0 > c1 {
        [
            color0,
            color1,
            interpolate_color(color0, color1, 2, 1),
            interpolate_color(color0, color1, 1, 2),
        ]
    } else {
        [
            color0,
            color1,
            interpolate_color(color0, color1, 1, 1),
            [0, 0, 0, 0],
        ]
    };

    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);

    for py in 0..4 {
        for px in 0..4 {
            let pixel_x = x + px;
            let pixel_y = y + py;

            if pixel_x < width && pixel_y < height {
                let idx = (py * 4 + px) * 2;
                let color_idx = ((indices >> idx) & 0x03) as usize;
                image.set_pixel(pixel_x, pixel_y, colors[color_idx]);
            }
        }
    }
}

fn decode_bc2(data: &[u8], width: u32, height: u32, image: &mut RgbaImage) {
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block_idx = (by * blocks_x + bx) as usize;
            let block_offset = block_idx * 16;

            if block_offset + 16 > data.len() {
                break;
            }

            let block = &data[block_offset..block_offset + 16];
            decode_bc2_block(block, bx * 4, by * 4, width, height, image);
        }
    }
}

fn decode_bc2_block(block: &[u8], x: u32, y: u32, width: u32, height: u32, image: &mut RgbaImage) {
    let alpha_block = &block[0..8];
    let color_block = &block[8..16];

    let c0 = u16::from_le_bytes([color_block[0], color_block[1]]);
    let c1 = u16::from_le_bytes([color_block[2], color_block[3]]);

    let color0 = rgb565_to_rgba(c0);
    let color1 = rgb565_to_rgba(c1);

    let colors = [
        color0,
        color1,
        interpolate_color(color0, color1, 2, 1),
        interpolate_color(color0, color1, 1, 2),
    ];

    let indices = u32::from_le_bytes([
        color_block[4],
        color_block[5],
        color_block[6],
        color_block[7],
    ]);

    for py in 0..4u32 {
        for px in 0..4u32 {
            let pixel_x = x + px;
            let pixel_y = y + py;

            if pixel_x < width && pixel_y < height {
                let idx = (py * 4 + px) * 2;
                let color_idx = ((indices >> idx) & 0x03) as usize;
                let mut rgba = colors[color_idx];

                let alpha_byte_idx = (py * 2 + px / 2) as usize;
                let alpha_nibble = if px % 2 == 0 {
                    alpha_block[alpha_byte_idx] & 0x0F
                } else {
                    (alpha_block[alpha_byte_idx] >> 4) & 0x0F
                };
                rgba[3] = (alpha_nibble << 4) | alpha_nibble;

                image.set_pixel(pixel_x, pixel_y, rgba);
            }
        }
    }
}

fn decode_bc3(data: &[u8], width: u32, height: u32, image: &mut RgbaImage) {
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block_idx = (by * blocks_x + bx) as usize;
            let block_offset = block_idx * 16;

            if block_offset + 16 > data.len() {
                break;
            }

            let block = &data[block_offset..block_offset + 16];
            decode_bc3_block(block, bx * 4, by * 4, width, height, image);
        }
    }
}

fn decode_bc3_block(block: &[u8], x: u32, y: u32, width: u32, height: u32, image: &mut RgbaImage) {
    let alpha_block = &block[0..8];
    let alphas = decode_bc3_alpha_palette(alpha_block[0], alpha_block[1]);
    let alpha_indices = decode_bc3_alpha_indices(&alpha_block[2..8]);

    let color_block = &block[8..16];
    let c0 = u16::from_le_bytes([color_block[0], color_block[1]]);
    let c1 = u16::from_le_bytes([color_block[2], color_block[3]]);

    let color0 = rgb565_to_rgba(c0);
    let color1 = rgb565_to_rgba(c1);

    let colors = [
        color0,
        color1,
        interpolate_color(color0, color1, 2, 1),
        interpolate_color(color0, color1, 1, 2),
    ];

    let indices = u32::from_le_bytes([
        color_block[4],
        color_block[5],
        color_block[6],
        color_block[7],
    ]);

    for py in 0..4u32 {
        for px in 0..4u32 {
            let pixel_x = x + px;
            let pixel_y = y + py;

            if pixel_x < width && pixel_y < height {
                let idx = (py * 4 + px) * 2;
                let color_idx = ((indices >> idx) & 0x03) as usize;
                let mut rgba = colors[color_idx];

                let alpha_idx = (py * 4 + px) as usize;
                rgba[3] = alphas[alpha_indices[alpha_idx] as usize];

                image.set_pixel(pixel_x, pixel_y, rgba);
            }
        }
    }
}

fn decode_bc3_alpha_palette(a0: u8, a1: u8) -> [u8; 8] {
    if a0 > a1 {
        [
            a0,
            a1,
            ((6 * a0 as u32 + a1 as u32) / 7) as u8,
            ((5 * a0 as u32 + 2 * a1 as u32) / 7) as u8,
            ((4 * a0 as u32 + 3 * a1 as u32) / 7) as u8,
            ((3 * a0 as u32 + 4 * a1 as u32) / 7) as u8,
            ((2 * a0 as u32 + 5 * a1 as u32) / 7) as u8,
            ((a0 as u32 + 6 * a1 as u32) / 7) as u8,
        ]
    } else {
        [
            a0,
            a1,
            ((4 * a0 as u32 + a1 as u32) / 5) as u8,
            ((3 * a0 as u32 + 2 * a1 as u32) / 5) as u8,
            ((2 * a0 as u32 + 3 * a1 as u32) / 5) as u8,
            ((a0 as u32 + 4 * a1 as u32) / 5) as u8,
            0,
            255,
        ]
    }
}

fn decode_bc3_alpha_indices(data: &[u8]) -> [u8; 16] {
    let mut indices = [0u8; 16];
    let bits = u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], 0, 0]);
    for (i, index) in indices.iter_mut().enumerate() {
        *index = ((bits >> (i * 3)) & 0x07) as u8;
    }
    indices
}

fn decode_bc4(data: &[u8], width: u32, height: u32, image: &mut RgbaImage) {
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block_idx = (by * blocks_x + bx) as usize;
            let block_offset = block_idx * 8;

            if block_offset + 8 > data.len() {
                break;
            }

            let block = &data[block_offset..block_offset + 8];
            let values = decode_bc3_alpha_palette(block[0], block[1]);
            let indices = decode_bc3_alpha_indices(&block[2..8]);

            for py in 0..4u32 {
                for px in 0..4u32 {
                    let pixel_x = bx * 4 + px;
                    let pixel_y = by * 4 + py;

                    if pixel_x < width && pixel_y < height {
                        let idx = (py * 4 + px) as usize;
                        let v = values[indices[idx] as usize];
                        image.set_pixel(pixel_x, pixel_y, [v, v, v, 255]);
                    }
                }
            }
        }
    }
}

fn decode_bc5(data: &[u8], width: u32, height: u32, image: &mut RgbaImage) {
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block_idx = (by * blocks_x + bx) as usize;
            let block_offset = block_idx * 16;

            if block_offset + 16 > data.len() {
                break;
            }

            let block = &data[block_offset..block_offset + 16];

            let red_values = decode_bc3_alpha_palette(block[0], block[1]);
            let red_indices = decode_bc3_alpha_indices(&block[2..8]);

            let green_values = decode_bc3_alpha_palette(block[8], block[9]);
            let green_indices = decode_bc3_alpha_indices(&block[10..16]);

            for py in 0..4u32 {
                for px in 0..4u32 {
                    let pixel_x = bx * 4 + px;
                    let pixel_y = by * 4 + py;

                    if pixel_x < width && pixel_y < height {
                        let idx = (py * 4 + px) as usize;
                        let r = red_values[red_indices[idx] as usize];
                        let g = green_values[green_indices[idx] as usize];
                        image.set_pixel(pixel_x, pixel_y, [r, g, 128, 255]);
                    }
                }
            }
        }
    }
}

fn rgb565_to_rgba(rgb565: u16) -> [u8; 4] {
    let r5 = ((rgb565 >> 11) & 0x1F) as u8;
    let g6 = ((rgb565 >> 5) & 0x3F) as u8;
    let b5 = (rgb565 & 0x1F) as u8;

    let r = (r5 << 3) | (r5 >> 2);
    let g = (g6 << 2) | (g6 >> 4);
    let b = (b5 << 3) | (b5 >> 2);

    [r, g, b, 255]
}

fn interpolate_color(c0: [u8; 4], c1: [u8; 4], w0: u32, w1: u32) -> [u8; 4] {
    let total = w0 + w1;
    [
        ((c0[0] as u32 * w0 + c1[0] as u32 * w1) / total) as u8,
        ((c0[1] as u32 * w0 + c1[1] as u32 * w1) / total) as u8,
        ((c0[2] as u32 * w0 + c1[2] as u32 * w1) / total) as u8,
        ((c0[3] as u32 * w0 + c1[3] as u32 * w1) / total) as u8,
    ]
}
