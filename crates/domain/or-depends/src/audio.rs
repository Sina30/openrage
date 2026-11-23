//! Which wave container holds a sound: every `.awc` in an install, named by the
//! bank its path makes, joined to every `.rel` item that names a wave in one.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use or_io::awc::{AwcFile, AwcKey};
use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::hash::string_hash_str;
use or_io::rel::sound::SoundItem;
use or_io::rel::{RelFile, RelKey};
use or_title::install::crawl::{self, ControlFlow};

/// The bits of a wave name hash a container's stream table keeps.
pub const STREAM_ID_MASK: u32 = 0x1FFF_FFFF;

/// The `relType` of a sound file, the one whose items name waves.
const SOUNDS: u32 = 54;

/// One `.awc` in the install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub logical: String,
    /// `<archive or folder>/<stem>`, as the path spells it.
    pub bank: String,
    /// Stream name hashes, masked to [`STREAM_ID_MASK`], sorted.
    pub streams: Vec<u32>,
}

/// One wave a `.rel` item names, by its bank's hash and its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveReference {
    /// Position of the `.rel` in [`AudioIndex::rel_files`].
    pub rel: usize,
    /// The item's key: its name's hash.
    pub item: u32,
    /// The item's type, as the engine names it.
    pub class: &'static str,
    pub bank: u32,
    pub wave: u32,
}

/// Whether a reference's wave is in a container the install holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resolution {
    /// A container of the named bank holds the wave.
    Held,
    /// The bank's container is in the install and no copy of it holds the wave.
    BankWithoutWave,
    /// No container in the install is the named bank.
    NoBank,
}

/// What a lookup keeps: a container path or bank, and an item class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Matched, ignoring case, anywhere in a container's path or bank.
    pub container: Option<String>,
    /// Matched, ignoring case, against an item's whole class name.
    pub class: Option<String>,
}

impl Filter {
    fn admits_container(&self, logical: &str, bank: &str) -> bool {
        self.container.as_deref().is_none_or(|want| {
            let want = want.to_ascii_lowercase().replace('\\', "/");
            logical.to_ascii_lowercase().contains(&want)
                || bank.to_ascii_lowercase().contains(&want)
        })
    }

    fn admits_class(&self, class: &str) -> bool {
        self.class
            .as_deref()
            .is_none_or(|want| want.eq_ignore_ascii_case(class))
    }
}

/// A reference and the containers holding its wave, or with none, containers holding a wave no `.rel` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit<'a> {
    pub reference: Option<&'a WaveReference>,
    pub containers: Vec<&'a Container>,
}

/// Every container and every wave reference an install holds.
#[derive(Debug, Default, Clone)]
pub struct AudioIndex {
    containers: Vec<Container>,
    by_bank: HashMap<u32, Vec<usize>>,
    rel_files: Vec<String>,
    references: Vec<WaveReference>,
    /// Bank names as the `.rel` name tables spell them, by hash.
    bank_names: HashMap<u32, String>,
    refused: Vec<(String, String)>,
}

