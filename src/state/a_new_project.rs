//! What the New project dialog writes, and what Make this a project does: nothing before Create, exactly the tree shown, and nothing of an adopted folder moved.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use or_project::{ModuleKind, OutputKind};

use super::front_door::cards::read_card;
use super::front_door::new_project::{resource_name_for, GAME_KEY};
use super::front_door::{GameChoice, NewProject, Offer, Template};
use super::project::load_project;
use super::{AppState, DocumentKind, Prefs};
use crate::ui::dock::Workspace;

fn draft(template: Template, place: &Path, name: &str) -> NewProject {
    let mut draft = NewProject::new(template, place.to_path_buf());
    draft.set_name(name.to_string());
    draft
}

/// Every file and folder under `root`, relative and with `/`, folders ending in `/`.
fn walk(root: &Path) -> BTreeSet<String> {
    fn inside(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("a folder").flatten() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("inside the root")
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                out.insert(format!("{relative}/"));
                inside(root, &path, out);
            } else {
                out.insert(relative);
            }
        }
    }
    let mut out = BTreeSet::new();
    inside(root, root, &mut out);
    out
}

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("a folder");
    std::fs::write(path, body).expect("a file");
}

/// Poll as the editor's frame does until the worker that makes a project has answered.
fn until_made(state: &mut AppState) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while state.is_creating() {
        assert!(
            std::time::Instant::now() < until,
            "the project was never made"
        );
        state.poll_creating();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_resource_is_named_after_its_project_in_lower_case_and_underscores() {
    for (project, resource) in [
        ("Police Pack", "police_pack"),
        ("police", "police"),
        ("  Los Santos -- 2.0!  ", "los_santos_2_0"),
        ("Éclair", "clair"),
        ("!!!", "resource"),
        ("", "resource"),
    ] {
        assert_eq!(resource_name_for(project), resource, "{project:?}");
        assert!(or_project::container::is_name(&resource_name_for(project)));
    }

    let mut typed = draft(Template::Files, Path::new("."), "Police Pack");
    assert_eq!(
        typed.resource, "police_pack",
        "the name fills the resource in"
    );
    typed.set_resource("pd".into());
    typed.set_name("Sheriff".into());
    assert_eq!(typed.resource, "pd", "a resource that was typed stays");
}

#[test]
fn nothing_is_written_before_create() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for template in Template::ALL {
        let draft = draft(template, dir.path(), "police");
        let _ = (
            draft.refusal(),
            draft.plan(),
            draft.tree(),
            draft.container(),
        );
        assert!(
            walk(dir.path()).is_empty(),
            "{template:?}: showing the dialog wrote something"
        );
    }
}

#[test]
fn the_tree_the_dialog_shows_is_exactly_what_create_writes() {
    for template in [Template::Empty, Template::Files, Template::Clothing] {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let draft = draft(template, dir.path(), "police");
        // A line of depth 2 is inside the folder above it of depth 1, so each line is read as the path it names.
        let mut folders: Vec<String> = Vec::new();
        let shown: BTreeSet<String> = draft
            .tree()
            .into_iter()
            .filter(|line| line.depth > 0)
            .map(|line| {
                folders.truncate(line.depth - 1);
                let path = format!("police/{}{}", folders.concat(), line.name);
                if line.is_folder {
                    folders.push(format!("{}/", line.name));
                    format!("{path}/")
                } else {
                    path
                }
            })
            .chain(["police/".to_string()])
            .collect();

        assert!(draft.tree().iter().all(|line| line.new), "{template:?}");

        let created = draft.create().expect("the project is made");

        assert_eq!(walk(dir.path()), shown, "{template:?}");
        assert_eq!(
            created.file,
            dir.path().join("police").join("police.openrage-proj")
        );
    }
}

