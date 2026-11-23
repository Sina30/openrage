//! Sharing a project: the Git files a project starts with, and what Check before sharing finds in a project
//! folder that is about to be sent.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys.

use std::path::Path;

use or_source::git;

use super::front_door::{NewProject, Template};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn draft(template: Template, place: &Path, name: &str) -> NewProject {
    let mut draft = NewProject::new(template, place.to_path_buf());
    draft.set_name(name.to_string());
    draft
}

#[test]
fn a_new_project_starts_with_the_ignore_and_attribute_blocks_and_a_second_write_changes_nothing() {
    for template in [Template::Empty, Template::Files, Template::Clothing] {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let created = draft(template, dir.path(), "police")
            .create()
            .expect("the project is made");
        let root = created.file.parent().unwrap().to_path_buf();

        let ignore = read(&root.join(".gitignore"));
        let attributes = read(&root.join(".gitattributes"));
        let lines = |text: &str| text.lines().map(str::to_string).collect::<Vec<_>>();
        for line in [git::BEGIN, "/build/", "/vfs/", git::END] {
            assert!(
                lines(&ignore).iter().any(|l| l == line),
                "{template:?}: {line} is not in\n{ignore}"
            );
        }
        for line in [git::BEGIN, "*.dds binary", "base.ydr binary", git::END] {
            assert!(
                lines(&attributes).iter().any(|l| l == line),
                "{template:?}: {line} is not in\n{attributes}"
            );
        }

        assert!(
            !git::update(&root).unwrap(),
            "{template:?}: a second write found a change to make"
        );
        assert_eq!(read(&root.join(".gitignore")), ignore);
        assert_eq!(read(&root.join(".gitattributes")), attributes);
    }
}

#[test]
fn an_adopted_folder_with_no_git_files_gets_them_and_one_with_a_gitignore_of_its_own_is_left_as_it_is(
) {
    let plain = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(plain.path().join("a.lua"), b"print(1)").unwrap();
    let mut adopting = NewProject::new(Template::Adopt, plain.path().to_path_buf());
    adopting.pick_place(plain.path());
    adopting.create().expect("the folder is adopted");
    assert!(read(&plain.path().join(".gitignore")).contains("/build/"));
    assert!(read(&plain.path().join(".gitattributes")).contains("*.dds binary"));

    let mine = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(mine.path().join("a.lua"), b"print(1)").unwrap();
    std::fs::write(mine.path().join(".gitignore"), "node_modules/\r\n*.log\r\n").unwrap();
    let mut adopting = NewProject::new(Template::Adopt, mine.path().to_path_buf());
    adopting.pick_place(mine.path());
    assert!(
        adopting
            .tree()
            .iter()
            .all(|line| line.name != ".gitignore" && line.name != ".gitattributes"),
        "the dialog promises no Git file where the folder has one of its own"
    );
    adopting.create().expect("the folder is adopted");
    assert_eq!(
        read(&mine.path().join(".gitignore")),
        "node_modules/\r\n*.log\r\n",
        "a person's own .gitignore is not rewritten when a folder is adopted"
    );
    assert!(!mine.path().join(".gitattributes").exists());
}

mod the_check {
    use super::super::every_add_lands_where_the_tree_shows_it::opened as open_folder;
    use super::super::importing_game_files_from_the_app::{
        imported, project, project_and_install, until, write,
    };
    use super::super::opening_a_ydd::node_at;
    use super::super::workbench::{Concern, ConcernKind, Question};
    use super::super::{AppState, Prefs};
    use super::*;

    fn asked(state: &AppState) -> Vec<Concern> {
        match &state.workbench.asking {
            Some(Question::Sharing { concerns }) => concerns.clone(),
            other => panic!("Check before sharing asked nothing: {other:?}"),
        }
    }

    fn of_kind(concerns: &[Concern], kind: ConcernKind) -> Vec<&Concern> {
        concerns.iter().filter(|c| c.kind == kind).collect()
    }

    /// Ask for the check as a person does, with no wait of the test's own for a scan, and read what it lists once the window shows it.
    fn checked(state: &mut AppState) -> Vec<Concern> {
        state.check_before_sharing();
        assert!(
            matches!(state.workbench.asking, Some(Question::SharingRead { .. })),
            "the window waits for the folders to be read"
        );
        until(state, "the check reading the folders", |s| {
            matches!(s.workbench.asking, Some(Question::Sharing { .. }))
        });
        asked(state)
    }

