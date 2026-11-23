//! A click in the viewport reaches a bone, a bound and a model as well as a geometry; in the
//! Animation workspace it reaches every mesh the preview draws, in the document it comes from; and
//! the box there names what the preview draws.

use glam::{Mat4, Vec3};
use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexLayout};
use or_io::decl::{Gen8, Gen9, Hash32};
use or_io::gta5::gen8;
use or_io::gta5::{
    DrawableTarget, FragDrawable, GrmGeometry, GrmModel, GrmShaderGroup, GtaDrawable, GtaFragType,
    PgDictionary, PhBoundAny, RmcLod,
};
use or_renderer::{OrbitCamera, ShadingMode};
use or_test_support::{skip_corpus, TARGET_GEN8, TARGET_GEN9};

use super::prefs::Prefs;
use super::textures::FRAGMENT_ENTRY_HASH;
use super::the_animation_preview_draws_what_its_sources_hold::{a_clip_over_the_rig, rigged, CLIP};
use super::viewport_basics_for_inspecting_assets::{
    opened, staged, the_last_in_sight, the_outline_shows, two_or_more,
};
use super::{AppState, DocPayload, DocumentId, MeshSource, SkeletonSource};
use crate::render::bound_meshes::shapes::{box_bound, composite, sphere};
use crate::render::viewport::pick::{select_model_under, select_under, Ray};
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::{
    dict_entry_id, dict_entry_part_id, pack_index, pack_lod_sub_index, pack_selection, SECT_BOUND,
    SECT_SKEL, YPT_MODEL_KIND_BONE, YPT_MODEL_KIND_BOUND, YPT_MODEL_KIND_LOD,
};

/// The hash a dictionary here holds its one entry under.
pub(super) const HASH: u32 = 0x1234_5678;

/// Every bone moved straight up and far, so no ray towards a face where the bind pose put it meets
/// the moved mesh.
pub(super) const MOVED: Vec3 = Vec3::new(0.0, 0.0, 1000.0);

/// What a target's drawable, dictionary and fragment become as an asset.
pub(super) trait Held: DrawableTarget + Sized {
    fn drawable(drawable: GtaDrawable<Self>) -> or_io::Asset;
    fn dictionary(drawable: GtaDrawable<Self>) -> or_io::Asset;
    fn fragment(drawable: GtaDrawable<Self>) -> or_io::Asset;
}

pub(super) fn one_entry<T: DrawableTarget>(
    drawable: GtaDrawable<T>,
) -> PgDictionary<GtaDrawable<T>> {
    PgDictionary {
        codes: vec![HASH],
        entries: vec![Some(Box::new(drawable))],
        ..Default::default()
    }
}

// The struct update the lint asks for does not compile: the fragment holds private fields.
#[allow(clippy::field_reassign_with_default)]
pub(super) fn a_fragment<T: DrawableTarget>(drawable: GtaDrawable<T>) -> GtaFragType<T> {
    let mut common = FragDrawable::<T>::default();
    common.base = drawable.base;
    let mut fragment = GtaFragType::<T>::default();
    fragment.common_drawable = Some(Box::new(common));
    fragment
}

impl Held for Gen8 {
    fn drawable(drawable: GtaDrawable<Self>) -> or_io::Asset {
        or_io::Asset::Drawable(drawable)
    }
    fn dictionary(drawable: GtaDrawable<Self>) -> or_io::Asset {
        or_io::Asset::DrawableDict(one_entry(drawable))
    }
    fn fragment(drawable: GtaDrawable<Self>) -> or_io::Asset {
        or_io::Asset::Fragment(a_fragment(drawable))
    }
}

impl Held for Gen9 {
    fn drawable(drawable: GtaDrawable<Self>) -> or_io::Asset {
        or_io::Asset::DrawableGen9(drawable)
    }
    fn dictionary(drawable: GtaDrawable<Self>) -> or_io::Asset {
        or_io::Asset::DrawableDictGen9(one_entry(drawable))
    }
    fn fragment(drawable: GtaDrawable<Self>) -> or_io::Asset {
        or_io::Asset::FragmentGen9(a_fragment(drawable))
    }
}

