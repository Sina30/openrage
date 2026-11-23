//! What a garment does is written in the metadata under the engine's names, read back from the written files, and
//! imported again as it was stated: the flags, the mask a skin implies, the audio, the stickiness and what a hat and
//! a hair do to the hair.
//!
//! The files are the plan's own, laid out as a build lays them. No staged file is needed.

mod common;

use std::path::Path;

use common::{garment, JBIB, LOWR, P_HEAD};
use or_io::decl::File;
use or_io::meta::CPedVariationInfo;
use or_project::Origin;
use or_wardrobe::audio::{self, Which};
use or_wardrobe::behaviour::{apply, Behaviour};
use or_wardrobe::flags::{self, Flag};
use or_wardrobe::mask::{Property, RenderFlag};
use or_wardrobe::verify::verify;
use or_wardrobe::{
    hair, import, inspect, layout, plan, Component, Depth, Ped, Planned, Skin, Slot, Wardrobe,
};

const HAIR: Slot = Slot::Component(Component::Hair);

fn pack(dir: &Path) -> Wardrobe {
    for name in [
        "top.ydd",
        "top.ytd",
        "hat.ydd",
        "hat.ytd",
        "hair.ydd",
        "hair.ytd",
        "pants.ydd",
        "pants.ytd",
    ] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let mut w = Wardrobe::new("pack");
    let mut top = garment(Ped::Male, JBIB, Some(0), "top.ydd", &["top.ytd"]);
    top.skin = Skin::Race;
    apply(&mut top, Behaviour::Flag(Flag::NotInCar, true)).unwrap();
    apply(&mut top, Behaviour::Flag(Flag::Armoured, true)).unwrap();
    apply(&mut top, Behaviour::Property(Property::Alpha, true)).unwrap();
    apply(
        &mut top,
        Behaviour::Audio(Which::First, Some("hash_49BC7916".into())),
    )
    .unwrap();
    let mut hat = garment(Ped::Male, P_HEAD, Some(0), "hat.ydd", &["hat.ytd"]);
    apply(&mut hat, Behaviour::Flag(Flag::DefaultHelmet, true)).unwrap();
    apply(&mut hat, Behaviour::Flag(Flag::UsePhysicsHat2, true)).unwrap();
    apply(&mut hat, Behaviour::Render(RenderFlag::Cutout, true)).unwrap();
    apply(&mut hat, Behaviour::Stickyness(Some(100))).unwrap();
    apply(&mut hat, Behaviour::HairScale(Some(-0.5))).unwrap();
    let mut hair = garment(Ped::Male, HAIR, Some(0), "hair.ydd", &["hair.ytd"]);
    apply(&mut hair, Behaviour::HairHeight(Some(0.19))).unwrap();
    apply(&mut hair, Behaviour::Property(Property::Cutout, true)).unwrap();
    // Legs of the race skin that state nothing: the mask follows the skin.
    let mut pants = garment(Ped::Male, LOWR, Some(0), "pants.ydd", &["pants.ytd"]);
    pants.skin = Skin::Race;
    w.garments.extend([top, hat, hair, pants]);
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
fn the_metadata_states_what_each_garment_does_under_the_engines_names() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let facts = inspect(dir.path(), &w, Depth::Digest);
    let planned = plan(&w, &facts, "wardrobe", dir.path());
    assert!(
        planned.contribution.problems.is_empty(),
        "{:?}",
        planned.contribution.problems
    );
    let info = &planned.documents.variations[&Ped::Male];

    // The top: a drawable of the race skin, its flags and its audio, and a mask that is the race mask with the alpha bit.
    let jbib =
        &info.a_component_data3[usize::from(info.avail_comp[usize::from(Component::Jbib.id())])];
    assert_eq!(
        jbib.a_drawbl_data3[0].prop_mask,
        Property::Active.mask() | Property::RaceTex.mask() | Property::Alpha.mask()
    );
    let top = info
        .comp_infos
        .iter()
        .find(|i| i.ped_xml_comp_idx == Component::Jbib.id())
        .unwrap();
    assert_eq!(top.flags, Flag::NotInCar.mask() | Flag::Armoured.mask());
    assert_eq!(top.ped_xml_audio_id.hash, 0x49BC_7916);

    let legs =
        &info.a_component_data3[usize::from(info.avail_comp[usize::from(Component::Lowr.id())])];
    assert_eq!(
        legs.a_drawbl_data3[0].prop_mask,
        Property::Active.mask() | Property::RaceTex.mask(),
        "a drawable of the race skin that states no mask asks for the race form of its files"
    );

    // The hat: the helmet and the second hat physics are bits of `propFlags` (bit 17 does not fit `flags`), its render and its stickiness.
    let hat = &info.prop_info.a_prop_meta_data[0];
    assert_eq!(
        hat.prop_flags,
        Flag::DefaultHelmet.mask() | Flag::UsePhysicsHat2.mask()
    );
    assert_eq!(hat.render_flags, RenderFlag::Cutout.mask());
    assert_eq!(hat.stickyness, 100);
    assert_eq!(hat.expression_mods[0], -0.5);

    // A hair with no race skin: the plain mask with the cutout bit, and its height as the third modifier.
    let hair_info = info
        .comp_infos
        .iter()
        .find(|i| i.ped_xml_comp_idx == Component::Hair.id())
        .unwrap();
    assert_eq!(hair_info.ped_xml_expression_mods[2], 0.19);

    // The creature metadata drives the two bones with the expressions that carry them.
    let creature = planned
        .documents
        .creature
        .as_ref()
        .expect("two garments state expressions");
    assert!(or_wardrobe::behaviour::drives(
        creature,
        P_HEAD,
        0,
        hair::SCALE_EXPRESSION,
        hair::SCALE_BONE
    ));
    assert!(or_wardrobe::behaviour::drives(
        creature,
        HAIR,
        0,
        hair::HEIGHT_EXPRESSION,
        hair::HEIGHT_BONE
    ));
}

