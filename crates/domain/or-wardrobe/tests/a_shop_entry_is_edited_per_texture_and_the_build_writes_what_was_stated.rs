//! The shop entry of a texture is edited: its price, its restriction tags, its forced components, and any other element kept as
//! written. Each is an edit with Undo, an entry no file stated is made by the first edit and dropped by the last, and the shop
//! metadata a build writes reads back with exactly what was stated.
//!
//! No renderer, no install and no corpus; the suite that measures the staged shop files needs `OPENRAGE_REFERENCE_PACK`.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, FEET, JBIB, P_HEAD};
use or_wardrobe::edit::{self, Edit};
use or_wardrobe::shop::{self, ShopFile};
use or_wardrobe::shop_edit::{self, Field, Forced};
use or_wardrobe::{plan, Component, Ped, ShopEntry, Wardrobe};

fn pack() -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(0),
        "jacket.ydd",
        &["jacket_a.ytd", "jacket_b.ytd"],
    ));
    w.garments.push(garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "hat.ydd",
        &["hat_a.ytd"],
    ));
    w
}

/// The shop file the plan writes for the male ped, read back.
fn written(w: &Wardrobe) -> ShopFile {
    let planned = plan(
        w,
        &or_wardrobe::Facts::default(),
        "wardrobe",
        Path::new("."),
    );
    let text = planned
        .documents
        .shops
        .get(&Ped::Male)
        .expect("the plan writes the male shop file");
    shop::parse(text, "the built shop file").expect("the shop file reads back")
}

fn entry(file: &ShopFile, prop: bool, drawable: u32, texture: u32) -> &ShopEntry {
    &file
        .entries
        .iter()
        .find(|r| r.prop == prop && r.drawable == drawable && r.texture == texture)
        .unwrap_or_else(|| panic!("no entry for {prop} {drawable} {texture}"))
        .entry
}

fn set(w: &mut Wardrobe, at: usize, texture: usize, field: Field) {
    let id = w.garments[at].id.clone();
    edit::apply(w, &id, Edit::Shop(texture, field)).expect("the edit is made");
}

fn shop_of(w: &Wardrobe, at: usize, texture: usize) -> Option<&ShopEntry> {
    w.garments[at].variations[texture].shop.as_ref()
}

#[test]
fn a_price_is_written_in_the_entry_of_its_texture_and_in_no_other() {
    let mut w = pack();
    set(&mut w, 0, 1, Field::Cost(Some(" 250 ".into())));

    let file = written(&w);
    assert_eq!(entry(&file, false, 0, 1).cost.as_deref(), Some("250"));
    assert_eq!(
        entry(&file, false, 0, 0).cost.as_deref(),
        Some("0"),
        "the other texture of the jacket keeps the 0 the build writes"
    );
    // The entry was made by the edit: it states no id, so the build names it as it names every entry.
    let made = shop_of(&w, 0, 1).expect("the edit made the entry");
    assert!(made.id.is_empty() && made.from.is_empty());
    assert!(!entry(&file, false, 0, 1).id.is_empty());

    // Taking the price away leaves nothing the entry states, so the entry goes.
    set(&mut w, 0, 1, Field::Cost(None));
    assert!(shop_of(&w, 0, 1).is_none(), "{:?}", shop_of(&w, 0, 1));
    assert_eq!(entry(&written(&w), false, 0, 1).cost.as_deref(), Some("0"));
}

#[test]
fn a_price_that_is_not_a_whole_number_is_refused_and_changes_nothing() {
    let mut w = pack();
    set(&mut w, 0, 0, Field::Cost(Some("100".into())));
    let before = w.clone();
    let id = w.garments[0].id.clone();
    for bad in ["abc", "-5", "1.5", "99999999999", "1 000", "0x10"] {
        let why = edit::apply(&mut w, &id, Edit::Shop(0, Field::Cost(Some(bad.into()))))
            .unwrap_err()
            .to_string();
        assert!(why.contains("whole number"), "{bad}: {why}");
    }
    let why = edit::apply(&mut w, &id, Edit::Shop(9, Field::Cost(Some("1".into()))))
        .unwrap_err()
        .to_string();
    assert!(why.contains("2 texture(s)"), "{why}");
    assert_eq!(w, before);
}

