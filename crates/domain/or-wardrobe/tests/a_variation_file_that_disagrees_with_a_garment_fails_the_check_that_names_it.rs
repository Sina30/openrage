//! The verifier reads a built variation file against the garments, one by one, and names the garment
//! and the way the file disagrees with it: a drawable that is missing, a texture count that is not the
//! garment's, a drawable with two component infos or none, and a prop that is missing or short of textures.
//!
//! The files are the plan's own, laid out as a build lays them, and the variation file is then replaced
//! by another that reopens, so each failure is the verifier's reading of a valid file and not a file
//! that cannot be read. No staged file is needed: the verifier looks at metadata only.

mod common;

use std::path::Path;

use common::{garment, JBIB, LOWR, P_HEAD};
use or_io::decl::File;
use or_io::meta::CPedVariationInfo;
use or_project::Origin;
use or_wardrobe::verify::verify;
use or_wardrobe::{inspect, layout, plan, Depth, Ped, Planned, Wardrobe};

const MALE: &str = "mp_m_freemode_01_mp_m_pack";

fn pack(dir: &Path) -> Wardrobe {
    for name in [
        "m0.ydd", "m1.ydd", "m2.ydd", "t0.ytd", "t1.ytd", "t2.ytd", "t3.ytd",
    ] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let mut w = Wardrobe::new("pack");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(0),
        "m0.ydd",
        &["t0.ytd", "t1.ytd"],
    ));
    w.garments
        .push(garment(Ped::Male, LOWR, Some(0), "m1.ydd", &["t2.ytd"]));
    w.garments.push(garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "m2.ydd",
        &["t2.ytd", "t3.ytd"],
    ));
    w
}

/// The metadata and shop files of the plan, where a build puts them in `resource`.
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

/// The problems the verifier finds once the male variation file is `change` of what the plan made.
fn after(
    w: &Wardrobe,
    planned: &Planned,
    resource: &Path,
    change: impl Fn(&mut CPedVariationInfo),
) -> Vec<String> {
    let mut info = planned.documents.variations[&Ped::Male].clone();
    change(&mut info);
    info.restate_counts();
    let path = layout::variation_path("pack", Ped::Male, "mp_m_pack");
    std::fs::write(
        resource.join(path),
        info.to_bytes().expect("the changed file is a valid one"),
    )
    .unwrap();
    verify(w, planned, resource).problems()
}

fn problem_with(problems: &[String], text: &str) -> bool {
    problems
        .iter()
        .any(|p| p.starts_with("mp_m_freemode_01 variation metadata") && p.contains(text))
}

#[test]
fn each_way_a_variation_file_can_disagree_with_a_garment_is_named_with_the_garment() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let facts = inspect(dir.path(), &w, Depth::Digest);
    let planned = plan(&w, &facts, "wardrobe", dir.path());
    assert!(planned.contribution.problems.is_empty());
    let resource = dir.path().join("resource");
    laid_out(&planned, &resource);

    let clean = verify(&w, &planned, &resource);
    assert!(clean.is_clean(), "{:?}", clean.problems());

    // A drawable that wears one texture fewer than its garment has.
    let problems = after(&w, &planned, &resource, |info| {
        info.a_component_data3[1].a_drawbl_data3[0].a_tex_data.pop();
    });
    assert!(
        problem_with(
            &problems,
            &format!("{MALE}^jbib_000 states 1 textures and has 2")
        ),
        "{problems:?}"
    );

    // A drawable the garment places that the file does not have, and the info that named it.
    let problems = after(&w, &planned, &resource, |info| {
        info.a_component_data3[0].a_drawbl_data3.pop();
        info.comp_infos.retain(|i| i.ped_xml_comp_idx != 4);
    });
    assert!(
        problem_with(&problems, &format!("{MALE}^lowr_000 has no drawable")),
        "{problems:?}"
    );

    // A drawable with a second component info, and one with none.
    let problems = after(&w, &planned, &resource, |info| {
        let second = info.comp_infos[0].clone();
        info.comp_infos.push(second);
    });
    assert!(
        problem_with(&problems, "has 2 component infos, and a drawable has one"),
        "{problems:?}"
    );
    let problems = after(&w, &planned, &resource, |info| {
        info.comp_infos.retain(|i| i.ped_xml_comp_idx != 11);
    });
    assert!(
        problem_with(&problems, &format!("{MALE}^jbib_000 has 0 component infos")),
        "{problems:?}"
    );

    // A prop that is short of a texture, and one the file lacks.
    let problems = after(&w, &planned, &resource, |info| {
        info.prop_info.a_prop_meta_data[0].tex_data.pop();
    });
    assert!(
        problem_with(
            &problems,
            "mp_m_freemode_01_p_mp_m_pack^p_head_000 states 1 textures and has 2"
        ),
        "{problems:?}"
    );
    let problems = after(&w, &planned, &resource, |info| {
        info.prop_info.a_prop_meta_data.pop();
    });
    assert!(
        problem_with(&problems, "p_head_000 has no prop"),
        "{problems:?}"
    );
}