#[test]
fn a_project_is_refused_with_a_sentence_and_nothing_is_written() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    write(&dir.path().join("taken/readme.txt"), b"mine");
    let before = walk(dir.path());

    let mut refused = Vec::new();
    for (name, place, resource) in [
        ("", dir.path().to_path_buf(), "police"),
        ("a/b", dir.path().to_path_buf(), "police"),
        ("police", dir.path().join("not-there"), "police"),
        ("taken", dir.path().to_path_buf(), "taken"),
        ("police", dir.path().to_path_buf(), "bad/name"),
    ] {
        let mut draft = draft(Template::Files, &place, name);
        draft.set_resource(resource.into());
        let why = draft
            .refusal()
            .unwrap_or_else(|| panic!("{name:?} was allowed"));
        assert!(draft.create().is_err());
        refused.push(why);
    }

    assert_eq!(
        walk(dir.path()),
        before,
        "a refused project wrote something"
    );
    assert!(
        refused[3].contains("already exists and is not empty"),
        "{refused:?}"
    );
}

#[test]
fn an_empty_folder_of_that_name_is_used_rather_than_refused() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::create_dir(dir.path().join("police")).expect("an empty folder");
    let draft = draft(Template::Files, dir.path(), "police");
    assert_eq!(draft.refusal(), None);
    assert!(!draft.tree()[0].new, "the folder was there already");
    draft.create().expect("made in the empty folder");
    assert!(dir.path().join("police/police.openrage-proj").is_file());
}

#[test]
fn the_templates_declare_what_they_say_and_open_on_the_project_workspace() {
    let dir = tempfile::tempdir().expect("a scratch folder");

    let mut empty = draft(Template::Empty, dir.path(), "nothing");
    empty.game = GameChoice::Legacy;
    let made = empty.create().expect("an empty project");
    let loaded = load_project(&made.file).expect("it reads");
    assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
    assert_eq!(loaded.project.workspace, Workspace::Project);
    assert!(loaded.project.container.modules.is_empty());
    assert!(loaded.project.container.outputs.is_empty());
    assert_eq!(loaded.project.extra.get(GAME_KEY), Some(&"legacy".into()));

    let mut files = draft(Template::Files, dir.path(), "Police Pack");
    files.game = GameChoice::Both;
    let made = files.create().expect("a Files project");
    let loaded = load_project(&made.file).expect("it reads");
    assert_eq!(loaded.project.workspace, Workspace::Project);
    let container = &loaded.project.container;
    assert_eq!(container.modules.len(), 1);
    assert_eq!(container.modules[0].kind, ModuleKind::Files);
    assert_eq!(container.outputs.len(), 1);
    assert_eq!(container.outputs[0].kind, OutputKind::FivemResource);
    assert_eq!(container.outputs[0].name, "police_pack");
    assert_eq!(container.outputs[0].take, ["files"]);
    assert_eq!(loaded.project.extra.get(GAME_KEY), Some(&"both".into()));
    assert!(
        dir.path().join("Police Pack/sources").is_dir(),
        "a sources folder is made beside the project file"
    );
}

