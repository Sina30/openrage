//! The sample project and the tour: a clothing project made of synthetic content a first launch can open, and six steps over the
//! screens that end on the guide.
//!
//! ⚠ Nothing of the game is in it: the models and textures are the synthetic ones the tests use, so the sample can be made on a
//! machine with no install, and it holds one problem on purpose (two jackets that ship the same model) for the tour to point at.

use std::path::{Path, PathBuf};

use or_project::PROJECT_EXTENSION;

use super::{NewProject, Template};
use crate::state::AppState;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;

/// The name of the sample project, its folder and its file.
const NAME: &str = "OpenRage sample";

/// The ped the sample's garments are for.
const PED: &str = "mp_m_freemode_01_mp_m_sample";

/// Where the tour's card stands: a corner of the window that covers nothing the step is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopRight,
    BottomRight,
}

/// One step of the tour: the screen it shows, and what it says about it.
#[derive(Debug, Clone, Copy)]
pub struct TourStep {
    pub title: &'static str,
    pub words: &'static str,
    pub workspace: Workspace,
    /// The panel brought forward on the workspace, none for the workspace as it is.
    pub panel: Option<crate::ui::panel_registry::PanelId>,
    /// The panels of the bottom row are what the steps about them show, so their card stands at the top.
    pub corner: Corner,
}

/// The tour: each step shows a screen and says what it is for in a sentence or two.
pub const TOUR: [TourStep; 6] = [
    TourStep {
        title: tr_key!("Everything the project holds"),
        words: tr_key!("Assets lists every file and source of the project with the state of each. Space looks at one without opening it, and Enter opens it."),
        workspace: Workspace::Project,
        panel: Some(ids::ASSETS),
        corner: Corner::BottomRight,
    },
    TourStep {
        title: tr_key!("What is wrong, and what to do"),
        words: tr_key!("Problems says what is wrong and offers what to do about it. Fix… says exactly what it would change before it changes anything. The sample project has one problem on purpose: two jackets ship the same model."),
        workspace: Workspace::Project,
        panel: Some(ids::PROBLEMS),
        corner: Corner::TopRight,
    },
    TourStep {
        title: tr_key!("The clothing"),
        words: tr_key!("In a project with clothing, the Wardrobe lists the garments of the pack. Drop models and textures on it to add more; OpenRage names them, numbers them and checks them."),
        workspace: Workspace::Wardrobe,
        panel: None,
        corner: Corner::BottomRight,
    },
    TourStep {
        title: tr_key!("Build"),
        words: tr_key!("Build writes the resource and reads every file back. It says that the files reload, and never that the game loads them: nobody can run the game from here."),
        workspace: Workspace::Project,
        panel: Some(ids::BUILD),
        corner: Corner::TopRight,
    },
    TourStep {
        title: tr_key!("Every change is a step"),
        words: tr_key!("History keeps a step for each change of the project. Click one to go back to it, or press Ctrl+Z for the last."),
        workspace: Workspace::Project,
        panel: Some(ids::HISTORY),
        corner: Corner::TopRight,
    },
    TourStep {
        title: tr_key!("The rest is in the guide"),
        words: tr_key!("The guide covers installing on a server, many garments, sharing a project and the keys. It opens when you go on."),
        workspace: Workspace::Project,
        panel: None,
        corner: Corner::BottomRight,
    },
];

/// The bytes of the sample's garments, which two jackets share.
fn garments() -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for n in 0..3 {
        let jacket = if n < 2 {
            tr!("the same jacket")
        } else {
            tr!("another jacket")
        };
        let colour = if n < 2 {
            tr!("the same colour").to_string()
        } else {
            format!("colour {n}")
        };
        files.push((
            format!("{PED}^jbib_{n:03}_u.ydd"),
            or_source::sample::drawables_of(jacket),
        ));
        files.push((
            format!("{PED}^jbib_diff_{n:03}_a_uni.ytd"),
            or_source::sample::textures_of(&colour),
        ));
    }
    files
}

impl AppState {
    /// Where the sample project is kept: beside the preferences, so it is the person's own copy to change.
    pub fn sample_project_root() -> Option<PathBuf> {
        crate::state::prefs::config_root().map(|root| root.join("sample"))
    }

