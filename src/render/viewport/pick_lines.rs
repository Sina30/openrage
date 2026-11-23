//! The lines a click can land on: the bones the viewport draws in front of everything, with the
//! bind-pose ghost of the Animation preview, and the bounds it draws as lines.

use or_io::gta5::{DrawableTarget, FragType};
use or_renderer::{LineVertex, ShadingMode};

use super::bones;
use super::pick::{nearest_polygon, Hit, Named, Ray};
use crate::render::bound_meshes::bound_node_meshes;
use crate::render::lines::{bound_node_lines, build_bound_lines, embedded_bound, physics_bound};
use crate::render::skeleton_lines::{bone_lines, ghost_lines};
use crate::state::{AppState, DocPayload, DocumentId, SkeletonSource};
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::{
    dict_entry_part_id, nav_polygon_id, physics_bound_hash, YPT_MODEL_KIND_BOUND,
};

/// How near the cursor a line has to pass to be clicked, in pixels.
const REACH: f32 = 5.0;

/// The bone of the skeletons the viewport draws bones of nearest the cursor, posed or in the
/// Animation preview's bind-pose ghost, in the document the skeleton comes from.
///
/// ⚠ Where the posed bone and its ghost are equally near, the posed bone is the one picked.
pub(super) fn bone_under(state: &AppState, ray: &Ray) -> Option<Hit> {
    let ghost =
        state.active_workspace == Workspace::Animation && state.preview_scene.show_bind_pose_ghost;
    let shown = bones::shown(state);
    let (t, _, owner, bone, in_the_bind_pose) = shown
        .iter()
        .flat_map(|owner| {
            let posed = bone_lines(&owner.skeleton, owner.world());
            let ghost = match ghost {
                true => ghost_lines(&owner.skeleton),
                false => Vec::new(),
            };
            (posed.into_iter())
                .map(|(bone, lines)| (bone, lines, false))
                .chain(ghost.into_iter().map(|(bone, lines)| (bone, lines, true)))
                .filter_map(move |(bone, lines, ghost)| {
                    let (t, away) = ray.near_lines(&lines, REACH, |_| true)?;
                    Some((t, away, owner, bone, ghost))
                })
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    // As the Outline names a bone.
    let name = (owner.skeleton.bones.iter())
        .find(|b| b.index == bone)
        .map_or("", |b| &b.name);
    let ghost = if in_the_bind_pose {
        " in the bind pose"
    } else {
        ""
    };
    Some(Hit {
        t,
        doc: owner.doc,
        row: owner.named.bone(bone),
        label: format!("{name} ({bone}){ghost}"),
        model: None,
    })
}

/// The bound of the active document the viewport draws as lines nearest the cursor, as the row its
/// node has in the Outline, where no mesh nearer than `mesh` hides it.
pub(super) fn bound_under(
    state: &AppState,
    named: Option<Named>,
    ray: &Ray,
    mesh: Option<f32>,
) -> Option<Hit> {
    let doc = state.active_document?;
    // Lines are depth-tested against the meshes, and Wireframe draws none.
    let wireframe = state.prefs.viewport.shading == ShadingMode::Wireframe;
    let shown = |t: f32| wireframe || mesh.is_none_or(|mesh| t <= mesh);
    let (t, _, row) = (drawn_bounds(state, named, wireframe).into_iter())
        .filter_map(|(row, lines)| {
            let (t, away) = ray.near_lines(&lines, REACH, shown)?;
            Some((t, away, row))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    Some(Hit {
        t,
        doc,
        row,
        label: "Bound".to_string(),
        model: None,
    })
}

/// The polygon of the active `.ynv` the cursor is on, as the row it has in the Outline.
pub(super) fn navmesh_under(state: &AppState, ray: &Ray) -> Option<Hit> {
    let doc = state.active_document?;
    let DocPayload::Resource(or_io::Asset::NavMesh(mesh)) = &state.document(doc)?.payload else {
        return None;
    };
    let mut best: Option<(f32, usize)> = None;
    for (index, corners) in crate::render::navmesh::polygons(mesh).iter().enumerate() {
        for k in 1..corners.len().saturating_sub(1) {
            let triangle = [corners[0], corners[k], corners[k + 1]];
            if let Some(t) = ray
                .meets(triangle)
                .filter(|t| best.is_none_or(|(b, _)| *t < b))
            {
                best = Some((t, index));
            }
        }
    }
    let (t, index) = best?;
    Some(Hit {
        t,
        doc,
        row: nav_polygon_id(index),
        label: format!("Polygon {index}"),
        model: None,
    })
}

/// The node of the active `.ybn` whose mesh the cursor is on, as the row it has in the Outline: the
/// shaded modes draw a `.ybn` as meshes, which carry no line for `bound_under` to find.
pub(super) fn bound_mesh_under(state: &AppState, ray: &Ray) -> Option<Hit> {
    if state.prefs.viewport.shading == ShadingMode::Wireframe {
        return None;
    }
    let doc = state.active_document?;
    let DocPayload::Resource(or_io::Asset::Bounds(bound)) = &state.document(doc)?.payload else {
        return None;
    };
    let palette = state.materials_palette.palette_ref();
    let mut best: Option<(f32, u64)> = None;
    for (row, meshes) in bound_node_meshes(bound, palette) {
        for upload in &meshes {
            let within = best.map_or(f32::INFINITY, |(t, _)| t);
            if let Some((t, _)) = nearest_polygon(upload, ray, within) {
                best = Some((t, row));
            }
        }
    }
    let (t, row) = best?;
    Some(Hit {
        t,
        doc,
        row,
        label: "Bound".to_string(),
        model: None,
    })
}

/// Each bound the viewport draws as lines for the active document, with the row its node has in the
/// Outline.
///
/// ⚠ Two things draw bounds as lines: Collision draws the one a drawable embeds, and Wireframe
/// draws a document's own lines, which hold a fragment's physics bound and a `.ybn`'s bounds. A
/// `.ybn` is drawn as meshes in the other modes, where `bound_mesh_under` picks its nodes by mesh.
fn drawn_bounds(
    state: &AppState,
    named: Option<Named>,
    wireframe: bool,
) -> Vec<(u64, Vec<LineVertex>)> {
    let mut out = Vec::new();
    if let (true, Some(named), Some(bound)) = (
        state.preview_scene.show_collision,
        named,
        embedded_bound(state),
    ) {
        out.extend(
            bound_node_lines(bound)
                .into_iter()
                .map(|(node, lines)| (named.bound(node), lines)),
        );
    }
    if wireframe {
        match state.active_document().map(|doc| &doc.payload) {
            // The Outline numbers a `.ybn`'s nodes as `bound_node_lines` does, with no section.
            Some(DocPayload::Resource(or_io::Asset::Bounds(bound))) => {
                out.extend(bound_node_lines(bound))
            }
            Some(DocPayload::Resource(or_io::Asset::Fragment(fragment))) => {
                out.extend(physics_bound_lines(fragment))
            }
            Some(DocPayload::Resource(or_io::Asset::FragmentGen9(fragment))) => {
                out.extend(physics_bound_lines(fragment))
            }
            _ => {}
        }
    }
    out
}

/// The row the Outline gives a fragment's physics bound, with the lines Wireframe draws for it.
fn physics_bound_lines<T: DrawableTarget>(
    fragment: &FragType<T>,
) -> Option<(u64, Vec<LineVertex>)> {
    let row = dict_entry_part_id(physics_bound_hash(0), YPT_MODEL_KIND_BOUND, 0);
    Some((row, build_bound_lines(physics_bound(fragment)?)))
}

/// The document a skeleton source is in, and how the Outline names its bones.
pub(super) fn skeleton_owner(
    state: &AppState,
    source: SkeletonSource,
) -> Option<(DocumentId, Named)> {
    use or_io::Asset as A;
    Some(match source {
        SkeletonSource::Drawable(doc) => (doc, Named::Drawable),
        SkeletonSource::Fragment(doc) => (
            doc,
            Named::Entry(crate::state::textures::FRAGMENT_ENTRY_HASH),
        ),
        SkeletonSource::DictionaryEntry(doc, index) => {
            let DocPayload::Resource(asset) = &state.document(doc)?.payload else {
                return None;
            };
            let hash = match asset {
                A::DrawableDict(dict) => dict.codes.get(index),
                A::DrawableDictGen9(dict) => dict.codes.get(index),
                _ => None,
            }?;
            (doc, Named::Entry(*hash))
        }
    })
}
