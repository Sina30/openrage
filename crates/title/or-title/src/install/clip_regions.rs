use thiserror::Error;

use crate::Vfs;

use or_io::gta5::data::ptx_clip_regions::{ClipRegionParseError, ClipRegionTable};

pub const CLIP_REGIONS_VFS_PATH: &str = "common:/data/effects/ptxclipregions.dat";

#[derive(Error, Debug)]
pub enum ClipRegionLoadError {
    #[error("`{path}` not found in the configured VFS (no common.rpf / update.rpf reachable)")]
    Missing { path: String },
    #[error("Failed to parse `{path}`: {source}")]
    Parse {
        path: String,
        #[source]
        source: ClipRegionParseError,
    },
}

pub fn load_clip_regions(vfs: &Vfs) -> Result<ClipRegionTable, ClipRegionLoadError> {
    let bytes = vfs
        .open(CLIP_REGIONS_VFS_PATH)
        .ok_or_else(|| ClipRegionLoadError::Missing {
            path: CLIP_REGIONS_VFS_PATH.to_string(),
        })?;
    let text = String::from_utf8_lossy(&bytes);
    ClipRegionTable::parse(&text).map_err(|source| ClipRegionLoadError::Parse {
        path: CLIP_REGIONS_VFS_PATH.to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::FileDevice;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Debug)]
    struct CannedDevice {
        files: HashMap<String, Vec<u8>>,
    }

    impl FileDevice for CannedDevice {
        fn open(&self, relative_path: &str) -> Option<Vec<u8>> {
            self.files
                .get(&relative_path.trim_start_matches('/').to_ascii_lowercase())
                .cloned()
        }
    }

    const FIXTURE: &str = "\
1
1
explosion 1 1
0.0 1.0 0.0 1.0
";

    #[test]
    fn load_resolves_through_common_mount() {
        let mut vfs = Vfs::new();
        let mut files = HashMap::new();
        files.insert(
            "data/effects/ptxclipregions.dat".to_string(),
            FIXTURE.as_bytes().to_vec(),
        );
        vfs.mount("common:", Arc::new(CannedDevice { files }));
        let table = load_clip_regions(&vfs).unwrap();
        assert!(table.get_by_name("explosion").is_some());
    }

    #[test]
    fn load_returns_missing_when_vfs_has_no_clip_regions() {
        let vfs = Vfs::new();
        match load_clip_regions(&vfs) {
            Err(ClipRegionLoadError::Missing { path }) => {
                assert_eq!(path, CLIP_REGIONS_VFS_PATH);
            }
            other => panic!("expected Missing, got {other:?}"),
        }
    }

    #[test]
    fn load_surfaces_parse_errors() {
        let mut vfs = Vfs::new();
        let mut files = HashMap::new();
        files.insert(
            "data/effects/ptxclipregions.dat".to_string(),
            b"not numbers at all\n".to_vec(),
        );
        vfs.mount("common:", Arc::new(CannedDevice { files }));
        match load_clip_regions(&vfs) {
            Err(ClipRegionLoadError::Parse { source, .. }) => {
                assert!(matches!(source, ClipRegionParseError::BadInt { .. }));
            }
            other => panic!("expected Parse, got {other:?}"),
        }
    }
}
