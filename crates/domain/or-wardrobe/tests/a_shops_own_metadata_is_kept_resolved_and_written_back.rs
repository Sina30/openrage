//! A server's own shop metadata: its ids kept exactly as written, resolved to the garments and
//! textures they state, listed where they do not, written back unchanged by a build, and read again
//! by a second import to the same ids.
//!
//! The pack is laid out by hand with dummy bytes (`common::demo`): the shop is read from names and
//! the metadata, and nothing here opens a model. The owner's own server pack is held to the same
//! in `a_server_pack_with_its_own_shop_metadata_is_imported_and_written_back`.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::demo::{self, F, LONG_ID, M, MP, OWN_DIRECTIVE, OWN_TYPE, SPAK};
use common::{project, snapshot, FEET, JBIB};
use or_project::Origin;
use or_wardrobe::edit::{self, add_clothes};
use or_wardrobe::import::{import, Imported, Options};
use or_wardrobe::{
    allocate, inspect, plan, plans, shop, verify, Carried, Depth, Ped, Slot, Wardrobe,
};

const MALE_META: &str = "data/mp_m_freemode_01_mp_m_demo.meta";
const FEMALE_META: &str = "data/mp_f_freemode_01_mp_f_demo.meta";

/// The id each texture is written with by what it points at, for the textures that state one.
fn stated_ids(w: &Wardrobe) -> BTreeMap<(Ped, Slot, u32, usize), String> {
    let mut ids = BTreeMap::new();
    for g in &w.garments {
        for (t, v) in g.variations.iter().enumerate() {
            if let (Some(ped), Some(slot), Some(position), Some(entry)) =
                (g.ped, g.slot, g.position, &v.shop)
            {
                ids.insert((ped, slot, position, t), entry.id.clone());
            }
        }
    }
    ids
}

fn imported(dir: &Path) -> Imported {
    demo::resource(dir);
    import(dir, &Options::default()).expect("the demo resource imports")
}

/// Write the files of a plan into `dest`, as a build would, with no reader asked.
fn write_plan(plan: &or_project::Plan, dest: &Path) {
    for file in &plan.files {
        let to = dest.join(&file.path);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        match &file.origin {
            Origin::Copy { from, .. } | Origin::Retarget { from, .. } => {
                std::fs::copy(from, &to).unwrap();
            }
            Origin::Generated(bytes) => std::fs::write(&to, bytes).unwrap(),
            Origin::Document(text) => std::fs::write(&to, text).unwrap(),
            Origin::Cook { .. } => panic!("a clothing pack cooks no source: {}", file.path),
        }
    }
}

