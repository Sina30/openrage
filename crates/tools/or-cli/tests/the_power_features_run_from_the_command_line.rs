//! `openrage-cli wardrobe query`, `bulk`, `review`, `credit`, `pack` and `unpack` do what the library does: a query selects the garments the pack
//! states, a bulk edit is a dry run until it is applied, two versions are reviewed read-only, a garment is credited in the `asset.json` of its
//! sources, and a whole pack leaves as one package and comes back as an equal pack.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_project::{Container, Module, Output, OutputKind, Project};
use or_wardrobe::edit::{update, Edit};
use or_wardrobe::Wardrobe;

fn wardrobe(args: &[&str]) -> ExitCode {
    let mut all = vec!["wardrobe".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// A project whose one clothing content holds the three jackets of a folder dropped on it, the first two tagged `wool`.
fn project(dir: &Path) -> PathBuf {
    let pack = dir.join("pack");
    or_wardrobe::edit::create(&pack, "demo").unwrap();
    let drop = dir.join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    let c = "mp_m_freemode_01_mp_m_demo";
    for n in 0..3 {
        let tag = format!("jacket{n}");
        std::fs::write(
            drop.join(format!("{c}^jbib_00{n}_u.ydd")),
            or_source::sample::drawables_of(&tag),
        )
        .unwrap();
        std::fs::write(
            drop.join(format!("{c}^jbib_diff_00{n}_a_uni.ytd")),
            or_source::sample::textures_of(&tag),
        )
        .unwrap();
    }
    or_wardrobe::edit::add_clothes(&pack, &[drop]).unwrap();
    let ids: Vec<String> = Wardrobe::read(&pack)
        .unwrap()
        .garments
        .iter()
        .map(|g| g.id.clone())
        .collect();
    for id in &ids[..2] {
        update(&pack, |w| {
            or_wardrobe::edit::apply(w, id, Edit::AddTag("wool".into()))
        })
        .unwrap();
    }
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "pack")],
        outputs: vec![Output::of(
            OutputKind::FivemResource,
            "fivem",
            "demo",
            &["wardrobe"],
        )],
    };
    let path = dir.join("demo.openrage-proj");
    Project::create(&path, &container, &[]).unwrap();
    path
}

fn tags(pack: &Path) -> Vec<Vec<String>> {
    (Wardrobe::read(pack).unwrap().garments.iter())
        .map(|g| g.tags.clone())
        .collect()
}

#[test]
fn a_query_selects_what_the_pack_states_and_a_term_that_does_not_read_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path());
    assert_eq!(
        wardrobe(&["query", text(&path), "tag:wool"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        wardrobe(&["query", text(&path), "tag:wool", "--json"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        wardrobe(&["query", text(&path), "colour:red"]),
        ExitCode::FAILURE
    );
    assert_ne!(
        wardrobe(&["query", text(&path)]),
        ExitCode::SUCCESS,
        "a query is needed"
    );
}

#[test]
fn a_bulk_edit_is_a_dry_run_until_it_is_applied_and_then_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path());
    let pack = dir.path().join("pack");
    let before = tags(&pack);

    assert_eq!(
        wardrobe(&["bulk", text(&path), "tag:wool", "--add-tag", "winter"]),
        ExitCode::SUCCESS
    );
    assert_eq!(tags(&pack), before, "a dry run writes nothing");

    assert_eq!(
        wardrobe(&[
            "bulk",
            text(&path),
            "tag:wool",
            "--add-tag",
            "winter",
            "--apply"
        ]),
        ExitCode::SUCCESS
    );
    let after = tags(&pack);
    assert_eq!(
        after
            .iter()
            .filter(|t| t.contains(&"winter".to_string()))
            .count(),
        2,
        "the garments the query selects and no other: {after:?}"
    );

    // One change at a time, and a change is needed.
    assert_ne!(
        wardrobe(&["bulk", text(&path), "tag:wool"]),
        ExitCode::SUCCESS
    );
    assert_ne!(
        wardrobe(&[
            "bulk",
            text(&path),
            "tag:wool",
            "--add-tag",
            "a",
            "--remove"
        ]),
        ExitCode::SUCCESS
    );
    // The review sees it: the oldest version against the pack as it is.
    let oldest = or_wardrobe::backup::versions(&pack)[0].n.to_string();
    assert_eq!(
        wardrobe(&["review", text(&path), "--list"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        wardrobe(&["review", text(&path), "--from", &oldest, "--json"]),
        ExitCode::SUCCESS
    );
    assert_ne!(
        wardrobe(&["review", text(&path)]),
        ExitCode::SUCCESS,
        "a side is needed"
    );
}

#[test]
fn a_garment_is_credited_and_a_whole_pack_leaves_as_one_package_and_comes_back_equal() {
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path());
    let pack = dir.path().join("pack");
    let first = Wardrobe::read(&pack).unwrap().garments[0].clone();

    assert_eq!(
        wardrobe(&[
            "credit",
            text(&path),
            "--garment",
            &first.id,
            "--author",
            "Jane Doe",
            "--licence",
            "CC-BY-4.0",
        ]),
        ExitCode::SUCCESS
    );
    let model = pack.join(first.model.as_deref().expect("a model"));
    let asset = std::fs::read_to_string(model.join("asset.json")).unwrap();
    assert!(asset.contains("\"licence\": \"CC-BY-4.0\""), "{asset}");

    assert_eq!(wardrobe(&["sizes", text(&path)]), ExitCode::SUCCESS);
    assert_eq!(
        wardrobe(&["sizes", text(&path), "--json"]),
        ExitCode::SUCCESS
    );

    let file = dir.path().join("demo.openrage-pack");
    assert_eq!(
        wardrobe(&["pack", text(&path), "--out", text(&file)]),
        ExitCode::SUCCESS
    );
    assert!(file.is_file());
    let there = dir.path().join("there");
    assert_eq!(
        wardrobe(&["unpack", text(&file), "--out", text(&there)]),
        ExitCode::SUCCESS
    );
    let (was, now) = (
        Wardrobe::read(&pack).unwrap(),
        Wardrobe::read(&there).unwrap(),
    );
    assert_eq!(
        now.garments
            .iter()
            .map(|g| (&g.id, &g.tags))
            .collect::<Vec<_>>(),
        was.garments
            .iter()
            .map(|g| (&g.id, &g.tags))
            .collect::<Vec<_>>()
    );
    // The credit travels with the source.
    let moved = std::fs::read_to_string(
        there
            .join(now.garments[0].model.as_deref().unwrap())
            .join("asset.json"),
    )
    .unwrap();
    assert!(moved.contains("Jane Doe"), "{moved}");

    // A folder that holds something is not unpacked into, and a file that is not a package is refused.
    assert_ne!(
        wardrobe(&["unpack", text(&file), "--out", text(&there)]),
        ExitCode::SUCCESS
    );
    assert_ne!(
        wardrobe(&[
            "unpack",
            text(&path),
            "--out",
            text(&dir.path().join("elsewhere"))
        ]),
        ExitCode::SUCCESS
    );
}