impl AudioIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every copy of every `.rel`, and of every `.awc` the filter's container admits, in the install.
    // ⚠ A crawl rather than the VFS mount: Legacy keeps its wave banks in
    // archives loose under `x64/audio/sfx/`, which the mount does not hold.
    pub fn read(
        game_root: &Path,
        provider: Arc<dyn RpfCryptoProvider>,
        key: AwcKey,
        filter: &Filter,
        warnings: &mut Vec<String>,
    ) -> Self {
        let mut out = Self::new();
        let _ = crawl::crawl(game_root, provider, warnings, &mut |found| {
            let leaf = found.leaf.to_ascii_lowercase();
            let logical = if found.container == crawl::LOOSE {
                found.path.to_string()
            } else {
                format!("{}/{}", found.container, found.path)
            };
            let admitted = leaf.ends_with(".rel")
                || (leaf.ends_with(".awc")
                    && filter.admits_container(&logical, &bank_of(&logical)));
            if admitted {
                match found.read() {
                    Ok(bytes) => out.add_file(&logical, &bytes, key),
                    Err(e) => out.refused.push((logical, e)),
                }
            }
            ControlFlow::Continue(())
        });
        out
    }

    /// Index one `.awc` or `.rel` by its extension, recording a refusal rather than failing.
    pub fn add_file(&mut self, logical: &str, bytes: &[u8], key: AwcKey) {
        let lower = logical.to_ascii_lowercase();
        let outcome = if lower.ends_with(".awc") {
            AwcFile::parse(bytes, key).map(|awc| self.add_container(logical, &awc))
        } else if lower.ends_with(".rel") {
            RelFile::open(bytes).and_then(|rel| self.add_rel(logical, &rel))
        } else {
            return;
        };
        if let Err(e) = outcome {
            self.refused.push((logical.to_string(), e.to_string()));
        }
    }

    pub fn add_container(&mut self, logical: &str, awc: &AwcFile) {
        let bank = bank_of(logical);
        let mut streams: Vec<u32> = awc.streams.iter().map(|s| s.id).collect();
        streams.sort_unstable();
        streams.dedup();
        self.by_bank
            .entry(string_hash_str(&bank))
            .or_default()
            .push(self.containers.len());
        self.containers.push(Container {
            logical: logical.to_string(),
            bank,
            streams,
        });
    }

    /// Index a sound file's wave references; any other `relType` names no wave.
    pub fn add_rel(&mut self, logical: &str, rel: &RelFile) -> or_io::Result<()> {
        if rel.rel_type != SOUNDS {
            return Ok(());
        }
        let items = rel.items()?;
        let at = self.rel_files.len();
        self.rel_files.push(logical.to_string());
        for name in &rel.names {
            self.bank_names
                .entry(string_hash_str(name))
                .or_insert_with(|| name.clone());
        }
        for item in items {
            let key = match item.key() {
                RelKey::Hash(hash) => hash,
                RelKey::Name(name) => string_hash_str(name),
            };
            let sound = SoundItem::decode(item.body())?;
            for (bank, wave) in waves_named(&sound) {
                self.references.push(WaveReference {
                    rel: at,
                    item: key,
                    class: sound.type_name(),
                    bank,
                    wave,
                });
            }
        }
        Ok(())
    }

    /// Every reference whose item or wave is `sound`, and every container holding `sound` as a wave.
    pub fn find(&self, sound: u32, filter: &Filter) -> Vec<Hit<'_>> {
        let mut hits = Vec::new();
        let mut covered: Vec<usize> = Vec::new();
        for reference in &self.references {
            let named = reference.item == sound
                || reference.wave & STREAM_ID_MASK == sound & STREAM_ID_MASK;
            if !named || !filter.admits_class(reference.class) {
                continue;
            }
            let holding = self.holding(reference.bank, reference.wave);
            covered.extend(holding.iter().copied());
            let containers: Vec<&Container> = holding
                .into_iter()
                .map(|i| &self.containers[i])
                .filter(|c| filter.admits_container(&c.logical, &c.bank))
                .collect();
            if filter.container.is_some() && containers.is_empty() {
                continue;
            }
            hits.push(Hit {
                reference: Some(reference),
                containers,
            });
        }
        if filter.class.is_none() {
            let unnamed: Vec<&Container> = self
                .containers
                .iter()
                .enumerate()
                .filter(|(i, c)| {
                    !covered.contains(i)
                        && c.streams.binary_search(&(sound & STREAM_ID_MASK)).is_ok()
                        && filter.admits_container(&c.logical, &c.bank)
                })
                .map(|(_, c)| c)
                .collect();
            if !unnamed.is_empty() {
                hits.push(Hit {
                    reference: None,
                    containers: unnamed,
                });
            }
        }
        hits
    }

    pub fn resolution(&self, reference: &WaveReference) -> Resolution {
        match self.by_bank.get(&reference.bank) {
            None => Resolution::NoBank,
            Some(_) if self.holding(reference.bank, reference.wave).is_empty() => {
                Resolution::BankWithoutWave
            }
            Some(_) => Resolution::Held,
        }
    }

    pub fn containers(&self) -> &[Container] {
        &self.containers
    }

    pub fn references(&self) -> &[WaveReference] {
        &self.references
    }

    /// The sound files indexed, by the position a [`WaveReference::rel`] names.
    pub fn rel_files(&self) -> &[String] {
        &self.rel_files
    }

    /// Files that would not open, with why.
    pub fn refused(&self) -> &[(String, String)] {
        &self.refused
    }

    /// A bank's name as a sound file's name table spells it.
    pub fn bank_name(&self, bank: u32) -> Option<&str> {
        self.bank_names.get(&bank).map(String::as_str)
    }

    fn holding(&self, bank: u32, wave: u32) -> Vec<usize> {
        self.by_bank
            .get(&bank)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| {
                self.containers[i]
                    .streams
                    .binary_search(&(wave & STREAM_ID_MASK))
                    .is_ok()
            })
            .collect()
    }
}

/// The bank a container's path makes: its archive or folder, less `.rpf`, then its stem.
// ⚠ The parent is part of the name, not only the stem; R34b's roadmap row
// records how many shipped waves this resolves.
pub fn bank_of(logical: &str) -> String {
    let mut segments = logical.rsplit(['/', '\\']);
    let leaf = segments.next().unwrap_or(logical);
    let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
    match segments.next() {
        Some(parent) => {
            let parent = match parent.len().checked_sub(4) {
                Some(cut) if parent[cut..].eq_ignore_ascii_case(".rpf") => &parent[..cut],
                _ => parent,
            };
            format!("{parent}/{stem}")
        }
        None => stem.to_string(),
    }
}

/// The `(bank, wave)` pairs a sound item names; an unused granular channel names none.
fn waves_named(sound: &SoundItem) -> Vec<(u32, u32)> {
    let pairs = match sound {
        SoundItem::SimpleSound(s) => vec![(s.wave_ref.bank_name, s.wave_ref.wave_name)],
        SoundItem::GranularSound(s) => vec![
            (s.channel0.bank_name, s.channel0.wave_name),
            (s.channel1.bank_name, s.channel1.wave_name),
            (s.channel2.bank_name, s.channel2.wave_name),
            (s.channel3.bank_name, s.channel3.wave_name),
            (s.channel4.bank_name, s.channel4.wave_name),
            (s.channel5.bank_name, s.channel5.wave_name),
        ],
        SoundItem::AutomationSound(s) => vec![(s.data_ref.bank_name, s.data_ref.wave_name)],
        _ => Vec::new(),
    };
    pairs
        .into_iter()
        .filter(|&(bank, wave)| bank != 0 || wave != 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bank_is_its_archive_or_folder_and_its_stem() {
        assert_eq!(
            bank_of("platform:/audio/sfx/RESIDENT.rpf/explosions.awc"),
            "RESIDENT/explosions"
        );
        assert_eq!(
            bank_of("audio\\sfx\\dlc_heist\\vault.awc"),
            "dlc_heist/vault"
        );
        assert_eq!(bank_of("loose.awc"), "loose");
    }
}
