//! Names for the root types of hash-only metadata containers.
//!
//! PSO and RBM store a type as a hash of its C++ class name; RBF0 stores the
//! name, so a type the game also ships as RBF names every PSO and RBM file
//! with the same root. This table supplies the rest: class names that are
//! public knowledge — CodeWalker names all of these in `Meta/MetaTypes.cs`
//! and its `.ymt` handling, and GTAMods documents most of them.
//!
//! ⚠ A name here is accepted only when its hash matches one the game shipped,
//! so a wrong or stale entry attaches to nothing and `openrage-cli meta types`
//! reports it as unused rather than printing it. That is why it is keyed by
//! hash rather than by extension.
//!
//! This is not a schema: it names root types, 46 over a retail install, and
//! naming fields is what a schema snapshot is for.
//!
//! ⚠ **A `rage::` class is spelled `rage__`, and getting that wrong is how a
//! name that is otherwise right hashes to nothing.** The hash is over the
//! literal C++ qualified name with `::` written as `__`, so
//! `cutfCutsceneFile2` and `rage__cutfCutsceneFile2` are different strings
//! and only the second one matches.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::hash::literal_string_hash;

/// Root-type class names that are public knowledge, most files first.
pub const KNOWN: &[&str] = &[
    "CMapData",
    "rage__cutfCutsceneFile2",
    // The `.ymt` roots, biggest first. The region, fog volume and
    // procedural-object managers match a shipped hash but no known filename.
    "naOcclusionInteriorMetadata",
    "CStreamingRequestRecord",
    "CPedModelInfo__InitDataList",
    "AnimPostFXManager",
    "camMetadataStore",
    "fwClipSetManager",
    "CVfxPedInfoMgr",
    "CVfxVehicleInfoMgr",
    "CVfxRegionInfoMgr",
    "CVfxFogVolumeInfoMgr",
    "CVfxInteriorInfoMgr",
    "CVfxWeaponInfoMgr",
    "CProceduralInfo",
    "CMapTypes",
    "CPedVariationInfo",
    "CPackFileMetaData",
    "CScenarioPointRegion",
    "CMovieSubtitleContainer",
    "CCreatureMetaData",
    "CDataFileMgr__ContentsOfDataFileXml",
    "CPopGroupList",
    "CMapParentTxds",
    "CExplosionInfoManager",
    "CScenarioPointManifest",
    "CVehicleModelInfoVarGlobal",
    "CVehicleModelInfoVariation",
];

fn index() -> &'static HashMap<u32, &'static str> {
    static INDEX: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();
    INDEX.get_or_init(|| {
        KNOWN
            .iter()
            .map(|name| (literal_string_hash(name.as_bytes()), *name))
            .collect()
    })
}

/// The public name for a root-type hash, if this table knows one.
pub fn name_for(hash: u32) -> Option<&'static str> {
    index().get(&hash).copied()
}

/// The hash each known name claims, for a caller reporting which entries
/// matched nothing.
pub fn claims() -> impl Iterator<Item = (u32, &'static str)> {
    index().iter().map(|(hash, name)| (*hash, *name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each pair is a hash a retail install actually ships as a root type,
    /// beside the class name CodeWalker gives it. `0xD3593FA6` is the root
    /// of all 19,387 `.ymap`; `0x93A68A2F` of all 1,792 PSO `.ymf`, which
    /// the game's own three RBF `.ymf` independently name
    /// `CPackFileMetaData`.
    #[test]
    fn every_name_hashes_to_the_number_the_game_ships() {
        for (name, expected) in [
            ("CMapData", 0xD359_3FA6u32),
            ("CMapTypes", 0xD98B_B561),
            ("CPedVariationInfo", 0x1676_0659),
            ("CPackFileMetaData", 0x93A6_8A2F),
            ("CScenarioPointRegion", 0x58FC_EA50),
            ("CCreatureMetaData", 0x79B7_DCE5),
            ("CMovieSubtitleContainer", 0x8FF7_6A47),
            ("CMapParentTxds", 0xAEF4_5801),
            ("rage__cutfCutsceneFile2", 0xD245_5C61),
            ("naOcclusionInteriorMetadata", 0xDE5D_B4C2),
            ("CStreamingRequestRecord", 0x0819_E70E),
            ("CPedModelInfo__InitDataList", 0xDD77_771E),
            ("fwClipSetManager", 0xF061_3E4B),
            ("camMetadataStore", 0x6172_064B),
        ] {
            assert_eq!(literal_string_hash(name.as_bytes()), expected, "{name}");
            assert_eq!(name_for(expected), Some(name));
        }
    }

    /// ⚠ The case-folding hash is one keystroke from the literal one, and a
    /// table built with it silently matches zero rows.
    #[test]
    fn the_literal_hash_is_the_one_a_container_stores() {
        assert_ne!(
            crate::hash::string_hash(b"CMapData"),
            0xD359_3FA6,
            "if these agreed nothing here could tell the two functions apart"
        );
    }

    #[test]
    fn no_two_names_claim_the_same_hash() {
        assert_eq!(index().len(), KNOWN.len(), "two entries collide");
    }

    #[test]
    fn an_unknown_hash_is_not_given_a_nearby_name() {
        assert_eq!(name_for(0xDEAD_BEEF), None);
        // `0xA89B70F0` is `physicstasks.ymt` and `0x771AE97C` is
        // `credits.ymt`, but no public source names their classes, and a
        // plausible-looking guess is what this table must not carry.
        assert_eq!(name_for(0xA89B_70F0), None);
        assert_eq!(name_for(0x771A_E97C), None);
    }

    /// PSO, RBM and RBF0 all use `.ymt`: `naOcclusionInteriorMetadata` files
    /// are PSO, `CStreamingRequestRecord` files are RBM, and `CMapTypes` ships
    /// as all three.
    #[test]
    fn the_ymt_roots_are_keyed_by_hash_rather_than_by_extension() {
        for name in [
            "naOcclusionInteriorMetadata",
            "CStreamingRequestRecord",
            "CPedModelInfo__InitDataList",
        ] {
            let hash = literal_string_hash(name.as_bytes());
            assert_eq!(name_for(hash), Some(name), "{name}");
        }
    }

    #[test]
    fn claims_reports_every_entry_once() {
        let claimed: Vec<&str> = claims().map(|(_, name)| name).collect();
        assert_eq!(claimed.len(), KNOWN.len());
        for name in KNOWN {
            assert!(claimed.contains(name), "{name} is not claimed");
        }
    }
}
