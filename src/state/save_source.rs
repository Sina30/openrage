//! What a save writes, and where its bytes go.

use super::*;

pub(in crate::state) enum SaveSource {
    Payload(Box<DocPayload>),
    PatchedImage(Arc<ResourceImage>),
}

impl SaveSource {
    /// The bytes it saves as, rewriting a count a resource states against its run as `repair` allows, and each count it rewrote.
    pub(in crate::state) fn into_bytes(
        self,
        rpf_mode: RpfOutputMode,
        crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider>,
        repair: or_io::decl::Repair,
    ) -> std::result::Result<(Vec<u8>, Vec<String>), String> {
        match self {
            Self::Payload(payload) => {
                serialize_payload_repairing(&payload, rpf_mode, crypto, repair)
            }
            Self::PatchedImage(image) => image.to_resource_bytes().map(|b| (b, Vec::new())),
        }
    }
}

/// Writes a saved file: into the particles source it is the cook of when it is one, else to `path`.
pub(super) fn write_the_file(
    path: &PathBuf,
    bytes: &[u8],
    into_source: Option<&std::path::Path>,
) -> std::io::Result<()> {
    match into_source {
        Some(dir) => or_source::particles::write_back(dir, bytes).map_err(|problems| {
            std::io::Error::other(
                problems
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        }),
        None => atomic_write_with_backup(path, bytes),
    }
}
