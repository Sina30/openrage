//! Applying a template file to the project that is open: the content and builds it states that the project lacks, as one step of History.

use std::path::Path;

use or_project::project_folder;

use crate::state::AppState;

impl AppState {
    /// Add what the template in `file` states and the project lacks: each content whose id the project does not hold, and each build likewise, named so no two write one folder.
    ///
    /// ⚠ A template adds and never replaces: a content or a build the project already holds by id is left as it is, and a template that adds nothing is refused.
    pub fn apply_template(&mut self, file: &Path) -> Result<String, String> {
        if let Some(why) = self.read_only_reason() {
            return Err(why);
        }
        let project = self.project_path.clone().ok_or(tr!("No project is open"))?;
        let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut container = self.project_carried.container.clone();
        let applied = or_content::template::apply_template(
            &mut container,
            &self.project_name().unwrap_or_default(),
            &project_folder(&project),
            &bytes,
            &self.pack_names(&project),
        )?;
        self.project_carried.container = container;
        self.workbench.want_scan();
        self.workbench.findings_stale = true;
        self.record_step(tr!(
            "Applied the template {template}",
            template = applied.template
        ));
        Ok(applied.sentence())
    }
}
