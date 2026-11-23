//! The commands of the Particle screen: giving a project particles, adding `.ypt` files to it, and making an empty list.

use super::table::{a_project, none_if, Action, Spec, BASE};
use crate::platform::file_dialogs;
use crate::state::AppState;

const NO_PARTICLES: &str =
    tr_key!("The project holds no particles: Project ▸ Add a particle pack… adds some");

fn has_particles(state: &AppState) -> Result<(), &'static str> {
    a_project(state)?;
    none_if(!state.has_particles(), crate::i18n::tr(NO_PARTICLES))
}

const LINKED: &str = tr_key!(
    "These particles are linked from another project, which alone edits them: open that project to change them"
);

fn writable_particles(state: &AppState) -> Result<(), &'static str> {
    has_particles(state)?;
    none_if(
        state.particles_pack().is_some_and(|pack| pack.linked),
        crate::i18n::tr(LINKED),
    )
}

pub(super) const ADD_PARTICLE_PACK: Spec = Spec {
    action: Action::AddParticlePack,
    id: "add_particle_pack",
    label: tr_key!("Add a particle pack…"),
    category: tr_key!("Project ▸ More ▸ Particles"),
    hover: tr_key!("Give the project particle effects: an empty pack in a folder of its own, shipped by every build, and the Particle screen to fill it. Add particle effects… puts .ypt files into a pack that is there"),
    enabled: a_project,
    run: |state, _| state.add_particle_content(),
    ..BASE
};

pub(super) const ADD_PARTICLES: Spec = Spec {
    action: Action::AddParticles,
    id: "add_particles",
    label: tr_key!("Add particle effects…"),
    category: tr_key!("Project ▸ More ▸ Particles"),
    hover: tr_key!("Imports into the particles the .ypt files you pick: each becomes a source of the pack, its effects as rules you can read and change and its textures as DDS files. A file the source form cannot hold is refused with the reason, and nothing of it is kept. Dropping them on the Particle screen does the same"),
    enabled: writable_particles,
    run: |state, _| {
        if let Some(files) = file_dialogs::open_any_files() {
            state.add_particles(files);
        }
    },
    ..BASE
};

pub(super) const NEW_PARTICLES: Spec = Spec {
    action: Action::NewParticles,
    id: "new_particles",
    label: tr_key!("New particle effects"),
    category: tr_key!("Project ▸ More ▸ Particles"),
    hover: tr_key!("Add an empty list of particle effects to the pack, as a source of its own called fx_new. Open it in the Particle workspace to add effects"),
    enabled: writable_particles,
    run: |state, _| state.new_particles(),
    ..BASE
};
