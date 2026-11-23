//! Posing the skinned meshes the viewport holds to the pose the preview holds.

use crate::render::meshes::MeshUpload;
use crate::state::AppState;

/// Poses the skinned `uploads` to the pose `state` holds unless `held` says they hold it already,
/// and says which of them moved.
///
/// ⚠ Keyed by the pose and not the transport's time: another clip or an edit poses a paused
/// preview at the same time.
pub(crate) fn pose_uploads(
    uploads: &mut [MeshUpload],
    held: &mut Option<u64>,
    state: &AppState,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
) -> Vec<usize> {
    let (revision, skin) = skin_of(state);
    if *held == Some(revision) {
        return Vec::new();
    }
    *held = Some(revision);
    (0..uploads.len())
        .filter(|&index| uploads[index].apply_pose(skin, positions, normals))
        .collect()
}

/// The skin matrices the held meshes are posed by and how often they changed.
pub(crate) fn skin_of(state: &AppState) -> (u64, &[glam::Mat4]) {
    // A GTA VI drawable outside the Animation workspace, and the one a GTA VI animation picked in
    // it, take that animation's pose.
    let kind = state.active_document().map(|d| d.kind);
    let gta6 = match state.active_workspace {
        crate::ui::dock::Workspace::Animation => kind == Some(crate::state::DocumentKind::Gta6Anim),
        _ => kind == Some(crate::state::DocumentKind::Gta6Drawable),
    };
    if gta6 {
        return crate::ui::panels::inspector::gta6_drawn(state)
            .map_or((0, &[][..]), |p| (p.revision, p.skin));
    }
    (state.anim.skin_revision(), &state.anim.skin_matrices)
}
