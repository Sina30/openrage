//! The wind scale of a garment's cloth is an edit of the pack with Undo, the build states it in every cloth of the
//! linked `.yld` and ships that file in place of the one linked, the linked file is never touched, and the verifier
//! reads the shipped cloth back and says when it does not hold the scale.
//!
//! The cloth here is authored with `or-io` and needs no game file.

mod common;

use std::path::Path;

use common::{garment, ydd, ytd, JBIB};
use or_io::decl::File;
use or_io::gta5::cloth_dict::*;
use or_io::gta5::shared::{PgDictionary, Vec3V};
use or_project::Origin;
use or_wardrobe::edit::{self, add_clothes, Edit};
use or_wardrobe::{inspect, plan, verify, wind, Depth, Ped, Wardrobe};

const M: &str = "mp_m_freemode_01_mp_m_x";

/// A cloth dictionary of one cloth whose controller states `scale` as its wind.
fn a_yld(scale: f32) -> Vec<u8> {
    let level = PhVerletCloth {
        cloth_data: PhClothData {
            vertex_positions: vec![Vec3V::default(); 3],
            ..Default::default()
        },
        edge_data: vec![PhEdgeData {
            vert_indices: [0, 1],
            edge_length2: 1.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut controller = CharacterClothController {
        bone_index_map: vec![7, 8],
        bone_id_map: vec![0x1234, 0x5678],
        wind_scale: scale,
        ..Default::default()
    };
    controller.base.name[..5].copy_from_slice(b"skirt");
    controller.base.cloth_0 = Some(Box::new(level));
    let cloth = CharacterCloth {
        cloth_controller: Some(Box::new(controller)),
        ..Default::default()
    };
    PgDictionary {
        codes: vec![0x1234_5678],
        entries: vec![Some(Box::new(cloth))],
        ..Default::default()
    }
    .to_bytes()
    .expect("a cloth dictionary saves")
}

fn put(dir: &Path, rel: &str, bytes: &[u8]) -> std::path::PathBuf {
    let to = dir.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(&to, bytes).unwrap();
    to
}

/// A pack of one jacket in `dir/clothing` with the cloth `a_yld(1.0)` linked, and its id.
fn a_jacket_with_cloth(dir: &Path) -> (std::path::PathBuf, String) {
    let module = dir.join("clothing");
    edit::create(&module, "demo").unwrap();
    let drop = dir.join("drop");
    put(&drop, &format!("{M}^jbib_000_u.ydd"), &ydd("jacket"));
    put(&drop, &format!("{M}^jbib_diff_000_a_uni.ytd"), &ytd("tex"));
    let added = add_clothes(&module, std::slice::from_ref(&drop)).expect("the drop");
    let yld = put(&dir.join("elsewhere"), "jacket.yld", &a_yld(1.0));
    edit::link_cloth(&module, &added.garments[0], &yld).expect("the cloth is linked");
    (module, added.garments[0].clone())
}

#[test]
fn a_scale_is_stated_in_every_cloth_of_a_file_and_read_back_and_a_file_that_is_no_cloth_is_refused()
{
    let bytes = a_yld(1.0);
    assert_eq!(wind::stated(&bytes).unwrap(), [1.0]);

    let windier = wind::with_scale(&bytes, 2.5).unwrap();
    assert_eq!(wind::stated(&windier).unwrap(), [2.5]);
    assert_ne!(windier, bytes, "the file changed");
    assert_eq!(
        wind::with_scale(&windier, 1.0).unwrap(),
        bytes,
        "stating the scale it had writes the file it was"
    );

    let why = wind::with_scale(b"not a cloth dictionary", 2.0).unwrap_err();
    assert!(!why.is_empty());
    assert!(wind::stated(b"nor this").is_err());
}

#[test]
fn a_wind_is_an_edit_that_names_what_it_refuses_and_undo_takes_back() {
    let dir = tempfile::tempdir().unwrap();
    let (module, id) = a_jacket_with_cloth(dir.path());
    let yld = Wardrobe::read(&module)
        .unwrap()
        .garment(&id)
        .unwrap()
        .cloth
        .clone()
        .unwrap();
    let linked_bytes = std::fs::read(module.join(&yld)).unwrap();

    let ((), revision) =
        edit::revise(&module, |w| edit::apply(w, &id, Edit::Wind(Some(2.5)))).unwrap();
    let revision = revision.expect("the wind is a change");
    assert_eq!(
        Wardrobe::read(&module)
            .unwrap()
            .garment(&id)
            .unwrap()
            .wind_scale,
        Some(2.5)
    );
    assert_eq!(
        std::fs::read(module.join(&yld)).unwrap(),
        linked_bytes,
        "the linked file is never touched: the build states the wind"
    );

    edit::undo_revision(&module, &revision).unwrap();
    assert_eq!(
        Wardrobe::read(&module)
            .unwrap()
            .garment(&id)
            .unwrap()
            .wind_scale,
        None
    );

    // Refused: not a number, not a scale, and a garment with no cloth to state it in; nothing changes then.
    for bad in [
        f32::NAN,
        f32::INFINITY,
        wind::LARGEST * 2.0,
        -wind::LARGEST * 2.0,
    ] {
        let why = edit::revise(&module, |w| edit::apply(w, &id, Edit::Wind(Some(bad))))
            .unwrap_err()
            .to_string();
        assert!(why.contains("wind scale"), "{bad}: {why}");
    }
    edit::unlink_cloth(&module, &id).unwrap();
    let why = edit::revise(&module, |w| edit::apply(w, &id, Edit::Wind(Some(2.0))))
        .unwrap_err()
        .to_string();
    assert!(why.contains("no cloth"), "{why}");
    let ((), none) = edit::revise(&module, |w| edit::apply(w, &id, Edit::Wind(None))).unwrap();
    assert!(
        none.is_none(),
        "taking back what was never stated changes nothing"
    );
    assert_eq!(
        Wardrobe::read(&module)
            .unwrap()
            .garment(&id)
            .unwrap()
            .wind_scale,
        None
    );
}

#[test]
fn the_plan_ships_the_linked_cloth_as_it_is_and_with_a_wind_ships_the_same_file_stating_it() {
    let dir = tempfile::tempdir().unwrap();
    let (module, id) = a_jacket_with_cloth(dir.path());
    let planned_files = |module: &Path| {
        let w = Wardrobe::read(module).unwrap();
        let planned = plan(&w, &inspect(module, &w, Depth::Stat), "wardrobe", module);
        let cloth = planned
            .contribution
            .files
            .iter()
            .find(|f| f.path.ends_with(".yld"))
            .expect("the plan ships the cloth")
            .clone();
        (planned, cloth)
    };

    let (_, plain) = planned_files(&module);
    assert!(
        matches!(plain.origin, Origin::Copy { .. }),
        "{:?}",
        plain.origin
    );

    edit::update(&module, |w| edit::apply(w, &id, Edit::Wind(Some(3.0)))).unwrap();
    let (planned, windy) = planned_files(&module);
    let Origin::Generated(bytes) = &windy.origin else {
        panic!("the cloth is written with its wind: {:?}", windy.origin);
    };
    assert_eq!(windy.path, plain.path, "under the same name");
    assert_eq!(wind::stated(bytes).unwrap(), [3.0]);
    assert!(
        planned.contribution.problems.is_empty(),
        "{:?}",
        planned.contribution.problems
    );

    // A wind changes what the garment writes, so the next build diff names the garment.
    let digest = |planned: &or_wardrobe::Planned| planned.contribution.items[0].digest.clone();
    let before = planned_files(&module).0;
    edit::update(&module, |w| edit::apply(w, &id, Edit::Wind(Some(4.0)))).unwrap();
    assert_ne!(digest(&before), digest(&planned_files(&module).0));

    // A cloth the wind cannot be stated in is a problem the build stops on, named for the garment.
    put(&module, "cloth/jacket.yld", b"damaged");
    let (planned, _) = planned_files(&module);
    assert!(
        planned
            .contribution
            .problems
            .iter()
            .any(|p| p.contains("cannot be given the wind scale")),
        "{:?}",
        planned.contribution.problems
    );
}

#[test]
fn the_verifier_reads_the_shipped_cloth_back_and_names_a_build_that_does_not_hold_the_scale() {
    let dir = tempfile::tempdir().unwrap();
    let (module, id) = a_jacket_with_cloth(dir.path());
    edit::update(&module, |w| edit::apply(w, &id, Edit::Wind(Some(3.0)))).unwrap();
    let w = Wardrobe::read(&module).unwrap();
    let planned = plan(&w, &inspect(&module, &w, Depth::Stat), "wardrobe", &module);

    // The files of a build, as the plan says it writes them: the generated ones are the bytes, the copies are the files.
    let built = dir.path().join("built");
    for file in &planned.contribution.files {
        let bytes = match &file.origin {
            Origin::Generated(bytes) => bytes.clone(),
            Origin::Document(text) => text.clone().into_bytes(),
            Origin::Copy { from, .. } => std::fs::read(from).unwrap(),
            // The models and textures are sources the build cooks; the cloth check reads neither.
            Origin::Cook { .. } | Origin::Retarget { .. } => continue,
        };
        put(&built, &file.path, &bytes);
    }
    let cloth = planned
        .contribution
        .files
        .iter()
        .find(|f| f.path.ends_with(".yld"))
        .unwrap()
        .path
        .clone();

    let found = verify::verify(&w, &planned, &built);
    let check = found
        .checks
        .iter()
        .find(|c| c.name == "cloth")
        .expect("the cloth is checked");
    assert!(check.passed, "{}", check.detail);
    assert!(
        check
            .detail
            .contains("1 of them state in the build the wind scale their pack says"),
        "{}",
        check.detail
    );

    // A build that shipped the cloth as it was linked does not state the scale.
    put(&built, &cloth, &a_yld(1.0));
    let found = verify::verify(&w, &planned, &built);
    let problems = found.problems();
    assert!(
        problems.iter().any(|p| p.starts_with("cloth: ")
            && p.contains("states the wind scale [1.0]")
            && p.contains("the pack says 3")),
        "{problems:?}"
    );
}

#[test]
fn a_prop_and_a_garment_without_cloth_ship_no_wind() {
    let mut w = Wardrobe::new("demo");
    let mut hat = garment(Ped::Male, common::P_HEAD, Some(0), "h.ydd", &["h.ytd"]);
    hat.cloth = Some("cloth/h.yld".into());
    w.garments.push(hat);
    let id = w.garments[0].id.clone();
    let why = edit::apply(&mut w, &id, Edit::Wind(Some(2.0)))
        .unwrap_err()
        .to_string();
    assert!(why.contains("prop"), "{why}");
    let _ = JBIB;
}

#[test]
fn the_wind_of_every_shipped_cloth_is_read_and_stated_again() {
    use or_test_support::{Corpus, TARGET_GEN8};
    let Some(files) = Corpus::new(env!("CARGO_MANIFEST_DIR"), "yld")
        .target(TARGET_GEN8)
        .find_or_skip("the_wind_of_every_shipped_cloth_is_read_and_stated_again")
    else {
        return;
    };
    let (mut least, mut most, mut read) = (f32::MAX, f32::MIN, 0usize);
    for file in &files {
        let bytes = std::fs::read(file).unwrap();
        let winds = wind::stated(&bytes).unwrap_or_else(|why| panic!("{}: {why}", file.display()));
        for scale in &winds {
            least = least.min(*scale);
            most = most.max(*scale);
        }
        read += winds.len();
        // Stating the scale a cloth has leaves a cloth that reads back with the same scale.
        if let Some(first) = winds.first() {
            let again = wind::with_scale(&bytes, *first)
                .unwrap_or_else(|why| panic!("{}: {why}", file.display()));
            assert_eq!(wind::stated(&again).unwrap(), vec![*first; winds.len()]);
        }
    }
    eprintln!(
        "{} cloth files, {read} controllers, wind scale from {least} to {most}",
        files.len()
    );
    assert!(read > 0, "no controller was read");
    assert!(
        most <= wind::SHIPPED_MOST && least >= 0.0,
        "the shipped cloths state wind scales from {least} to {most}"
    );
    assert!(
        wind::LARGEST > most,
        "a person may state more than any shipped cloth does"
    );
}

#[test]
fn the_wind_a_linked_file_states_is_read_where_it_is_and_through_a_reference() {
    let dir = tempfile::tempdir().unwrap();
    let (module, id) = a_jacket_with_cloth(dir.path());
    let cloth = Wardrobe::read(&module)
        .unwrap()
        .garment(&id)
        .unwrap()
        .cloth
        .clone()
        .unwrap();
    assert_eq!(wind::stated_by(&module, &cloth).unwrap(), [1.0]);

    // A reference leads to a file of the project, which is where its wind is read.
    put(&module, "other/windy.yld", &a_yld(2.5));
    let reference = or_project::reference::Reference::here("other/windy.yld", None);
    std::fs::create_dir_all(module.join("cloth")).unwrap();
    std::fs::write(module.join("cloth/windy.ref.json"), reference.to_text()).unwrap();
    assert_eq!(
        wind::stated_by(&module, "cloth/windy.ref.json").unwrap(),
        [2.5]
    );

    let why = wind::stated_by(&module, "cloth/not_here.yld").unwrap_err();
    assert!(!why.is_empty());
}
