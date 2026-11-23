//! A hoodie and a hat that make the game's hair and the pack's own hair wear an alternate model, and a mask that
//! wears a first person alternate: the plan writes both files and registers them, the read-back names a file that is
//! not the plan's, and an import makes the same sets and numbers again.
//!
//! The files are the plan's own, laid out as a build lays them. No staged file is needed.

mod common;

use std::path::Path;

use common::{garment, JBIB, P_HEAD};
use or_io::decl::File;
use or_project::Origin;
use or_wardrobe::alternate::{self, Asset};
use or_wardrobe::firstperson;
use or_wardrobe::options::Options;
use or_wardrobe::verify::verify;
use or_wardrobe::{
    import, inspect, plan, plan_for, Build, Component, Depth, Ped, Planned, Slot, Wardrobe,
};

const HAIR: Slot = Slot::Component(Component::Hair);
const BERD: Slot = Slot::Component(Component::Berd);

fn pack(dir: &Path) -> Wardrobe {
    for name in [
        "hoodie.ydd",
        "hoodie.ytd",
        "hat.ydd",
        "hat.ytd",
        "hair.ydd",
        "hair.ytd",
        "hair_1.ydd",
        "hair_2.ydd",
        "mask.ydd",
        "mask.ytd",
        "mask_1.ydd",
    ] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let mut w = Wardrobe::new("pack");
    let hoodie = garment(Ped::Male, JBIB, Some(3), "hoodie.ydd", &["hoodie.ytd"]);
    let hat = garment(Ped::Male, P_HEAD, Some(1), "hat.ydd", &["hat.ytd"]);
    let mut hair = garment(Ped::Male, HAIR, Some(0), "hair.ydd", &["hair.ytd"]);
    hair.alternates = vec!["hair_1.ydd".into(), "hair_2.ydd".into()];
    let mut mask = garment(Ped::Male, BERD, Some(0), "mask.ydd", &["mask.ytd"]);
    mask.alternates = vec!["mask_1.ydd".into()];
    firstperson::set(&mut mask, Some(1)).unwrap();
    let ids = (hoodie.id.clone(), hat.id.clone(), hair.id.clone());
    w.garments.extend([hoodie, hat, hair, mask]);

    // The hoodie and the hat squash the game's hair (alternate 3 of one, 1 of another) and the pack's own (alternate 2).
    let set = alternate::new_set(&mut w, Ped::Male, "hair under a hood").unwrap();
    alternate::add_switch(
        &mut w,
        &set,
        Asset::outside(Some("mp_m_gunrunning_hair_01"), HAIR, 4),
        3,
    )
    .unwrap();
    alternate::add_switch(&mut w, &set, Asset::outside(None, HAIR, 7), 1).unwrap();
    alternate::add_switch(&mut w, &set, Asset::garment(&ids.2), 2).unwrap();
    alternate::join(&mut w, &set, Asset::garment(&ids.0)).unwrap();
    alternate::join(&mut w, &set, Asset::garment(&ids.1)).unwrap();
    w
}

fn laid_out(planned: &Planned, resource: &Path) {
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
}

#[test]
fn the_plan_writes_and_registers_the_switches_the_sets_state_and_the_first_person_alternate() {
    let dir = tempfile::tempdir().unwrap();
    let p = pack(dir.path());
    let facts = inspect(dir.path(), &p, Depth::Digest);
    let planned = plan(&p, &facts, "wardrobe", dir.path());
    assert!(
        planned.contribution.problems.is_empty(),
        "{:?}",
        planned.contribution.problems
    );
    let alternates = planned
        .documents
        .alternates
        .as_ref()
        .expect("the sets write a file");
    let switches = &alternates.peds[0].switches;
    assert_eq!(switches.len(), 3);
    assert_eq!(switches[0].source_assets.len(), 2);
    assert!(planned.contribution.manifest.iter().any(|m| m.values
        == [
            "ALTERNATE_VARIATIONS_FILE".to_string(),
            "data/pedalternatevariations.meta".to_string()
        ]));
    let first_person = planned
        .documents
        .first_person
        .as_ref()
        .expect("the mask states one");
    assert_eq!(first_person.alternates.len(), 1);
    assert!(planned
        .contribution
        .manifest
        .iter()
        .any(|m| m.values.first().map(String::as_str) == Some("PED_FIRST_PERSON_ALTERNATE_DATA")));
    // The pack's own hair states the two alternate models it holds, which the switch to alternate 2 wears.
    let info = &planned.documents.variations[&Ped::Male];
    let hair =
        &info.a_component_data3[usize::from(info.avail_comp[usize::from(Component::Hair.id())])];
    assert_eq!(hair.a_drawbl_data3[0].num_alternatives, 2);
}

#[test]
fn a_build_that_leaves_alternate_models_out_writes_no_switch_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let p = pack(dir.path());
    let facts = inspect(dir.path(), &p, Depth::Digest);
    let mut build = Build::standard(&p);
    build.options = Options {
        alternates: false,
        ..Options::default()
    };
    let planned = plan_for(&p, &facts, "wardrobe", dir.path(), &build);
    assert!(planned.documents.alternates.is_none() && planned.documents.first_person.is_none());
    let said = planned.contribution.notes.join(" ");
    assert!(
        said.contains("alternate variations") && said.contains("first person"),
        "{said}"
    );
}

