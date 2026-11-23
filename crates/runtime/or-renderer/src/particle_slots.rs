//! What a frame's particle batches are drawn with: one pass for each batch, kept from a frame to the next until the batch asks for something else.

use std::collections::HashSet;
use std::sync::Arc;

use crate::game_shaders::effect_pass::EffectError;
use crate::game_shaders::mesh::Image;
use crate::particles::{ParticleBlendMode, ParticleCullMode};

/// What a batch's pass was prepared for: a batch that asks for something else prepares it again.
#[derive(Clone, PartialEq)]
pub(crate) struct Prepared {
    pub technique: String,
    pub blend: ParticleBlendMode,
    pub cull: ParticleCullMode,
    pub depth_test: bool,
    pub depth_write: bool,
    pub texture: usize,
    /// Which depth map the pass reads, none being nought.
    pub depth: u64,
}

impl Prepared {
    /// The identity of a texture, which a batch shares with the cache it came from.
    pub fn texture_of(image: Option<&Arc<Image>>) -> usize {
        image.map_or(0, |image| Arc::as_ptr(image) as usize)
    }
}

pub(crate) struct Slots<P> {
    slots: Vec<Option<(Prepared, P)>>,
    /// What was refused, so that it is neither tried nor said again.
    refused: HashSet<String>,
}

impl<P> Default for Slots<P> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            refused: HashSet::new(),
        }
    }
}

impl<P> Slots<P> {
    /// One slot for each of `count` batches, the surplus dropped.
    pub fn resize(&mut self, count: usize) {
        self.slots.resize_with(count, || None);
    }

    /// Take the pass of batch `at` away.
    pub fn clear(&mut self, at: usize) {
        self.slots[at] = None;
    }

    /// The pass of batch `at`, prepared by `make` where it has none or `wanted` is not what it has; none where `make` refuses, which is said once for its technique.
    pub fn ensure(
        &mut self,
        at: usize,
        wanted: Prepared,
        make: impl FnOnce() -> Result<P, EffectError>,
    ) -> Option<&mut P> {
        let current = self.slots[at].as_ref().map(|(prepared, _)| prepared);
        if current != Some(&wanted) {
            match make() {
                Ok(pass) => self.slots[at] = Some((wanted, pass)),
                Err(error) => {
                    self.slots[at] = None;
                    if self.refused.insert(wanted.technique.clone()) {
                        tracing::warn!(technique = %wanted.technique, %error, "particles of this technique are not drawn");
                    }
                    return None;
                }
            }
        }
        self.slots[at].as_mut().map(|(_, pass)| pass)
    }

    /// The pass of batch `at`, if it has one.
    pub fn get(&self, at: usize) -> Option<&P> {
        self.slots[at].as_ref().map(|(_, pass)| pass)
    }

    pub fn get_mut(&mut self, at: usize) -> Option<&mut P> {
        self.slots[at].as_mut().map(|(_, pass)| pass)
    }

    /// Every pass that is held.
    pub fn passes(&self) -> impl Iterator<Item = &P> {
        self.slots.iter().flatten().map(|(_, pass)| pass)
    }

    /// How many batches have a pass.
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}
