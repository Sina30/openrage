//! What the app knows of a project that holds addon peds: the card on the start page, the check of a
//! build against the model, and a folder of peds linked from another project.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a
//! worker for each scan and build that a case waits on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_addonped::{AddonPeds, Ped, PedFile, Waiver};
use or_project::{Container, Module, ModuleKind, Output, Project};

use super::front_door::cards::read_card;
use super::front_door::{NewProject, Template};
use super::workbench::{BuildDot, LastBuild};
use super::{AppState, Prefs};

/// The bytes of a ped's model and of its textures.
struct Staged {
    model: Vec<u8>,
    textures: Vec<u8>,
}

/// Bytes no reader opens: enough for a card and a plan, which only count and hash.
fn junk() -> Staged {
    Staged {
        model: b"a model".to_vec(),
        textures: b"textures".to_vec(),
    }
}

/// A model and a texture dictionary of the corpus; none where this machine has no corpus, and the test is skipped.
fn corpus(test: &str) -> Option<Staged> {
    use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};
    let read = |extension: &str, prefer: &str| {
        let files = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), extension)
            .target(TARGET_GEN8)
            .find_or_skip(test)?;
        let one = files
            .iter()
            .find(|f| f.to_string_lossy().contains(prefer))
            .or(files.first())?;
        std::fs::read(one).ok()
    };
    Some(Staged {
        model: read("yft", "hipster")?,
        textures: read("ytd", "jbib_diff")?,
    })
}

fn bare() -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state
}

fn put(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A data file with an entry for each of `names`.
fn peds_meta(names: &[&str]) -> String {
    let items: String = names
        .iter()
        .map(|n| format!("    <Item>\n      <Name>{n}</Name>\n    </Item>\n"))
        .collect();
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CPedModelInfo__InitDataList>\n  <InitDatas>\n{items}  </InitDatas>\n</CPedModelInfo__InitDataList>\n")
}

/// `dir/peds.openrage-proj`: one addon ped content in `modules/peds` with a ped for each of `names`, a data file that defines `defined`, and one build.
fn project_of_peds(dir: &Path, names: &[&str], defined: &[&str], staged: &Staged) -> PathBuf {
    let folder = dir.join("modules").join("peds");
    let mut model = AddonPeds::new();
    model.meta = Some("peds.meta".into());
    for name in names {
        let mut ped = Ped::new(name);
        for (file, bytes) in [
            (format!("{name}.yft"), &staged.model),
            (format!("{name}.ytd"), &staged.textures),
        ] {
            put(&folder.join(&file), bytes);
            ped.files.push(PedFile {
                path: file,
                at: None,
            });
        }
        model.peds.push(ped);
    }
    put(&folder.join("peds.meta"), peds_meta(defined).as_bytes());
    model.write(&folder).unwrap();
    let container = Container {
        modules: vec![Module::addon_ped("peds", "modules/peds")],
        outputs: vec![Output::fivem_resource("fivem", "ped_pack", &["peds"])],
    };
    let file = dir.join("peds.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    file
}

/// Polls the workbench until `done`, which a worker's answer makes true.
fn until(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        state.poll_workbench();
        if done(state) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_card_counts_the_peds_of_an_addon_ped_content() {
    let dir = tempfile::tempdir().unwrap();
    let file = project_of_peds(
        dir.path(),
        &["my_ped", "other"],
        &["my_ped", "other"],
        &junk(),
    );
    assert_eq!(read_card(&file).content, ["Addon ped · 2 peds"]);
}

#[test]
fn a_card_says_addon_ped_without_a_count_when_the_model_cannot_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = project_of_peds(dir.path(), &["my_ped"], &["my_ped"], &junk());
    std::fs::write(
        dir.path().join("modules/peds").join(or_addonped::FILE),
        b"not json",
    )
    .unwrap();
    assert_eq!(read_card(&file).content, ["Addon ped"]);
}

#[test]
fn a_card_says_link_for_addon_peds_another_project_holds() {
    let dir = tempfile::tempdir().unwrap();
    let container = Container {
        modules: vec![Module::linked(
            "theirs",
            ModuleKind::AddonPed,
            "../elsewhere/modules/peds",
        )],
        outputs: Vec::new(),
    };
    let file = dir.path().join("linked.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    assert_eq!(read_card(&file).content, ["Addon ped · linked"]);
}

#[test]
fn a_folder_of_peds_another_project_holds_is_linked_as_addon_ped_content_and_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("owner");
    project_of_peds(&owner, &["my_ped"], &["my_ped"], &junk());
    let folder = owner.join("modules").join("peds");
    let before = std::fs::read(folder.join(or_addonped::FILE)).unwrap();

    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut draft = NewProject::new(Template::Files, work);
    draft.set_name("police".into());
    let file = draft.create().expect("made").file;
    let mut state = bare();
    state.load_project_from(file.clone());

    let id = state
        .link_content(&folder)
        .expect("a folder of peds is content");
    let linked = state
        .project_carried
        .container
        .module(&id)
        .expect("the link is a module");
    assert!(linked.is_link() && linked.kind == ModuleKind::AddonPed);
    until(&mut state, "the link read", |s| {
        s.workbench.link(&id).is_some()
    });
    let banner = state.workbench.link(&id).unwrap().banner();
    assert!(banner.contains("peds.openrage-proj"), "{banner}");

    state.save_project_to(file.clone()).unwrap();
    assert!(read_card(&file)
        .content
        .contains(&"Addon ped · linked".to_string()));
    assert_eq!(
        std::fs::read(folder.join(or_addonped::FILE)).unwrap(),
        before,
        "the project that links a folder writes nothing in it"
    );
}

/// Open the project of `names` with a data file that defines `defined`, build its one build, and wait for the build.
fn built(dir: &Path, names: &[&str], defined: &[&str], staged: &Staged, waive: bool) -> AppState {
    let file = project_of_peds(dir, names, defined, staged);
    if waive {
        let folder = dir.join("modules").join("peds");
        let mut model = AddonPeds::read(&folder).unwrap();
        model.waivers.push(Waiver {
            code: "meta-no-entry".into(),
            ped: None,
            reason: "the base game's data file defines it".into(),
        });
        model.write(&folder).unwrap();
    }
    let mut state = bare();
    state.load_project_from(file);
    until(&mut state, "the scan", |s| {
        !s.workbench.resources_of("fivem").is_empty()
    });
    state.build_selected();
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running() && s.workbench.builds().last("fivem").is_some()
    });
    state
}

