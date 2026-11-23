//! What the Problems list does that leaves a file or changes the rules: the export of what it shows, the export of the rule list, and the rules of a clothing content as one step of Undo.

use or_wardrobe::settings::Settings;

use super::problems_view::{export, kept, Format, View};
use super::Question;
use crate::platform::file_dialogs;
use crate::state::AppState;

impl AppState {
    /// Write the problems `view` shows to a file the person names, in the format its extension asks for, and say where.
    pub fn export_problems(&mut self, view: &View) {
        let findings = self.workbench.findings().to_vec();
        let container = self.project_carried.container.clone();
        let shown = kept(&findings, view, &container);
        if shown.is_empty() {
            self.toasts.warning(tr!("There are no problems to export"));
            return;
        }
        let Some(path) = file_dialogs::ask(tr!("Export problems"), |dialog| {
            dialog
                .add_filter(tr!("Text"), &["txt"])
                .add_filter(tr!("CSV"), &["csv"])
                .add_filter(tr!("JSON"), &["json"])
                .set_file_name("problems.txt")
                .save_file()
        }) else {
            return;
        };
        let text = export(&shown, Format::of(&path.to_string_lossy()), &container);
        match std::fs::write(&path, text) {
            Ok(()) => self.toasts.success(tr!(
                "Wrote {said} to {path}",
                said = super::Counts::of(shown.iter().copied()).said(),
                path = path.display()
            )),
            Err(e) => self.toasts.error(tr!(
                "Could not write {path}: {e}",
                e = e,
                path = path.display()
            )),
        }
    }

    /// Write the list of rules the Wardrobe holds a pack to, as the document a script reads or as lines of text by the name chosen.
    pub fn export_rules(&mut self) {
        let Some(path) = file_dialogs::ask(tr!("Export rules"), |dialog| {
            dialog
                .add_filter(tr!("JSON"), &["json"])
                .add_filter(tr!("Text"), &["txt"])
                .set_file_name("wardrobe-rules.json")
                .save_file()
        }) else {
            return;
        };
        let text = match Format::of(&path.to_string_lossy()) {
            Format::Json => {
                let mut text =
                    serde_json::to_string_pretty(&or_wardrobe::rules::export()).unwrap_or_default();
                text.push('\n');
                text
            }
            _ => or_wardrobe::rules::export_text(),
        };
        match std::fs::write(&path, text) {
            Ok(()) => self
                .toasts
                .success(tr!("Wrote the rules to {path}", path = path.display())),
            Err(e) => self.toasts.error(tr!(
                "Could not write {path}: {e}",
                e = e,
                path = path.display()
            )),
        }
    }

    /// The rules the pack of the content `module` holds itself to as the last scan read them, none for a pack that did not read.
    pub fn rules_of_pack(&self, module: &str) -> Option<Settings> {
        let pack = self.workbench.packs().iter().find(|p| p.module == module)?;
        Some(pack.read.as_ref().ok()?.wardrobe.rules.clone())
    }

    /// How many drawables the last build of the clothing content `module` froze, none while it has none frozen.
    pub fn frozen_places(&self, module: &str) -> Option<usize> {
        let pack = self.workbench.packs().iter().find(|p| p.module == module)?;
        let lock = pack.read.as_ref().ok()?.wardrobe.lock.as_ref()?;
        Some(lock.places.len())
    }

    /// Open the rules of the clothing content `module` for the person to change.
    pub fn ask_about_rules(&mut self, module: String) {
        let Some(draft) = self.rules_of_pack(&module) else {
            self.toasts.warning(tr!(
                "The clothing could not be read, so its rules cannot be shown"
            ));
            return;
        };
        self.workbench.asking = Some(Question::Rules { module, draft });
    }

    /// Make `draft` the rules of the content `module`. Undo takes it back as one step.
    pub fn apply_rules(&mut self, module: String, draft: Settings) {
        let step = ("rules", tr!("Changed the rules of the pack").to_string());
        self.pack_edit(
            tr!("Changing the rules"),
            step,
            &module.clone(),
            false,
            move |w| {
                w.rules = draft;
                Ok(tr!("Changed the rules of the pack").to_string())
            },
        );
    }
}
