//! What a kind of content that lives in a folder of the project offers a project, and how a project asks them all.
//!
//! The Files content is the project's own and goes through [`crate::plan::files_contributions`].
//! Every other kind (clothing, addon peds) is a [`Content`]: it recognises its folder when one is
//! adopted ([`Recogniser`]), hands a plan what a module of its kind holds, and starts an empty folder
//! for a new module. Which kinds a build knows is not decided here, since a kind depends on this
//! crate and not the other way round: the caller hands its list in.

use std::path::{Path, PathBuf};

pub mod check;
pub mod facts;
pub mod folder;
pub mod rule;

use crate::adopt::Recogniser;
use crate::container::{Container, Module, ModuleKind, Output};
use crate::contribution::{Contribution, SourceFolder};
use crate::paths::resolved;
use crate::plan::{files_contributions_with, NoSuchOutput, Origin, Plan, Source};
use crate::project::Project;
use crate::split::plans_with;

/// A kind of content with a folder of its own.
pub trait Content: Recogniser {
    /// What `module`, whose folder is in or linked from `project_folder`, hands to a plan; a module whose folder cannot be read hands over the reason.
    fn contribution(&self, project_folder: &Path, module: &Module) -> Contribution;

    /// What `module` hands to `output`, whose kind and options may shape the files it writes; the default is what it hands to any build.
    fn contribution_for(
        &self,
        project_folder: &Path,
        module: &Module,
        _output: &Output,
    ) -> Contribution {
        self.contribution(project_folder, module)
    }

    /// Make the folder of `module` ready to hold content called `name`, keeping what is there.
    fn start(&self, project_folder: &Path, module: &Module, name: &str) -> Result<(), String>;
}

/// The kind in `kinds` that knows `module`.
fn knowing<'a>(kinds: &[&'a dyn Content], module: &Module) -> Option<&'a dyn Content> {
    kinds
        .iter()
        .copied()
        .find(|kind| kind.kind() == module.kind)
}

/// What every module of `container` that a kind in `kinds` knows hands over, except the modules `made` already covers.
///
/// ⚠ All of them, whatever an output takes: the names the game resolves once are one namespace for the whole project.
pub fn contributions(
    kinds: &[&dyn Content],
    container: &Container,
    project_folder: &Path,
    made: &[Contribution],
) -> Vec<Contribution> {
    contributions_to(kinds, container, project_folder, made, None)
}

/// [`contributions`] for the build `output`, which shapes what a kind of content hands over; none asks for what any build gets.
pub fn contributions_to(
    kinds: &[&dyn Content],
    container: &Container,
    project_folder: &Path,
    made: &[Contribution],
    output: Option<&Output>,
) -> Vec<Contribution> {
    container
        .modules
        .iter()
        .filter(|module| !made.iter().any(|c| c.module == module.id))
        .filter_map(|module| {
            let kind = knowing(kinds, module)?;
            Some(match output {
                Some(output) => kind.contribution_for(project_folder, module, output),
                None => kind.contribution(project_folder, module),
            })
        })
        .collect()
}

/// The plans of `output` of a project given as its parts: the Files modules' contributions, the
/// contributions in `made` (which a screen that planned a module already has), and those of every other module a kind in `kinds` knows.
/// `folders` are the `.src` folders of the project, which the first Files module holds and a build cooks.
pub fn plans_given(
    kinds: &[&dyn Content],
    container: &Container,
    output: &str,
    folder: &Path,
    held: &[Source],
    folders: &[SourceFolder],
    made: Vec<Contribution>,
) -> Result<Vec<Plan>, NoSuchOutput> {
    let kept: Vec<SourceFolder> = folders
        .iter()
        .filter(|f| content_owning(container, folder, &f.folder).is_none())
        .cloned()
        .collect();
    let mut handed = files_contributions_with(container, output, folder, held, &kept)?;
    let files = handed.len();
    handed.extend(contributions_to(
        kinds,
        container,
        folder,
        &made,
        container.output(output),
    ));
    handed.extend(made);
    leave_to_owner(&mut handed, files);
    plans_with(container, output, folder, &handed)
}

/// The content of a kind other than Files whose folder holds `source`, which owns what that folder holds.
///
/// ⚠ A content cooks the sources its own data names, so a source its folder holds that nothing names is not shipped: the Files
/// content, which ships every other `.src` folder of the project, does not take it. A content whose folder is the project's own
/// owns nothing in particular, and a link is read where it leads.
pub fn content_owning<'a>(
    container: &'a Container,
    project: &Path,
    source: &Path,
) -> Option<&'a Module> {
    let inside = resolved(source);
    container
        .modules
        .iter()
        .filter(|m| m.kind != ModuleKind::Files && !m.is_link())
        .find(|m| {
            m.path().filter(|path| *path != ".").is_some_and(|path| {
                let content =
                    resolved(&project.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)));
                inside.starts_with(&content) && inside != content
            })
        })
}

/// A `.src` folder another content cooks is that content's: the Files content that found it among the project's folders leaves it out, so one source is not cooked twice.
///
/// `files` is how many of the first contributions are the Files modules'. A garment that names a drawables source of its pack is such a source.
fn leave_to_owner(handed: &mut [Contribution], files: usize) {
    let (theirs, others) = handed.split_at_mut(files);
    let owned: Vec<PathBuf> = (others.iter())
        .flat_map(|c| &c.files)
        .filter_map(|f| match &f.origin {
            Origin::Cook { source, .. } => Some(resolved(source)),
            _ => None,
        })
        .collect();
    if owned.is_empty() {
        return;
    }
    for contribution in theirs {
        contribution.files.retain(|f| match &f.origin {
            Origin::Cook { source, .. } => !owned.contains(&resolved(source)),
            _ => true,
        });
    }
}

/// The same with nothing made yet.
pub fn plans_from(
    kinds: &[&dyn Content],
    container: &Container,
    output: &str,
    folder: &Path,
    held: &[Source],
    folders: &[SourceFolder],
) -> Result<Vec<Plan>, NoSuchOutput> {
    plans_given(kinds, container, output, folder, held, folders, Vec::new())
}

/// The plans of `output` of `project`.
pub fn plans(
    kinds: &[&dyn Content],
    project: &Project,
    output: &str,
    folders: &[SourceFolder],
) -> Result<Vec<Plan>, NoSuchOutput> {
    plans_from(
        kinds,
        &project.container,
        output,
        &project.folder(),
        &project.sources(),
        folders,
    )
}

/// Make the folder of `module` ready for the kind that knows it, or say that none does.
pub fn start_module(
    kinds: &[&dyn Content],
    project_folder: &Path,
    module: &Module,
    name: &str,
) -> Result<(), String> {
    match knowing(kinds, module) {
        Some(kind) => kind.start(project_folder, module, name),
        None => Err(format!(
            "content `{}` is of a kind ({}) this build has no folder to start for",
            module.id,
            module.kind.label()
        )),
    }
}
