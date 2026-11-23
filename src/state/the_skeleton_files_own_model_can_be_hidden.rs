//! The Animation preview draws the skeleton file's own model unless a box in Preview settings
//! hides it, which hides neither the skeleton nor any mesh source, and a restart keeps the choice.

use egui::accesskit::{Role, Toggled};
use or_io::decl::Gen8;
use or_io::gta5::GtaDrawable;
use or_test_support::{skip_corpus, Corpus};

use super::an_edit_stays_where_it_was_made::open;
use super::prefs::{keep_prefs_in, load_prefs, load_prefs_from, Prefs};
use super::the_animation_preview_draws_what_its_sources_hold::{a_clip_over_the_rig, rigged, CLIP};
use super::the_viewport_picks_what_it_draws::{a_triangle, Holder};
use super::{AppState, DocPayload, DocumentId, MeshSource, SkeletonSource};
use crate::render::meshes::build_meshes_for_active_document;
use crate::render::skeleton_lines::build_pose_comparison;
use crate::render::viewport::MeshCacheKey;
use crate::ui::dock::Workspace;
use crate::ui::panels::viewport_layout::overlay_text;

const THE_BOX: &str = "Draw the skeleton file's own model";

/// ⚠ A click on the box saves the preferences, which no other test may read: a test that clicks it
/// calls this first and holds the folder until it ends.
fn keep_prefs_to_this_test() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a scratch folder");
    keep_prefs_in(dir.path());
    dir
}

/// Whether the preferences this test holds say the box is unticked, which a click saves and a restart keeps.
///
/// ⚠ The half of the leak check that can fail while saving works: a click that does not save leaves the box ticked here.
fn this_tests_folder_holds_the_box_unticked() -> bool {
    !load_prefs().draw_skeleton_model
}

/// Whether a test on another thread finds the box ticked: it reads a folder of its own, so it finds the default, and this fails only if a click reached a folder that thread reads.
fn other_tests_find_the_box_ticked() -> bool {
    let theirs = std::thread::spawn(load_prefs).join();
    theirs
        .expect("the preferences are read")
        .draw_skeleton_model
}

/// Preview settings drawn frame by frame, and what a user clicks in it.
struct Settings {
    ctx: egui::Context,
    time: f64,
}

impl Settings {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self { ctx, time: 0.0 }
    }

    fn frame(&mut self, state: &mut AppState, events: Vec<egui::Event>) -> egui::FullOutput {
        // ⚠ A press held past egui's click duration is a drag, not a click.
        self.time += 0.1;
        let input = egui::RawInput {
            events,
            time: Some(self.time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 4000.0),
            )),
            ..Default::default()
        };
        let draw = |ui: &mut egui::Ui| crate::ui::panels::preview_settings::draw(ui, state);
        self.ctx.run_ui(input, draw)
    }

    /// Where the box is on screen and whether it is ticked, if the panel draws it.
    fn the_box(&mut self, state: &mut AppState) -> Option<(egui::Pos2, bool)> {
        let mut found = None;
        for _ in 0..3 {
            let output = self.frame(state, Vec::new());
            let update = output.platform_output.accesskit_update?;
            found = update.nodes.iter().find_map(|(_, node)| {
                let named = node.role() == Role::CheckBox && node.label() == Some(THE_BOX);
                let bounds = node.bounds().filter(|_| named)?;
                let centre = egui::pos2(
                    ((bounds.x0 + bounds.x1) / 2.0) as f32,
                    ((bounds.y0 + bounds.y1) / 2.0) as f32,
                );
                Some((centre, node.toggled() == Some(Toggled::True)))
            });
            if found.is_some() {
                break;
            }
        }
        found
    }

    fn ticked(&mut self, state: &mut AppState) -> bool {
        let found = self.the_box(state);
        found
            .unwrap_or_else(|| panic!("Preview settings draws no `{THE_BOX}`"))
            .1
    }

    fn click_the_box(&mut self, state: &mut AppState) {
        let (at, _) = (self.the_box(state)).unwrap_or_else(|| panic!("no `{THE_BOX}` to click"));
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frame(state, vec![egui::Event::PointerMoved(at), button(true)]);
        self.frame(state, vec![button(false)]);
    }

    /// Every text the panel paints.
    fn words(&mut self, state: &mut AppState) -> Vec<String> {
        let shapes = self.frame(state, Vec::new()).shapes;
        super::an_edit_stays_where_it_was_made::texts(&shapes)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }
}