    /// A source taken from the game, written from outside the app: only its `asset.json`, so the scan could not read it.
    fn write_a_source_of_the_game(folder: &Path, name: &str) {
        use or_source::asset::{AssetManifest, How, Origin, MANIFEST};

        let origin = Origin {
            how: How::Extracted,
            from_game: true,
            virtual_path: Some(format!("common.rpf/{name}.ytd")),
            game_build: None,
            sha256: None,
            cooked_sha256: None,
            extra: Default::default(),
        };
        let manifest = AssetManifest::new("textures", name, "gen8", origin).unwrap();
        write(
            &folder
                .join("sources")
                .join(format!("{name}.textures.src"))
                .join(MANIFEST),
            serde_json::to_string(&manifest).unwrap().as_bytes(),
        );
    }

    /// A project holding `a`, a source taken from the game, linked to the project in `shared`, and saved.
    fn a_project_that_is_not_all_its_own(dir: &Path) -> AppState {
        let (mut state, _, _) = project_and_install(dir);
        let doc = open_folder(&mut state, dir);
        let entry = node_at(&mut state, doc, "GTAV/x64/pack.rpf/a.ytd");
        state.import_rows(doc, &[entry]);
        imported(&mut state);
        until(&mut state, "the source", |s| {
            s.workbench
                .rows()
                .iter()
                .any(|r| r.name == "a" && r.from_game)
        });
        let shared = draft(Template::Files, dir, "shared")
            .create()
            .expect("the other project");
        state
            .link_folder(shared.file.parent().unwrap())
            .expect("the other project is linked");
        assert!(state.save_active_project(|_| None));
        state
    }

    #[test]
    fn it_flags_a_source_of_the_game_a_path_of_this_machine_and_the_folder_of_this_machine() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let mut state = a_project_that_is_not_all_its_own(dir.path());
        let root = state
            .project_path
            .clone()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(
            root.join(".openrage").is_dir(),
            "the changes of a session leave a history beside the project"
        );

        let found = checked(&mut state);