#[test]
fn restriction_tags_are_written_in_the_order_given_and_a_prop_writes_them_before_its_place() {
    let mut w = pack();
    set(
        &mut w,
        1,
        0,
        Field::Tags(vec!["HAT".into(), " BERET ".into()]),
    );

    let file = written(&w);
    let hat = entry(&file, true, 0, 0);
    assert_eq!(shop_edit::tags(hat), ["HAT", "BERET"]);

    // The text itself: for a prop the tags follow the id, as the staged files write them.
    let planned = plan(
        &w,
        &or_wardrobe::Facts::default(),
        "wardrobe",
        Path::new("."),
    );
    let text = &planned.documents.shops[&Ped::Male];
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("no {needle} in {text}"))
    };
    assert!(at("<restrictionTags>") > at("<uniqueNameHash>"));
    assert!(at("<restrictionTags>") < at("<localPropIndex"), "{text}");

    set(&mut w, 1, 0, Field::Tags(vec![]));
    assert!(shop_of(&w, 1, 0).is_none());
    assert!(
        written(&w)
            .entries
            .iter()
            .all(|r| shop_edit::tags(&r.entry).is_empty()),
        "no tag is written once they are all taken away"
    );

    let id = w.garments[1].id.clone();
    let why = edit::apply(
        &mut w,
        &id,
        Edit::Shop(0, Field::Tags(vec!["two\nlines".into()])),
    )
    .unwrap_err()
    .to_string();
    assert!(why.contains("one line"), "{why}");
}

#[test]
fn forced_components_are_stated_as_the_staged_files_state_them() {
    let mut w = pack();
    let forced = vec![
        Forced {
            name_hash: "0".into(),
            enum_value: 14,
            component: Component::Uppr,
        },
        Forced {
            name_hash: "DLC_MP_X22_M_TORSO_0_0".into(),
            enum_value: 1,
            component: Component::Hand,
        },
    ];
    set(&mut w, 0, 0, Field::Forced(forced.clone()));

    let file = written(&w);
    let read = entry(&file, false, 0, 0);
    assert_eq!(shop_edit::forced(read), Some(forced));
    assert_eq!(
        read.fields,
        [
            "<forcedComponents><Item><nameHash>0</nameHash><enumValue value=\"14\"/><eCompType>PV_COMP_UPPR</eCompType></Item><Item><nameHash>DLC_MP_X22_M_TORSO_0_0</nameHash><enumValue value=\"1\"/><eCompType>PV_COMP_HAND</eCompType></Item></forcedComponents>"
        ]
    );

    set(&mut w, 0, 0, Field::Forced(vec![]));
    assert!(shop_of(&w, 0, 0).is_none());
}

#[test]
fn an_element_no_staged_file_states_is_kept_as_written_and_edited_as_xml() {
    let mut w = pack();
    set(
        &mut w,
        0,
        0,
        Field::Raw(
            "lockHash".into(),
            Some("<lockHash>  LOCK_ME </lockHash>".into()),
        ),
    );
    set(
        &mut w,
        0,
        0,
        Field::Raw(
            "variantComponents".into(),
            Some(
                "<variantComponents><Item><nameHash>0</nameHash></Item></variantComponents>".into(),
            ),
        ),
    );

    let file = written(&w);
    let read = entry(&file, false, 0, 0);
    let others = shop_edit::others(read);
    let names: Vec<&str> = others.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["lockHash", "variantComponents"]);
    assert_eq!(others[0].1, "<lockHash>LOCK_ME</lockHash>");

    // Stating an element again replaces it in its place; none takes it away.
    set(
        &mut w,
        0,
        0,
        Field::Raw("lockHash".into(), Some("<lockHash>OTHER</lockHash>".into())),
    );
    let fields = &shop_of(&w, 0, 0).unwrap().fields;
    assert_eq!(fields[0], "<lockHash>OTHER</lockHash>");
    assert_eq!(fields.len(), 2);
    set(&mut w, 0, 0, Field::Raw("lockHash".into(), None));
    set(&mut w, 0, 0, Field::Raw("variantComponents".into(), None));
    assert!(shop_of(&w, 0, 0).is_none());
}

