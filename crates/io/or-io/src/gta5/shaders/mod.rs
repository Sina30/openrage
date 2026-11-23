//! GTA V's shaders: the `.fxc` effects, Enhanced's shader container and the
//! `.sps` presets a title ships, the material parameter database built from
//! them, and the manifests of the baked shader packs.

#[cfg(feature = "fxc")]
pub mod fxc;
#[cfg(feature = "gta6")]
pub mod pipeline_cache;
#[cfg(feature = "fxc")]
pub mod sga;
#[cfg(all(feature = "fxc", feature = "gta6"))]
pub mod sgd5;
#[cfg(all(feature = "fxc", feature = "gta6"))]
pub mod sgd5_pack;
pub mod shader_defs;
pub mod shader_names;
pub mod spirv_pack;
pub mod sps;
#[cfg(feature = "gta6")]
pub mod xdat;