#[test]
fn a_build_of_addon_peds_is_read_back_against_the_model_and_a_data_file_that_does_not_define_a_ped_fails_that_check(
) {
    let Some(staged) = corpus(
        "a_build_of_addon_peds_is_read_back_against_the_model_and_a_data_file_that_does_not_define_a_ped_fails_that_check",
    ) else {
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let state = built(dir.path(), &["my_ped"], &["my_ped"], &staged, false);
    let Some(LastBuild::Built(done)) = state.workbench.builds().last("fivem") else {
        panic!("{:?}", state.workbench.builds().last("fivem"));
    };
    assert!(
        done.pack_problems().is_empty(),
        "{:?}",
        done.pack_problems()
    );
    let checks: Vec<(String, String)> = (done.parts.iter())
        .flat_map(|p| &p.pack_checks)
        .flat_map(|(module, checks)| checks.iter().map(|c| (module.clone(), c.name.clone())))
        .collect();
    assert_eq!(
        checks,
        [
            ("peds".to_string(), "model of my_ped".to_string()),
            ("peds".to_string(), "data file".to_string())
        ]
    );
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Built);

    // A waiver lets the plan through with a data file that defines someone else: the check of the build still says so.
    let dir = tempfile::tempdir().unwrap();
    let state = built(dir.path(), &["my_ped"], &["someone_else"], &staged, true);
    let Some(LastBuild::Built(done)) = state.workbench.builds().last("fivem") else {
        panic!("{:?}", state.workbench.builds().last("fivem"));
    };
    let problems = done.pack_problems();
    assert!(
        problems.iter().any(
            |p| p.starts_with("peds: data file: ") && p.contains("has no entry named `my_ped`")
        ),
        "{problems:?}"
    );
    assert_eq!(state.workbench.dot("fivem"), BuildDot::Unchecked);
}
