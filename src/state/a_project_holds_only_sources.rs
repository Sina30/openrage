//! A project holds only sources: a game file that comes in by any door is decompiled at once, a new source starts empty and is
//! filled, and what a content needs and does not own is a reference, chosen in the picker.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, and a worker for each scan, import, edit and build
//! that a case waits on. The files are small valid ones, since the door decompiles them.

use std::path::{Path, PathBuf};

use or_project::reference::Reference;
use or_project::{Container, Project};
use or_source::asset::Source;
use or_source::sample::{drawable_dictionary, texture_dictionary};
use or_wardrobe::follow::{follow, Led};
use or_wardrobe::Garment;

use super::front_door::{NewProject, Template};
use super::importing_game_files_from_the_app::{imported, project, row, sources_of, until};
use super::os_drop::DropTarget;
use super::workbench::{Choice, LastBuild, Missing, Question, Wanting};
use super::{AppState, Prefs};

const C: &str = "mp_m_freemode_01_mp_m_t";

/// Write `body` at `path`, making the folders on the way; the path.
fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

/// Every compiled game file below `folder`, outside the folders of sources, by its path from `folder`.
fn compiled_outside_sources(folder: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with('.')
                || name.ends_with(".src")
                || matches!(name.as_str(), "build" | "vfs")
            {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, out);
            } else if name.ends_with(".ytd") || name.ends_with(".ydd") || name.ends_with(".ypt") {
                let rel = path.strip_prefix(base).unwrap_or(&path);
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(folder, folder, &mut out);
    out.sort();
    out
}

/// A clothing project made the way the template makes it, opened in a state, and its folder.
fn clothing(dir: &Path) -> (AppState, PathBuf) {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let created = draft.create().expect("the template makes a project");
    let folder = created.file.parent().unwrap().to_path_buf();
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(created.file);
    until(&mut state, "the first scan", |s| s.workbench.is_scanned());
    (state, folder)
}

fn garments(state: &AppState) -> Vec<Garment> {
    state
        .wardrobe_pack()
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| read.wardrobe.garments.clone())
        .unwrap_or_default()
}

/// A clothing project holding one jacket, selected, whose model and texture are sources of its pack.
fn with_a_jacket(dir: &Path) -> (AppState, PathBuf) {
    let (mut state, folder) = clothing(dir);
    state.add_clothes(vec![
        write(
            &dir.join("drop").join(format!("{C}^jbib_000_u.ydd")),
            &drawable_dictionary("jacket", 6, 0.0),
        ),
        write(
            &dir.join("drop")
                .join(format!("{C}^jbib_diff_000_a_uni.ytd")),
            &texture_dictionary("tex", 3),
        ),
    ]);
    until(&mut state, "the drop", |s| garments(s).len() == 1);
    let module = state.wardrobe_module_id().unwrap();
    let id = garments(&state)[0].id.clone();
    state.select_garment(&module, Some(id));
    (state, folder)
}

#[test]
fn a_ytd_added_by_each_door_leaves_a_source_folder_and_no_compiled_file_in_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());

    // Add, a file of elsewhere and a file already inside the project's folder.
    let outside = write(
        &dir.path().join("downloads/road.ytd"),
        &texture_dictionary("road", 3),
    );
    let inside = write(
        &folder.join("mods/lamp.ytd"),
        &texture_dictionary("lamp", 4),
    );
    state.add_to_project(vec![outside, inside.clone()]);
    imported(&mut state);
    assert_eq!(
        sources_of(&folder),
        ["lamp.textures.src", "road.textures.src"],
        "each file Add was given is a source"
    );
    assert!(
        state
            .member_paths()
            .iter()
            .all(|m| !m.to_string_lossy().ends_with(".ytd")),
        "no compiled game file joined the Files content: {:?}",
        state.member_paths()
    );

    // A drop on the Project workspace.
    let dropped = write(
        &dir.path().join("dropped/sign.ytd"),
        &texture_dictionary("sign", 5),
    );
    state.handle_os_drop(vec![dropped], DropTarget::Project);
    if matches!(state.workbench.asking, Some(Question::DropClothes { .. })) {
        state.answer_dropped_clothes(false);
    }
    until(&mut state, "the drop's import", |s| {
        !s.workbench.imports.is_running()
            && sources_of(&folder).contains(&"sign.textures.src".to_string())
    });

    // The Wardrobe's drop, and Replace texture, in a clothing project.
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = with_a_jacket(dir.path());
    let module = state.wardrobe_module_id().unwrap();
    let id = garments(&state)[0].id.clone();
    let swap = write(
        &dir.path().join("swap/better.ytd"),
        &texture_dictionary("better", 9),
    );
    state.replace_texture(id, 0, swap);
    until(&mut state, "the replacement", |s| {
        garments(s)[0].variations[0]
            .texture
            .as_deref()
            .is_some_and(|t| t.ends_with("better.textures.src"))
    });

    assert!(state
        .member_paths()
        .iter()
        .all(|m| m.extension().is_none_or(|e| e != "ytd")));
    assert_eq!(
        compiled_outside_sources(&folder),
        Vec::<String>::new(),
        "the pack holds sources, not the .ydd and .ytd that were dropped and chosen: {module}"
    );
    let pack = state.wardrobe_pack().unwrap().folder.clone();
    for named in [
        garments(&state)[0].model.clone().unwrap(),
        garments(&state)[0].variations[0].texture.clone().unwrap(),
    ] {
        assert!(
            Source::open(&pack.join(&named)).is_ok(),
            "{named} is a source"
        );
    }
}

