//! A click in the viewport picks every line it draws: the bind-pose ghost of the Animation
//! preview, and the physics bounds of a fragment and a `.ybn`'s bounds, which Wireframe draws. A
//! bone is picked while every mesh the preview was given draws nothing.

use glam::{Mat4, Vec3};
use or_io::decl::{Gen8, Gen9};
use or_io::gta5::{
    FragPhysicsLOD, FragPhysicsLODGroup, GtaDrawable, GtaFragType, PgDictionary, PhBoundAny,
};
use or_renderer::ShadingMode;

use super::prefs::Prefs;
use super::the_animation_preview_draws_what_its_sources_hold::CLIP;
use super::the_viewport_picks_what_it_draws::{
    a_fragment, click_on_the_sphere, embedding_nested_bounds, picked, previewing, ray,
    the_side_that_edge_faces, Held, Holder, HASH, MIDDLE_OF_THE_BONE, MOVED,
};
use super::viewport_basics_for_inspecting_assets::{
    the_outline_rows_labelled, the_outline_selects, the_outline_shows,
};
use super::{AppState, DocPayload, MeshSource};
use crate::render::bound_meshes::shapes::{box_bound, composite, sphere};
use crate::render::viewport::pick::{select_under, Ray};
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::{
    dict_entry_id, dict_entry_part_id, physics_bound_hash, YPT_MODEL_KIND_BOUND,
};

fn the_ghost_of<T: Held>() {
    for holder in Holder::ALL {
        let mut p = previewing::<T>(holder);
        // The pose has carried every bone far from where the bind pose stands, which the ghost draws.
        for world in &mut p.state.anim.world_matrices {
            *world = Mat4::from_translation(MOVED) * *world;
        }
        let click = || ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO);

        select_under(&mut p.state, &click());
        assert_eq!(
            picked(&p.state),
            None,
            "{holder:?}: a click where the ghost is not drawn picked something"
        );

        p.state.preview_scene.show_bind_pose_ghost = true;
        select_under(&mut p.state, &click());
        assert_eq!(
            picked(&p.state),
            Some((p.rig, "BONE_1 (1) in the bind pose")),
            "{holder:?}: a click on the ghost's segment missed its bone"
        );
        assert_eq!(
            p.state
                .preview_scene
                .picked
                .as_ref()
                .map(|picked| picked.row),
            Some(holder.bone(1)),
            "{holder:?}: the ghost's bone has the row the posed bone has"
        );

        // The ghost draws with the bones, and with them alone.
        p.state.preview_scene.show_bones = false;
        select_under(&mut p.state, &click());
        assert_eq!(
            picked(&p.state),
            None,
            "{holder:?}: a click picked a ghost the viewport does not draw"
        );
    }
}

#[test]
fn a_click_on_the_bind_pose_ghost_picks_the_bone_it_draws() {
    the_ghost_of::<Gen8>();
    the_ghost_of::<Gen9>();
}

fn a_bone_over_its_ghost<T: Held>() {
    for holder in Holder::ALL {
        let mut p = previewing::<T>(holder);
        // The pose rests where the bind pose stands, so a bone and its ghost lie on one segment,
        // and the one picked is the one that comes first in `bone_under`.
        let bind = (p.state.anim.skeleton()).map(|skeleton| skeleton.bind_world.clone());
        p.state.anim.world_matrices = bind.expect("a skeleton to pose");
        p.state.preview_scene.show_bind_pose_ghost = true;

        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            picked(&p.state),
            Some((p.rig, "BONE_1 (1)")),
            "{holder:?}: a bone and its ghost equally near gave the ghost"
        );
    }
}

#[test]
fn a_click_where_a_bone_and_its_ghost_are_equally_near_picks_the_posed_bone() {
    a_bone_over_its_ghost::<Gen8>();
    a_bone_over_its_ghost::<Gen9>();
}

/// What a fragment of a target is held as.
trait Fragmentary: Held {
    fn fragment_asset(fragment: GtaFragType<Self>) -> or_io::Asset;
}

