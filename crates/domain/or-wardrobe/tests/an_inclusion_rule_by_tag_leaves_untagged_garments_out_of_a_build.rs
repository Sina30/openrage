//! A build can take only the garments filed under a tag, such as `singleplayer`: the others ship no file and no metadata of their
//! own, and keep their place as a drawable with no model, so the numbers of the rest are the numbers they have in a build of
//! the whole pack and a frozen pack does not call it a renumbering.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{garment, put_bytes, ydd, ytd, JBIB};
use or_project::OutputKind;
use or_wardrobe::layout::Paths;
use or_wardrobe::options::Options;
use or_wardrobe::plan::Build;
use or_wardrobe::{freeze, inspect, plan_for, Depth, Ped, Planned, Wardrobe};

fn pack(folder: &Path) -> Wardrobe {
    let mut w = Wardrobe::new("tagged");
    for (name, tags) in [
        ("anorak", vec!["singleplayer"]),
        ("blazer", vec![]),
        ("cardigan", vec!["Singleplayer", "wool"]),
        ("duffel", vec!["wool"]),
    ] {
        put_bytes(folder, &format!("models/{name}.ydd"), &ydd(name));
        put_bytes(folder, &format!("textures/{name}.ytd"), &ytd(name));
        let mut g = garment(
            Ped::Male,
            JBIB,
            None,
            &format!("models/{name}.ydd"),
            &[&format!("textures/{name}.ytd")],
        );
        g.label = Some(name.into());
        g.tags = tags.into_iter().map(String::from).collect();
        w.garments.push(g);
    }
    w.write(folder).unwrap();
    w
}

fn planned(folder: &Path, w: &Wardrobe, tags: &[&str]) -> Planned {
    let options = Options {
        include_tags: tags.iter().map(|t| t.to_string()).collect(),
        ..Options::default()
    };
    let build = Build {
        paths: Paths::resource(&w.pack, true),
        options,
        kind: OutputKind::FivemResource,
    };
    plan_for(w, &inspect(folder, w, Depth::Open), "pack", folder, &build)
}

fn streamed(p: &Planned) -> BTreeSet<String> {
    (p.contribution.files.iter())
        .map(|f| f.path.clone())
        .filter(|path| path.ends_with(".ydd") || path.ends_with(".ytd"))
        .collect()
}

fn figure(p: &Planned, name: &str) -> u64 {
    (p.contribution.figures.iter())
        .find(|f| f.name == name)
        .map_or(0, |f| f.amount)
}

#[test]
fn an_inclusion_rule_by_tag_ships_the_tagged_garments_at_the_numbers_they_have_in_the_whole_pack() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);
    let whole = planned(at, &w, &[]);
    let some = planned(at, &w, &["SINGLEPLAYER"]);

    // Eight files for four garments; the rule keeps the model and the texture of the two tagged ones, at the same places.
    let all = streamed(&whole);
    let kept = streamed(&some);
    assert_eq!(all.len(), 8);
    let place = |path: &str| {
        let number = path.rsplit('^').next().unwrap();
        number
            .trim_start_matches("jbib_diff_")
            .trim_start_matches("jbib_")[..3]
            .to_string()
    };
    assert_eq!(
        kept.iter().map(|p| place(p)).collect::<BTreeSet<_>>(),
        BTreeSet::from(["000".to_string(), "002".to_string()]),
        "anorak is place 0 and cardigan place 2: {kept:?}"
    );
    assert_eq!(kept.len(), 4);
    assert!(kept.is_subset(&all), "no file is renamed by the rule");

    // The untagged garments are places, so the metadata still counts four drawables and the ones after them did not move.
    assert_eq!(
        figure(&some, "clothing_drawables"),
        figure(&whole, "clothing_drawables")
    );
    assert_eq!(figure(&some, "clothing_drawables"), 4);
    assert!(some.findings.iter().all(|f| f.code != "renumbered"));

    // A word no garment carries leaves everything out but the places, and the build still plans.
    let none = planned(at, &w, &["nobody"]);
    assert!(streamed(&none).is_empty());
    assert_eq!(figure(&none, "clothing_drawables"), 4);
}

#[test]
fn a_frozen_pack_does_not_call_a_tag_rule_a_renumbering_and_would_call_a_removal_one() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    pack(at);
    freeze::keep(at).unwrap();
    let locked = Wardrobe::read(at).unwrap();
    assert!(locked.lock.is_some());

    let some = planned(at, &locked, &["singleplayer"]);
    assert!(
        some.findings.iter().all(|f| f.code != "renumbered"),
        "{:?}",
        some.findings
    );

    // The control: the same garments taken out of the pack move the ones after them, and the lock refuses that.
    let mut removed = locked.clone();
    removed
        .garments
        .retain(|g| g.carries_a_tag_of(&["singleplayer".to_string()]));
    let moved = planned(at, &removed, &[]);
    assert!(
        moved.findings.iter().any(|f| f.code == "renumbered"),
        "the cardigan would move from 2 to 1: {:?}",
        moved.findings
    );
}
