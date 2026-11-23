//! The power features of the Wardrobe on the state side: saved queries, a bulk edit on a query with its dry run, the review of two versions,
//! the credit of a garment's sources, and a whole pack as a package.
//!
//! ⚠ **Nothing here is a rule or a write of its own.** The query, the dry run, the diff, the credits and the package are `or-wardrobe`'s;
//! this asks for them, runs them off the UI thread one after the other like every other edit of the pack, and keeps the step of Undo.

use std::path::PathBuf;

use or_source::asset::Credit;
use or_wardrobe::bulk::{dry_run, DryRun};
use or_wardrobe::credits::Stated;
use or_wardrobe::edit::{self, Edit};
use or_wardrobe::query;
use or_wardrobe::review::{between, Review};
use or_wardrobe::{Ped, Wardrobe};

use super::wardrobe::Done;
use super::Question;
use crate::state::front_door::counted;
use crate::state::toasts::{Toast, ToastAction};
use crate::state::AppState;

/// What a bulk edit does to each garment it acts on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BulkKind {
    #[default]
    AddTag,
    RemoveTag,
    SetGroup,
    ClearGroup,
    SetPed,
    Remove,
}

impl BulkKind {
    pub const ALL: [BulkKind; 6] = [
        BulkKind::AddTag,
        BulkKind::RemoveTag,
        BulkKind::SetGroup,
        BulkKind::ClearGroup,
        BulkKind::SetPed,
        BulkKind::Remove,
    ];

    /// What the choice is called on screen.
    pub fn label(self) -> &'static str {
        match self {
            BulkKind::AddTag => tr!("Add a tag"),
            BulkKind::RemoveTag => tr!("Take a tag off"),
            BulkKind::SetGroup => tr!("Put in a group"),
            BulkKind::ClearGroup => tr!("Take out of their group"),
            BulkKind::SetPed => tr!("Say which ped they dress"),
            BulkKind::Remove => tr!("Take out of the pack"),
        }
    }

    /// Whether the edit needs a word typed.
    pub fn needs_text(self) -> bool {
        matches!(
            self,
            BulkKind::AddTag | BulkKind::RemoveTag | BulkKind::SetGroup
        )
    }
}

/// The edit a person is making to the garments of a query, as the window holds it before Apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkDraft {
    pub kind: BulkKind,
    pub text: String,
    pub ped: Ped,
}

impl Default for BulkDraft {
    fn default() -> Self {
        Self {
            kind: BulkKind::default(),
            text: String::new(),
            ped: Ped::Male,
        }
    }
}

impl BulkDraft {
    /// The edit it states, none while a word it needs is not typed.
    pub fn edit(&self) -> Option<Edit> {
        let word = self.text.trim();
        let needs = self.kind.needs_text() && word.is_empty();
        if needs {
            return None;
        }
        Some(match self.kind {
            BulkKind::AddTag => Edit::AddTag(word.to_string()),
            BulkKind::RemoveTag => Edit::RemoveTag(word.to_string()),
            BulkKind::SetGroup => Edit::Group(Some(word.to_string())),
            BulkKind::ClearGroup => Edit::Group(None),
            BulkKind::SetPed => Edit::Ped(Some(self.ped)),
            BulkKind::Remove => Edit::Remove,
        })
    }

    /// What the edit did to `n` garments, in the past, as a step of Undo is called.
    fn done_to(&self, n: usize) -> String {
        let what = counted(n, "garment");
        let word = self.text.trim();
        match self.kind {
            BulkKind::AddTag => tr!("Tagged {what} {word}", what = what, word = word),
            BulkKind::RemoveTag => tr!("Took the tag {word} off {what}", word = word, what = what),
            BulkKind::SetGroup => tr!("Put {what} in {word}", what = what, word = word),
            BulkKind::ClearGroup => tr!("Took {what} out of their group", what = what),
            BulkKind::SetPed => tr!("Said which ped {what} dress", what = what),
            BulkKind::Remove => tr!("Removed {what} from the pack", what = what),
        }
    }
}

