//! The editor and the command line plan, adopt and start a project's content with one list of kinds,
//! and a project that holds more than one kind goes through the one plan.

use std::path::Path;

use or_content::{contributions, kinds, plans_from, plans_given, recognisers, start_module};
use or_project::{Container, Contribution, Module, ModuleKind, Output};

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn the_list_of_kinds_and_the_list_of_recognisers_name_the_same_kinds_in_the_same_order() {
    let by_kind: Vec<ModuleKind> = kinds().iter().map(|k| k.kind()).collect();
    let by_recogniser: Vec<ModuleKind> = recognisers().iter().map(|r| r.kind()).collect();
    assert_eq!(
        by_kind,
        [
            ModuleKind::Clothing,
            ModuleKind::AddonPed,
            ModuleKind::Weapons,
            ModuleKind::Particles,
            ModuleKind::Maps
        ]
    );
    assert_eq!(by_recogniser, by_kind);
}

#[test]
fn a_new_module_gets_the_folder_and_the_empty_model_its_own_kind_keeps() {
    let dir = tempfile::tempdir().unwrap();
    start_module(dir.path(), &Module::clothing("c", "modules/c"), "my_pack").unwrap();
    start_module(dir.path(), &Module::addon_ped("p", "modules/p"), "my_pack").unwrap();
    start_module(dir.path(), &Module::weapons("w", "modules/w"), "my_pack").unwrap();
    start_module(dir.path(), &Module::particles("x", "modules/x"), "my_pack").unwrap();
    start_module(dir.path(), &Module::maps("m", "modules/m"), "my_pack").unwrap();

    let pack = std::fs::read_to_string(dir.path().join("modules/c/wardrobe.json")).unwrap();
    assert!(pack.contains("\"pack\": \"my_pack\""), "{pack}");
    let peds = std::fs::read_to_string(dir.path().join("modules/p/addonped.json")).unwrap();
    assert!(peds.contains("openrage-addonped"), "{peds}");
    let weapons = std::fs::read_to_string(dir.path().join("modules/w/weapons.json")).unwrap();
    assert!(weapons.contains("openrage-weapons"), "{weapons}");
    let particles = std::fs::read_to_string(dir.path().join("modules/x/effects.json")).unwrap();
    assert!(particles.contains("openrage-particles"), "{particles}");
    let maps = std::fs::read_to_string(dir.path().join("modules/m/maps.json")).unwrap();
    assert!(maps.contains("openrage-maps"), "{maps}");
    assert!(!dir.path().join("modules/p/wardrobe.json").exists());
    assert!(!dir.path().join("modules/w/addonped.json").exists());
    assert!(!dir.path().join("modules/c/addonped.json").exists());

    let files = start_module(dir.path(), &Module::files("f"), "x").unwrap_err();
    assert!(files.contains("no folder to start"), "{files}");
}

fn container() -> Container {
    Container {
        modules: vec![
            Module::clothing("c", "modules/c"),
            Module::addon_ped("p", "modules/p"),
        ],
        outputs: vec![Output::fivem_resource("fivem", "both", &["c", "p"])],
    }
}