#[test]
fn the_read_back_names_a_garment_whose_written_behaviour_is_not_the_packs() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let facts = inspect(dir.path(), &w, Depth::Digest);
    let planned = plan(&w, &facts, "wardrobe", dir.path());
    let resource = dir.path().join("resource");
    laid_out(&planned, &resource);
    let clean = verify(&w, &planned, &resource);
    assert!(clean.is_clean(), "{:?}", clean.problems());
    assert!(
        clean.checks.iter().any(|c| c.name == "garment behaviour"),
        "the check is part of the read-back"
    );

    let path = resource.join(layout::variation_path("pack", Ped::Male, "mp_m_pack"));
    let write = |change: &dyn Fn(&mut CPedVariationInfo)| {
        let mut info = planned.documents.variations[&Ped::Male].clone();
        change(&mut info);
        std::fs::write(&path, info.to_bytes().unwrap()).unwrap();
        verify(&w, &planned, &resource).problems()
    };
    let has = |problems: &[String], text: &str| {
        problems
            .iter()
            .any(|p| p.starts_with("garment behaviour") && p.contains(text))
    };

    // The hat loses the physics bit in the written file.
    let problems =
        write(&|info| info.prop_info.a_prop_meta_data[0].prop_flags = Flag::DefaultHelmet.mask());
    assert!(has(&problems, "flags"), "{problems:?}");
    // The top asks for the universal form of its files.
    let problems = write(&|info| {
        let c = usize::from(info.avail_comp[usize::from(Component::Jbib.id())]);
        info.a_component_data3[c].a_drawbl_data3[0].prop_mask = 1;
    });
    assert!(has(&problems, "propMask"), "{problems:?}");
    // The hat is no longer squashed in the file.
    let problems = write(&|info| info.prop_info.a_prop_meta_data[0].expression_mods[0] = 0.0);
    assert!(has(&problems, "expression modifiers"), "{problems:?}");
    // The creature metadata stops driving the hair.
    let creature_path = resource.join(planned.build.paths.creature_path());
    let mut creature = planned.documents.creature.clone().unwrap();
    creature.ped_prop_expressions.clear();
    std::fs::write(&creature_path, creature.to_bytes().unwrap()).unwrap();
    std::fs::write(
        &path,
        planned.documents.variations[&Ped::Male].to_bytes().unwrap(),
    )
    .unwrap();
    let problems = verify(&w, &planned, &resource).problems();
    assert!(has(&problems, "does not drive bone 13201"), "{problems:?}");
}

