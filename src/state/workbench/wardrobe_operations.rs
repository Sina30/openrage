//! The Wardrobe's edit operations on garments and their textures: a model or a texture replaced, a garment copied to the
//! other ped, moved in its slot, textures added, moved, tinted and taken out, and the same over a list.
//!
//! ⚠ **Nothing here is a rule or a write of its own.** `or-wardrobe` replaces, copies, moves and exports; this asks, runs
//! it on the Wardrobe's worker one edit after the other, and keeps what it handed back for the one Undo of the workspace.
//! Every operation is a step of Undo; an export writes outside the pack and so has none.

use std::path::{Path, PathBuf};

use or_wardrobe::edit::{self, Edit, Toward};
use or_wardrobe::{duplicate, export, package, replace, variations, Ped, Slot};

use super::wardrobe::{variation_letter, Done};
use crate::state::front_door::counted;
use crate::state::AppState;

/// The name of `path` as a sentence says it.
fn file_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn ped_word(ped: Ped) -> &'static str {
    match ped {
        Ped::Male => "male",
        Ped::Female => "female",
    }
}

/// How many folders called after one garment a package looks through for a free one.
const PACKAGE_FOLDER_TRIES: u32 = 1000;

/// The folder a package of the garment called `name` is written into: `to` when it holds nothing, else a free folder of its own inside it.
fn package_folder(to: &Path, name: &str) -> Result<PathBuf, String> {
    if std::fs::read_dir(to).is_ok_and(|mut d| d.next().is_none()) {
        return Ok(to.to_path_buf());
    }
    let safe: String = (name.chars())
        .map(|c| match c.is_alphanumeric() || "_-.".contains(c) {
            true => c,
            false => '_',
        })
        .collect();
    let safe = safe.trim_matches('.');
    let safe = if safe.is_empty() { "garment" } else { safe };
    (1..=PACKAGE_FOLDER_TRIES)
        .map(|n| match n {
            1 => to.join(safe),
            n => to.join(format!("{safe}-{n}")),
        })
        .find(|candidate| !candidate.exists())
        .ok_or_else(|| {
            tr!("{to} already holds {package_folder_tries} folders called {safe}: package into another folder", package_folder_tries = PACKAGE_FOLDER_TRIES, safe = safe, to = to.display())
        })
}

/// `one`'s name, or how many garments a list is.
fn subject(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        many => counted(many.len(), "garment"),
    }
}

impl AppState {
    /// The content shown, its folder to write and the name garment `id` goes by, or a toast that says why this window may not write.
    fn garment_to_edit(&mut self, id: &str) -> Option<(String, PathBuf, String)> {
        let module = self.wardrobe_module_id()?;
        let folder = self.folder_to_edit(&module)?;
        let name = self.garment_name(&module, id);
        Some((module, folder, name))
    }

    /// Put the file or source folder `source` in the place of the model of garment `id`; the garment keeps all it states. Undo writes the old file back.
    pub fn replace_model(&mut self, id: String, source: PathBuf) {
        if let Some(module) = self.wardrobe_module_id() {
            self.replace_model_in(module, id, source);
        }
    }