#[test]
fn adopting_a_folder_moves_nothing_and_adds_only_the_project_file_and_the_git_files_it_had_none_of()
{
    let dir = tempfile::tempdir().expect("a scratch folder");
    let folder = dir.path().join("police_pack");
    write(&folder.join("fxmanifest.lua"), b"fx_version 'cerulean'");
    write(&folder.join("stream/a.ydd"), b"drawable");
    write(&folder.join("client/main.lua"), b"print('hi')");
    write(&folder.join("logo.textures.src/asset.json"), b"{}");
    write(&folder.join("build/old/fxmanifest.lua"), b"old build");
    write(&folder.join("vfs/common/x.dat"), b"shadow");
    write(&folder.join(".git/config"), b"hidden");
    let before = walk(&folder);
    let bytes_before = std::fs::read(folder.join("stream/a.ydd")).expect("a file");

    let mut adopting = NewProject::new(Template::Adopt, folder.clone());
    adopting.pick_place(&folder);
    assert_eq!(adopting.name, "police_pack");
    assert_eq!(adopting.resource, "police_pack");
    assert_eq!(adopting.refusal(), None);
    assert_eq!(walk(&folder), before, "choosing a folder wrote something");
    let tree = adopting.tree();
    assert!(!tree[0].new, "the folder is shown as it is");
    assert_eq!(
        tree.len(),
        4,
        "three new lines: the project file and the two Git files the folder has none of"
    );

    let created = adopting.create().expect("the folder is adopted");

    let after = walk(&folder);
    let added: Vec<&String> = after.difference(&before).collect();
    assert_eq!(
        added,
        [".gitattributes", ".gitignore", "police_pack.openrage-proj"],
        "only the project file and the Git files"
    );
    assert!(before.is_subset(&after), "something was removed or moved");
    assert_eq!(
        std::fs::read(folder.join("stream/a.ydd")).expect("a file"),
        bytes_before
    );

    let loaded = load_project(&created.file).expect("it reads");
    let held: BTreeSet<PathBuf> = loaded
        .project
        .documents
        .iter()
        .map(|d| d.path.clone())
        .collect();
    let expected: BTreeSet<PathBuf> = ["fxmanifest.lua", "stream/a.ydd", "client/main.lua"]
        .iter()
        .map(|p| folder.join(p))
        .collect();
    assert_eq!(
        held, expected,
        "the folder's files, and not its builds, shadow, sources or hidden folders"
    );
    assert_eq!(created.adopted, 3);
    assert!(!created.truncated);
    assert_eq!(loaded.project.container.modules[0].kind, ModuleKind::Files);
    assert_eq!(loaded.project.container.outputs[0].name, "police_pack");
}

#[test]
fn a_walk_for_an_adopted_folder_is_bounded() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut deep = dir.path().to_path_buf();
    for level in 0..12 {
        deep = deep.join(format!("l{level}"));
        write(&deep.join("file.txt"), b"x");
    }
    let (files, too_deep) = or_project::adopt::files_of(dir.path());
    assert!(!too_deep.is_empty(), "a folder twelve deep is cut");
    assert!(files.len() < 12, "{} files", files.len());
}

#[test]
fn a_folder_that_holds_a_project_is_not_adopted_a_second_time() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let folder = dir.path().join("pack");
    write(&folder.join("a.lua"), b"x");
    let first = NewProject::adopting(&folder)
        .create()
        .expect("adopted once");

    let again = NewProject::adopting(&folder);
    let why = again.refusal().expect("adopted twice");
    assert!(
        why.contains("already holds the project pack.openrage-proj"),
        "{why}"
    );
    assert!(again.create().is_err());
    assert!(first.file.is_file());
}

#[test]
fn creating_a_project_in_the_app_opens_it_on_its_workspace_and_remembers_it() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut state = AppState::from_prefs(Prefs::default());
    state.start_new_project(Template::Files);
    let draft = state
        .front_door
        .new_project
        .as_mut()
        .expect("the dialog is open");
    draft.place = dir.path().display().to_string();
    draft.set_name("police".into());

    let draft = state
        .front_door
        .new_project
        .clone()
        .expect("the dialog is open");
    state.start_creating(draft);
    until_made(&mut state);

    let file = dir.path().join("police").join("police.openrage-proj");
    assert!(state.front_door.new_project.is_none(), "the dialog closed");
    assert_eq!(state.project_path.as_deref(), Some(file.as_path()));
    assert_eq!(state.active_workspace, Workspace::Project);
    assert_eq!(state.prefs.recent_projects.first(), Some(&file));
    assert_eq!(state.prefs.last_project.as_deref(), Some(file.as_path()));
}

