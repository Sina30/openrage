//! A source states who made it and on what terms in its `asset.json`; a build that asks for credits writes a file that lists each
//! source with its author, licence and link, and lists apart the sources that state none.

mod common;

use std::path::Path;

use common::{drawables_source, garment, put_bytes, ytd, JBIB, LOWR};
use or_project::{Origin, OutputKind};
use or_source::asset::{set_credit, Credit, Source};
use or_wardrobe::credits::{credited, entries, markdown, state_for, FILE};
use or_wardrobe::layout::Paths;
use or_wardrobe::options::Options;
use or_wardrobe::plan::Build;
use or_wardrobe::{inspect, plan_for, Depth, Ped, Planned, Wardrobe};

/// A jacket and trousers whose models are sources of the pack, and a pair of shoes whose model is a compiled file.
fn pack(folder: &Path) -> Wardrobe {
    drawables_source(&folder.join("models/jacket.drawables.src"), "jacket");
    drawables_source(&folder.join("models/trousers.drawables.src"), "trousers");
    put_bytes(folder, "models/shoes.ydd", &common::ydd("shoes"));
    put_bytes(folder, "textures/a.ytd", &ytd("a"));
    let mut w = Wardrobe::new("credited");
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        None,
        "models/jacket.drawables.src",
        &["textures/a.ytd"],
    );
    jacket.label = Some("Wool jacket".into());
    let mut trousers = garment(Ped::Male, LOWR, None, "models/trousers.drawables.src", &[]);
    trousers.label = Some("Chinos".into());
    let mut shoes = garment(Ped::Male, common::FEET, None, "models/shoes.ydd", &[]);
    shoes.label = Some("Shoes".into());
    w.garments = vec![jacket, trousers, shoes];
    w.write(folder).unwrap();
    w
}

fn planned(folder: &Path, w: &Wardrobe, options: Options, kind: OutputKind) -> Planned {
    let build = Build {
        paths: match kind {
            OutputKind::FivemResource => Paths::resource(&w.pack, true),
            _ => Paths::pack(&w.pack),
        },
        options,
        kind,
    };
    plan_for(w, &inspect(folder, w, Depth::Open), "pack", folder, &build)
}

fn document(p: &Planned, path: &str) -> Option<String> {
    p.contribution.files.iter().find_map(|f| match &f.origin {
        Origin::Document(text) if f.path == path => Some(text.clone()),
        _ => None,
    })
}

#[test]
fn a_source_states_its_credit_in_its_asset_json_and_a_recook_has_nothing_to_do_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);
    let source = at.join("models/jacket.drawables.src");
    let before = Source::open(&source).unwrap().manifest.digest_text();

    let credit = Credit {
        author: "Jane Doe".into(),
        url: "https://example.org/jacket".into(),
        licence: "CC-BY-4.0".into(),
    };
    let stated = state_for(at, &w, &w.garments[0].id, &credit).unwrap();
    assert_eq!(
        stated.set,
        ["jacket"],
        "the jacket's own model; the texture is a compiled file"
    );
    assert!(stated.left.is_empty());

    let text = std::fs::read_to_string(source.join("asset.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["credit"]["author"], "Jane Doe");
    assert_eq!(json["credit"]["licence"], "CC-BY-4.0");
    assert_eq!(json["credit"]["url"], "https://example.org/jacket");
    let after = Source::open(&source).unwrap();
    assert_eq!(after.manifest.credit.as_ref(), Some(&credit));
    assert_eq!(
        after.manifest.digest_text(),
        before,
        "nothing compiles from a credit, so it is no change of the source"
    );

    // An empty credit takes the statement away, and the key with it.
    set_credit(&source, Credit::default()).unwrap();
    assert!(!std::fs::read_to_string(source.join("asset.json"))
        .unwrap()
        .contains("credit"));
}

#[test]
fn a_build_that_asks_for_credits_writes_a_file_that_lists_the_sources_with_their_licence() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);
    let credit = Credit {
        author: "Jane | Doe".into(),
        url: "https://example.org/jacket".into(),
        licence: "CC-BY-4.0".into(),
    };
    state_for(at, &w, &w.garments[0].id, &credit).unwrap();

    let asked = Options {
        credits: true,
        ..Options::default()
    };
    let p = planned(at, &w, asked.clone(), OutputKind::FivemResource);
    let text = document(&p, FILE).expect("the build writes the credits");
    let row = text
        .lines()
        .find(|l| l.contains("jacket"))
        .unwrap_or_else(|| panic!("the jacket is listed: {text}"));
    for said in [
        "Wool jacket",
        "Jane \\| Doe",
        "CC-BY-4.0",
        "https://example.org/jacket",
        "drawables",
    ] {
        assert!(row.contains(said), "{said} in {row}");
    }
    let (credited_part, without) = text
        .split_once("## Without a credit")
        .expect("a section for the rest");
    assert!(!credited_part.contains("Chinos"), "{text}");
    assert!(
        without.contains("trousers") && without.contains("Chinos"),
        "{text}"
    );
    assert!(
        !text.contains("shoes"),
        "a compiled file has no credit to list: {text}"
    );
    assert!(
        p.contribution
            .notes
            .iter()
            .any(|n| n.contains("1 of the 2 sources")),
        "{:?}",
        p.contribution.notes
    );

    // The same pack, the same text; and a build that does not ask writes none.
    assert_eq!(
        document(
            &planned(at, &w, asked.clone(), OutputKind::FivemResource),
            FILE
        ),
        Some(text)
    );
    assert_eq!(
        document(
            &planned(at, &w, Options::default(), OutputKind::FivemResource),
            FILE
        ),
        None
    );

    // A pack registers only files the game reads, so it leaves the file out and says so.
    let pack_plan = planned(at, &w, asked, OutputKind::SingleplayerDlc);
    assert_eq!(document(&pack_plan, FILE), None);
    assert!(pack_plan
        .contribution
        .notes
        .iter()
        .any(|n| n.contains("credits file")));

    let all = entries(&w, at);
    assert_eq!((all.len(), credited(&all)), (2, 1));
    assert!(markdown("x", &[]).contains("names no source folder"));
}

#[test]
fn the_option_is_stored_in_the_build_and_read_back() {
    use or_project::Output;
    let mut out = Output::of(OutputKind::FivemResource, "fivem", "pack", &["wardrobe"]);
    let options = Options {
        credits: true,
        include_tags: vec!["singleplayer".into(), "Uniform".into()],
        ..Options::default()
    };
    options.store(&mut out);
    assert_eq!(Options::of(&out), (options, Vec::new()));
    let mut again = Options::default();
    again.set("credits", "on").unwrap();
    again
        .set("include-tags", "singleplayer, UNIFORM,,singleplayer")
        .unwrap();
    assert_eq!(again.include_tags, ["singleplayer", "UNIFORM"]);
    again.set("include-tags", "none").unwrap();
    assert!(again.include_tags.is_empty());
    assert!(again.set("credits", "maybe").is_err());
}