/// Who made a garment's sources, as the window holds it before Apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreditDraft {
    pub author: String,
    pub url: String,
    pub licence: String,
}

impl CreditDraft {
    pub fn credit(&self) -> Credit {
        let said = |text: &str| text.trim().to_string();
        Credit {
            author: said(&self.author),
            url: said(&self.url),
            licence: said(&self.licence),
        }
    }
}

impl AppState {
    /// The pack the Wardrobe shows, as the last scan read it.
    fn shown_wardrobe(&self, module: &str) -> Option<&Wardrobe> {
        (self.workbench.packs().iter())
            .find(|p| p.module == module)
            .and_then(|p| p.read.as_ref().ok())
            .map(|read| &read.wardrobe)
    }

    /// Ask what to do to the garments `ids` that the query `query` selects.
    pub fn ask_to_bulk_edit(&mut self, module: String, query: String, ids: Vec<String>) {
        self.workbench.asking = Some(Question::Bulk {
            module,
            query,
            ids,
            draft: BulkDraft::default(),
            shown: None,
        });
    }

    /// What `draft` would do to the garments `ids`, before anything is written; none while the draft states no edit.
    pub fn bulk_dry_run(
        &self,
        module: &str,
        ids: &[String],
        draft: &BulkDraft,
    ) -> Option<std::sync::Arc<DryRun>> {
        let edit = draft.edit()?;
        Some(std::sync::Arc::new(dry_run(
            self.shown_wardrobe(module)?,
            ids,
            &edit,
        )))
    }

    /// Make the edit of `draft` to the garments `ids`, all or none. One Undo takes all of it back.
    pub fn bulk_edit(&mut self, module: String, ids: Vec<String>, draft: BulkDraft) {
        let Some(edit) = draft.edit() else {
            return;
        };
        if let Some((_, selected)) = &self.workbench.wardrobe.selected {
            if edit == Edit::Remove && ids.contains(selected) {
                self.workbench.wardrobe.selected = None;
            }
        }
        let said = draft.done_to(ids.len());
        let moves = matches!(draft.kind, BulkKind::SetPed | BulkKind::Remove);
        let step = ("bulk", said.clone());
        self.pack_edit(tr!("Editing garments"), step, &module, moves, move |w| {
            edit::apply_all(w, &ids, edit)?;
            Ok(said)
        });
    }

    /// Why `text` cannot be kept as a query called `name` in the pack, none where it can.
    pub fn query_problem(&self, module: &str, name: &str, text: &str) -> Option<String> {
        let mut copy = self.shown_wardrobe(module)?.clone();
        query::save(&mut copy, name, text)
            .err()
            .map(|e| e.to_string())
    }

    /// Keep the query `text` in the pack under `name`, where the list offers it as a group. One Undo takes it back.
    pub fn save_query(&mut self, module: String, name: String, text: String) {
        let step = (
            tr!("save query"),
            tr!("Saved the query {name}", name = name.trim()),
        );
        let said = tr!(
            "Saved the query {name}: {text}",
            name = name.trim(),
            text = text.trim()
        );
        self.pack_edit(tr!("Saving a query"), step, &module, false, move |w| {
            query::save(w, &name, &text)?;
            Ok(said)
        });
    }

    /// Let the saved query `name` go. One Undo brings it back.
    pub fn forget_query(&mut self, module: String, name: String) {
        let step = (
            tr!("forget query"),
            tr!("Forgot the query {name}", name = name),
        );
        let said = tr!("Forgot the query {name}", name = name);
        self.pack_edit(tr!("Forgetting a query"), step, &module, false, move |w| {
            query::forget(w, &name)?;
            Ok(said)
        });
    }

