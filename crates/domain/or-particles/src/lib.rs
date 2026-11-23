//! The particles engine: the particle effects a person streams into the game, as content of a project, with no UI.
//!
//! [`model`] is the assets of a pack (the file `effects.json`), each the source of a `.ypt`; [`layout`] says where each lands in a
//! resource, [`facts`] looks at the sources a module names and reads the rules they state, [`rules`] says what is wrong as data with codes,
//! severities and waivers, [`plan`] hands all of it to the plan of a build as a [`or_project::Contribution`], [`verify`] reads a build
//! back, [`adopt`] recognises a folder of particles and starts an empty one, and [`import`] makes sources of game files.
//! `openrage-cli particles` runs it without the editor.
//!
//! The source of a `.ypt` is `or_source::particles`: its rules as one document, its textures as exact DDS files and its models kept whole.
//! `or-particle` is the editor's model of an open list, and this crate is the kind of content a project holds.
//!
//! ⚠ **Nothing here says an effect plays in game.** A build says that every file it wrote reloads through the readers, which is all
//! that can be measured without running GTA V.

pub mod adopt;
pub mod facts;
pub mod import;
pub mod layout;
pub mod model;
pub mod plan;
pub mod rules;
pub mod verify;

pub use adopt::ParticlePack;
pub use facts::{inspect, Depth, Fact, Facts, Inside, SourceFact};
pub use layout::{stream_path, STREAM};
pub use model::{Asset, Particles, Waiver, FILE};
pub use plan::{
    contribution, contributions, module_folder, module_folder_to_edit, planned, Planned,
};
pub use rules::{check, rule_of, Finding, Rule, Severity, RULES};
