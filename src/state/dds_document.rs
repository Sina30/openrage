//! A loose `.dds` as one Legacy texture named after its file, so the `.ytd`
//! preview, encoder and replace path take it; it saves back through `or_io::dds`.

use or_io::codec::texture::{format_of_dds, to_dds};
use or_io::dds::Dds;
use or_io::gta5::gen8::GrcTexture;

/// The texture `dds` holds, named `name`; refused when it is not one 2D surface or its payload is short.
pub fn open(name: &str, dds: &Dds) -> Result<GrcTexture, String> {
    if !dds.is_one_surface() {
        return Err(
            "this DDS is a cube map, a volume or an array; OpenRage opens a single 2D texture"
                .to_string(),
        );
    }
    let (format, srgb) = format_of_dds(dds).map_err(|e| e.to_string())?;
    let (Ok(width), Ok(height)) = (
        u16::try_from(dds.header.width.max(1)),
        u16::try_from(dds.header.height.max(1)),
    ) else {
        return Err(format!(
            "{}x{} does not fit a texture's 16-bit sides",
            dds.header.width, dds.header.height
        ));
    };
    let mips = u8::try_from(dds.header.mip_map_count.max(1)).map_err(|_| {
        format!(
            "{} mip levels is more than a texture holds",
            dds.header.mip_map_count
        )
    })?;
    let size = format.total_size(width.into(), height.into(), mips.into()) as usize;
    let Some(payload) = dds.data.get(..size) else {
        return Err(format!(
            "the payload is {} bytes but {width}x{height} {format:?} with {mips} mips needs {size}",
            dds.data.len()
        ));
    };
    Ok(GrcTexture {
        name: name.to_string(),
        width,
        height,
        depth: 1,
        mip_stride: u16::try_from(format.row_pitch(width.into())).unwrap_or(0),
        format: format.to_raw_format(),
        mip_count: mips,
        info_bits: u8::from(srgb),
        backing_store: payload.to_vec(),
        resource_type_and_conversion_flags: or_io::gta5::texture_dict::gen8::PROCESSED,
        ..Default::default()
    })
}

/// `texture` as a `.dds` file's bytes.
pub fn save(texture: &GrcTexture) -> Result<Vec<u8>, String> {
    to_dds(texture)
        .and_then(|dds| dds.to_bytes())
        .map_err(|e| e.to_string())
}