#[test]
fn a_folder_to_adopt_is_never_a_guess_and_a_folder_that_was_typed_stays() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut draft = NewProject::new(Template::Adopt, dir.path().to_path_buf());
    assert_eq!(
        draft.place, "",
        "the default place is where a project is made, not what is adopted"
    );
    draft.set_name("pack".into());
    let why = draft.refusal().expect("no folder was chosen");
    assert!(
        why.contains("Choose the folder to make a project of"),
        "{why}"
    );

    draft.choose(Template::Files);
    assert_eq!(
        PathBuf::from(&draft.place),
        dir.path(),
        "another template makes its project there"
    );
    draft.choose(Template::Adopt);
    assert_eq!(draft.place, "", "and back");

    draft.place = "D:/mine".into();
    draft.choose(Template::Files);
    assert_eq!(
        draft.place, "D:/mine",
        "a folder that was typed is the user's"
    );
}

#[test]
fn the_dialog_offers_the_folder_the_last_project_was_made_in() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let prefs = Prefs {
        recent_projects: vec![dir.path().join("police").join("police.openrage-proj")],
        ..Prefs::default()
    };
    std::fs::create_dir_all(dir.path().join("police")).expect("a folder");
    let mut state = AppState::from_prefs(prefs);
    state.start_new_project(Template::Empty);
    let draft = state.front_door.new_project.as_ref().expect("open");
    assert_eq!(PathBuf::from(&draft.place), dir.path());
}

#[test]
fn an_opened_folder_offers_to_become_a_project_until_declined_or_a_project_is_open() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let folder = dir.path().join("pack");
    write(&folder.join("a.lua"), b"x");
    let mut state = AppState::from_prefs(Prefs::default());
    assert_eq!(state.adoption_offer(), None, "nothing is open");

    state.open_folder(folder.clone());
    assert_eq!(
        state.active_document().map(|d| d.kind),
        Some(DocumentKind::Folder)
    );
    assert_eq!(state.adoption_offer(), Some(Offer::Make(folder.clone())));

    state.decline_adoption(&folder);
    assert_eq!(state.adoption_offer(), None, "declined");

    let other = dir.path().join("held");
    let held = write_a_project_in(&other);
    let mut state = AppState::from_prefs(Prefs::default());
    state.open_folder(other.clone());
    assert_eq!(
        state.adoption_offer(),
        Some(Offer::Open(held)),
        "a folder that holds a project offers that project"
    );
}

fn write_a_project_in(folder: &Path) -> PathBuf {
    write(&folder.join("a.lua"), b"x");
    NewProject::adopting(folder)
        .create()
        .expect("a project")
        .file
}

#[test]
fn making_an_opened_folder_a_project_moves_nothing_and_opens_the_project() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let folder = dir.path().join("pack");
    write(&folder.join("a.lua"), b"print(1)");
    let before = walk(&folder);
    let mut state = AppState::from_prefs(Prefs::default());
    state.open_folder(folder.clone());

    state
        .make_folder_a_project(&folder)
        .expect("the folder is adopted");
    until_made(&mut state);

    let file = folder.join("pack.openrage-proj");
    assert_eq!(state.project_path.as_deref(), Some(file.as_path()));
    assert!(file.is_file());
    assert!(
        before.is_subset(&walk(&folder)),
        "a file of the folder is gone"
    );
    assert_eq!(
        std::fs::read(folder.join("a.lua")).expect("a file"),
        b"print(1)"
    );
    assert_eq!(state.adoption_offer(), None, "a project is open now");
}

