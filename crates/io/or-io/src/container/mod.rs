//! RSC7 resources and the RPF and IMG archives that hold them: what a file is made of,
//! before anything asks what it means.

pub mod paging;
pub mod resource;
#[cfg(feature = "archives")]
pub mod rpf;

pub use paging::{DatResource, PgRscBuilder, FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE, NULL_PTR};
pub use resource::{
    dat_compress, dat_decompress, DatResourceFileType, ResourceEnvelope, ResourceType, Rsc7,
    Rsc7Seal,
};
#[cfg(feature = "crypto")]
pub use rpf::{
    extract_keys, fingerprint_executable, EncryptCache, EncryptDeriveProgress, ExtractionProgress,
    Gta5Keys, Gta5NgEncryptState, Gta5RetailCryptoProvider, KeysCache, KeysCacheMetadata,
    ENCRYPT_CACHE_SCHEMA, KEYS_CACHE_SCHEMA,
};
#[cfg(feature = "archives")]
pub use rpf::{
    Compressor, CryptoTarget, NoOpCryptoProvider, RpfArchive, RpfBinaryEntry, RpfBuilder,
    RpfCryptoProvider, RpfDirectoryEntry, RpfEncryption, RpfEntry, RpfEntryKind, RpfHeader,
    RpfNode, RpfResourceEntry, RpfVersion, TocDecryptOutcome,
};

pub use crate::{ErrorMode, Generation, IoError, Result};