impl Fragmentary for Gen8 {
    fn fragment_asset(fragment: GtaFragType<Self>) -> or_io::Asset {
        or_io::Asset::Fragment(fragment)
    }
}

impl Fragmentary for Gen9 {
    fn fragment_asset(fragment: GtaFragType<Self>) -> or_io::Asset {
        or_io::Asset::FragmentGen9(fragment)
    }
}

/// A fragment whose highest physics level holds `bound`, beside a common drawable of no geometry.
// The struct update the lint asks for does not compile: both hold private fields.
#[allow(clippy::field_reassign_with_default)]
fn with_physics_bound<T: Fragmentary>(bound: PhBoundAny) -> or_io::Asset {
    let mut level = FragPhysicsLOD::<T>::default();
    level.composite_bounds = Some(Box::new(bound));
    let mut group = FragPhysicsLODGroup::<T>::default();
    group.high_lod = Some(Box::new(level));
    let mut fragment = a_fragment(GtaDrawable::<T>::default());
    fragment.physics_lod_group = Some(Box::new(group));
    T::fragment_asset(fragment)
}

fn physics_bounds_of<T: Fragmentary>() {
    // A box a metre either way of the origin, clicked at the middle of its edge at y = -1, z = 1,
    // seen from the side that edge faces.
    let from = the_side_that_edge_faces();
    let click = |shift| ray(Vec3::new(0.0, -1.0, 1.0), from, 6.0, shift);
    let row = dict_entry_part_id(physics_bound_hash(0), YPT_MODEL_KIND_BOUND, 0);
    let mut state = AppState::from_prefs(Prefs::default());
    let bound = box_bound([0.0; 3], [1.0; 3]);
    let doc = state.insert_document(
        "C:/mods/held.yft".into(),
        DocPayload::Resource(with_physics_bound::<T>(bound)),
        None,
        None,
    );

    select_under(&mut state, &click(egui::Vec2::ZERO));
    assert_eq!(
        state.selection.selected(doc),
        None,
        "a fragment's physics bounds are picked where the viewport draws none"
    );

    state.prefs.viewport.shading = ShadingMode::Wireframe;
    select_under(&mut state, &click(egui::Vec2::ZERO));
    assert_eq!(
        state.selection.selected(doc),
        Some(row),
        "a click on the physics bound's edge in Wireframe missed it"
    );
    assert!(
        the_outline_shows(&mut state, "Bound"),
        "the Outline does not show the physics bound clicked"
    );

    // Twenty points is thirty pixels on this screen, past a line's reach.
    select_under(&mut state, &click(egui::vec2(0.0, 20.0)));
    assert_eq!(
        state.selection.selected(doc),
        None,
        "a click thirty pixels from the bound still picked it"
    );
}

#[test]
fn a_click_on_a_fragments_physics_bounds_picks_their_row() {
    physics_bounds_of::<Gen8>();
    physics_bounds_of::<Gen9>();
}

