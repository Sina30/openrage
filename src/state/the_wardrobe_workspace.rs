//! What the Wardrobe workspace knows of a clothing project: where it is offered, what dropping files does and what
//! Undo takes back, how the engine's findings reach Problems with a fix and a waiver, and a build checked against the pack.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a worker
//! for each scan, edit and build that a case waits on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_project::{Container, Module, Output, Project};
use or_wardrobe::edit::Edit;

use super::front_door::{GameChoice, NewProject, Template};
use super::os_drop::{plan, DropAction, DropTarget};
use super::toasts::ToastAction;
use super::workbench::{BuildDot, Fix, LastBuild, Place, Question};
use super::{AppState, Prefs};
use crate::ui::dock::Workspace;

pub(super) const C: &str = "mp_m_freemode_01_mp_m_t";

pub(super) fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

/// A state with nothing open, as `from_prefs` makes one.
pub(super) fn bare() -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state
}

/// A clothing project made the way the template makes it, opened in a state.
pub(super) fn opened(dir: &Path) -> (AppState, PathBuf) {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let created = draft.create().expect("the template makes a project");
    let mut state = bare();
    state.load_project_from(created.file.clone());
    (state, created.file)
}

/// Polls the workbench until `done`, which a worker's answer makes true.
pub(super) fn until(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_workbench();
        if done(state) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The garments of the content shown, as the last scan read them.
pub(super) fn garments(state: &AppState) -> usize {
    state
        .wardrobe_pack()
        .and_then(|p| p.read.as_ref().ok())
        .map_or(0, |read| read.wardrobe.garments.len())
}

pub(super) use super::dropped_files::drop_files;

#[test]
fn the_clothing_pack_template_makes_a_pack_and_opens_the_project_on_the_wardrobe() {
    let dir = tempfile::tempdir().unwrap();
    let mut draft = NewProject::new(Template::Clothing, dir.path().to_path_buf());
    draft.set_name("My Tshirts".into());
    draft.game = GameChoice::Enhanced;
    assert_eq!(draft.refusal(), None);

    // What the dialog shows is what Create makes: the pack's file sits under the folder it is in.
    let tree: Vec<(usize, String)> = draft
        .tree()
        .into_iter()
        .map(|l| (l.depth, l.name))
        .collect();
    assert!(
        tree.contains(&(1, "clothing".into())) && tree.contains(&(2, "wardrobe.json".into())),
        "{tree:?}"
    );

    let created = draft.create().expect("it is made");
    let folder = created.file.parent().unwrap();
    let pack = or_wardrobe::Wardrobe::read(&folder.join("clothing")).unwrap();
    assert_eq!((pack.pack.as_str(), pack.garments.len()), ("my_tshirts", 0));
    assert!(folder.join("sources").is_dir());

    let project = Project::load(&created.file).unwrap();
    let module = project.container.modules.first().unwrap();
    assert_eq!(module.kind, or_project::ModuleKind::Clothing);
    assert_eq!(module.path(), Some("clothing"));
    let output = project.container.outputs.first().unwrap();
    assert_eq!(
        (output.name.as_str(), output.take.as_slice()),
        ("my_tshirts", ["clothing".to_string()].as_slice())
    );
    assert_eq!(
        output.target,
        Some(or_project::Target::Enhanced),
        "the project's game is the edition a new build ships for"
    );

    let mut state = bare();
    state.load_project_from(created.file);
    assert_eq!(state.active_workspace, Workspace::Wardrobe);
    assert_eq!(
        state.workspaces_shown(),
        [
            Workspace::Project,
            Workspace::Model,
            Workspace::Texture,
            Workspace::Wardrobe
        ],
        "a clothing pack shows the Wardrobe, Project, Texture and Model"
    );
}

#[test]
fn a_clothing_project_ships_its_build_and_each_new_one_for_the_game_it_is_for_and_as_authored_for_both(
) {
    let dir = tempfile::tempdir().unwrap();
    for (game, wanted) in [
        (GameChoice::Both, None),
        (GameChoice::Legacy, Some(or_project::Target::Legacy)),
    ] {
        let mut draft = NewProject::new(Template::Clothing, dir.path().to_path_buf());
        draft.set_name(format!("pack_{}", game.word()));
        draft.game = game;
        let created = draft.create().unwrap();
        let mut state = bare();
        state.load_project_from(created.file);
        assert_eq!(state.default_edition(), wanted, "{game:?}");
        let id = state
            .add_build(
                "second",
                or_project::OutputKind::FivemResource,
                state.default_edition(),
            )
            .unwrap();
        let output = state.project_carried.container.output(&id).unwrap();
        assert_eq!(output.target, wanted, "{game:?}");
        let first = &state.project_carried.container.outputs[0];
        assert_eq!(first.target, wanted, "the template's own build, {game:?}");
    }
}

#[test]
fn the_wardrobe_is_offered_only_while_the_project_holds_clothing_and_a_project_can_be_given_some() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("files_only");
    let file = folder.join("files_only.openrage-proj");
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::fivem_resource("fivem", "files_only", &["files"])],
    };
    write(&folder.join("a.lua"), b"print(1)");
    Project::create(&file, &container, &[folder.join("a.lua")]).unwrap();
    let mut state = bare();
    state.load_project_from(file);
    assert!(!state.workspaces_offered().contains(&Workspace::Wardrobe));
    assert!(!state.has_clothing());

    state.add_clothing_content();

    assert!(state.has_clothing());
    assert_eq!(state.active_workspace, Workspace::Wardrobe);
    assert!(state.workspaces_offered().contains(&Workspace::Wardrobe));
    let container = &state.project_carried.container;
    assert_eq!(container.modules.len(), 2);
    assert!(
        container.outputs[0].take.contains(&"clothing".to_string()),
        "the build the project has ships the new clothing: {:?}",
        container.outputs[0].take
    );
    assert!(folder.join("clothing/wardrobe.json").is_file());
    assert!(
        state.project_carried.changed,
        "the project file has an edit to save"
    );
}

