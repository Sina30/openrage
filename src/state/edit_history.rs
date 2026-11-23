//! A document's history of edits: the steps it can undo and redo, and where its file stands among them.

use std::any::Any;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use or_io::decl::Heap;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::CrSkeletonData;

use super::memory::vec_bytes;
use super::meta_edit::MetaEdit;
use super::{DocPayload, ResourceImage};

/// Why a step could not finish, where what it changed has no sentence of its own.
pub(super) const GONE: &str = tr_key!("what it changed is no longer where it was");

/// One edit a document can undo.
#[derive(Debug)]
pub struct Step {
    /// Never reused within a history, so a position the history lost matches nothing.
    pub id: u64,
    pub label: String,
    pub change: Change,
    /// For an edit of the contents, the image the document holds on the step's other side, kept
    /// only while a grid step there was made on it.
    ///
    /// ⚠ A grid step's offsets mean something only in the image it was made on: one built again
    /// from the contents is laid out anew.
    pub(super) image: Option<Arc<ResourceImage>>,
}

/// What a step changed, holding the side of it the document does not hold now; undoing or redoing swaps the two.
#[derive(Debug)]
pub enum Change {
    /// The pose edit layer.
    Pose(or_anim::PoseEdits),
    ClipFields {
        clip_key: u32,
        other: ClipFieldSnapshot,
    },
    /// Edits of a metadata instance, in the order the next flip applies them.
    Meta(Vec<MetaEdit>),
    /// Bytes of the resource image from `at`.
    Bytes { at: usize, other: Vec<u8> },
    /// The whole resource image, which a plugin's write replaced and an undo puts back.
    Image { other: Arc<ResourceImage> },
    /// Parts of the payload, in the order the next flip swaps them; edits of one `key` straight after each other fold together.
    Part {
        key: Option<String>,
        parts: Vec<Box<dyn Swap>>,
    },
    /// Shader `from` of drawable `owner` moved to `to`, with the geometry that names it.
    MoveShader { owner: u32, from: usize, to: usize },
}

/// A part of the payload, holding the value the payload does not hold now.
pub trait Swap {
    /// Swap with the place it was taken from in `payload`; `false` where that place is gone.
    ///
    /// ⚠ A part returning `false` must have changed nothing itself: only the
    /// parts of its step swapped before it are swapped back.
    fn swap(&mut self, payload: &mut DocPayload) -> bool;

    /// Why the last swap could not be made, as the user reads it.
    fn why_not(&self) -> String {
        crate::i18n::tr(GONE).to_string()
    }

    /// The file or folder on disk a swap writes, which the jobs reading files and the edit gate are asked about first.
    fn on_disk(&self) -> Option<&Path> {
        None
    }

    /// The other document whose tree a swap changes on disk without being able to put its rows back, which is read again once the step is made.
    fn leaves_a_tree_stale(&self) -> Option<crate::state::DocumentId> {
        None
    }

    /// Whether it holds rows that read entries of an archive, which a write of that archive leaves stale.
    fn reads_an_archive(&self) -> bool {
        false
    }

    /// What it holds on the CPU besides itself.
    fn held_bytes(&self) -> usize {
        0
    }
}

/// Flip `items` newest first, leaving them in the order the next flip takes; where one cannot flip, those flipped before it flip back, and its index is the error.
pub(super) fn flip_whole<T>(
    items: &mut [T],
    mut flip: impl FnMut(&mut T) -> bool,
) -> Result<(), usize> {
    for at in (0..items.len()).rev() {
        if !flip(&mut items[at]) {
            for item in &mut items[at + 1..] {
                if !flip(item) {
                    tracing::warn!("a part of a step that could not finish did not flip back");
                }
            }
            return Err(at);
        }
    }
    items.reverse();
    Ok(())
}

impl std::fmt::Debug for dyn Swap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(tr!("a part of the payload"))
    }
}

struct Slot<T, F> {
    other: T,
    place: F,
}

impl<T: 'static, F> Swap for Slot<T, F>
where
    F: for<'a> Fn(&'a mut DocPayload) -> Option<&'a mut T>,
{
    fn swap(&mut self, payload: &mut DocPayload) -> bool {
        (self.place)(payload)
            .map(|at| std::mem::swap(at, &mut self.other))
            .is_some()
    }

    fn held_bytes(&self) -> usize {
        part_bytes(&self.other)
    }
}

