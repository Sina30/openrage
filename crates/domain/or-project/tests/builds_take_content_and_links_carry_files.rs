//! A build ships exactly the content it takes, a Files content can be linked from another project, and a
//! project of links alone builds; a link is not followed into the links of the project it leads to.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use or_project::links::{add_link, add_project_links, links};
use or_project::{
    build, cannot_ship, plan, plans_with, verify, Container, Contribution, Module, ModuleKind,
    Origin, Output, OutputKind, Plan, PlannedFile, Project,
};

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A content of two streamed files named for it, so the manifest does not list them.
fn content(id: &str) -> Contribution {
    let mut contribution = Contribution::of(id);
    for n in 1..=2 {
        contribution.files.push(PlannedFile {
            path: format!("stream/{id}/{n}.dat"),
            origin: Origin::Document(format!("{id} {n}")),
        });
    }
    contribution
}

fn stand_in(ids: &[&str]) -> Container {
    Container {
        modules: ids
            .iter()
            .map(|id| Module {
                id: id.to_string(),
                kind: ModuleKind::Unknown("widgets".into()),
                extra: Default::default(),
            })
            .collect(),
        outputs: vec![
            Output::fivem_resource("everything", "pack", ids),
            Output::fivem_resource("update", "pack_update", ids),
        ],
    }
}

fn paths_of(plan: &Plan) -> BTreeSet<String> {
    (plan.files.iter())
        .map(|f| f.path.clone())
        .filter(|p| p.starts_with("stream/"))
        .collect()
}

fn planned(container: &Container, id: &str, ids: &[&str]) -> Plan {
    let contributions: Vec<Contribution> = ids.iter().map(|id| content(id)).collect();
    let mut plans = plans_with(container, id, Path::new("/projects/pack"), &contributions)
        .expect("the build is declared");
    assert_eq!(plans.len(), 1);
    plans.remove(0)
}

#[test]
fn a_build_over_two_chosen_contents_contains_exactly_their_files() {
    let mut container = stand_in(&["a", "b", "c"]);
    container.set_taken("update", "b", false).unwrap();
    container.set_taken("update", "c", false).unwrap();

    let everything = planned(&container, "everything", &["a", "b", "c"]);
    let only_a = planned(&container, "update", &["a", "b", "c"]);
    assert_eq!(
        paths_of(&only_a),
        content("a").files.iter().map(|f| f.path.clone()).collect()
    );
    assert_eq!(paths_of(&everything).len(), 6);

    // A split for update cadence is two builds over different contents, which share no file.
    container.set_taken("update", "a", false).unwrap();
    container.set_taken("update", "c", true).unwrap();
    container.set_taken("update", "b", true).unwrap();
    let cadence = planned(&container, "update", &["a", "b", "c"]);
    let wanted: BTreeSet<String> = ["b", "c"]
        .iter()
        .flat_map(|id| content(id).files.into_iter().map(|f| f.path))
        .collect();
    assert_eq!(paths_of(&cadence), wanted);
    assert!(
        paths_of(&cadence).is_disjoint(&paths_of(&only_a)),
        "two builds over different contents ship different files"
    );
    assert_eq!(
        container.output("update").unwrap().take,
        ["b", "c"],
        "ticked contents keep the order of the project's content"
    );
}

/// A project folder `name` under `dir` holding `files`, with the build `build` that takes its one Files content when there is one.
fn project_with(dir: &Path, name: &str, files: &[(&str, &[u8])], build: Option<&str>) -> PathBuf {
    let folder = dir.join(name);
    let members: Vec<PathBuf> = files
        .iter()
        .map(|(rel, body)| {
            write(&folder.join(rel), body);
            folder.join(rel)
        })
        .collect();
    let mut container = Container::default();
    if let Some(resource) = build {
        container
            .outputs
            .push(Output::fivem_resource("fivem", resource, &["files"]));
    }
    let file = folder.join(format!("{name}.openrage-proj"));
    Project::create(&file, &container, &members).expect("a new project");
    file
}

fn plan_of(file: &Path) -> Plan {
    let project = Project::load(file).expect("the project reads");
    plan(
        &project.container,
        &project.container.outputs[0].id,
        &project.folder(),
        &project.sources(),
    )
    .expect("the build is declared")
}