#[test]
fn a_card_says_what_a_project_holds_and_whether_it_is_built() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let files = draft(Template::Files, dir.path(), "police")
        .create()
        .expect("made")
        .file;
    let empty = draft(Template::Empty, dir.path(), "nothing")
        .create()
        .expect("made")
        .file;

    let card = read_card(&files);
    assert!(card.found && card.unreadable.is_none());
    assert_eq!(card.name, "police");
    assert_eq!(card.content, ["Files · 0 files"]);
    assert_eq!(card.builds, "Not built");
    let card = read_card(&empty);
    assert!(card.content.is_empty());
    assert_eq!(card.builds, "No build declared");

    // Built: the project holds a file, is saved, and the build writes the resource and its marker.
    let mut state = AppState::from_prefs(Prefs::default());
    state.load_project_from(files.clone());
    let script = dir.path().join("police").join("client.lua");
    write(&script, b"print('police')");
    assert_eq!(state.add_files_to_project(vec![script]).added, 1);
    assert!(state.save_active_project(|_| None));
    state.build_selected();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while state.workbench.builds().last("fivem").is_none() {
        assert!(std::time::Instant::now() < until, "the build never ended");
        state.poll_workbench();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let card = read_card(&files);
    assert_eq!(card.content, ["Files · 1 file"]);
    assert_eq!(card.builds, "Built");

    let missing = read_card(&dir.path().join("moved").join("moved.openrage-proj"));
    assert!(!missing.found);
    assert_eq!(missing.name, "moved");
    let broken = dir.path().join("broken.openrage-proj");
    write(&broken, b"not json");
    let card = read_card(&broken);
    assert!(card.found && card.unreadable.is_some(), "{card:?}");
}

/// Falsified by looking for the output's own name among the built resources: a split output left
/// `police_1` and `police_2` and the card read `Not built`.
#[test]
fn a_split_output_that_is_built_reads_as_built_on_its_card() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = draft(Template::Files, dir.path(), "police")
        .create()
        .expect("made")
        .file;
    let mut project: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).expect("the project")).expect("json");
    project["outputs"][0]["split"] = serde_json::json!(true);
    write(&file, &serde_json::to_vec(&project).expect("json"));
    let builds = or_project::build_root(&file);
    let marker = |resource: &str| builds.join(format!(".{resource}.openrage-build"));

    // A resource that only starts with the output's name is another's.
    write(&marker("police_x"), b"");
    write(&marker("policeman"), b"");
    assert_eq!(read_card(&file).builds, "Not built");
    write(&marker("police_1"), b"");
    write(&marker("police_2"), b"");
    assert_eq!(read_card(&file).builds, "Built");
}

#[test]
fn what_the_dialog_calls_a_project_is_said_in_the_language_in_use() {
    let _french = crate::i18n::pin("fr");
    let dir = tempfile::tempdir().unwrap();
    for template in [Template::Empty, Template::Files, Template::Clothing] {
        let nameless = draft(template, dir.path(), "");
        let asks = nameless.refusal().expect("a name is asked for");
        let reserved = draft(template, dir.path(), "a:b");
        let refuses = reserved.refusal().expect("a reserved name is refused");
        for said in [asks, refuses] {
            assert!(
                !said.contains("project") && !said.contains("clothing pack"),
                "{template:?} says a word of English in French: {said}"
            );
        }
        // The tree stands in `<name>` for the name not typed yet, as the folder and the project file both.
        let tree = nameless.tree();
        assert!(
            !tree.iter().any(|line| line.name.contains("<name>")),
            "{template:?} stands in an English name: {:?}",
            tree.iter().map(|line| &line.name).collect::<Vec<_>>()
        );
    }
}

#[test]
fn the_builds_of_a_template_file_are_said_in_the_language_in_use() {
    use or_project::template::Template as File;
    use or_project::{Container, Module, Output};

    let _french = crate::i18n::pin("fr");
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::fivem_resource("fivem", "police_pack", &["files"])],
    };
    let (template, findings) = File::from_project("Police pack", "", &container, None, None, None);
    let mut draft = NewProject::new(Template::Files, PathBuf::new());
    draft.offer(super::front_door::Loaded {
        template,
        from: None,
        findings,
    });
    let said = draft.builds();
    assert!(
        said.starts_with("Ressource FiveM") && !said.contains(" named "),
        "{said}"
    );
}
