//! A small server pack laid out by hand, with shop metadata of its own: the shape a server's resource has, in no real name.
//!
//! The ids are the kinds a real shop file holds: a name the game hashes, a literal value of 12
//! hexadecimal digits that a 32 bit hash does not hold, and one with an entity in it. The pack is
//! written with small valid files for its models and textures, because a drop decompiles them and the shop is read from names.

#![allow(dead_code)]

use std::path::Path;

pub const M: &str = "mp_m_freemode_01_mp_m_demo";
pub const F: &str = "mp_f_freemode_01_mp_f_demo";
pub const MP: &str = "mp_m_freemode_01_p_mp_m_demo";

/// The directive the demo server registers its own shop file with, which is not `data_file`.
pub const OWN_DIRECTIVE: &str = "server_data_file";
pub const OWN_TYPE: &str = "SHOP_PED_APPAREL_SPAK_FILE";
pub const SPAK: &[u8] = b"SPAK a server's own extension, which this engine does not read";

/// An id that does not fit 32 bits: 12 hexadecimal digits.
pub const LONG_ID: &str = "1A2B3C4D5E6F";

/// One component entry of a shop file.
pub fn component(
    cost: &str,
    id: &str,
    drawable: u32,
    comp: &str,
    texture: u32,
    extra: &str,
) -> String {
    format!(
        "    <Item>\r\n      <cost value=\"{cost}\"/>\r\n      <uniqueNameHash>{id}</uniqueNameHash>\r\n      <localDrawableIndex value=\"{drawable}\"/>\r\n      <eCompType>{comp}</eCompType>\r\n      <textureIndex value=\"{texture}\"/>\r\n{extra}    </Item>\r\n"
    )
}

/// One prop entry of a shop file.
pub fn prop(id: &str, index: u32, anchor: &str, texture: u32, tags: &str) -> String {
    format!(
        "    <Item>\r\n      <cost value=\"0\"/>\r\n      <uniqueNameHash>{id}</uniqueNameHash>\r\n{tags}      <localPropIndex value=\"{index}\"/>\r\n      <eAnchorPoint>{anchor}</eAnchorPoint>\r\n      <textureIndex value=\"{texture}\"/>\r\n    </Item>\r\n"
    )
}

pub const FORCED: &str = "      <forcedComponents>\r\n        <Item>\r\n          <nameHash>0</nameHash>\r\n          <enumValue value=\"14\"/>\r\n          <eCompType>PV_COMP_UPPR</eCompType>\r\n        </Item>\r\n      </forcedComponents>\r\n";
pub const HAT_TAG: &str = "      <restrictionTags>\r\n        <Item>\r\n          <tagNameHash>HAT</tagNameHash>\r\n        </Item>\r\n      </restrictionTags>\r\n";

pub fn shop_file(ped: &str, collection: &str, components: &str, props: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<ShopPedApparel>\r\n  <pedName>{ped}</pedName>\r\n  <dlcName>{collection}</dlcName>\r\n  <fullDlcName>{ped}_{collection}</fullDlcName>\r\n  <eCharacter>SCR_CHAR_MULTIPLAYER</eCharacter>\r\n  <pedComponents>\r\n{components}  </pedComponents>\r\n  <pedProps>\r\n{props}  </pedProps>\r\n</ShopPedApparel>\r\n"
    )
}

/// The male shop file: six component entries (three of them point at nothing) and one prop.
pub fn male_shop() -> String {
    let comps = [
        component("0", "65D90E59C0AE", 0, "PV_COMP_JBIB", 0, ""),
        component("150", "DEMO_M_JBIB_0_B", 0, "PV_COMP_JBIB", 1, FORCED),
        component("0", LONG_ID, 1, "PV_COMP_JBIB", 0, ""),
        // A switched-off entry is no entry.
        "    <!-- <Item><uniqueNameHash>OFF</uniqueNameHash></Item> -->\r\n".to_string(),
        component("0", "DEMO&amp;AMP", 9, "PV_COMP_JBIB", 0, ""),
        component("0", "DEMO_M_JBIB_1_Z", 1, "PV_COMP_JBIB", 5, ""),
        component("0", "DEMO_M_JBIB_1_AGAIN", 1, "PV_COMP_JBIB", 0, ""),
    ]
    .concat();
    shop_file(
        "mp_m_freemode_01",
        "mp_m_demo",
        &comps,
        &prop("DEMO_DUP", 0, "ANCHOR_HEAD", 0, HAT_TAG),
    )
}

