//! Two clothing modules that claim one collection are refused at planning, whichever output takes
//! them, and a build that was changed afterwards fails the check that names the file.

mod common;

use common::{garment, put, staged, JBIB};
use or_project::{build, build_root, Container, Module, Output, Project};
use or_wardrobe::verify::verify;
use or_wardrobe::{plans, Depth, Ped, Wardrobe};

const WHAT: &str = "two_packs_of_one_collection_are_refused_and_a_tampered_build_fails_its_check";

fn module(project_dir: &std::path::Path, folder: &str, pack: &str) -> Option<()> {
    let ydds = staged("ydd", 1, WHAT)?;
    let ytds = staged("ytd", 1, WHAT)?;
    let dir = project_dir.join(folder);
    put(&dir, "m.ydd", &ydds[0]);
    put(&dir, "t.ytd", &ytds[0]);
    let mut w = Wardrobe::new(pack);
    w.garments
        .push(garment(Ped::Male, JBIB, None, "m.ydd", &["t.ytd"]));
    w.write(&dir).unwrap();
    Some(())
}

#[test]
fn two_modules_claiming_one_collection_are_refused_even_when_an_output_takes_one() {
    let dir = tempfile::tempdir().unwrap();
    if module(dir.path(), "one", "police").is_none()
        || module(dir.path(), "two", "police").is_none()
    {
        return;
    }
    let container = Container {
        modules: vec![
            Module::clothing("one", "one"),
            Module::clothing("two", "two"),
        ],
        outputs: vec![
            Output::fivem_resource("both", "both_pack", &["one", "two"]),
            Output::fivem_resource("first", "first_pack", &["one"]),
        ],
    };
    let path = dir.path().join("p.openrage-proj");
    Project::create(&path, &container, &[]).unwrap();
    let project = Project::load(&path).unwrap();

    for output in ["both", "first"] {
        let plan = plans(&project, output).unwrap().remove(0);
        assert!(
            plan.problems
                .iter()
                .any(|p| p.contains("mp_m_police") && p.contains("`one`") && p.contains("`two`")),
            "{output}: {:?}",
            plan.problems
        );
        assert!(!plan.is_buildable());
    }

    // The same collection under another pack name is another collection.
    module(dir.path(), "two", "other").unwrap();
    let plan = plans(&project, "both").unwrap().remove(0);
    let claimed: Vec<_> = plan
        .problems
        .iter()
        .filter(|p| p.contains("clothing collection"))
        .collect();
    assert!(claimed.is_empty(), "{claimed:?}");
}

#[test]
fn a_build_that_was_changed_afterwards_fails_the_check_that_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    if module(dir.path(), ".", "police").is_none() {
        return;
    }
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::fivem_resource(
            "fivem",
            "police_pack",
            &["wardrobe"],
        )],
    };
    let path = dir.path().join("p.openrage-proj");
    Project::create(&path, &container, &[]).unwrap();
    let project = Project::load(&path).unwrap();
    let w = Wardrobe::read(dir.path()).unwrap();
    let planned = or_wardrobe::planned(
        &project.folder(),
        project.container.module("wardrobe").unwrap(),
        Depth::Digest,
    )
    .unwrap();
    let plan = plans(&project, "fivem").unwrap().remove(0);
    let built = build(&plan, &build_root(&path)).expect("it builds");

    let clean = verify(&w, &planned, &built.folder);
    assert!(clean.is_clean(), "{:?}", clean.problems());
    assert_eq!(
        clean.checks.len(),
        3,
        "the male variation file and shop file, and what the garments do"
    );

    // The shop metadata loses an entry, and the variation file is another pack's.
    let shop = built.folder.join("data/mp_m_freemode_01_mp_m_police.meta");
    let text = std::fs::read_to_string(&shop).unwrap();
    let start = text.find("    <Item>").unwrap();
    let end = text[start..].find("</Item>\n").unwrap() + start + "</Item>\n".len();
    std::fs::write(&shop, format!("{}{}", &text[..start], &text[end..])).unwrap();
    let ymt = built
        .folder
        .join("stream/police/mp_m_freemode_01_mp_m_police.ymt");
    let mut bytes = std::fs::read(&ymt).unwrap();
    bytes.truncate(bytes.len() - 4);
    std::fs::write(&ymt, bytes).unwrap();

    let found = verify(&w, &planned, &built.folder);
    assert!(!found.is_clean());
    let problems = found.problems();
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("mp_m_freemode_01 shop metadata")
                && p.contains("pedComponents holds 0")),
        "{problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("mp_m_freemode_01 variation metadata")),
        "{problems:?}"
    );
}
