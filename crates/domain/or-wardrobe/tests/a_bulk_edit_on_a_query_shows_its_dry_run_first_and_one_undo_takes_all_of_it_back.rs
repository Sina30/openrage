//! A bulk edit acts on the garments a query selects: its dry run says what changes in how many garments and writes nothing, the edit
//! is one step, and one Undo restores every garment it touched.

mod common;

use std::path::Path;

use common::{garment, JBIB, LOWR};
use or_wardrobe::backup::versions;
use or_wardrobe::bulk::{dry_run, ids_of, make};
use or_wardrobe::edit::{undo_revision, Edit};
use or_wardrobe::{Garment, Ped, Wardrobe};

fn named(label: &str, ped: Ped, slot: or_wardrobe::Slot, tags: &[&str]) -> Garment {
    let mut g = garment(ped, slot, None, &format!("models/{label}.ydd"), &[]);
    g.label = Some(label.into());
    g.tags = tags.iter().map(|t| t.to_string()).collect();
    g
}

fn pack(folder: &Path) -> Wardrobe {
    let mut w = Wardrobe::new("bulk");
    w.garments = vec![
        named("anorak", Ped::Male, JBIB, &["wool", "singleplayer"]),
        named("blazer", Ped::Male, JBIB, &["wool"]),
        named("cardigan", Ped::Male, JBIB, &["singleplayer"]),
        named("denim", Ped::Female, LOWR, &[]),
        named("etui", Ped::Female, LOWR, &["wool"]),
    ];
    w.write(folder).unwrap();
    w
}

fn tags_of(w: &Wardrobe, label: &str) -> Vec<String> {
    (w.garments.iter())
        .find(|g| g.label.as_deref() == Some(label))
        .map(|g| g.tags.clone())
        .unwrap()
}

#[test]
fn a_dry_run_says_what_changes_in_how_many_garments_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    pack(at);
    let wool = ids_of(at, "tag:wool").unwrap();
    assert_eq!(
        wool.len(),
        3,
        "the query selects the garments the pack states"
    );

    let held = std::fs::read(at.join("wardrobe.json")).unwrap();
    let kept = versions(at).len();
    let w = Wardrobe::read(at).unwrap();

    let run = dry_run(&w, &wool, &Edit::AddTag("winter".into()));
    assert_eq!((run.selected, run.changed.len(), run.unchanged), (3, 3, 0));
    assert!(run.refused.is_empty());
    assert_eq!(run.sentence(), "Would change all 3 garments");
    let anorak = run.changed.iter().find(|c| c.name == "anorak").unwrap();
    assert_eq!(
        anorak.lines,
        ["tags: wool, singleplayer -> wool, singleplayer, winter"]
    );

    // One of the three has the tag already, so it is left as it is and counted so.
    let run = dry_run(&w, &wool, &Edit::AddTag("Singleplayer".into()));
    assert_eq!((run.selected, run.changed.len(), run.unchanged), (3, 2, 1));
    assert_eq!(run.sentence(), "Would change 2 of the 3 garments selected");
    let run = dry_run(&w, &wool[..1], &Edit::AddTag("singleplayer".into()));
    assert_eq!(
        run.sentence(),
        "Would change nothing: the garment selected already states it"
    );

    // Taking them out is said as that.
    let run = dry_run(&w, &wool, &Edit::Remove);
    assert_eq!(run.changed.len(), 3);
    assert_eq!(run.changed[0].lines, ["is taken out of the pack"]);

    assert_eq!(
        std::fs::read(at.join("wardrobe.json")).unwrap(),
        held,
        "a dry run writes nothing"
    );
    assert_eq!(versions(at).len(), kept, "and keeps no version");
}

#[test]
fn a_bulk_edit_on_a_query_is_one_step_and_one_undo_restores_every_garment_it_touched() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    pack(at);
    let before = Wardrobe::read(at).unwrap();
    let singleplayer = ids_of(at, "tag:singleplayer slot:jbib").unwrap();
    assert_eq!(singleplayer.len(), 2);

    let (run, step) = make(at, &singleplayer, Edit::AddTag("uniform".into())).unwrap();
    assert_eq!(run.changed.len(), 2);
    let step = step.expect("something changed, so there is a step to take back");

    let after = Wardrobe::read(at).unwrap();
    assert_eq!(
        tags_of(&after, "anorak"),
        ["wool", "singleplayer", "uniform"]
    );
    assert_eq!(tags_of(&after, "cardigan"), ["singleplayer", "uniform"]);
    for untouched in ["blazer", "denim", "etui"] {
        assert_eq!(tags_of(&after, untouched), tags_of(&before, untouched));
    }
    assert_eq!(step.before, before);
    assert_eq!(step.after, after);

    // The query that selected them selects them still, and the edit made again changes nothing.
    assert_eq!(ids_of(at, "tag:uniform").unwrap(), singleplayer);
    let (again, none) = make(at, &singleplayer, Edit::AddTag("uniform".into())).unwrap();
    assert_eq!((again.changed.len(), again.unchanged), (0, 2));
    assert!(none.is_none(), "nothing changed, so nothing to take back");

    undo_revision(at, &step).unwrap();
    assert_eq!(
        Wardrobe::read(at).unwrap(),
        before,
        "all of it, in one Undo"
    );
}

#[test]
fn an_edit_one_garment_refuses_is_refused_for_all_and_the_dry_run_says_which() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    pack(at);
    let before = std::fs::read(at.join("wardrobe.json")).unwrap();
    let wool = ids_of(at, "tag:wool").unwrap();

    // A heel is a value of shoes, and these are tops and legs.
    let run = dry_run(&Wardrobe::read(at).unwrap(), &wool, &Edit::Heels(Some(0.1)));
    assert_eq!(run.refused.len(), 3);
    assert!(run.changed.is_empty());
    assert!(
        run.sentence()
            .contains("3 garments refuse it, so none is changed"),
        "{}",
        run.sentence()
    );
    assert!(!run.is_worth_making());

    let why = make(at, &wool, Edit::Heels(Some(0.1)))
        .unwrap_err()
        .to_string();
    assert!(why.contains("anorak"), "{why}");
    assert_eq!(std::fs::read(at.join("wardrobe.json")).unwrap(), before);

    // A query that selects nothing changes nothing, and says so.
    let none = ids_of(at, "tag:nobody").unwrap();
    assert!(none.is_empty());
    assert_eq!(
        dry_run(
            &Wardrobe::read(at).unwrap(),
            &none,
            &Edit::AddTag("x".into())
        )
        .sentence(),
        "The query selects no garment, so there is nothing to change"
    );
}
