//! Making a change again after it was taken back.
//!
//! Taking a drop, a link or an edit back writes the pack as it was and removes the files the change copied. What
//! Redo needs is kept by that: the pack on both sides of the undo, and the files it moved aside, which wait in the
//! pack's private folder (`.openrage/held`) instead of being deleted. They go when a new change ends the chance of a Redo
//! ([`purge_held`]), and never sooner.
//!
//! ⚠ Redo is made over the pack Undo left and over no other: a pack that something changed since is refused with
//! the reason, because writing the older pack back would lose that change.

use std::path::{Path, PathBuf};

use crate::backup::{make_private, private_dir};
use crate::edit::{files_of, Copying, Error};
use crate::facts::stays_inside;
use crate::model::Wardrobe;

const HELD: &str = "held";

/// What taking a change back left, which is all Redo needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Redo {
    /// The pack as taking the change back left it: the only pack Redo is made over.
    pub before: Wardrobe,
    /// The pack as the change had left it.
    pub after: Wardrobe,
    /// The files taking it back moved aside, as paths inside the pack's folder.
    pub held: Vec<String>,
}

fn held_path(folder: &Path, rel: &str) -> PathBuf {
    private_dir(folder).join(HELD).join(rel)
}

/// Run `change` over the pack in `folder` as Undo, then remove the files `copied` that no garment names any more (keeping them for Redo) and the folders `made` that they leave empty.
pub(crate) fn undone(
    folder: &Path,
    copied: &[String],
    made: &[String],
    change: impl FnOnce(&mut Wardrobe) -> Result<(), Error>,
) -> Result<Redo, Error> {
    let after = Wardrobe::read(folder)?;
    let mut pack = after.clone();
    change(&mut pack)?;
    pack.write(folder)?;
    let before = Wardrobe::read(folder)?;
    let named: Vec<String> = (before.garments.iter())
        .flat_map(files_of)
        .chain(before.shop.carried.iter().map(|c| c.file.clone()))
        .collect();
    // ⚠ A garment that names a source folder names every file in it.
    let is_named = |rel: &str| {
        (named.iter()).any(|n| {
            rel == n
                || rel
                    .strip_prefix(n.as_str())
                    .is_some_and(|r| r.starts_with('/'))
        })
    };
    let gone: Vec<String> = (copied.iter())
        .filter(|rel| !is_named(rel))
        .cloned()
        .collect();
    let mut copying = Copying::new(folder);
    copying.copied = gone.clone();
    copying.made = made.to_vec();
    let held = copying.hold();
    Ok(Redo {
        before,
        after,
        held,
    })
}

impl Copying<'_> {
    /// [`Copying::undo`], except that each file is moved to the private folder and not deleted; the paths of the files that wait there.
    ///
    /// ⚠ A file that cannot be moved is deleted as `undo` does: Undo is not refused for a Redo that cannot be had.
    pub(crate) fn hold(&mut self) -> Vec<String> {
        let ready = make_private(self.folder).is_ok();
        let mut held = Vec::new();
        for rel in std::mem::take(&mut self.copied) {
            let (from, to) = (self.folder.join(&rel), held_path(self.folder, &rel));
            let moved = ready
                && to
                    .parent()
                    .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
                && (!to.exists() || std::fs::remove_file(&to).is_ok())
                && std::fs::rename(&from, &to).is_ok();
            match moved {
                true => held.push(rel),
                false => {
                    let _ = std::fs::remove_file(&from);
                }
            }
        }
        self.undo();
        held
    }
}

/// Make the change again: the files taking it back moved aside return, and the pack is what the change had left.
///
/// A file that is gone from the private folder is not made again, and the garment that names it shows it missing; a file
/// that is in the pack again with other bytes refuses the Redo.
pub fn redo(folder: &Path, redo: &Redo) -> Result<(), Error> {
    if !Wardrobe::read(folder)?.states_the_same_as(&redo.before) {
        return Err(Error::Refused(
            "the pack was changed after that was taken back, so making it again would lose the later changes: take those back first".into(),
        ));
    }
    let mut returned: Vec<(PathBuf, PathBuf)> = Vec::new();
    let put_aside_again = |returned: &[(PathBuf, PathBuf)]| {
        for (from, to) in returned {
            let _ = std::fs::rename(to, from);
        }
    };
    for rel in redo.held.iter().filter(|rel| stays_inside(rel)) {
        let (from, to) = (held_path(folder, rel), folder.join(rel));
        if !from.is_file() {
            continue;
        }
        if to.exists() {
            put_aside_again(&returned);
            return Err(Error::Refused(format!(
                "{rel} is in the pack again with other contents, so the change cannot be made again over it"
            )));
        }
        let moved = to
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::rename(&from, &to));
        if let Err(e) = moved {
            put_aside_again(&returned);
            return Err(Error::Io(to.display().to_string(), e));
        }
        returned.push((from, to));
    }
    if let Err(e) = redo.after.write(folder) {
        put_aside_again(&returned);
        return Err(e.into());
    }
    Ok(())
}

/// Delete the files kept for a Redo: a new change has ended the chance of one.
pub fn purge_held(folder: &Path) {
    let _ = std::fs::remove_dir_all(private_dir(folder).join(HELD));
}