#[test]
fn a_click_on_a_ybns_bounds_picks_the_node_it_draws() {
    // The nodes number the composite 0, its box 1 and its sphere, five metres along x, 2.
    let bound = composite(vec![
        box_bound([0.0; 3], [1.0; 3]),
        sphere([5.0, 0.0, 0.0], 1.0),
    ]);
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document(
        "C:/mods/held.ybn".into(),
        DocPayload::Resource(or_io::Asset::Bounds(bound)),
        None,
        None,
    );
    let on_the_box = || {
        ray(
            Vec3::new(0.0, -1.0, 1.0),
            the_side_that_edge_faces(),
            6.0,
            egui::Vec2::ZERO,
        )
    };

    // Shaded draws the bound as meshes: a click on a face of the box picks its node, and one that
    // meets nothing leaves the selection where it was, as a miss in Wireframe does.
    let face = || ray(Vec3::new(0.0, 0.0, 0.0), Vec3::Y, 6.0, egui::Vec2::ZERO);
    select_under(&mut state, &face());
    assert_eq!(
        state.selection.selected(doc),
        Some(1),
        "a click on the box's face in Shaded missed its node"
    );
    select_under(
        &mut state,
        &ray(Vec3::new(0.0, 0.0, 50.0), Vec3::Y, 6.0, egui::Vec2::ZERO),
    );
    assert_eq!(
        state.selection.selected(doc),
        Some(1),
        "a click past the bound moved the selection"
    );
    select_under(
        &mut state,
        &ray(Vec3::new(5.0, 0.0, 0.0), Vec3::Y, 6.0, egui::Vec2::ZERO),
    );
    assert_eq!(
        state.selection.selected(doc),
        Some(2),
        "a click on the sphere's surface in Shaded missed its node"
    );

    state.prefs.viewport.shading = ShadingMode::Wireframe;
    select_under(&mut state, &on_the_box());
    assert_eq!(
        state.selection.selected(doc),
        Some(1),
        "a click on the box's edge missed its node"
    );
    select_under(&mut state, &click_on_the_sphere());
    assert_eq!(
        state.selection.selected(doc),
        Some(2),
        "a click on the sphere's circles missed its node"
    );
    assert!(
        the_outline_shows(&mut state, "Child 1 (sphere)"),
        "the Outline does not show the node clicked"
    );
}

#[test]
fn a_picked_node_inside_a_composite_ybn_bound_is_opened_in_the_outline() {
    // The nodes number the outer composite 0, the inner 1, its box 2 and its sphere 3, then the outer's box 4.
    let inner = composite(vec![
        box_bound([0.0; 3], [1.0; 3]),
        sphere([5.0, 0.0, 0.0], 1.0),
    ]);
    let bound = composite(vec![inner, box_bound([10.0, 0.0, 0.0], [1.0; 3])]);
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document(
        "C:/mods/held.ybn".into(),
        DocPayload::Resource(or_io::Asset::Bounds(bound)),
        None,
        None,
    );
    state.prefs.viewport.shading = ShadingMode::Wireframe;

    select_under(&mut state, &click_on_the_sphere());
    assert_eq!(state.selection.selected(doc), Some(3));
    // The inner composite is closed until the Outline is sent to a node inside it.
    assert!(
        the_outline_shows(&mut state, "Child 1 (sphere)"),
        "the Outline does not show the sphere in the closed composite"
    );
}

/// A click on the box that follows a composite the Outline shows closed, ten metres along x.
fn click_on_the_box_after_the_composite() -> Ray {
    ray(
        Vec3::new(10.0, -1.0, 1.0),
        the_side_that_edge_faces(),
        6.0,
        egui::Vec2::ZERO,
    )
}

#[test]
fn a_picked_ybn_node_after_a_closed_composite_is_the_row_the_outline_selects() {
    // The nodes number the outer composite 0, the inner 1, its box 2 and its sphere 3, then the outer's box 4.
    let inner = composite(vec![
        box_bound([0.0; 3], [1.0; 3]),
        sphere([5.0, 0.0, 0.0], 1.0),
    ]);
    let bound = composite(vec![inner, box_bound([10.0, 0.0, 0.0], [1.0; 3])]);
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document(
        "C:/mods/held.ybn".into(),
        DocPayload::Resource(or_io::Asset::Bounds(bound)),
        None,
        None,
    );
    state.prefs.viewport.shading = ShadingMode::Wireframe;

    select_under(&mut state, &click_on_the_box_after_the_composite());
    assert_eq!(state.selection.selected(doc), Some(4));
    // The inner composite stays closed, since the box is not inside it, and the Outline still
    // numbers the box as the pick does.
    assert!(
        the_outline_selects(&mut state, "Child 1 (box)"),
        "the Outline has no selected row for the box after the closed composite"
    );
}

