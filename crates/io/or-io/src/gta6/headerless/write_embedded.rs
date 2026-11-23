//! A drawable's embedded texture written in place from pixels of its own size, format and mips:
//! found by the key its record at +0x30 holds, in the dictionary its shader group points at from
//! +0x08, as the head's reader finds it.

use super::bytes::sections;
use super::write::write_object;
use super::{HeaderlessError, Result};

/// The texture of `key` the drawable, or its fragment's where `fragment`, embeds, from `pixels`.
// `pixels` are its levels top first, as the head's reader gives them.
pub fn write_embedded_texture(
    virtual_section: &[u8],
    physical: &mut [u8],
    fragment: bool,
    key: u64,
    pixels: &[u8],
) -> Result<()> {
    let none = || HeaderlessError::NotInPlace("the drawable embeds no texture of that key");
    let at = sections(virtual_section, None, |v, _| {
        let root = match fragment {
            true => v.to("drawable", 0x18)?,
            false => 0,
        };
        let group = v.to("shader group", root + 0x10)?;
        let Some(dictionary) = v.ptr("embedded texture dictionary", group + 0x08)? else {
            return Err(none());
        };
        let count = usize::from(v.u16("embedded dictionary's texture count", dictionary + 0x38)?);
        if count == 0 {
            return Err(none());
        }
        let list = v.to("embedded textures", dictionary + 0x30)?;
        v.array("embedded textures", list, count, 8)?;
        for k in 0..count {
            let at = v.to("embedded texture", list + 8 * k)?;
            let record = v.to("embedded texture's key record", at + 0x30)?;
            if v.u64("embedded texture's key", record + 0x08)? == key {
                return Ok(at);
            }
        }
        Err(none())
    })?;
    write_object(virtual_section, physical, at, false, pixels)
}