#[test]
fn a_ypt_added_by_each_door_leaves_a_particles_source_and_no_compiled_file_in_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());

    // Add, a file of elsewhere and a file already inside the project's folder.
    let outside = write(
        &dir.path().join("downloads/smoke.ypt"),
        &or_particle::sample::list_of("smoke", &["fx_smoke"]),
    );
    let inside = write(
        &folder.join("mods/spark.ypt"),
        &or_particle::sample::list_of("spark", &["fx_spark"]),
    );
    state.add_to_project(vec![outside, inside]);
    imported(&mut state);
    assert_eq!(
        sources_of(&folder),
        ["smoke.particles.src", "spark.particles.src"],
        "each file Add was given is a particles source"
    );
    assert!(
        state
            .member_paths()
            .iter()
            .all(|m| !m.to_string_lossy().ends_with(".ypt")),
        "no compiled game file joined the Files content: {:?}",
        state.member_paths()
    );

    // A drop on the Project workspace.
    let dropped = write(
        &dir.path().join("dropped/dust.ypt"),
        &or_particle::sample::list_of("dust", &["fx_dust"]),
    );
    state.handle_os_drop(vec![dropped], DropTarget::Project);
    if matches!(state.workbench.asking, Some(Question::DropClothes { .. })) {
        state.answer_dropped_clothes(false);
    }
    until(&mut state, "the drop's import", |s| {
        !s.workbench.imports.is_running()
            && sources_of(&folder).contains(&"dust.particles.src".to_string())
    });
    // An import only adds: the file that was already in the folder is as it was, and no door copied a `.ypt` in beside it.
    assert_eq!(
        compiled_outside_sources(&folder),
        ["mods/spark.ypt"],
        "the project holds sources, and the only compiled file is the one that was there"
    );
    let made = Source::open(&folder.join("sources/dust.particles.src")).expect("a source");
    assert_eq!(made.manifest.kind, "particles");
}

#[test]
fn a_new_texture_dictionary_is_an_empty_source_that_validates_and_compiles_to_a_dictionary_on_build(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());

    state.ask_for_a_new_source("textures");
    assert!(
        matches!(state.workbench.asking, Some(Question::NewSource { .. })),
        "New asks for a name"
    );
    let made = state
        .create_source("textures", "shirts")
        .expect("the source is made");
    state.workbench.asking = None;

    assert_eq!(made, folder.join("sources/shirts.textures.src"));
    let source = Source::open(&made).expect("it validates as a source");
    assert_eq!(source.manifest.kind, "textures");
    until(&mut state, "its row", |s| {
        s.workbench.rows().iter().any(|r| r.name == "shirts")
    });
    assert_eq!(
        row(&state, "shirts").source_kind.as_deref(),
        Some("textures")
    );
    assert!(
        state.create_source("drawables", "x").is_err(),
        "a kind with no empty form says so"
    );

    until(&mut state, "the plan", |s| s.workbench.is_scanned());
    let id = state.project_carried.container.outputs[0].id.clone();
    until(&mut state, "the plan", |s| !s.workbench.is_blocked(&id));
    state.build_selected();
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running()
    });
    let LastBuild::Built(done) = state.workbench.builds().last(&id).expect("it ended") else {
        panic!(
            "the build stopped: {:?}",
            state.workbench.builds().last(&id)
        );
    };
    let shipped = done.parts[0].folder.join("stream/shirts.ytd");
    assert!(
        shipped.is_file(),
        "the source compiled to a dictionary on build"
    );

    // Undo import takes the new source back, as it does an import's.
    state.undo_last_import();
    assert!(!made.exists());
}

