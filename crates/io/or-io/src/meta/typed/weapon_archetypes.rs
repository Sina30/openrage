//! The root of a weapon archetypes file (`CWeaponModelInfo::InitDataList`): the model, texture dictionary and effects asset a weapon is drawn with.

use super::weapon::{open_text, save_text};
use super::*;

meta_record! {
    /// `CWeaponModelInfo::InitDataList`, as the install's weapon metadata states it.
    CWeaponModelInfoInitDataList = "CWeaponModelInfo__InitDataList" {
        init_datas: Vec<CWeaponModelInfoInitData> = "InitDatas",
    }
}

meta_record! {
    /// `CWeaponModelInfo::InitData`, as the install's weapon metadata states it.
    CWeaponModelInfoInitData = "CWeaponModelInfo__InitData" {
        expression_dictionary_name: String = "ExpressionDictionaryName",
        expression_name: String = "ExpressionName",
        lod_dist: i32 = "lodDist",
        model_name: String = "modelName",
        ptfx_asset_name: String = "ptfxAssetName",
        txd_name: String = "txdName",
    }
}

meta_origin!(CWeaponModelInfoInitDataList);

impl File for CWeaponModelInfoInitDataList {
    const EXTENSION: &'static str = "meta";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let (mut root, arrival) = open_text("CWeaponModelInfo__InitDataList", bytes, Self::read)?;
        root.carry.arrival = Some(Box::new(arrival));
        Ok(root)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        save_text(self.write())
    }
}
