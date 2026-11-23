//! A particle list at either target, found in the payload that holds it.

use or_io::decl::{Gen8, Gen9};
use or_io::gta5::PtxFxList;

use super::textures::Lookup;
use super::DocPayload;
use crate::render::meshes::Material;

/// A target a `.ypt` is declared at: what its list is read from, and where a payload keeps it.
pub(crate) trait ParticleTarget: Material + Lookup + Clone + 'static {
    /// Whether `tick_sim` builds a simulation from a list at this target.
    const SIMULATED: bool;

    fn list(payload: &DocPayload) -> Option<&PtxFxList<Self>>;

    fn list_mut(payload: &mut DocPayload) -> Option<&mut PtxFxList<Self>>;
}

impl ParticleTarget for Gen8 {
    const SIMULATED: bool = true;

    fn list(payload: &DocPayload) -> Option<&PtxFxList<Self>> {
        match payload {
            DocPayload::Resource(or_io::Asset::Particles(list)) => Some(list),
            _ => None,
        }
    }

    fn list_mut(payload: &mut DocPayload) -> Option<&mut PtxFxList<Self>> {
        match payload {
            DocPayload::Resource(or_io::Asset::Particles(list)) => Some(list),
            _ => None,
        }
    }
}

impl ParticleTarget for Gen9 {
    const SIMULATED: bool = false;

    fn list(payload: &DocPayload) -> Option<&PtxFxList<Self>> {
        match payload {
            DocPayload::Resource(or_io::Asset::ParticlesGen9(list)) => Some(list),
            _ => None,
        }
    }

    fn list_mut(payload: &mut DocPayload) -> Option<&mut PtxFxList<Self>> {
        match payload {
            DocPayload::Resource(or_io::Asset::ParticlesGen9(list)) => Some(list),
            _ => None,
        }
    }
}

/// Runs `$body` with `$target` the target of the particle list `$payload` holds, and `$other` for a payload that holds none.
macro_rules! on_particle_target {
    ($payload:expr, $target:ident => $body:expr, else $other:expr) => {
        match $payload {
            $crate::state::DocPayload::Resource(or_io::Asset::Particles(_)) => {
                type $target = or_io::decl::Gen8;
                $body
            }
            $crate::state::DocPayload::Resource(or_io::Asset::ParticlesGen9(_)) => {
                type $target = or_io::decl::Gen9;
                $body
            }
            _ => $other,
        }
    };
}
pub(crate) use on_particle_target;
