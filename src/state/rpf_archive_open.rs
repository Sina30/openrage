use std::path::Path;
use std::sync::Arc;

use or_io::container::{RpfArchive, RpfCryptoProvider};

pub fn open_archive_from_disk(
    path: &Path,
    crypto: Arc<dyn RpfCryptoProvider>,
) -> Result<Arc<RpfArchive>, String> {
    RpfArchive::open_with(path, crypto)
        .map(Arc::new)
        .map_err(|e| match e {
            or_io::IoError::Rpf9TocEncrypted => super::gta_vi::rpf9_refusal_at(path)
                .unwrap_or_else(|| format!("open '{}': {e}", path.display())),
            _ => format!("open '{}': {e}", path.display()),
        })
}
