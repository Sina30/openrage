//! The garments picked in the Wardrobe's list: one, several by Shift or Ctrl, or a heading's, and what the keys do with them.
//!
//! ⚠ **The garment selected is the source of truth.** The picks are read only while one is selected and only for the
//! garments the pack still holds, so a pick left over from a removal or another content is inert, never a garment a
//! Delete would take that the list does not show picked. A filter that hides garments picked earlier lets them go
//! (`keep_picks_listed`), so a pick of several is always what the list shows.

use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};

use or_wardrobe::facts::stays_inside;
use or_wardrobe::follow::{follow, Led};

use crate::state::{AppState, DocumentKind};
use crate::ui::dock::Workspace;

/// The file or source folder a pack names as `name` (with `/`) in its `folder`, where a reference leads to it, or none for a name that leaves the pack (absolute, `..`, a drive or a share), which nothing here may stat, read or open, and for an id alone, which is no file.
pub fn in_pack(folder: &Path, name: &str) -> Option<PathBuf> {
    if !stays_inside(name) {
        return None;
    }
    match follow(folder, name) {
        Led::Own => Some(folder.join(name.replace('/', MAIN_SEPARATOR_STR))),
        Led::To { at, .. } => Some(at),
        Led::Id(_) | Led::Nowhere { .. } => None,
    }
}

impl AppState {
    /// The ids of the garments picked in the content shown, in the pack's order; the garment selected alone where none was picked with it.
    pub fn picked_garments(&self) -> Vec<String> {
        let work = &self.workbench.wardrobe;
        let Some((module, selected)) = &work.selected else {
            return Vec::new();
        };
        let Some(read) = self
            .wardrobe_pack()
            .filter(|p| p.module == *module)
            .and_then(|p| p.read.as_ref().ok())
        else {
            return Vec::new();
        };
        let ids: Vec<String> = (read.wardrobe.garments.iter())
            .filter(|g| work.picked.contains(&g.id))
            .map(|g| g.id.clone())
            .collect();
        match ids.contains(selected) {
            true => ids,
            false => vec![selected.clone()],
        }
    }

    /// The list shows only the garments of `order`: let go of every garment picked that it does not show, so that no command acts on a garment the person cannot see.
    ///
    /// ⚠ Only a pick of several is cut. One garment selected stays selected, listed or not, since the Garment panel names it; a pick whose garments are all hidden is left as the garment selected alone.
    pub fn keep_picks_listed(&mut self, module: &str, order: &[String]) {
        let work = &mut self.workbench.wardrobe;
        if work.picked.len() < 2 {
            return;
        }
        let listed = |id: &String| order.contains(id);
        let kept: Vec<String> = work
            .picked
            .iter()
            .filter(|id| listed(id))
            .cloned()
            .collect();
        let selected_listed = (work.selected.as_ref()).is_some_and(|(_, id)| listed(id));
        let selected = match (selected_listed, kept.last()) {
            (false, Some(last)) => Some((module.to_string(), last.clone())),
            _ => work.selected.clone(),
        };
        work.picked = kept;
        work.selected = selected;
    }

    /// Ctrl and a click: put garment `id` among the picks, or take it out when it is in. The last one in is the garment selected.
    pub fn toggle_garment(&mut self, module: &str, id: String) {
        let mut picked = self.picked_garments();
        match picked.iter().position(|p| *p == id) {
            Some(at) => {
                picked.remove(at);
            }
            None => picked.push(id.clone()),
        }
        let selected = picked.last().cloned();
        self.pick_set(module, picked, selected);
        self.workbench.wardrobe.anchor = Some(id);
    }

    /// Shift and a click: pick the garments from the anchor to `last` in `order`, the ids as the list shows them. The anchor stays where it was.
    pub fn span_garments(&mut self, module: &str, order: &[String], last: String) {
        let work = &self.workbench.wardrobe;
        let anchor = work
            .anchor
            .clone()
            .or_else(|| work.selected.as_ref().map(|(_, id)| id.clone()));
        let ids = between(order, anchor.as_deref(), &last);
        self.pick_set(module, ids, Some(last));
        self.workbench.wardrobe.anchor = anchor;
    }

    /// A click on a heading: pick every garment under it, the first being the one selected.
    pub fn pick_garments(&mut self, module: &str, ids: Vec<String>) {
        let first = ids.first().cloned();
        self.pick_set(module, ids, first.clone());
        self.workbench.wardrobe.anchor = first;
    }

    fn pick_set(&mut self, module: &str, ids: Vec<String>, selected: Option<String>) {
        let work = &mut self.workbench.wardrobe;
        work.picked = ids;
        work.selected = selected.map(|id| (module.to_string(), id));
        work.module = Some(module.to_string());
    }

    /// F2: the Garment panel puts the keyboard in the name of the garment selected.
    pub fn ask_to_rename_garment(&mut self) {
        self.workbench.wardrobe.rename_asked = true;
    }

