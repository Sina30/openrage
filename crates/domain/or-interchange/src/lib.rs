//! A drawable as an external DCC tool sees it, and the interchange files that carry it.
//!
//! [`scene`] is the model, with no RAGE binary concern and no glTF concern;
//! [`extract`] fills it from a `rmcDrawable`, [`gltf`] writes and reads it, [`fbx`] reads one more, and
//! [`import`] puts it back into a drawable that already exists.
//!
//! ⚠ Import is not a mirror of export: it has to satisfy invariants a reader
//! tolerates, which is where the hazards are.

pub mod doctor;
pub mod extract;
pub mod fbx;
pub mod gltf;
pub mod import;
pub mod scene;

pub use extract::{drawable_scene, report_lines, ExtractOptions, LodSelection, SceneBuilder};
pub use import::{
    address, replace_geometry, replace_geometry_named, replace_in_file, Destination, ImportNames,
    ImportOptions, ImportReport,
};
pub use scene::{
    AlphaMode, Bone, Dropped, ExtractReport, Image, LodSlot, Material, Mesh, Object, Primitive,
    Scene, TextureBinding,
};