        let paths = of_kind(&found, ConcernKind::MachinePath);
        assert_eq!(paths.len(), 1, "{found:?}");
        assert!(
            paths[0].text.contains("modules[") && paths[0].text.contains("shared"),
            "the line says where in the file and which path: {}",
            paths[0].text
        );
        let game = of_kind(&found, ConcernKind::GameContent);
        assert_eq!(game.len(), 1, "{found:?}");
        assert!(
            game[0].text.contains("sources/a.textures.src"),
            "{}",
            game[0].text
        );
        assert_eq!(
            of_kind(&found, ConcernKind::PrivateFolder).len(),
            1,
            "{found:?}"
        );
        assert!(
            of_kind(&found, ConcernKind::Unsaved).is_empty(),
            "the project was saved: {found:?}"
        );
    }

    #[test]
    fn a_change_the_file_does_not_hold_is_said_because_the_file_is_what_gets_sent() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let mut state = a_project_that_is_not_all_its_own(dir.path());
        state.project_carried.changed = true;

        let found = checked(&mut state);

        assert_eq!(of_kind(&found, ConcernKind::Unsaved).len(), 1);
    }

    #[test]
    fn a_project_of_files_alone_has_no_path_and_no_game_content_to_flag() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let (mut state, _) = project(dir.path());

        let found = checked(&mut state);

        assert!(
            of_kind(&found, ConcernKind::MachinePath).is_empty(),
            "{found:?}"
        );
        assert!(
            of_kind(&found, ConcernKind::GameContent).is_empty(),
            "{found:?}"
        );
    }

    #[test]
    fn a_source_of_the_game_that_the_scan_could_not_read_is_flagged_all_the_same() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let (mut state, folder) = project(dir.path());
        write_a_source_of_the_game(&folder, "lamp");
        state.workbench.want_scan();
        until(&mut state, "the row of the broken source", |s| {
            s.workbench.rows().iter().any(|r| r.name == "lamp")
        });
        let row = state
            .workbench
            .rows()
            .iter()
            .find(|r| r.name == "lamp")
            .unwrap();
        assert!(
            !row.from_game,
            "the scan read no payload, so it says nothing of the origin"
        );

        let found = checked(&mut state);

        let game = of_kind(&found, ConcernKind::GameContent);
        assert_eq!(game.len(), 1, "{found:?}");
        assert!(
            game[0].text.contains("sources/lamp.textures.src"),
            "{}",
            game[0].text
        );
    }

    #[test]
    fn a_reference_that_stores_a_path_of_this_machine_is_flagged_with_the_file_that_holds_it() {
        use or_project::reference::Reference;

        let dir = tempfile::tempdir().expect("a scratch folder");
        let (mut state, folder) = project(dir.path());
        let absolute = Reference::other("C:\\Work\\other", "sources/lamp.textures.src", None);
        write(
            &folder.join("cloth").join("jacket.ref.json"),
            absolute.to_text().as_bytes(),
        );
        // A reference to a file of this project, an id and a reference to a sibling project name no place of this machine's.
        write(
            &folder.join("cloth").join("here.ref.json"),
            Reference::here("sources/a.textures.src", None)
                .to_text()
                .as_bytes(),
        );
        write(
            &folder.join("cloth").join("typed.ref.json"),
            Reference::by_id("jbib_001_u").to_text().as_bytes(),
        );

        let found = checked(&mut state);

        let paths = of_kind(&found, ConcernKind::MachinePath);
        assert_eq!(paths.len(), 1, "{found:?}");
        assert!(
            paths[0].text.contains("cloth/jacket.ref.json")
                && paths[0].text.contains("C:\\Work\\other"),
            "the line names the file and the path: {}",
            paths[0].text
        );
    }

    #[test]
    fn a_project_checked_before_its_folders_were_read_is_read_first_and_does_not_pass_for_one_with_nothing_to_flag(
    ) {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let created = draft(Template::Files, dir.path(), "pack")
            .create()
            .expect("the project");
        let folder = created.file.parent().unwrap().to_path_buf();
        write_a_source_of_the_game(&folder, "lamp");
        let mut state = AppState::from_prefs(Prefs::default());
        state.load_project_from(created.file);
        assert!(!state.workbench.is_scanned());

        let found = checked(&mut state);

        assert_eq!(
            of_kind(&found, ConcernKind::GameContent).len(),
            1,
            "{found:?}"
        );
    }

    #[test]
    fn a_source_of_the_game_is_found_by_a_check_made_the_moment_its_import_ends() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let (mut state, _, _) = project_and_install(dir.path());
        let doc = open_folder(&mut state, dir.path());
        let entry = node_at(&mut state, doc, "GTAV/x64/pack.rpf/a.ytd");
        state.import_rows(doc, &[entry]);
        imported(&mut state);

        let found = checked(&mut state);

        let game = of_kind(&found, ConcernKind::GameContent);
        assert_eq!(game.len(), 1, "{found:?}");
        assert!(
            game[0].text.contains("sources/a.textures.src"),
            "{}",
            game[0].text
        );
    }

    #[test]
    fn a_source_of_the_game_written_from_outside_the_app_is_found_though_no_scan_was_asked_for() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let (mut state, folder) = project(dir.path());
        for _ in 0..20 {
            state.poll_workbench();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        write_a_source_of_the_game(&folder, "lamp");
        assert!(
            !state.workbench.rows().iter().any(|r| r.name == "lamp"),
            "the last scan is older than the source"
        );

        let found = checked(&mut state);

        assert_eq!(
            of_kind(&found, ConcernKind::GameContent).len(),
            1,
            "{found:?}"
        );
    }

    #[test]
    fn a_search_that_stopped_at_its_bounds_says_so() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let (mut state, folder) = project(dir.path());
        let deep = (0..12).fold(folder.clone(), |at, n| at.join(format!("d{n}")));
        std::fs::create_dir_all(&deep).unwrap();
        state.workbench.want_scan();
        until(&mut state, "the scan to stop at its bounds", |s| {
            s.workbench.is_truncated()
        });

        let found = checked(&mut state);

        assert_eq!(of_kind(&found, ConcernKind::Bounded).len(), 1);
        let _ = Prefs::default();
    }
}

mod the_zip {
    use std::io::Write;
    use std::path::PathBuf;