#[test]
fn files_dropped_on_the_wardrobe_are_added_as_clothes_and_undo_takes_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    let paths = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"texture a"),
            (&format!("{C}^jbib_diff_000_b_uni.ytd"), b"texture b"),
            ("my hat.ydd", b"hat"),
        ],
    );

    // The window routes a drop on the Wardrobe's panel to Add clothes.
    assert_eq!(
        plan(&paths, DropTarget::Wardrobe, None, |_| true),
        [DropAction::AddClothes(paths.clone())]
    );
    state.handle_os_drop(paths, DropTarget::Wardrobe);
    until(&mut state, "the garments", |s| garments(s) == 2);

    let pack = state.wardrobe_pack().unwrap().read.as_ref().unwrap();
    let jacket = pack
        .wardrobe
        .garments
        .iter()
        .find(|g| g.variations.len() == 2)
        .expect("the jacket with its two textures");
    assert_eq!(jacket.ped, Some(or_wardrobe::Ped::Male));
    let nameless = pack
        .wardrobe
        .garments
        .iter()
        .find(|g| g.ped.is_none())
        .unwrap();
    assert_eq!(
        nameless.label.as_deref(),
        Some("my hat"),
        "a name that says nothing makes a garment marked ?"
    );
    let folder = file.parent().unwrap().join("clothing");
    assert!(folder
        .join(format!("models/{C}^jbib_000_u.drawables.src/asset.json"))
        .is_file());

    // A toast says what was done, with Undo in it.
    let toast = state
        .toasts
        .iter()
        .find(|t| matches!(t.action, Some(ToastAction::Undo(_))))
        .expect("the drop's toast has Undo");
    assert!(
        toast.message.contains("Added 2 garments"),
        "{}",
        toast.message
    );

    let undo = toast.action.expect("the button");
    state.run_toast_action(undo);
    until(&mut state, "the undo", |s| garments(s) == 0);
    // The files the drop copied go with it, but the worker takes them away after it takes the garments off the list
    until(
        &mut state,
        "the files the drop copied going with it",
        |_| !folder.join("models").exists(),
    );
    assert!(state.workbench.wardrobe.undo.is_empty());
}