#[test]
fn the_read_back_names_a_file_that_is_not_the_plans() {
    let dir = tempfile::tempdir().unwrap();
    let p = pack(dir.path());
    let facts = inspect(dir.path(), &p, Depth::Digest);
    let planned = plan(&p, &facts, "wardrobe", dir.path());
    let resource = dir.path().join("resource");
    laid_out(&planned, &resource);
    let clean = verify(&p, &planned, &resource);
    assert!(clean.is_clean(), "{:?}", clean.problems());
    for name in ["alternate variations", "first person alternates"] {
        assert!(
            clean.checks.iter().any(|c| c.name == name && c.passed),
            "{name}"
        );
    }
    // A switch is missing from the written file.
    let path = resource.join("data/pedalternatevariations.meta");
    let mut changed = planned.documents.alternates.clone().unwrap();
    changed.peds[0].switches.pop();
    std::fs::write(&path, changed.to_bytes().unwrap()).unwrap();
    let problems = verify(&p, &planned, &resource).problems();
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("alternate variations")
                && p.contains("not the metadata the plan made")),
        "{problems:?}"
    );
    // The first person file is gone.
    std::fs::write(
        &path,
        planned
            .documents
            .alternates
            .clone()
            .unwrap()
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    std::fs::remove_file(resource.join("data/pedfirstpersonalternates.meta")).unwrap();
    let problems = verify(&p, &planned, &resource).problems();
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("first person alternates")),
        "{problems:?}"
    );
}

#[test]
fn an_import_makes_the_sets_and_the_number_again_and_a_garment_taken_out_leaves_them() {
    let dir = tempfile::tempdir().unwrap();
    let p = pack(dir.path());
    let facts = inspect(dir.path(), &p, Depth::Digest);
    let planned = plan(&p, &facts, "wardrobe", dir.path());
    let resource = dir.path().join("resource");
    laid_out(&planned, &resource);
    for placed in &planned.allocation.placed {
        let garment = &p.garments[placed.garment];
        let collection = p.collection(placed.ped);
        let folder = resource.join(planned.build.paths.directory(
            placed.ped,
            &collection,
            placed.slot,
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let model = planned.build.paths.model_name(
            placed.ped,
            &collection,
            placed.slot,
            placed.position,
            garment.skin,
        );
        std::fs::write(folder.join(&model), b"model").unwrap();
        for (i, _) in garment.alternates.iter().enumerate() {
            std::fs::write(
                folder.join(or_wardrobe::layout::alternate_name(&model, i + 1)),
                b"alternate",
            )
            .unwrap();
        }
        let texture = planned.build.paths.texture_name(
            placed.ped,
            &collection,
            placed.slot,
            placed.position,
            0,
            garment.skin.tint(),
        );
        std::fs::write(folder.join(texture), b"texture").unwrap();
    }
    let imported =
        import::import(&resource, &import::Options::default()).expect("the resource imports");
    assert_eq!(imported.report.switches.sets, 1);
    assert_eq!(imported.report.switches.switches, 3);
    assert_eq!(imported.report.first_person.matched, 1);
    let set = &imported.wardrobe.alternates[0];
    // The hoodie, the hat and the hair are garments of the pack again, found by the places they hold.
    let id_at = |slot: Slot, position: u32| {
        imported
            .wardrobe
            .garments
            .iter()
            .find(|g| g.slot == Some(slot) && g.position == Some(position))
            .map(|g| g.id.clone())
            .unwrap()
    };
    assert_eq!(
        set.sources,
        [
            Asset::garment(&id_at(JBIB, 3)),
            Asset::garment(&id_at(P_HEAD, 1))
        ]
    );
    assert_eq!(set.switches[2].target, Asset::garment(&id_at(HAIR, 0)));
    assert_eq!(
        set.switches[0].target,
        Asset::outside(Some("mp_m_gunrunning_hair_01"), HAIR, 4)
    );
    assert_eq!(set.switches[1].target, Asset::outside(None, HAIR, 7));
    let mask = imported
        .wardrobe
        .garments
        .iter()
        .find(|g| g.slot == Some(BERD))
        .unwrap();
    assert_eq!(mask.meta.first_person_alternate, Some(1));

    // The imported pack plans the same two files.
    let facts = inspect(&resource, &imported.wardrobe, Depth::Digest);
    let again = plan(&imported.wardrobe, &facts, "wardrobe", &resource);
    assert!(again.documents.alternates == planned.documents.alternates);
    assert!(again.documents.first_person == planned.documents.first_person);

    // The removal an app makes keeps what the garment was in the sets, so that putting it back (Undo) restores them where they were.
    let mut kept = imported.wardrobe.clone();
    let removed = or_wardrobe::edit::take_out(&mut kept, &id_at(JBIB, 3)).unwrap();
    assert_eq!(
        kept.alternates[0].sources,
        [Asset::garment(&id_at(P_HEAD, 1))]
    );
    or_wardrobe::edit::put_back(&mut kept, &removed).unwrap();
    assert!(kept == imported.wardrobe, "the sets are as they were");

    // Taking the hoodie out takes it out of the set; the hat still causes the switches.
    let mut w = imported.wardrobe.clone();
    or_wardrobe::edit::apply(&mut w, &id_at(JBIB, 3), or_wardrobe::edit::Edit::Remove).unwrap();
    assert_eq!(w.alternates[0].sources, [Asset::garment(&id_at(P_HEAD, 1))]);
}
