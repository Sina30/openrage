//! The bones the viewport draws, which a click on a line of picks: outside the Animation workspace
//! the skeleton of the drawable it draws, at rest, while its Bones toggle is on; in it the skeleton
//! source's, posed by the clip that binds to it and at rest where none does.

use std::ops::Deref;
use std::sync::Arc;

use glam::Mat4;
use or_anim::Skeleton;
use or_io::gta5::CrSkeletonData;
use or_renderer::LineVertex;

use super::pick::{skeletons, Named};
use super::pick_lines::skeleton_owner;
use crate::render::skeleton_lines::build_pose_comparison;
use crate::state::preview_scene::skeleton_data_from;
use crate::state::{AppState, DocumentId};
use crate::ui::dock::Workspace;

/// A skeleton the viewport draws bones of: the session's own while a clip poses it, else one built from the document's data.
pub(super) enum Rest<'a> {
    Posed(&'a Skeleton),
    Built(Arc<Skeleton>),
}

impl Deref for Rest<'_> {
    type Target = Skeleton;

    fn deref(&self) -> &Skeleton {
        match self {
            Rest::Posed(skeleton) => skeleton,
            Rest::Built(skeleton) => skeleton,
        }
    }
}

/// A skeleton the viewport draws bones of, in the document it is in.
pub(super) struct Shown<'a> {
    pub doc: DocumentId,
    /// How the Outline names its bones.
    pub named: Named,
    pub skeleton: Rest<'a>,
    /// Where the clip poses each bone, where one drives the skeleton.
    posed: Option<&'a [Mat4]>,
}

impl Shown<'_> {
    /// Where each bone is drawn: where the clip poses it, else at rest.
    pub fn world(&self) -> &[Mat4] {
        self.posed.unwrap_or(&self.skeleton.bind_world)
    }

    /// The bone whose Outline row is the one selected in its document, else the one a click picked in the Animation preview, which keeps its pick apart from the selection.
    fn selected(&self, state: &AppState) -> Option<u16> {
        let bone_of = |row: u64| {
            (self.skeleton.bones.iter())
                .map(|bone| bone.index)
                .find(|index| self.named.bone(*index) == row)
        };
        let picked = (state.preview_scene.picked.as_ref())
            .filter(|picked| picked.document == self.doc)
            .map(|picked| picked.row);
        (state.selection.selected(self.doc).and_then(bone_of)).or_else(|| picked.and_then(bone_of))
    }
}

/// The document a skeleton is in, how the Outline names its bones, and its bones.
type Source<'a> = (DocumentId, Named, &'a CrSkeletonData);

/// The skeletons with bones the viewport could draw, ticked or not: the Animation preview's skeleton
/// source, else the skeleton of each drawable drawn for the active document.
fn sources(state: &AppState) -> Vec<Source<'_>> {
    // The dressed ped has no document to take a skeleton from.
    if state.active_workspace == Workspace::Wardrobe {
        return Vec::new();
    }
    let with_bones = |(_, _, data): &Source<'_>| !data.bones.is_empty();
    if state.active_workspace == Workspace::Animation {
        let chosen = state.preview_scene.skeleton_source.and_then(|source| {
            let (doc, named) = skeleton_owner(state, source)?;
            let data = skeleton_data_from(&state.document(doc)?.payload, source)?;
            Some((doc, named, data))
        });
        return chosen.filter(with_bones).into_iter().collect();
    }
    let Some(doc) = state.active_document else {
        return Vec::new();
    };
    (skeletons(state, doc).into_iter())
        .map(|(data, named)| (doc, named, data))
        .filter(with_bones)
        .collect()
}

/// Whether the viewport has a skeleton to draw bones of, ticked or not.
pub(crate) fn can_draw(state: &AppState) -> bool {
    !sources(state).is_empty()
        || gta6_rest(state).is_some()
        || crate::ui::panels::inspector::gta6_drawn(state).is_some()
}

/// The skeleton of the active scanned GTA VI drawable or fragment, or of the dictionary entry
/// selected, outside the Animation workspace, built once for as long as the document holds it.
fn gta6_rest(state: &AppState) -> Option<(DocumentId, Named, Arc<Skeleton>)> {
    use crate::state::gta6_entry_skeletons::{entry_of, shown};
    if matches!(
        state.active_workspace,
        Workspace::Animation | Workspace::Wardrobe
    ) {
        return None;
    }
    let doc = state.active_document()?;
    if !matches!(doc.payload, crate::state::DocPayload::Gta6Drawable(_)) {
        return None;
    }
    // A dictionary's entry names its bones under its code, as the Outline lists them.
    let selection = state.selection.selected(doc.id);
    let entry = entry_of(doc, selection);
    let named = entry.map_or(Named::Drawable, Named::Entry);
    let data = entry.map_or(usize::MAX, |code| code as usize);
    let skeleton = kept((doc.id, doc.payload_epoch(), data), || {
        let read = shown(doc, selection)?;
        (!read.bones.is_empty()).then(|| or_anim::gta6::skeleton(&read))
    })?;
    Some((doc.id, named, skeleton))
}