    /// Open the model of garment `id` where it is edited: the Model workspace.
    pub fn open_garment_model(&mut self, id: &str) {
        let Some(pack) = self.wardrobe_pack() else {
            return;
        };
        let Some(model) = (pack.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(id))
            .and_then(|g| g.model.clone())
        else {
            self.show_info_toast(tr!(
                "This place is held with no model, so there is nothing to open"
            ));
            return;
        };
        let Some(mut path) = in_pack(&pack.folder, &model) else {
            self.show_info_toast(tr!(
                "{model} is outside the pack, so it is not opened",
                model = model
            ));
            return;
        };
        // A model that is a source opens the file its cook makes in the project's shadow folder, cooked now from what the folder holds.
        if path.is_dir() {
            let Some(shadow) = self.project_path.as_deref().map(or_project::shadow_root) else {
                self.show_info_toast(tr!(
                    "{model} is a source, which opens from a project",
                    model = model
                ));
                return;
            };
            match or_source::cook::cook_source(&path, &shadow) {
                Ok(output) => path = output,
                Err(why) => {
                    self.show_error_toast(tr!(
                        "{model} does not cook: {why}",
                        model = model,
                        why = why.join("; ")
                    ));
                    return;
                }
            }
        }
        let Some(kind) = DocumentKind::from_path(&path).filter(|_| path.is_file()) else {
            self.show_info_toast(tr!(
                "{model} is not a file the Model workspace opens",
                model = model
            ));
            return;
        };
        self.active_workspace = Workspace::for_kind(kind);
        self.load_file(path);
    }
}

/// The ids from `from` to `to` in `order`, both included and in either direction; `to` alone where `from` is not in it.
fn between(order: &[String], from: Option<&str>, to: &str) -> Vec<String> {
    let at = |id: &str| order.iter().position(|o| o == id);
    match (from.and_then(at), at(to)) {
        (Some(a), Some(b)) => order[a.min(b)..=a.max(b)].to_vec(),
        _ => vec![to.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{between, in_pack};

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_filter_lets_go_of_the_picks_it_hides_and_leaves_a_single_selection_alone() {
        let mut state = crate::state::AppState::default();
        let pick = |state: &mut crate::state::AppState, picked: &[&str], selected: &str| {
            let work = &mut state.workbench.wardrobe;
            work.picked = ids(picked);
            work.selected = Some(("m".into(), selected.into()));
        };
        let selected = |state: &crate::state::AppState| {
            (state.workbench.wardrobe.selected.clone()).map(|(_, id)| id)
        };
        // b is hidden: the pick is a and c, and c is still the one selected.
        pick(&mut state, &["a", "b", "c"], "c");
        state.keep_picks_listed("m", &ids(&["a", "c"]));
        assert_eq!(state.workbench.wardrobe.picked, ids(&["a", "c"]));
        assert_eq!(selected(&state).as_deref(), Some("c"));
        // The selected garment is hidden too: the last of the garments still picked takes its place.
        pick(&mut state, &["a", "b", "c"], "c");
        state.keep_picks_listed("m", &ids(&["a", "b"]));
        assert_eq!(state.workbench.wardrobe.picked, ids(&["a", "b"]));
        assert_eq!(selected(&state).as_deref(), Some("b"));
        // Every one is hidden: the garment selected stays, alone, and the panel names it.
        pick(&mut state, &["a", "b"], "b");
        state.keep_picks_listed("m", &ids(&["x"]));
        assert!(state.workbench.wardrobe.picked.is_empty());
        assert_eq!(selected(&state).as_deref(), Some("b"));
        // One garment selected is never cut, whatever the list shows.
        pick(&mut state, &["a"], "a");
        state.keep_picks_listed("m", &ids(&["x"]));
        assert_eq!(state.workbench.wardrobe.picked, ids(&["a"]));
    }

    #[test]
    fn a_span_runs_from_the_anchor_to_the_click_in_the_order_the_list_shows() {
        let order = ids(&["a", "b", "c", "d"]);
        assert_eq!(between(&order, Some("b"), "d"), ids(&["b", "c", "d"]));
        assert_eq!(between(&order, Some("d"), "b"), ids(&["b", "c", "d"]));
        assert_eq!(between(&order, Some("c"), "c"), ids(&["c"]));
        // An anchor the list does not show, as a filter may have hidden it, leaves the click alone.
        assert_eq!(between(&order, Some("x"), "c"), ids(&["c"]));
        assert_eq!(between(&order, None, "c"), ids(&["c"]));
    }

    #[test]
    fn a_name_that_leaves_the_pack_is_no_file_of_it() {
        let folder = Path::new("pack");
        for name in [
            "",
            "/model.ydd",
            "../model.ydd",
            "models/../../model.ydd",
            "C:/model.ydd",
            r"C:\model.ydd",
            r"\\host\share\model.ydd",
        ] {
            assert_eq!(in_pack(folder, name), None, "{name:?}");
        }
        assert_eq!(
            in_pack(folder, "models/a.ydd"),
            Some(folder.join("models").join("a.ydd"))
        );
    }
}
