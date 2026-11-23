//! Plugins that extend a build: they run over each plan the project scan and a build make.
//!
//! A validator's findings join the Problems list, an error among them stops the build as a problem
//! of the plan does, and an emitter's files join the plan, which checks them as it checks every
//! other file. A validator granted `cap.read_pack` is also handed the clothing the build takes as
//! data, and a finding it states with a fix carries that fix to the Problems list, where Fix… applies it.
//! A plugin that is refused or stops costs itself and never the plan: it is said as a
//! warning of the build.
//!
//! ⚠ Runs where the plan is made, on a worker, so a slow plugin delays a scan and never a frame.
//! The wasm is read there too.

use std::path::{Path, PathBuf};

use or_plugin::{extension, Grants, Level, Limits, Status, SuggestedFix};
use or_project::{Container, ModuleKind, Plan};
use serde_json::{json, Value};

use super::InstalledPlugin;
use crate::state::AppState;

/// An installed plugin the user has answered yes for, that extends a build.
#[derive(Debug, Clone)]
pub struct Extension {
    pub id: String,
    pub name: String,
    wasm: PathBuf,
    grants: Grants,
    validates: bool,
    emits: bool,
    /// Whether it asked to read the pack, which is then built and handed over with each plan.
    pub reads_pack: bool,
    limits: Limits,
    /// The codes the project lets stand (`plugin.<id>.<code>`), whose findings are no problem of a plan.
    waived: Vec<String>,
}

impl Extension {
    /// `plugin` as an extension, when its manifest extends a build and it is not refused.
    fn of(plugin: &InstalledPlugin, limits: Limits) -> Option<Self> {
        let manifest = &plugin.manifest;
        (plugin.refused.is_none() && manifest.extends_a_build()).then(|| Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            wasm: plugin.wasm.clone(),
            grants: manifest.requested(),
            validates: manifest.validates(),
            emits: manifest.emits(),
            reads_pack: manifest.wants.contains(&or_plugin::Capability::ReadPack),
            limits,
            waived: Vec::new(),
        })
    }
}

/// One thing a plugin said about a plan, or the reason it could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub plugin: String,
    /// The plugin's name, which a question about its fix puts first.
    pub name: String,
    pub output: String,
    pub resource: String,
    pub level: Level,
    pub text: String,
    /// What the plugin suggests doing about it, which changes nothing until a person accepts it.
    pub fix: Option<SuggestedFix>,
}

/// A plan after the plugins, and what they said.
#[derive(Debug, Clone)]
pub struct Extended {
    pub plan: Plan,
    pub said: Vec<Said>,
}

/// The bytes of `plan` a guest is handed.
fn handed(plan: &Plan) -> Vec<u8> {
    serde_json::to_vec(&plan.guest_document()).expect("a JSON value serialises")
}

/// The clothing contents build `output` takes, as the JSON document a validator granted `cap.read_pack` reads.
///
/// ⚠ Each content's `wardrobe.json` as the file holds it, with the content's id and its folder by its path in the
/// project, and no path of the user's disk. A content that is a link is another project's to change, so it is not handed over.
pub fn pack_document(container: &Container, folder: &Path, output: &str) -> Vec<u8> {
    let take = (container.output(output)).map_or_else(Vec::new, |declared| declared.take.clone());
    let packs: Vec<Value> = (container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Clothing && !m.is_link() && take.contains(&m.id))
        .filter_map(|module| {
            let at = or_wardrobe::module_folder(folder, module).ok()?;
            let wardrobe = or_wardrobe::Wardrobe::read(&at).ok()?;
            let inside = or_project::relative_inside(&at, folder)?;
            let wardrobe: Value = serde_json::from_str(&wardrobe.to_json()).ok()?;
            Some(json!({ "content": module.id, "folder": inside, "wardrobe": wardrobe }))
        })
        .collect();
    serde_json::to_vec(&json!({ "format": "openrage-pack", "schema": 1, "packs": packs }))
        .expect("a JSON value serialises")
}

