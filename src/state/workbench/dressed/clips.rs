//! Reading the clip dictionary the dressed ped plays: the default movement of the install for a preset, or a `.ycd` file a person names.

use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::gta5::CrClipDictionary;
use or_wardrobe::Ped;

use super::motion::dictionary_file;
use super::Mounted;
use crate::state::{DocPayload, DocumentKind};

/// A dictionary on its way from a worker, with the sentence that names what is being read.
pub(super) struct Reading {
    pub label: String,
    /// The clip to start with: a preset's, none for a file, which starts with its first.
    pub wanted: Option<&'static str>,
    pub receiver: Receiver<Result<Arc<CrClipDictionary>, String>>,
}

/// The default movement dictionary of `ped`, read from the install `mounted`.
pub(super) fn read_preset(
    mounted: &Mounted,
    ped: Ped,
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Result<Arc<CrClipDictionary>, String> {
    let file = dictionary_file(ped);
    let mut logical = None;
    mounted.game.for_each_entry(|facts| {
        if logical.is_none() && facts.leaf.eq_ignore_ascii_case(&file) {
            logical = Some(facts.logical.clone());
        }
    });
    let logical = logical.ok_or_else(|| tr!("{file} is not in the install", file = file))?;
    let bytes = (mounted.game.vfs.open(&logical))
        .ok_or_else(|| tr!("{file} cannot be read", file = file))?;
    parse(Path::new(&file), &bytes, crypto)
}

/// The clip dictionary in the file at `path`.
pub(super) fn read_file(
    path: &Path,
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Result<Arc<CrClipDictionary>, String> {
    let bytes = std::fs::read(path).map_err(|e| tr!("cannot be read: {e}", e = e))?;
    parse(path, &bytes, crypto)
}

fn parse(
    name: &Path,
    bytes: &[u8],
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Result<Arc<CrClipDictionary>, String> {
    let payload = crate::state::document_io::parse_payload(
        name,
        DocumentKind::Ycd,
        bytes,
        Arc::clone(crypto),
    )?;
    match payload {
        DocPayload::Resource(or_io::Asset::Clips(dictionary)) => Ok(Arc::new(dictionary)),
        _ => Err(tr!("is not a clip dictionary").to_string()),
    }
}