#[test]
fn a_raw_element_must_be_one_element_of_the_name_given_and_not_one_the_id_or_a_typed_edit_owns() {
    let mut w = pack();
    let id = w.garments[0].id.clone();
    let mut refuse = |name: &str, xml: Option<&str>, says: &str| {
        let why = edit::apply(
            &mut w,
            &id,
            Edit::Shop(0, Field::Raw(name.into(), xml.map(String::from))),
        )
        .unwrap_err()
        .to_string();
        assert!(why.contains(says), "{name}: {why}");
    };
    refuse("lockHash", Some("<lockHash>unclosed"), "not one element");
    refuse(
        "lockHash",
        Some("<other>x</other>"),
        "should be called `lockHash`",
    );
    refuse("two words", Some("<a/>"), "not the name of an element");
    refuse("cost", Some("<cost value=\"1\"/>"), "id and the place");
    refuse(
        "uniqueNameHash",
        Some("<uniqueNameHash>X</uniqueNameHash>"),
        "id and the place",
    );
    refuse("restrictionTags", Some("<restrictionTags/>"), "own edit");
    refuse("forcedComponents", None, "own edit");
}

#[test]
fn an_entry_a_file_stated_keeps_its_id_and_its_place_in_the_file_whatever_is_edited() {
    let mut w = pack();
    w.garments[0].variations[0].shop = Some(ShopEntry {
        id: "CLO_JACKET_0".into(),
        from: "shop.meta".into(),
        entry: 7,
        cost: Some("100".into()),
        fields: vec![
            "<forcedComponents><Item><nameHash>0</nameHash><enumValue value=\"3\"/><eCompType>PV_COMP_LOWR</eCompType></Item></forcedComponents>".into(),
        ],
    });
    set(&mut w, 0, 0, Field::Tags(vec!["HAT".into()]));
    set(&mut w, 0, 0, Field::Cost(None));

    let kept = shop_of(&w, 0, 0).expect("the entry the file stated stays");
    assert_eq!(
        (kept.id.as_str(), kept.from.as_str(), kept.entry),
        ("CLO_JACKET_0", "shop.meta", 7)
    );
    assert_eq!(kept.cost, None);
    assert_eq!(
        kept.fields.len(),
        2,
        "the forced component is kept, the tags follow it"
    );
    assert!(kept.fields[0].starts_with("<forcedComponents>"));
    assert_eq!(
        entry(&written(&w), false, 0, 0).id,
        "CLO_JACKET_0",
        "the id the file stated is the one written"
    );
    // Emptying the typed fields does not drop an entry that still has an id.
    set(&mut w, 0, 0, Field::Forced(vec![]));
    set(&mut w, 0, 0, Field::Tags(vec![]));
    assert!(shop_of(&w, 0, 0).is_some());
}

#[test]
fn forced_components_of_a_shape_this_does_not_name_are_kept_and_refuse_a_typed_edit() {
    let mut w = pack();
    let odd = "<forcedComponents><Item><nameHash>0</nameHash><enumValue value=\"3\"/><eCompType>PV_COMP_NOPE</eCompType></Item></forcedComponents>";
    w.garments[0].variations[0].shop = Some(ShopEntry {
        fields: vec![odd.into()],
        ..ShopEntry::default()
    });
    assert_eq!(shop_edit::forced(shop_of(&w, 0, 0).unwrap()), None);
    let id = w.garments[0].id.clone();
    let before = w.clone();
    let why = edit::apply(&mut w, &id, Edit::Shop(0, Field::Forced(vec![])))
        .unwrap_err()
        .to_string();
    assert!(why.contains("edit them as XML"), "{why}");
    assert_eq!(w, before, "what it states is kept as written");

    // They are listed with the other elements kept as written, and edited as XML.
    let others = shop_edit::others(shop_of(&w, 0, 0).unwrap());
    assert_eq!(others.len(), 1, "{others:?}");
    assert_eq!(others[0].0, "forcedComponents");
    set(
        &mut w,
        0,
        0,
        Field::Raw(
            "forcedComponents".into(),
            Some("<forcedComponents/>".into()),
        ),
    );
    assert_eq!(shop_edit::forced(shop_of(&w, 0, 0).unwrap()), Some(vec![]));
    set(&mut w, 0, 0, Field::Forced(vec![]));
    assert!(shop_of(&w, 0, 0).is_none());
}

