//! A scanned GTA VI drawable's, fragment's or dictionary entry's bones edited as GTA V's are: a
//! bone's position, rotation or scale, each a step of undo, with the inverse binds of it and of the
//! bones under it derived again. A save writes them over the body read again, in place.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::{EulerRot, Quat};
use or_io::container::rpf::Rpf9Role;
use or_io::gta6::headerless::{
    bone_name_holds, rebind_under, rename_bones, write_skeleton, Resource, Skeleton, SkeletonOwner,
};

use super::bone_numbers::BoneEdit;
use super::document::Document;
use super::gta6_edit::EditSections;
use super::gta6_resource_save::EditedSave;
use super::{AppState, Change, DocPayload, DocumentId, Swap};

/// Which skeleton of a scanned body a step edits: its own, or a dictionary entry's by its code.
#[derive(Clone, Copy)]
enum Whose {
    Own,
    Entry(u32),
}

/// `skeleton` swapped with the one held for the scanned body at `path`; `false` where none is.
fn swap_held(path: &Path, whose: Whose, skeleton: &mut Skeleton) -> bool {
    if let Whose::Entry(code) = whose {
        return super::gta6_entry_skeletons::swap(path, code, skeleton);
    }
    match super::rpf9_scan::skeletons().get_mut(path) {
        Some(Ok(held)) => {
            std::mem::swap(held, skeleton);
            true
        }
        _ => false,
    }
}

/// The skeleton a step holds of the scanned body at `path`, the side of it the body does not hold.
// ⚠ The skeleton is not in the payload: the scan's map of skeletons by path is what the viewport,
// the Outline and the export read.
struct Bones {
    path: PathBuf,
    whose: Whose,
    other: Skeleton,
}

impl Swap for Bones {
    fn swap(&mut self, _: &mut DocPayload) -> bool {
        swap_held(&self.path, self.whose, &mut self.other)
    }
}

/// `edit` made to bone `bone` of `skeleton`, its inverse bind and those under it derived again.
fn apply(skeleton: &mut Skeleton, bone: usize, edit: &BoneEdit) -> Result<(), String> {
    let held = (skeleton.bones.get_mut(bone))
        .ok_or_else(|| tr!("The skeleton holds no such bone.").to_string())?;
    let numbers = match edit {
        // The hash stays: a clip's tracks and a geometry's palette find the bone by it.
        BoneEdit::Name(name) => {
            bone_name_holds(name).map_err(|_| {
                let why = match name.is_empty() {
                    true => tr!("a bone's name holds at least one character"),
                    false => tr!("a bone's name ends at its first NUL, so it holds none"),
                };
                tr!("The bone is not renamed: {why}", why = why)
            })?;
            held.name.clone_from(name);
            return Ok(());
        }
        BoneEdit::Position(v) | BoneEdit::Rotation(v) | BoneEdit::Scale(v) => *v,
    };
    if !numbers.iter().all(|n| n.is_finite()) {
        return Err(tr!("A bone's numbers must be finite.").to_string());
    }
    match edit {
        BoneEdit::Position(v) => held.translation = *v,
        BoneEdit::Scale(v) => held.scale = *v,
        BoneEdit::Rotation(degrees) => {
            let [x, y, z] = degrees.map(f32::to_radians);
            let q = Quat::from_euler(EulerRot::ZYX, z, y, x).normalize();
            held.rotation = q.to_array();
        }
        BoneEdit::Name(_) => {}
    }
    rebind_under(skeleton, bone).map_err(|_| {
        tr!("That scale or rotation leaves the bone with no inverse bind: a scale of 0 is refused.")
            .to_string()
    })
}

impl AppState {
    /// Bone `bone` of the scanned GTA VI drawable or fragment `doc` given `edit`, as a step of undo.
    pub(crate) fn edit_gta6_bone(
        &mut self,
        doc: DocumentId,
        bone: usize,
        edit: BoneEdit,
    ) -> Result<(), String> {
        self.edit_held_bone(doc, Whose::Own, bone, edit)
    }

    /// `edit_gta6_bone` for the skeleton entry `code` of the scanned dictionary `doc` holds.
    pub(crate) fn edit_gta6_entry_bone(
        &mut self,
        doc: DocumentId,
        code: u32,
        bone: usize,
        edit: BoneEdit,
    ) -> Result<(), String> {
        self.edit_held_bone(doc, Whose::Entry(code), bone, edit)
    }