#[test]
fn a_finding_of_the_engine_is_in_problems_with_its_fix_and_its_waiver_and_each_does_what_it_says() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    // Two garments with the same bytes, and a second pinned where the first is.
    let paths = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_001_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"t0"),
            (&format!("{C}^jbib_diff_001_a_uni.ytd"), b"t0"),
        ],
    );
    state.add_clothes(paths);
    until(&mut state, "the garments", |s| garments(s) == 2);
    let ids: Vec<String> = (state.wardrobe_pack().unwrap().read.as_ref().unwrap())
        .wardrobe
        .garments
        .iter()
        .map(|g| g.id.clone())
        .collect();
    for id in &ids {
        state.edit_garment(id.clone(), Edit::Position(Some(4)));
    }
    let module = state.wardrobe_module_id().unwrap();
    let shown = |s: &AppState, code: &str| {
        s.workbench
            .findings()
            .iter()
            .find(|f| {
                (f.fixes.iter()).any(|fix| match fix {
                    Fix::Remedy { finding, .. } | Fix::Waive { finding, .. } => {
                        finding.code == code
                    }
                    _ => false,
                })
            })
            .cloned()
    };
    until(&mut state, "the findings", |s| {
        shown(s, "position-clash").is_some() && shown(s, "duplicate-content").is_some()
    });

    // A clash has a fix and no waiver; a duplicate has both. Neither lists a copy of its own in the plan's problems.
    let clash = shown(&state, "position-clash").unwrap();
    assert!(matches!(clash.place, Place::Garment { id: Some(_), .. }));
    assert_eq!(clash.fixes.len(), 1, "{:?}", clash.fixes);
    let duplicate = shown(&state, "duplicate-content").unwrap();
    assert_eq!(duplicate.fixes.len(), 2, "{:?}", duplicate.fixes);
    assert_eq!(
        state
            .workbench
            .findings()
            .iter()
            .filter(|f| matches!(f.place, Place::Garment { .. })
                && f.text.contains("pinned at position"))
            .count(),
        1,
        "the plan's own sentence is not listed a second time"
    );

    // Fix… asks first, and says what it changes.
    state.ask_about_fix(&clash.fixes[0]);
    assert!(matches!(state.workbench.asking, Some(Question::Fix { .. })));
    let Fix::Remedy { finding, .. } = clash.fixes[0].clone() else {
        panic!("a fix");
    };
    state.workbench.asking = None;
    state.fix_finding(module.clone(), finding);
    until(&mut state, "the fix", |s| {
        shown(s, "position-clash").is_none()
    });

    // Waive… records the reason in the pack, and the finding leaves the list.
    let Fix::Waive { finding, .. } = duplicate.fixes[1].clone() else {
        panic!("a waiver");
    };
    state.waive_finding(
        module,
        finding.code.to_string(),
        finding.garment.clone(),
        "the same mesh on purpose".into(),
    );
    until(&mut state, "the waiver", |s| {
        shown(s, "duplicate-content").is_none()
    });
    let pack = state.wardrobe_pack().unwrap().read.as_ref().unwrap();
    assert_eq!(pack.wardrobe.waivers.len(), 1);
    assert_eq!(pack.wardrobe.waivers[0].reason, "the same mesh on purpose");
}

#[test]
fn the_pack_option_for_durtys_shop_names_is_written_by_the_engine_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.wardrobe_pack().is_some());
    let folder = file.parent().unwrap().join("clothing");
    assert!(
        !or_wardrobe::Wardrobe::read(&folder)
            .unwrap()
            .shop
            .durty_hashes,
        "off by default"
    );

    state.set_durty_hashes(true);

    // The engine wrote it, and the next scan read it back.
    until(&mut state, "the option", |s| {
        (s.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
            .is_some_and(|read| read.wardrobe.shop.durty_hashes)
    });
    assert!(
        or_wardrobe::Wardrobe::read(&folder)
            .unwrap()
            .shop
            .durty_hashes
    );
}

/// A drawable and a texture of the corpus that a reader opens; none where this machine has no corpus, and the test is skipped.
fn a_drawable_and_a_texture(test: &str) -> Option<Vec<PathBuf>> {
    use or_test_support::{Corpus, TARGET_GEN8};
    let mut found = Vec::new();
    for ext in ["ydd", "ytd"] {
        let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
            .target(TARGET_GEN8)
            .find_or_skip(test)
            .unwrap_or_default();
        files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
        let format = or_io::gta5::formats::format(ext).unwrap();
        let opens = files
            .into_iter()
            .take(40)
            .find(|p| std::fs::read(p).is_ok_and(|b| format.open_declared(&b).is_ok()));
        found.push(opens?);
    }
    Some(found)
}

