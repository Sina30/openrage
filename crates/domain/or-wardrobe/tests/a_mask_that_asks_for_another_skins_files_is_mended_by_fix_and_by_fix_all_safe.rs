//! A pack built before the mask followed the skin wrote propMask 1 for a race garment, and the pack reopened from it states that
//! mask. The skin bit is the one thing in it that disagrees with the garment, so Fix and Fix all safe state it again and leave
//! every other bit; a mask that asks for the match form is a choice, and stands with the reason.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, put_bytes, snapshot, FEET, JBIB, LOWR};
use or_source::sample;
use or_wardrobe::edit::{remedy, undo_revision, Watch};
use or_wardrobe::fix::{all_safe, Step};
use or_wardrobe::mask::Property;
use or_wardrobe::{allocate, check, inspect, Depth, Finding, Ped, Skin, Slot, Wardrobe};

/// A race top that states 1 and 2, a universal pair of legs that states the skin bit, and shoes that state the match bit.
fn a_pack(dir: &Path) -> (PathBuf, [String; 3]) {
    let pack = dir.join("clothing");
    let mut w = Wardrobe::new("demo");
    for (slot, name, skin, mask) in [
        (JBIB, "jbib_000_r", Skin::Race, 1 | 2),
        (LOWR, "lowr_000_u", Skin::Universal, 1 | 16),
        (FEET, "feet_000_u", Skin::Universal, 1 | 32),
    ] {
        let model = format!("models/{name}.ydd");
        put_bytes(&pack, &model, &sample::drawable_dictionary(name, 6, 0.0));
        let mut g = garment(Ped::Male, slot, None, &model, &[]);
        g.skin = skin;
        g.meta.prop_mask = Some(mask);
        w.garments.push(g);
    }
    w.write(&pack).unwrap();
    let ids = [0, 1, 2].map(|i| w.garments[i].id.clone());
    (pack, ids)
}

fn mask_findings(pack: &Path) -> Vec<Finding> {
    let w = Wardrobe::read(pack).unwrap();
    check(&w, &allocate(&w), &inspect(pack, &w, Depth::Measure))
        .into_iter()
        .filter(|f| f.code == "mask-skin")
        .collect()
}

fn mask_of(pack: &Path, id: &str) -> Option<u8> {
    Wardrobe::read(pack)
        .unwrap()
        .garment(id)
        .unwrap()
        .meta
        .prop_mask
}

#[test]
fn fix_states_the_skin_bit_again_and_leaves_the_other_bits_and_a_match_mask_has_no_fix() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, [top, legs, shoes]) = a_pack(dir.path());
    let mut w = Wardrobe::read(&pack).unwrap();
    let found = mask_findings(&pack);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|f| f.blocks() && f.waived.is_none()));

    let of = |id: &str| {
        found
            .iter()
            .find(|f| f.garment.as_deref() == Some(id))
            .unwrap()
    };
    let fix = remedy(&w, of(&top)).expect("a race garment that states 1 and 2 has a fix");
    assert!(
        fix.what.contains("skin bit") && fix.what.contains("r form"),
        "{}",
        fix.what
    );
    fix.apply(&mut w).unwrap();
    assert_eq!(w.garment(&top).unwrap().meta.prop_mask, Some(1 | 2 | 16));

    let fix = remedy(&w, of(&legs)).expect("a universal garment that states the race bit has one");
    fix.apply(&mut w).unwrap();
    assert_eq!(
        w.garment(&legs).unwrap().meta.prop_mask,
        None,
        "the mask the skin implies is not stated"
    );

    assert!(
        remedy(&w, of(&shoes)).is_none(),
        "taking the match property off is the person's to decide"
    );
    assert_eq!(w.garment(&shoes).unwrap().meta.prop_mask, Some(1 | 32));
}

#[test]
fn fix_all_safe_mends_the_skin_bit_says_what_stands_and_a_second_run_changes_no_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, [top, legs, shoes]) = a_pack(dir.path());

    let (report, revision) = all_safe(&pack, &Watch::default()).expect("the pipeline runs");

    let mask = report
        .steps
        .iter()
        .find(|s| s.step == Step::Mask)
        .expect("a report for the mask");
    assert_eq!(mask.changed.len(), 2, "{mask:?}");
    assert_eq!(mask_of(&pack, &top), Some(1 | 2 | 16));
    assert_eq!(mask_of(&pack, &legs), None);
    assert_eq!(mask_of(&pack, &shoes), Some(1 | 32));
    let left = mask_findings(&pack);
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!(left[0].garment.as_deref(), Some(shoes.as_str()));
    let stands = (report.left.iter())
        .find(|s| s.code == "mask-skin")
        .expect("what stands is listed");
    assert!(
        stands.why.contains("match") && !stands.why.contains("needs a choice or a file"),
        "{stands:?}"
    );

    let settled = snapshot(&pack);
    let (again, none) = all_safe(&pack, &Watch::default()).unwrap();
    assert!(none.is_none() && !again.changed(), "{again:?}");
    assert_eq!(snapshot(&pack), settled, "a second run changes no byte");

    let revision = revision.expect("one step for the pipeline");
    undo_revision(&pack, &revision).expect("Undo takes the pipeline back");
    assert_eq!(mask_of(&pack, &top), Some(1 | 2));
    assert_eq!(mask_of(&pack, &legs), Some(1 | 16));
}