/// `plan` after every extension has read it, in the order of their ids, and what each said.
///
/// A validator's error is also a problem of the plan, so a build is refused; its warnings are only said.
/// `pack` is [`pack_document`], which only an extension that asked for the pack is handed.
pub fn extend(extensions: &[Extension], plan: Plan, pack: &[u8]) -> Extended {
    let mut plan = plan;
    let mut said = Vec::new();
    for extension in extensions {
        let wasm = match std::fs::read(&extension.wasm) {
            Ok(wasm) => wasm,
            Err(e) => {
                said.push(unable(
                    extension,
                    &plan,
                    format!("{}: {e}", extension.wasm.display()),
                ));
                continue;
            }
        };
        let document = handed(&plan);
        if extension.validates {
            let pack = if extension.reads_pack { pack } else { &[] };
            let ran = extension::validate_with_pack(
                &wasm,
                &extension.grants,
                extension.limits,
                &document,
                pack,
            );
            match ran {
                Ok(found) => {
                    for finding in found.findings {
                        let code = format!("plugin.{}.{}", extension.id, finding.code);
                        let text = format!("{code}: {}", finding.message);
                        // ⚠ A waived finding is not said at all: it is no problem of the plan and no row of any list.
                        if extension.waived.contains(&code) {
                            continue;
                        }
                        if finding.level == Level::Error {
                            plan.problems.push(text.clone());
                        }
                        said.push(Said {
                            plugin: extension.id.clone(),
                            name: extension.name.clone(),
                            output: plan.output.clone(),
                            resource: plan.resource.clone(),
                            level: finding.level,
                            text,
                            fix: finding.fix,
                        });
                    }
                    said.extend(refusals(extension, &plan, &found.failures));
                }
                Err(e) => said.push(unable(extension, &plan, e.to_string())),
            }
        }
        if extension.emits {
            match extension::emit(&wasm, &extension.grants, extension.limits, &document) {
                Ok(found) => {
                    let files: Vec<(String, String)> =
                        found.files.into_iter().map(|f| (f.path, f.text)).collect();
                    plan = plan.with_emitted(&extension.id, &files);
                    said.extend(refusals(extension, &plan, &found.failures));
                }
                Err(e) => said.push(unable(extension, &plan, e.to_string())),
            }
        }
    }
    Extended { plan, said }
}

fn unable(extension: &Extension, plan: &Plan, why: String) -> Said {
    Said {
        plugin: extension.id.clone(),
        name: extension.name.clone(),
        output: plan.output.clone(),
        resource: plan.resource.clone(),
        level: Level::Warning,
        text: format!(
            "plugin {} ({}) could not run over this plan: {why}",
            extension.id, extension.name
        ),
        fix: None,
    }
}

/// The host calls the plugin made that were refused, each with the sentence the host kept; a retry and an absent child are not faults.
fn refusals(extension: &Extension, plan: &Plan, failures: &[or_plugin::Failure]) -> Vec<Said> {
    failures
        .iter()
        .filter(|f| !matches!(f.status, Status::BufferTooSmall | Status::NullPointer))
        .map(|f| Said {
            plugin: extension.id.clone(),
            name: extension.name.clone(),
            output: plan.output.clone(),
            resource: plan.resource.clone(),
            level: Level::Warning,
            text: format!(
                "plugin {}: {} was refused: {}",
                extension.id,
                f.function,
                f.because.as_deref().unwrap_or(f.status.means())
            ),
            fix: None,
        })
        .collect()
}

impl AppState {
    /// The extensions to run over a plan: every installed plugin that extends a build and has been answered yes.
    ///
    /// ⚠ From the installed list this session scanned once, so asking for it at every scan reads no folder.
    pub fn extensions(&mut self) -> Vec<Extension> {
        let installed = self.installed_plugins().to_vec();
        let waived: Vec<String> = self.plugin_waivers().into_iter().map(|w| w.code).collect();
        let mut found: Vec<Extension> = installed
            .iter()
            .filter(|plugin| self.is_consented(plugin))
            .filter_map(|plugin| Extension::of(plugin, self.plugins.limits))
            .map(|extension| Extension {
                waived: waived.clone(),
                ..extension
            })
            .collect();
        found.sort_by(|a, b| a.id.cmp(&b.id));
        found
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use or_project::{plan_with, Container, Contribution, Module, Origin, Output, PlannedFile};

    use super::handed;

    #[test]
    fn the_plan_a_plugin_is_handed_names_no_folder_of_the_users_disk() {
        let folder = Path::new("/home/someone/openrage/pack");
        let container = Container {
            modules: vec![Module::files("files")],
            outputs: vec![Output::fivem_resource("fivem", "pack", &["files"])],
        };
        let mut contribution = Contribution::of("files");
        contribution.files = vec![PlannedFile {
            path: "stream/a.ydd".into(),
            origin: Origin::Copy {
                from: folder.join("a.ydd"),
                bytes: 3,
            },
        }];
        let plan =
            plan_with(&container, "fivem", folder, &[contribution]).expect("the output exists");
        assert!(
            serde_json::to_string(&plan.document())
                .unwrap()
                .contains("someone"),
            "the command line prints where a file comes from"
        );

        let text = String::from_utf8(handed(&plan)).expect("a document is text");
        assert!(!text.contains("someone"), "a guest has no path: {text}");
        let document: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            document["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["path"] == "stream/a.ydd" && f["bytes"] == 3),
            "it still lists the file and its size: {text}"
        );
    }
}