/// How a drawable is held: by itself, as the one entry of a dictionary, or as a fragment's own.
#[derive(Clone, Copy, Debug)]
pub(super) enum Holder {
    Drawable,
    Entry,
    Fragment,
}

impl Holder {
    pub(super) const ALL: [Self; 3] = [Self::Drawable, Self::Entry, Self::Fragment];

    pub(super) fn asset<T: Held>(self, drawable: GtaDrawable<T>) -> or_io::Asset {
        match self {
            Self::Drawable => T::drawable(drawable),
            Self::Entry => T::dictionary(drawable),
            Self::Fragment => T::fragment(drawable),
        }
    }

    pub(super) fn file(self) -> &'static str {
        match self {
            Self::Drawable => "C:/mods/held.ydr",
            Self::Entry => "C:/mods/held.ydd",
            Self::Fragment => "C:/mods/held.yft",
        }
    }

    /// The hash the Outline names the drawable's parts under; a drawable's own tree names them by section.
    fn hash(self) -> Option<u32> {
        match self {
            Self::Drawable => None,
            Self::Entry => Some(HASH),
            Self::Fragment => Some(FRAGMENT_ENTRY_HASH),
        }
    }

    /// The row the Outline gives bone `bone`.
    pub(super) fn bone(self, bone: u32) -> u64 {
        match self.hash() {
            None => pack_index(SECT_SKEL, bone),
            Some(hash) => dict_entry_part_id(hash, YPT_MODEL_KIND_BONE, bone),
        }
    }

    /// The row the Outline gives the geometry `geometry` of the first model of the first level.
    pub(super) fn geometry(self, geometry: u32) -> u64 {
        match self.hash() {
            None => pack_selection(0, 0, Some(geometry)),
            Some(hash) => {
                let sub = pack_lod_sub_index(0, 0, Some(geometry as u16));
                dict_entry_part_id(hash, YPT_MODEL_KIND_LOD, sub)
            }
        }
    }

    /// The row the Outline gives the first model of the first level.
    pub(super) fn model(self) -> u64 {
        match self.hash() {
            None => pack_selection(0, 0, None),
            Some(hash) => {
                dict_entry_part_id(hash, YPT_MODEL_KIND_LOD, pack_lod_sub_index(0, 0, None))
            }
        }
    }

    /// The row the Outline gives the bound's node `node`: an entry has the one.
    pub(super) fn bound(self, node: u32) -> u64 {
        match self.hash() {
            None => pack_index(SECT_BOUND, node),
            Some(hash) => dict_entry_part_id(hash, YPT_MODEL_KIND_BOUND, 0),
        }
    }

    /// The skeleton source this holder makes of the document that holds the drawable.
    pub(super) fn skeleton(self, doc: DocumentId) -> SkeletonSource {
        match self {
            Self::Drawable => SkeletonSource::Drawable(doc),
            Self::Entry => SkeletonSource::DictionaryEntry(doc, 0),
            Self::Fragment => SkeletonSource::Fragment(doc),
        }
    }

    /// The mesh source this holder makes of the document that holds the drawable.
    pub(super) fn mesh(self, doc: DocumentId) -> MeshSource {
        MeshSource {
            document: doc,
            entry: matches!(self, Self::Entry).then_some(0),
        }
    }
}

/// A camera `distance` from `at` towards `from`, a unit vector, looking at it.
pub(super) fn camera(at: Vec3, from: Vec3, distance: f32) -> OrbitCamera {
    OrbitCamera {
        target: at,
        yaw: from.y.atan2(from.x),
        pitch: from.z.asin(),
        distance,
        ..OrbitCamera::default()
    }
}

/// A ray through the middle of a viewport, as if it were `distance` from `at` towards `from` (a
/// unit vector), moved `shift` points over the screen.
pub(super) fn ray(at: Vec3, from: Vec3, distance: f32, shift: egui::Vec2) -> Ray {
    let camera = camera(at, from, distance);
    let rect = egui::Rect::from_min_size(egui::pos2(300.0, 40.0), egui::vec2(800.0, 500.0));
    Ray::at(&camera, rect, (1200, 750), rect.center() + shift).expect("a ray")
}

