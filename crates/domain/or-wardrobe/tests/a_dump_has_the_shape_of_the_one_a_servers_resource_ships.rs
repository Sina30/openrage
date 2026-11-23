//! The clothes dump of a pack, held to the dump the resource of a server ships: the same objects and keys in the same
//! order, one entry for each texture, the ids the shop metadata states, and a place held listed once with no texture.
//!
//! The reference is a clothing resource somebody built and a server runs: it is not in the repository, so
//! `OPENRAGE_REFERENCE_PACK` names its folder, the one that holds `stream/` and `data/clothes_dump.json`. The suites
//! that read it skip without it, which is not a pass, and it is only read.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{garment, put_bytes, ydd, ytd, JBIB, LOWR, P_HEAD};
use or_wardrobe::dump::{dump, Dump};
use or_wardrobe::import::{import, Options};
use or_wardrobe::{allocate, inspect, plan, Depth, Garment, Ped, Wardrobe};
use serde_json::Value;

const WHAT: &str = "a_dump_of_the_reference_pack_has_the_shape_of_the_one_it_ships";

/// The keys of a ped in the dump, in order.
const PED_KEYS: [&str; 4] = [
    "DlcCollectionName",
    "PedName",
    "ComponentVariations",
    "Props",
];

/// The key after those four that says which place `DrawableId` is: ours, and no key of the reference's.
const MARKER: &str = "DrawableIdIsRelativeTo";

/// The keys of a component's entry, in order, as the reference writes them.
const COMPONENT_KEYS: [&str; 9] = [
    "DrawableName",
    "TextureName",
    "NameHash",
    "ComponentType",
    "ComponentId",
    "DrawableId",
    "TextureId",
    "RelativeCollectionDrawableId",
    "IsDummyPlaceholder",
];

/// The keys of a prop's entry: the reference spells the last one with a small `i`.
const PROP_KEYS: [&str; 9] = [
    "DrawableName",
    "TextureName",
    "NameHash",
    "AnchorPoint",
    "ComponentId",
    "DrawableId",
    "TextureId",
    "RelativeCollectionDrawableId",
    "isDummyPlaceholder",
];

/// The keys of the objects of `text`, by the number of spaces they are indented with, in the order they are written.
fn keys_by_indent(text: &str) -> BTreeMap<usize, Vec<String>> {
    let mut found: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let rest = line.trim_start();
        if let Some(rest) = rest.strip_prefix('"') {
            if let Some((key, _)) = rest.split_once("\": ") {
                found.entry(indent).or_default().push(key.to_string());
            }
        }
    }
    found
}

