//! The addon ped engine: ped models a person streams into the game, and the data that defines them, as content of a project, with no UI.
//!
//! [`model`] is the peds as a person edits them (the file `addonped.json`), [`layout`] says where
//! each file lands in a resource, [`facts`] looks at the files a module names, [`rules`] says what is
//! wrong as data with codes, severities and waivers, [`plan`] hands all of it to the plan of a build as
//! a [`or_project::Contribution`], [`verify`] reads a build back and [`adopt`] recognises a folder
//! of peds and starts an empty one. `openrage-cli addonped` runs it without the editor.
//!
//! The data file that defines peds to the game (`peds.meta`) is shipped as the person wrote it, and
//! this reads one thing of it: the names it defines peds under ([`metadata`]).
//!
//! ⚠ **Nothing here says a ped loads in game.** A build says that every file it wrote reloads through
//! the readers, which is all that can be measured without running GTA V.

pub mod adopt;
pub mod facts;
pub mod layout;
pub mod metadata;
pub mod model;
pub mod plan;
pub mod rules;
pub mod verify;

pub use adopt::AddonPed;
pub use facts::{inspect, Depth, Fact, Facts};
pub use model::{AddonPeds, Ped, PedFile, Waiver, FILE};
pub use plan::{
    contribution, contributions, module_folder, module_folder_to_edit, planned, Planned,
};
pub use rules::{check, rule_of, Finding, Rule, Severity, RULES};
