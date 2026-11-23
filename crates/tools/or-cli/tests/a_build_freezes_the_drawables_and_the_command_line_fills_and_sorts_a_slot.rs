//! `openrage-cli wardrobe build` freezes the drawables it shipped, a rebuild that would move one is refused until a reason
//! says why, `wardrobe freeze` says where things stand, and `wardrobe dummies` fills the holes of a slot or sorts its dummies last.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_project::{Container, Module, Output, OutputKind, Project};
use or_wardrobe::{Garment, Ped, Slot, Variation, Wardrobe};

fn run(group: &str, args: &[&str]) -> ExitCode {
    let mut all = vec![group.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

fn put(folder: &Path, rel: &str, bytes: &[u8]) {
    let to = folder.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, bytes).unwrap();
}

/// A project in `dir/pack` holding three jackets for the male ped, none pinned.
fn project(dir: &Path) -> (PathBuf, PathBuf) {
    let folder = dir.join("pack");
    put(
        &folder,
        "textures/t.ytd",
        &or_source::sample::textures_of("t"),
    );
    let mut w = Wardrobe::new("demo");
    for i in 0..3 {
        put(
            &folder,
            &format!("models/m{i}.ydd"),
            &or_source::sample::drawables_of(&format!("m{i}")),
        );
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some("jbib".parse::<Slot>().unwrap());
        g.model = Some(format!("models/m{i}.ydd"));
        g.label = Some(format!("jacket {i}"));
        g.variations = vec![Variation {
            texture: Some("textures/t.ytd".into()),
            ..Variation::default()
        }];
        w.garments.push(g);
    }
    w.write(&folder).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::of(
            OutputKind::FivemResource,
            "fivem",
            "demo",
            &["wardrobe"],
        )],
    };
    let file = folder.join("demo.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    (file, folder)
}

#[test]
fn a_build_freezes_the_drawables_and_a_rebuild_that_would_move_one_is_refused_until_a_reason_says_why(
) {
    let dir = tempfile::tempdir().unwrap();
    let (file, folder) = project(dir.path());
    assert!(!folder.join("wardrobe.lock.json").exists());

    assert_eq!(run("wardrobe", &["build", text(&file)]), ExitCode::SUCCESS);
    let lock = std::fs::read_to_string(folder.join("wardrobe.lock.json")).expect("the build froze");
    assert!(lock.contains("openrage-wardrobe-lock"), "{lock}");
    assert_eq!(run("wardrobe", &["freeze", text(&file)]), ExitCode::SUCCESS);

    // The first jacket leaves, and the other two would take its place and the next.
    let mut w = Wardrobe::read(&folder).unwrap();
    w.garments.remove(0);
    w.write(&folder).unwrap();
    assert_ne!(
        run("wardrobe", &["build", text(&file)]),
        ExitCode::SUCCESS,
        "refused: a frozen drawable would move"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("wardrobe.lock.json")).unwrap(),
        lock,
        "a build that stopped froze nothing"
    );

    // A reason lets it through, and the next lock is what shipped.
    let mut w = Wardrobe::read(&folder).unwrap();
    or_wardrobe::edit::waive(&mut w, "renumbered", None, "the server wipes its outfits").unwrap();
    w.write(&folder).unwrap();
    assert_eq!(run("wardrobe", &["build", text(&file)]), ExitCode::SUCCESS);
    assert_ne!(
        std::fs::read_to_string(folder.join("wardrobe.lock.json")).unwrap(),
        lock
    );

    // Thawed, the pack renumbers without a reason.
    assert_eq!(
        run("wardrobe", &["freeze", text(&file), "--thaw"]),
        ExitCode::SUCCESS
    );
    assert!(!folder.join("wardrobe.lock.json").exists());
}

#[test]
fn the_command_line_fills_the_holes_of_a_slot_with_dummies_and_sorts_them_last() {
    let dir = tempfile::tempdir().unwrap();
    let (file, folder) = project(dir.path());
    let mut w = Wardrobe::read(&folder).unwrap();
    w.garments[0].position = Some(0);
    w.garments[1].position = Some(3);
    w.garments[2].position = Some(5);
    w.write(&folder).unwrap();

    let slot = ["--ped", "male", "--slot", "jbib"];
    let mut fill = vec!["dummies", "fill", text(&file)];
    fill.extend(slot);
    assert_eq!(run("wardrobe", &fill), ExitCode::SUCCESS);
    let w = Wardrobe::read(&folder).unwrap();
    assert_eq!(w.garments.len(), 3 + 3, "holes at 1, 2 and 4");
    assert_eq!(w.garments.iter().filter(|g| g.is_reserved()).count(), 3);

    let mut sort = vec!["dummies", "sort", text(&file)];
    sort.extend(slot);
    assert_eq!(run("wardrobe", &sort), ExitCode::SUCCESS);
    let w = Wardrobe::read(&folder).unwrap();
    let allocation = or_wardrobe::allocate(&w);
    let real: Vec<u32> = allocation
        .placed
        .iter()
        .filter(|p| !w.garments[p.garment].is_reserved())
        .map(|p| p.place)
        .collect();
    assert_eq!(real, [0, 1, 2], "the garments with a model come first");

    assert_ne!(
        run("wardrobe", &["dummies", "shuffle", text(&file)]),
        ExitCode::SUCCESS
    );
    assert_ne!(
        run("wardrobe", &["dummies", "fill", text(&file)]),
        ExitCode::SUCCESS,
        "a ped and a slot are needed"
    );
}
