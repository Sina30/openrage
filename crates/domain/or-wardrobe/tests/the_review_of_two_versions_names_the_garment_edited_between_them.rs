//! Review mode reads two versions of a pack and says which garments, textures and settings were added, changed and removed between
//! them, by the garment's stable id, and writes and changes nothing.

mod common;

use std::path::Path;

use common::{garment, JBIB, LOWR};
use or_wardrobe::backup::versions;
use or_wardrobe::edit::{apply, revise, Edit};
use or_wardrobe::review::{between, of_versions, Kind};
use or_wardrobe::{Garment, Ped, Variation, Wardrobe};

fn named(label: &str, slot: or_wardrobe::Slot, textures: &[&str]) -> Garment {
    let mut g = garment(
        Ped::Male,
        slot,
        None,
        &format!("models/{label}.ydd"),
        textures,
    );
    g.label = Some(label.into());
    g
}

#[test]
fn the_review_names_the_garment_edited_between_two_versions() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let mut w = Wardrobe::new("review");
    w.garments = vec![
        named("anorak", JBIB, &["textures/a.ytd", "textures/b.ytd"]),
        named("blazer", JBIB, &["textures/c.ytd"]),
        named("chinos", LOWR, &[]),
    ];
    w.write(at).unwrap();
    let ids: Vec<String> = w.garments.iter().map(|g| g.id.clone()).collect();

    // The edits of a session, each a write: a tag on the blazer, a tint on the anorak's second texture, a third texture on it, the
    // chinos taken out, a coat added, and a setting of the pack.
    revise(at, |w| apply(w, &ids[1], Edit::AddTag("winter".into()))).unwrap();
    revise(at, |w| {
        w.garments[0].variations[1].tint = Some("whi".into());
        w.garments[0].variations.push(Variation {
            texture: Some("textures/d.ytd".into()),
            ..Variation::default()
        });
        Ok(())
    })
    .unwrap();
    revise(at, |w| apply(w, &ids[2], Edit::Remove)).unwrap();
    let coat = named("coat", JBIB, &["textures/e.ytd"]);
    let coat_id = coat.id.clone();
    revise(at, |w| {
        w.garments.push(coat);
        w.collections.insert(Ped::Male, "mp_m_mine".into());
        Ok(())
    })
    .unwrap();

    // The pack as it was found is the oldest version kept, and the review reads it against the pack as it is now.
    let oldest = versions(at)
        .into_iter()
        .next()
        .expect("the first change keeps the pack as it was");
    assert_eq!(oldest.label, "The pack as it was found");
    let first = oldest.n;
    let held = std::fs::read(at.join("wardrobe.json")).unwrap();
    let kept = versions(at).len();
    let review = of_versions(at, Some(first), None).unwrap();

    let kind_of = |id: &str| review.garments.iter().find(|g| g.id == id).map(|g| g.kind);
    assert_eq!(kind_of(&ids[0]), Some(Kind::Changed), "the anorak");
    assert_eq!(kind_of(&ids[1]), Some(Kind::Changed), "the blazer");
    assert_eq!(kind_of(&ids[2]), Some(Kind::Removed), "the chinos");
    assert_eq!(kind_of(&coat_id), Some(Kind::Added), "the coat");
    assert_eq!(review.garments.len(), 4);

    let blazer = review.garments.iter().find(|g| g.id == ids[1]).unwrap();
    assert_eq!(blazer.name, "blazer");
    assert_eq!(blazer.lines, ["tags: none -> winter"]);
    assert!(blazer.textures.is_empty());

    let anorak = review.garments.iter().find(|g| g.id == ids[0]).unwrap();
    assert!(anorak.lines.is_empty(), "its own fields are as they were");
    let textures: Vec<_> = anorak
        .textures
        .iter()
        .map(|t| (t.number, t.kind, t.file.as_deref()))
        .collect();
    assert_eq!(
        textures,
        [
            (2, Kind::Changed, Some("textures/b.ytd")),
            (3, Kind::Added, Some("textures/d.ytd")),
        ]
    );
    assert_eq!(anorak.textures[0].lines, ["tint: none -> whi"]);

    let coat = review.garments.iter().find(|g| g.id == coat_id).unwrap();
    assert_eq!(coat.textures.len(), 1);
    assert_eq!(coat.textures[0].kind, Kind::Added);

    assert_eq!(review.settings.len(), 1, "{:?}", review.settings);
    assert!(
        review.settings[0].contains("collections"),
        "{:?}",
        review.settings
    );

    assert_eq!(
        review.summary(),
        "garments: 1 added, 2 changed, 1 removed; textures: 2 added, 1 changed; settings: 1 changed"
    );
    assert!(review.lines().contains(&"blazer changed".to_string()));

    // Read-only: nothing is written and no version is kept.
    assert_eq!(std::fs::read(at.join("wardrobe.json")).unwrap(), held);
    assert_eq!(versions(at).len(), kept);

    // A version against itself differs in nothing; a version against a later one is the reverse of the later against it.
    let same = of_versions(at, Some(first), Some(first)).unwrap();
    assert!(same.is_empty() && same.summary() == "The two versions state the same");
    let later = Wardrobe::read(at).unwrap();
    let earlier = or_wardrobe::backup::read_version(at, first).unwrap();
    let back = between(&later, &earlier);
    assert_eq!(
        back.garments
            .iter()
            .find(|g| g.id == coat_id)
            .map(|g| g.kind),
        Some(Kind::Removed)
    );
}

#[test]
fn a_version_that_is_not_kept_is_refused_with_a_sentence() {
    let dir = tempfile::tempdir().unwrap();
    Wardrobe::new("none").write(dir.path()).unwrap();
    let why = of_versions(Path::new(dir.path()), Some(9999), None).unwrap_err();
    assert!(why.to_string().contains("not kept"), "{why}");
}