#[test]
fn a_linked_files_content_lands_in_the_resource_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let police = project_with(
        dir.path(),
        "police",
        &[("server.lua", b"print('s')")],
        Some("police_pack"),
    );
    let brand = project_with(
        dir.path(),
        "brand",
        &[
            ("client/main.lua", b"print('c')\r\n"),
            ("html/ui.html", b"<p>ui</p>"),
        ],
        None,
    );
    let brand_folder = brand.parent().unwrap();

    or_project::project::edit_container(&police, |container| {
        add_link(container, &police, ModuleKind::Files, brand_folder, None).map(|_| ())
    })
    .expect("a project's Files content is linked by its folder");
    let linked = Project::load(&police).unwrap();
    assert_eq!(linked.container.outputs[0].take, ["files", "brand"]);

    let planned = plan_of(&police);
    assert!(planned.is_buildable(), "{:?}", planned.problems);
    let shipped: BTreeSet<&str> = planned.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        shipped,
        BTreeSet::from([
            "client/main.lua",
            "fxmanifest.lua",
            "html/ui.html",
            "server.lua"
        ])
    );

    let root = dir.path().join("out");
    let built = build(&planned, &root).expect("the build succeeds");
    assert_eq!(
        std::fs::read(built.folder.join("client/main.lua")).unwrap(),
        b"print('c')\r\n",
        "the linked file is copied as it is"
    );
    let found = verify(&planned, &built.folder);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(found.checked, 4, "every file is read back against the plan");
}

#[test]
fn a_project_of_links_only_builds() {
    let dir = tempfile::tempdir().unwrap();
    let hub = project_with(dir.path(), "hub", &[], Some("hub_pack"));
    let one = project_with(dir.path(), "one", &[("a/one.lua", b"1")], None);
    let two = project_with(dir.path(), "two", &[("b/two.lua", b"2")], None);
    or_project::project::edit_container(&hub, |container| {
        container.modules.clear();
        container.outputs[0].take.clear();
        for folder in [&one, &two] {
            add_link(
                container,
                &hub,
                ModuleKind::Files,
                folder.parent().unwrap(),
                None,
            )?;
        }
        Ok(())
    })
    .unwrap();

    let loaded = Project::load(&hub).unwrap();
    assert!(
        loaded.container.modules.iter().all(Module::is_link),
        "the project holds links and nothing of its own"
    );
    let planned = plan_of(&hub);
    assert!(planned.is_buildable(), "{:?}", planned.problems);
    let root = dir.path().join("out");
    let built = build(&planned, &root).expect("a project of links builds");
    for shipped in ["a/one.lua", "b/two.lua", "fxmanifest.lua"] {
        assert!(built.folder.join(shipped).is_file(), "{shipped}");
    }
    assert!(verify(&planned, &built.folder).problems.is_empty());
}

#[test]
fn a_link_is_not_followed_and_the_banner_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let logos = project_with(dir.path(), "logos", &[("logo/a.lua", b"logo")], None);
    let brand = project_with(dir.path(), "brand", &[("client/main.lua", b"brand")], None);
    let police = project_with(dir.path(), "police", &[], Some("police_pack"));
    or_project::project::edit_container(&brand, |container| {
        add_link(
            container,
            &brand,
            ModuleKind::Files,
            logos.parent().unwrap(),
            None,
        )
        .map(|_| ())
    })
    .unwrap();

    let linked = or_project::project::edit_container(&police, |container| {
        add_project_links(container, &police, brand.parent().unwrap())
    })
    .expect("a whole project is linked");

    assert_eq!(linked.ids, ["brand"]);
    assert_eq!(linked.not_followed, ["logos"]);
    let project = Project::load(&police).unwrap();
    assert!(
        project.container.module("logos").is_none(),
        "the link of the linked project is not linked here"
    );
    let banner = links(&project.container, &police)
        .into_iter()
        .find(|link| link.module == "brand")
        .expect("the link is listed")
        .banner();
    assert!(
        banner.contains("logos") && banner.contains("not followed"),
        "{banner}"
    );
    let shipped: Vec<String> = plan_of(&police).files.into_iter().map(|f| f.path).collect();
    assert!(
        shipped.contains(&"client/main.lua".to_string()),
        "{shipped:?}"
    );
    assert!(
        !shipped.contains(&"logo/a.lua".to_string()),
        "what the linked project links is not shipped: {shipped:?}"
    );
}

