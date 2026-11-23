//! What stops a build of peds, what a waiver with a reason lets through, what the verifier says of a
//! tampered build, which module owns a ped's name, and how a folder of ped files becomes a module.

mod common;

use common::{container, ped_module, plan, staged_files, write};
use or_addonped::model::{AddonPeds, Waiver};
use or_addonped::verify::verify;
use or_addonped::{AddonPed, FILE};
use or_project::adopt::{analyse, Recogniser};
use or_project::{build, Container, Module, Output};

#[test]
fn a_model_that_streams_under_another_name_stops_the_build_until_a_waiver_gives_a_reason() {
    let Some(files) = staged_files() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let module = ped_module(dir.path(), "my_ped", &files);
    let container = container(&["peds"]);

    // The model is renamed in the module and the ped's file list follows it: the game would not find it.
    std::fs::rename(module.join("my_ped.yft"), module.join("not_my_ped.yft")).unwrap();
    let mut model = AddonPeds::read(&module).unwrap();
    model.peds[0].files[0].path = "not_my_ped.yft".into();
    model.write(&module).unwrap();

    let refused = plan(&container, dir.path());
    assert!(!refused.is_buildable());
    assert!(
        refused
            .problems
            .iter()
            .any(|p| p.starts_with("model-name: ") && p.contains("`my_ped`")),
        "{:?}",
        refused.problems
    );
    let root = dir.path().join("build");
    assert!(
        build(&refused, &root).is_err(),
        "a plan with a problem is not built"
    );
    assert!(!root.join("ped_pack").exists());

    model.waivers.push(Waiver {
        code: "model-name".into(),
        ped: None,
        reason: "the base game's own ped is replaced by name in a script".into(),
    });
    model.write(&module).unwrap();
    let allowed = plan(&container, dir.path());
    assert_eq!(
        allowed.problems,
        Vec::<String>::new(),
        "a waiver with a reason lets it through"
    );
    build(&allowed, &root).expect("and the build goes");
}

#[test]
fn a_data_file_that_does_not_define_a_ped_is_a_problem_of_the_plan_and_of_a_tampered_build() {
    let Some(files) = staged_files() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let module = ped_module(dir.path(), "my_ped", &files);
    let container = container(&["peds"]);
    let root = dir.path().join("build");
    let built = build(&plan(&container, dir.path()), &root).expect("a good module builds");
    let model = AddonPeds::read(&module).unwrap();
    assert!(verify(&model, &built.folder).is_clean());

    // Tampered after the build: the data file no longer defines the ped.
    std::fs::write(
        built.folder.join("peds.meta"),
        common::peds_meta(&["someone_else"]),
    )
    .unwrap();
    let tampered = verify(&model, &built.folder);
    assert!(!tampered.is_clean());
    assert!(
        tampered
            .problems()
            .iter()
            .any(|p| p.contains("no entry named `my_ped`")),
        "{:?}",
        tampered.problems()
    );

    // Tampered after the build: the manifest no longer registers the data file.
    let manifest = built.folder.join("fxmanifest.lua");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace("data_file 'PED_METADATA_FILE' 'peds.meta'", ""),
    )
    .unwrap();
    std::fs::write(
        built.folder.join("peds.meta"),
        common::peds_meta(&["my_ped"]),
    )
    .unwrap();
    let unregistered = verify(&model, &built.folder);
    assert!(
        unregistered
            .problems()
            .iter()
            .any(|p| p.contains("the manifest does not state")),
        "{:?}",
        unregistered.problems()
    );

    // The same in the module: the plan refuses it before anything is written.
    std::fs::write(
        module.join("peds.meta"),
        common::peds_meta(&["someone_else"]),
    )
    .unwrap();
    let plan = plan(&container, dir.path());
    assert!(
        plan.problems
            .iter()
            .any(|p| p.starts_with("meta-no-entry: ")),
        "{:?}",
        plan.problems
    );
}