fn a_drawables_bound_after_a_closed_composite<T: Held>() {
    let mut state = AppState::from_prefs(Prefs::default());
    let holder = Holder::Drawable;
    let doc = state.insert_document(
        holder.file().into(),
        DocPayload::Resource(holder.asset(embedding_nested_bounds::<T>())),
        None,
        None,
    );
    state.preview_scene.show_collision = true;

    select_under(&mut state, &click_on_the_box_after_the_composite());
    assert_eq!(state.selection.selected(doc), Some(holder.bound(4)));
    assert!(
        the_outline_selects(&mut state, "Child 1 (box)"),
        "the Outline has no selected row for the box after the closed composite"
    );
}

#[test]
fn a_picked_drawable_bound_node_after_a_closed_composite_is_the_row_the_outline_selects() {
    a_drawables_bound_after_a_closed_composite::<Gen8>();
    a_drawables_bound_after_a_closed_composite::<Gen9>();
}

#[test]
fn a_bone_is_picked_while_every_mesh_the_preview_was_given_draws_nothing() {
    for holder in Holder::ALL {
        let mut p = previewing::<Gen8>(holder);
        // A dictionary whose one entry could not be read draws nothing, and while a mesh is
        // chosen the rig's own drawable is not what the preview draws.
        let dictionary = PgDictionary::<GtaDrawable<Gen8>> {
            codes: vec![HASH],
            entries: vec![None],
            ..Default::default()
        };
        let empty = p.state.insert_document(
            "C:/mods/empty.ydd".into(),
            DocPayload::Resource(or_io::Asset::DrawableDict(dictionary)),
            None,
            None,
        );
        // Opening a dictionary switches to the workspace that shows it.
        p.state.active_workspace = Workspace::Animation;
        p.state.active_document = Some(p.clips);
        p.state.preview_scene.add_mesh(MeshSource {
            document: empty,
            entry: Some(0),
        });
        assert_eq!(p.state.preview_scene.drawn_sources(), [(empty, Some(0))]);
        assert!(p.state.anim.skeleton().is_some(), "the skeleton is shown");
        let clip_row = dict_entry_id(CLIP);
        p.state.selection.select(p.clips, clip_row);

        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            picked(&p.state),
            Some((p.rig, "BONE_1 (1)")),
            "{holder:?}: a click on the bone missed it with no mesh drawn"
        );

        // A click on nothing, with nothing drawn, leaves the clip where it was.
        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::vec2(0.0, 20.0)),
        );
        assert_eq!(p.state.selection.selected(p.clips), Some(clip_row));
    }
}

/// A fragment whose High and Medium physics levels each hold a box.
#[allow(clippy::field_reassign_with_default)]
fn with_two_physics_levels<T: Fragmentary>() -> or_io::Asset {
    let level = || {
        let mut level = FragPhysicsLOD::<T>::default();
        level.composite_bounds = Some(Box::new(box_bound([0.0; 3], [1.0; 3])));
        Some(Box::new(level))
    };
    let mut group = FragPhysicsLODGroup::<T>::default();
    group.high_lod = level();
    group.medium_lod = level();
    let mut fragment = a_fragment(GtaDrawable::<T>::default());
    fragment.physics_lod_group = Some(Box::new(group));
    T::fragment_asset(fragment)
}

fn the_outline_of_a_fragments_physics<T: Fragmentary>() {
    let mut state = AppState::from_prefs(Prefs::default());
    state.insert_document(
        "C:/mods/held.yft".into(),
        DocPayload::Resource(with_two_physics_levels::<T>()),
        None,
        None,
    );
    assert_eq!(
        the_outline_rows_labelled(&mut state, "Bound"),
        1,
        "the Outline lists a Bound row for each physics level, and the viewport draws only the High one"
    );
}

#[test]
fn the_outline_lists_a_bound_row_for_the_physics_level_the_viewport_draws_and_no_other() {
    the_outline_of_a_fragments_physics::<Gen8>();
    the_outline_of_a_fragments_physics::<Gen9>();
}

