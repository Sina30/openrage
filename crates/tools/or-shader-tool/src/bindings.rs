//! Whether the SPIR-V the translator wrote binds what the container's own reflection says it binds, register for register.

use std::collections::BTreeSet;

use or_io::gta5::shaders::fxc::{DxbcBindKind, DxbcBlob, DxbcReflection};
use or_shader_front::{BindingLayout, ResourceClass};
use serde_json::{json, Value};

/// The descriptor class a reflected binding is, or `None` for a kind this does not place.
pub fn class_of(kind: DxbcBindKind) -> Option<ResourceClass> {
    match kind {
        DxbcBindKind::CBuffer => Some(ResourceClass::Cbv),
        DxbcBindKind::Sampler => Some(ResourceClass::Sampler),
        DxbcBindKind::TBuffer
        | DxbcBindKind::Texture
        | DxbcBindKind::Structured
        | DxbcBindKind::ByteAddress => Some(ResourceClass::Srv),
        DxbcBindKind::UavTyped
        | DxbcBindKind::UavStructured
        | DxbcBindKind::UavByteAddress
        | DxbcBindKind::UavAppend
        | DxbcBindKind::UavConsume
        | DxbcBindKind::UavCounter => Some(ResourceClass::Uav),
        DxbcBindKind::Other(_) => None,
    }
}

/// Whether a UAV of this kind has a hidden counter descriptor of its own.
pub(crate) fn has_counter(kind: DxbcBindKind) -> bool {
    matches!(
        kind,
        DxbcBindKind::UavAppend | DxbcBindKind::UavConsume | DxbcBindKind::UavCounter
    )
}

/// Every `(set, binding)` the reflection's registers land on under `layout`; a register past its window is left out.
pub fn expected(reflection: &DxbcReflection, layout: &BindingLayout) -> BTreeSet<(u32, u32)> {
    let mut out = BTreeSet::new();
    for b in &reflection.bindings {
        let Some(class) = class_of(b.kind) else {
            continue;
        };
        let counter = has_counter(b.kind).then_some(ResourceClass::UavCounter);
        for class in std::iter::once(class).chain(counter) {
            if let Some(at) = layout.binding(class, 0, b.register) {
                out.insert((at.set, at.binding));
            }
        }
    }
    out
}

/// Every `(set, binding)` a SPIR-V module decorates on a variable.
pub fn decorated(spirv: &[u8]) -> BTreeSet<(u32, u32)> {
    let words: Vec<u32> = spirv
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let (mut sets, mut bindings) = (Vec::new(), Vec::new());
    let mut at = 5;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        // OpDecorate: DescriptorSet is 34 and Binding is 33.
        if words[at] & 0xffff == 71 && at + 3 < words.len() {
            match words[at + 2] {
                34 => sets.push((words[at + 1], words[at + 3])),
                33 => bindings.push((words[at + 1], words[at + 3])),
                _ => {}
            }
        }
        at += count.max(1);
    }
    bindings
        .iter()
        .filter_map(|(id, binding)| {
            let set = sets.iter().find(|(target, _)| target == id)?.1;
            Some((set, *binding))
        })
        .collect()
}

/// How a module's bindings compare with its container's reflection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agreement {
    Same,
    /// The module binds some of what the reflection lists and nothing else.
    ModuleBindsLess,
    /// The module binds everything the reflection lists and more.
    ModuleBindsMore,
    Differs,
    /// The container has no `RDEF` part, as a stripped build has none.
    NoReflection,
    /// The `RDEF` part did not parse.
    Unreadable,
}

/// Compare `spirv`, translated from `container` under `layout` for the container's own stage, with the container's reflection.
pub fn agreement(container: &[u8], spirv: &[u8], layout: &BindingLayout) -> Agreement {
    let layout = layout.for_program_type(or_shader_front::program_type(container).unwrap_or(1));
    let blob = DxbcBlob {
        offset: 0,
        size: container.len(),
        bytes: container,
    };
    let reflection = match blob.reflect() {
        None => return Agreement::NoReflection,
        Some(Err(_)) => return Agreement::Unreadable,
        Some(Ok(reflection)) => reflection,
    };
    let (wanted, found) = (expected(&reflection, &layout), decorated(spirv));
    if wanted == found {
        Agreement::Same
    } else if found.is_subset(&wanted) {
        Agreement::ModuleBindsLess
    } else if wanted.is_subset(&found) {
        Agreement::ModuleBindsMore
    } else {
        Agreement::Differs
    }
}

/// [`Agreement`] counted over many containers.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tally {
    pub same: usize,
    pub module_binds_less: usize,
    pub module_binds_more: usize,
    pub differs: usize,
    pub no_reflection: usize,
    pub unreadable: usize,
}

impl Tally {
    pub fn add(&mut self, agreement: Agreement) {
        let slot = match agreement {
            Agreement::Same => &mut self.same,
            Agreement::ModuleBindsLess => &mut self.module_binds_less,
            Agreement::ModuleBindsMore => &mut self.module_binds_more,
            Agreement::Differs => &mut self.differs,
            Agreement::NoReflection => &mut self.no_reflection,
            Agreement::Unreadable => &mut self.unreadable,
        };
        *slot += 1;
    }

    pub fn merge(&mut self, other: Tally) {
        self.same += other.same;
        self.module_binds_less += other.module_binds_less;
        self.module_binds_more += other.module_binds_more;
        self.differs += other.differs;
        self.no_reflection += other.no_reflection;
        self.unreadable += other.unreadable;
    }

    pub fn total(&self) -> usize {
        self.same
            + self.module_binds_less
            + self.module_binds_more
            + self.differs
            + self.no_reflection
            + self.unreadable
    }

    pub fn report(&self) -> String {
        format!(
            "bindings against reflection, of {} translated: {} the same, {} bind less, {} bind more, {} differ, {} with no reflection, {} unreadable\n",
            self.total(),
            self.same,
            self.module_binds_less,
            self.module_binds_more,
            self.differs,
            self.no_reflection,
            self.unreadable,
        )
    }

    pub fn to_json(&self) -> Value {
        json!({
            "same": self.same,
            "module_binds_less": self.module_binds_less,
            "module_binds_more": self.module_binds_more,
            "differs": self.differs,
            "no_reflection": self.no_reflection,
            "unreadable": self.unreadable,
        })
    }
}

#[cfg(test)]
mod tests;