#[test]
fn two_contents_that_name_one_ped_are_refused_and_so_is_one_that_leaves_it_to_the_other() {
    let Some(files) = staged_files() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    ped_module(dir.path(), "my_ped", &files);
    // A second module in another folder with a ped of the same name, spelled in capitals in its model.
    let second = dir.path().join("modules").join("more");
    write(
        &second.join("my_ped.yft"),
        &std::fs::read(&files.model).unwrap(),
    );
    write(
        &second.join("my_ped.ytd"),
        &std::fs::read(&files.textures).unwrap(),
    );
    write(
        &second.join("peds.meta"),
        common::peds_meta(&["MY_PED"]).as_bytes(),
    );
    let mut model = AddonPeds::new();
    model.meta = Some("peds.meta".into());
    let mut ped = or_addonped::Ped::new("MY_PED");
    ped.files = ["my_ped.yft", "my_ped.ytd"]
        .map(|path| or_addonped::PedFile {
            path: path.into(),
            at: None,
        })
        .to_vec();
    model.peds.push(ped);
    model.write(&second).unwrap();

    let both = Container {
        modules: vec![
            Module::addon_ped("peds", "modules/peds"),
            Module::addon_ped("more", "modules/more"),
        ],
        outputs: vec![
            Output::fivem_resource("everything", "ped_pack", &["peds", "more"]),
            Output::fivem_resource("only_the_first", "first_only", &["peds"]),
        ],
    };
    let contributions = or_addonped::contributions(&both, dir.path());
    let together = or_project::plan_with(&both, "everything", dir.path(), &contributions).unwrap();
    assert!(
        together
            .problems
            .iter()
            .any(|p| p.contains("ped model") && p.to_lowercase().contains("my_ped")),
        "one name for the game, whatever case it is spelled in: {:?}",
        together.problems
    );
    let alone = or_project::plan_with(&both, "only_the_first", dir.path(), &contributions).unwrap();
    assert!(
        alone.problems.iter().any(|p| p.contains("ped model")),
        "a clash is a problem of every build, including one that leaves the other module out: {:?}",
        alone.problems
    );
}

#[test]
fn a_folder_of_ped_files_is_recognised_as_peds_and_adopting_it_claims_what_the_model_names() {
    let Some(files) = staged_files() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let copy = |from: &std::path::Path, to: &str| {
        write(&dir.path().join(to), &std::fs::read(from).unwrap())
    };
    copy(&files.model, "stream/my_ped.yft");
    copy(&files.textures, "stream/my_ped.ytd");
    copy(&files.drawables, "stream/my_ped/head_000_r.ydd");
    copy(&files.expressions, "stream/other_ped.yed");
    write(
        &dir.path().join("peds.meta"),
        common::peds_meta(&["my_ped"]).as_bytes(),
    );
    write(
        &dir.path().join("fxmanifest.lua"),
        b"fx_version 'cerulean'\n",
    );
    write(&dir.path().join("server.lua"), b"print('hi')\n");
    write(
        &dir.path().join("2fast.yft"),
        &std::fs::read(&files.model).unwrap(),
    );

    assert!(!AddonPed.holds(dir.path()), "nothing states peds yet");
    let adoption = analyse(dir.path(), &[&AddonPed]);

    let [found] = adoption.found.as_slice() else {
        panic!("one content: {:?}", adoption.found);
    };
    assert_eq!(found.module.kind, or_project::ModuleKind::AddonPed);
    assert!(
        found.summary.starts_with("1 ped(s), 3 file(s): my_ped"),
        "{}",
        found.summary
    );
    let written = adoption
        .writes
        .iter()
        .find(|w| w.path.ends_with(FILE))
        .expect("it adds the model");
    let model = AddonPeds::from_json(&written.bytes, "adopted").unwrap();
    assert_eq!(model.meta.as_deref(), Some("peds.meta"));
    let ped = &model.peds[0];
    assert_eq!(ped.name, "my_ped");
    let mut paths: Vec<&str> = ped.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "stream/my_ped.yft",
            "stream/my_ped.ytd",
            "stream/my_ped/head_000_r.ydd"
        ]
    );
    assert!(
        ped.files.iter().all(|f| f.at.is_none()),
        "every file is where the layout puts it already"
    );

    let left: Vec<String> = adoption
        .files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(left.contains(&"server.lua".to_string()), "{left:?}");
    assert!(
        left.contains(&"other_ped.yed".to_string()),
        "a file of no ped stays a loose file: {left:?}"
    );
    assert!(left.contains(&"2fast.yft".to_string()), "{left:?}");
    assert!(!left.contains(&"my_ped.yft".to_string()) && !left.contains(&"peds.meta".to_string()));
    assert!(
        adoption.unplaced.iter().any(|u| u.path == "fxmanifest.lua"),
        "the manifest is said not to be carried: {:?}",
        adoption.unplaced
    );
    assert!(
        adoption
            .unplaced
            .iter()
            .any(|u| u.path == "2fast.yft" && u.why.contains("not named like a ped")),
        "{:?}",
        adoption.unplaced
    );
}