    /// [`Self::replace_model`] in the content `module`, which a Problem may name and the workspace not show.
    pub(super) fn replace_model_in(&mut self, module: String, id: String, source: PathBuf) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let file = file_of(&source);
        self.wardrobe_job(
            tr!("Replacing a model"),
            &module,
            Box::new(move || match replace::model(&folder, &id, &source) {
                Ok(revision) => Done::revised(
                    tr!(
                        "Replaced the model of {name} with {file}",
                        name = name,
                        file = file
                    ),
                    ("model", tr!("Replaced the model of {name}", name = name)),
                    Some(revision),
                ),
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }

    /// Put the image or texture dictionary `source` into texture `variation` of garment `id`: its tint, ids and flags stay. Undo writes the old file back.
    pub fn replace_texture(&mut self, id: String, variation: usize, source: PathBuf) {
        let Some((module, folder, name)) = self.garment_to_edit(&id) else {
            return;
        };
        let (file, letter) = (file_of(&source), variation_letter(variation));
        let is_dictionary = source
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ytd"));
        // A place that holds no file is filled.
        let held = (self.wardrobe_pack())
            .and_then(|pack| pack.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(&id))
            .and_then(|g| g.variations.get(variation))
            .is_some_and(|v| v.texture.is_none());
        let said = match (held, is_dictionary) {
            (true, _) => tr!(
                "Filled texture {letter} of {name} with {file}",
                letter = letter,
                name = name,
                file = file
            ),
            (false, true) => tr!(
                "Replaced texture {letter} of {name} with {file}",
                letter = letter,
                name = name,
                file = file
            ),
            (false, false) => tr!(
                "Made the texture {letter} of {name} from {file}",
                letter = letter,
                name = name,
                file = file
            ),
        };
        let step = match held {
            true => tr!(
                "Filled texture {letter} of {name}",
                letter = letter,
                name = name
            ),
            false => tr!(
                "Replaced texture {letter} of {name}",
                letter = letter,
                name = name
            ),
        };
        let job = match is_dictionary {
            true => tr!("Replacing a texture"),
            false => tr!("Making a texture from an image"),
        };
        self.wardrobe_job(
            job,
            &module,
            Box::new(
                move || match replace::texture(&folder, &id, variation, &source) {
                    Ok(revision) => Done::revised(said, ("texture", step), Some(revision)),
                    Err(why) => Done::said(Err(why.to_string())),
                },
            ),
        );
    }

    /// Copy the garments `ids` to `ped`, each with its own copy of its files and every flag. Undo takes the copies out.
    pub fn duplicate_garments(&mut self, ids: Vec<String>, ped: Ped) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let names: Vec<String> = ids
            .iter()
            .map(|id| self.garment_name(&module, id))
            .collect();
        let (what, ped_word) = (subject(&names), ped_word(ped));
        self.wardrobe_job(
            tr!("Copying garments"),
            &module,
            Box::new(move || match duplicate::to(&folder, &ids, ped) {
                Ok(done) => Done::revised(
                    tr!("Copied {what} to the {ped_word} ped; each copy takes the lowest free place of its slot", what = what, ped_word = ped_word),
                    ("copy", tr!("Copied {what} to the {ped_word} ped", what = what, ped_word = ped_word)),
                    Some(done.revision),
                ),
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }

    /// Swap the place of garment `id` with its neighbour's in its slot. Undo puts them back, and the sentence says which garments took another place.
    pub fn move_garment(&mut self, id: String, toward: Toward) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let word = match toward {
            Toward::Earlier => "earlier",
            Toward::Later => "later",
        };
        let said = tr!("Moved {name} {word} in its slot", name = name, word = word);
        let step = ("order", said.clone());
        self.pack_edit(tr!("Moving a garment"), step, &module, true, move |w| {
            edit::reorder(w, &id, toward)?;
            Ok(said)
        });
    }

    /// Add the texture dictionary `source` to garment `id` as its next texture. Undo takes it out.
    pub fn add_variation(&mut self, id: String, source: PathBuf) {
        let Some((module, folder, name)) = self.garment_to_edit(&id) else {
            return;
        };
        let file = file_of(&source);
        self.wardrobe_job(
            tr!("Adding a texture"),
            &module,
            Box::new(move || match variations::add(&folder, &id, &source) {
                Ok(revision) => Done::revised(
                    tr!(
                        "Added {file} to {name} as a texture",
                        file = file,
                        name = name
                    ),
                    (
                        tr!("texture added"),
                        tr!("Added a texture to {name}", name = name),
                    ),
                    Some(revision),
                ),
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }

    /// Take texture `variation` of garment `id`, which names `file`, out of it; its file stays. Undo puts it back.
    pub fn remove_variation(&mut self, id: String, variation: usize, file: Option<String>) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let (name, letter) = (self.garment_name(&module, &id), variation_letter(variation));
        let step = (
            tr!("texture removal"),
            tr!(
                "Removed texture {letter} of {name}",
                letter = letter,
                name = name
            ),
        );
        self.pack_edit(tr!("Removing a texture"), step, &module, false, move |w| {
            variations::remove(w, &id, variation, file.as_deref())?;
            Ok(tr!(
                "Removed texture {letter} of {name}; the textures after it moved up a letter",
                letter = letter,
                name = name
            ))
        });
    }

    /// Give texture `variation` of garment `id`, which names `file`, the letter at place `to`; the ones it passes change letter.
    pub fn move_variation(
        &mut self,
        id: String,
        variation: usize,
        file: Option<String>,
        to: usize,
    ) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let (from, now) = (variation_letter(variation), variation_letter(to));
        let said = tr!(
            "Moved texture {from} of {name} to {now}; the textures between changed letter",
            from = from,
            name = name,
            now = now
        );
        let step = (
            tr!("texture order"),
            tr!(
                "Moved texture {from} of {name} to {now}",
                from = from,
                name = name,
                now = now
            ),
        );
        self.pack_edit(tr!("Moving a texture"), step, &module, false, move |w| {
            variations::move_to(w, &id, variation, file.as_deref(), to)?;
            Ok(said)
        });
    }

    /// Give texture `variation` of garment `id`, which names `file`, the tint `tint`; none writes the skin's word again.
    pub fn tint_variation(
        &mut self,
        id: String,
        variation: usize,
        file: Option<String>,
        tint: Option<String>,
    ) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let (name, letter) = (self.garment_name(&module, &id), variation_letter(variation));
        let step = (
            "tint",
            tr!(
                "Changed the tint of texture {letter} of {name}",
                letter = letter,
                name = name
            ),
        );
        self.pack_edit(tr!("Changing a tint"), step, &module, false, move |w| {
            variations::tint(w, &id, variation, file.as_deref(), tint.as_deref())?;
            Ok(String::new())
        });
    }

    /// Take the garments `ids` out of the pack together; their files stay. One Undo puts them all back.
    pub fn remove_garments(&mut self, ids: Vec<String>) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        if let [one] = ids.as_slice() {
            return self.edit_garment(one.clone(), Edit::Remove);
        }
        if let Some((_, selected)) = &self.workbench.wardrobe.selected {
            if ids.contains(selected) {
                self.workbench.wardrobe.selected = None;
            }
        }
        let what = counted(ids.len(), "garment");
        let step = ("removal", tr!("Removed {what} from the pack", what = what));
        self.pack_edit(tr!("Removing garments"), step, &module, true, move |w| {
            edit::apply_all(w, &ids, Edit::Remove)?;
            Ok(tr!(
                "Removed {what} from the pack; their files stay",
                what = what
            ))
        });
    }

    /// Say that the garments `ids` dress `ped`. One Undo takes all of them back.
    pub fn set_ped_of(&mut self, ids: Vec<String>, ped: Option<Ped>) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let what = subject(
            &ids.iter()
                .map(|id| self.garment_name(&module, id))
                .collect::<Vec<_>>(),
        );
        let step = ("ped", tr!("Changed the ped of {what}", what = what));
        self.pack_edit(tr!("Changing a ped"), step, &module, false, move |w| {
            edit::apply_all(w, &ids, Edit::Ped(ped))?;
            Ok(String::new())
        });
    }

    /// Say that the garments `ids` are of `slot`. One Undo takes all of them back.
    pub fn set_slot_of(&mut self, ids: Vec<String>, slot: Option<Slot>) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let what = subject(
            &ids.iter()
                .map(|id| self.garment_name(&module, id))
                .collect::<Vec<_>>(),
        );
        let step = ("slot", tr!("Changed the slot of {what}", what = what));
        self.pack_edit(tr!("Changing a slot"), step, &module, true, move |w| {
            edit::apply_all(w, &ids, Edit::Slot(slot))?;
            Ok(String::new())
        });
    }