/// One context the Outline is drawn on, a frame at a time, on a screen tall enough for every row.
struct Outline {
    ctx: egui::Context,
    time: f64,
}

impl Outline {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self { ctx, time: 0.0 }
    }

    /// The label and centre of every widget a frame of the Outline drew.
    fn frame(
        &mut self,
        state: &mut AppState,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Pos2)> {
        self.time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 400.0),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let output = (self.ctx).run_ui(input, |ui| {
            crate::ui::panels::hierarchy::draw_outline(ui, state)
        });
        let nodes = output.platform_output.accesskit_update.map(|u| u.nodes);
        (nodes.into_iter().flatten())
            .filter_map(|(_, node)| {
                let b = node.bounds()?;
                let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                Some((node.label().or_else(|| node.value())?.to_owned(), at))
            })
            .collect()
    }

    /// The Outline after a few frames: what it drew last.
    fn settled(&mut self, state: &mut AppState) -> Vec<(String, egui::Pos2)> {
        let mut drawn = Vec::new();
        for _ in 0..8 {
            drawn = self.frame(state, Vec::new());
        }
        drawn
    }

    fn click(&mut self, state: &mut AppState, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(state, vec![egui::Event::PointerMoved(at)]);
        self.frame(state, vec![button(true)]);
        self.frame(state, vec![button(false)]);
    }
}

fn a_physics_bound_picked_inside_a_collapsed_section<T: Fragmentary>() {
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document(
        "C:/mods/held.yft".into(),
        DocPayload::Resource(with_physics_bound::<T>(box_bound([0.0; 3], [1.0; 3]))),
        None,
        None,
    );
    let mut outline = Outline::new();
    let shows = |drawn: &[(String, egui::Pos2)], label: &str| {
        drawn.iter().any(|(text, _)| text.contains(label))
    };
    let drawn = outline.settled(&mut state);
    assert!(
        shows(&drawn, "Bound"),
        "the Physics section starts open: {drawn:?}"
    );

    let (_, header) = drawn
        .iter()
        .find(|(text, _)| text.starts_with("Physics ("))
        .expect("the Physics section is listed");
    let (_, toggle) = drawn
        .iter()
        .find(|(text, at)| text == "Collapse" && (at.y - header.y).abs() < 1.0)
        .expect("the Physics header has a button that collapses it");
    outline.click(&mut state, *toggle);
    let drawn = outline.settled(&mut state);
    assert!(
        !shows(&drawn, "Bound"),
        "a click on the Physics header did not close it: {drawn:?}"
    );

    state.prefs.viewport.shading = ShadingMode::Wireframe;
    let from = the_side_that_edge_faces();
    select_under(
        &mut state,
        &ray(Vec3::new(0.0, -1.0, 1.0), from, 6.0, egui::Vec2::ZERO),
    );
    assert_eq!(
        state.selection.selected(doc),
        Some(dict_entry_part_id(
            physics_bound_hash(0),
            YPT_MODEL_KIND_BOUND,
            0
        )),
        "the click missed the physics bound"
    );
    let drawn = outline.settled(&mut state);
    assert!(
        shows(&drawn, "Bound"),
        "a pick of the physics bound left its row inside a closed section: {drawn:?}"
    );
}

#[test]
fn a_click_on_a_physics_bound_opens_the_section_that_holds_its_row() {
    a_physics_bound_picked_inside_a_collapsed_section::<Gen8>();
    a_physics_bound_picked_inside_a_collapsed_section::<Gen9>();
}

