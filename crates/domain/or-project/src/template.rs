//! Templates: a starting set of content, builds and layout, as a file anyone can share.
//!
//! A template is data and holds no code, no file and no path of a machine. It is applied once: a
//! project made from it records the template's name and nothing else, and the template and the
//! project go separate ways after that.

use serde_json::{json, Map, Value};

use crate::container::{is_name, Container, ModuleKind};
use crate::paths::is_component;

/// The extension of a template file.
pub const EXTENSION: &str = "openrage-template";
pub const FORMAT: &str = "openrage-template";
pub const SCHEMA: u32 = 1;

/// The key of a project file that records the name of the template it was made from.
pub const TEMPLATE_KEY: &str = "template";

/// What a build's name holds in a template where the new project's resource name goes.
pub const RESOURCE: &str = "{resource}";

/// The key of a Files content that names files of its project, which a template has none of.
const FILE_TYPES: &str = "file_types";

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub name: String,
    pub description: String,
    /// The word of the workspace a project made from it opens on; none says the neutral one.
    pub opens_on: Option<String>,
    /// The words of the workspace buttons it shows; none says every one.
    pub shown: Option<Vec<String>>,
    /// The game the project is for, as the project file spells it.
    pub game: Option<String>,
    /// The content and builds; a build's name holds [`RESOURCE`].
    pub container: Container,
}

/// A folder a project made from a template needs for a content of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFolder {
    pub module: String,
    pub kind: ModuleKind,
    /// Inside the project's folder, with `/`; `.` is the project's folder itself.
    pub path: String,
}

/// A template applied to a name.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub container: Container,
    pub folders: Vec<ModuleFolder>,
}

impl Template {
    /// The template of a project, and one sentence for each thing it left out.
    ///
    /// ⚠ A link leads to a place on one machine and a Files content's `file_types` name files of
    /// one project, so neither is carried. The first build is named for the resource and the others
    /// for the resource and their own id, since a name that was one project's would be every one's.
    pub fn from_project(
        name: &str,
        description: &str,
        container: &Container,
        opens_on: Option<&str>,
        shown: Option<&[String]>,
        game: Option<&str>,
    ) -> (Self, Vec<String>) {
        let mut left_out = Vec::new();
        let mut modules = Vec::new();
        for module in &container.modules {
            if module.is_link() {
                left_out.push(format!(
                    "content `{}` is a link to another place, which a template does not carry",
                    module.id
                ));
                continue;
            }
            let mut module = module.clone();
            if module.extra.remove(FILE_TYPES).is_some() {
                left_out.push(format!(
                    "the file types of content `{}` name files of this project, which a template does not carry",
                    module.id
                ));
            }
            modules.push(module);
        }
        let kept = |id: &String| modules.iter().any(|m| &m.id == id);
        let outputs = container
            .outputs
            .iter()
            .enumerate()
            .map(|(n, output)| {
                let mut output = output.clone();
                output.name = match n {
                    0 => RESOURCE.to_string(),
                    _ => format!("{RESOURCE}_{}", output.id),
                };
                output.take.retain(kept);
                output
            })
            .collect();
        let template = Self {
            name: name.to_string(),
            description: description.to_string(),
            opens_on: opens_on.map(str::to_string),
            shown: shown.map(<[String]>::to_vec),
            game: game.map(str::to_string),
            container: Container { modules, outputs },
        };
        (template, left_out)
    }