/// A triangle of `corners`, skinned wholly to bone `bone` where there is one.
fn triangle(corners: [[f32; 3]; 3], bone: Option<u16>) -> GrmGeometry<Gen8> {
    let mut channels = vec![
        (GrcFvfChannel::Position, GrcDataSize::Float3),
        (GrcFvfChannel::Normal, GrcDataSize::Float3),
    ];
    if bone.is_some() {
        channels.push((GrcFvfChannel::Weight, GrcDataSize::Color));
        channels.push((GrcFvfChannel::Binding, GrcDataSize::UByte4));
    }
    channels.push((GrcFvfChannel::Tex0, GrcDataSize::Float2));
    let mut fvf = gen8::GrcFvf::default();
    for &(channel, size) in &channels {
        fvf.fvf |= 1 << channel as u32;
        fvf.channel_sizes |= (size as u64) << (channel as u64 * 4);
    }
    fvf.channel_count = channels.len() as u8;
    fvf.fvf_size = VertexLayout::of(fvf.fvf, fvf.channel_sizes).packed_stride() as u8;

    let mut data = Vec::new();
    for corner in corners {
        data.extend(corner.iter().flat_map(|axis| axis.to_le_bytes()));
        data.extend(
            [0.0f32, 1.0, 0.0]
                .iter()
                .flat_map(|axis| axis.to_le_bytes()),
        );
        if bone.is_some() {
            data.extend([255u8, 0, 0, 0]);
            data.extend([1u8, 0, 0, 0]);
        }
        data.extend([0.0f32, 0.0].iter().flat_map(|axis| axis.to_le_bytes()));
    }
    GrmGeometry {
        primitive_type: 3,
        vertex_buffer: Box::new(gen8::GrcVertexBuffer {
            data,
            fvf: Some(Box::new(fvf)),
            ..Default::default()
        }),
        index_buffer: Box::new(gen8::GrcIndexBuffer {
            indices: vec![0, 1, 2],
            ..Default::default()
        }),
        // Slot 1 is bone 1, which the weights name.
        matrix_palette: bone.map_or_else(Vec::new, |bone| vec![0, bone]),
        ..Default::default()
    }
}

/// A Legacy drawable of one triangle, with `bound` embedded where there is one.
// The struct update the lint asks for does not compile: the drawable holds private fields.
#[allow(clippy::field_reassign_with_default)]
pub(super) fn a_triangle(
    corners: [[f32; 3]; 3],
    bone: Option<u16>,
    bound: Option<PhBoundAny>,
) -> GtaDrawable<Gen8> {
    let model = GrmModel {
        aabbs: vec![Default::default()],
        shader_indices: vec![0],
        geometries: vec![Box::new(triangle(corners, bone))],
        ..Default::default()
    };
    let mut drawable = GtaDrawable::<Gen8>::default();
    drawable.base.shader_group = Some(Box::new(GrmShaderGroup {
        shaders: vec![Box::new(gen8::GrcInstanceData {
            basis_hash: Hash32::of("default"),
            ..Default::default()
        })],
        ..Default::default()
    }));
    drawable.base.lod_group.lods[0] = RmcLod {
        models: vec![Box::new(model)],
        ..Default::default()
    }
    .into();
    drawable.bound = bound.map(Box::new);
    drawable
}

/// A drawable of target `T` embedding a composite of a composite and a box, the inner one holding a
/// box and then a sphere five metres along x, so the nodes number root 0, inner 1, box 2, sphere 3, box 4.
#[allow(clippy::field_reassign_with_default)]
pub(super) fn embedding_nested_bounds<T: DrawableTarget>() -> GtaDrawable<T> {
    let mut drawable = GtaDrawable::<T>::default();
    let inner = composite(vec![
        box_bound([0.0; 3], [1.0; 3]),
        sphere([5.0, 0.0, 0.0], 1.0),
    ]);
    let outer = composite(vec![inner, box_bound([10.0, 0.0, 0.0], [1.0; 3])]);
    drawable.bound = Some(Box::new(outer));
    drawable
}