/// The Animation workspace over a clip dictionary, which is the active document, with one skeleton
/// source chosen and no mesh source.
struct Preview {
    state: AppState,
    settings: Settings,
    /// The document that holds the skeleton source.
    held: DocumentId,
    clips: DocumentId,
}

fn previewing(mut state: AppState, held: DocumentId, skeleton: SkeletonSource) -> Preview {
    let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
    let clips = state.insert_document("C:/mods/clips.ycd".into(), clips, None, None);
    assert_eq!(state.active_workspace, Workspace::Animation);
    state.preview_scene.set_clip(clips, CLIP);
    state.preview_scene.pin_skeleton(Some(skeleton));
    state.preview_scene.show_bones = true;
    state.tick_animation();
    Preview {
        state,
        settings: Settings::new(),
        held,
        clips,
    }
}

/// A Legacy drawable of one triangle skinned to the root bone, on the rig of `rigged`: a ped's
/// fragment in little.
fn a_ped() -> GtaDrawable<Gen8> {
    let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut drawable = a_triangle(corners, Some(0), None);
    drawable.base.skeleton = rigged::<Gen8>().base.skeleton;
    drawable
}

/// `a_ped` held as `holder` holds a drawable, chosen alone as the skeleton source.
fn a_ped_alone(state: AppState, holder: Holder) -> Preview {
    let mut state = state;
    let asset = holder.asset(a_ped());
    let held = state.insert_document(
        holder.file().into(),
        DocPayload::Resource(asset),
        None,
        None,
    );
    previewing(state, held, holder.skeleton(held))
}

impl Preview {
    /// The mesh source of each mesh the preview builds.
    fn drawn(&self) -> Vec<Option<MeshSource>> {
        (build_meshes_for_active_document(&self.state).into_iter())
            .map(|upload| upload.source)
            .collect()
    }

    /// What the viewport builds its meshes from, which builds them again when it moves.
    fn key(&self) -> MeshCacheKey {
        MeshCacheKey::of(&self.state, self.clips, 0)
    }

    /// A mesh source in a document of its own, the clip dictionary active again.
    fn a_wall(&mut self) -> MeshSource {
        let wall = [[1.0, -0.5, -2.0], [1.0, 3.5, -2.0], [1.0, 1.5, 2.0]];
        let asset = or_io::Asset::Drawable(a_triangle(wall, None, None));
        let path = "C:/mods/wall.ydr".into();
        let document = (self.state).insert_document(path, DocPayload::Resource(asset), None, None);
        // Opening a drawable switches to the workspace that shows it.
        self.state.active_workspace = Workspace::Animation;
        self.state.active_document = Some(self.clips);
        MeshSource {
            document,
            entry: None,
        }
    }

    /// Whether the box at the viewport's corner names the skeleton source's file as drawn.
    fn names_the_model(&self) -> bool {
        let name = (self.state.document(self.held))
            .map(|doc| doc.file_name())
            .expect("the skeleton source is open");
        let text = overlay_text(&self.state, None);
        text.lines()
            .any(|line| line.starts_with(&name) && line.contains(" \u{b7} "))
    }

    fn names_the_bones(&self) -> bool {
        let name = (self.state.document(self.held))
            .map(|doc| doc.file_name())
            .expect("the skeleton source is open");
        overlay_text(&self.state, None).contains(&format!("Bones of {name}"))
    }

    /// Whether the viewport has bones to draw: the skeleton is bound and its lines are not empty.
    fn draws_the_skeleton(&self) -> bool {
        let skeleton = self.state.anim.skeleton();
        skeleton.is_some_and(|skeleton| {
            let lines =
                build_pose_comparison(skeleton, &self.state.anim.world_matrices, false, None);
            !lines.is_empty()
        })
    }
}

#[test]
fn a_fragment_chosen_alone_has_the_box_ticked_and_draws_its_own_model() {
    for holder in Holder::ALL {
        let mut p = a_ped_alone(AppState::from_prefs(Prefs::default()), holder);
        assert!(
            p.settings.ticked(&mut p.state),
            "{holder:?}: the box starts unticked"
        );
        assert_eq!(
            p.drawn(),
            vec![Some(holder.mesh(p.held))],
            "{holder:?}: the preview does not draw the skeleton source's own model"
        );
        assert!(
            p.names_the_model(),
            "{holder:?}: the box does not name what is drawn: {}",
            overlay_text(&p.state, None)
        );
    }
}