#[test]
fn an_asset_json_a_textures_src_and_a_folder_with_nested_sources_import_in_one_step_with_the_count()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let scan = dir.path().join("pack");
    let make = |parent: &Path, name: &str, shade: u8| {
        std::fs::create_dir_all(parent).unwrap();
        or_source::game_import::decompile(
            &texture_dictionary(name, shade),
            &format!("{name}.ytd"),
            parent,
            &Default::default(),
        )
        .unwrap()
    };
    let (one, two) = (
        make(&scan.join("a"), "one", 1),
        make(&scan.join("b/deep"), "two", 2),
    );
    let alone = make(&dir.path().join("elsewhere"), "alone", 3);
    write(&scan.join("c/road.ytd"), &texture_dictionary("road", 4));
    write(&scan.join("c/notes.txt"), b"hello");

    // A folder with nested sources, a .textures.src, and an asset.json.
    state.import_game_files(vec![scan.clone(), one.clone(), alone.join("asset.json")]);
    imported(&mut state);

    let report = &state.workbench.imports.report().expect("a report").report;
    let [made, already, kept, refused] = super::workbench::counts(report);
    assert_eq!(
        (made, kept, refused),
        (4, 1, 0),
        "one, two, road and alone are made, the text file is kept: {report:#?}"
    );
    assert_eq!(
        already, 1,
        "{one:?} was met twice, as a source in the scan and as itself"
    );
    let mut wanted = vec![
        "alone.textures.src",
        "one.textures.src",
        "road.textures.src",
        "two.textures.src",
    ];
    wanted.sort_unstable();
    assert_eq!(sources_of(&folder), wanted);
    assert_eq!(
        compiled_outside_sources(&folder),
        Vec::<String>::new(),
        "nothing compiled is kept but the file with no source form: {two:?}"
    );
    state.undo_import();
    assert!(sources_of(&folder).is_empty(), "one step, one undo");
}

/// A second project in `dir/brand` holding a cloth, and the cloth.
fn another_project(dir: &Path) -> PathBuf {
    let folder = dir.join("brand");
    let file = folder.join("brand.openrage-proj");
    std::fs::create_dir_all(&folder).unwrap();
    Project::create(&file, &Container::default(), &[]).expect("a project");
    write(&folder.join("cloth/jacket.yld"), b"cloth of the brand")
}

/// Choose `path`, a file of another place, for the cloth of the garment selected.
fn choose_a_cloth_from_elsewhere(state: &mut AppState, path: PathBuf) {
    let module = state.wardrobe_module_id().unwrap();
    let garment = garments(state)[0].id.clone();
    state.ask_for_a_file(Wanting::Cloth { module, garment });
    let Some(Question::Pick(picker)) = state.workbench.asking.clone() else {
        panic!("the picker opens: {:?}", state.workbench.asking);
    };
    state.choose(picker.wanting, Choice::Outside(path));
}

#[test]
fn a_reference_to_another_projects_file_asks_the_question_and_each_answer_leaves_the_project_as_it_says(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = with_a_jacket(dir.path());
    let yld = another_project(dir.path());
    let pack = state.wardrobe_pack().unwrap().folder.clone();

    // Only reference it: the file stays in the other project and the pack holds a small reference.
    choose_a_cloth_from_elsewhere(&mut state, yld.clone());
    let Some(Question::ImportOrReference { whose, .. }) = state.workbench.asking.clone() else {
        panic!(
            "a file of another project asks: {:?}",
            state.workbench.asking
        );
    };
    assert_eq!(
        whose.as_deref(),
        Some("brand"),
        "the question says whose file it is"
    );
    state.answer_import_or_reference(false);
    until(&mut state, "the reference", |s| {
        garments(s)[0].cloth.is_some()
    });
    let named = garments(&state)[0].cloth.clone().unwrap();
    assert!(named.ends_with(".ref.json"), "{named}");
    assert!(
        !pack.join("cloth/jacket.yld").exists(),
        "nothing was copied"
    );
    assert_eq!(
        follow(&pack, &named),
        Led::To {
            at: yld.clone(),
            source: false
        },
        "the build reads it where it is"
    );
    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s)[0].cloth.is_none());
    assert!(!pack.join(&named).exists(), "Undo takes the reference out");

    // Import it: the cloth has no source form, so it is kept as it is in the pack, and no reference is made.
    choose_a_cloth_from_elsewhere(&mut state, yld.clone());
    state.answer_import_or_reference(true);
    until(&mut state, "the import", |s| garments(s)[0].cloth.is_some());
    let kept = garments(&state)[0].cloth.clone().unwrap();
    assert_eq!(kept, "cloth/jacket.yld");
    assert_eq!(
        std::fs::read(pack.join(&kept)).unwrap(),
        b"cloth of the brand"
    );
    assert!(yld.is_file(), "the other project's file is not touched");
}