#[test]
fn a_project_of_both_kinds_plans_with_what_each_hands_over() {
    let dir = tempfile::tempdir().unwrap();
    start_module(dir.path(), &Module::clothing("c", "modules/c"), "my_pack").unwrap();
    // A ped whose files are not game files: the plan copies them, and opening them is the validator's and the build's.
    let folder = dir.path().join("modules/p");
    write(&folder.join("my_ped.yft"), b"model");
    write(&folder.join("my_ped.ytd"), b"textures");
    write(
        &folder.join("peds.meta"),
        b"<Item><Name>my_ped</Name></Item>",
    );
    let mut model = or_addonped::AddonPeds::new();
    model.meta = Some("peds.meta".into());
    let mut ped = or_addonped::Ped::new("my_ped");
    ped.files = ["my_ped.yft", "my_ped.ytd"]
        .map(|path| or_addonped::PedFile {
            path: path.into(),
            at: None,
        })
        .to_vec();
    model.peds.push(ped);
    std::fs::create_dir_all(&folder).unwrap();
    model.write(&folder).unwrap();

    let plans =
        plans_from(&container(), "fivem", dir.path(), &[], &[]).expect("the output is declared");
    let [plan] = plans.as_slice() else {
        panic!("one resource: {}", plans.len());
    };
    assert_eq!(plan.problems, Vec::<String>::new());
    let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.contains(&"stream/my_ped.yft") && paths.contains(&"peds.meta"),
        "{paths:?}"
    );
    assert_eq!(
        plan.items
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
        ["my_ped"],
        "the ped is an item; an empty pack has none"
    );
    assert!(
        plan.budgets
            .iter()
            .any(|b| b.name == "addon_peds" && b.used == 1),
        "{:?}",
        plan.budgets
    );
}

#[test]
fn a_module_the_build_has_no_kind_for_is_refused_by_name_and_not_dropped() {
    let mut container = container();
    container.modules.push(Module {
        id: "w".into(),
        kind: ModuleKind::Unknown("widgets".into()),
        extra: Default::default(),
    });
    container.outputs[0].take.push("w".into());
    let dir = tempfile::tempdir().unwrap();
    let plans = plans_from(&container, "fivem", dir.path(), &[], &[]).unwrap();
    assert!(
        plans[0]
            .problems
            .iter()
            .any(|p| p.contains("content `w` is of type `widgets`")),
        "{:?}",
        plans[0].problems
    );
}

#[test]
fn what_a_screen_already_made_is_used_and_the_module_is_not_asked_again() {
    // The folders do not exist: asking the kinds for either module would say so.
    let dir = tempfile::tempdir().unwrap();
    let mut made = Contribution::of("p");
    made.problems.push("made by the screen".into());

    let asked = contributions(&container(), dir.path(), &[made.clone()]);
    assert_eq!(
        asked.iter().map(|c| c.module.as_str()).collect::<Vec<_>>(),
        ["c"],
        "only the module nobody has made a contribution for is asked"
    );
    assert!(
        asked[0].problems.iter().any(|p| p.contains("modules")),
        "{:?}",
        asked[0].problems
    );

    let plans = plans_given(&container(), "fivem", dir.path(), &[], &[], vec![made]).unwrap();
    assert!(plans[0].problems.iter().any(|p| p == "made by the screen"));
    assert!(
        !plans[0]
            .problems
            .iter()
            .any(|p| p.contains("addonped.json")),
        "the made contribution replaced the one the kind would have made: {:?}",
        plans[0].problems
    );
}

#[test]
fn a_ped_and_a_clothing_collection_of_one_spelling_are_two_names_and_two_peds_of_one_are_not() {
    let claim = |module: &str, kind: &str, name: &str| {
        let mut c = Contribution::of(module);
        c.claims.push(or_project::Claim {
            kind: kind.to_string(),
            name: name.to_string(),
        });
        c
    };
    let dir = Path::new("/projects/pack");
    let plan = |made: Vec<Contribution>| {
        plans_given(&container(), "fivem", dir, &[], &[], made)
            .unwrap()
            .remove(0)
    };

    let apart = plan(vec![
        claim("c", or_wardrobe::plan::COLLECTION, "mp_m_police"),
        claim("p", or_addonped::plan::PED_MODEL, "MP_M_Police"),
    ]);
    assert_eq!(
        apart.problems,
        Vec::<String>::new(),
        "the game resolves them in different tables"
    );

    let together = plan(vec![
        claim("c", or_addonped::plan::PED_MODEL, "my_ped"),
        claim("p", or_addonped::plan::PED_MODEL, "MY_PED"),
    ]);
    assert!(
        together.problems.iter().any(|p| p.contains("ped model")),
        "{:?}",
        together.problems
    );
}
