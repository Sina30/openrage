//! The weapons engine: weapons a person streams into the game, and the metadata that defines them, as content of a project, with no UI.
//!
//! [`model`] is the weapons as a person edits them (the file `weapons.json`), [`layout`] says where each source lands in a resource,
//! [`facts`] looks at the sources a module names, [`metadata`] reads what their metadata defines through the typed family of `or-io`,
//! [`rules`] says what is wrong as data with codes, severities and waivers, [`plan`] hands all of it to the plan of a build as a
//! [`or_project::Contribution`], [`verify`] reads a build back, [`adopt`] recognises a folder of weapons and starts an empty one,
//! [`import`] makes sources of game files, [`edit`] changes a weapon's metadata through the typed family and [`assemble`] says which
//! parts a weapon wears and at which bone. `openrage-cli weapons` runs it without the editor.
//!
//! The metadata is read and written through `or_io::meta::typed::weapon`, whose members are the ones the install's own documents
//! state; what no member names arrives in a save as it came.
//!
//! ⚠ **Nothing here says a weapon loads in game.** A build says that every file it wrote reloads through the readers, which is all
//! that can be measured without running GTA V.

pub mod adopt;
pub mod assemble;
pub mod edit;
pub mod facts;
pub mod import;
pub mod layout;
pub mod metadata;
pub mod model;
pub mod plan;
pub mod rules;
pub mod verify;

pub use adopt::WeaponPack;
pub use facts::{inspect, Depth, Fact, Facts, SourceFact};
pub use layout::{data_file_type, DATA_FILE_TYPES, DATA_FOLDER};
pub use model::{Waiver, Weapon, Weapons, FILE};
pub use plan::{
    contribution, contributions, module_folder, module_folder_to_edit, planned, Planned,
};
pub use rules::{check, rule_of, Finding, Rule, Severity, RULES};