    fn edit_held_bone(
        &mut self,
        doc: DocumentId,
        whose: Whose,
        bone: usize,
        edit: BoneEdit,
    ) -> Result<(), String> {
        let document = self
            .document(doc)
            .ok_or_else(|| tr!("its document is closed"))?;
        let DocPayload::Gta6Drawable(resource) = &document.payload else {
            return Err(
                tr!("Only a GTA VI drawable's or fragment's bones are edited here.").to_string(),
            );
        };
        let holds = match whose {
            Whose::Own => matches!(
                **resource,
                Resource::Drawable(_) | Resource::DrawableWithBound(_)
            ),
            Whose::Entry(_) => matches!(**resource, Resource::Dictionary(_)),
        };
        if !holds {
            return Err(
                tr!("Only a GTA VI drawable's or fragment's bones are edited here.").to_string(),
            );
        }
        let path = (document.path.clone())
            .ok_or_else(|| tr!("This document has no file, so it holds no GTA VI skeleton."))?;
        let mut after = match whose {
            Whose::Own => super::rpf9_scan::scanned_skeleton(&path)?,
            Whose::Entry(code) => super::gta6_entry_skeletons::of(&path, code).ok_or_else(|| {
                tr!("This dictionary entry held no skeleton that read when it opened, so it has no bones to edit.")
                    .to_string()
            })?,
        };
        apply(&mut after, bone, &edit)?;
        let name = after.bones[bone].name.clone();
        let (label, field) = match &edit {
            BoneEdit::Position(_) => (tr!("Move bone {name}", name = name), "position"),
            BoneEdit::Rotation(_) => (tr!("Turn bone {name}", name = name), "rotation"),
            BoneEdit::Scale(_) => (tr!("Scale bone {name}", name = name), "scale"),
            BoneEdit::Name(_) => (tr!("Rename bone {name}", name = name), "name"),
        };
        if !swap_held(&path, whose, &mut after) {
            return Err(tr!("The skeleton this body opened with is no longer held.").to_string());
        }
        let key = match whose {
            Whose::Own => format!("gta6 bone {bone} {field}"),
            Whose::Entry(code) => format!("gta6 bone {code:08X} {bone} {field}"),
        };
        let change = Change::Part {
            key: Some(key),
            parts: vec![Box::new(Bones {
                path,
                whose,
                other: after,
            })],
        };
        self.record(doc, label, change);
        Ok(())
    }
}

/// The GTA VI drawable row's `edited_sections`: the materials' and physics' edits, then the bones
/// the document shows, written over the body read again; `None` where it holds no edit.
pub(super) fn edited_save(document: &Document) -> Option<Result<EditedSave, String>> {
    let save = super::gta6_physics_edit::edited_save(document)?;
    let DocPayload::Gta6Drawable(resource) = &document.payload else {
        return Some(save);
    };
    let Some(path) = document.path.as_deref() else {
        return Some(save);
    };
    // Each skeleton held, `None` the body's own, else the dictionary entry's code.
    let skeletons: Vec<(Option<u32>, Skeleton)> = match **resource {
        Resource::Drawable(_) | Resource::DrawableWithBound(_) => {
            (super::rpf9_scan::scanned_skeleton(path).ok().into_iter())
                .map(|skeleton| (None, skeleton))
                .collect()
        }
        Resource::Dictionary(_) => (super::gta6_entry_skeletons::all(path).into_iter())
            .map(|(code, skeleton)| (Some(code), skeleton))
            .collect(),
        _ => Vec::new(),
    };
    if skeletons.is_empty() {
        return Some(save);
    }
    Some(save.map(|save| match save {
        EditedSave::Body {
            archive,
            offset,
            edit,
        } => {
            let edit: Arc<EditSections> = Arc::new(move |role, sections| {
                edit(role, sections)?;
                for (code, skeleton) in &skeletons {
                    let owner = match (code, role) {
                        (None, Some(Rpf9Role::Drawable)) => SkeletonOwner::Drawable,
                        (None, Some(Rpf9Role::DrawableWithBound)) => SkeletonOwner::Fragment,
                        (Some(code), Some(Rpf9Role::Dictionary)) => SkeletonOwner::Entry(*code),
                        _ => continue,
                    };
                    // Bones as read are written as nothing; names first, which the numbers'
                    // check holds to.
                    rename_bones(sections, owner, skeleton).map_err(|e| e.to_string())?;
                    write_skeleton(&mut sections.virtual_section, owner, skeleton)
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            });
            EditedSave::Body {
                archive,
                offset,
                edit,
            }
        }
        held => held,
    }))
}

#[cfg(test)]
#[path = "gta6_bone_edit_tests.rs"]
mod tests;
