//! One image for each slice of a cube, an array or a volume given another size: the image for the
//! slice it is for, and every other slice's own top mip resampled to that size.

use super::{Texture, TexturePack, TtpfError};
use crate::codec::texture::{decode_level, resize_rgba, MipFilter, RgbaImage};
use crate::error::{Error, Result};

/// One image for each slice from `tops`, each slice's own top mip: slice `shown` takes `image` and every other its own top mip resampled to the image's size with `filter`; with no image, `tops`.
pub fn slice_images(
    tops: Vec<RgbaImage>,
    image: Option<&RgbaImage>,
    shown: usize,
    filter: MipFilter,
) -> Result<Vec<RgbaImage>> {
    let Some(image) = image else {
        return Ok(tops);
    };
    if shown >= tops.len() {
        return Err(Error::InvalidData {
            context: format!("there is no slice {shown} of {}", tops.len()),
        });
    }
    let to = (image.width, image.height);
    (tops.into_iter().enumerate())
        .map(|(slice, top)| {
            if slice == shown {
                return Ok(image.clone());
            }
            let from = (top.width, top.height);
            let data = resize_rgba(&top.data, from, to, filter, top.is_srgb)?;
            Ok(RgbaImage {
                width: to.0,
                height: to.1,
                data,
                is_srgb: top.is_srgb,
            })
        })
        .collect()
}

impl Texture {
    /// One image for each of its faces or slices, as [`slice_images`] gives them from each one's top mip.
    pub fn slice_sources(
        &self,
        image: Option<&RgbaImage>,
        shown: usize,
        filter: MipFilter,
    ) -> Result<Vec<RgbaImage>> {
        let tops = (0..self.faces)
            .map(|slice| {
                let face = self.face(slice).ok_or_else(|| Error::InvalidData {
                    context: format!("there is no slice {slice} of {}", self.faces),
                })?;
                decode_level(&face, 0)
            })
            .collect::<Result<Vec<_>>>()?;
        slice_images(tops, image, shown, filter)
    }
}

impl TexturePack {
    /// One image for each slice of texture `index`, as [`slice_images`] gives them; a 2D texture's is `image` itself, read from no row.
    pub fn slice_sources(
        &self,
        index: usize,
        image: Option<&RgbaImage>,
        shown: u32,
        filter: MipFilter,
    ) -> std::result::Result<Vec<RgbaImage>, TtpfError> {
        let count = self.entries.len();
        let entry = (self.entries.get(index)).ok_or(TtpfError::NoSuchTexture { index, count })?;
        let flat = matches!((entry.dimension, entry.slices), (1 | 5, 1));
        if let Some(image) = image.filter(|_| flat) {
            if shown > 0 {
                let (slice, slices) = (shown, 1);
                return Err(TtpfError::NoSuchSlice {
                    index,
                    slice,
                    slices,
                });
            }
            return Ok(vec![image.clone()]);
        }
        let read = self.texture(index)?;
        if image.is_some() && shown >= read.faces {
            let (slice, slices) = (shown, read.faces);
            return Err(TtpfError::NoSuchSlice {
                index,
                slice,
                slices,
            });
        }
        (read.slice_sources(image, shown as usize, filter))
            .map_err(|e| TtpfError::Encode(format!("texture {index}'s slices: {e}")))
    }
}