#[test]
fn a_folder_holding_shop_metadata_resolves_every_entry_the_pack_allows_and_lists_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    demo::resource(dir.path());
    let before = snapshot(dir.path());
    let imported = import(dir.path(), &Options::default()).expect("the demo resource imports");
    assert_eq!(snapshot(dir.path()), before, "an import writes nothing");
    let (w, shop) = (&imported.wardrobe, &imported.report.shop);

    // The two files a manifest registers as shop apparel data, and nothing a comment switched off.
    assert_eq!(shop.files.len(), 2, "{:?}", shop.files);
    assert_eq!(shop.entries, 8);
    assert_eq!(shop.resolved, 5);
    assert!(shop.unread.is_empty(), "{:?}", shop.unread);

    // Every id is the text the file wrote, whatever it looks like.
    let ids = stated_ids(w);
    let id = |ped, slot, position, texture| ids[&(ped, slot, position, texture)].as_str();
    assert_eq!(id(Ped::Male, JBIB, 0, 0), "65D90E59C0AE");
    assert_eq!(id(Ped::Male, JBIB, 0, 1), "DEMO_M_JBIB_0_B");
    assert_eq!(id(Ped::Male, JBIB, 1, 0), LONG_ID);
    assert!(
        u32::from_str_radix(LONG_ID, 16).is_err() && LONG_ID.len() == 12,
        "the id is 12 hexadecimal digits, which 32 bits do not hold"
    );
    assert_eq!(id(Ped::Male, common::P_HEAD, 0, 0), "DEMO_DUP");
    assert_eq!(id(Ped::Female, JBIB, 0, 0), "demo_dup");
    assert_eq!(ids.len(), 5);

    // The cost and the elements the plan does not name travel with the id, with where they came from.
    let jacket = w
        .garments
        .iter()
        .find(|g| g.ped == Some(Ped::Male) && g.slot == Some(JBIB) && g.position == Some(0))
        .unwrap();
    let entry = jacket.variations[1].shop.as_ref().unwrap();
    assert_eq!(entry.cost.as_deref(), Some("150"));
    assert_eq!(entry.from, MALE_META);
    assert_eq!(entry.entry, 1);
    assert_eq!(entry.fields.len(), 1);
    assert!(
        entry.fields[0].starts_with("<forcedComponents>"),
        "{:?}",
        entry.fields
    );
    assert_eq!(
        jacket.variations[0].shop.as_ref().unwrap().cost.as_deref(),
        Some("0")
    );

    // What points at nothing is kept as stated and listed with the reason; none is dropped.
    let unplaced: Vec<_> = w
        .shop
        .unplaced
        .iter()
        .map(|s| s.entry.id.as_str())
        .collect();
    assert_eq!(
        unplaced,
        ["DEMO&AMP", "DEMO_M_JBIB_1_Z", "DEMO_M_JBIB_1_AGAIN"]
    );
    assert_eq!(shop.unplaced.len(), 3);
    let why = |at: usize| shop.unplaced[at].why.as_str();
    assert!(why(0).contains("no such drawable"), "{}", why(0));
    assert!(why(1).contains("no such drawable"), "{}", why(1));
    assert!(why(2).contains("second entry"), "{}", why(2));
    assert_eq!(w.shop.unplaced[1].texture, 5);
    assert_eq!(w.shop.unplaced[0].drawable, 9);

    // The garments with no entry for a texture are listed, and the build names those.
    assert_eq!(shop.unentried.len(), 2, "{:?}", shop.unentried);
    assert!(shop
        .unentried
        .iter()
        .any(|l| l.why.contains("texture(s) c")));
    assert!(shop
        .unentried
        .iter()
        .any(|l| l.why.contains("texture(s) a")));

    // Two entries claim one id, without regard to case.
    assert_eq!(shop.clashes.len(), 1, "{:?}", shop.clashes);
    assert!(shop.clashes[0].path.eq_ignore_ascii_case("demo_dup"));

    // The file the server registered under its own directive travels as it is.
    assert_eq!(
        w.shop.carried,
        [Carried {
            file: "data/mp_m_freemode_01_mp_m_demo.spak".into(),
            directive: OWN_DIRECTIVE.into(),
            kind: OWN_TYPE.into(),
        }]
    );
    assert_eq!(shop.carried.len(), 1);
    // The build writes the shop metadata again, so the files it was read from are the pack's.
    assert!(imported.report.metadata.iter().any(|p| p == MALE_META));
}