/// A pack of a male jacket with two textures, a male hat, and a female pair of trousers beside a place held.
fn pack(folder: &Path) -> Wardrobe {
    put_bytes(folder, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(folder, "models/hat.ydd", &ydd("hat"));
    put_bytes(folder, "models/trousers.ydd", &ydd("trousers"));
    for name in ["a", "b", "hat", "t"] {
        put_bytes(folder, &format!("textures/{name}.ytd"), &ytd(name));
    }
    let mut w = Wardrobe::new("demo");
    let jacket = garment(
        Ped::Male,
        JBIB,
        Some(0),
        "models/jacket.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    let hat = garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "models/hat.ydd",
        &["textures/hat.ytd"],
    );
    let trousers = garment(
        Ped::Female,
        LOWR,
        Some(0),
        "models/trousers.ydd",
        &["textures/t.ytd"],
    );
    let mut held = Garment::new();
    held.ped = Some(Ped::Female);
    held.slot = Some(LOWR);
    held.position = Some(1);
    w.garments.extend([jacket, hat, trousers, held]);
    w
}

fn made(w: &Wardrobe) -> Dump {
    dump(w, &allocate(w))
}

#[test]
fn the_dump_writes_the_objects_and_keys_of_the_reference_in_its_order() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let text = made(&w).json();

    assert!(
        text.starts_with("[\n  {\n    \"DlcCollectionName\": \"mp_m_demo\","),
        "{text}"
    );
    assert!(text.ends_with("  }\n]\n"), "{text}");
    let keys = keys_by_indent(&text);
    // A ped holds the reference's four keys and then the marker, at four spaces; an entry its nine, at eight.
    let ped_keys: Vec<&String> = keys[&4].iter().collect();
    let one: Vec<&str> = PED_KEYS.into_iter().chain([MARKER]).collect();
    assert_eq!(
        ped_keys,
        [one.clone(), one].concat().iter().collect::<Vec<_>>(),
        "two peds, each with the keys in order"
    );
    let entry_keys = &keys[&8];
    // The male has two component entries and one prop, the female two component entries (a texture and a place held).
    let expected: Vec<&str> = [
        COMPONENT_KEYS.as_slice(),
        COMPONENT_KEYS.as_slice(),
        PROP_KEYS.as_slice(),
        COMPONENT_KEYS.as_slice(),
        COMPONENT_KEYS.as_slice(),
    ]
    .concat();
    assert_eq!(
        entry_keys.iter().map(String::as_str).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn each_value_is_of_the_kind_the_reference_gives_it_and_says_what_the_pack_states() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let parsed: Value = serde_json::from_str(&made(&w).json()).expect("JSON");

    let male = &parsed[0];
    assert_eq!(male["PedName"], "mp_m_freemode_01");
    assert_eq!(male["DlcCollectionName"], "mp_m_demo");
    assert_eq!(
        male[MARKER], "DlcCollectionName",
        "the dump says in its own text that DrawableId counts from the start of the collection and not of the game's list"
    );
    let top = &male["ComponentVariations"][1];
    assert_eq!(top["DrawableName"], "Top");
    assert_eq!(top["ComponentType"], "PV_COMP_JBIB");
    assert_eq!(top["ComponentId"], 11);
    assert_eq!(top["TextureId"], 1);
    assert_eq!(top["RelativeCollectionDrawableId"], 0);
    assert_eq!(
        top["DrawableId"], top["RelativeCollectionDrawableId"],
        "the place in the collection, since a build cannot know how many drawables the game and the packs before it hold"
    );
    assert_eq!(top["IsDummyPlaceholder"], false);
    assert_eq!(
        top["TextureName"], "mp_m_freemode_01_mp_m_demo^jbib_diff_000_b_uni",
        "the dictionary's name, without its extension"
    );
    // The id is the one the shop metadata states for that texture.
    let facts = inspect(dir.path(), &w, Depth::Open);
    let planned = plan(&w, &facts, "pack", dir.path());
    let shop = &planned.documents.shops[&Ped::Male];
    let hash = top["NameHash"].as_str().unwrap();
    assert!(
        shop.contains(&format!("<uniqueNameHash>{hash}</uniqueNameHash>")),
        "{hash} in {shop}"
    );
    // A prop is under its anchor, and its flag is spelled as the reference spells it.
    let hat = &male["Props"][0];
    assert_eq!(hat["DrawableName"], "PropHead");
    assert_eq!(hat["AnchorPoint"], "ANCHOR_HEAD");
    assert_eq!(hat["ComponentId"], 0);
    assert_eq!(hat["isDummyPlaceholder"], false);
    assert!(hat.get("IsDummyPlaceholder").is_none());

    // A place held is listed once, with no texture.
    let female = &parsed[1];
    let held = &female["ComponentVariations"][1];
    assert_eq!(held["IsDummyPlaceholder"], true);
    assert_eq!(held["TextureName"], "");
    assert_eq!(held["RelativeCollectionDrawableId"], 1);
    assert_eq!(female["Props"].as_array().map(Vec::len), Some(0));
    assert!(
        made(&w).json().contains("\"Props\": []"),
        "an empty list is written as the reference writes it"
    );
}

#[test]
fn the_entries_of_a_ped_are_sorted_by_the_word_they_are_shown_under_and_the_dump_is_the_same_twice()
{
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let first = made(&w);
    assert_eq!(first, made(&w));
    assert_eq!(first.json(), made(&w).json());
    let words: Vec<&str> = first.peds[1]
        .components
        .iter()
        .map(|e| e.drawable_name.as_str())
        .collect();
    let mut sorted = words.clone();
    sorted.sort_unstable();
    assert_eq!(words, sorted);
}

#[test]
fn the_lua_table_says_what_the_json_says() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let dump = made(&w);
    let lua = dump.lua();
    assert!(
        lua.contains("return {") && lua.trim_end().ends_with('}'),
        "{lua}"
    );
    let opened = lua.matches('{').count();
    let closed = lua.matches('}').count();
    assert_eq!(opened, closed, "braces balance:\n{lua}");
    // Every key of the JSON is a key of the table, with the same value.
    for ped in &dump.peds {
        assert!(lua.contains(&format!("DlcCollectionName = \"{}\",", ped.collection)));
        assert!(
            lua.contains(&format!("{MARKER} = \"DlcCollectionName\",")),
            "{lua}"
        );
        for e in ped.components.iter().chain(&ped.props) {
            assert!(
                lua.contains(&format!("NameHash = \"{}\",", e.name_hash)),
                "{lua}"
            );
            assert!(lua.contains(&format!("TextureId = {},", e.texture_id)));
        }
    }
    assert!(lua.contains("IsDummyPlaceholder = true,"));
    assert!(lua.contains("isDummyPlaceholder = false,"));
    assert_eq!(
        lua.matches("NameHash = ").count(),
        dump.peds
            .iter()
            .map(|p| p.components.len() + p.props.len())
            .sum::<usize>()
    );
}

fn reference() -> Option<PathBuf> {
    let path = std::env::var_os("OPENRAGE_REFERENCE_PACK").map(PathBuf::from);
    match path.filter(|p| p.join("stream").is_dir() && p.join("data/clothes_dump.json").is_file()) {
        Some(path) => Some(path),
        None => {
            or_test_support::skip_corpus(
                WHAT,
                "OPENRAGE_REFERENCE_PACK names no clothing resource with a data/clothes_dump.json; it is a pack of the owner's server, in no repository",
            );
            None
        }
    }
}

