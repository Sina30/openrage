use std::path::Path;

use image::{ImageEncoder, ImageFormat};

use super::image::RgbaImage;
use crate::error::{IoError, Result};

pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage> {
    let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|e| IoError::InvalidData {
            context: format!("failed to decode PNG: {e}"),
        })?
        .into_rgba8();

    Ok(RgbaImage {
        width: decoded.width(),
        height: decoded.height(),
        data: decoded.into_raw(),
        is_srgb: true,
    })
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let expected = (image.width as usize) * (image.height as usize) * 4;
    if image.data.len() != expected {
        return Err(IoError::InvalidData {
            context: format!(
                "image buffer is {} bytes, expected {} for {}x{} RGBA8",
                image.data.len(),
                expected,
                image.width,
                image.height
            ),
        });
    }

    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            &image.data,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| IoError::InvalidData {
            context: format!("failed to encode PNG: {e}"),
        })?;
    Ok(out)
}

pub fn load_png(path: impl AsRef<Path>) -> Result<RgbaImage> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| IoError::FileReadFailed {
        path: path.to_path_buf(),
        source,
    })?;
    decode_png(&bytes)
}

pub fn save_png(path: impl AsRef<Path>, image: &RgbaImage) -> Result<()> {
    std::fs::write(path.as_ref(), encode_png(image)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_pixels_and_channel_order() {
        let mut source = RgbaImage::new(4, 2);
        source.set_pixel(0, 0, [255, 0, 0, 255]);
        source.set_pixel(1, 0, [0, 255, 0, 255]);
        source.set_pixel(2, 0, [0, 0, 255, 255]);
        source.set_pixel(3, 0, [10, 20, 30, 40]);

        let decoded = decode_png(&encode_png(&source).unwrap()).unwrap();
        assert_eq!((decoded.width, decoded.height), (4, 2));
        assert_eq!(decoded.data, source.data);
        assert_eq!(
            decoded.get_pixel(0, 0),
            [255, 0, 0, 255],
            "red must stay red"
        );
        assert_eq!(
            decoded.get_pixel(2, 0),
            [0, 0, 255, 255],
            "blue must stay blue"
        );
        assert!(decoded.is_srgb);
    }

    #[test]
    fn encoding_is_reproducible() {
        let mut image = RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                image.set_pixel(x, y, [(x * 16) as u8, (y * 16) as u8, 64, 255]);
            }
        }
        assert_eq!(encode_png(&image).unwrap(), encode_png(&image).unwrap());
    }

    #[test]
    fn non_png_input_is_rejected() {
        let message = decode_png(b"not a png at all").unwrap_err().to_string();
        assert!(message.contains("PNG"), "unhelpful error: {message}");
    }

    #[test]
    fn buffer_size_mismatch_is_rejected() {
        let bad = RgbaImage {
            width: 4,
            height: 4,
            data: vec![0; 10],
            is_srgb: true,
        };
        assert!(encode_png(&bad).is_err());
    }
}
