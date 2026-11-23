//! Red Dead Redemption 2's formats, declared as GTA V's are and read from an RSC8 resource.
//!
//! RDR2 has no source here to name a field by, so a declaration takes the engine's name where it
//! knows one and CodeX's layout (`CodeX.Games.RDR2/RSC8`) for the rest. Nothing here reaches into
//! `gta5`. Reading comes first: nothing here writes an RSC8 file.

pub mod shared;
pub mod texture;

pub use shared::{PgBase, PgDictionary};
pub use texture::{GrcTextureRdr2, ShaderResourceViewRdr2, TextureDictionary};