/// A build of a clothing project is planned with the clothing in it, written, read back, and checked against the pack.
#[test]
fn a_build_of_a_clothing_project_is_checked_against_the_pack_it_was_made_from() {
    let Some(found) = a_drawable_and_a_texture(
        "a_build_of_a_clothing_project_is_checked_against_the_pack_it_was_made_from",
    ) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    let drop = dir.path().join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    let model = drop.join(format!("{C}^jbib_000_u.ydd"));
    let texture = drop.join(format!("{C}^jbib_diff_000_a_uni.ytd"));
    std::fs::copy(&found[0], &model).unwrap();
    std::fs::copy(&found[1], &texture).unwrap();
    state.add_clothes(vec![model, texture]);
    until(&mut state, "the garment", |s| garments(s) == 1);
    // ⚠ A file of the corpus may state more than a limit allows (the smallest dictionary here is a bike's, 288 pixels square with one mip), which is a warning and stops no build.
    until(&mut state, "a plan with no error", |s| {
        (s.workbench.findings().iter())
            .all(|f| f.severity != crate::state::workbench::Severity::Error)
            && !s.workbench.is_blocked("fivem")
    });

    state.build_selected();
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running() && s.workbench.builds().last("fivem").is_some()
    });

    let Some(LastBuild::Built(done)) = state.workbench.builds().last("fivem") else {
        panic!("{:?}", state.workbench.builds().last("fivem"));
    };
    assert!(
        done.pack_problems().is_empty(),
        "{:?}",
        done.pack_problems()
    );
    let names: Vec<String> = (done.parts.iter())
        .flat_map(|p| &p.pack_checks)
        .flat_map(|(_, checks)| checks.iter().map(|c| c.name.clone()))
        .collect();
    assert_eq!(
        names,
        [
            format!("{} variation metadata", or_wardrobe::Ped::Male),
            format!("{} shop metadata", or_wardrobe::Ped::Male),
            "garment behaviour".to_string()
        ],
        "the checks of a pack with one male garment, which states no expression for the creature file"
    );
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Built);
}

/// Every file under `dir` with the bytes it holds, to say that nothing was written there.
fn held(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path.strip_prefix(root).unwrap().to_string_lossy();
                out.insert(name.replace('\\', "/"), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// The project `brand` holds a pack of two garments and the files of both, and the project `police` links that
/// pack; `police` is open in the state. A tangled pack pins its garments at one place, has the same bytes in both
/// models and records one waiver, so the engine has findings with a fix and a waiver to offer.
fn linked(dir: &Path, tangled: bool) -> (AppState, PathBuf) {
    let mut brand = NewProject::new(Template::Clothing, dir.to_path_buf());
    brand.set_name("brand".into());
    let owner = brand.create().expect("the owner is made").file;
    let pack = owner.parent().unwrap().join("clothing");
    let dropped = drop_files(
        dir,
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"the same jacket"),
            (
                &format!("{C}^jbib_001_u.ydd"),
                if tangled {
                    b"the same jacket"
                } else {
                    b"another jacket"
                },
            ),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"t0"),
            (
                &format!("{C}^jbib_diff_001_a_uni.ytd"),
                if tangled { b"t0" } else { b"t1" },
            ),
        ],
    );
    or_wardrobe::edit::add_clothes(&pack, &dropped).expect("the owner adds clothes");
    if tangled {
        or_wardrobe::edit::update(&pack, |w| {
            w.garments[0].position = Some(4);
            w.garments[1].position = Some(4);
            let first = w.garments[0].id.clone();
            or_wardrobe::edit::waive(w, "duplicate-content", Some(&first), "the same mesh")
        })
        .expect("the owner pins and waives");
    }

    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut police = NewProject::new(Template::Files, work);
    police.set_name("police".into());
    let file = police.create().expect("the linking project is made").file;
    let mut state = bare();
    state.load_project_from(file);
    state.link_content(&pack).expect("the pack is linked");
    (state, pack)
}

