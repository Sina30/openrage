//! Which extensions the editor offers and names a document kind for, by
//! resource and target; what a file holds is `or_io::open`'s answer.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock, RwLockReadGuard};

use or_io::container::resource::ResourceType;
use or_io::Generation;

/// Which registration wins when two claim the same format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Fallback,
    Standard,
    Preferred,
}

/// A resource at one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormatId {
    pub resource: ResourceType,
    pub target: Generation,
}

impl FormatId {
    pub const fn new(resource: ResourceType, target: Generation) -> Self {
        Self { resource, target }
    }
}

impl std::fmt::Display for FormatId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.resource, self.target.slug())
    }
}

/// A format the editor offers, and the extensions that name it.
#[derive(Debug, Clone)]
pub struct FormatHandler {
    pub id: FormatId,
    pub extensions: &'static [&'static str],
}

#[derive(Default)]
pub struct FormatRegistry {
    tiers: HashMap<Tier, HashMap<FormatId, FormatHandler>>,
}

impl FormatRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// One handler per resource and per target it ships.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        for resource in ResourceType::ALL {
            let extensions = resource.extensions();
            if extensions.is_empty() {
                continue;
            }
            for target in [Generation::Gen8, Generation::Gen9] {
                if target.is_gen9() && !resource.has_gen9_variant() {
                    continue;
                }
                registry.register_at(
                    Tier::Standard,
                    FormatHandler {
                        id: FormatId::new(resource, target),
                        extensions,
                    },
                );
            }
        }
        registry
    }

    pub fn register_at(&mut self, tier: Tier, handler: FormatHandler) {
        self.tiers
            .entry(tier)
            .or_default()
            .insert(handler.id, handler);
    }

    #[cfg(test)]
    pub fn get(&self, id: FormatId) -> Option<&FormatHandler> {
        [Tier::Preferred, Tier::Standard, Tier::Fallback]
            .into_iter()
            .find_map(|tier| self.tiers.get(&tier).and_then(|m| m.get(&id)))
    }

    /// Every handler once, the highest tier's for each format, Legacy before Enhanced.
    pub fn handlers(&self) -> Vec<&FormatHandler> {
        let mut out: Vec<&FormatHandler> = Vec::new();
        for tier in [Tier::Preferred, Tier::Standard, Tier::Fallback] {
            let Some(handlers) = self.tiers.get(&tier) else {
                continue;
            };
            let mut ids: Vec<&FormatId> = handlers.keys().collect();
            ids.sort_by_key(|id| (id.resource as u8, id.target));
            for id in ids {
                if !out.iter().any(|h| h.id == *id) {
                    out.push(&handlers[id]);
                }
            }
        }
        out
    }

    pub fn by_extension(&self, extension: &str) -> Vec<&FormatHandler> {
        let extension = extension.trim_start_matches('.').to_ascii_lowercase();
        self.handlers()
            .into_iter()
            .filter(|h| h.extensions.contains(&extension.as_str()))
            .collect()
    }

    #[cfg(test)]
    pub fn known_extensions(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = self
            .handlers()
            .into_iter()
            .flat_map(|h| h.extensions.iter().copied())
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn live() -> &'static RwLock<FormatRegistry> {
    static LIVE: OnceLock<RwLock<FormatRegistry>> = OnceLock::new();
    LIVE.get_or_init(|| RwLock::new(FormatRegistry::builtin()))
}

fn read() -> RwLockReadGuard<'static, FormatRegistry> {
    live().read().expect("format registry poisoned")
}

// A registration seam: only `adding_a_format` calls it until a plugin host does.
#[cfg_attr(not(test), allow(dead_code))]
pub fn register_at(tier: Tier, handler: FormatHandler) {
    live()
        .write()
        .expect("format registry poisoned")
        .register_at(tier, handler);
}

pub fn handlers() -> Vec<FormatHandler> {
    read().handlers().into_iter().cloned().collect()
}

pub fn by_extension(extension: &str) -> Vec<FormatHandler> {
    read()
        .by_extension(extension)
        .into_iter()
        .cloned()
        .collect()
}

#[cfg(test)]
pub fn known_extensions() -> Vec<&'static str> {
    read().known_extensions()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler(extensions: &'static [&'static str]) -> FormatHandler {
        FormatHandler {
            id: FormatId::new(ResourceType::Ydr, Generation::Gen8),
            extensions,
        }
    }

    #[test]
    fn the_builtin_registry_covers_every_paged_format() {
        let registry = FormatRegistry::builtin();
        for resource in ResourceType::ALL {
            assert!(
                registry
                    .get(FormatId::new(resource, Generation::Gen8))
                    .is_some(),
                "{resource} has no Gen8 handler"
            );
        }
        assert!(!ResourceType::Ybn.has_gen9_variant());
        assert!(registry
            .get(FormatId::new(ResourceType::Ybn, Generation::Gen9))
            .is_none());
    }

    #[test]
    fn tier_precedence_not_registration_order_decides() {
        let mut registry = FormatRegistry::new();
        let id = FormatId::new(ResourceType::Ydr, Generation::Gen8);
        registry.register_at(Tier::Fallback, handler(&["fallback"]));
        assert_eq!(registry.get(id).unwrap().extensions, ["fallback"]);
        registry.register_at(Tier::Preferred, handler(&["preferred"]));
        registry.register_at(Tier::Standard, handler(&["standard"]));
        assert_eq!(registry.get(id).unwrap().extensions, ["preferred"]);
    }

    #[test]
    fn the_extension_list_is_sorted_unique_and_takes_any_spelling() {
        let registry = FormatRegistry::builtin();
        let extensions = registry.known_extensions();
        for resource in ResourceType::ALL {
            for extension in resource.extensions() {
                assert!(extensions.contains(extension), "`.{extension}` is missing");
            }
        }
        let mut sorted = extensions.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(extensions, sorted);
        assert_eq!(
            registry.by_extension(".YDR").len(),
            registry.by_extension("ydr").len()
        );
        assert!(registry.by_extension("txt").is_empty());
    }

    #[test]
    fn no_live_registration_removes_a_builtin_extension() {
        let live = known_extensions();
        for resource in ResourceType::ALL {
            for extension in resource.extensions() {
                assert!(
                    live.contains(extension),
                    "`.{extension}` no longer resolves in the live registry; something \
                     registered over the {resource} builtin with a narrower extension list"
                );
            }
        }
    }
}
