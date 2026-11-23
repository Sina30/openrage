//! Fitting an image inside a maximum side.

use crate::codec::texture::{generate_mip_chain, MipFilter, RgbaImage};

/// How many halvings bring both axes to `limit` or below.
pub fn halvings_to_fit(width: u32, height: u32, limit: u32) -> u32 {
    let mut n = 0;
    while (width >> n).max(1) > limit || (height >> n).max(1) > limit {
        n += 1;
    }
    n
}

/// `image` reduced to `limit` on its longest side, aspect preserved.
///
/// `None` when the resample fails; `name` is only there to say which texture
/// in the warning.
pub fn fit_within(image: RgbaImage, limit: u32, name: &str) -> Option<RgbaImage> {
    if image.width <= limit && image.height <= limit {
        return Some(image);
    }

    let n = halvings_to_fit(image.width, image.height, limit);
    let chain = match generate_mip_chain(
        image.width,
        image.height,
        &image.data,
        Some(n + 1),
        MipFilter::Box,
        image.is_srgb,
    ) {
        Ok(chain) => chain,
        Err(e) => {
            tracing::warn!(
                texture = %name,
                error = %e,
                "Failed to resample an oversized texture"
            );
            return None;
        }
    };

    let data = chain.into_iter().nth(n as usize)?;
    let fitted = RgbaImage {
        width: (image.width >> n).max(1),
        height: (image.height >> n).max(1),
        data,
        is_srgb: image.is_srgb,
    };
    tracing::debug!(
        texture = %name,
        from = format!("{}x{}", image.width, image.height),
        to = format!("{}x{}", fitted.width, fitted.height),
        limit,
        "Reduced an oversized texture to the maximum side"
    );
    Some(fitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_already_inside_the_limit_is_left_alone() {
        let image = RgbaImage {
            width: 64,
            height: 32,
            data: vec![0xAB; 64 * 32 * 4],
            is_srgb: true,
        };
        let fitted = fit_within(image, 128, "small").expect("no work to do");
        assert_eq!((fitted.width, fitted.height), (64, 32));
    }

    #[test]
    fn halvings_are_counted_per_axis_and_stop_at_one() {
        assert_eq!(halvings_to_fit(64, 64, 64), 0);
        assert_eq!(halvings_to_fit(4096, 4096, 2048), 1);
        assert_eq!(halvings_to_fit(4096, 4096, 64), 6);
        assert_eq!(halvings_to_fit(2048, 16, 64), 5);
        assert_eq!(halvings_to_fit(4096, 4096, 1), 12);
    }
}
