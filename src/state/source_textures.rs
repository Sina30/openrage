//! The textures a preview mesh source's own document embeds.

use super::document::DocPayload;
use super::textures::{texture_named, Texture};
use crate::ui::panels::selection_ids::dict_entry_id;

/// The texture `name` a preview mesh source's own document embeds: in its `entry` of a drawable dictionary, else in any of them.
pub fn texture_embedded_in<'p>(
    payload: &'p DocPayload,
    entry: Option<usize>,
    name: &str,
) -> Option<Texture<'p>> {
    let codes = match payload {
        DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => &dict.codes,
        DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => &dict.codes,
        // A drawable or a fragment names no selection.
        _ => return texture_named(payload, None, name),
    };
    (codes.iter().enumerate())
        .filter(|(index, _)| entry.is_none_or(|chosen| chosen == *index))
        .find_map(|(_, code)| texture_named(payload, Some(dict_entry_id(*code)), name))
}
