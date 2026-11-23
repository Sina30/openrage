//! A DXBC install, folder or file through the vendored translator, the legalizer and naga, tallied: the whole front end in one run.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use or_io::gta5::shaders::fxc::DxbcBlob;
use or_shader_front::BindingLayout;
use serde_json::{json, Value};

use crate::bindings::{self, agreement};
use crate::inventory::{containers, facts, for_each_source, InventoryError, Kind, Target};
use crate::spirv_check::{check_with, fold, ModuleCompiler, Tally};

/// What came of each container offered to the translator.
#[derive(Debug, Default)]
pub struct TranslateTally {
    /// Containers read, whatever their bytecode.
    pub containers: usize,
    /// Containers set aside before translation, by reason.
    pub skipped: BTreeMap<&'static str, usize>,
    pub translated: usize,
    /// Containers the translator refused, by its message.
    pub refused: BTreeMap<String, usize>,
    /// How far naga got with what was translated.
    pub check: Tally,
    /// What was translated, against what each container's reflection says it binds.
    pub bindings: bindings::Tally,
}

impl TranslateTally {
    fn merge(&mut self, other: TranslateTally) {
        self.containers += other.containers;
        for (reason, n) in other.skipped {
            *self.skipped.entry(reason).or_default() += n;
        }
        self.translated += other.translated;
        for (message, n) in other.refused {
            *self.refused.entry(message).or_default() += n;
        }
        self.check.merge(other.check);
        self.bindings.merge(other.bindings);
    }

    fn add(&mut self, blob: &[u8], compiler: Option<&dyn ModuleCompiler>) {
        self.containers += 1;
        let facts = facts(&DxbcBlob {
            offset: 0,
            size: blob.len(),
            bytes: blob,
        });
        match facts.kind {
            Kind::Dxil => *self.skipped.entry("DXIL").or_default() += 1,
            Kind::Neither => *self.skipped.entry("no program part").or_default() += 1,
            Kind::Dxbc => match or_shader_front::dxbc_to_spirv(blob) {
                Ok(spirv) => {
                    self.translated += 1;
                    self.bindings
                        .add(agreement(blob, &spirv, &BindingLayout::OPENRAGE));
                    self.check.add(&check_with(&spirv, compiler));
                }
                Err(error) => *self.refused.entry(fold(&error.to_string())).or_default() += 1,
            },
        }
    }

    pub fn report(&self) -> String {
        let skipped: usize = self.skipped.values().sum();
        let mut out = format!(
            "{} containers: {} translated, {} refused, {} set aside\n",
            self.containers,
            self.translated,
            self.refused.values().sum::<usize>(),
            skipped,
        );
        for (reason, n) in &self.skipped {
            out.push_str(&format!("  {n:>6}  set aside: {reason}\n"));
        }
        for (message, n) in &self.refused {
            out.push_str(&format!("  {n:>6}  refused by the translator: {message}\n"));
        }
        if self.translated > 0 {
            out.push('\n');
            out.push_str(&self.bindings.report());
            out.push('\n');
            out.push_str(&self.check.report());
        }
        out
    }

    pub fn to_json(&self) -> Value {
        json!({
            "containers": self.containers,
            "translated": self.translated,
            "skipped": self.skipped,
            "refused": self.refused,
            "bindings": self.bindings.to_json(),
            "check": self.check.to_json(),
        })
    }
}

/// Translate and check every DXBC container under `path`, on every core.
pub fn translate_check(path: &Path, target: Target) -> Result<TranslateTally, InventoryError> {
    translate_check_with(path, target, None)
}

/// [`translate_check`], offering every module that survives naga to `compiler`.
pub fn translate_check_with(
    path: &Path,
    target: Target,
    compiler: Option<&dyn ModuleCompiler>,
) -> Result<TranslateTally, InventoryError> {
    let mut blobs: Vec<Vec<u8>> = Vec::new();
    for_each_source(path, target, &mut |_, bytes| {
        blobs.extend(containers(bytes).into_iter().map(<[u8]>::to_vec));
    })?;
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    let mut total = TranslateTally::default();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut local = TranslateTally::default();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(blob) = blobs.get(index) else { break };
                        local.add(blob, compiler);
                    }
                    local
                })
            })
            .collect();
        for handle in handles {
            if let Ok(local) = handle.join() {
                total.merge(local);
            }
        }
    });
    Ok(total)
}

#[cfg(test)]
mod tests;