#[test]
fn a_build_writes_the_imported_ids_back_unchanged_and_a_second_import_gives_the_same_ids() {
    let dir = tempfile::tempdir().unwrap();
    let first = imported(dir.path());
    let source = project(dir.path(), &first.wardrobe, "demo_pack");
    let built = plans(&source, "fivem").unwrap().remove(0);
    assert!(built.problems.is_empty(), "{:?}", built.problems);

    let out = tempfile::tempdir().unwrap();
    write_plan(&built, out.path());
    let shop = std::fs::read_to_string(out.path().join(MALE_META)).unwrap();
    for stated in [
        "<uniqueNameHash>65D90E59C0AE</uniqueNameHash>",
        "<uniqueNameHash>DEMO_M_JBIB_0_B</uniqueNameHash>",
        &format!("<uniqueNameHash>{LONG_ID}</uniqueNameHash>"),
        "<uniqueNameHash>DEMO&amp;AMP</uniqueNameHash>",
        "<cost value=\"150\"/>",
        "<forcedComponents>",
        "<tagNameHash>HAT</tagNameHash>",
    ] {
        assert!(shop.contains(stated), "{stated} is not in\n{shop}");
    }
    // A garment the shop file stated nothing for is named by the build.
    assert!(
        shop.contains("<uniqueNameHash>MP_M_DEMO_LOWR_000_0</uniqueNameHash>"),
        "{shop}"
    );
    assert!(
        shop.contains("<uniqueNameHash>MP_M_DEMO_JBIB_000_2</uniqueNameHash>"),
        "{shop}"
    );

    // The server's own file ships beside the rest, under the line that registered it.
    assert_eq!(
        std::fs::read(out.path().join("data/mp_m_freemode_01_mp_m_demo.spak")).unwrap(),
        SPAK
    );
    let manifest = std::fs::read_to_string(out.path().join("fxmanifest.lua")).unwrap();
    let own = format!("{OWN_DIRECTIVE} '{OWN_TYPE}' 'data/mp_m_freemode_01_mp_m_demo.spak'");
    assert!(manifest.contains(&own), "{manifest}");
    assert!(manifest
        .contains("data_file 'SHOP_PED_APPAREL_META_FILE' 'data/mp_f_freemode_01_mp_f_demo.meta'"));

    // Importing the output gives the same ids, the same entries kept for want of a garment and the same carried file.
    let second = import(out.path(), &Options::default()).expect("the output imports");
    let (before, after) = (stated_ids(&first.wardrobe), stated_ids(&second.wardrobe));
    for (place, id) in &before {
        assert_eq!(after.get(place), Some(id), "{place:?} keeps its id");
    }
    // What the second import reads more is what the build named: the textures the shop file stated nothing for.
    let named: BTreeMap<_, _> = after
        .iter()
        .filter(|(place, _)| !before.contains_key(place))
        .collect();
    let by_the_build: BTreeMap<_, _> = shop::written(&first.wardrobe, &allocate(&first.wardrobe))
        .into_iter()
        .filter(|w| w.garment.is_some())
        .map(|w| {
            let garment = &first.wardrobe.garments[w.garment.unwrap()];
            let slot = garment.slot.unwrap();
            ((w.ped, slot, w.position, w.texture), w.id)
        })
        .filter(|(place, _)| !before.contains_key(place))
        .collect();
    assert_eq!(by_the_build.len(), 2, "{by_the_build:?}");
    assert_eq!(
        named,
        by_the_build.iter().collect::<BTreeMap<_, _>>(),
        "the second import states the names the build gave"
    );
    let kept = |w: &Wardrobe| {
        w.shop
            .unplaced
            .iter()
            .map(|s| {
                (
                    s.ped,
                    s.kind.clone(),
                    s.drawable,
                    s.texture,
                    s.entry.id.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(kept(&second.wardrobe), kept(&first.wardrobe));
    assert_eq!(second.wardrobe.shop.carried, first.wardrobe.shop.carried);
    let cost_of_the_jackets_second_texture = |w: &Wardrobe| {
        w.garments
            .iter()
            .find(|g| g.ped == Some(Ped::Male) && g.slot == Some(JBIB) && g.position == Some(0))
            .and_then(|g| g.variations[1].shop.as_ref())
            .and_then(|e| e.cost.clone())
    };
    assert_eq!(
        cost_of_the_jackets_second_texture(&second.wardrobe),
        Some("150".into())
    );
    // Nothing the shop stated was lost on the way through, and the second import lists nothing new.
    assert_eq!(
        second.report.shop.unplaced.len(),
        first.report.shop.unplaced.len()
    );
}

#[test]
fn the_durty_option_names_only_the_entries_that_state_no_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = imported(dir.path());
    first.wardrobe.shop.durty_hashes = true;
    let planned = plan(
        &first.wardrobe,
        &inspect(dir.path(), &first.wardrobe, Depth::Stat),
        "wardrobe",
        dir.path(),
    );
    let male = &planned.documents.shops[&Ped::Male];
    assert!(
        male.contains("<uniqueNameHash>65D90E59C0AE</uniqueNameHash>"),
        "{male}"
    );
    assert!(
        male.contains("<uniqueNameHash>CLO_DCT_LEGS_0_0_M</uniqueNameHash>"),
        "{male}"
    );
    assert!(!male.contains("MP_M_DEMO_LOWR"), "{male}");
}

#[test]
fn an_id_somebody_chose_wins_over_the_one_an_import_read_and_taking_it_back_restores_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = imported(dir.path());
    let jacket = first
        .wardrobe
        .garments
        .iter()
        .find(|g| g.slot == Some(JBIB) && g.ped == Some(Ped::Male) && g.position == Some(0))
        .unwrap()
        .id
        .clone();
    let text = |w: &Wardrobe| {
        plan(
            w,
            &inspect(dir.path(), w, Depth::Stat),
            "wardrobe",
            dir.path(),
        )
        .documents
        .shops[&Ped::Male]
            .clone()
    };
    let previous = edit::set_shop_id(
        &mut first.wardrobe,
        &jacket,
        0,
        Some("  MY_OWN_ID  ".into()),
    )
    .unwrap();
    assert_eq!(previous, None);
    let changed = text(&first.wardrobe);
    assert!(
        changed.contains("<uniqueNameHash>MY_OWN_ID</uniqueNameHash>"),
        "{changed}"
    );
    assert!(!changed.contains("65D90E59C0AE"), "{changed}");
    // What the import read is still there to go back to.
    let held = first.wardrobe.garment(&jacket).unwrap().variations[0]
        .shop
        .as_ref()
        .unwrap();
    assert_eq!(held.id, "65D90E59C0AE");
    let chosen = edit::set_shop_id(&mut first.wardrobe, &jacket, 0, None).unwrap();
    assert_eq!(chosen.as_deref(), Some("MY_OWN_ID"));
    assert!(text(&first.wardrobe).contains("<uniqueNameHash>65D90E59C0AE</uniqueNameHash>"));

    // An id is one short line, on a texture the garment has.
    for bad in ["two\nlines".to_string(), "x".repeat(edit::MAX_SHOP_ID + 1)] {
        assert!(edit::set_shop_id(&mut first.wardrobe, &jacket, 0, Some(bad)).is_err());
    }
    assert!(edit::set_shop_id(&mut first.wardrobe, &jacket, 9, Some("x".into())).is_err());
}

#[test]
fn two_entries_claiming_one_id_and_entries_pointing_at_nothing_are_findings_a_reason_can_set_aside()
{
    let dir = tempfile::tempdir().unwrap();
    let mut first = imported(dir.path());
    let planned = |w: &Wardrobe| {
        plan(
            w,
            &inspect(dir.path(), w, Depth::Stat),
            "wardrobe",
            dir.path(),
        )
    };
    let before = planned(&first.wardrobe);
    let clash = before
        .findings
        .iter()
        .find(|f| f.code == "shop-id-clash")
        .expect("the clash is a finding");
    assert!(
        clash.message.contains("DEMO_DUP") || clash.message.to_lowercase().contains("demo_dup"),
        "{}",
        clash.message
    );
    assert!(!clash.blocks(), "a shop id two entries claim is a warning");
    let unplaced = before
        .findings
        .iter()
        .find(|f| f.code == "shop-entry-unplaced")
        .expect("the unplaced entries are a finding");
    assert!(
        unplaced
            .message
            .starts_with("3 entries of data/mp_m_freemode_01_mp_m_demo.meta"),
        "{}",
        unplaced.message
    );

    edit::waive(
        &mut first.wardrobe,
        "shop-entry-unplaced",
        None,
        "the server keeps ids for outfits it has retired",
    )
    .unwrap();
    let after = planned(&first.wardrobe);
    let waived = after
        .findings
        .iter()
        .find(|f| f.code == "shop-entry-unplaced")
        .unwrap();
    assert!(waived.waived.is_some());
}

#[test]
fn the_verifier_holds_the_shop_file_to_the_ids_the_plan_wrote_and_not_to_a_count() {
    let dir = tempfile::tempdir().unwrap();
    let first = imported(dir.path());
    let source = project(dir.path(), &first.wardrobe, "demo_pack");
    let built = plans(&source, "fivem").unwrap().remove(0);
    let out = tempfile::tempdir().unwrap();
    write_plan(&built, out.path());
    let planned = plan(
        &first.wardrobe,
        &inspect(dir.path(), &first.wardrobe, Depth::Stat),
        "wardrobe",
        dir.path(),
    );
    let shop_check = |v: &or_wardrobe::verify::Verification| {
        v.checks
            .iter()
            .find(|c| c.name == format!("{} shop metadata", Ped::Male))
            .cloned()
            .expect("a check of the male shop metadata")
    };
    let clean = verify::verify(&first.wardrobe, &planned, out.path());
    assert!(shop_check(&clean).passed, "{}", shop_check(&clean).detail);

    // Same number of entries, one id changed: the file is not the plan's.
    let file = out.path().join(MALE_META);
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, text.replace("65D90E59C0AE", "65D90E59C0AF")).unwrap();
    let tampered = verify::verify(&first.wardrobe, &planned, out.path());
    let check = shop_check(&tampered);
    assert!(!check.passed);
    assert!(
        check
            .detail
            .contains("states the id 65D90E59C0AF and the plan 65D90E59C0AE"),
        "{}",
        check.detail
    );
}

/// A server resource whose female shop file and manifest are there and whose female garments are not.
#[test]
fn entries_kept_for_a_ped_that_has_no_garment_are_still_written_registered_and_verified() {
    let dir = tempfile::tempdir().unwrap();
    demo::resource(dir.path());
    std::fs::remove_dir_all(dir.path().join(format!("stream/demo/{F}"))).unwrap();
    let first = import(dir.path(), &Options::default()).expect("the partial resource imports");
    assert!(first
        .wardrobe
        .garments
        .iter()
        .all(|g| g.ped != Some(Ped::Female)));
    let kept = |w: &Wardrobe| {
        w.shop
            .unplaced
            .iter()
            .filter(|s| s.ped == Ped::Female)
            .map(|s| s.entry.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(kept(&first.wardrobe), ["demo_dup"]);

    // The plan writes the file of a ped whose only entries are the ones kept, and registers it.
    let planned = plan(
        &first.wardrobe,
        &inspect(dir.path(), &first.wardrobe, Depth::Stat),
        "wardrobe",
        dir.path(),
    );
    assert!(
        planned.documents.shops.contains_key(&Ped::Female),
        "no shop file is planned for the female ped"
    );
    assert!(
        !planned.documents.variations.contains_key(&Ped::Female),
        "a ped with no garment has no variation metadata"
    );
    let source = project(dir.path(), &first.wardrobe, "demo_pack");
    let built = plans(&source, "fivem").unwrap().remove(0);
    assert!(built.problems.is_empty(), "{:?}", built.problems);
    let out = tempfile::tempdir().unwrap();
    write_plan(&built, out.path());
    let female = std::fs::read_to_string(out.path().join(FEMALE_META))
        .expect("the female shop file is in the build");
    assert!(
        female.contains("<uniqueNameHash>demo_dup</uniqueNameHash>"),
        "{female}"
    );
    let manifest = std::fs::read_to_string(out.path().join("fxmanifest.lua")).unwrap();
    assert!(manifest.contains(&format!(
        "data_file 'SHOP_PED_APPAREL_META_FILE' '{FEMALE_META}'"
    )));

    // The verifier holds that file to the ids the plan wrote, as it holds the male one.
    let shop_check = |v: &verify::Verification| {
        v.checks
            .iter()
            .find(|c| c.name == format!("{} shop metadata", Ped::Female))
            .cloned()
            .expect("a check of the female shop metadata")
    };
    let clean = verify::verify(&first.wardrobe, &planned, out.path());
    assert!(shop_check(&clean).passed, "{}", shop_check(&clean).detail);
    let file = out.path().join(FEMALE_META);
    std::fs::write(&file, female.replace("demo_dup", "demo_dupe")).unwrap();
    let tampered = verify::verify(&first.wardrobe, &planned, out.path());
    assert!(!shop_check(&tampered).passed);

    // Importing the build keeps the same entry.
    std::fs::write(&file, &female).unwrap();
    let second = import(out.path(), &Options::default()).expect("the build imports");
    assert_eq!(kept(&second.wardrobe), ["demo_dup"]);
}

#[test]
fn shop_metadata_dropped_with_garments_resolves_by_the_numbers_their_names_state_and_undo_takes_it_back(
) {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("clothing");
    edit::create(&module, "demo").unwrap();
    let dropped = dir.path().join("res");
    demo::resource(&dropped);
    // A garment whose name states number 5 is not the first of its slot, so the allocator would put it at 0.
    let high = dir.path().join("high");
    std::fs::create_dir_all(&high).unwrap();
    std::fs::write(
        high.join(format!("{M}^jbib_005_u.ydd")),
        common::ydd("coat"),
    )
    .unwrap();
    std::fs::write(
        high.join(format!("{M}^jbib_diff_005_a_uni.ytd")),
        common::ytd("c"),
    )
    .unwrap();
    std::fs::write(
        high.join("shop.meta"),
        demo::shop_file(
            "mp_m_freemode_01",
            "mp_m_demo",
            &demo::component("0", "HIGH_COAT_5", 5, "PV_COMP_JBIB", 0, ""),
            "",
        ),
    )
    .unwrap();

    let added = add_clothes(&module, std::slice::from_ref(&high)).expect("the drop");
    assert_eq!(added.garments.len(), 1);
    assert_eq!(added.shop.resolved, 1, "{:?}", added.shop);
    let w = Wardrobe::read(&module).unwrap();
    assert_eq!(
        w.garments[0].variations[0].shop.as_ref().unwrap().id,
        "HIGH_COAT_5"
    );
    edit::undo(&module, &added).unwrap();
    assert!(Wardrobe::read(&module).unwrap().garments.is_empty());

    // The demo resource: five garments, the ids of five textures, three entries kept, and the server's own file copied in.
    let added = add_clothes(&module, std::slice::from_ref(&dropped)).expect("the drop");
    assert_eq!(added.garments.len(), 5, "{:?}", added.left_out);
    assert_eq!(added.shop.resolved, 5, "{:?}", added.shop);
    assert_eq!(added.shop.unplaced.len(), 3);
    assert_eq!(added.shop.carried.len(), 1);
    let w = Wardrobe::read(&module).unwrap();
    assert_eq!(
        stated_ids_by_label(&w, "mp_m_freemode_01_mp_m_demo^jbib_000_u", 1),
        "DEMO_M_JBIB_0_B"
    );
    assert_eq!(w.shop.unplaced.len(), 3);
    assert_eq!(w.shop.carried.len(), 1);
    let copy = module.join(&w.shop.carried[0].file);
    assert_eq!(std::fs::read(&copy).unwrap(), SPAK);
    assert!(added.copied.contains(&w.shop.carried[0].file));

    // Dropping it again replaces what the first drop kept and lists nothing twice.
    let again = add_clothes(&module, std::slice::from_ref(&dropped)).expect("the drop again");
    assert_eq!(Wardrobe::read(&module).unwrap().shop.unplaced.len(), 3);
    assert_eq!(Wardrobe::read(&module).unwrap().shop.carried.len(), 1);
    edit::undo(&module, &again).unwrap();
    assert_eq!(
        Wardrobe::read(&module).unwrap().shop.unplaced.len(),
        3,
        "the first drop's are back"
    );

    // Undo of the first drop takes the garments, the entries, the carried file and its folder.
    edit::undo(&module, &added).unwrap();
    let w = Wardrobe::read(&module).unwrap();
    assert!(w.garments.is_empty() && w.shop.unplaced.is_empty() && w.shop.carried.is_empty());
    assert!(!copy.exists() && !module.join("shop").exists());
    let _ = (FEET, F, MP);
}

#[test]
fn a_garment_a_drop_leaves_out_does_not_move_the_ids_of_the_garments_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("clothing");
    edit::create(&module, "demo").unwrap();
    let high = dir.path().join("high");
    std::fs::create_dir_all(&high).unwrap();
    // Number 0 is not a drawable, so it is left out, and the coat of number 5 is made after it.
    std::fs::write(high.join(format!("{M}^jbib_000_u.ydd")), b"not a drawable").unwrap();
    std::fs::write(
        high.join(format!("{M}^jbib_005_u.ydd")),
        common::ydd("coat"),
    )
    .unwrap();
    std::fs::write(
        high.join(format!("{M}^jbib_diff_005_a_uni.ytd")),
        common::ytd("c"),
    )
    .unwrap();
    let component = demo::component("0", "HIGH_COAT_5", 5, "PV_COMP_JBIB", 0, "");
    let shop = demo::shop_file("mp_m_freemode_01", "mp_m_demo", &component, "");
    std::fs::write(high.join("shop.meta"), shop).unwrap();

    let added = add_clothes(&module, std::slice::from_ref(&high)).expect("the drop");
    assert_eq!(added.garments.len(), 1, "{:?}", added.left_out);
    assert_eq!(added.left_out.len(), 1, "{:?}", added.left_out);
    assert_eq!(added.shop.resolved, 1, "{:?}", added.shop);
    let w = Wardrobe::read(&module).unwrap();
    assert_eq!(
        w.garment(&added.garments[0]).unwrap().variations[0]
            .shop
            .as_ref()
            .map(|e| e.id.as_str()),
        Some("HIGH_COAT_5")
    );
}

/// The id of texture `texture` of the garment labelled `label`.
fn stated_ids_by_label<'a>(w: &'a Wardrobe, label: &str, texture: usize) -> &'a str {
    w.garments
        .iter()
        .find(|g| g.label.as_deref() == Some(label))
        .and_then(|g| g.variations[texture].shop.as_ref())
        .map(|e| e.id.as_str())
        .unwrap_or_else(|| panic!("no id for {label} texture {texture}"))
}