/// The Animation workspace previewing `rig` against a clip dictionary, with the bones shown and
/// the clip evaluated; the clip dictionary is the active document.
pub(super) struct Preview {
    pub(super) state: AppState,
    /// The document that holds the rig.
    pub(super) rig: DocumentId,
    pub(super) clips: DocumentId,
}

pub(super) fn previewing<T: Held>(holder: Holder) -> Preview {
    let mut state = AppState::from_prefs(Prefs::default());
    let rig = state.insert_document(
        holder.file().into(),
        DocPayload::Resource(holder.asset(rigged::<T>())),
        None,
        None,
    );
    let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
    let clips = state.insert_document("C:/mods/clips.ycd".into(), clips, None, None);
    assert_eq!(state.active_workspace, Workspace::Animation);
    state.preview_scene.set_clip(clips, CLIP);
    state.preview_scene.pin_skeleton(Some(holder.skeleton(rig)));
    state.preview_scene.show_bones = true;
    state.tick_animation();
    assert!(state.anim.skeleton().is_some(), "the clip binds to the rig");
    Preview { state, rig, clips }
}

/// The middle of the rig's second bone, whose segment runs from the root at y = 1 to y = 2.
pub(super) const MIDDLE_OF_THE_BONE: Vec3 = Vec3::new(0.0, 1.5, 0.0);

fn bones_of<T: Held>() {
    for holder in Holder::ALL {
        let mut p = previewing::<T>(holder);
        // ⚠ The Outline lists the active document, which in the Animation workspace is the clip
        // dictionary the app activates: look at the rig's.
        p.state.active_document = Some(p.rig);

        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            p.state.selection.selected(p.rig),
            Some(holder.bone(1)),
            "{holder:?}: a click on the bone's segment missed it"
        );
        assert!(
            the_outline_shows(&mut p.state, "BONE_1 (1)"),
            "{holder:?}: the Outline does not show the bone clicked"
        );

        // Twenty points is thirty pixels on this screen, past the bone's reach.
        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::vec2(0.0, 20.0)),
        );
        assert_eq!(
            p.state.selection.selected(p.rig),
            None,
            "{holder:?}: a click thirty pixels from any bone still picked one"
        );

        p.state.preview_scene.show_bones = false;
        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            p.state.selection.selected(p.rig),
            None,
            "{holder:?}: a click picked a bone the viewport does not draw"
        );
    }
}

#[test]
fn a_click_on_a_bone_selects_its_row_in_the_outline() {
    bones_of::<Gen8>();
    bones_of::<Gen9>();
}

#[test]
fn a_bone_is_picked_through_a_mesh_nearer_the_camera() {
    for holder in Holder::ALL {
        let mut p = previewing::<Gen8>(holder);
        // A wall across the camera's view of the bone, a metre from the bone and nearer the camera.
        let wall = [[1.0, -0.5, -2.0], [1.0, 3.5, -2.0], [1.0, 1.5, 2.0]];
        let mesh = p.state.insert_document(
            "C:/mods/wall.ydr".into(),
            DocPayload::Resource(or_io::Asset::Drawable(a_triangle(wall, None, None))),
            None,
            None,
        );
        p.state.active_workspace = Workspace::Animation;
        p.state.active_document = Some(p.clips);
        p.state.preview_scene.add_mesh(MeshSource {
            document: mesh,
            entry: None,
        });
        let click = || ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO);

        select_under(&mut p.state, &click());
        assert_eq!(
            picked(&p.state),
            Some((p.rig, "BONE_1 (1)")),
            "{holder:?}: the wall hid the bone, which draws in front of it"
        );

        p.state.preview_scene.show_bones = false;
        select_under(&mut p.state, &click());
        assert_eq!(
            picked(&p.state),
            Some((mesh, "Geometry 0 of model 0")),
            "{holder:?}: with the bones hidden the wall is what the click lands on"
        );
    }
}

