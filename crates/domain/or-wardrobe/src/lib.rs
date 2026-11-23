//! The Wardrobe engine: a clothing pack for the freemode peds as a model, and everything a build of it needs, with no UI.
//!
//! [`model`] is the pack as a person edits it, [`resolve`] reads the names of files into it and [`edit`] is
//! every change a screen makes to it, with [`replace`], [`duplicate`] and [`variations`] making the changes that
//! put files in place and [`export`] writing them out, and [`fix`] and [`textures`] mending a whole pack at once, the first by the steps
//! of Fix all safe and the second by making its textures lighter, each as one change that can be taken back;
//! [`alloc`] gives each garment its place among the drawables of its slot; [`rules`] says what is
//! wrong, as data with codes, severities and waivers; [`metadata`] writes the `.ymt` and the shop
//! metadata the places imply; [`plan`] hands all of it to the plan of a build as a
//! [`or_project::Contribution`]; [`verify`] reads a build back; [`search`] says what a garment is found by; and
//! [`import`] makes a pack of an existing resource's files. `openrage-cli wardrobe` runs it without the editor.
//!
//! ⚠ **Nothing here says a pack loads in game.** A build says that every file it wrote reloads
//! through the readers, which is all that can be measured without running GTA V.

pub mod adopt;
pub mod alloc;
pub mod alternate;
pub mod audio;
pub mod backup;
pub mod behaviour;
pub mod budget;
pub mod bulk;
pub mod cache;
pub mod changes;
pub mod credits;
pub mod dress;
pub mod dummies;
pub mod dump;
pub mod duplicate;
pub mod edit;
pub mod export;
pub mod facts;
pub mod firstperson;
pub mod fix;
pub mod flags;
pub mod follow;
pub mod freeze;
pub mod gctproject;
pub mod hair;
pub mod heels;
mod images;
pub mod import;
mod intake;
pub mod layout;
pub mod lock;
pub mod maps;
pub mod mask;
pub mod measure;
pub mod mesh;
pub mod metadata;
pub mod model;
pub mod names;
pub mod options;
pub mod outfit;
pub mod pack_package;
pub mod package;
pub mod plan;
pub mod query;
pub mod redo;
pub mod reopen;
mod repair;
pub mod replace;
pub mod replace_mode;
pub mod resolve;
pub mod review;
pub mod rules;
pub mod search;
pub mod settings;
pub mod shop;
pub mod shop_edit;
pub mod sizes;
pub mod slots;
mod source_facts;
pub mod textures;
pub mod variations;
pub mod verify;
pub mod wind;

pub use alloc::{allocate, places_of, Allocation, Placed, Refused, MAX_PARTS, MAX_POSITION};
pub use facts::{inspect, Depth, Fact, Facts};
pub use model::{
    Carried, Expression, Garment, Meta, Shop, ShopEntry, Stated, Variation, Waiver, Wardrobe, FILE,
};
pub use names::{Component, Ped, Prop, Skin, Slot};
pub use plan::{
    contribution, contribution_for, contributions, contributions_to, is_standard, module_folder,
    module_folder_to_edit, plan, plan_for, planned, planned_for, planned_with_model, plans,
    plans_from, plans_given, Build, Documents, Planned,
};
pub use redo::{purge_held, redo, Redo};
pub use resolve::{resolve, Resolved, Role, Unread};
pub use rules::{check, rule_of, Finding, Rule, Severity, RULES};
pub use settings::Replace;
