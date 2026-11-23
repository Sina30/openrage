//! Fix and Waive over the rules: a remedy says what it changes, changes that and clears the finding
//! it was offered for, and a waiver is recorded only for a rule a reason can mend, with a reason.

use or_wardrobe::edit::{apply, remedy, waive, Edit};
use or_wardrobe::{
    allocate, check, facts::named_paths, Component, Facts, Finding, Garment, Ped, Prop, Skin, Slot,
    Variation, Waiver, Wardrobe,
};

const JBIB: Slot = Slot::Component(Component::Jbib);

fn garment(model: &str, position: Option<u32>) -> Garment {
    let mut g = Garment::new();
    g.ped = Some(Ped::Male);
    g.slot = Some(JBIB);
    g.position = position;
    g.model = Some(model.to_string());
    g.variations = vec![Variation {
        texture: Some(format!("{model}.ytd")),
        ..Default::default()
    }];
    g
}

fn found(w: &Wardrobe, facts: &Facts) -> Vec<Finding> {
    check(w, &allocate(w), facts)
}

fn everything_there(w: &Wardrobe) -> Facts {
    Facts::from_files(named_paths(w).map(|p| (p, 10, p)))
}

fn with_code<'a>(findings: &'a [Finding], code: &str) -> Vec<&'a Finding> {
    findings.iter().filter(|f| f.code == code).collect()
}

/// The remedy of the only finding with `code`, applied: what it said it changes, and what the pack then finds.
fn mended(w: &mut Wardrobe, facts: &Facts, code: &str) -> (String, Vec<Finding>) {
    let findings = found(w, facts);
    let finding = (*with_code(&findings, code)
        .first()
        .unwrap_or_else(|| panic!("no {code} in {findings:?}")))
    .clone();
    let fix = remedy(w, &finding).unwrap_or_else(|| panic!("no remedy for {code}"));
    fix.apply(w).unwrap();
    (fix.what, found(w, facts))
}

#[test]
fn two_pins_at_one_place_and_a_pin_past_the_end_are_mended_by_unpinning_the_one_refused() {
    let mut w = Wardrobe::new("p");
    w.garments = vec![garment("a.ydd", Some(4)), garment("b.ydd", Some(4))];
    let facts = everything_there(&w);
    let (what, after) = mended(&mut w, &facts, "position-clash");
    assert!(what.contains("Unpin") && what.contains("004"), "{what}");
    assert!(after.is_empty(), "{after:?}");
    assert_eq!(w.garments[0].position, Some(4), "the first keeps its place");
    assert_eq!(w.garments[1].position, None);

    let mut w = Wardrobe::new("p");
    w.garments = vec![garment("a.ydd", Some(9000))];
    let facts = everything_there(&w);
    let (what, after) = mended(&mut w, &facts, "slot-full");
    assert!(what.contains("9000"), "{what}");
    assert!(after.is_empty(), "{after:?}");
}

#[test]
fn a_duplicate_is_mended_by_removing_the_later_garment_and_its_files_are_not_touched() {
    let mut w = Wardrobe::new("p");
    w.garments = vec![garment("a.ydd", None), garment("copy.ydd", None)];
    let facts = Facts::from_files([
        ("a.ydd", 10, "same"),
        ("copy.ydd", 10, "same"),
        ("a.ydd.ytd", 10, "t1"),
        ("copy.ydd.ytd", 10, "t1"),
    ]);
    let kept = w.garments[0].id.clone();
    let (what, after) = mended(&mut w, &facts, "duplicate-content");
    assert!(
        what.contains("Remove") && what.contains("files are not touched"),
        "{what}"
    );
    assert!(after.is_empty(), "{after:?}");
    assert_eq!(
        w.garments.iter().map(|g| &g.id).collect::<Vec<_>>(),
        [&kept]
    );
}

#[test]
fn cloth_that_cannot_ship_is_mended_by_saying_what_is_true() {
    // A garment whose metadata states ownsCloth and that has no cloth linked: the pack stops expecting one.
    let mut w = Wardrobe::new("p");
    let mut g = garment("a.ydd", None);
    g.missing_cloth = true;
    w.garments = vec![g];
    let facts = everything_there(&w);
    let (what, after) = mended(&mut w, &facts, "cloth-missing");
    assert!(what.contains("has no cloth"), "{what}");
    assert!(after.is_empty(), "{after:?}");
    assert!(!w.garments[0].missing_cloth);

    // A prop that names a cloth: the name goes.
    let mut w = Wardrobe::new("p");
    let mut prop = garment("a.ydd", None);
    prop.slot = Some(Slot::Prop(Prop::Head));
    prop.cloth = Some("a.yld".into());
    w.garments = vec![prop];
    let facts = everything_there(&w);
    let (what, after) = mended(&mut w, &facts, "cloth-ignored");
    assert!(what.contains("Stop naming the cloth"), "{what}");
    assert!(after.is_empty(), "{after:?}");
    assert_eq!(w.garments[0].cloth, None);
}