/// The female shop file: one entry, whose id is the prop's of the male file in another case.
pub fn female_shop() -> String {
    shop_file(
        "mp_f_freemode_01",
        "mp_f_demo",
        &component("0", "demo_dup", 0, "PV_COMP_JBIB", 0, ""),
        "",
    )
}

pub const MANIFEST: &str = "fx_version 'cerulean'\r\ngame 'gta5'\r\n\r\nfiles {\r\n  'data/mp_m_freemode_01_mp_m_demo.meta',\r\n  'data/mp_f_freemode_01_mp_f_demo.meta',\r\n  'data/mp_m_freemode_01_mp_m_demo.spak',\r\n}\r\n\r\n-- data_file 'SHOP_PED_APPAREL_META_FILE' 'data/never.meta'\r\ndata_file 'SHOP_PED_APPAREL_META_FILE' 'data/mp_m_freemode_01_mp_m_demo.meta'\r\ndata_file 'SHOP_PED_APPAREL_META_FILE' 'data/mp_f_freemode_01_mp_f_demo.meta'\r\nserver_data_file 'SHOP_PED_APPAREL_SPAK_FILE' 'data/mp_m_freemode_01_mp_m_demo.spak'\r\n";

fn put(dir: &Path, rel: &str, bytes: &[u8]) {
    let to = dir.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, bytes).unwrap();
}

/// The garments of the demo pack, by the file names that state them.
pub fn files(dir: &Path) {
    let jbib = format!("stream/demo/{M}/jbib");
    put(
        dir,
        &format!("{jbib}/{M}^jbib_000_u.ydd"),
        &super::ydd("jacket"),
    );
    for letter in ["a", "b", "c"] {
        put(
            dir,
            &format!("{jbib}/{M}^jbib_diff_000_{letter}_uni.ytd"),
            &super::ytd(letter),
        );
    }
    put(
        dir,
        &format!("{jbib}/{M}^jbib_001_u.ydd"),
        &super::ydd("shirt"),
    );
    put(
        dir,
        &format!("{jbib}/{M}^jbib_diff_001_a_uni.ytd"),
        &super::ytd("s"),
    );
    let lowr = format!("stream/demo/{M}/lowr");
    put(
        dir,
        &format!("{lowr}/{M}^lowr_000_r.ydd"),
        &super::ydd("trousers"),
    );
    put(
        dir,
        &format!("{lowr}/{M}^lowr_diff_000_a_whi.ytd"),
        &super::ytd("t"),
    );
    let head = format!("stream/demo/{MP}/p_head");
    put(
        dir,
        &format!("{head}/{MP}^p_head_000.ydd"),
        &super::ydd("hat"),
    );
    put(
        dir,
        &format!("{head}/{MP}^p_head_diff_000_a.ytd"),
        &super::ytd("h"),
    );
    let female = format!("stream/demo/{F}/jbib");
    put(
        dir,
        &format!("{female}/{F}^jbib_000_u.ydd"),
        &super::ydd("blouse"),
    );
    put(
        dir,
        &format!("{female}/{F}^jbib_diff_000_a_uni.ytd"),
        &super::ytd("b"),
    );
}

/// The whole demo resource: the garments, both shop files, the server's own file and the manifest.
pub fn resource(dir: &Path) {
    files(dir);
    put(
        dir,
        "data/mp_m_freemode_01_mp_m_demo.meta",
        male_shop().as_bytes(),
    );
    put(
        dir,
        "data/mp_f_freemode_01_mp_f_demo.meta",
        female_shop().as_bytes(),
    );
    put(dir, "data/mp_m_freemode_01_mp_m_demo.spak", SPAK);
    put(dir, "fxmanifest.lua", MANIFEST.as_bytes());
}
