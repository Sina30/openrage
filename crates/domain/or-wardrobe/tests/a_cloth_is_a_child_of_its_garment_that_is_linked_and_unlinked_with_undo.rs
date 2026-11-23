//! A garment has cloth when one is linked, and the flag that says so is written from the link:
//! linking turns it on, unlinking turns it off, a garment whose metadata stated the flag with no cloth
//! is listed as missing it until one is linked, and every one of these is an edit Undo takes back.

use std::path::{Path, PathBuf};

mod common;

use common::{ydd, ytd};
use or_wardrobe::edit::{self, add_clothes};
use or_wardrobe::{inspect, plan, Depth, Ped, Planned, Slot, Wardrobe};

const M: &str = "mp_m_freemode_01_mp_m_x";

fn put(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let to = dir.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(&to, bytes).unwrap();
    to
}

/// A pack of one jacket, in `dir/clothing`, and the folder it was dropped from.
fn a_pack_with_a_jacket(dir: &Path) -> (PathBuf, String) {
    let module = dir.join("clothing");
    edit::create(&module, "demo").unwrap();
    let drop = dir.join("drop");
    put(&drop, &format!("{M}^jbib_000_u.ydd"), &ydd("jacket"));
    put(&drop, &format!("{M}^jbib_diff_000_a_uni.ytd"), &ytd("tex"));
    let added = add_clothes(&module, std::slice::from_ref(&drop)).expect("the drop");
    assert_eq!(added.garments.len(), 1);
    (module, added.garments[0].clone())
}

fn planned(module: &Path) -> Planned {
    let w = Wardrobe::read(module).unwrap();
    plan(&w, &inspect(module, &w, Depth::Stat), "wardrobe", module)
}

/// Whether the variation metadata the plan writes for the male ped states `ownsCloth` for its first drawable.
fn states_the_flag(planned: &Planned) -> bool {
    let info = &planned.documents.variations[&Ped::Male];
    info.a_component_data3[0].a_drawbl_data3[0]
        .cloth_data
        .owns_cloth
}

fn codes(planned: &Planned) -> Vec<&'static str> {
    planned
        .findings
        .iter()
        .map(|f| f.code)
        .filter(|c| c.starts_with("cloth"))
        .collect()
}

#[test]
fn linking_a_cloth_turns_the_flag_on_and_unlinking_turns_it_off() {
    let dir = tempfile::tempdir().unwrap();
    let (module, id) = a_pack_with_a_jacket(dir.path());
    assert!(!states_the_flag(&planned(&module)), "no cloth, no flag");

    let yld = put(&dir.path().join("elsewhere"), "my_jacket.yld", b"cloth");
    let linked = edit::link_cloth(&module, &id, &yld).expect("the cloth is linked");
    let w = Wardrobe::read(&module).unwrap();
    let cloth = w.garment(&id).unwrap().cloth.clone().expect("it is linked");
    assert_eq!(cloth, "cloth/my_jacket.yld");
    assert_eq!(linked.copied, std::slice::from_ref(&cloth));
    assert_eq!(std::fs::read(module.join(&cloth)).unwrap(), b"cloth");
    let after = planned(&module);
    assert!(states_the_flag(&after), "the flag follows the link");
    assert!(
        after
            .contribution
            .files
            .iter()
            .any(|f| f.path.ends_with("^jbib_000_u.yld")),
        "the plan ships it under the model's name"
    );
    assert!(codes(&after).is_empty(), "{:?}", after.findings);

    let unlinked = edit::unlink_cloth(&module, &id).expect("the cloth is unlinked");
    let w = Wardrobe::read(&module).unwrap();
    assert_eq!(w.garment(&id).unwrap().cloth, None);
    assert!(!states_the_flag(&planned(&module)), "unlinked, no flag");
    assert!(
        module.join(&cloth).exists(),
        "unlinking does not touch the file"
    );

    // Undo of the unlink puts the link back, and Undo of the link takes the file it copied away.
    edit::undo_cloth(&module, &unlinked).unwrap();
    assert_eq!(
        Wardrobe::read(&module).unwrap().garment(&id).unwrap().cloth,
        Some(cloth.clone())
    );
    assert!(states_the_flag(&planned(&module)));
    edit::undo_cloth(&module, &linked).unwrap();
    assert_eq!(
        Wardrobe::read(&module).unwrap().garment(&id).unwrap().cloth,
        None
    );
    assert!(!module.join(&cloth).exists() && !module.join("cloth").exists());
    assert!(yld.exists(), "the file linked from is never touched");
}