#[test]
fn a_linked_pack_is_shown_in_the_wardrobe_and_every_edit_of_it_is_refused_naming_the_project_that_writes_it(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = linked(dir.path(), true);
    until(&mut state, "the linked pack", |s| {
        (s.wardrobe_pack().and_then(|p| p.read.as_ref().ok())).is_some_and(|r| {
            r.wardrobe.garments.len() == 2 && r.findings.iter().any(|f| f.code == "position-clash")
        })
    });
    let module = state.wardrobe_module_id().unwrap();
    let read = state.wardrobe_pack().unwrap().read.as_ref().unwrap();
    let ids: Vec<String> = read
        .wardrobe
        .garments
        .iter()
        .map(|g| g.id.clone())
        .collect();
    let clash = read
        .findings
        .iter()
        .find(|f| f.code == "position-clash")
        .cloned()
        .unwrap();
    assert_eq!(
        read.wardrobe.waivers.len(),
        1,
        "the waiver is there to be removed"
    );
    let before = held(&pack);
    assert!(before.len() >= 5, "{:?}", before.keys());

    // The findings of the pack are listed, and none offers a fix or a waiver that would be refused.
    let listed: Vec<_> = (state.workbench.findings().iter())
        .filter(|f| matches!(f.place, Place::Garment { .. }))
        .collect();
    assert!(
        listed.iter().any(|f| f.text.contains("pinned at position")),
        "{listed:?}"
    );
    assert!(
        listed.iter().all(|f| f.fixes.is_empty()),
        "a linked pack offers no fix: {listed:?}"
    );

    let more = drop_files(dir.path(), &[(&format!("{C}^lowr_000_r.ydd"), b"trousers")]);
    type Try<'a> = (&'a str, Box<dyn Fn(&mut AppState)>);
    let tries: Vec<Try> = vec![
        ("add clothes", {
            let more = more.clone();
            Box::new(move |s| s.add_clothes(more.clone()))
        }),
        ("move a garment", {
            let id = ids[0].clone();
            Box::new(move |s| s.edit_garment(id.clone(), Edit::Position(Some(9))))
        }),
        ("remove a garment", {
            let id = ids[1].clone();
            Box::new(move |s| s.edit_garment(id.clone(), Edit::Remove))
        }),
        ("fix a finding", {
            let (module, clash) = (module.clone(), clash.clone());
            Box::new(move |s| s.fix_finding(module.clone(), clash.clone()))
        }),
        ("waive a finding", {
            let (module, id) = (module.clone(), ids[1].clone());
            Box::new(move |s| {
                s.waive_finding(
                    module.clone(),
                    "duplicate-content".into(),
                    Some(id.clone()),
                    "on purpose".into(),
                )
            })
        }),
        ("remove a waiver", Box::new(|s| s.remove_waiver(0))),
        (
            "name the shop entries as Durty does",
            Box::new(|s| s.set_durty_hashes(true)),
        ),
    ];
    for (what, attempt) in &tries {
        state.toasts.clear();
        attempt(&mut state);
        until(&mut state, what, |s| s.workbench.wardrobe.is_idle());
        let said: Vec<String> = state.toasts.iter().map(|t| t.message.clone()).collect();
        assert!(
            said.iter()
                .any(|m| m.contains("brand.openrage-proj") && m.contains("only reads it")),
            "{what}: the refusal names the project that writes the pack, and not {said:?}"
        );
        assert_eq!(
            held(&pack),
            before,
            "{what} wrote into a pack another project holds"
        );
    }
}

/// A resource that only has names: a jacket and its texture of the pack `harbor`, and a script.
fn resource(folder: &Path) {
    use or_wardrobe::{layout, Component, Ped, Skin, Slot};
    let slot = Slot::Component(Component::Jbib);
    let directory = layout::directory("harbor", Ped::Male, "mp_m_harbor", slot);
    let model = layout::model_name(Ped::Male, "mp_m_harbor", slot, 0, Skin::Universal);
    let texture = layout::texture_name(Ped::Male, "mp_m_harbor", slot, 0, 0, "uni");
    write(&folder.join(&directory).join(model), b"model");
    write(&folder.join(&directory).join(texture), b"texture");
    write(&folder.join("client/main.lua"), b"print('hi')");
}

/// The problems and the file paths of the plan of build `output`, as the last scan left them.
fn planned_of(state: &AppState, output: &str) -> (Vec<String>, Vec<String>) {
    let resources = state.workbench.resources_of(output);
    (
        (resources.iter())
            .flat_map(|r| r.plan.problems.clone())
            .collect(),
        (resources.iter())
            .flat_map(|r| r.plan.files.iter().map(|f| f.path.clone()))
            .collect(),
    )
}