#[test]
fn the_panel_offers_the_match_property_only_where_the_mask_states_it_so_that_it_can_be_taken_off() {
    let mut g = garment(Ped::Male, JBIB, None, "a.ydd", &[]);
    assert!(g.slot.is_some_and(|s| matches!(s, Slot::Component(_))));
    let offered = |g: &or_wardrobe::Garment| or_wardrobe::mask::offered(g);
    assert!(!offered(&g).contains(&Property::MatchPrev));
    assert_eq!(offered(&g).len(), Property::ALL.len() - 1);
    g.meta.prop_mask = Some(1 | 32);
    assert!(offered(&g).contains(&Property::MatchPrev));
    assert_eq!(offered(&g).len(), Property::ALL.len());
}

/// The resource a build lays out for a race top, with the mask the planner wrote before the mask followed the skin: 1.
#[test]
fn a_pack_an_older_build_wrote_reopens_with_its_mask_and_fix_states_the_one_the_skin_implies() {
    use or_project::Origin;
    use or_wardrobe::{import, layout, plan, Facts};

    let dir = tempfile::tempdir().unwrap();
    for name in ["top.ydd", "top.ytd"] {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    let mut w = Wardrobe::new("pack");
    let mut top = garment(Ped::Male, JBIB, Some(0), "top.ydd", &["top.ytd"]);
    top.skin = Skin::Race;
    w.garments.push(top);
    let planned = plan(
        &w,
        &inspect(dir.path(), &w, Depth::Digest),
        "wardrobe",
        dir.path(),
    );
    let resource = dir.path().join("resource");
    for file in &planned.contribution.files {
        let bytes = match &file.origin {
            Origin::Generated(bytes) => bytes.clone(),
            Origin::Document(text) => text.clone().into_bytes(),
            _ => continue,
        };
        let at = resource.join(&file.path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, bytes).unwrap();
    }
    let placed = planned.allocation.placed[0];
    let collection = w.collection(placed.ped);
    let folder = resource.join(
        planned
            .build
            .paths
            .directory(placed.ped, &collection, placed.slot),
    );
    std::fs::create_dir_all(&folder).unwrap();
    let paths = &planned.build.paths;
    let model = paths.model_name(
        placed.ped,
        &collection,
        placed.slot,
        placed.position,
        Skin::Race,
    );
    let texture = paths.texture_name(
        placed.ped,
        &collection,
        placed.slot,
        placed.position,
        0,
        Skin::Race.tint(),
    );
    std::fs::write(folder.join(model), b"model").unwrap();
    std::fs::write(folder.join(texture), b"texture").unwrap();

    // What the planner wrote: the race top asks for the universal form.
    let mut info = planned.documents.variations[&Ped::Male].clone();
    let component = usize::from(info.avail_comp[usize::from(or_wardrobe::Component::Jbib.id())]);
    info.a_component_data3[component].a_drawbl_data3[0].prop_mask = 1;
    let path = resource.join(layout::variation_path("pack", Ped::Male, "mp_m_pack"));
    std::fs::write(&path, or_io::decl::File::to_bytes(&info).unwrap()).unwrap();

    let imported = import::import(&resource, &import::Options::default()).expect("it reopens");
    let mut reopened = imported.wardrobe;
    let top = &reopened.garments[0];
    assert_eq!((top.skin, top.meta.prop_mask), (Skin::Race, Some(1)));
    let found: Vec<Finding> = check(&reopened, &allocate(&reopened), &Facts::default())
        .into_iter()
        .filter(|f| f.code == "mask-skin")
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].blocks(), "{found:?}");
    let fix = remedy(&reopened, &found[0]).expect("a reopened race top has a fix");
    fix.apply(&mut reopened).unwrap();
    assert_eq!(reopened.garments[0].meta.prop_mask, None);
    let after = check(&reopened, &allocate(&reopened), &Facts::default());
    assert!(after.iter().all(|f| f.code != "mask-skin"), "{after:?}");
}