#[test]
fn a_vehicle_folder_stays_files_and_a_model_no_data_file_names_is_no_ped() {
    let dir = tempfile::tempdir().unwrap();
    let put = |name: &str, body: &[u8]| write(&dir.path().join(name), body);
    let manifest = b"fx_version 'cerulean'\nclient_script 'client.lua'\ndata_file 'VEHICLE_METADATA_FILE' 'vehicles.meta'\n";
    put("fxmanifest.lua", manifest);
    put("stream/adder.yft", b"a vehicle model");
    put("stream/adder_hi.yft", b"its high detail model");
    put("stream/adder.ytd", b"its textures");
    put("vehicles.meta", b"<CVehicleModelInfo__InitDataList/>");
    put("handling.meta", b"<CHandlingDataMgr/>");
    put("client.lua", b"print('adder')");

    let adoption = analyse(dir.path(), &[&AddonPed]);

    assert!(adoption.found.is_empty(), "{:?}", adoption.found);
    assert!(
        adoption.unplaced.is_empty() && adoption.writes.is_empty(),
        "the manifest is the user's own: {:?} {:?}",
        adoption.unplaced,
        adoption.writes
    );
    assert_eq!(adoption.files.len(), 7, "every file stays a file");

    // A data file that names one model makes that model a ped and no other.
    put("peds.meta", common::peds_meta(&["adder"]).as_bytes());
    let adoption = analyse(dir.path(), &[&AddonPed]);
    let [found] = adoption.found.as_slice() else {
        panic!("{:?}", adoption.found);
    };
    assert!(
        found.summary.starts_with("1 ped(s), 2 file(s): adder"),
        "{}",
        found.summary
    );
    let left: Vec<String> = adoption
        .files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        left.contains(&"adder_hi.yft".to_string()),
        "a model the data file does not name stays a file: {left:?}"
    );
}

#[test]
fn an_empty_module_is_started_with_a_model_that_holds_no_ped_and_keeps_one_that_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let module = Module::addon_ped("peds", "modules/peds");
    or_addonped::adopt::start_module(dir.path(), &module).unwrap();
    let model = AddonPeds::read(&dir.path().join("modules/peds")).unwrap();
    assert!(model.peds.is_empty() && model.meta.is_none());

    let mut changed = model.clone();
    changed.meta = Some("peds.meta".into());
    changed.write(&dir.path().join("modules/peds")).unwrap();
    or_addonped::adopt::start_module(dir.path(), &module).unwrap();
    assert_eq!(
        AddonPeds::read(&dir.path().join("modules/peds"))
            .unwrap()
            .meta
            .as_deref(),
        Some("peds.meta"),
        "starting a module that has a model leaves it"
    );
}