/// The two staged Legacy peds hold a plane and nothing else
/// (`a_peds_fragment_chosen_alone_draws_the_one_plane_its_own_bounds_state`).
#[test]
fn a_ped_chosen_alone_finds_the_box_ticked_and_the_plane_drawn_until_it_is_unticked() {
    const WHAT: &str =
        "a_ped_chosen_alone_finds_the_box_ticked_and_the_plane_drawn_until_it_is_unticked";
    let _prefs = keep_prefs_to_this_test();
    let staged = Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft").find();
    for name in ["a_m_y_skater_01.yft", "a_f_y_hipster_01.yft"] {
        let Some(path) = (staged.iter()).find(|p| p.file_name().is_some_and(|n| n == name)) else {
            skip_corpus(WHAT, format_args!("{name} is not staged"));
            continue;
        };
        let mut state = AppState::from_prefs(Prefs::default());
        state.documents.clear();
        let fragment = open(&mut state, path);
        let mut p = previewing(state, fragment, SkeletonSource::Fragment(fragment));

        assert!(
            p.settings.ticked(&mut p.state),
            "{name}: the box starts unticked"
        );
        let drawn = build_meshes_for_active_document(&p.state);
        let [plane] = drawn.as_slice() else {
            panic!("{name}: the preview draws {} meshes", drawn.len());
        };
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for vertex in &plane.vertices {
            for axis in 0..3 {
                low[axis] = low[axis].min(vertex.position[axis]);
                high[axis] = high[axis].max(vertex.position[axis]);
            }
        }
        let side = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
        assert_eq!(
            side,
            [25.0, 25.0, 0.0],
            "{name}: the plane's sides in metres"
        );

        p.settings.click_the_box(&mut p.state);
        assert!(
            !p.settings.ticked(&mut p.state),
            "{name}: the click did not untick it"
        );
        assert!(
            this_tests_folder_holds_the_box_unticked() && other_tests_find_the_box_ticked(),
            "{name}: the click did not save to this test's folder, or reached the preferences other tests read"
        );
        assert!(
            p.drawn().is_empty(),
            "{name}: the plane is drawn with the box unticked"
        );
        p.settings.click_the_box(&mut p.state);
        assert_eq!(
            p.drawn().len(),
            1,
            "{name}: the plane is not drawn again once ticked"
        );
    }
}

#[test]
fn unticking_the_box_draws_no_mesh_but_the_skeleton_and_a_mesh_source_added_is_still_drawn() {
    let _prefs = keep_prefs_to_this_test();
    for holder in Holder::ALL {
        let mut p = a_ped_alone(AppState::from_prefs(Prefs::default()), holder);
        assert!(
            p.draws_the_skeleton(),
            "{holder:?}: the rig binds to the clip"
        );
        let own = holder.mesh(p.held);
        assert_eq!(p.drawn(), vec![Some(own)], "{holder:?}");
        let ticked = p.key();

        p.settings.click_the_box(&mut p.state);
        assert!(
            !p.settings.ticked(&mut p.state),
            "{holder:?}: the click did not untick the box"
        );
        assert!(
            this_tests_folder_holds_the_box_unticked() && other_tests_find_the_box_ticked(),
            "{holder:?}: the click did not save to this test's folder, or reached the preferences other tests read"
        );
        assert!(
            p.drawn().is_empty(),
            "{holder:?}: the skeleton source's own model is drawn with the box unticked"
        );
        assert!(
            p.key() != ticked,
            "{holder:?}: the viewport does not build its meshes again"
        );
        assert!(
            p.draws_the_skeleton(),
            "{holder:?}: unticking the box hid the skeleton"
        );
        assert!(
            p.names_the_bones() && !p.names_the_model(),
            "{holder:?}: the box does not say what is drawn: {}",
            overlay_text(&p.state, None)
        );
        assert!(
            p.state.preview_scene.drawn_documents().is_empty()
                && p.state.textured_documents().is_empty(),
            "{holder:?}: a click, a pin or a texture still reaches the hidden model"
        );
        assert!(
            p.settings
                .words(&mut p.state)
                .iter()
                .any(|line| line.contains("model is hidden")),
            "{holder:?}: the mesh section still says the skeleton's drawable is shown"
        );

        let wall = p.a_wall();
        p.state.preview_scene.add_mesh(wall);
        assert_eq!(
            p.drawn(),
            vec![Some(wall)],
            "{holder:?}: a mesh source added is not drawn alone"
        );
        p.state.preview_scene.remove_mesh(wall);
        assert!(
            p.drawn().is_empty(),
            "{holder:?}: the model came back with the mesh gone"
        );

        p.settings.click_the_box(&mut p.state);
        assert_eq!(
            p.drawn(),
            vec![Some(own)],
            "{holder:?}: ticking the box does not draw the model again"
        );
        assert_eq!(p.key(), ticked, "{holder:?}: nothing else moved");
    }
}