#[test]
fn a_garment_that_states_the_flag_with_no_cloth_is_listed_until_one_is_linked_and_undo_lists_it_again(
) {
    let dir = tempfile::tempdir().unwrap();
    let (module, id) = a_pack_with_a_jacket(dir.path());
    edit::update(&module, |w| {
        w.garments[0].missing_cloth = true;
        Ok(())
    })
    .unwrap();
    let before = planned(&module);
    assert_eq!(codes(&before), ["cloth-missing"], "{:?}", before.findings);
    assert!(states_the_flag(&before), "the flag stays as it was read");
    assert!(
        !before.contribution.problems.is_empty(),
        "an error a build stops on"
    );

    let yld = put(&dir.path().join("elsewhere"), "jacket.yld", b"cloth");
    let linked = edit::link_cloth(&module, &id, &yld).unwrap();
    let w = Wardrobe::read(&module).unwrap();
    assert!(!w.garment(&id).unwrap().missing_cloth);
    let after = planned(&module);
    assert!(codes(&after).is_empty(), "{:?}", after.findings);
    assert!(after.contribution.problems.is_empty());

    edit::undo_cloth(&module, &linked).unwrap();
    let w = Wardrobe::read(&module).unwrap();
    assert!(w.garment(&id).unwrap().missing_cloth && w.garment(&id).unwrap().cloth.is_none());
    assert_eq!(codes(&planned(&module)), ["cloth-missing"]);

    // Unlinking a garment that has none, and that states none, is refused; one that is missing its cloth stops expecting it.
    edit::unlink_cloth(&module, &id).expect("the claim of a cloth is taken back");
    assert!(!states_the_flag(&planned(&module)));
    assert!(edit::unlink_cloth(&module, &id).is_err());
}

#[test]
fn a_cloth_is_refused_for_a_prop_a_place_held_and_a_file_that_is_no_cloth() {
    let dir = tempfile::tempdir().unwrap();
    let (module, id) = a_pack_with_a_jacket(dir.path());
    let yld = put(&dir.path().join("elsewhere"), "jacket.yld", b"cloth");

    let not_cloth = put(&dir.path().join("elsewhere"), "jacket.ydd", b"model");
    assert!(edit::link_cloth(&module, &id, &not_cloth).is_err());
    assert!(edit::link_cloth(&module, &id, &dir.path().join("elsewhere")).is_err());
    assert!(edit::link_cloth(&module, "no-such-garment", &yld).is_err());

    edit::update(&module, |w| {
        w.garments[0].slot = Some(Slot::Prop(or_wardrobe::Prop::Head));
        Ok(())
    })
    .unwrap();
    let why = edit::link_cloth(&module, &id, &yld)
        .unwrap_err()
        .to_string();
    assert!(why.contains("a prop owns no cloth"), "{why}");

    edit::update(&module, |w| {
        w.garments[0].slot = Some("jbib".parse().unwrap());
        w.garments[0].model = None;
        Ok(())
    })
    .unwrap();
    let why = edit::link_cloth(&module, &id, &yld)
        .unwrap_err()
        .to_string();
    assert!(why.contains("no model"), "{why}");
    assert!(!module.join("cloth").exists(), "a refusal copies nothing");
}

#[test]
fn a_cloth_linked_to_a_prop_or_a_place_held_is_ignored_by_the_game_and_the_pack_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (module, _) = a_pack_with_a_jacket(dir.path());
    edit::update(&module, |w| {
        w.garments[0].slot = Some(Slot::Prop(or_wardrobe::Prop::Head));
        w.garments[0].cloth = Some("cloth/by-hand.yld".into());
        Ok(())
    })
    .unwrap();
    put(&module, "cloth/by-hand.yld", b"cloth");
    let found = planned(&module);
    assert!(
        codes(&found).contains(&"cloth-ignored"),
        "{:?}",
        found.findings
    );
    assert!(
        !found
            .findings
            .iter()
            .find(|f| f.code == "cloth-ignored")
            .unwrap()
            .blocks(),
        "ignored cloth is a warning, not a stop"
    );
}
