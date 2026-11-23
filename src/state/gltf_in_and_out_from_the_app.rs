//! A model goes out to glTF and comes back in from its menus. The report each
//! shows before a file is written or the model changes is the one
//! `openrage-cli mesh export|import` gives for the same file, and the model an
//! import leaves saves as that command writes it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_interchange::gltf::{self, Container};
use or_interchange::{report_lines, ExtractOptions, ImportOptions, LodSelection, Scene};
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8, TARGET_GEN9};

use super::gltf::{Asked, Export, ExportChoice};
use super::the_files_view_in_the_centre::{leaf_of, listed};
use super::{AppState, DocumentId, EditMode, Game, Prefs, RpfNodeId};
use crate::render::meshes::build_meshes_for_active_document;
use crate::render::viewport::MeshCacheKey;
use crate::ui::panels::registry::ids;

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
const DEADLINE: Duration = Duration::from_secs(180);
/// How far a modder's edit in another tool moves every vertex up.
const LIFT: f32 = 2.0;

/// A widget one frame drew: its text, its centre, and whether it can be clicked.
#[derive(Debug)]
struct Drawn {
    label: String,
    at: egui::Pos2,
    enabled: bool,
}

/// One frame of the tab strip, the dock and the questions, as AccessKit describes it.
fn frame(ctx: &egui::Context, state: &mut AppState, events: Vec<egui::Event>) -> Vec<Drawn> {
    state.poll_expansions();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        crate::ui::panels::document_tabs::draw(ui, state);
        crate::ui::dock::draw(ui, state, None);
        crate::ui::panels::file_prompts::draw(ui.ctx(), state);
    });
    let Some(update) = output.platform_output.accesskit_update else {
        return Vec::new();
    };
    (update.nodes.iter())
        .filter_map(|(_, node)| {
            let b = node.bounds()?;
            Some(Drawn {
                label: node.label().or_else(|| node.value())?.to_string(),
                at: egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32),
                enabled: !node.is_disabled(),
            })
        })
        .collect()
}

fn settled(ctx: &egui::Context, state: &mut AppState) -> Vec<Drawn> {
    frame(ctx, state, Vec::new());
    frame(ctx, state, Vec::new())
}

/// The widget labelled `label` inside `area`, behind the glyph a row's icon puts first.
fn find<'a>(shown: &'a [Drawn], area: egui::Rect, label: &str) -> &'a Drawn {
    let bare = |text: &str| -> bool {
        text.trim_start_matches(|c: char| ('\u{e000}'..='\u{f8ff}').contains(&c) || c == ' ')
            == label
    };
    (shown.iter())
        .find(|d| bare(&d.label) && area.contains(d.at))
        .unwrap_or_else(|| {
            let labels: Vec<&str> = shown.iter().map(|d| d.label.as_str()).collect();
            panic!("no `{label}` in {area:?}: {labels:?}")
        })
}

fn click_with(
    ctx: &egui::Context,
    state: &mut AppState,
    at: egui::Pos2,
    which: egui::PointerButton,
) {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: which,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(ctx, state, vec![egui::Event::PointerMoved(at), press(true)]);
    frame(ctx, state, vec![press(false)]);
}

fn click(ctx: &egui::Context, state: &mut AppState, at: egui::Pos2) {
    click_with(ctx, state, at, egui::PointerButton::Primary);
}

/// The items of the menu a right click on `row` inside `area` opens, left open.
fn menu(ctx: &egui::Context, state: &mut AppState, area: egui::Rect, row: &str) -> Vec<Drawn> {
    let shown = settled(ctx, state);
    let at = find(&shown, area, row).at;
    click_with(ctx, state, at, egui::PointerButton::Secondary);
    frame(ctx, state, Vec::new())
}

fn tab_strip() -> egui::Rect {
    egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::pos2(1600.0, crate::ui::theme::TAB_STRIP_HEIGHT),
    )
}