    /// Open the review of the pack's versions: the newest earlier one against the pack as it is.
    pub fn ask_to_review(&mut self) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        self.load_versions();
        let from = (self.workbench.wardrobe.versions.as_ref())
            .and_then(|v| v.list.get(1).or(v.list.first()))
            .map(|(version, _)| version.n);
        let shown = self.review_of(&module, from, None).map(std::sync::Arc::new);
        self.workbench.asking = Some(Question::Review {
            module,
            from,
            to: None,
            shown,
        });
    }

    /// The pack of `module` as version `n` held it, the pack as it is where `n` is none.
    pub fn review_side(&self, module: &str, n: Option<u64>) -> Option<Wardrobe> {
        match n {
            None => self.shown_wardrobe(module).cloned(),
            Some(n) => (self.workbench.wardrobe.versions.as_ref())
                .filter(|v| v.module == module)
                .and_then(|v| v.list.iter().find(|(version, _)| version.n == n))
                .map(|(_, pack)| pack.clone()),
        }
    }

    /// What differs between the versions `from` and `to` of the pack of `module`; a side that is none is the pack as it is.
    pub fn review_of(&self, module: &str, from: Option<u64>, to: Option<u64>) -> Option<Review> {
        Some(between(
            &self.review_side(module, from)?,
            &self.review_side(module, to)?,
        ))
    }

    /// Draw each garment that differs between the versions `from` and `to` of the pack of `module` as each held it, into `out`.
    pub fn review_pictures(
        &mut self,
        module: &str,
        from: Option<u64>,
        to: Option<u64>,
        out: PathBuf,
    ) {
        let (Some(before), Some(after)) =
            (self.review_side(module, from), self.review_side(module, to))
        else {
            return;
        };
        let review = between(&before, &after);
        match self.start_review_pictures(out, &review, &before, &after) {
            Ok(()) => self.show_toast(tr!("Drawing the pictures of both sides: the preview's Pictures shows how far it has got")),
            Err(why) => self.show_warning_toast(why),
        }
    }

    /// What garment `id` of the pack of `module` is called in a sentence.
    pub fn garment_display_name(&self, module: &str, id: &str) -> String {
        self.garment_name(module, id)
    }

    /// Ask who made the sources of garment `id`, with what they state already.
    pub fn ask_to_credit(&mut self) {
        let (Some(module), Some(id)) = (
            self.wardrobe_module_id(),
            self.workbench
                .wardrobe
                .selected
                .as_ref()
                .map(|(_, id)| id.clone()),
        ) else {
            return;
        };
        let Some(pack) = self.wardrobe_pack() else {
            return;
        };
        let draft = pack
            .read
            .as_ref()
            .ok()
            .map(|read| or_wardrobe::credits::entries(&read.wardrobe, &pack.folder))
            .and_then(|entries| {
                let garment = self.shown_wardrobe(&module)?.garment(&id)?.clone();
                let name = or_wardrobe::rules::name_of(&garment);
                entries
                    .into_iter()
                    .find(|e| e.used_by.contains(&name))
                    .and_then(|e| e.credit)
            })
            .map(|c| CreditDraft {
                author: c.author,
                url: c.url,
                licence: c.licence,
            })
            .unwrap_or_default();
        let had = draft != CreditDraft::default();
        self.workbench.asking = Some(Question::Credit {
            module,
            garment: id,
            draft,
            had,
        });
    }

    /// State the credit in the `asset.json` of every source of the pack's own folder that garment `id` names; an empty one takes the credit away. The pack is not changed, so there is no Undo: state it again to change it.
    pub fn credit_garment(&mut self, module: String, id: String, draft: CreditDraft) {
        let Some(pack) = self.wardrobe_pack() else {
            return;
        };
        let (folder, name) = (pack.folder.clone(), self.garment_name(&module, &id));
        let Some(wardrobe) = self.shown_wardrobe(&module).cloned() else {
            return;
        };
        self.wardrobe_job(
            tr!("Crediting a garment"),
            &module,
            Box::new(move || {
                match or_wardrobe::credits::state_for(&folder, &wardrobe, &id, &draft.credit()) {
                    Ok(Stated { set, left }) => {
                        let mut said = match set.len() {
                            0 => tr!(
                                "{name} names no source of the pack's own to credit",
                                name = name
                            ),
                            n => tr!(
                                "Credited {n}: {set}",
                                n = counted(n, "source"),
                                set = set.join(", ")
                            ),
                        };
                        if !left.is_empty() {
                            said.push_str(&tr!(
                                "; {left} is another project's and states its own credit",
                                left = left.join(", ")
                            ));
                        }
                        Done::said(Ok(said))
                    }
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Write the pack of `module` as one package file `to`. The pack is not changed, so there is no Undo.
    pub fn export_pack_package(&mut self, to: PathBuf) {
        let Some(pack) = self.wardrobe_pack() else {
            return;
        };
        let (module, folder) = (pack.module.clone(), pack.folder.clone());
        self.wardrobe_job(
            tr!("Packaging the pack"),
            &module,
            Box::new(
                move || match or_wardrobe::pack_package::export(&folder, &to) {
                    Ok(packed) => Done::said(Ok(tr!(
                        "Packaged {garments} and {files} into {to}",
                        garments = counted(packed.garments, "garment"),
                        files = counted(packed.files, "file"),
                        to = to.display()
                    ))),
                    Err(why) => Done::said(Err(why.to_string())),
                },
            ),
        );
    }

    /// Unpack the package `from` into a clothing folder of its own in the project and add it as a clothing content that every build ships.
    ///
    /// ponytail: on the UI thread, which a package of sources unpacks in a second or two; a job when packs outgrow that.
    pub fn import_pack_package(&mut self, from: PathBuf) {
        if self.refused_as_read_only() {
            return;
        }
        let Some(project) = self.project_path.clone() else {
            return;
        };
        let container = &self.project_carried.container;
        let taken: Vec<&str> = container.modules.iter().map(|m| m.id.as_str()).collect();
        let id = super::commands::unique("clothing", taken.into_iter());
        let folder = or_project::project_folder(&project).join(&id);
        let refused = or_project::links::refuse_write(
            &self.project_carried.container,
            &project,
            &folder.join(or_wardrobe::FILE),
        );
        if let Err(why) = refused {
            self.show_error_toast(why);
            return;
        }
        let taken = self.pack_names(&project);
        let mut imported = match or_wardrobe::pack_package::import(&from, &folder) {
            Ok(imported) => imported,
            Err(why) => {
                self.show_error_toast(why.to_string());
                return;
            }
        };
        // ⚠ Two packs of one name claim the same collections, which a build refuses: the pack that comes in takes a name that is free, and the toast says so.
        let asked_for = imported.pack.clone();
        if taken.contains(&asked_for) {
            let free = super::wardrobe::free_pack_name(&asked_for, &taken);
            if edit::update(&folder, |w| edit::rename_pack(w, &free)).is_ok() {
                imported.pack = free;
            }
        }
        let container = &mut self.project_carried.container;
        container
            .modules
            .push(or_project::Module::clothing(&id, &id));
        for output in &mut container.outputs {
            output.take.push(id.clone());
        }
        self.workbench.want_scan();
        self.workbench.wardrobe.module = Some(id.clone());
        self.record_step(tr!(
            "Unpacked the pack {pack} as the clothing content {id}",
            id = id,
            pack = imported.pack
        ));
        self.active_workspace = crate::ui::dock::Workspace::Wardrobe;
        let held = format!(
            "{} and {}",
            counted(imported.garments, "garment"),
            counted(imported.files, "file")
        );
        if taken.contains(&imported.pack) {
            // The name could not be changed, so a build refuses until someone does: a toast that stays, with the way to do it.
            self.toasts.push(
                Toast::warning(tr!("Unpacked the pack {pack}: {held}. Another pack of this project has the same name, so no build can go through until one is renamed", held = held, pack = imported.pack))
                .with_action(ToastAction::RenamePack)
                .lasting(),
            );
        } else if imported.pack != asked_for {
            self.show_toast_with_action(
                tr!("Unpacked the pack {asked_for} as {pack}, because another pack of this project is already called {asked_for}: {held}", asked_for = asked_for, held = held, pack = imported.pack),
                ToastAction::RenamePack,
            );
        } else {
            self.show_toast(tr!(
                "Unpacked the pack {pack}: {held}",
                held = held,
                pack = imported.pack
            ));
        }
    }
}
