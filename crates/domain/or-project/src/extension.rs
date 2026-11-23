//! What a script or a plugin reads of a plan and may add to it: the plan as a JSON document, and text files an emitter hands back.
//!
//! Data in and data out. The document is the one `openrage-cli wardrobe plan` prints, so what a
//! plugin sees is what a CI job sees, except that a plugin is not told where on the disk a file is
//! copied from ([`Plan::guest_document`]). An emitted file is checked as every other file of the plan
//! is, and a plan that cannot take it says why in its problems, which stop the build.

use serde_json::{json, Value};

use crate::container::OutputKind;
use crate::paths::{is_component, is_under};
use crate::plan::{ceiling_problems, Origin, Plan, PlannedFile, Totals};

/// The `format` of the document, which the Wardrobe's first plan emitter named and scripts read.
pub const DOCUMENT_FORMAT: &str = "openrage-wardrobe-plan";

/// The `schema` of the document; a key is added without a bump, and one is never removed or renamed.
pub const DOCUMENT_SCHEMA: u32 = 1;

impl Plan {
    /// The plan as the document a script or a plugin reads: its files, totals, budgets, items and problems.
    pub fn document(&self) -> Value {
        let files: Vec<Value> = self
            .files
            .iter()
            .map(|f| {
                let (how, from) = match &f.origin {
                    Origin::Copy { from, .. } => ("copy", Some(from.display().to_string())),
                    Origin::Retarget { from, .. } => ("retarget", Some(from.display().to_string())),
                    Origin::Cook { source, .. } => ("cook", Some(source.display().to_string())),
                    Origin::Document(_) => ("document", None),
                    Origin::Generated(_) => ("generated", None),
                };
                json!({ "path": f.path, "how": how, "bytes": f.bytes(), "from": from })
            })
            .collect();
        let budgets: Vec<Value> = self
            .budgets
            .iter()
            .map(|b| json!({ "name": b.name, "used": b.used, "ceiling": b.ceiling }))
            .collect();
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|i| json!({ "id": i.id, "name": i.name, "digest": i.digest }))
            .collect();
        json!({
            "format": DOCUMENT_FORMAT,
            "schema": DOCUMENT_SCHEMA,
            "output": self.output,
            "resource": self.resource,
            "kind": self.kind.word(),
            "target": self.target.map(|t| t.word()),
            "files": files,
            "totals": { "files": self.totals.files, "bytes": self.totals.bytes },
            "budgets": budgets,
            "items": items,
            "problems": self.problems,
        })
    }

    /// The document a plugin is handed: [`Plan::document`] without where on the user's disk each file is copied from.
    ///
    /// ⚠ A guest has no path, and an emitter writes into a resource the user then publishes, so a folder
    /// of the disk (a user name, a drive) must not cross.
    pub fn guest_document(&self) -> Value {
        let mut document = self.document();
        if let Some(files) = document["files"].as_array_mut() {
            for file in files {
                file["from"] = Value::Null;
            }
        }
        document
    }

    /// This plan with the text files plugin `plugin` emitted, or the problem each one is.
    ///
    /// ⚠ Only a FiveM resource takes them: a singleplayer DLC would have to register each file with a
    /// type, and an `.oiv` names its files in a manifest, and an emitter can state neither. An emitted
    /// file is not listed in the manifest's `files` either, since the plugin cannot edit the manifest;
    /// a script on the server reads it from the resource's folder.
    pub fn with_emitted(&self, plugin: &str, files: &[(String, String)]) -> Plan {
        let mut plan = self.clone();
        if files.is_empty() {
            return plan;
        }
        if plan.kind != OutputKind::FivemResource {
            plan.problems.push(format!(
                "plugin `{plugin}` adds {} file(s), which only a FiveM resource takes: a {} registers each file with a type or names it in a manifest, and a plugin states neither",
                files.len(),
                plan.kind.label()
            ));
            return plan;
        }
        let said = ceiling_problems(&plan);
        for (path, text) in files {
            let refusal = if path.is_empty() || !path.split('/').all(is_component) {
                Some(format!(
                    "plugin `{plugin}` adds `{path}`, which leaves the resource or is no path every host can create"
                ))
            } else if is_under(path, "stream") {
                Some(format!(
                    "plugin `{plugin}` adds `{path}` to stream/, which holds the game's files that content ships"
                ))
            } else {
                let folded = path.to_lowercase();
                plan.files
                    .iter()
                    .find(|f| f.path.to_lowercase() == folded)
                    .map(|first| {
                        format!(
                            "`{}` and `{path}` are one path in the resource (plugin `{plugin}`)",
                            first.path
                        )
                    })
            };
            match refusal {
                Some(problem) => plan.problems.push(problem),
                None => plan.files.push(PlannedFile {
                    path: path.clone(),
                    origin: Origin::Document(text.clone()),
                }),
            }
        }
        plan.files.sort_by(|a, b| a.path.cmp(&b.path));
        plan.totals = Totals {
            files: plan.files.len() as u64,
            bytes: plan.files.iter().map(PlannedFile::bytes).sum(),
        };
        plan.problems.retain(|p| !said.contains(p));
        let ceilings = ceiling_problems(&plan);
        plan.problems.extend(ceilings);
        plan
    }
}