#[test]
fn the_choice_to_hide_the_skeleton_files_model_is_kept_across_a_restart() {
    let _prefs = keep_prefs_to_this_test();
    // A run of the editor: the preferences it finds, and a ped chosen alone.
    let run = || a_ped_alone(AppState::from_prefs(load_prefs()), Holder::Fragment);

    let mut first = run();
    assert!(
        first.settings.ticked(&mut first.state),
        "a first run starts ticked"
    );
    first.settings.click_the_box(&mut first.state);
    assert!(
        !first.settings.ticked(&mut first.state),
        "the click did not untick it"
    );
    assert!(first.drawn().is_empty());

    let mut second = run();
    assert!(
        !second.settings.ticked(&mut second.state),
        "a restart finds the box ticked again"
    );
    assert!(
        second.drawn().is_empty(),
        "a restart draws the model it was told to hide"
    );
    assert!(second.draws_the_skeleton(), "a restart hid the skeleton");
    second.settings.click_the_box(&mut second.state);

    let mut third = run();
    assert!(
        third.settings.ticked(&mut third.state),
        "a restart loses a ticked box"
    );
    assert_eq!(
        third.drawn().len(),
        1,
        "a restart does not draw the model it was told to"
    );
}

#[test]
fn a_preference_file_without_the_choice_draws_the_model() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = dir.path().join("prefs.json");
    for json in [
        serde_json::json!({}),
        serde_json::json!({ "viewport": { "show_grid": false } }),
    ] {
        std::fs::write(&file, json.to_string()).expect("prefs.json written");
        let state = AppState::from_prefs(load_prefs_from(&file));
        assert!(state.prefs.draw_skeleton_model, "{json}");
        assert!(state.preview_scene.draw_skeleton_model, "{json}");
    }
}

#[test]
fn a_skeleton_chosen_with_its_model_hidden_and_the_bones_off_is_not_told_to_choose_one() {
    let _prefs = keep_prefs_to_this_test();
    let mut p = a_ped_alone(AppState::from_prefs(Prefs::default()), Holder::ALL[0]);
    p.settings.click_the_box(&mut p.state);
    p.state.preview_scene.show_bones = false;
    assert!(p.state.preview_scene.drawn_sources().is_empty());
    let said = overlay_text(&p.state, None);
    assert!(
        said.contains("Nothing to draw"),
        "the box says nothing is drawn: {said}"
    );
    assert!(
        !said.contains("choose a mesh or a skeleton"),
        "a skeleton is chosen, and the box tells the person to choose one: {said}"
    );
    assert!(
        said.contains("Show bones"),
        "the box says what would draw something: {said}"
    );
}

#[test]
fn the_mesh_section_says_nothing_of_a_hidden_model_where_no_skeleton_is_offered() {
    let _prefs = keep_prefs_to_this_test();
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
    state.insert_document("C:/mods/clips.ycd".into(), clips, None, None);
    assert_eq!(state.active_workspace, Workspace::Animation);
    state.preview_scene.draw_skeleton_model = false;
    let mut settings = Settings::new();
    let words = settings.words(&mut state).join("\n");
    assert!(
        words.contains("None added"),
        "the section says no mesh is added: {words}"
    );
    assert!(
        !words.contains("model is hidden"),
        "the box the sentence is about is not drawn without a skeleton: {words}"
    );
}