/// Whether the box that draws the bones is ticked: Show bones of the preview in the Animation
/// workspace, the viewport's own Bones outside it.
fn ticked(state: &AppState) -> bool {
    match state.active_workspace {
        Workspace::Animation => state.preview_scene.show_bones,
        _ => state.preview_scene.show_rest_bones,
    }
}

/// The document whose bones the viewport draws, if it draws any.
pub(crate) fn drawn_from(state: &AppState) -> Option<DocumentId> {
    let first = sources(state).into_iter().next().map(|(doc, ..)| doc);
    let first = first.or_else(|| gta6_rest(state).map(|(doc, ..)| doc));
    first.filter(|_| ticked(state))
}

/// The skeletons the viewport draws bones of.
///
/// ⚠ A clip that does not bind leaves the session with its skeleton and with no pose, and a skeleton
/// no clip was chosen for has no session at all: both draw at rest, from the skeleton source's bones.
pub(super) fn shown(state: &AppState) -> Vec<Shown<'_>> {
    if !ticked(state) {
        return Vec::new();
    }
    let animation = state.active_workspace == Workspace::Animation;
    let gta6 = gta6_rest(state).map(|(doc, named, skeleton)| Shown {
        doc,
        named,
        skeleton: Rest::Built(skeleton),
        posed: None,
    });
    (sources(state).into_iter())
        .map(|(doc, named, data)| {
            let posed = (state.anim.skeleton()).filter(|skeleton| {
                animation
                    && skeleton.bone_count() == data.bones.len()
                    && state.anim.world_matrices.len() == skeleton.bone_count()
            });
            match posed {
                Some(skeleton) => Shown {
                    doc,
                    named,
                    skeleton: Rest::Posed(skeleton),
                    posed: Some(&state.anim.world_matrices),
                },
                None => Shown {
                    doc,
                    named,
                    skeleton: Rest::Built(built(
                        doc,
                        state.document(doc).map_or(0, |d| d.payload_epoch()),
                        data,
                    )),
                    posed: None,
                },
            }
        })
        .chain(gta6)
        .collect()
}

/// The lines of the bones the viewport draws, the one selected in the Outline highlighted.
pub(crate) fn lines(state: &AppState) -> Vec<LineVertex> {
    let ghost =
        state.active_workspace == Workspace::Animation && state.preview_scene.show_bind_pose_ghost;
    // A GTA VI drawable a GTA VI animation poses draws the bones of the skeleton it is posed on.
    if let Some(posed) = crate::ui::panels::inspector::gta6_drawn(state).filter(|_| ticked(state)) {
        return build_pose_comparison(posed.rig, posed.world, ghost, None);
    }
    (shown(state).iter())
        .flat_map(|s| build_pose_comparison(&s.skeleton, s.world(), ghost, s.selected(state)))
        .collect()
}

#[cfg(test)]
thread_local! {
    static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many skeletons this thread has built for the viewport's bones.
#[cfg(test)]
pub(crate) fn builds() -> usize {
    BUILDS.with(std::cell::Cell::get)
}

/// How many skeletons stay built: the ones a frame asks for, which are a drawable's or the preview's one.
const KEPT: usize = 4;

/// The document, its payload's epoch and where the data lies, which name a skeleton built.
type Key = (DocumentId, u64, usize);

thread_local! {
    /// The skeletons built lately, by the document, its payload's epoch and the data they were built from.
    static BUILT: std::cell::RefCell<Vec<(Key, Arc<Skeleton>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The skeleton of `data`, in the document `doc` at `epoch` of its payload, built once for as long as the document holds it: a ped's 354 bones take 0.8 ms to build, and the viewport asks for the skeleton on every frame its bones are ticked.
fn built(doc: DocumentId, epoch: u64, data: &CrSkeletonData) -> Arc<Skeleton> {
    let key: Key = (doc, epoch, data as *const CrSkeletonData as usize);
    kept(key, || Some(Skeleton::build(data))).unwrap_or_else(|| Arc::new(Skeleton::build(data)))
}

/// The skeleton kept under `key`, else the one `build` makes, kept; `None` where it makes none.
fn kept(key: Key, build: impl FnOnce() -> Option<Skeleton>) -> Option<Arc<Skeleton>> {
    BUILT.with(|built| {
        let mut built = built.borrow_mut();
        if let Some((_, skeleton)) = built.iter().find(|(held, _)| *held == key) {
            return Some(Arc::clone(skeleton));
        }
        #[cfg(test)]
        BUILDS.with(|builds| builds.set(builds.get() + 1));
        let skeleton = Arc::new(build()?);
        if built.len() == KEPT {
            built.remove(0);
        }
        built.push((key, Arc::clone(&skeleton)));
        Some(skeleton)
    })
}