#[test]
fn a_script_cannot_ship_in_a_singleplayer_dlc_and_the_reason_names_it() {
    let dlc = OutputKind::SingleplayerDlc;
    let mut scripts = Contribution::of("scripts");
    for path in ["client.lua", "assets.rpf/shirt.ydd", "data/vehicles.meta"] {
        scripts.files.push(PlannedFile {
            path: path.into(),
            origin: Origin::Document(String::new()),
        });
    }

    let why = cannot_ship(&scripts, &dlc).expect("a script has no type the game knows");
    assert!(why.contains("client.lua"), "{why}");
    assert!(
        cannot_ship(&scripts, &OutputKind::FivemResource).is_none(),
        "a resource runs scripts"
    );
    assert!(cannot_ship(&scripts, &OutputKind::OivPackage).is_none());

    // The game is told a type once the content states one, and a file in an archive is the archive's.
    let scripts = scripts.with_file_types(Some(&serde_json::json!({
        "client.lua": "HANDLING_FILE",
        "data/vehicles.meta": "VEHICLE_METADATA_FILE"
    })));
    assert!(cannot_ship(&scripts, &dlc).is_none());

    let mut asks_for_a_manifest = Contribution::of("scripts");
    asks_for_a_manifest
        .manifest
        .push(or_project::ManifestEntry {
            key: "client_script".into(),
            values: vec!["client.lua".into()],
        });
    let why = cannot_ship(&asks_for_a_manifest, &dlc).expect("a pack has no manifest");
    assert!(why.contains("client_script"), "{why}");
}

#[test]
fn a_linked_projects_source_folders_are_cooked_into_the_build_and_never_shipped_as_files() {
    use or_project::links::linked_sources;
    use or_project::{files_contributions_with, Cooks, SourceFolder};

    let dir = tempfile::tempdir().unwrap();
    let police = project_with(
        dir.path(),
        "police",
        &[("server.lua", b"s")],
        Some("police_pack"),
    );
    // ⚠ The linked project holds its source folder and a file in it as members, as a project that took in
    // its whole folder does: only then has the filter in `linked_files` anything to drop.
    let folder = dir.path().join("brand");
    let source = folder.join("art/logo.textures.src");
    write(&folder.join("client.lua"), b"c");
    write(&source.join("asset.json"), b"{}");
    write(&source.join("logo.dds"), b"dds");
    let brand = folder.join("brand.openrage-proj");
    let members = [
        folder.join("client.lua"),
        source.clone(),
        source.join("asset.json"),
        source.join("logo.dds"),
    ];
    Project::create(&brand, &Container::default(), &members).expect("a new project");
    or_project::project::edit_container(&police, |container| {
        add_link(
            container,
            &police,
            ModuleKind::Files,
            brand.parent().unwrap(),
            None,
        )
        .map(|_| ())
    })
    .unwrap();
    let project = Project::load(&police).unwrap();

    let (found, cut) = linked_sources(&project.container, &project.folder());

    assert!(!cut);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].ends_with("logo.textures.src"));
    let folders = vec![SourceFolder {
        folder: found[0].clone(),
        cooks: Ok(Cooks {
            kind: "textures".into(),
            name: "logo".into(),
            extension: "ytd".into(),
            bytes: 3,
            game: Vec::new(),
        }),
    }];
    let handed = files_contributions_with(
        &project.container,
        "fivem",
        &project.folder(),
        &project.sources(),
        &folders,
    )
    .unwrap();
    let planned = plans_with(&project.container, "fivem", &project.folder(), &handed)
        .unwrap()
        .remove(0);

    assert!(planned.is_buildable(), "{:?}", planned.problems);
    let cooked: Vec<&str> = (planned.files.iter())
        .filter(|f| matches!(f.origin, Origin::Cook { .. }))
        .map(|f| f.path.as_str())
        .collect();
    assert_eq!(
        cooked,
        ["stream/logo.ytd"],
        "the source is cooked once, into the build"
    );
    assert!(
        !planned
            .files
            .iter()
            .any(|f| f.path.contains("logo.dds") || f.path.contains("asset.json")),
        "the folder never ships: {:?}",
        planned.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    // The project's own files are not told the linked project's folders.
    let own_only = files_contributions_with(
        &project.container,
        "fivem",
        &project.folder(),
        &project.sources(),
        &[],
    )
    .unwrap();
    let own_plan = plans_with(&project.container, "fivem", &project.folder(), &own_only)
        .unwrap()
        .remove(0);
    assert!(!own_plan
        .files
        .iter()
        .any(|f| matches!(f.origin, Origin::Cook { .. })));
}
