//! Applying a template file to a project that exists: the content and builds it states that the project lacks.
//!
//! The editor's Apply a template… and `openrage-cli project apply-template` do it with this, so a template makes the same project either way.

use std::collections::BTreeSet;
use std::path::Path;

use or_project::template::{resource_name_for, Template};
use or_project::Container;

/// `name`, else `name_2`, `name_3` and on, the first of `taken` does not hold.
fn free_name(name: &str, taken: &BTreeSet<String>) -> String {
    let mut candidate = name.to_string();
    let mut n = 1;
    while taken.contains(&candidate) {
        n += 1;
        candidate = format!("{name}_{n}");
    }
    candidate
}

/// `wanted`, else `wanted_2`, `wanted_3` and on, the first no pack of `taken` has.
pub fn free_pack_name(wanted: &str, taken: &[String]) -> String {
    let mut name = wanted.to_string();
    let mut n = 1;
    while taken.contains(&name) {
        n += 1;
        name = format!("{wanted}_{n}");
    }
    name
}

/// What applying a template added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub template: String,
    /// The ids of the content it added.
    pub contents: Vec<String>,
    /// The ids of the builds it added.
    pub builds: Vec<String>,
    /// What the template could not carry, one sentence each.
    pub left_out: Vec<String>,
}

impl Applied {
    /// One sentence for a toast or a line of output.
    pub fn sentence(&self) -> String {
        let mut said = format!(
            "Applied the template {}: {} added",
            self.template,
            [
                (!self.contents.is_empty())
                    .then(|| format!("content {}", self.contents.join(", "))),
                (!self.builds.is_empty()).then(|| format!("build {}", self.builds.join(", "))),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" and ")
        );
        if !self.left_out.is_empty() {
            said.push_str(&format!("; left out: {}", self.left_out.join("; ")));
        }
        said
    }
}

/// Add to `container` what the template in `bytes` states and the project lacks: each content whose id the project does not hold, and each build likewise, named so no two write one folder.
///
/// `project_name` names the resource, `folder` is the project's, and `pack_names` are the names the clothing packs the project holds already have.
///
/// ⚠ A template adds and never replaces: a content or a build the project already holds by id is left as it is, and a template that adds nothing is refused.
pub fn apply_template(
    container: &mut Container,
    project_name: &str,
    folder: &Path,
    bytes: &[u8],
    pack_names: &[String],
) -> Result<Applied, String> {
    let (template, left_out) = Template::from_json(bytes)?;
    let resource = resource_name_for(project_name);
    let instance = template.instantiate(&resource)?;
    let pack = free_pack_name(&or_wardrobe::names::identifier_for(&resource), pack_names);

    let mut contents = Vec::new();
    for module in &instance.container.modules {
        if container.module(&module.id).is_some() {
            continue;
        }
        crate::start_module(folder, module, &pack)?;
        contents.push(module.id.clone());
        container.modules.push(module.clone());
    }
    let mut names: BTreeSet<String> = container.outputs.iter().map(|o| o.name.clone()).collect();
    let mut builds = Vec::new();
    for output in &instance.container.outputs {
        if container.output(&output.id).is_some() {
            continue;
        }
        let mut output = output.clone();
        output.take.retain(|id| container.module(id).is_some());
        let name = if names.contains(&output.name) {
            free_name(&format!("{}_{}", output.name, output.id), &names)
        } else {
            output.name.clone()
        };
        output.name = name.clone();
        names.insert(name);
        builds.push(output.id.clone());
        container.outputs.push(output);
    }
    if contents.is_empty() && builds.is_empty() {
        return Err(format!(
            "The project already holds everything the template {} states",
            template.name
        ));
    }
    Ok(Applied {
        template: template.name,
        contents,
        builds,
        left_out,
    })
}