    /// The template a file holds, and one sentence for each thing in it that was left out.
    ///
    /// ⚠ A template is somebody else's file: a content whose folder is not a plain folder inside the
    /// project, or that is a link, is left out, so applying a template never writes outside the
    /// project's folder.
    pub fn from_json(bytes: &[u8]) -> Result<(Self, Vec<String>), String> {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let Ok(Value::Object(top)) = serde_json::from_slice::<Value>(bytes) else {
            return Err("this is not a template: a template is a JSON object".into());
        };
        match top.get("format").and_then(Value::as_str) {
            Some(FORMAT) => {}
            other => {
                return Err(format!(
                    "this is not a template: its format is {}, not `{FORMAT}`",
                    other.map_or("missing".to_string(), |f| format!("`{f}`"))
                ))
            }
        }
        if let Some(newer) = top
            .get("schema")
            .and_then(Value::as_u64)
            .filter(|n| *n > u64::from(SCHEMA))
        {
            return Err(format!(
                "this template was written by a newer OpenRage (schema {newer}) than this one reads (schema {SCHEMA})"
            ));
        }
        let name = match top.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => name.to_string(),
            _ => return Err("this template has no name".into()),
        };
        let text = |key: &str| top.get(key).and_then(Value::as_str).map(str::to_string);
        let shown = top
            .get("shown_workspaces")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_string))
                    .collect()
            });

        let mut findings = Vec::new();
        let mut container = Container::read(&top, &mut findings);
        container.modules.retain(|module| {
            let why = if module.is_link() {
                Some("is a link, which a template does not carry")
            } else if module.path().is_some_and(|path| !is_inside(path)) {
                Some("has a folder that is not a plain folder inside the project")
            } else {
                None
            };
            if let Some(why) = why {
                findings.push(format!("content `{}` {why}; left out", module.id));
            }
            why.is_none()
        });
        let kept: Vec<String> = container.modules.iter().map(|m| m.id.clone()).collect();
        for output in &mut container.outputs {
            output.take.retain(|id| kept.contains(id));
        }
        let template = Self {
            name,
            description: text("description").unwrap_or_default(),
            opens_on: text("opens_on"),
            shown,
            game: text("game"),
            container,
        };
        Ok((template, findings))
    }

    /// The canonical text of the file: keys sorted, two-space indent, LF.
    pub fn to_json(&self) -> String {
        let mut top = Map::new();
        top.insert("format".into(), json!(FORMAT));
        top.insert("schema".into(), json!(SCHEMA));
        top.insert("name".into(), json!(self.name));
        top.insert("description".into(), json!(self.description));
        if let Some(opens_on) = &self.opens_on {
            top.insert("opens_on".into(), json!(opens_on));
        }
        if let Some(shown) = &self.shown {
            top.insert("shown_workspaces".into(), json!(shown));
        }
        if let Some(game) = &self.game {
            top.insert("game".into(), json!(game));
        }
        self.container.write(&mut top);
        let mut text =
            serde_json::to_string_pretty(&Value::Object(top)).expect("a JSON value serialises");
        text.push('\n');
        text
    }

    /// The keys a project made from this template carries beside its content and builds: where it came from, and its layout.
    pub fn project_keys(&self) -> Map<String, Value> {
        let mut keys = Map::new();
        keys.insert(TEMPLATE_KEY.into(), json!(self.name));
        if let Some(game) = &self.game {
            keys.insert("game".into(), json!(game));
        }
        if let Some(opens_on) = &self.opens_on {
            keys.insert("workspace".into(), json!(opens_on));
        }
        if let Some(shown) = &self.shown {
            keys.insert("shown_workspaces".into(), json!(shown));
        }
        keys
    }

    /// The project this makes for a resource called `resource`.
    pub fn instantiate(&self, resource: &str) -> Result<Instance, String> {
        if !is_name(resource) {
            return Err(format!(
                "`{resource}` cannot name a folder on every machine: no slash, colon or other reserved character, no leading dot and no trailing dot or space"
            ));
        }
        let mut container = self.container.clone();
        for output in &mut container.outputs {
            output.name = output.name.replace(RESOURCE, resource);
            if !is_name(&output.name) {
                return Err(format!(
                    "build `{}` would be named `{}`, which is not a folder name",
                    output.id, output.name
                ));
            }
        }
        let folders = container
            .modules
            .iter()
            .filter(|module| {
                matches!(
                    module.kind,
                    ModuleKind::Clothing
                        | ModuleKind::AddonPed
                        | ModuleKind::Weapons
                        | ModuleKind::Particles
                        | ModuleKind::Maps
                )
            })
            .filter_map(|module| {
                Some(ModuleFolder {
                    module: module.id.clone(),
                    kind: module.kind.clone(),
                    path: module.path()?.to_string(),
                })
            })
            .collect();
        Ok(Instance { container, folders })
    }
}

/// The resource name a project name suggests: lower case letters and digits, with `_` between words.
pub fn resource_name_for(name: &str) -> String {
    let mut resource = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            resource.push(c.to_ascii_lowercase());
        } else if !resource.ends_with('_') && !resource.is_empty() {
            resource.push('_');
        }
    }
    let resource = resource.trim_end_matches('_');
    if resource.is_empty() {
        "resource".to_string()
    } else {
        resource.to_string()
    }
}