#[test]
fn the_reference_writes_the_keys_the_dump_writes() {
    let Some(root) = reference() else {
        return;
    };
    let text = std::fs::read_to_string(root.join("data/clothes_dump.json")).unwrap();
    let keys = keys_by_indent(&text);
    assert!(
        keys[&4].chunks(4).all(|ped| ped == PED_KEYS),
        "the objects of a ped"
    );
    let entries = &keys[&8];
    // Each entry is one of the two shapes, and both shapes occur in a pack with props.
    let mut shapes = [0usize; 2];
    for entry in entries.chunks(9) {
        match entry {
            e if e == COMPONENT_KEYS => shapes[0] += 1,
            e if e == PROP_KEYS => shapes[1] += 1,
            other => panic!("an entry of another shape: {other:?}"),
        }
    }
    assert!(shapes[0] > 0, "{shapes:?}");
}

#[test]
fn the_dump_of_the_reference_pack_says_what_the_reference_says_of_the_places_it_has() {
    let Some(root) = reference() else {
        return;
    };
    let shipped: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("data/clothes_dump.json")).unwrap(),
    )
    .unwrap();
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    let ours = dump(&imported.wardrobe, &planned.allocation);

    // The key of an entry in either dump: the ped, the slot it is under, the place in the collection and the texture.
    type Key = (String, String, u64, u64);
    // What an entry says besides: the word it is shown under, its component and whether it is a place held.
    type Said = (String, u64, bool);
    let mut theirs: BTreeMap<Key, Said> = BTreeMap::new();
    for ped in shipped.as_array().unwrap() {
        let name = ped["PedName"].as_str().unwrap().to_string();
        for (list, kind) in [
            ("ComponentVariations", "ComponentType"),
            ("Props", "AnchorPoint"),
        ] {
            for e in ped[list].as_array().unwrap() {
                let flag = e
                    .get("IsDummyPlaceholder")
                    .or_else(|| e.get("isDummyPlaceholder"));
                theirs.insert(
                    (
                        name.clone(),
                        e[kind].as_str().unwrap().to_string(),
                        e["RelativeCollectionDrawableId"].as_u64().unwrap(),
                        e["TextureId"].as_u64().unwrap(),
                    ),
                    (
                        e["DrawableName"].as_str().unwrap().to_string(),
                        e["ComponentId"].as_u64().unwrap(),
                        flag.and_then(Value::as_bool).unwrap_or(false),
                    ),
                );
            }
        }
    }
    let (mut same, mut differ, mut missing) = (0usize, Vec::new(), Vec::new());
    let mut seen = 0usize;
    let mut ours_by_key: BTreeMap<Key, Said> = BTreeMap::new();
    for ped in &ours.peds {
        for e in ped.components.iter().chain(&ped.props) {
            seen += 1;
            let key = (
                ped.ped.model().to_string(),
                e.kind.clone(),
                u64::from(e.relative),
                u64::from(e.texture_id),
            );
            let said = (e.drawable_name.clone(), u64::from(e.component_id), e.dummy);
            ours_by_key.insert(key.clone(), said.clone());
            match theirs.get(&key) {
                Some(their) if *their == said => same += 1,
                Some(their) => differ.push(format!("{key:?}: ours {said:?} theirs {their:?}")),
                None => missing.push(format!("{key:?}")),
            }
        }
    }
    eprintln!(
        "{seen} entries of the dump, {same} say what the reference says of the place, {} say another, {} are not in the reference's {}",
        differ.len(),
        missing.len(),
        theirs.len()
    );
    eprintln!("another: {:?}", &differ[..differ.len().min(6)]);
    eprintln!("absent: {:?}", &missing[..missing.len().min(6)]);
    // Every place the reference lists is a place of the dump, with the component and the flag the reference gives it.
    let unlisted: Vec<&Key> = theirs
        .keys()
        .filter(|k| !ours_by_key.contains_key(*k))
        .collect();
    assert!(
        unlisted.is_empty(),
        "{} places of the reference are not in the dump: {:?}",
        unlisted.len(),
        &unlisted[..unlisted.len().min(5)]
    );
    let other_component = theirs
        .iter()
        .filter(|(k, their)| {
            ours_by_key
                .get(*k)
                .is_some_and(|ours| ours.1 != their.1 || ours.2 != their.2)
        })
        .count();
    assert_eq!(
        other_component, 0,
        "a place is under another component or is held in one dump and not the other"
    );
    // The word an entry is shown under is the slot's; the reference's own differs for the few entries it took from a source named for another slot.
    assert!(
        differ.len() * 100 <= theirs.len(),
        "{} of {} entries are under another word: {:?}",
        differ.len(),
        theirs.len(),
        &differ[..differ.len().min(5)]
    );
    assert!(
        same > 0,
        "no entry of the dump is an entry of the reference"
    );
}