#[test]
fn an_id_alone_builds_with_the_id_and_a_plain_sentence_and_a_missing_file_is_a_problem_with_locate()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = with_a_jacket(dir.path());
    let module = state.wardrobe_module_id().unwrap();
    let garment = garments(&state)[0].id.clone();
    let wanting = Wanting::Cloth {
        module: module.clone(),
        garment: garment.clone(),
    };

    state.choose(wanting, Choice::Id("cloth_jbib_000".into()));
    until(&mut state, "the reference", |s| {
        garments(s)[0].cloth.is_some()
    });
    let pack = state.wardrobe_pack().unwrap().folder.clone();
    let named = garments(&state)[0].cloth.clone().unwrap();
    assert_eq!(
        Reference::read(&pack.join(&named)).unwrap(),
        Reference::by_id("cloth_jbib_000")
    );

    let output = state.project_carried.container.outputs[0].id.clone();
    until(&mut state, "the plan", |s| !s.workbench.is_blocked(&output));
    let plans: Vec<_> = state
        .workbench
        .resources_of(&output)
        .iter()
        .map(|r| r.plan.clone())
        .collect();
    let note = plans
        .iter()
        .flat_map(|p| &p.notes)
        .find(|n| n.contains("cloth_jbib_000"))
        .unwrap_or_else(|| panic!("{plans:#?}"));
    assert!(note.contains("not in the project"), "{note}");
    state.build_selected();
    until(&mut state, "the build", |s| {
        !s.workbench.builds().is_running()
    });
    assert!(
        matches!(
            state.workbench.builds().last(&output),
            Some(LastBuild::Built(_))
        ),
        "an id alone builds: {:?}",
        state.workbench.builds().last(&output)
    );

    // A reference to a source of the project that is gone is a problem, and it can be located.
    let source = another_source_of_the_project(&folder);
    let reference = Reference::here(&source.0, Some(&source.1));
    state.refer(Wanting::Model { module, garment }, reference);
    until(&mut state, "the reference", |s| {
        garments(s)[0]
            .model
            .as_deref()
            .is_some_and(|m| m.ends_with(".ref.json"))
    });
    std::fs::remove_dir_all(folder.join(&source.0)).unwrap();
    state.workbench.want_scan();
    until(&mut state, "the problem", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| f.code == Some("model-missing"))
    });
    let finding = state
        .workbench
        .findings()
        .iter()
        .find(|f| f.code == Some("model-missing"))
        .cloned()
        .unwrap();
    let fix = finding
        .fixes
        .iter()
        .find(|f| f.label() == "Locate…")
        .cloned()
        .expect("a missing model has a Locate");
    state.ask_about_fix(&fix);
    let Some(Question::Pick(picker)) = state.workbench.asking.clone() else {
        panic!("Locate opens the picker: {:?}", state.workbench.asking);
    };
    assert!(matches!(
        picker.wanting,
        Wanting::Locate {
            what: Missing::Model,
            ..
        }
    ));
}

/// A drawables source in the project's `sources/`, its path from the project's folder and its id, for a reference to name.
fn another_source_of_the_project(folder: &Path) -> (String, String) {
    let parent = folder.join("sources");
    std::fs::create_dir_all(&parent).unwrap();
    let source = or_source::game_import::decompile(
        &drawable_dictionary("scarf", 6, 1.0),
        "scarf.ydd",
        &parent,
        &Default::default(),
    )
    .unwrap();
    let id = Source::open(&source).unwrap().manifest.id;
    ("sources/scarf.drawables.src".to_string(), id)
}

#[test]
fn pictures_added_as_a_textures_source_name_no_platform_whatever_edition_the_project_is_for() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    state.project_carried.top.insert(
        "game".into(),
        serde_json::Value::String("gta5_enhanced".into()),
    );
    let art = dir.path().join("art");
    std::fs::create_dir_all(&art).unwrap();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([40, 0, 0, 255]))
        .save(art.join("logo.png"))
        .unwrap();

    state.add_images_as_textures_source(vec![art.join("logo.png")]);
    imported(&mut state);

    let made = folder.join("sources/logo.textures.src");
    assert_eq!(
        Source::open(&made).unwrap().manifest.target,
        or_source::create::AUTHORED_TARGET,
        "the build chooses the platform, not the project it was added to"
    );
}

#[test]
fn the_new_commands_are_in_the_palette() {
    let state = AppState::from_prefs(Prefs::default());
    let listed = crate::ui::commands::all(&state);
    let spec = crate::ui::commands::spec(crate::ui::commands::Action::NewTexturesSource)
        .expect("a row of the table");
    assert!(listed.iter().any(|c| c.id == spec.id));
}