/// A `.ynv` of two triangles in a quadtree box ten metres a side: one at the origin corner and one at the far corner.
fn a_navmesh_of_two_triangles() -> or_io::gta5::CNavMesh {
    use or_io::gta5::{
        AiSplitArray, CNavMesh, CNavMeshCompressedVertex, CNavMeshQuadTree, SubArray, TNavMeshPoly,
        Vector3,
    };
    fn split<T: or_io::decl::Owned>(elements: Vec<T>) -> Option<Box<AiSplitArray<T>>>
    where
        or_io::decl::Raw<T>: or_io::decl::Plain + or_io::decl::Element + or_io::decl::Param,
    {
        Some(Box::new(AiSplitArray {
            sub_arrays: vec![SubArray {
                elements,
                ..Default::default()
            }],
            ..Default::default()
        }))
    }
    let at = |x: u16, y: u16| CNavMeshCompressedVertex {
        data: [x, y, 0],
        ..Default::default()
    };
    let corner = |x: f32, y: f32, z: f32| Vector3 {
        x,
        y,
        z,
        ..Default::default()
    };
    let mut triangle = TNavMeshPoly::default();
    triangle.set_num_vertices(3).expect("a triangle");
    let (low, high) = (39321, 65535);
    CNavMesh {
        quad_tree: Some(Box::new(CNavMeshQuadTree {
            mins: corner(0.0, 0.0, 0.0),
            maxs: corner(10.0, 10.0, 10.0),
            ..Default::default()
        })),
        compressed_vertex_array: split(vec![
            at(0, 0),
            at(high, 0),
            at(0, high),
            at(low, low),
            at(high, low),
            at(low, high),
        ]),
        vertex_index_array: split::<u16>(vec![0, 1, 2, 3, 4, 5]),
        polys_array: split(vec![triangle.clone(), triangle]),
        ..Default::default()
    }
}

#[test]
fn a_click_on_a_navmesh_polygon_selects_its_row_which_the_outline_opens_and_the_viewport_highlights(
) {
    use crate::render::skeleton_lines::SELECTED;
    use crate::ui::panels::selection_ids::nav_polygon_id;
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document(
        "C:/mods/cell.ynv".into(),
        DocPayload::Resource(or_io::Asset::NavMesh(a_navmesh_of_two_triangles())),
        None,
        None,
    );
    let highlighted = |state: &AppState| {
        crate::render::lines::build_lines_for_active_document(state)
            .iter()
            .filter(|vertex| vertex.colour == SELECTED)
            .count()
    };
    let shows = |drawn: &[(String, egui::Pos2)], label: &str| {
        drawn.iter().any(|(text, _)| text.contains(label))
    };
    let mut outline = Outline::new();
    assert!(
        !shows(&outline.settled(&mut state), "Polygon 1"),
        "the Polygons section starts open"
    );
    assert_eq!(
        highlighted(&state),
        0,
        "a polygon is highlighted with none selected"
    );

    // Down onto the far triangle, then onto the near one
    let onto = |x: f32, y: f32| ray(Vec3::new(x, y, 0.0), Vec3::Z, 20.0, egui::Vec2::ZERO);
    let unpicked = crate::render::viewport::LinesCacheKey::of(&state, doc);
    select_under(&mut state, &onto(7.0, 7.0));
    assert_ne!(
        crate::render::viewport::LinesCacheKey::of(&state, doc),
        unpicked,
        "the viewport keeps the lines it built before the pick"
    );
    assert_eq!(
        state.selection.selected(doc),
        Some(nav_polygon_id(1)),
        "a click on the far triangle missed its polygon"
    );
    let drawn = outline.settled(&mut state);
    assert!(
        shows(&drawn, "Polygon 1"),
        "a pick left its row inside a closed section: {drawn:?}"
    );
    assert_eq!(
        highlighted(&state),
        6,
        "the picked polygon's outline is not highlighted"
    );

    select_under(&mut state, &onto(2.0, 2.0));
    assert_eq!(state.selection.selected(doc), Some(nav_polygon_id(0)));

    // A click on a row of the Outline selects the polygon as well
    let drawn = outline.settled(&mut state);
    let (_, row) = drawn
        .iter()
        .find(|(text, _)| text.contains("Polygon 1"))
        .expect("the row of polygon 1");
    outline.click(&mut state, *row);
    assert_eq!(
        state.selection.selected(doc),
        Some(nav_polygon_id(1)),
        "a click on a polygon's row did not select it"
    );
}
