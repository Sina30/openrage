//! Where the game keeps the freemode peds' own drawables and variation metadata.

use crate::vfs::{MountedGame, Vfs};

/// The archive of streamed peds that holds a folder for each freemode ped.
pub const PED_ARCHIVE: &str = "platform:/models/cdimages/streamedpeds_mp.rpf";

/// `file` in the folder of the freemode ped `model` (`mp_m_freemode_01`): `uppr_000_r.ydd`, `head_diff_000_a_whi.ytd`.
pub fn path(model: &str, file: &str) -> String {
    format!("{PED_ARCHIVE}/{model}/{file}")
}

/// The variation metadata of the freemode ped `model`, which lists its drawables and their textures.
pub fn variation_path(model: &str) -> String {
    format!("{PED_ARCHIVE}/{model}.ymt")
}

/// `file` of the freemode ped `model` as the mounted game has it, none where the game has no such file.
pub fn open(vfs: &Vfs, model: &str, file: &str) -> Option<Vec<u8>> {
    vfs.open(&path(model, file))
}

/// The freemode peds, by the model name their shop metadata is filed under.
const PEDS: [&str; 2] = ["mp_m_freemode_01", "mp_f_freemode_01"];

/// The shop metadata is registered with this type.
const SHOP_FILE_TYPE: &str = "SHOP_PED_APPAREL_META_FILE";

/// A collection of a freemode ped that a DLC pack of the game adds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DlcCollection {
    /// The DLC pack that registers it, by the device its setup names.
    pub pack: String,
    /// The freemode ped it dresses: `mp_m_freemode_01` or `mp_f_freemode_01`.
    pub ped: String,
    /// The collection as the shop metadata names it in `dlcName`, in lower case since the game resolves a name without regard to case.
    pub collection: String,
}

/// What the text of `element` is in `xml`, none where it has none.
fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let rest = &xml[xml.find(&format!("<{name}>"))? + name.len() + 2..];
    let text = rest[..rest.find(&format!("</{name}>"))?].trim();
    (!text.is_empty()).then_some(text)
}

/// The ped and the collection a shop metadata file registers, from its `pedName` and its `dlcName`.
///
/// ⚠ Never from the file's name: the retail packs name their shop files `mp_m_freemode_01_beach_shop.meta` for a collection
/// that is `Male_freemode_beach`, and `mp_m_freemode_01_male_xmas.meta` for `male_xmas`.
pub fn collection_in(shop: &str) -> Option<(String, String)> {
    let ped = element(shop, "pedName")?;
    PEDS.contains(&ped).then_some(())?;
    let collection = element(shop, "dlcName")?;
    Some((ped.to_string(), collection.to_ascii_lowercase()))
}

/// Every collection the DLC packs of a mounted game register for the freemode peds, by pack and name.
///
/// Each shop file a pack registers is opened through the mount and read for the collection it states; a file that does not open adds none.
pub fn dlc_collections(game: &MountedGame) -> Vec<DlcCollection> {
    let mut all: Vec<DlcCollection> = Vec::new();
    for pack in &game.packs {
        let shops =
            (pack.data_files.iter()).filter(|f| f.file_type == SHOP_FILE_TYPE && !f.disabled);
        for file in shops {
            let Some(bytes) = game.vfs.open(&file.filename) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            if let Some((ped, collection)) = collection_in(text.trim_start_matches('\u{feff}')) {
                all.push(DlcCollection {
                    pack: pack.device_name.clone(),
                    ped,
                    collection,
                });
            }
        }
    }
    all.sort();
    all.dedup();
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shop_file_states_the_collection_it_registers_and_not_its_name() {
        let shop = |ped: &str, dlc: &str| {
            format!(
                "﻿<?xml version=\"1.0\"?>
<ShopPedApparel>
  <pedName>{ped}</pedName>
  <dlcName>{dlc}</dlcName>
</ShopPedApparel>"
            )
        };
        let text = shop("mp_m_freemode_01", "Male_freemode_demo");
        assert_eq!(
            collection_in(text.trim_start_matches('﻿')),
            Some((
                "mp_m_freemode_01".to_string(),
                "male_freemode_demo".to_string()
            )),
            "lower case, since the game resolves a name without regard to case"
        );
        assert_eq!(
            collection_in(&shop("mp_f_freemode_01", "female_demo"))
                .map(|c| c.1)
                .as_deref(),
            Some("female_demo")
        );
        assert_eq!(
            collection_in(&shop("a_cop", "cops")),
            None,
            "only the freemode peds"
        );
        assert_eq!(collection_in(&shop("mp_m_freemode_01", " ")), None);
        assert_eq!(collection_in("<ShopPedApparel></ShopPedApparel>"), None);
    }

    #[test]
    fn a_ped_file_is_in_the_peds_folder_of_the_archive_and_its_metadata_beside_the_folder() {
        assert_eq!(
            path("mp_m_freemode_01", "uppr_000_r.ydd"),
            "platform:/models/cdimages/streamedpeds_mp.rpf/mp_m_freemode_01/uppr_000_r.ydd"
        );
        assert_eq!(
            variation_path("mp_f_freemode_01"),
            "platform:/models/cdimages/streamedpeds_mp.rpf/mp_f_freemode_01.ymt"
        );
    }
}
