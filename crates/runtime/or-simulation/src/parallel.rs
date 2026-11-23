use crate::emitter_inst::{BehaviourParamsTemplate, EmitterInst};
use rayon::prelude::*;
pub const PARALLEL_THRESHOLD: usize = 3;
pub fn tick_emitters(emitters: &mut [EmitterInst], dt: f32, template: BehaviourParamsTemplate<'_>) {
    if emitters.len() < PARALLEL_THRESHOLD {
        for em in emitters.iter_mut() {
            em.update(dt, template);
        }
        return;
    }

    // A rule's runtime flags, which could keep it on the main thread, are the game's to set; a file holds none.
    emitters.par_iter_mut().for_each(|em| {
        em.update(dt, template);
    });
}