/// Polls until the question's export or import is worked out.
fn worked_out(state: &mut AppState) {
    let done = |state: &AppState| match &state.gltf.asked {
        Some(Asked::Export(asked)) => asked.made.is_some(),
        Some(Asked::Import(asked)) => asked.made.is_some(),
        None => true,
    };
    let until = Instant::now() + DEADLINE;
    while !done(state) && Instant::now() < until {
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(done(state), "the glTF work never finished");
}

fn written_by_the_job(state: &mut AppState) {
    let until = Instant::now() + DEADLINE;
    while state.extract_jobs.is_running() && Instant::now() < until {
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn opened(state: &mut AppState) -> DocumentId {
    let until = Instant::now() + DEADLINE;
    while state.load_receiver.is_some() && Instant::now() < until {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    state.active_document.expect("the model opened")
}

fn saved(state: &mut AppState, path: &Path) -> Vec<u8> {
    assert!(state.save_active_document_as(path.to_path_buf()));
    let until = Instant::now() + DEADLINE;
    while state.save_receiver.is_some() && Instant::now() < until {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::read(path)
        .unwrap_or_else(|e| panic!("{}: the save wrote nothing: {e}", path.display()))
}

/// `openrage-cli` run with `words`, which must write `out`.
fn cli(words: &[&str], out: &Path) -> Vec<u8> {
    let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    let _ = or_cli::run(&words);
    std::fs::read(out).unwrap_or_else(|e| panic!("{words:?} wrote no {}: {e}", out.display()))
}

fn words(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The smallest staged file of `ext` and `target` that `wanted` takes, given its name and bytes.
fn staged(ext: &str, target: &str, wanted: impl Fn(&str, &[u8]) -> bool) -> Option<PathBuf> {
    let mut files = Corpus::new(MANIFEST, ext).target(target).find();
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    files.into_iter().find(|path| {
        let (Ok(bytes), Some(name)) = (std::fs::read(path), path.file_name()) else {
            return false;
        };
        wanted(&name.to_string_lossy(), &bytes)
    })
}

/// What `mesh export` makes of every drawable of a file.
fn exported(
    name: &str,
    bytes: &[u8],
    lods: LodSelection,
    textures: bool,
) -> Option<or_cli::Exported> {
    let options = ExtractOptions { lods, textures };
    or_cli::export_scene(name, bytes, "subject", None, &options).ok()
}

/// Whether its most detailed level goes out to glTF and back in unasked.
fn comes_back(name: &str, bytes: &[u8]) -> bool {
    let Some(out) = exported(name, bytes, LodSelection::Highest, false) else {
        return false;
    };
    let glb = gltf::write(&out.scene, "subject", Container::Glb).primary;
    gltf::read::load(Path::new("subject.glb"), &glb).is_ok_and(|scene| {
        or_cli::replace_mesh(name, bytes, 0, &scene, &ImportOptions::default()).is_ok()
    })
}

/// Whether every level and no textures export something else than the defaults.
///
/// ⚠ Without a level below the most detailed and a texture of its own, an
/// export that ignored both choices would still match the command's.
fn the_choices_tell(name: &str, bytes: &[u8]) -> bool {
    let meshes = |lods| {
        let out = exported(name, bytes, lods, false)?;
        Some(
            out.scene
                .objects
                .iter()
                .map(|o| o.meshes.len())
                .sum::<usize>(),
        )
    };
    let deeper = matches!(
        (meshes(LodSelection::All), meshes(LodSelection::Highest)),
        (Some(all), Some(highest)) if all > highest
    );
    deeper
        && exported(name, bytes, LodSelection::Highest, true)
            .is_some_and(|out| !out.scene.images.is_empty())
}

/// `input` open as a folder in Browse.
fn browsing(input: &Path) -> (AppState, egui::Context, DocumentId) {
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(input.to_path_buf());
    let folder = state.active_document.expect("the folder opens");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let until = Instant::now() + DEADLINE;
    while listed(&state, folder).is_empty() && Instant::now() < until {
        frame(&ctx, &mut state, Vec::new());
    }
    (state, ctx, folder)
}

fn row(state: &AppState, folder: DocumentId, name: &str) -> RpfNodeId {
    let Some(super::DocPayload::Rpf(rpf)) = state.document(folder).map(|d| &d.payload) else {
        panic!("the folder is open");
    };
    match super::navigation::resolve_path(&rpf.session, name) {
        super::navigation::Resolved::Found(id) => id,
        other => panic!("{name}: {other:?}"),
    }
}

/// Every vertex of `scene` moved up, as a modder's edit in another tool would.
fn lifted(mut scene: Scene) -> Scene {
    for object in &mut scene.objects {
        for mesh in &mut object.meshes {
            for primitive in &mut mesh.primitives {
                for position in &mut primitive.positions {
                    position[2] += LIFT;
                }
            }
        }
    }
    scene
}

/// The first vertex the viewport draws of the active model.
fn first_vertex_drawn(state: &AppState) -> [f32; 3] {
    let meshes = build_meshes_for_active_document(state);
    let first = meshes.iter().find_map(|mesh| mesh.vertices.first());
    first.expect("the viewport draws a vertex of it").position
}

#[test]
fn a_ydr_exports_and_imports_back_from_its_menus_and_saves_what_the_cli_writes() {
    const WHAT: &str =
        "a_ydr_exports_and_imports_back_from_its_menus_and_saves_what_the_cli_writes";
    for target in [TARGET_GEN8, TARGET_GEN9] {
        let Some(path) = staged("ydr", target, comes_back) else {
            skip_corpus(
                WHAT,
                format_args!("no {target} .ydr goes out to glTF and back"),
            );
            continue;
        };
        let Copied {
            scratch,
            input,
            by_app,
            by_cli,
            model,
            name,
            stem,
        } = copied(&path);
        let bytes = std::fs::read(&model).expect("read");

        // Out, from the row's menu in the Files view.
        let cli_glb = by_cli.join(format!("{stem}.glb"));
        let clis_glb = cli(
            &["mesh", "export", &words(&model), &words(&cli_glb)],
            &cli_glb,
        );
        let (mut state, ctx, folder) = browsing(&input);
        let centre = leaf_of(&state, ids::DOCUMENT);
        let items = menu(&ctx, &mut state, centre, &name);
        let export = find(&items, egui::Rect::EVERYTHING, "Export to glTF…");
        assert!(
            export.enabled,
            "{name}: its row's menu offers no export it can run"
        );
        let at = export.at;
        click(&ctx, &mut state, at);
        worked_out(&mut state);
        let asked = settled(&ctx, &mut state);
        find(
            &asked,
            egui::Rect::EVERYTHING,
            &format!("Export {name} to glTF"),
        );
        let out = or_cli::export_scene(&name, &bytes, &stem, None, &ExtractOptions::default())
            .expect("the command exports it");
        for note in report_lines(&out.scene.report) {
            find(&asked, egui::Rect::EVERYTHING, &note);
        }
        let ready = find(&asked, egui::Rect::EVERYTHING, "Export…");
        assert!(ready.enabled, "{name}: the question offers no write");
        assert!(
            std::fs::read_dir(&by_app)
                .expect("a folder")
                .next()
                .is_none(),
            "{name}: something was written before the report was answered"
        );
        let app_glb = by_app.join(format!("{stem}.glb"));
        state.write_gltf(app_glb.clone());
        written_by_the_job(&mut state);
        let apps_glb = std::fs::read(&app_glb).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            apps_glb == clis_glb,
            "{name}: the app exported other bytes than the command"
        );

        // A modder's edit in another tool, then back over the model opened from its row.
        let edited = scratch.path().join("edited.glb");
        let scene = lifted(out.scene);
        std::fs::write(&edited, gltf::write(&scene, &stem, Container::Glb).primary)
            .expect("written");
        let cli_ydr = by_cli.join(&name);
        let clis_ydr = cli(
            &[
                "mesh",
                "import",
                &words(&model),
                &words(&edited),
                &words(&cli_ydr),
            ],
            &cli_ydr,
        );
        state.open_rpf_entry(folder, row(&state, folder, &name));
        let doc = opened(&mut state);
        let title = state.tab_title(state.document(doc).expect("open"));
        let items = menu(&ctx, &mut state, tab_strip(), &title);
        let import = find(&items, egui::Rect::EVERYTHING, "Replace from glTF…");
        assert!(
            import.enabled,
            "{name}: its tab's menu offers no import it can run"
        );
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let (drawn, vertex) = (MeshCacheKey::of(&state, doc, 0), first_vertex_drawn(&state));

        // What the file dialog's answer calls.
        state.import_gltf(doc, edited.clone());
        worked_out(&mut state);
        let asked = settled(&ctx, &mut state);
        find(
            &asked,
            egui::Rect::EVERYTHING,
            &format!("Replace {name} from edited.glb"),
        );
        let read =
            gltf::read::load(&edited, &std::fs::read(&edited).expect("read")).expect("a mesh");
        let (_, report) = or_cli::replace_mesh(&name, &bytes, 0, &read, &ImportOptions::default())
            .expect("the command imports it");
        for line in report.lines() {
            find(&asked, egui::Rect::EVERYTHING, &line);
        }
        let document = state.document(doc).expect("open");
        assert!(
            !document.dirty && document.history.done().is_empty(),
            "{name}: the model changed before the report was answered"
        );
        assert!(
            MeshCacheKey::of(&state, doc, 0) == drawn,
            "{name}: the viewport moved early"
        );

        let at = find(&asked, egui::Rect::EVERYTHING, "Replace").at;
        click(&ctx, &mut state, at);
        assert!(
            state.gltf.asked.is_none(),
            "{name}: the question is still open"
        );
        let document = state.document(doc).expect("open");
        let steps: Vec<&str> = document
            .history
            .done()
            .iter()
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(
            steps,
            ["Replace from edited.glb"],
            "{name}: the import is not one step"
        );
        assert!(
            document.dirty,
            "{name}: the import did not mark the model unsaved"
        );
        assert!(
            MeshCacheKey::of(&state, doc, 0) != drawn,
            "{name}: the viewport does not build the model again"
        );
        let now = first_vertex_drawn(&state);
        assert!(
            (now[2] - vertex[2] - LIFT).abs() < 1e-2 && (now[0] - vertex[0]).abs() < 1e-2,
            "{name}: the viewport draws {now:?} where the import put {vertex:?} up by {LIFT}"
        );

        let apps_ydr = saved(&mut state, &by_app.join(&name));
        assert!(
            apps_ydr == clis_ydr,
            "{name}: the app saved other bytes than the command's import"
        );
        assert_eq!(state.undo().as_deref(), Some("Replace from edited.glb"));
        let payload = &state.document(doc).expect("open").payload;
        let writer = super::payload_handlers::handler_for_payload(payload).expect("a writer");
        let undone = writer.save(payload).expect("it saves");
        let as_opened = or_io::open(&model).and_then(|asset| asset.to_bytes());
        assert!(
            undone == as_opened.expect("it saves"),
            "{name}: undone, it is not the model it was"
        );
        assert!(undone != apps_ydr, "{name}: the import changed nothing");
    }
}

/// A staged file copied into a scratch `in` folder, beside an `app` and a `cli` one.
struct Copied {
    scratch: tempfile::TempDir,
    input: PathBuf,
    by_app: PathBuf,
    by_cli: PathBuf,
    model: PathBuf,
    name: String,
    stem: String,
}

fn copied(path: &Path) -> Copied {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let folders = ["in", "app", "cli"].map(|f| scratch.path().join(f));
    for folder in &folders {
        std::fs::create_dir_all(folder).expect("a folder");
    }
    let [input, by_app, by_cli] = folders;
    let name = path
        .file_name()
        .expect("a file")
        .to_string_lossy()
        .into_owned();
    let model = input.join(&name);
    std::fs::copy(path, &model).expect("staged");
    Copied {
        scratch,
        input,
        by_app,
        by_cli,
        model,
        name,
        stem: path
            .file_stem()
            .expect("a stem")
            .to_string_lossy()
            .into_owned(),
    }
}

/// The export question of `staged`'s row, worked out, in Browse.
fn exporting(staged: &Copied) -> (AppState, egui::Context) {
    let (mut state, ctx, _) = browsing(&staged.input);
    let centre = leaf_of(&state, ids::DOCUMENT);
    let items = menu(&ctx, &mut state, centre, &staged.name);
    let at = find(&items, egui::Rect::EVERYTHING, "Export to glTF…").at;
    click(&ctx, &mut state, at);
    worked_out(&mut state);
    (state, ctx)
}

/// Clicks the widget labelled `label` in the question, and waits for what it changed.
fn choose(ctx: &egui::Context, state: &mut AppState, label: &str) {
    let shown = settled(ctx, state);
    let at = find(&shown, egui::Rect::EVERYTHING, label).at;
    click(ctx, state, at);
    worked_out(state);
}

fn every_level_without_textures_as_gltf(s: &Copied) {
    let model = words(&s.model);
    let cli_glb = s.by_cli.join(format!("{}.glb", s.stem));
    let clis = cli(&["mesh", "export", &model, &words(&cli_glb)], &cli_glb);
    let (mut state, _) = exporting(s);
    let app_glb = s.by_app.join(format!("{}.glb", s.stem));
    state.write_gltf(app_glb.clone());
    written_by_the_job(&mut state);
    let apps = std::fs::read(&app_glb).unwrap_or_else(|e| panic!("{}: {e}", s.name));
    assert!(
        apps == clis,
        "{}: the app's export with its textures is not the command's",
        s.name
    );

    let cli_gltf = s.by_cli.join(format!("{}.gltf", s.stem));
    let exports = ["mesh", "export", &model, &words(&cli_gltf)];
    let clis = cli(
        &[&exports[..], &["--lods", "all", "--no-textures"]].concat(),
        &cli_gltf,
    );
    let (mut state, ctx) = exporting(s);
    for label in [
        "Every level of detail",
        "Its own textures, as PNG",
        ".gltf beside a .bin and a .png per texture",
    ] {
        choose(&ctx, &mut state, label);
    }
    let Some(Asked::Export(asked)) = &state.gltf.asked else {
        panic!("{}: the question closed", s.name);
    };
    let chosen = ExportChoice {
        entry: None,
        every_lod: true,
        textures: false,
        glb: false,
    };
    assert_eq!(
        asked.choice, chosen,
        "{}: the clicks chose something else",
        s.name
    );
    let app_gltf = s.by_app.join(format!("{}.gltf", s.stem));
    state.write_gltf(app_gltf.clone());
    written_by_the_job(&mut state);
    let apps = std::fs::read(&app_gltf).unwrap_or_else(|e| panic!("{}: {e}", s.name));
    assert!(
        apps == clis,
        "{}: the app's .gltf is not the command's",
        s.name
    );
    let bin = format!("{}.bin", s.stem);
    assert!(
        std::fs::read(s.by_app.join(&bin)).ok() == std::fs::read(s.by_cli.join(&bin)).ok(),
        "{}: the app's {bin} is not the command's",
        s.name
    );
}

fn one_drawable(s: &Copied) {
    let (mut state, ctx) = exporting(s);
    let Some(Asked::Export(asked)) = &state.gltf.asked else {
        panic!("{}: the question closed", s.name);
    };
    let (entry, drawable) = (asked.drawables.get(1).cloned())
        .unwrap_or_else(|| panic!("{}: two drawables are not listed", s.name));
    let cli_glb = s.by_cli.join(format!("{}.glb", s.stem));
    let entry_word = entry.to_string();
    let exports = ["mesh", "export", &words(&s.model), &words(&cli_glb)];
    let clis = cli(
        &[&exports[..], &["--entry", &entry_word]].concat(),
        &cli_glb,
    );
    // The combo box shows what is chosen, and a click there opens its list.
    choose(&ctx, &mut state, "Every drawable");
    choose(&ctx, &mut state, &format!("{entry}  {drawable}"));
    let Some(Asked::Export(asked)) = &state.gltf.asked else {
        panic!("{}: the question closed", s.name);
    };
    assert_eq!(
        asked.choice.entry,
        Some(entry),
        "{}: another drawable was chosen",
        s.name
    );
    let app_glb = s.by_app.join(format!("{}.glb", s.stem));
    state.write_gltf(app_glb.clone());
    written_by_the_job(&mut state);
    let apps = std::fs::read(&app_glb).unwrap_or_else(|e| panic!("{}: {e}", s.name));
    assert!(
        apps == clis,
        "{}: the app's drawable {entry} is not the command's",
        s.name
    );
}

#[test]
fn the_exports_choices_reach_the_files_it_writes() {
    const WHAT: &str = "the_exports_choices_reach_the_files_it_writes";
    let telling = staged("ydr", TARGET_GEN8, the_choices_tell)
        .or_else(|| staged("yft", TARGET_GEN8, the_choices_tell));
    match telling {
        Some(path) => every_level_without_textures_as_gltf(&copied(&path)),
        None => skip_corpus(
            WHAT,
            "no Legacy .ydr or .yft has a level below its most detailed and a texture of its own",
        ),
    }
    let two = |name: &str, bytes: &[u8]| {
        exported(name, bytes, LodSelection::Highest, false).is_some_and(|o| o.entries.len() >= 2)
    };
    match staged("ydd", TARGET_GEN8, two) {
        Some(path) => one_drawable(&copied(&path)),
        None => skip_corpus(WHAT, "no Legacy .ydd holds two drawables"),
    }
}

#[test]
fn an_import_the_library_refuses_says_why_and_changes_nothing() {
    const WHAT: &str = "an_import_the_library_refuses_says_why_and_changes_nothing";
    let Some(path) = staged("ydr", TARGET_GEN8, comes_back) else {
        skip_corpus(WHAT, "no Legacy .ydr goes out to glTF and back");
        return;
    };
    let name = path
        .file_name()
        .expect("a file")
        .to_string_lossy()
        .into_owned();
    let stem = path
        .file_stem()
        .expect("a stem")
        .to_string_lossy()
        .into_owned();
    let bytes = std::fs::read(&path).expect("read");
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let model = scratch.path().join(&name);
    std::fs::copy(&path, &model).expect("staged");

    // One geometry more than the model's most detailed level holds.
    let mut scene = or_cli::export_scene(&name, &bytes, &stem, None, &ExtractOptions::default())
        .expect("it exports")
        .scene;
    let mesh = &mut scene.objects[0].meshes[0];
    let extra = mesh.primitives[0].clone();
    mesh.primitives.push(extra);
    let grown = scratch.path().join("grown.glb");
    std::fs::write(&grown, gltf::write(&scene, &stem, Container::Glb).primary).expect("written");
    let read = gltf::read::load(&grown, &std::fs::read(&grown).expect("read")).expect("a mesh");
    let why = or_cli::replace_mesh(&name, &bytes, 0, &read, &ImportOptions::default())
        .expect_err("the command refuses it");

    let mut state = AppState::from_prefs(Prefs::default());
    state.load_file(model);
    let doc = opened(&mut state);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    state.import_gltf(doc, grown);
    worked_out(&mut state);
    let asked = settled(&ctx, &mut state);
    find(
        &asked,
        egui::Rect::EVERYTHING,
        &format!("It cannot replace {name}, which is unchanged:"),
    );
    find(&asked, egui::Rect::EVERYTHING, &why);
    assert!(
        !find(&asked, egui::Rect::EVERYTHING, "Replace").enabled,
        "{name}: a refused import can still be applied"
    );
    let at = find(&asked, egui::Rect::EVERYTHING, "Cancel").at;
    click(&ctx, &mut state, at);
    assert!(
        state.gltf.asked.is_none(),
        "{name}: Cancel left the question open"
    );
    let document = state.document(doc).expect("open");
    assert!(
        !document.dirty && document.history.done().is_empty(),
        "{name}: a refused import changed the model"
    );
}

/// What `name`'s most detailed level exports as, with every normal the mesh states left out.
fn without_normals(name: &str, stem: &str, bytes: &[u8]) -> Option<Scene> {
    let mut scene = or_cli::export_scene(name, bytes, stem, None, &ExtractOptions::default())
        .ok()?
        .scene;
    for object in &mut scene.objects {
        for mesh in &mut object.meshes {
            for primitive in &mut mesh.primitives {
                primitive.normals.clear();
            }
        }
    }
    Some(scene)
}

#[test]
fn a_channel_the_mesh_leaves_out_is_refused_in_words_and_never_by_a_flag() {
    const WHAT: &str = "a_channel_the_mesh_leaves_out_is_refused_in_words_and_never_by_a_flag";
    let refused = |name: &str, bytes: &[u8]| {
        let stem = name.trim_end_matches(".ydr");
        without_normals(name, stem, bytes).is_some_and(|scene| {
            or_cli::replace_mesh(name, bytes, 0, &scene, &ImportOptions::default()).is_err()
        })
    };
    let Some(path) = staged("ydr", TARGET_GEN8, refused) else {
        skip_corpus(WHAT, "no Legacy .ydr declares a normal");
        return;
    };
    let name = path
        .file_name()
        .expect("a file")
        .to_string_lossy()
        .into_owned();
    let stem = name.trim_end_matches(".ydr");
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let model = scratch.path().join(&name);
    std::fs::copy(&path, &model).expect("staged");
    let scene =
        without_normals(&name, stem, &std::fs::read(&path).expect("read")).expect("exports");
    let mesh = scratch.path().join("flat.glb");
    std::fs::write(&mesh, gltf::write(&scene, stem, Container::Glb).primary).expect("written");

    let mut state = AppState::from_prefs(Prefs::default());
    state.load_file(model);
    let doc = opened(&mut state);
    state.import_gltf(doc, mesh);
    worked_out(&mut state);
    let Some(Asked::Import(asked)) = &state.gltf.asked else {
        panic!("{name}: the import asked nothing");
    };
    let Some(Err(why)) = &asked.made else {
        panic!("{name}: a mesh without normals was not refused");
    };
    assert!(why.contains("normal channel"), "{name}: {why}");
    assert!(
        !why.contains("--"),
        "{name}: the question tells a command line's flag: {why}"
    );
}

#[test]
fn a_gltf_export_asks_the_edit_gate_where_it_writes() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let install = scratch.path().join("GTAV");
    std::fs::create_dir_all(install.join("mods")).expect("an install");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("a marker");
    let mut state = AppState::from_prefs(Prefs::default());
    (state.prefs.game_paths).set(Game::GtaVLegacy, Some(install.clone()));
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.gltf.asked = Some(Asked::Export(Export {
        name: "prop.ydr".into(),
        bytes: Default::default(),
        choice: ExportChoice {
            entry: None,
            every_lod: false,
            textures: true,
            glb: true,
        },
        drawables: Vec::new(),
        made: Some(Ok(Scene::default())),
        gta6: None,
    }));

    let inside = install.join("mods").join("prop.glb");
    state.write_gltf(inside.clone());
    written_by_the_job(&mut state);
    assert!(
        !inside.exists(),
        "Read only writes nothing inside the install"
    );
    assert!(
        state.gltf.asked.is_some(),
        "a refused write keeps the question, to choose another place"
    );

    let outside = scratch.path().join("prop.glb");
    state.write_gltf(outside.clone());
    written_by_the_job(&mut state);
    assert!(outside.is_file(), "outside the install it writes");
    assert!(state.gltf.asked.is_none(), "a write answers the question");
}
