//! The binary `.dat` tables the game names by file: the cache loader's
//! `*_cache_y.dat`, `metadata.dat` and `audioworldsectorsinfo.dat`.

use std::path::Path;

use or_io::audio_world_sectors::AudioWorldSectors;
use or_io::cache_loader::CacheFile;
use or_io::social_club_metadata::SocialClubMetadata;

/// One of the tables, as its file name says.
#[derive(Debug, Clone, PartialEq)]
pub enum DatTable {
    CacheFile(CacheFile),
    SocialClubMetadata(SocialClubMetadata),
    AudioWorldSectors(AudioWorldSectors),
}

const CACHE_SUFFIX: &str = "_cache_y.dat";
const METADATA: &str = "metadata.dat";
const SECTORS: &str = "audioworldsectorsinfo.dat";

impl DatTable {
    /// Whether a lowercase file name is one of the tables.
    pub fn names(name: &str) -> bool {
        name.ends_with(CACHE_SUFFIX) || name == METADATA || name == SECTORS
    }

    /// Reads `data` as the table `path`'s file name names.
    pub fn open(path: &Path, data: &[u8]) -> Result<Self, String> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let table = if name.ends_with(CACHE_SUFFIX) {
            CacheFile::open(data).map(Self::CacheFile)
        } else if name == METADATA {
            SocialClubMetadata::open(data).map(Self::SocialClubMetadata)
        } else if name == SECTORS {
            AudioWorldSectors::open(data).map(Self::AudioWorldSectors)
        } else {
            return Err(format!(
                "{name} is not a *{CACHE_SUFFIX}, {METADATA} or {SECTORS}"
            ));
        };
        table.map_err(|e| format!("Parse error: {e}"))
    }

    pub fn save(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::CacheFile(cache) => cache.save(),
            Self::SocialClubMetadata(metadata) => metadata.save(),
            Self::AudioWorldSectors(grid) => grid.save(),
        }
        .map_err(|e| e.to_string())
    }

    pub fn held_bytes(&self) -> usize {
        match self {
            Self::CacheFile(cache) => {
                std::mem::size_of_val(cache.file_dates.as_slice())
                    + std::mem::size_of_val(cache.map_data_store.as_slice())
                    + std::mem::size_of_val(cache.interior_proxies.as_slice())
                    + std::mem::size_of_val(cache.bounds_store.as_slice())
            }
            Self::SocialClubMetadata(metadata) => metadata.body.len(),
            Self::AudioWorldSectors(grid) => std::mem::size_of_val(grid.sectors.as_slice()),
        }
    }
}
