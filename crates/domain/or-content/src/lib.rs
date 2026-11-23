//! The kinds of content this build knows, listed once.
//!
//! A kind of content depends on `or-project` and not the other way round, so something above them
//! has to say which kinds there are. That is this crate and nothing else: [`kinds`] is the list, and
//! the functions here are `or_project::content`'s with it filled in, so the editor and the command line
//! plan, adopt and start a project's content with one list and cannot disagree about it.
//!
//! Adding a kind is a `Content` impl in its own crate and one line in [`kinds`].

pub mod sharing;
pub mod template;

use std::path::Path;

use or_project::adopt::Recogniser;
use or_project::content::{self, Content};
use or_project::{
    Container, Contribution, Module, NoSuchOutput, Plan, Project, Source, SourceFolder,
};

/// Every kind of content with a folder, in the order they are asked.
pub fn kinds() -> [&'static dyn Content; 5] {
    [
        &or_wardrobe::adopt::Clothing,
        &or_addonped::AddonPed,
        &or_weapons::WeaponPack,
        &or_particles::ParticlePack,
        &or_maps::MapPack,
    ]
}

/// The same kinds as recognisers, which an adoption and a link ask whether a folder is one.
pub fn recognisers() -> [&'static dyn Recogniser; 5] {
    [
        &or_wardrobe::adopt::Clothing,
        &or_addonped::AddonPed,
        &or_weapons::WeaponPack,
        &or_particles::ParticlePack,
        &or_maps::MapPack,
    ]
}

/// What every module of `container` that a kind knows hands over, except the modules `made` covers.
pub fn contributions(
    container: &Container,
    project_folder: &Path,
    made: &[Contribution],
) -> Vec<Contribution> {
    content::contributions(&kinds(), container, project_folder, made)
}

/// The plans of `output` of a project given as its parts, with the contributions in `made` already
/// made by a screen that planned those modules.
pub fn plans_given(
    container: &Container,
    output: &str,
    folder: &Path,
    held: &[Source],
    folders: &[SourceFolder],
    made: Vec<Contribution>,
) -> Result<Vec<Plan>, NoSuchOutput> {
    content::plans_given(&kinds(), container, output, folder, held, folders, made)
}

/// The same with nothing made yet.
pub fn plans_from(
    container: &Container,
    output: &str,
    folder: &Path,
    held: &[Source],
    folders: &[SourceFolder],
) -> Result<Vec<Plan>, NoSuchOutput> {
    content::plans_from(&kinds(), container, output, folder, held, folders)
}

/// The plans of `output` of `project`.
pub fn plans(
    project: &Project,
    output: &str,
    folders: &[SourceFolder],
) -> Result<Vec<Plan>, NoSuchOutput> {
    content::plans(&kinds(), project, output, folders)
}

/// Make the folder of `module` ready for the kind that knows it, holding content called `name`.
pub fn start_module(project_folder: &Path, module: &Module, name: &str) -> Result<(), String> {
    content::start_module(&kinds(), project_folder, module, name)
}