#[test]
fn clothing_that_was_adopted_or_linked_is_planned_by_the_app_and_not_refused_as_content_nobody_contributed_for(
) {
    // Adopted: a folder that holds a pack becomes clothing content beside the files of its resource.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("harbor_resource");
    resource(&folder);
    let created = NewProject::adopting(&folder).create().expect("adopted");
    let mut state = bare();
    state.load_project_from(created.file);
    until(&mut state, "the adopted pack", |s| {
        (s.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
            .is_some_and(|r| r.wardrobe.garments.len() == 1)
            && !s.workbench.resources_of("fivem").is_empty()
    });
    let (problems, files) = planned_of(&state, "fivem");
    assert!(problems.is_empty(), "{problems:?}");
    assert!(files.iter().any(|f| f.ends_with(".ydd")), "{files:?}");
    assert!(
        files.iter().any(|f| f.ends_with("client/main.lua")),
        "the Files content still ships its scripts: {files:?}"
    );
    assert!(state
        .workbench
        .findings()
        .iter()
        .all(|f| !f.text.contains("nothing contributed")));
    assert_ne!(state.workbench.dot("fivem"), BuildDot::Blocked);

    // Linked: a pack another project holds is read into this project's builds.
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = linked(dir.path(), false);
    until(&mut state, "the linked pack", |s| {
        (s.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
            .is_some_and(|r| r.wardrobe.garments.len() == 2)
            && !s.workbench.resources_of("fivem").is_empty()
    });
    let (problems, files) = planned_of(&state, "fivem");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        files.iter().filter(|f| f.ends_with(".ydd")).count(),
        2,
        "{files:?}"
    );
    assert_ne!(state.workbench.dot("fivem"), BuildDot::Blocked);
}

#[test]
fn a_card_counts_the_garments_of_a_clothing_pack_and_a_pack_that_cannot_be_read_is_clothing_without_a_count(
) {
    use super::front_door::cards::read_card;
    let dir = tempfile::tempdir().unwrap();
    let mut draft = NewProject::new(Template::Clothing, dir.path().to_path_buf());
    draft.set_name("harbor".into());
    let file = draft.create().expect("made").file;
    let pack = file.parent().unwrap().join("clothing");
    assert_eq!(read_card(&file).content, ["Clothing · 0 garments"]);

    let dropped = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^lowr_000_r.ydd"), b"trousers"),
        ],
    );
    or_wardrobe::edit::add_clothes(&pack, &dropped).expect("two garments");
    assert_eq!(read_card(&file).content, ["Clothing · 2 garments"]);

    std::fs::write(pack.join(or_wardrobe::FILE), b"not json").unwrap();
    assert_eq!(read_card(&file).content, ["Clothing"]);
}

#[test]
fn giving_a_project_clothing_never_writes_into_a_folder_another_project_holds() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut police = NewProject::new(Template::Files, work);
    police.set_name("police".into());
    let file = police.create().expect("made").file;
    let mut state = bare();
    state.load_project_from(file.clone());

    // A link whose folder is the one a new clothing content would take (a hand-edited project file can state it).
    let taken = file.parent().unwrap().join("clothing");
    std::fs::create_dir_all(&taken).unwrap();
    state.project_carried.container.modules.push(Module::linked(
        "theirs",
        or_project::ModuleKind::Clothing,
        "clothing",
    ));
    let modules = state.project_carried.container.modules.len();

    state.add_clothing_content();

    assert!(
        !taken.join(or_wardrobe::FILE).exists(),
        "a pack was written into a folder that is linked content"
    );
    assert_eq!(state.project_carried.container.modules.len(), modules);
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("linked content `theirs`")),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
}

/// The folder of the clothing content `module` of the project whose file is `file`.
pub(super) fn folder_of(file: &Path, module: &str) -> PathBuf {
    file.parent().unwrap().join(module)
}