/// `part` and, for the values a step is known to keep whole (a texture, a skeleton, a payload), the heap behind them.
fn part_bytes<T: 'static>(part: &T) -> usize {
    let any: &dyn Any = part;
    let heap = if let Some(texture) = any.downcast_ref::<GrcTexture>() {
        texture.heap_bytes()
    } else if let Some(skeleton) = any.downcast_ref::<CrSkeletonData>() {
        skeleton.heap_bytes()
    } else if let Some(DocPayload::Resource(asset)) = any.downcast_ref::<DocPayload>() {
        asset.heap_bytes()
    } else {
        0
    };
    std::mem::size_of::<T>() + heap
}

impl Change {
    /// The part of the payload `place` finds, which held `other` before the edit.
    pub fn part<T: 'static, F>(key: impl Into<String>, other: T, place: F) -> Self
    where
        F: for<'a> Fn(&'a mut DocPayload) -> Option<&'a mut T> + 'static,
    {
        Self::Part {
            key: Some(key.into()),
            parts: vec![Box::new(Slot { other, place })],
        }
    }

    /// This step with one more part, the one `place` finds, which held `other` before the edit.
    #[must_use]
    pub fn and_part<T: 'static, F>(mut self, other: T, place: F) -> Self
    where
        F: for<'a> Fn(&'a mut DocPayload) -> Option<&'a mut T> + 'static,
    {
        if let Self::Part { parts, .. } = &mut self {
            parts.push(Box::new(Slot { other, place }));
        }
        self
    }

    /// Whether `newer`, made straight after to the same thing, folds into this step, which keeps what came first.
    fn takes_in(&self, newer: &Change) -> bool {
        match (self, newer) {
            (Self::Bytes { at, other }, Self::Bytes { at: to, other: was }) => {
                at == to && other.len() == was.len()
            }
            (Self::ClipFields { clip_key, .. }, Self::ClipFields { clip_key: key, .. }) => {
                clip_key == key
            }
            (Self::Meta(edits), Self::Meta(newer)) => matches!(
                (edits.as_slice(), newer.as_slice()),
                ([MetaEdit::Set { path, .. }], [MetaEdit::Set { path: to, .. }]) if path == to
            ),
            (Self::Part { key: Some(key), .. }, Self::Part { key: Some(to), .. }) => key == to,
            _ => false,
        }
    }

    /// Whether this change is to the resource image rather than the payload.
    pub fn patches_the_image(&self) -> bool {
        matches!(self, Self::Bytes { .. } | Self::Image { .. })
    }

    /// Whether this change holds a value the document held (bytes, a texture, a payload), which an undo writes back over whatever the document holds then.
    ///
    /// ⚠ Only the pose layer, which is not in the file, and a move by index hold none.
    pub fn holds_old_contents(&self) -> bool {
        !matches!(self, Self::Pose(_) | Self::MoveShader { .. })
    }

    /// What the change holds on the CPU besides itself.
    fn held_bytes(&self) -> usize {
        match self {
            Self::Pose(_) | Self::MoveShader { .. } => 0,
            Self::ClipFields { other, .. } => other.name.capacity(),
            Self::Meta(edits) => vec_bytes(edits),
            Self::Bytes { other, .. } => other.capacity(),
            Self::Image { other } => other.data.capacity(),
            Self::Part { key, parts } => {
                let held: usize = parts.iter().map(|part| part.held_bytes()).sum();
                key.as_ref().map_or(0, String::capacity) + vec_bytes(parts) + held
            }
        }
    }

    /// The other documents whose trees a flip of this change leaves reading stale.
    pub fn stale_trees(&self) -> Vec<crate::state::DocumentId> {
        match self {
            Self::Part { parts, .. } => parts
                .iter()
                .filter_map(|part| part.leaves_a_tree_stale())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The files and folders on disk a flip of this change writes.
    pub fn on_disk(&self) -> Vec<&Path> {
        match self {
            Self::Part { parts, .. } => parts.iter().filter_map(|part| part.on_disk()).collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditDirection {
    Undo,
    Redo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipFieldSnapshot {
    pub name: String,
    pub start_time: f32,
    pub end_time: f32,
    pub rate: f32,
    pub looped: bool,
}

#[derive(Debug, Default)]
pub struct EditHistory {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Where the document stands with every step undone: as it opened, or as the oldest step kept began.
    base: u64,
    issued: u64,
    /// Where the file on disk stands.
    saved: u64,
    /// Whether the newest step may take in the next edit of the same thing.
    open: bool,
    batching: bool,
    /// The step a batch is folding parts into.
    batch: Option<u64>,
    /// Why the last undo or redo could not finish, until the history moves again.
    refused: Option<String>,
}

impl EditHistory {
    pub const CAP: usize = 200;
    /// How many images its steps may keep before the oldest steps are forgotten: each is a whole file.
    pub const IMAGES: usize = 4;

    /// Where the document stands: the newest step applied, or the base.
    pub fn at(&self) -> u64 {
        self.undo.last().map_or(self.base, |step| step.id)
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn saved(&self) -> u64 {
        self.saved
    }

    pub fn at_saved(&self) -> bool {
        self.saved == self.at()
    }

    /// The file now holds the document as it stood at `at`.
    pub fn saved_at(&mut self, at: u64) {
        self.saved = at;
        self.open = false;
    }

    /// What the steps keep: their labels, the bytes and parts they swap back, and each image they hold besides `current`.
    pub fn held_bytes(&self, current: Option<&Arc<ResourceImage>>) -> usize {
        let mut images: HashSet<*const ResourceImage> =
            current.map(Arc::as_ptr).into_iter().collect();
        let mut held = |step: &Step| {
            let image = (step.image.as_ref()).filter(|image| images.insert(Arc::as_ptr(image)));
            step.label.capacity()
                + image.map_or(0, |image| image.data.len())
                + step.change.held_bytes()
        };
        let steps: usize = self.undo.iter().chain(&self.redo).map(&mut held).sum();
        vec_bytes(&self.undo) + vec_bytes(&self.redo) + steps
    }

    /// The steps applied, oldest first.
    pub fn done(&self) -> &[Step] {
        &self.undo
    }

    /// The steps undone, the next to redo first.
    pub fn undone(&self) -> impl Iterator<Item = &Step> {
        self.redo.iter().rev()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|step| step.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|step| step.label.as_str())
    }

    /// The labels of the steps made since the file was written, oldest first; none when the file holds a state the steps do not lead back to.
    pub fn since_saved(&self) -> Vec<&str> {
        let after = if self.saved == self.base {
            0
        } else {
            match self.undo.iter().position(|step| step.id == self.saved) {
                Some(at) => at + 1,
                None => return Vec::new(),
            }
        };
        self.undo[after..]
            .iter()
            .map(|step| step.label.as_str())
            .collect()
    }

    /// Why the last undo or redo could not finish, until the history moves again.
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// The step the next undo or redo takes.
    pub fn next(&self, direction: EditDirection) -> Option<&Step> {
        match direction {
            EditDirection::Undo => self.undo.last(),
            EditDirection::Redo => self.redo.last(),
        }
    }

    /// Count `change` as the newest step; `image` is the one an edit of the contents replaced.
    pub fn push(&mut self, label: String, change: Change, image: Option<Arc<ResourceImage>>) {
        let change = match (self.batch.and_then(|id| self.top_with(id)), change) {
            (Some(Change::Part { parts, .. }), Change::Part { parts: more, .. }) => {
                parts.extend(more);
                return;
            }
            (_, change) => change,
        };
        self.refused = None;
        // ⚠ Never into a step the file holds, which would then not be what was saved.
        if self.open
            && !self.at_saved()
            && self
                .undo
                .last()
                .is_some_and(|top| top.change.takes_in(&change))
        {
            return;
        }
        self.redo.clear();
        let id = self.fresh();
        let image = image.filter(|_| made_on_the_image(&self.undo));
        self.undo.push(Step {
            id,
            label,
            change,
            image,
        });
        while self.undo.len() > Self::CAP || self.images() > Self::IMAGES {
            self.base = self.undo.remove(0).id;
            // The oldest step kept has no grid step before it to keep an image for.
            if let Some(oldest) = self.undo.first_mut() {
                oldest.image = None;
            }
        }
        self.open = true;
        if self.batching {
            self.batch = Some(id);
        }
    }

    fn images(&self) -> usize {
        self.undo.iter().filter(|step| step.image.is_some()).count()
    }

    fn top_with(&mut self, id: u64) -> Option<&mut Change> {
        let top = self.undo.last_mut().filter(|step| step.id == id)?;
        Some(&mut top.change)
    }

    /// Make every part pushed until [`Self::end_batch`] one step.
    pub fn begin_batch(&mut self) {
        (self.batching, self.batch) = (true, None);
    }

    /// End the batch, naming the step it made `label`.
    pub fn end_batch(&mut self, label: String) {
        if let Some(top) =
            (self.batch.take()).and_then(|id| self.undo.last_mut().filter(|s| s.id == id))
        {
            top.label = label;
        }
        self.batching = false;
    }

    /// Whether a step holds rows that read entries of an archive.
    pub fn reads_an_archive(&self) -> bool {
        (self.undo.iter().chain(&self.redo)).any(|step| match &step.change {
            Change::Part { parts, .. } => parts.iter().any(|part| part.reads_an_archive()),
            _ => false,
        })
    }

    /// End the edit the newest step is taking in, so the next one is a step of its own.
    pub fn seal(&mut self) {
        self.open = false;
    }

    /// Let go of the steps that hold a value of the old contents and of the image every other step holds: the document was given other contents.
    pub(super) fn forget_old_contents(&mut self) {
        for steps in [&mut self.undo, &mut self.redo] {
            steps.retain(|step| !step.change.holds_old_contents());
            steps.iter_mut().for_each(|step| step.image = None);
        }
    }

    /// Forget every step: the document now holds an edit none of them can be undone past.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.base = self.fresh();
        self.open = false;
        self.refused = None;
    }

    pub(super) fn take(&mut self, direction: EditDirection) -> Option<Step> {
        self.open = false;
        self.refused = None;
        match direction {
            EditDirection::Undo => self.undo.pop(),
            EditDirection::Redo => self.redo.pop(),
        }
    }

    /// Keep `step`, just undone or redone, for the opposite move.
    pub(super) fn put_back(&mut self, mut step: Step, direction: EditDirection) {
        let onto = match direction {
            EditDirection::Undo => &mut self.redo,
            EditDirection::Redo => &mut self.undo,
        };
        if !made_on_the_image(onto) {
            step.image = None;
        }
        onto.push(step);
    }

    /// Forget the step `id`, taken to undo or redo, which could not finish and changed nothing: the document keeps its change, and the saved mark stays true. `refused` says why.
    ///
    /// ⚠ Each step after it was made on its change, so a failed redo forgets them
    /// too; a failed undo leaves every position under it holding the change as
    /// well, so a saved mark there is gone, as no position holds what the file
    /// does. Unless `alone`, the steps before it go as well.
    pub(super) fn dropped(
        &mut self,
        id: u64,
        direction: EditDirection,
        alone: bool,
        refused: String,
    ) {
        let stood = match direction {
            EditDirection::Undo => id,
            EditDirection::Redo => self.at(),
        };
        let above = self.saved == id || self.redo.iter().any(|step| step.id == self.saved);
        let kept = match (direction, alone) {
            (EditDirection::Undo, true) => above,
            (EditDirection::Redo, true) => !above,
            (_, false) => false,
        };
        if direction == EditDirection::Redo || !alone {
            self.redo.clear();
        }
        if !alone {
            self.undo.clear();
            self.base = self.fresh();
        }
        if self.saved == stood {
            self.saved = self.at();
        } else if !kept {
            self.saved = self.fresh();
        }
        self.refused = Some(refused);
    }

    /// Which way leads to `target`, or `None` when the document stands there or the history no longer holds it.
    pub fn way_to(&self, target: u64) -> Option<EditDirection> {
        if target == self.at() {
            None
        } else if target == self.base || self.undo.iter().any(|step| step.id == target) {
            Some(EditDirection::Undo)
        } else if self.redo.iter().any(|step| step.id == target) {
            Some(EditDirection::Redo)
        } else {
            None
        }
    }

    fn fresh(&mut self) -> u64 {
        self.issued += 1;
        self.issued
    }
}

/// Whether the newest of `steps` is a grid step, made on the image a step put above it takes.
///
/// Every other step replaces the image, so a grid step can only lie straight beneath the one that
/// took the image it was made on.
fn made_on_the_image(steps: &[Step]) -> bool {
    steps
        .last()
        .is_some_and(|step| step.change.patches_the_image())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A history of `n` steps, each of its own, and their ids.
    fn steps(n: usize) -> (EditHistory, Vec<u64>) {
        let mut history = EditHistory::default();
        let ids = (0..n)
            .map(|at| {
                let change = Change::Bytes { at, other: vec![0] };
                history.push(format!("edit {at}"), change, None);
                history.seal();
                history.at()
            })
            .collect();
        (history, ids)
    }

    /// Take the next step `direction`, as an undo or redo does, and move it as one that finished.
    fn stepped(history: &mut EditHistory, direction: EditDirection) {
        let step = history.take(direction).expect("a step to take");
        history.put_back(step, direction);
    }

    /// Take the next step `direction`, as an undo or redo does, and drop it as one that could not finish.
    fn failed(history: &mut EditHistory, direction: EditDirection, alone: bool) {
        let step = history.take(direction).expect("a step to take");
        history.dropped(step.id, direction, alone, "why".to_string());
    }

    #[test]
    fn a_step_that_swaps_a_skeleton_counts_the_bones_it_holds() {
        use or_io::gta5::{CrBoneData, CrSkeletonData, Mat34V};
        let bones = 300;
        let skeleton = CrSkeletonData {
            bones: (0..bones)
                .map(|at| CrBoneData {
                    name: format!("BONE_{at}"),
                    ..CrBoneData::default()
                })
                .collect(),
            inverse_transforms: vec![Mat34V::default(); bones],
            default_transforms: vec![Mat34V::default(); bones],
            ..CrSkeletonData::default()
        };
        let own = std::mem::size_of::<CrSkeletonData>();
        let carried =
            bones * (std::mem::size_of::<CrBoneData>() + 2 * std::mem::size_of::<Mat34V>());
        let change = Change::part(
            "bone 0 1 position",
            skeleton,
            |_| None::<&mut CrSkeletonData>,
        );
        assert!(
            change.held_bytes() >= own + carried,
            "a skeleton of {bones} bones is counted as {} bytes, under the {} it carries",
            change.held_bytes(),
            own + carried
        );
    }
    #[test]
    fn a_step_that_cannot_finish_leaves_the_saved_mark_where_the_file_stands() {
        use EditDirection::{Redo, Undo};

        let (mut on_it, ids) = steps(2);
        on_it.saved_at(ids[1]);
        failed(&mut on_it, Undo, true);
        assert!(
            on_it.at_saved() && on_it.at() == ids[0],
            "saved on it, the document still holds its change, as the file does"
        );
        stepped(&mut on_it, Undo);
        assert!(!on_it.at_saved());

        let (mut under_it, ids) = steps(2);
        under_it.saved_at(ids[0]);
        failed(&mut under_it, Undo, true);
        assert!(
            !under_it.at_saved() && under_it.way_to(under_it.saved()).is_none(),
            "saved under it, no position holds the file once its change cannot go"
        );

        let (mut above_it, ids) = steps(3);
        above_it.saved_at(ids[2]);
        stepped(&mut above_it, Undo);
        failed(&mut above_it, Undo, true);
        stepped(&mut above_it, Redo);
        assert!(
            above_it.at_saved(),
            "saved above it, the step after it redoes onto its change"
        );

        let (mut redone, ids) = steps(3);
        redone.saved_at(ids[2]);
        stepped(&mut redone, Undo);
        stepped(&mut redone, Undo);
        failed(&mut redone, Redo, true);
        assert!(
            !redone.can_redo() && !redone.at_saved() && redone.at() == ids[0],
            "a redo that cannot finish takes the steps made on its change with it"
        );

        let (mut emptied, ids) = steps(2);
        emptied.saved_at(ids[1]);
        failed(&mut emptied, Undo, false);
        assert!(
            !emptied.can_undo() && emptied.at_saved() && emptied.refused() == Some("why"),
            "where the history empties, the file still holds the document as it stands"
        );
    }
}
