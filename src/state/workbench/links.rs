//! Linking content: a pack another project holds is read into this one, and that project stays its only writer.

use std::path::Path;

use or_project::links::{add_link, add_project_links, project_file_in, remove_link, Link};

use crate::state::front_door::recognisers;
use crate::state::AppState;

impl AppState {
    /// Link the folder `folder` into the open project as content; its id.
    ///
    /// ⚠ Only a folder that holds content OpenRage knows is linked, and one that is the project's own, around it or already linked is refused with a sentence.
    pub fn link_content(&mut self, folder: &Path) -> Result<String, String> {
        if let Some(why) = self.read_only_reason() {
            return Err(why);
        }
        let Some(project) = self.project_path.clone() else {
            return Err(tr!("No project is open").into());
        };
        let kind = recognisers()
            .iter()
            .find(|recogniser| recogniser.holds(folder))
            .map(|recogniser| recogniser.kind())
            .ok_or_else(|| {
                tr!("{folder} holds no content OpenRage knows: a clothing folder holds a wardrobe.json", folder = folder.display())
            })?;
        let id = add_link(
            &mut self.project_carried.container,
            &project,
            kind,
            folder,
            None,
        )?;
        self.workbench.selected_content = Some(id.clone());
        self.workbench.want_scan();
        self.record_step(tr!(
            "Linked {id} from {folder}",
            id = id,
            folder = folder.display()
        ));
        Ok(id)
    }

    /// Link what the folder holds and say what was linked: the content of a project (its Files content and each pack it holds, and none of the links it holds) or one pack.
    pub fn link_folder(&mut self, folder: &Path) -> Result<String, String> {
        if let Some(why) = self.read_only_reason() {
            return Err(why);
        }
        if project_file_in(folder).is_none() {
            let id = self.link_content(folder)?;
            return Ok(tr!("Linked {id}; Save project keeps the link", id = id));
        }
        let Some(project) = self.project_path.clone() else {
            return Err(tr!("No project is open").into());
        };
        let linked = add_project_links(&mut self.project_carried.container, &project, folder)?;
        self.workbench.selected_content = linked.ids.first().cloned();
        self.workbench.want_scan();
        let ids = linked.ids.join(", ");
        self.record_step(tr!(
            "Linked {ids} from {folder}",
            ids = ids,
            folder = folder.display()
        ));
        let mut said = tr!("Linked {ids}", ids = ids);
        if !linked.not_followed.is_empty() {
            said.push_str(&tr!(
                "; that project links {not_followed} itself, and a link is not followed",
                not_followed = linked.not_followed.join(", ")
            ));
        }
        Ok(tr!("{said}; Save project keeps the link", said = said))
    }

    /// Stop linking `id`; the folder and the project that holds it are not touched.
    pub fn unlink_content(&mut self, id: &str) -> Result<(), String> {
        if let Some(why) = self.read_only_reason() {
            return Err(why);
        }
        remove_link(&mut self.project_carried.container, id)?;
        if self.workbench.selected_content.as_deref() == Some(id) {
            self.workbench.selected_content = None;
        }
        self.workbench.want_scan();
        self.record_step(tr!("Unlinked {id}", id = id));
        Ok(())
    }

    /// The link of the content chosen in the Content panel, when it is one.
    pub fn selected_link(&self) -> Option<&Link> {
        self.workbench
            .link(self.workbench.selected_content.as_deref()?)
    }

    /// Open the project that holds linked content in a window of its own, where it is edited.
    pub fn open_linked_source(&mut self, id: &str) {
        let owner = self.workbench.link(id).and_then(|link| link.owner.clone());
        match owner {
            Some(owner) => self.open_project_in_new_window(&owner),
            None => self.show_info_toast(tr!("No project holds this folder, so nothing opens it")),
        }
    }
}