#[test]
fn every_edit_of_an_entry_is_one_step_that_undo_takes_back_whole() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("clothing");
    edit::create(&module, "demo").unwrap();
    let mut w = Wardrobe::read(&module).unwrap();
    let mut jacket = garment(Ped::Male, FEET, Some(0), "feet.ydd", &["feet_a.ytd"]);
    jacket.id = "boots".into();
    w.garments.push(jacket);
    w.write(&module).unwrap();

    let ((), revision) = edit::revise(&module, |w| {
        edit::apply(w, "boots", Edit::Shop(0, Field::Cost(Some("75".into()))))
    })
    .unwrap();
    let revision = revision.expect("a price is a change");
    let read = Wardrobe::read(&module).unwrap();
    assert_eq!(
        read.garments[0].variations[0]
            .shop
            .as_ref()
            .and_then(|e| e.cost.as_deref()),
        Some("75"),
        "it reads back from the pack's own file"
    );
    edit::undo_revision(&module, &revision).unwrap();
    assert!(Wardrobe::read(&module).unwrap().garments[0].variations[0]
        .shop
        .is_none());

    // The same price twice is no change, and so no step.
    edit::update(&module, |w| {
        edit::apply(w, "boots", Edit::Shop(0, Field::Cost(Some("75".into()))))
    })
    .unwrap();
    let ((), again) = edit::revise(&module, |w| {
        edit::apply(w, "boots", Edit::Shop(0, Field::Cost(Some("75".into()))))
    })
    .unwrap();
    assert!(again.is_none());
}

fn reference() -> Option<PathBuf> {
    let path = std::env::var_os("OPENRAGE_REFERENCE_PACK").map(PathBuf::from);
    match path.filter(|p| p.join("data").is_dir()) {
        Some(path) => Some(path),
        None => {
            or_test_support::skip_corpus(
                "the_staged_shop_files_state_only_the_elements_the_editor_names",
                "OPENRAGE_REFERENCE_PACK names no clothing resource; it is a pack of the owner's server, in no repository",
            );
            None
        }
    }
}

#[test]
fn the_staged_shop_files_state_only_the_elements_the_editor_names() {
    let Some(root) = reference() else {
        return;
    };
    let (mut entries, mut forced_items, mut tagged) = (0usize, 0usize, 0usize);
    let mut others = std::collections::BTreeSet::new();
    for file in std::fs::read_dir(root.join("data")).unwrap().flatten() {
        let Ok(bytes) = std::fs::read(file.path()) else {
            continue;
        };
        if !shop::looks_like_shop(&bytes) {
            continue;
        }
        let parsed = shop::parse(&String::from_utf8_lossy(&bytes), "staged").unwrap();
        for raw in &parsed.entries {
            entries += 1;
            others.extend(
                shop_edit::others(&raw.entry)
                    .into_iter()
                    .map(|(name, _)| name),
            );
            let forced = shop_edit::forced(&raw.entry).unwrap_or_else(|| {
                panic!(
                    "an entry states forced components this does not name: {:?}",
                    raw.entry.fields
                )
            });
            let tags = shop_edit::tags(&raw.entry);
            forced_items += forced.len();
            tagged += usize::from(!tags.is_empty());

            // Stating again what the file states writes it back as it was, in its place.
            let mut w = Wardrobe::new("x");
            let mut g = garment(Ped::Male, JBIB, Some(0), "x.ydd", &["x.ytd"]);
            g.variations[0].shop = Some(ShopEntry {
                fields: raw.entry.fields.clone(),
                ..ShopEntry::default()
            });
            let id = g.id.clone();
            w.garments.push(g);
            if !forced.is_empty() {
                edit::apply(&mut w, &id, Edit::Shop(0, Field::Forced(forced))).unwrap();
            }
            if !tags.is_empty() {
                edit::apply(&mut w, &id, Edit::Shop(0, Field::Tags(tags))).unwrap();
            }
            let now = w.garments[0].variations[0].shop.as_ref().unwrap();
            assert_eq!(
                now.fields, raw.entry.fields,
                "entry {} of {}",
                raw.entry.entry, raw.entry.from
            );
        }
    }
    assert!(entries > 1000, "only {entries} entries were read");
    assert!(forced_items > 0 && tagged > 0);
    assert!(
        others.is_empty(),
        "the staged files state elements this has no typed edit for: {others:?}"
    );
}
