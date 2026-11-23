//! Normal and specular maps and a texture budget over a pack: both are made in copies of the sources, the garments in scope name
//! the copies, and one Undo takes each back; a plan shows what a budget would do and changes no byte until it is accepted.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, put_bytes, snapshot, ytd_of, JBIB};
use or_io::codec::texture::GrcTextureFormat;
use or_source::asset::Source;
use or_source::budget::{measure, Options as BudgetOptions};
use or_source::game_import::{decompile, Facts as GameFacts};
use or_source::maps::Options as MapOptions;
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::{budget, maps};
use or_wardrobe::{Ped, Wardrobe};

const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;
const DIFFUSE: &str = "textures/jacket.textures.src";

fn a_pack(dir: &Path, textures: &[(&str, u32)], sources: &[&str]) -> (PathBuf, Vec<String>) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("textures")).unwrap();
    for source in sources {
        let name = source
            .trim_start_matches("textures/")
            .trim_end_matches(".textures.src");
        let list: Vec<(&str, GrcTextureFormat, u32, u32, u32)> = textures
            .iter()
            .map(|(n, side)| (*n, DXT5, *side, *side, 7))
            .collect();
        decompile(
            &ytd_of(&list),
            &format!("{name}.ytd"),
            &pack.join("textures"),
            &GameFacts::default(),
        )
        .unwrap();
    }
    put_bytes(
        &pack,
        "textures/shipped.ytd",
        &ytd_of(&[("jbib_diff_000_b_uni", DXT5, 64, 64, 7)]),
    );
    let mut w = Wardrobe::new("demo");
    let mut ids = Vec::new();
    for (n, texture) in sources
        .iter()
        .chain(&sources[..1])
        .chain(&["textures/shipped.ytd"])
        .enumerate()
    {
        let mut g = garment(Ped::Male, JBIB, Some(n as u32), "models/m.ydd", &[*texture]);
        g.label = Some(format!("jacket {n}"));
        ids.push(g.id.clone());
        w.garments.push(g);
    }
    w.write(&pack).unwrap();
    (pack, ids)
}

fn textures_of(pack: &Path) -> Vec<String> {
    Wardrobe::read(pack)
        .unwrap()
        .garments
        .iter()
        .map(|g| g.variations[0].texture.clone().unwrap())
        .collect()
}

#[test]
fn maps_are_made_in_a_copy_a_plan_changes_nothing_and_undo_takes_the_copy_away() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, ids) = a_pack(dir.path(), &[("jbib_diff_000_a_uni", 64)], &[DIFFUSE]);
    let before = snapshot(&pack);
    let options = MapOptions::default();

    // The plan names the maps and the garments that would have them, and writes nothing.
    let plan = maps::plan(&pack, None, &options).unwrap();
    assert_eq!(plan.made.len(), 1, "{plan:?}");
    assert_eq!(plan.made[0].into, None);
    assert_eq!(plan.made[0].garments, ["jacket 0", "jacket 1"]);
    assert_eq!(plan.maps(), 2);
    assert!(
        plan.left.iter().any(|l| l.file == "textures/shipped.ytd"),
        "a compiled file is left, with why: {:?}",
        plan.left
    );
    assert_eq!(snapshot(&pack), before, "a plan changes no byte");

    let (report, revision) =
        maps::generate(&pack, Some(&ids[..1]), &options, &Watch::default()).unwrap();
    let into = report.made[0].into.clone().expect("a copy");
    assert_eq!(report.maps(), 2);
    assert_eq!(textures_of(&pack)[0], into);
    assert_eq!(
        textures_of(&pack)[1],
        DIFFUSE,
        "only the garment asked for names the copy"
    );
    let after = snapshot(&pack);
    for (path, bytes) in &before {
        if !path.ends_with("wardrobe.json") {
            assert_eq!(after.get(path), Some(bytes), "{path} was changed");
        }
    }
    let copy = Source::open(&pack.join(&into)).unwrap();
    let held: Vec<String> = or_source::textures::inputs_of(&copy)
        .unwrap()
        .into_iter()
        .map(|i| i.texture)
        .collect();
    assert_eq!(
        held,
        [
            "jbib_diff_000_a_uni",
            "jbib_normal_000_a_uni",
            "jbib_spec_000_a_uni"
        ]
    );
    let compiled = or_source::textures::compile(&copy).unwrap();
    assert!(!compiled.bytes.is_empty());

    // Run again over the copy: it has them, and nothing is copied.
    let (again, none) =
        maps::generate(&pack, Some(&ids[..1]), &options, &Watch::default()).unwrap();
    assert!(again.made.is_empty() && none.is_none(), "{again:?}");

    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    assert!(!pack.join(&into).exists());
    let undone = snapshot(&pack);
    for (path, bytes) in &before {
        assert_eq!(undone.get(path), Some(bytes), "{path} is not as it was");
    }
}