    /// Say what the garments `ids` are, the ped and the slot together: the answer to names a drop could not read. A side left none keeps what each garment states. One Undo takes all of it back.
    pub fn resolve_garments(&mut self, ids: Vec<String>, ped: Option<Ped>, slot: Option<Slot>) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        if ped.is_none() && slot.is_none() {
            return;
        }
        let what = self.names_of(&module, &ids);
        let is = if ids.len() == 1 { "is" } else { "are" };
        let said: Vec<String> = [
            ped.map(|p| ped_word(p).to_string()),
            slot.map(|s| s.name().to_string()),
        ]
        .into_iter()
        .flatten()
        .collect();
        let said = tr!(
            "Said what {what} {is}: {said}",
            what = what,
            is = is,
            said = said.join(", ")
        );
        let step = ("names", said.clone());
        self.pack_edit(
            tr!("Saying what garments are"),
            step,
            &module,
            true,
            move |w| {
                if let Some(ped) = ped {
                    edit::apply_all(w, &ids, Edit::Ped(Some(ped)))?;
                }
                if let Some(slot) = slot {
                    edit::apply_all(w, &ids, Edit::Slot(Some(slot)))?;
                }
                Ok(said)
            },
        );
    }

    /// The name `what` of the garments `ids` goes by in a sentence.
    fn names_of(&self, module: &str, ids: &[String]) -> String {
        subject(
            &ids.iter()
                .map(|id| self.garment_name(module, id))
                .collect::<Vec<_>>(),
        )
    }

    /// File the garments `ids` under `tag`. One Undo takes all of them back.
    pub fn tag_garments(&mut self, ids: Vec<String>, tag: String) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let what = self.names_of(&module, &ids);
        let step = (
            "tag",
            tr!("Tagged {what} {tag}", what = what, tag = tag.trim()),
        );
        self.pack_edit(tr!("Tagging garments"), step, &module, false, move |w| {
            edit::apply_all(w, &ids, Edit::AddTag(tag))?;
            Ok(String::new())
        });
    }

    /// Take `tag` off the garments `ids`. One Undo puts it back on all of them.
    pub fn untag_garments(&mut self, ids: Vec<String>, tag: String) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let what = self.names_of(&module, &ids);
        let step = (
            "tag",
            tr!("Took the tag {tag} off {what}", tag = tag, what = what),
        );
        self.pack_edit(tr!("Untagging garments"), step, &module, false, move |w| {
            edit::apply_all(w, &ids, Edit::RemoveTag(tag))?;
            Ok(String::new())
        });
    }

    /// Keep the garments `ids` in the group `group`, or in none. One Undo takes all of them back.
    pub fn group_garments(&mut self, ids: Vec<String>, group: Option<String>) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let what = self.names_of(&module, &ids);
        let step = match group.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
            Some(group) => (
                "group",
                tr!("Put {what} in {group}", what = what, group = group),
            ),
            None => ("group", tr!("Took {what} out of their group", what = what)),
        };
        self.pack_edit(tr!("Grouping garments"), step, &module, false, move |w| {
            edit::apply_all(w, &ids, Edit::Group(group))?;
            Ok(String::new())
        });
    }

    /// Write the files of the garments `ids` into the folder `to`, under the names a build gives them. The pack is not changed, so there is no Undo.
    pub fn export_garments(&mut self, ids: Vec<String>, to: PathBuf) {
        self.export_files(ids, None, to);
    }

    /// Write garment `id` as a package in `to`, or in a folder of its own inside it when `to` holds something. The pack is not changed, so there is no Undo.
    pub fn package_garment(&mut self, id: String, to: PathBuf) {
        let Some(pack) = self.wardrobe_pack() else {
            return;
        };
        let (module, folder) = (pack.module.clone(), pack.folder.clone());
        let name = self.garment_name(&module, &id);
        self.wardrobe_job(
            tr!("Packaging a garment"),
            &module,
            Box::new(move || {
                let into = match package_folder(&to, &name) {
                    Ok(into) => into,
                    Err(why) => return Done::said(Err(why)),
                };
                match package::export(&folder, &id, &into) {
                    Ok(done) => Done::said(Ok(tr!(
                        "Wrote the package of {name} to {folder}: {files} beside {package}",
                        name = name,
                        folder = into.display(),
                        files = counted(done.files.len(), "file"),
                        package = package::FILE
                    ))),
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Add the garment the package in the folder `package` holds to the pack, as a garment of its own. Undo takes it out.
    pub fn add_package(&mut self, from: PathBuf) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let shown = file_of(&from);
        self.wardrobe_job(
            tr!("Adding a garment package"),
            &module,
            Box::new(move || match package::import(&folder, &from) {
                Ok(added) => {
                    let mut said = tr!(
                        "Added the garment in the package {shown} to the pack",
                        shown = shown
                    );
                    if !added.notes.is_empty() {
                        said = tr!(
                            "{said}; the Log says what changed on the way in",
                            said = said
                        );
                    }
                    let step = ("package", tr!("Added the package {shown}", shown = shown));
                    Done::revised(said, step, Some(added.revision)).with_notes(added.notes)
                }
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }

    /// Write the file of texture `variation` of garment `id` into the folder `to`, under the name a build gives it.
    pub fn export_variation(&mut self, id: String, variation: usize, to: PathBuf) {
        self.export_files(vec![id], Some(variation), to);
    }

    fn export_files(&mut self, ids: Vec<String>, texture: Option<usize>, to: PathBuf) {
        let Some(pack) = self.wardrobe_pack() else {
            return;
        };
        let (module, folder) = (pack.module.clone(), pack.folder.clone());
        let shown = to.display().to_string();
        self.wardrobe_job(
            tr!("Exporting files"),
            &module,
            Box::new(move || {
                let exported = match texture {
                    Some(variation) => export::variation(&folder, &ids[0], variation, &to),
                    None => export::garments(&folder, &ids, &to),
                };
                match exported {
                    Err(why) => Done::said(Err(why.to_string())),
                    Ok(done) => {
                        let notes: Vec<String> = (done.left_out.iter())
                            .map(|left| format!("{}: {}", left.path, left.why))
                            .collect();
                        let mut said = tr!(
                            "Wrote {count} to {shown}",
                            shown = shown,
                            count = counted(done.written.len(), "file")
                        );
                        if !notes.is_empty() {
                            said = tr!(
                                "{said}; {count} left out, the Log has them",
                                said = said,
                                count = notes.len()
                            );
                        }
                        let nothing = done.written.is_empty();
                        let first = notes.first().cloned();
                        let said = match (nothing, first) {
                            (true, Some(first)) => Err(first),
                            _ => Ok(said),
                        };
                        Done::said(said).with_notes(notes)
                    }
                }
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_takes_the_folder_if_it_is_empty_else_the_first_free_name_and_never_panics_when_none_is(
    ) {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            package_folder(dir.path(), "lamp"),
            Ok(dir.path().to_path_buf())
        );

        std::fs::write(dir.path().join("keep.txt"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("lamp")).unwrap();
        std::fs::create_dir(dir.path().join("lamp-2")).unwrap();
        assert_eq!(
            package_folder(dir.path(), "lamp"),
            Ok(dir.path().join("lamp-3"))
        );
        assert_eq!(
            package_folder(dir.path(), "a/b"),
            Ok(dir.path().join("a_b")),
            "a name a folder cannot carry is made one"
        );

        for n in 3..=PACKAGE_FOLDER_TRIES {
            std::fs::create_dir(dir.path().join(format!("lamp-{n}"))).unwrap();
        }
        let why = package_folder(dir.path(), "lamp").expect_err("every name is taken");
        assert!(
            why.contains("lamp") && why.contains("another folder"),
            "{why}"
        );
    }
}