/// What the Animation preview says was picked, and in which document.
pub(super) fn picked(state: &AppState) -> Option<(DocumentId, &str)> {
    let picked = state.preview_scene.picked.as_ref()?;
    Some((picked.document, picked.label.as_str()))
}

/// A click on the sphere, at a point on two of its circles.
pub(super) fn click_on_the_sphere() -> Ray {
    let from = Vec3::new(0.3, -1.0, 0.6).normalize();
    ray(Vec3::new(6.0, 0.0, 0.0), from, 10.0, egui::Vec2::ZERO)
}

fn bound_of<T: Held>() {
    for holder in [Holder::Drawable, Holder::Entry] {
        let mut state = AppState::from_prefs(Prefs::default());
        let doc = state.insert_document(
            holder.file().into(),
            DocPayload::Resource(holder.asset(embedding_nested_bounds::<T>())),
            None,
            None,
        );

        select_under(&mut state, &click_on_the_sphere());
        assert_ne!(
            state.selection.selected(doc),
            Some(holder.bound(3)),
            "{holder:?}: the bound is picked with Collision off"
        );

        state.preview_scene.show_collision = true;
        select_under(&mut state, &click_on_the_sphere());
        assert_eq!(
            state.selection.selected(doc),
            Some(holder.bound(3)),
            "{holder:?}: a click on the sphere's circles missed its node"
        );
        // The sphere is the one bound of its type, in a composite the Outline shows closed.
        let label = match holder {
            Holder::Drawable => "sphere",
            _ => "Bound",
        };
        assert!(
            the_outline_shows(&mut state, label),
            "{holder:?}: the Outline does not show the bound clicked"
        );
    }
}

#[test]
fn a_click_on_a_bound_selects_its_row_in_the_outline() {
    bound_of::<Gen8>();
    bound_of::<Gen9>();
}

/// The side the middle of a box's edge at y = -1, z = 1 faces, as a unit vector from it.
pub(super) fn the_side_that_edge_faces() -> Vec3 {
    Vec3::new(0.35, -1.0, 1.0).normalize()
}

/// A triangle across the line of sight to the middle of that edge, `at` metres from it towards `from`.
pub(super) fn wall_across_the_edge(from: Vec3, at: f32) -> [[f32; 3]; 3] {
    let up = from.cross(Vec3::X).normalize();
    let side = up.cross(from).normalize();
    let centre = Vec3::new(0.0, -1.0, 1.0) + from * at;
    [
        centre - side * 3.0 - up * 3.0,
        centre + side * 3.0 - up * 3.0,
        centre + up * 3.0,
    ]
    .map(|corner| corner.to_array())
}