    use super::super::importing_game_files_from_the_app::{imported, until, write, ytd};
    use super::super::{AppState, Prefs};
    use super::*;

    const C: &str = "mp_m_freemode_01_mp_m_t";

    /// A project of loose files, a clothing pack of one jacket and a texture source, saved.
    fn a_pack_of_files_clothes_and_a_source(dir: &Path) -> (AppState, PathBuf) {
        let created = draft(Template::Files, dir, "pack")
            .create()
            .expect("the project");
        let root = created.file.parent().unwrap().to_path_buf();
        let mut state = AppState::from_prefs(Prefs::default());
        state.documents.clear();
        state.active_document = None;
        state.load_project_from(created.file);
        until(&mut state, "the first scan", |s| s.workbench.is_scanned());

        state.add_clothing_content();
        let script = root.join("client/main.lua");
        write(&script, b"print('hi')");
        assert_eq!(state.add_files_to_project(vec![script]).added, 1);
        let dropped = [
            write_and_name(&dir.join("drop"), &format!("{C}^jbib_000_u.ydd"), b"jacket"),
            write_and_name(
                &dir.join("drop"),
                &format!("{C}^jbib_diff_000_a_uni.ytd"),
                b"tex",
            ),
        ];
        or_wardrobe::edit::add_clothes(&root.join("clothing"), &dropped).expect("the garment");
        write(&dir.join("downloads/lamp.ytd"), &ytd("lamp", 9));
        state.import_game_files(vec![dir.join("downloads/lamp.ytd")]);
        imported(&mut state);
        assert!(state.save_active_project(|_| None));
        state.workbench.want_scan();
        (state, root)
    }

    fn write_and_name(folder: &Path, name: &str, body: &[u8]) -> PathBuf {
        write(
            &folder.join(name),
            &or_source::sample::as_a_game_file(name, body),
        );
        folder.join(name)
    }

    /// The plan of the `fivem` build once the scan has read the whole pack, in text, with the project's folder written `<project>`.
    fn the_plan(state: &mut AppState, root: &Path) -> String {
        until(
            state,
            "a plan holding the garment, the script and the source",
            |s| {
                let plans = s.workbench.resources_of("fivem");
                plans.first().is_some_and(|r| {
                    let said = format!("{:?}", r.plan.files);
                    said.contains("jbib") && said.contains("main.lua") && said.contains("lamp")
                })
            },
        );
        let plan = format!("{:?}", state.workbench.resources_of("fivem")[0].plan);
        let shown = format!("{:?}", root.to_string_lossy());
        let shown = &shown[1..shown.len() - 1];
        assert!(plan.contains(shown), "the plan names the folder it is in");
        // ⚠ A path read from the file joins its `/` to the folder and one made in the session does not, so the
        // separators are written alike: it is the same file.
        plan.replace(shown, "<project>").replace("\\\\", "/")
    }

    fn zip_folder(folder: &Path, archive: &Path) {
        fn files(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let parent = folder.parent().unwrap();
        let mut found = Vec::new();
        files(folder, &mut found);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(archive).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for path in found {
            let name = path
                .strip_prefix(parent)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options).unwrap();
            zip.write_all(&std::fs::read(&path).unwrap()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn a_project_folder_zipped_and_opened_under_another_path_has_the_plan_it_had() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let (mut state, root) = a_pack_of_files_clothes_and_a_source(dir.path());
        let before = the_plan(&mut state, &root);
        assert!(
            root.join(".openrage").is_dir(),
            "the zip carries this machine's folder too"
        );

        let archive = dir.path().join("pack.zip");
        zip_folder(&root, &archive);
        let elsewhere = dir.path().join("elsewhere");
        zip::ZipArchive::new(std::fs::File::open(&archive).unwrap())
            .unwrap()
            .extract(&elsewhere)
            .unwrap();
        let copy = elsewhere.join("pack");
        assert_ne!(copy, root);

        let mut opened = AppState::from_prefs(Prefs::default());
        opened.documents.clear();
        opened.active_document = None;
        opened.load_project_from(copy.join("pack.openrage-proj"));
        assert_eq!(
            opened.project_path.as_deref(),
            Some(copy.join("pack.openrage-proj").as_path())
        );
        assert_eq!(the_plan(&mut opened, &copy), before);
    }
}