#[test]
fn shop_metadata_dropped_alone_onto_a_pack_resolves_against_its_garments() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("clothing");
    edit::create(&module, "demo").unwrap();
    // The garments first, with no shop file among them.
    let garments = dir.path().join("garments");
    demo::files(&garments);
    let first = add_clothes(&module, &[garments]).expect("the garments");
    assert_eq!(first.garments.len(), 5);
    assert!(first.shop.is_empty(), "{:?}", first.shop);
    assert!(first.shop_before.is_none());

    // Then the shop's files alone: the manifest and the metadata it registers.
    let shop = dir.path().join("shop");
    std::fs::create_dir_all(shop.join("data")).unwrap();
    std::fs::write(shop.join("fxmanifest.lua"), demo::MANIFEST).unwrap();
    std::fs::write(shop.join(MALE_META), demo::male_shop()).unwrap();
    std::fs::write(
        shop.join("data/mp_f_freemode_01_mp_f_demo.meta"),
        demo::female_shop(),
    )
    .unwrap();
    std::fs::write(shop.join("data/mp_m_freemode_01_mp_m_demo.spak"), SPAK).unwrap();
    let added = add_clothes(&module, &[shop]).expect("the shop files");
    assert!(added.garments.is_empty());
    assert_eq!(added.shop.resolved, 5, "{:?}", added.shop);
    let w = Wardrobe::read(&module).unwrap();
    assert_eq!(
        stated_ids(&w).len(),
        0,
        "dropped garments are not pinned, so the pack's keys are by label"
    );
    let ids = w
        .garments
        .iter()
        .flat_map(|g| g.variations.iter().filter_map(|v| v.shop.as_ref()))
        .map(|e| e.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 5);
    assert!(ids.contains(&LONG_ID.to_string()));

    edit::undo(&module, &added).unwrap();
    let w = Wardrobe::read(&module).unwrap();
    assert!(w
        .garments
        .iter()
        .all(|g| g.variations.iter().all(|v| v.shop.is_none())));
    assert!(w.shop.unplaced.is_empty() && w.shop.carried.is_empty());
    assert_eq!(w.garments.len(), 5, "the garments of the earlier drop stay");
}
