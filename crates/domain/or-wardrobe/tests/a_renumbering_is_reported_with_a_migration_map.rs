//! A garment that is not pinned moves when one before it leaves, and the build says so: by the
//! stable id, with a redirect map from the name the game saw to the name it will.

mod common;

use common::{garment, project, put, staged, JBIB};
use or_project::{build, build_root, since_last_build};
use or_wardrobe::{plans, Ped, Wardrobe};

const WHAT: &str = "a_renumbering_is_reported_with_a_migration_map";

fn pack(dir: &std::path::Path, pins: [Option<u32>; 3]) -> Option<Wardrobe> {
    let ydds = staged("ydd", 4, WHAT)?;
    let ytds = staged("ytd", 1, WHAT)?;
    put(dir, "t.ytd", &ytds[0]);
    let mut w = Wardrobe::new("police");
    for (i, pin) in pins.into_iter().enumerate() {
        put(dir, &format!("m{i}.ydd"), &ydds[i]);
        let mut g = garment(Ped::Male, JBIB, pin, &format!("m{i}.ydd"), &["t.ytd"]);
        g.label = Some(format!("g{i}"));
        w.garments.push(g);
    }
    put(dir, "m3.ydd", &ydds[3]);
    w.garments
        .push(garment(Ped::Male, JBIB, Some(10), "m3.ydd", &["t.ytd"]));
    Some(w)
}

fn name(n: u32) -> String {
    format!("mp_m_freemode_01_mp_m_police^jbib_{n:03}")
}

#[test]
fn removing_a_garment_before_unpinned_ones_moves_them_and_the_diff_says_where() {
    let dir = tempfile::tempdir().unwrap();
    let Some(mut w) = pack(dir.path(), [None, None, None]) else {
        return;
    };
    let ids: Vec<String> = w.garments.iter().map(|g| g.id.clone()).collect();
    let project_before = project(dir.path(), &w, "police_pack");
    let root = build_root(&project_before.path);
    build(&plans(&project_before, "fivem").unwrap().remove(0), &root).expect("the first build");

    // The first garment leaves; the two unpinned ones after it shift down, and the pinned one stays.
    w.garments.remove(0);
    let project_after = project(dir.path(), &w, "police_pack");
    let plan = plans(&project_after, "fivem").unwrap().remove(0);
    let diff = since_last_build(&plan, &root).expect("the first build recorded its items");

    let moved: Vec<(&str, &str, &str)> = diff
        .moved
        .iter()
        .map(|m| (m.id.as_str(), m.from.as_str(), m.to.as_str()))
        .collect();
    let (from1, to1, from2, to2) = (name(1), name(0), name(2), name(1));
    let mut expected = vec![
        (ids[1].as_str(), from1.as_str(), to1.as_str()),
        (ids[2].as_str(), from2.as_str(), to2.as_str()),
    ];
    expected.sort();
    assert_eq!(moved, expected, "the pinned garment at 10 is not in it");
    assert_eq!(diff.removed.len(), 1);
    assert_eq!(diff.removed[0].id, ids[0]);
    assert!(diff.added.is_empty());

    // The redirect map is the diff's moves, by id, as canonical JSON.
    let map: serde_json::Value = serde_json::from_str(&diff.redirect_map()).unwrap();
    let redirects = map["redirects"].as_array().unwrap();
    assert_eq!(redirects.len(), 2);
    assert!(redirects
        .iter()
        .any(|r| r["id"] == ids[1].as_str() && r["from"] == name(1) && r["to"] == name(0)));

    // A build says the same, and records the new numbering for the next.
    let built = build(&plan, &root).expect("the second build");
    assert_eq!(built.since.as_ref().map(|d| d.moved.len()), Some(2));
}

#[test]
fn pinned_garments_are_not_renumbered_when_one_before_them_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let Some(mut w) = pack(dir.path(), [Some(0), Some(1), Some(2)]) else {
        return;
    };
    let before = project(dir.path(), &w, "police_pack");
    let root = build_root(&before.path);
    build(&plans(&before, "fivem").unwrap().remove(0), &root).expect("the first build");

    w.garments.remove(0);
    let after = project(dir.path(), &w, "police_pack");
    let plan = plans(&after, "fivem").unwrap().remove(0);
    let diff = since_last_build(&plan, &root).unwrap();
    assert!(diff.moved.is_empty(), "{:?}", diff.moved);
    assert_eq!(diff.removed.len(), 1);
}