#[test]
fn a_bound_behind_a_mesh_is_not_picked_and_in_front_of_it_is() {
    // A box a metre either way of the origin, clicked at the middle of its edge at y = -1, z = 1,
    // seen from the side that edge faces.
    let from = the_side_that_edge_faces();
    let wall = |at: f32| wall_across_the_edge(from, at);
    for (at, expected) in [(0.5, "the wall"), (-0.5, "the bound")] {
        let bound = Some(box_bound([0.0; 3], [1.0; 3]));
        let mut state = AppState::from_prefs(Prefs::default());
        let doc = state.insert_document(
            "C:/mods/held.ydr".into(),
            DocPayload::Resource(or_io::Asset::Drawable(a_triangle(wall(at), None, bound))),
            None,
            None,
        );
        state.preview_scene.show_collision = true;

        select_under(
            &mut state,
            &ray(Vec3::new(0.0, -1.0, 1.0), from, 6.0, egui::Vec2::ZERO),
        );
        let picked = match state.selection.selected(doc) {
            Some(row) if row == pack_selection(0, 0, Some(0)) => "the wall",
            Some(row) if row == pack_index(SECT_BOUND, 0) => "the bound",
            other => panic!("a wall at {at}: the click selected {other:?}"),
        };
        assert_eq!(picked, expected, "a wall at {at} from the bound's edge");

        // Wireframe draws no mesh to hide the line.
        state.prefs.viewport.shading = ShadingMode::Wireframe;
        select_under(
            &mut state,
            &ray(Vec3::new(0.0, -1.0, 1.0), from, 6.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            state.selection.selected(doc),
            Some(pack_index(SECT_BOUND, 0)),
            "a wall at {at}: Wireframe hid the bound behind a mesh it does not draw"
        );
    }
}

#[test]
fn a_hidden_edge_nearer_the_cursor_does_not_take_the_click_from_a_visible_one() {
    // The box above with its far edge (y = 1, z = -1) moved two millimetres off the line of sight
    // through the near one, so the two draw 0.2 pixels apart instead of as one line. A wall
    // between them hides the far edge. A click a point to either side of the near edge is nearer
    // the far edge on one of the sides.
    let from = the_side_that_edge_faces();
    let mut state = AppState::from_prefs(Prefs::default());
    let bound = Some(box_bound([0.0, 0.001, 0.001], [1.0, 1.001, 0.999]));
    let corners = wall_across_the_edge(from, -0.5);
    let doc = state.insert_document(
        "C:/mods/held.ydr".into(),
        DocPayload::Resource(or_io::Asset::Drawable(a_triangle(corners, None, bound))),
        None,
        None,
    );
    state.preview_scene.show_collision = true;

    for side in [-1.0, 1.0] {
        let beside = egui::vec2(0.0, side);
        select_under(
            &mut state,
            &ray(Vec3::new(0.0, -1.0, 1.0), from, 6.0, beside),
        );
        assert_eq!(
            state.selection.selected(doc),
            Some(pack_index(SECT_BOUND, 0)),
            "a click {side} points beside the near edge, with the far edge hidden behind a wall"
        );
    }
}

#[test]
fn an_alt_click_selects_the_model_that_holds_the_geometry() {
    for holder in Holder::ALL {
        let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let mut state = AppState::from_prefs(Prefs::default());
        let doc = state.insert_document(
            holder.file().into(),
            DocPayload::Resource(holder.asset(a_triangle(corners, None, None))),
            None,
            None,
        );
        let click = || ray(Vec3::new(0.25, 0.0, 0.25), -Vec3::Y, 3.0, egui::Vec2::ZERO);

        select_under(&mut state, &click());
        assert_eq!(
            state.selection.selected(doc),
            Some(holder.geometry(0)),
            "{holder:?}"
        );

        select_model_under(&mut state, &click());
        assert_eq!(
            state.selection.selected(doc),
            Some(holder.model()),
            "{holder:?}: an Alt+click selected the geometry, not its model"
        );
        assert!(
            the_outline_shows(&mut state, "Model 0 \u{2022}"),
            "{holder:?}: the Outline does not show the model clicked"
        );
    }
}

/// The Animation workspace over a clip dictionary, with the triangle drawable held as `holder`
/// added as a mesh source, and no skeleton.
pub(super) fn previewing_a_triangle(
    holder: Holder,
    bone: Option<u16>,
) -> (AppState, DocumentId, DocumentId) {
    let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let mut state = AppState::from_prefs(Prefs::default());
    let mesh = state.insert_document(
        holder.file().into(),
        DocPayload::Resource(holder.asset(a_triangle(corners, bone, None))),
        None,
        None,
    );
    let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
    let clips = state.insert_document("C:/mods/clips.ycd".into(), clips, None, None);
    assert_eq!(state.active_workspace, Workspace::Animation);
    state.preview_scene.add_mesh(holder.mesh(mesh));
    (state, mesh, clips)
}

#[test]
fn a_click_reaches_a_mesh_source_drawn_beside_a_clip_dictionary() {
    for holder in Holder::ALL {
        let (mut state, mesh, clips) = previewing_a_triangle(holder, None);
        assert_eq!(state.active_document, Some(clips), "{holder:?}: the focus");
        let clip_row = dict_entry_id(CLIP);
        state.selection.select(clips, clip_row);
        let on_it = || ray(Vec3::new(0.25, 0.0, 0.25), -Vec3::Y, 3.0, egui::Vec2::ZERO);

        // A click on nothing leaves the clip the dictionary has selected: it is not what is drawn.
        select_under(
            &mut state,
            &ray(Vec3::new(50.0, 0.0, 50.0), -Vec3::Y, 3.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            state.selection.selected(clips),
            Some(clip_row),
            "{holder:?}"
        );

        select_under(&mut state, &on_it());
        assert_eq!(
            picked(&state),
            Some((mesh, "Geometry 0 of model 0")),
            "{holder:?}: a click on the mesh the preview draws did not pick it"
        );
        assert_eq!(
            state.selection.selected(clips),
            Some(clip_row),
            "{holder:?}: the pick took the clip off the dictionary"
        );

        // A click on nothing now clears the pick, and still leaves the clip.
        select_under(
            &mut state,
            &ray(Vec3::new(50.0, 0.0, 50.0), -Vec3::Y, 3.0, egui::Vec2::ZERO),
        );
        assert_eq!(picked(&state), None, "{holder:?}");
        assert_eq!(
            state.selection.selected(clips),
            Some(clip_row),
            "{holder:?}"
        );
    }
}

/// A click on the middle of the rig's second bone, seen from the side.
pub(super) fn click_on_the_bone() -> Ray {
    ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO)
}

#[test]
fn a_pick_in_the_animation_workspace_leaves_the_clip_selected_and_the_box_names_it() {
    use crate::ui::panels::viewport_layout::overlay_text;
    let clip_row = dict_entry_id(CLIP);
    for holder in Holder::ALL {
        let (mut state, mesh, clips) = previewing_a_triangle(holder, None);
        state.selection.select(clips, clip_row);
        let on_it = ray(Vec3::new(0.25, 0.0, 0.25), -Vec3::Y, 3.0, egui::Vec2::ZERO);
        select_under(&mut state, &on_it);
        assert_eq!(
            state.selection.selected(clips),
            Some(clip_row),
            "{holder:?}: a click on a mesh took the clip off the dictionary"
        );
        assert_eq!(picked(&state), Some((mesh, "Geometry 0 of model 0")));
        let text = overlay_text(&state, None);
        assert!(
            text.contains("Picked: Geometry 0 of model 0 \u{b7} held."),
            "{holder:?}: {text}"
        );
        // The preview stops drawing that mesh, and the box stops naming the pick in it.
        state.preview_scene.remove_mesh(holder.mesh(mesh));
        let text = overlay_text(&state, None);
        assert!(!text.contains("Picked:"), "{holder:?}: {text}");

        let mut p = previewing::<Gen8>(holder);
        p.state.selection.select(p.clips, clip_row);
        select_under(&mut p.state, &click_on_the_bone());
        assert_eq!(
            p.state.selection.selected(p.clips),
            Some(clip_row),
            "{holder:?}: a click on a bone took the clip off the dictionary"
        );
        assert_eq!(picked(&p.state), Some((p.rig, "BONE_1 (1)")));
        let text = overlay_text(&p.state, None);
        assert!(
            text.contains("Picked: BONE_1 (1) \u{b7} held."),
            "{holder:?}: {text}"
        );

        // Closing the document the pick is in forgets it.
        p.state.preview_scene.forget_document(p.rig);
        assert_eq!(picked(&p.state), None, "{holder:?}");
    }
}

#[test]
fn a_click_reaches_a_mesh_source_where_its_pose_draws_it() {
    for holder in Holder::ALL {
        let (mut state, mesh, _) = previewing_a_triangle(holder, Some(1));
        state.anim.skin_matrices = vec![Mat4::from_translation(MOVED); 2];
        let at = |over: Vec3| {
            ray(
                Vec3::new(0.25, 0.0, 0.25) + over,
                -Vec3::Y,
                3.0,
                egui::Vec2::ZERO,
            )
        };

        select_under(&mut state, &at(MOVED));
        assert_eq!(
            picked(&state),
            Some((mesh, "Geometry 0 of model 0")),
            "{holder:?}: a click where the pose draws the mesh missed it"
        );
        select_under(&mut state, &at(Vec3::ZERO));
        assert_eq!(
            picked(&state),
            None,
            "{holder:?}: a click where the bind pose would put the mesh found it"
        );
    }
}

#[test]
fn a_click_reaches_a_staged_drawable_previewed_beside_a_clip_dictionary() {
    const WHAT: &str = "a_click_reaches_a_staged_drawable_previewed_beside_a_clip_dictionary";
    for target in [TARGET_GEN8, TARGET_GEN9] {
        let Some((path, asset, parts)) = staged("ydr", target, two_or_more) else {
            skip_corpus(WHAT, format_args!("no {target} .ydr draws two geometries"));
            continue;
        };
        let ((model, geometry), centre, normal) = the_last_in_sight(&parts)
            .unwrap_or_else(|| panic!("{}: no face of its last geometry in sight", path.display()));
        let (mut state, mesh) = opened(&path, asset);
        let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
        state.insert_document("C:/mods/clips.ycd".into(), clips, None, None);
        state.preview_scene.add_mesh(MeshSource {
            document: mesh,
            entry: None,
        });

        select_under(&mut state, &ray(centre, normal, 0.1, egui::Vec2::ZERO));
        assert_eq!(
            picked(&state),
            Some((
                mesh,
                format!("Geometry {geometry} of model {model}").as_str()
            )),
            "{}: a click on geometry {geometry} of model {model} did not pick it",
            path.display()
        );
    }
}

#[test]
fn the_box_in_the_animation_workspace_names_what_the_preview_draws() {
    use crate::state::DocumentKind;
    use crate::ui::panels::viewport_layout::overlay_text;
    let mut p = previewing::<Gen8>(Holder::Drawable);
    let drawable = DocumentKind::Ydr.info().noun;
    let first_line = |p: &Preview| {
        overlay_text(&p.state, None)
            .lines()
            .next()
            .map(str::to_owned)
    };

    // Only the skeleton source's own drawable is drawn while no mesh is chosen.
    let text = overlay_text(&p.state, None);
    assert!(
        text.starts_with(&format!("held.ydr \u{b7} {drawable}\n")),
        "{text}"
    );
    assert!(text.contains("Bones of held.ydr"), "{text}");
    assert!(
        !text.contains("clips.ycd"),
        "the active document is named: {text}"
    );

    p.state.preview_scene.show_bones = false;
    assert!(!overlay_text(&p.state, None).contains("Bones of"));

    let more = |p: &mut Preview, name: &str| {
        let asset = or_io::Asset::Drawable(GtaDrawable::<Gen8>::default());
        let path = format!("C:/mods/{name}.ydr");
        let doc = p
            .state
            .insert_document(path.into(), DocPayload::Resource(asset), None, None);
        p.state.preview_scene.add_mesh(MeshSource {
            document: doc,
            entry: None,
        });
        // Opening a drawable switches to the workspace that shows it.
        p.state.active_workspace = Workspace::Animation;
        p.state.active_document = Some(p.clips);
    };
    more(&mut p, "alpha");
    more(&mut p, "bravo");
    assert_eq!(first_line(&p), Some(format!("alpha.ydr \u{b7} {drawable}")));
    let text = overlay_text(&p.state, None);
    assert!(
        text.contains(&format!("bravo.ydr \u{b7} {drawable}\n")),
        "{text}"
    );
    assert!(
        !text.contains("held.ydr"),
        "the skeleton's drawable is drawn beside chosen meshes: {text}"
    );
    assert!(!text.contains(" more"), "{text}");

    for name in ["charlie", "delta", "echo"] {
        more(&mut p, name);
    }
    let text = overlay_text(&p.state, None);
    assert!(
        text.contains("charlie.ydr") && !text.contains("delta.ydr"),
        "{text}"
    );
    assert!(text.contains("and 2 more"), "{text}");

    // Nothing to draw says so, rather than naming the clip dictionary.
    let mut p = previewing::<Gen8>(Holder::Drawable);
    p.state.preview_scene.pin_skeleton(None);
    p.state.preview_scene.show_bones = false;
    let text = overlay_text(&p.state, None);
    assert!(text.starts_with("Nothing to draw"), "{text}");
}