/// Whether `path` is `.` or plain folders inside the project, with `/` between them.
fn is_inside(path: &str) -> bool {
    path == "." || (!path.is_empty() && path.split('/').all(is_component))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Module, Output, Target};

    fn police() -> Container {
        let mut police =
            Output::fivem_resource("police", "police_pack", &["files", "uniforms", "logos"]);
        police.max_files = Some(500);
        police.split = true;
        police.target = Some(Target::Enhanced);
        police.limits.insert("clothing_drawables".into(), 255);
        let lite = Output::of(
            crate::OutputKind::SingleplayerDlc,
            "lite",
            "police_lite",
            &["uniforms"],
        );
        let mut files = Module::files("files");
        files.extra.insert(
            "file_types".into(),
            json!({ "data/handling.meta": "HANDLING_FILE" }),
        );
        Container {
            modules: vec![
                files,
                Module::clothing("uniforms", "modules/uniforms"),
                Module::linked("logos", ModuleKind::Clothing, "../brand/logos"),
            ],
            outputs: vec![police, lite],
        }
    }

    #[test]
    fn a_template_made_by_one_project_creates_the_same_project_in_another() {
        let (template, left_out) = Template::from_project(
            "Police pack",
            "Uniforms and a resource for them",
            &police(),
            Some("Project"),
            Some(&["Project".to_string(), "Texture".to_string()]),
            Some("both"),
        );
        assert_eq!(left_out.len(), 2, "{left_out:?}");

        let text = template.to_json();
        let (read, findings) = Template::from_json(text.as_bytes()).expect("it reads");
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(read, template);
        assert_eq!(read.to_json(), text, "a template is canonical text");

        let made = read.instantiate("harbor_pack").expect("a name fills it");
        let ids: Vec<&str> = made
            .container
            .modules
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, ["files", "uniforms"], "a link is not carried");
        assert!(made.container.modules[0].extra.get("file_types").is_none());
        assert_eq!(
            made.folders,
            vec![ModuleFolder {
                module: "uniforms".into(),
                kind: ModuleKind::Clothing,
                path: "modules/uniforms".into()
            }]
        );
        let names: Vec<&str> = made
            .container
            .outputs
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(names, ["harbor_pack", "harbor_pack_lite"]);
        let first = &made.container.outputs[0];
        assert_eq!(first.take, ["files", "uniforms"]);
        assert_eq!(
            (first.max_files, first.split, first.target),
            (Some(500), true, Some(Target::Enhanced))
        );
        assert_eq!(first.limits["clothing_drawables"], 255);
        assert_eq!(
            made.container.outputs[1].kind,
            crate::OutputKind::SingleplayerDlc
        );
        assert_eq!(read.opens_on.as_deref(), Some("Project"));
        assert_eq!(read.game.as_deref(), Some("both"));
    }

    #[test]
    fn a_resource_name_must_name_a_folder() {
        let (template, _) = Template::from_project("t", "", &police(), None, None, None);
        assert!(
            template.instantiate("harbor_pack").is_ok(),
            "a folder name fills it"
        );
        for bad in ["", "a/b", ".hidden", "trailing."] {
            assert!(template.instantiate(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_template_from_somebody_else_cannot_reach_outside_the_project() {
        let text = json!({
            "format": FORMAT, "schema": SCHEMA, "name": "hostile",
            "modules": [
                { "id": "up", "type": "clothing", "path": "../elsewhere" },
                { "id": "abs", "type": "clothing", "path": "/etc" },
                { "id": "drive", "type": "clothing", "path": "C:/Windows" },
                { "id": "ok", "type": "clothing", "path": "modules/ok" },
                { "id": "linked", "type": "clothing", "path": "modules/x", "link": true },
            ],
            "outputs": [{ "id": "fivem", "kind": "fivem-resource", "name": "{resource}",
                          "take": ["up", "ok"] }],
        })
        .to_string();
        let (template, findings) = Template::from_json(text.as_bytes()).expect("it reads");
        let ids: Vec<&str> = template
            .container
            .modules
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, ["ok"]);
        assert_eq!(findings.len(), 4, "{findings:#?}");
        let made = template.instantiate("pack").unwrap();
        assert_eq!(made.container.outputs[0].take, ["ok"]);
    }

    #[test]
    fn what_is_not_a_template_or_is_newer_than_this_build_is_refused() {
        let cases = [
            ("not json", "not a template".to_string()),
            ("[]", "not a template".to_string()),
            (
                r#"{"format":"something-else","schema":1,"name":"x"}"#,
                "format".to_string(),
            ),
            (
                &format!(
                    r#"{{"format":"{FORMAT}","schema":{},"name":"x"}}"#,
                    SCHEMA + 1
                ),
                "newer".to_string(),
            ),
            (
                &format!(r#"{{"format":"{FORMAT}","schema":1}}"#),
                "name".to_string(),
            ),
        ];
        for (text, says) in cases {
            let why = Template::from_json(text.as_bytes()).expect_err(text);
            assert!(why.to_lowercase().contains(&says), "{text}: {why}");
        }
    }
}