#[test]
fn a_budget_plan_changes_no_byte_and_an_accepted_one_meets_the_target_and_undo_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let textures = [
        ("jbib_diff_000_a_uni", 128),
        ("jbib_diff_000_c_uni", 128),
        ("jbib_diff_000_d_uni", 128),
        ("jbib_diff_000_e_uni", 128),
    ];
    let (pack, ids) = a_pack(
        dir.path(),
        &textures,
        &[DIFFUSE, "textures/trousers.textures.src"],
    );
    let before = snapshot(&pack);
    let options = BudgetOptions::default();
    let watch = Watch::default();

    // The plan, for a target of a half of what the textures it can change take.
    let all = budget::plan(&pack, None, u64::MAX, &options, &watch).unwrap();
    assert!(all.sources.is_empty() && all.met, "{all:?}");
    let total = all.before;
    assert!(total > 0);
    let target = total / 2;
    let plan = budget::plan(&pack, None, target, &options, &watch).unwrap();
    assert_eq!(snapshot(&pack), before, "a plan changes no byte");
    assert!(plan.met, "{plan:?}");
    assert!(plan.after <= target && plan.slack < 0.15, "{plan:?}");
    assert_eq!(plan.before, total);
    assert!(plan.textures() > 0);
    for source in &plan.sources {
        for choice in &source.choices {
            assert!(choice.now().bytes < choice.was().bytes);
        }
    }
    assert!(plan
        .lines()
        .iter()
        .any(|l| l.contains("dB") || l.contains("becomes")));

    // Accepting it names copies, which hold what the plan said.
    let (report, revision) = budget::accept(&pack, None, &plan, &options, &watch).unwrap();
    assert_eq!(report.textures(), plan.textures(), "{report:?}");
    let names = textures_of(&pack);
    assert!(names
        .iter()
        .all(|n| n != DIFFUSE && n != "textures/trousers.textures.src" || n.ends_with(".ytd")));
    let mut stored = 0;
    for source in &report.changed {
        let measured = measure(&pack.join(&source.into), &options, &|| false).unwrap();
        stored += measured.bytes();
    }
    let untouched: u64 = plan
        .sources
        .iter()
        .map(|s| {
            measure(&pack.join(&s.source), &options, &|| false)
                .unwrap()
                .bytes()
        })
        .sum();
    let _ = untouched;
    assert!(stored <= target, "{stored} is over the target {target}");
    assert_eq!(stored, plan.after, "the plan said what it would store");

    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    let undone = snapshot(&pack);
    for (path, bytes) in &before {
        assert_eq!(undone.get(path), Some(bytes), "{path} is not as it was");
    }
    let _ = ids;
}

#[test]
fn a_stop_leaves_the_pack_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, _) = a_pack(dir.path(), &[("jbib_diff_000_a_uni", 64)], &[DIFFUSE]);
    let before = snapshot(&pack);
    let watch = Watch::default();
    watch.cancel.set();
    assert!(maps::generate(&pack, None, &MapOptions::default(), &watch).is_err());
    assert!(budget::plan(&pack, None, 1, &BudgetOptions::default(), &watch).is_err());
    assert_eq!(snapshot(&pack), before);
}