#[test]
fn a_waiver_that_gives_no_reason_or_names_no_rule_is_mended_by_removing_it() {
    for (code, reason, wanted) in [
        ("duplicate-content", " ", "waiver-without-reason"),
        ("no-such-rule", "x", "waiver-unknown"),
    ] {
        let mut w = Wardrobe::new("p");
        w.garments = vec![garment("a.ydd", None)];
        w.waivers = vec![
            Waiver {
                code: "texture-count-wraps".into(),
                garment: None,
                reason: "kept".into(),
            },
            Waiver {
                code: code.into(),
                garment: None,
                reason: reason.into(),
            },
        ];
        let facts = everything_there(&w);
        let (what, after) = mended(&mut w, &facts, wanted);
        // The waiver is named by what its rule says, or by the word it gave where no rule has it; never by a bare code.
        let named = match or_wardrobe::rule_of(code) {
            Some(rule) => rule.summary.to_string(),
            None => code.to_string(),
        };
        assert!(what.starts_with("Remove the waiver of"), "{what}");
        assert!(what.contains(&named) && !what.contains('`'), "{what}");
        assert!(after.is_empty(), "{after:?}");
        assert_eq!(w.waivers.len(), 1, "only the waiver that was wrong goes");
        assert_eq!(w.waivers[0].code, "texture-count-wraps");
    }
}

#[test]
fn a_finding_that_needs_a_choice_has_no_remedy_and_a_stale_one_changes_nothing() {
    let mut w = Wardrobe::new("p");
    let mut nobody = garment("a.ydd", None);
    nobody.ped = None;
    w.garments = vec![nobody];
    let facts = everything_there(&w);
    let findings = found(&w, &facts);
    assert_eq!(with_code(&findings, "garment-unplaced").len(), 1);
    assert!(
        findings.iter().all(|f| remedy(&w, f).is_none()),
        "nobody is given a ped"
    );

    // A remedy made for a garment that has since gone is not applied.
    let mut w = Wardrobe::new("p");
    w.garments = vec![garment("a.ydd", Some(4)), garment("b.ydd", Some(4))];
    let facts = everything_there(&w);
    let findings = found(&w, &facts);
    let clash = with_code(&findings, "position-clash")[0].clone();
    let fix = remedy(&w, &clash).unwrap();
    let gone = w.garments[1].id.clone();
    apply(&mut w, &gone, Edit::Remove).unwrap();
    let before = w.clone();
    assert!(fix.apply(&mut w).is_err());
    assert_eq!(w, before);
    assert!(remedy(&w, &clash).is_none(), "the pack no longer shows it");
}

#[test]
fn a_waiver_is_recorded_for_a_rule_a_reason_can_mend_with_a_reason_and_replaced_not_doubled() {
    let mut w = Wardrobe::new("p");
    w.garments = vec![garment("a.ydd", None), garment("copy.ydd", None)];
    let id = w.garments[1].id.clone();
    let facts = Facts::from_files([
        ("a.ydd", 10, "same"),
        ("copy.ydd", 10, "same"),
        ("a.ydd.ytd", 10, "t1"),
        ("copy.ydd.ytd", 10, "t1"),
    ]);

    for (code, garment, reason, why) in [
        ("model-missing", None, "x", "cannot be waived"),
        ("no-such-rule", None, "x", "is no rule"),
        ("duplicate-content", None, "  ", "needs a reason"),
        ("duplicate-content", Some("nobody"), "x", "no garment"),
    ] {
        let error = waive(&mut w, code, garment, reason)
            .unwrap_err()
            .to_string();
        assert!(error.contains(why), "{error}");
    }
    assert!(w.waivers.is_empty(), "a refusal records nothing");

    waive(
        &mut w,
        "duplicate-content",
        Some(&id),
        "the same mesh on purpose",
    )
    .unwrap();
    waive(&mut w, "duplicate-content", Some(&id), "  a better reason ").unwrap();
    assert_eq!(w.waivers.len(), 1);
    assert_eq!(w.waivers[0].reason, "a better reason");
    let findings = found(&w, &facts);
    assert_eq!(
        with_code(&findings, "duplicate-content")[0]
            .waived
            .as_deref(),
        Some("a better reason")
    );
}

#[test]
fn what_a_person_says_of_a_garment_is_set_and_an_unknown_garment_is_refused() {
    let mut w = Wardrobe::new("p");
    w.garments = vec![garment("a.ydd", Some(2))];
    let id = w.garments[0].id.clone();
    apply(&mut w, &id, Edit::Label(Some("  ".into()))).unwrap();
    assert_eq!(w.garments[0].label, None, "a blank label is none");
    apply(&mut w, &id, Edit::Label(Some("jacket".into()))).unwrap();
    apply(&mut w, &id, Edit::Ped(Some(Ped::Female))).unwrap();
    apply(
        &mut w,
        &id,
        Edit::Slot(Some(Slot::Component(Component::Lowr))),
    )
    .unwrap();
    apply(&mut w, &id, Edit::Position(None)).unwrap();
    apply(&mut w, &id, Edit::Skin(Skin::Race)).unwrap();
    let g = &w.garments[0];
    assert_eq!(
        (g.label.as_deref(), g.ped, g.slot, g.position, g.skin),
        (
            Some("jacket"),
            Some(Ped::Female),
            Some(Slot::Component(Component::Lowr)),
            None,
            Skin::Race
        )
    );
    assert!(apply(&mut w, "nobody", Edit::Remove).is_err());
    apply(&mut w, &id, Edit::Remove).unwrap();
    assert!(w.garments.is_empty());
}
