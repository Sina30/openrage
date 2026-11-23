//! One SHA-1 over a baked pack's modules and its manifest, so a pack baked on another OS is compared by one line.

use std::path::Path;

use or_io::gta5::shaders::spirv_pack::{module_path, SpirvPack, MANIFEST_FILE};
use sha1::{Digest, Sha1};

/// How many modules a pack holds, and the digest of them and of the manifest.
///
/// ⚠ The manifest is digested without `source_build_hash`, which names the executable the sources came from and so differs by install and not by translator.
pub fn pack_digest(root: &Path) -> Result<(usize, String), String> {
    let manifest = root.join(MANIFEST_FILE);
    let mut pack: SpirvPack = serde_json::from_slice(
        &std::fs::read(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?,
    )
    .map_err(|e| format!("{}: {e}", manifest.display()))?;
    pack.source_build_hash.clear();
    let mut hasher = Sha1::new();
    let text = serde_json::to_vec(&pack).map_err(|e| e.to_string())?;
    hasher.update((text.len() as u64).to_le_bytes());
    hasher.update(&text);
    let mut modules = 0;
    for (key, program) in &pack.programs {
        if !matches!(
            program.outcome,
            or_io::gta5::shaders::spirv_pack::Outcome::Translated { .. }
        ) {
            continue;
        }
        let path = root.join(module_path(key));
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        modules += 1;
    }
    Ok((
        modules,
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    ))
}

#[cfg(test)]
mod tests;