/// The project `tshirts` with a jacket in its pack, and a second clothing content with a jacket in its own.
fn two_packs(dir: &Path) -> (AppState, PathBuf, String) {
    let (mut state, file) = opened(dir);
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    let jacket = |tag: &str| {
        drop_files(
            &dir.join(tag),
            &[
                (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
                (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            ],
        )
    };
    state.add_clothes(jacket("one"));
    until(&mut state, "the first jacket", |s| garments(s) == 1);
    state.add_clothing_content();
    let second = state.wardrobe_module_id().expect("the second content");
    assert_ne!(second, "clothing");
    state.add_clothes(jacket("two"));
    until(&mut state, "both packs", both_packs_hold_a_garment);
    (state, file, second)
}

fn both_packs_hold_a_garment(state: &AppState) -> bool {
    state.workbench.packs().len() == 2
        && (state.workbench.packs().iter()).all(|p| {
            p.read
                .as_ref()
                .is_ok_and(|r| r.wardrobe.garments.len() == 1)
        })
}

#[test]
fn a_second_clothing_content_is_given_a_pack_name_no_other_pack_has_so_two_filled_packs_do_not_clash(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file, second) = two_packs(dir.path());

    let pack = |module: &str| {
        or_wardrobe::Wardrobe::read(&folder_of(&file, module))
            .unwrap()
            .pack
    };
    assert_ne!(pack("clothing"), pack(&second), "one name for two packs");
    let output = state.project_carried.container.outputs[0].id.clone();
    until(&mut state, "the plan", |s| {
        !s.workbench.resources_of(&output).is_empty()
    });
    let (problems, files) = planned_of(&state, &output);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        files.iter().filter(|f| f.ends_with(".ydd")).count(),
        2,
        "{files:?}"
    );
}

#[test]
fn a_pack_is_renamed_from_the_wardrobe_and_a_name_another_pack_has_or_no_pack_can_have_is_refused()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file, second) = two_packs(dir.path());
    let folder = folder_of(&file, &second);
    let pack_of = |folder: &Path| or_wardrobe::Wardrobe::read(folder).unwrap().pack;
    let first = pack_of(&folder_of(&file, "clothing"));

    state.rename_pack("uniforms".into());
    until(&mut state, "the new name", |_| {
        pack_of(&folder) == "uniforms"
    });

    for (name, why) in [
        (first.as_str(), "already"),
        ("Not A Name", "not a pack name"),
    ] {
        state.toasts.clear();
        state.rename_pack(name.into());
        until(&mut state, name, |s| s.workbench.wardrobe.is_idle());
        assert!(
            state.toasts.iter().any(|t| t.message.contains(why)),
            "`{name}`: {:?}",
            state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
        assert_eq!(pack_of(&folder), "uniforms", "`{name}` changed the pack");
    }
}

/// A build that splits between two clothing contents reads each resource back against the pack it holds.
#[test]
fn a_split_build_of_two_packs_reads_each_resource_back_against_its_own_pack() {
    let Some(found) = a_drawable_and_a_texture(
        "a_split_build_of_two_packs_reads_each_resource_back_against_its_own_pack",
    ) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    let pair = |tag: &str| {
        let drop = dir.path().join(tag);
        std::fs::create_dir_all(&drop).unwrap();
        let model = drop.join(format!("{C}^jbib_000_u.ydd"));
        let texture = drop.join(format!("{C}^jbib_diff_000_a_uni.ytd"));
        std::fs::copy(&found[0], &model).unwrap();
        std::fs::copy(&found[1], &texture).unwrap();
        vec![model, texture]
    };
    state.add_clothes(pair("one"));
    until(&mut state, "the first pack", |s| garments(s) == 1);
    state.add_clothing_content();
    state.add_clothes(pair("two"));
    until(&mut state, "both packs", both_packs_hold_a_garment);
    // One drawable is all a resource may hold, so the build divides between the two contents.
    let output = state.project_carried.container.outputs[0].id.clone();
    {
        let declared = &mut state.project_carried.container.outputs[0];
        declared.split = true;
        declared.limits.insert("clothing_drawables".into(), 1);
    }
    state.workbench.want_scan();
    until(&mut state, "a plan in two resources", |s| {
        s.workbench.resources_of(&output).len() == 2
    });
    assert!(!state.workbench.is_blocked(&output));

    state.build_selected();
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running() && s.workbench.builds().last(&output).is_some()
    });

    let Some(LastBuild::Built(done)) = state.workbench.builds().last(&output) else {
        panic!("{:?}", state.workbench.builds().last(&output));
    };
    assert_eq!(done.parts.len(), 2);
    let checked: Vec<Vec<&str>> = (done.parts.iter())
        .map(|part| part.pack_checks.iter().map(|(m, _)| m.as_str()).collect())
        .collect();
    assert!(
        checked.iter().all(|modules| modules.len() == 1) && checked[0] != checked[1],
        "each resource is read back against the one pack it holds: {checked:?}"
    );
    assert!(
        done.pack_problems().is_empty(),
        "{:?}",
        done.pack_problems()
    );
    assert_eq!(state.workbench.dot(&output), BuildDot::Built);
}