#[test]
fn an_import_gives_back_the_behaviour_the_build_stated() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let facts = inspect(dir.path(), &w, Depth::Digest);
    let planned = plan(&w, &facts, "wardrobe", dir.path());
    // The resource as a build lays it, with the files the garments name where the layout puts them.
    let resource = dir.path().join("resource");
    laid_out(&planned, &resource);
    for placed in &planned.allocation.placed {
        let garment = &w.garments[placed.garment];
        let collection = w.collection(placed.ped);
        let dir = resource.join(planned.build.paths.directory(
            placed.ped,
            &collection,
            placed.slot,
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let model = planned.build.paths.model_name(
            placed.ped,
            &collection,
            placed.slot,
            placed.position,
            garment.skin,
        );
        std::fs::write(dir.join(&model), b"model").unwrap();
        let texture = planned.build.paths.texture_name(
            placed.ped,
            &collection,
            placed.slot,
            placed.position,
            0,
            garment.skin.tint(),
        );
        std::fs::write(dir.join(texture), b"texture").unwrap();
    }
    let imported =
        import::import(&resource, &import::Options::default()).expect("the resource imports");
    let read = |slot: Slot| {
        imported
            .wardrobe
            .garments
            .iter()
            .find(|g| g.slot == Some(slot))
            .unwrap_or_else(|| panic!("no garment of {slot:?} was imported"))
    };
    let (top, hat, hair_g) = (read(JBIB), read(P_HEAD), read(HAIR));
    assert_eq!(
        flags::word(top),
        Flag::NotInCar.mask() | Flag::Armoured.mask()
    );
    assert!(
        or_wardrobe::mask::has_property(top, Property::Alpha)
            && or_wardrobe::mask::has_property(top, Property::RaceTex)
    );
    assert_eq!(top.skin, Skin::Race);
    assert_eq!(audio::get(top, Which::First), "hash_49BC7916");
    assert_eq!(
        flags::word(hat),
        Flag::DefaultHelmet.mask() | Flag::UsePhysicsHat2.mask()
    );
    assert_eq!(hair::scale_of(hat), Some(-0.5));
    assert_eq!(hat.meta.stickyness, Some(100));
    assert_eq!(hair::height_of(hair_g), Some(0.19));
    // The creature metadata, read back, gave each garment the expression it states.
    assert!(hat
        .expressions
        .iter()
        .any(|e| e.ids == vec![hair::SCALE_BONE]));
}

#[test]
fn a_prop_at_an_anchor_no_staged_document_states_is_left_out_by_name_and_not_dropped_silently() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let facts = inspect(dir.path(), &w, Depth::Digest);
    let planned = plan(&w, &facts, "wardrobe", dir.path());
    let resource = dir.path().join("resource");
    laid_out(&planned, &resource);
    // The hat is stated at the anchor of the left placeholder hand, the eleventh the engine's table names.
    let mut info = planned.documents.variations[&Ped::Male].clone();
    info.prop_info.a_prop_meta_data[0].anchor_id = 11;
    info.prop_info.a_anchors[0].anchor = or_io::meta::Anchor(11);
    let path = resource.join(layout::variation_path("pack", Ped::Male, "mp_m_pack"));
    std::fs::write(&path, info.to_bytes().unwrap()).unwrap();
    // One component model gives the import a garment to place.
    let folder = resource.join(planned.build.paths.directory(Ped::Male, "mp_m_pack", JBIB));
    std::fs::create_dir_all(&folder).unwrap();
    let model = planned
        .build
        .paths
        .model_name(Ped::Male, "mp_m_pack", JBIB, 0, Skin::Race);
    std::fs::write(folder.join(model), b"model").unwrap();
    let imported = import::import(&resource, &import::Options::default()).expect("it imports");
    let said = imported
        .report
        .left_out
        .iter()
        .find(|l| l.why.contains("anchor 11"))
        .expect("the prop is named in what was left out");
    assert!(said.why.contains("ANCHOR_PH_L_HAND"), "{}", said.why);
    assert!(imported
        .wardrobe
        .garments
        .iter()
        .all(|g| !matches!(g.slot, Some(Slot::Prop(_)))));
}