    /// Make the sample project under `root` and give its file; one that is there is the person's now and is left as it is.
    pub fn make_sample_project(root: &Path) -> Result<PathBuf, String> {
        let mut draft = NewProject::new(Template::Clothing, root.to_path_buf());
        draft.set_name(crate::i18n::tr(NAME).into());
        let file = root
            .join(crate::i18n::tr(NAME))
            .join(format!("{NAME}.{PROJECT_EXTENSION}"));
        if file.is_file() {
            return Ok(file);
        }
        std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let created = draft.create()?;

        // The synthetic files are written to the project's private folder and removed, since the drop reads files and keeps sources.
        let scratch = or_project::snapshot::private_dir(&created.file).join("sample-files");
        let result = (|| {
            std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
            let mut dropped = Vec::new();
            for (name, bytes) in garments() {
                let path = scratch.join(name);
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                dropped.push(path);
            }
            let pack = created
                .file
                .parent()
                .map(|p| p.join("clothing"))
                .ok_or(tr!("the sample has no folder"))?;
            or_wardrobe::edit::add_clothes(&pack, &dropped).map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        })();
        let _ = std::fs::remove_dir_all(&scratch);
        result?;
        Ok(created.file)
    }

    /// Open the sample project, making it first where it is not made yet.
    pub fn open_sample_project(&mut self) {
        let Some(root) = Self::sample_project_root() else {
            return self.show_error_toast(tr!(
                "There is no configuration folder to keep the sample in"
            ));
        };
        match Self::make_sample_project(&root) {
            Ok(file) => self.load_project_from(file),
            Err(why) => self.show_error_toast(tr!(
                "The sample project could not be made: {why}",
                why = why
            )),
        }
    }

    /// Begin the tour at its first step, on the project that is open, or on the sample when none is.
    ///
    /// ⚠ The guide is put away: it would sit in the middle of the screen over the panel the step is about.
    pub fn start_tour(&mut self) {
        self.front_door.guide = None;
        if self.project_path.is_none() {
            self.open_sample_project();
            if self.project_path.is_none() {
                return;
            }
        }
        self.front_door.tour = Some(0);
        self.show_tour_step();
    }

    /// Go on to the next step; past the last one the tour ends on the guide.
    pub fn tour_next(&mut self) {
        match self.front_door.tour {
            Some(at) if at + 1 < TOUR.len() => {
                self.front_door.tour = Some(at + 1);
                self.show_tour_step();
            }
            Some(_) => {
                self.front_door.tour = None;
                self.front_door.show_guide("");
            }
            None => {}
        }
    }

    /// Go back a step; the first one stays.
    pub fn tour_back(&mut self) {
        if let Some(at) = self.front_door.tour {
            self.front_door.tour = Some(at.saturating_sub(1));
            self.show_tour_step();
        }
    }

    /// Leave the tour where it is.
    pub fn end_tour(&mut self) {
        self.front_door.tour = None;
    }

    /// Show the screen of the step the tour is at.
    fn show_tour_step(&mut self) {
        let Some(step) = self.front_door.tour.and_then(|at| TOUR.get(at)) else {
            return;
        };
        self.hide_start_page();
        // ⚠ A project without the content of a task screen has no such screen: the step shows the Project screen instead of a screen that is not there.
        let workspace = if self.workspaces_offered().contains(&step.workspace) {
            step.workspace
        } else {
            Workspace::Project
        };
        self.active_workspace = workspace;
        if let Some(panel) = step.panel {
            self.workspace_docks.reveal(workspace, panel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ A step that names a panel its screen no longer docks would show the screen and point at nothing.
    #[test]
    fn every_step_of_the_tour_shows_a_panel_its_screen_docks() {
        let docks = crate::ui::dock::WorkspaceDocks::default();
        for step in TOUR {
            let Some(panel) = step.panel else {
                continue;
            };
            let dock = docks.peek(step.workspace).expect("built at default");
            assert!(
                dock.iter_all_tabs().any(|(_, tab)| tab.id() == panel),
                "{}: `{panel}` is in no tab of {:?}",
                step.title,
                step.workspace
            );
        }
    }
}
