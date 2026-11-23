//! The edits the Wardrobe makes to a pack's own record, each one step of Undo with a label that names what it changed.
//!
//! ⚠ A step keeps the pack as it was and as the edit left it, so Undo puts back exactly that and is refused once
//! something has changed the pack since ([`or_wardrobe::edit::undo_revision`]); the drops, the removals and the cloth links keep their own, finer steps.

use std::path::PathBuf;

use or_wardrobe::behaviour::Behaviour;
use or_wardrobe::edit::{self, Edit};
use or_wardrobe::rules::name_of;
use or_wardrobe::shop_edit::Field as ShopField;
use or_wardrobe::{Finding, Wardrobe};

use super::wardrobe::{variation_letter, Change, Done};
use super::wardrobe_words::with_moves;
use super::{Fix, Missing, Question};
use crate::state::AppState;

/// What a step of Undo is called: the word its button ends with, and the sentence that says what it takes back.
pub(super) type Step = (&'static str, String);

/// What an edit of one garment is called, from the name the garment had before it.
fn step_of(name: &str, edit: &Edit) -> Step {
    match edit {
        Edit::Label(_) => ("name", tr!("Changed the name of {name}", name = name)),
        Edit::Ped(_) => ("ped", tr!("Changed the ped of {name}", name = name)),
        Edit::Slot(_) => ("slot", tr!("Changed the slot of {name}", name = name)),
        Edit::Position(Some(at)) => (
            "place",
            tr!(
                "Pinned {name} at {at}",
                name = name,
                at = format!("{at:03}")
            ),
        ),
        Edit::Position(None) => ("place", tr!("Unpinned {name}", name = name)),
        Edit::Skin(_) => ("skin", tr!("Changed the skin of {name}", name = name)),
        Edit::Heels(Some(_)) => ("heels", tr!("Changed the heels of {name}", name = name)),
        Edit::Heels(None) => ("heels", tr!("Took the heels off {name}", name = name)),
        Edit::Behaviour(change) => ("behaviour", behaviour_step(name, change)),
        Edit::AddTag(tag) => ("tag", tr!("Tagged {name} {tag}", name = name, tag = tag)),
        Edit::RemoveTag(tag) => (
            "tag",
            tr!("Took the tag {tag} off {name}", tag = tag, name = name),
        ),
        Edit::Group(Some(group)) => (
            "group",
            tr!("Put {name} in {group}", name = name, group = group),
        ),
        Edit::Group(None) => ("group", tr!("Took {name} out of its group", name = name)),
        Edit::Wind(Some(_)) => (
            "wind",
            tr!("Changed the wind of the cloth of {name}", name = name),
        ),
        Edit::Wind(None) => (
            "wind",
            tr!(
                "Gave the cloth of {name} the wind its file states",
                name = name
            ),
        ),
        Edit::Shop(texture, field) => ("shop", shop_step(name, *texture, field)),
        Edit::Remove => ("remove", tr!("Removed {name} from the pack", name = name)),
    }
}

/// What an edit of the shop entry of texture `texture` is called.
fn shop_step(name: &str, texture: usize, field: &ShopField) -> String {
    let letter = variation_letter(texture);
    match field {
        ShopField::Cost(Some(_)) => tr!(
            "Changed the price of texture {letter} of {name}",
            letter = letter,
            name = name
        ),
        ShopField::Cost(None) => tr!(
            "Took the price off texture {letter} of {name}",
            letter = letter,
            name = name
        ),
        ShopField::Tags(tags) if tags.is_empty() => {
            tr!(
                "Took the restriction tags off texture {letter} of {name}",
                letter = letter,
                name = name
            )
        }
        ShopField::Tags(_) => tr!(
            "Changed the restriction tags of texture {letter} of {name}",
            letter = letter,
            name = name
        ),
        ShopField::Forced(forced) if forced.is_empty() => {
            tr!(
                "Took the forced components off texture {letter} of {name}",
                letter = letter,
                name = name
            )
        }
        ShopField::Forced(_) => {
            tr!(
                "Changed the forced components of texture {letter} of {name}",
                letter = letter,
                name = name
            )
        }
        ShopField::Raw(element, Some(_)) => {
            tr!(
                "Changed {element} in the shop entry of texture {letter} of {name}",
                element = element,
                letter = letter,
                name = name
            )
        }
        ShopField::Raw(element, None) => {
            tr!(
                "Took {element} off the shop entry of texture {letter} of {name}",
                element = element,
                letter = letter,
                name = name
            )
        }
    }
}

/// What a change of behaviour is called.
fn behaviour_step(name: &str, change: &Behaviour) -> String {
    match change {
        Behaviour::Flag(flag, true) => {
            tr!("Set {flag} on {name}", name = name, flag = flag.label())
        }
        Behaviour::Flag(flag, false) => {
            tr!("Cleared {flag} on {name}", name = name, flag = flag.label())
        }
        Behaviour::Flags(_) => tr!("Changed the flags of {name}", name = name),
        Behaviour::Property(p, true) => tr!("Set {p} on {name}", name = name, p = p.label()),
        Behaviour::Property(p, false) => tr!("Cleared {p} on {name}", name = name, p = p.label()),
        Behaviour::Properties(_) => tr!("Changed the mask of {name}", name = name),
        Behaviour::Render(f, true) => tr!("Set {f} on {name}", name = name, f = f.label()),
        Behaviour::Render(f, false) => tr!("Cleared {f} on {name}", name = name, f = f.label()),
        Behaviour::Stickyness(_) => tr!("Changed how firmly {name} stays on", name = name),
        Behaviour::HairScale(Some(_)) => tr!("Changed the hair scale of {name}", name = name),
        Behaviour::HairScale(None) => tr!("Took the hair scale off {name}", name = name),
        Behaviour::HairHeight(Some(_)) => tr!("Changed the hair height of {name}", name = name),
        Behaviour::HairHeight(None) => tr!("Took the hair height off {name}", name = name),
        Behaviour::Audio(..) => tr!("Changed the audio of {name}", name = name),
        Behaviour::FirstPerson(Some(_)) => {
            tr!("Changed the first person alternate of {name}", name = name)
        }
        Behaviour::FirstPerson(None) => {
            tr!("Took the first person alternate off {name}", name = name)
        }
    }
}

impl AppState {
    /// Change what the selected garment, or the garment `id`, states. Undo takes it back.
    pub fn edit_garment(&mut self, id: String, change: Edit) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        if matches!(change, Edit::Remove) {
            if self.workbench.wardrobe.selected.as_ref().map(|(_, g)| g) == Some(&id) {
                self.workbench.wardrobe.selected = None;
            }
            return self.remove_garment(&module, id);
        }
        let step = step_of(&self.garment_name(&module, &id), &change);
        self.pack_edit(tr!("Changing a garment"), step, &module, false, move |w| {
            edit::apply(w, &id, change).map(|()| String::new())
        });
    }

    /// Give texture `texture` of garment `id` the shop id `shop_id`, or take that choice back with none so that the id an import read, or the build's name, is written again.
    pub fn set_shop_id(&mut self, id: String, texture: usize, shop_id: Option<String>) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let step = (
            tr!("shop id"),
            tr!("Changed a shop id of {name}", name = name),
        );
        self.pack_edit(tr!("Changing a shop id"), step, &module, false, move |w| {
            edit::set_shop_id(w, &id, texture, shop_id).map(|_| String::new())
        });
    }

    /// Take garment `id` out of the pack of `module`, keeping it whole so that Undo can put it back, and say which garments took another place.
    pub(super) fn remove_garment(&mut self, module: &str, id: String) {
        let Some(folder) = self.folder_to_edit(module) else {
            return;
        };
        self.wardrobe_job(
            tr!("Removing a garment"),
            module,
            Box::new(move || {
                let removed = edit::update(&folder, |w| {
                    let before = w.clone();
                    let removed = edit::take_out(w, &id)?;
                    Ok((removed, edit::moved(&before, w)))
                });
                match removed {
                    Ok((removed, moved)) => Done::with(
                        with_moves(
                            tr!(
                                "Removed {garment} from the pack; its files stay",
                                garment = name_of(&removed.garment)
                            ),
                            &moved,
                        ),
                        Change::Removed(Box::new(removed)),
                    ),
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Apply the remedy of `finding` to the content `module`. Undo takes it back.
    pub fn fix_finding(&mut self, module: String, finding: Finding) {
        let step = ("fix", tr!("Fixed: {message}", message = finding.message));
        self.pack_edit(tr!("Fixing"), step, &module.clone(), true, move |w| {
            let fix = edit::remedy(w, &finding).ok_or_else(|| {
                edit::Error::Refused(tr!("There is nothing to fix there any more").into())
            })?;
            fix.apply(w)?;
            Ok(tr!("Fixed: {what}", what = fix.what))
        });
    }

    /// Record that `code` stands for `garment` (every garment where none), for `reason`. Undo takes it back.
    pub fn waive_finding(
        &mut self,
        module: String,
        code: String,
        garment: Option<String>,
        reason: String,
    ) {
        let rule =
            or_wardrobe::rule_of(&code).map_or_else(|| code.clone(), |r| r.summary.to_string());
        let shown = match garment.as_deref() {
            Some(id) => tr!(
                "\"{rule}\" for {garment}",
                rule = rule,
                garment = self.garment_name(&module, id)
            ),
            None => tr!("\"{rule}\" for every garment", rule = rule),
        };
        let step = ("waiver", tr!("Waived {shown}", shown = shown));
        self.pack_edit(
            tr!("Recording a waiver"),
            step,
            &module.clone(),
            false,
            move |w| {
                edit::waive(w, &code, garment.as_deref(), &reason)?;
                Ok(tr!(
                    "Waived {shown}: {reason}",
                    shown = shown,
                    reason = reason.trim()
                ))
            },
        );
    }

    /// Take the waiver at `index` of the content shown out of the pack, so its finding stands again.
    pub fn remove_waiver(&mut self, index: usize) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let step = (tr!("waiver removal"), tr!("Removed a waiver").to_string());
        self.pack_edit(tr!("Removing a waiver"), step, &module, false, move |w| {
            if index >= w.waivers.len() {
                return Err(edit::Error::Refused(
                    tr!("That waiver is not in the pack any more").into(),
                ));
            }
            w.waivers.remove(index);
            Ok(String::new())
        });
    }

    /// Do what a person chose from the buttons of a finding of a clothing content.
    ///
    /// Fix… and Waive… ask first, what the fix will change or why the finding stands; Locate… asks for the file; Remove acts, since Undo takes it back.
    pub fn ask_about_fix(&mut self, fix: &Fix) {
        match fix {
            Fix::Remedy { module, finding } => {
                self.workbench.asking = Some(Question::Fix {
                    module: module.clone(),
                    finding: finding.clone(),
                });
            }
            Fix::Waive { module, finding } => {
                self.workbench.asking = Some(Question::Waive {
                    module: module.clone(),
                    finding: finding.clone(),
                    reason: String::new(),
                    focus: true,
                });
            }
            Fix::FindFile {
                module,
                garment,
                what,
            } => self.locate_missing(module.clone(), garment.clone(), what.clone()),
            Fix::DropFile {
                module,
                garment,
                what,
            } => self.drop_missing(module.clone(), garment.clone(), what.clone()),
            Fix::Repair { module, garment } => self.repair_model(module.clone(), garment.clone()),
            Fix::WaivePlugin { code } => {
                self.workbench.asking = Some(Question::WaivePlugin {
                    code: code.clone(),
                    reason: String::new(),
                    focus: true,
                });
            }
            Fix::Plugin {
                name,
                code,
                message,
                fix,
            } => {
                self.workbench.asking = Some(Question::PluginFix {
                    name: name.clone(),
                    code: code.clone(),
                    message: message.clone(),
                    fix: fix.clone(),
                });
            }
            Fix::Materials { module, garment } => {
                self.complete_materials(module.clone(), Some(vec![garment.clone()]))
            }
            Fix::Locate(_) | Fix::Remove(_) | Fix::Reid(_) => {}
        }
    }

    /// Ask, in the picker, for the file a texture, a cloth or a model of a garment is in now, and point the garment at it.
    pub fn locate_missing(&mut self, module: String, garment: String, what: Missing) {
        self.ask_for_a_file(super::Wanting::Locate {
            module,
            garment,
            what,
        });
    }

    /// Point a texture or a cloth of garment `garment` of the content `module` at `file`, copying it into the pack. Undo takes it back.
    pub fn locate_file(&mut self, module: String, garment: String, what: Missing, file: PathBuf) {
        let (texture, named) = match what {
            Missing::Texture(texture, named) => (texture, named),
            Missing::Cloth => return self.link_cloth_in(module, garment, file),
            Missing::Model => return self.replace_model_in(module, garment, file),
        };
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let (name, letter) = (
            self.garment_name(&module, &garment),
            variation_letter(texture),
        );
        let shown =
            (file.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.wardrobe_job(
            tr!("Locating a texture"),
            &module,
            Box::new(move || {
                match edit::locate_texture(&folder, &garment, texture, &named, &file) {
                    Ok(revision) => Done::revised(
                        tr!(
                            "Texture {letter} of {name} now comes from {shown}",
                            letter = letter,
                            name = name,
                            shown = shown
                        ),
                        (
                            "locate",
                            tr!(
                                "Located texture {letter} of {name}",
                                letter = letter,
                                name = name
                            ),
                        ),
                        Some(revision),
                    ),
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Take a texture or a cloth that is not there off garment `garment`; its file, where there is one, stays. Undo takes it back.
    pub fn drop_missing(&mut self, module: String, garment: String, what: Missing) {
        let (texture, named) = match what {
            Missing::Texture(texture, named) => (texture, named),
            Missing::Cloth => return self.unlink_cloth_in(module, garment),
            // A model that is not there is put right by choosing another, and a garment is taken out in the list.
            Missing::Model => return,
        };
        let (name, letter) = (
            self.garment_name(&module, &garment),
            variation_letter(texture),
        );
        let step = (
            tr!("texture removal"),
            tr!(
                "Removed texture {letter} of {name}",
                letter = letter,
                name = name
            ),
        );
        self.pack_edit(tr!("Removing a texture"), step, &module, false, move |w| {
            edit::remove_texture(w, &garment, texture, &named)?;
            Ok(tr!(
                "Removed texture {letter} of {name}; the textures after it moved up a letter",
                letter = letter,
                name = name
            ))
        });
    }

    /// Name the pack of the content shown `name`; a name another clothing content's pack has is refused, since two packs of one name claim the same collections.
    pub fn rename_pack(&mut self, name: String) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let name = name.trim().to_string();
        let taken = (self.workbench.packs().iter())
            .filter(|p| p.module != module)
            .filter_map(|p| p.read.as_ref().ok())
            .any(|read| read.wardrobe.pack == name);
        if taken {
            let why = tr!("Another clothing content already has a pack called {name}: two packs of one name claim the same collections", name = name);
            self.workbench.note(why.clone());
            self.show_error_toast(why);
            return;
        }
        let step = (
            tr!("pack name"),
            tr!("Renamed the pack to {name}", name = name),
        );
        self.pack_edit(tr!("Renaming the pack"), step, &module, false, move |w| {
            edit::rename_pack(w, &name)?;
            Ok(tr!("The pack is called {name}", name = name))
        });
    }

    /// Name the shop entries as Durty Cloth Tool does, or as OpenRage does.
    pub fn set_durty_hashes(&mut self, on: bool) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let step = (
            tr!("shop names"),
            match on {
                true => tr!("Named shop entries as Durty Cloth Tool does").to_string(),
                false => tr!("Named shop entries as OpenRage does").to_string(),
            },
        );
        self.pack_edit(
            tr!("Changing the shop names"),
            step,
            &module,
            false,
            move |w| {
                w.shop.durty_hashes = on;
                Ok(String::new())
            },
        );
    }

    /// Run `change` on the pack of `module` off the UI thread: read, change, write; it says what it did and keeps what the pack was for Undo.
    ///
    /// `moves` adds which garments took another place, for an edit that takes one out of the way.
    pub(super) fn pack_edit(
        &mut self,
        job: &str,
        step: Step,
        module: &str,
        moves: bool,
        change: impl FnOnce(&mut Wardrobe) -> Result<String, edit::Error> + Send + 'static,
    ) {
        let Some(folder) = self.folder_to_edit(module) else {
            return;
        };
        self.wardrobe_job(
            job,
            module,
            Box::new(move || match edit::revise(&folder, change) {
                Ok((said, revision)) => {
                    let said = match (&revision, moves) {
                        (Some(r), true) => with_moves(said, &edit::moved(&r.before, &r.after)),
                        _ => said,
                    };
                    Done::revised(said, step, revision)
                }
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }
}
