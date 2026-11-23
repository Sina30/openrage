//! The skeleton each drawable entry of a dictionary opened from a scan holds, read when it opened.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use or_io::gta6::headerless::{read_dictionary_skeletons, Resource, Skeleton};

use super::{DocPayload, Document};
use crate::ui::panels::selection_ids::{
    dict_entry_unpack_part, is_dict_entry_selection, YPT_MODEL_KIND_BONE,
};

type Held = BTreeMap<PathBuf, Vec<(u32, Skeleton)>>;

// ponytail: one map for the whole app, which no close empties; skeletons are small.
static ENTRIES: Mutex<Held> = Mutex::new(BTreeMap::new());

fn entries() -> MutexGuard<'static, Held> {
    ENTRIES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keep the skeletons of the drawable entries of the dictionary at the root of `virtual_section`,
/// opened at `path`; one whose skeletons do not read keeps none, and its entries export with no bones.
pub(super) fn keep(path: &Path, virtual_section: &[u8]) {
    let skeletons = read_dictionary_skeletons(virtual_section).unwrap_or_default();
    entries().insert(path.to_path_buf(), skeletons);
}

/// The skeleton entry `code` of the dictionary opened at `path` holds.
pub(crate) fn of(path: &Path, code: u32) -> Option<Skeleton> {
    (entries().get(path)?.iter())
        .find(|(c, _)| *c == code)
        .map(|(_, skeleton)| skeleton.clone())
}

/// Hold `skeletons` as the ones the entries of the dictionary opened at `path` hold.
#[cfg(test)]
pub(crate) fn hold(path: &Path, skeletons: Vec<(u32, Skeleton)>) {
    entries().insert(path.to_path_buf(), skeletons);
}

/// Every entry skeleton held for the dictionary opened at `path`, by code.
pub(super) fn all(path: &Path) -> Vec<(u32, Skeleton)> {
    entries().get(path).cloned().unwrap_or_default()
}

/// `skeleton` swapped with entry `code`'s of the dictionary at `path`; `false` where none is held.
pub(super) fn swap(path: &Path, code: u32, skeleton: &mut Skeleton) -> bool {
    let mut held = entries();
    let found = (held.get_mut(path).into_iter().flatten()).find(|(c, _)| *c == code);
    match found {
        Some((_, kept)) => {
            std::mem::swap(kept, skeleton);
            true
        }
        None => false,
    }
}

/// The code of the dictionary entry `selection` is in, where `doc` is a scanned dictionary.
pub(crate) fn entry_of(doc: &Document, selection: Option<u64>) -> Option<u32> {
    let DocPayload::Gta6Drawable(resource) = &doc.payload else {
        return None;
    };
    if !matches!(**resource, Resource::Dictionary(_)) {
        return None;
    }
    let selection = selection.filter(|s| is_dict_entry_selection(*s))?;
    Some(dict_entry_unpack_part(selection).0)
}

/// The entry code and the bone `selection` names in a dictionary's Outline, where it names a bone.
pub(crate) fn entry_bone_of(selection: u64) -> Option<(u32, usize)> {
    if !is_dict_entry_selection(selection) {
        return None;
    }
    let (code, kind, bone) = dict_entry_unpack_part(selection);
    (kind == YPT_MODEL_KIND_BONE).then_some((code, bone as usize))
}

/// The skeleton `doc` shows for `selection`: its own, or the one its entry selected holds.
pub(crate) fn shown(doc: &Document, selection: Option<u64>) -> Option<Skeleton> {
    let path = doc.path.as_deref()?;
    match entry_of(doc, selection) {
        Some(code) => of(path, code),
        None => super::rpf9_scan::scanned_skeleton(path).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::headerless::Bone;

    fn skeleton(name: &str) -> Skeleton {
        Skeleton {
            bones: vec![Bone {
                name: name.to_string(),
                hash: or_io::gta6::hash::name_hash32(name.as_bytes()),
                parent: None,
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation: [0.0; 3],
                scale: [1.0; 3],
                inverse_bind: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]],
            }],
        }
    }

    #[test]
    fn an_entry_s_skeleton_is_found_by_its_code_and_a_body_that_does_not_read_keeps_none() {
        let path = Path::new("entry_skeletons_test/a.ydd");
        entries().insert(
            path.to_path_buf(),
            vec![
                (0x1111_2222, skeleton("SKEL_ROOT")),
                (0x3333_4444, skeleton("SKEL_Pelvis")),
            ],
        );
        let found = of(path, 0x3333_4444).expect("kept");
        assert_eq!(found.bones[0].name, "SKEL_Pelvis");
        assert_eq!(of(path, 0x5555_6666), None);
        keep(path, &[0; 4]);
        assert_eq!(
            of(path, 0x1111_2222),
            None,
            "what the dictionary held before is gone"
        );
    }

    #[test]
    fn a_swap_exchanges_only_the_entry_named_and_an_absent_one_is_refused() {
        let path = Path::new("entry_skeletons_test/swap.ydd");
        hold(
            path,
            vec![
                (0x1111_2222, skeleton("SKEL_ROOT")),
                (0x3333_4444, skeleton("SKEL_Pelvis")),
            ],
        );
        let mut other = skeleton("SKEL_Renamed");
        assert!(swap(path, 0x3333_4444, &mut other));
        assert_eq!(other.bones[0].name, "SKEL_Pelvis");
        let names: Vec<String> = (all(path).into_iter())
            .map(|(_, s)| s.bones[0].name.clone())
            .collect();
        assert_eq!(names, ["SKEL_ROOT", "SKEL_Renamed"]);
        assert!(!swap(path, 0x5555_6666, &mut other));
        assert!(!swap(
            Path::new("entry_skeletons_test/none.ydd"),
            0x1111_2222,
            &mut other
        ));
        assert_eq!(
            other.bones[0].name, "SKEL_Pelvis",
            "a refused swap changes nothing"
        );
    }

    #[test]
    fn a_bone_row_names_its_entry_and_its_index_and_no_other_row_does() {
        use crate::ui::panels::selection_ids::{
            dict_entry_id, dict_entry_part_id, pack_index, DOC_ROOT_SEL, SECT_SKEL,
            YPT_MODEL_KIND_LOD,
        };
        let row = dict_entry_part_id(0x3333_4444, YPT_MODEL_KIND_BONE, 7);
        assert_eq!(entry_bone_of(row), Some((0x3333_4444, 7)));
        let others = [
            dict_entry_id(0x3333_4444),
            dict_entry_part_id(0x3333_4444, YPT_MODEL_KIND_LOD, 7),
            pack_index(SECT_SKEL, 7),
            DOC_ROOT_SEL,
        ];
        for other in others {
            assert_eq!(entry_bone_of(other), None, "{other:#X}");
        }
    }
}
