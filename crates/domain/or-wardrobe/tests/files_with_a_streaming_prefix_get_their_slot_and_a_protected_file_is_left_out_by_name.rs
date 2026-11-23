//! A name behind the `dlc^` streaming prefix states its garment as a bare name does, and a file that opens with
//! `FXAP` (a protected asset, no RSC7 resource) is left out by its name with the reason, in a drop and in an import.

mod common;

use common::{ydd, ytd};
use std::path::{Path, PathBuf};

use or_wardrobe::edit::add_clothes;
use or_wardrobe::import::{import, Options};
use or_wardrobe::{Component, Ped, Slot, Wardrobe};

const C: &str = "mp_m_freemode_01_mp_m_x";

fn module(dir: &Path) -> PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    module
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, bytes).unwrap();
    at
}

#[test]
fn files_behind_the_dlc_streaming_prefix_get_their_ped_slot_and_textures() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("stream");
    put(
        &drop,
        &format!("dlc^{C}^jbib_001_u.ydd"),
        &ydd("jacket one"),
    );
    put(
        &drop,
        &format!("dlc^{C}^jbib_diff_001_a_uni.ytd"),
        &ytd("tex a"),
    );
    put(
        &drop,
        &format!("dlc^{C}^jbib_diff_001_b_uni.ytd"),
        &ytd("tex b"),
    );
    put(&drop, &format!("dlc^{C}^lowr_000_r.ydd"), &ydd("trousers"));

    let added = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    assert_eq!(added.garments.len(), 2, "{added:?}");
    assert_eq!(
        (added.unread, added.textures, added.left_out.len()),
        (0, 2, 0),
        "{added:?}"
    );
    let pack = Wardrobe::read(&module).unwrap();
    let jacket = pack
        .garments
        .iter()
        .find(|g| g.slot == Some(Slot::Component(Component::Jbib)))
        .expect("the jacket has its slot");
    assert_eq!(jacket.ped, Some(Ped::Male));
    assert_eq!(jacket.variations.len(), 2, "its textures went with it");
    assert!(pack
        .garments
        .iter()
        .any(|g| g.slot == Some(Slot::Component(Component::Lowr)) && g.ped == Some(Ped::Male)));
}

#[test]
fn a_protected_file_in_a_drop_is_named_in_the_left_out_list_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    put(&drop, &format!("{C}^jbib_002_u.ydd"), b"FXAP\0\0\0\0sealed");
    put(
        &drop,
        &format!("{C}^jbib_diff_002_a_uni.ytd"),
        b"tex of the sealed one",
    );

    let added = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    assert_eq!(added.garments.len(), 1, "the sealed model makes no garment");
    let sealed = added
        .left_out
        .iter()
        .find(|l| l.path.ends_with("jbib_002_u.ydd"))
        .unwrap_or_else(|| panic!("the file is named: {:?}", added.left_out));
    assert!(
        sealed.why.contains("FXAP") && sealed.why.contains("protected"),
        "{sealed:?}"
    );
    assert!(
        added
            .left_out
            .iter()
            .any(|l| l.path.ends_with("jbib_diff_002_a_uni.ytd")),
        "its texture has no model to go with: {:?}",
        added.left_out
    );
    assert!(
        !module
            .join("models")
            .join(format!("{C}^jbib_002_u.ydd"))
            .exists(),
        "the sealed file is not copied"
    );
}

#[test]
fn a_protected_file_in_a_folder_that_is_imported_is_named_in_the_left_out_list() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    put(&pack, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    put(&pack, &format!("{C}^jbib_diff_001_a_uni.ytd"), &ytd("tex"));
    put(&pack, &format!("{C}^jbib_002_u.ydd"), b"FXAP\0\0\0\0sealed");

    let imported = import(&pack, &Options::default()).unwrap();

    assert_eq!(imported.report.models, 1);
    let sealed = imported
        .report
        .left_out
        .iter()
        .find(|l| l.path.ends_with("jbib_002_u.ydd"))
        .unwrap_or_else(|| panic!("{:?}", imported.report.left_out));
    assert!(sealed.why.contains("FXAP"), "{sealed:?}");
}
