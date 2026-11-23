#[cfg(feature = "crypto")]
pub mod aes;
#[cfg(feature = "crypto")]
pub mod cache;
#[cfg(feature = "crypto")]
pub mod dotnet_random;
#[cfg(feature = "crypto")]
pub mod encrypt_cache;
#[cfg(feature = "crypto")]
pub mod extractor;
#[cfg(feature = "crypto")]
pub mod hashes;
#[cfg(feature = "crypto")]
pub mod keys;
#[cfg(feature = "crypto")]
pub mod magic;
#[cfg(feature = "crypto")]
pub mod ng;
#[cfg(feature = "crypto")]
pub mod ng_encrypt_state;
#[cfg(feature = "crypto")]
pub mod provenance;
pub mod provider;
#[cfg(feature = "crypto")]
pub mod rdr2;
#[cfg(feature = "crypto")]
pub mod retail;

#[cfg(feature = "crypto")]
pub use cache::{KeysCache, KeysCacheMetadata, KEYS_CACHE_SCHEMA};
#[cfg(feature = "crypto")]
pub use encrypt_cache::{EncryptCache, ENCRYPT_CACHE_SCHEMA};
#[cfg(feature = "crypto")]
pub use extractor::{
    aes_key_from_executable, extract_keys, extract_keys_from_magic, extract_keys_with,
    extract_keys_with_snapshot, fingerprint_executable, key_coverage, ExeFingerprint,
    ExtractionMode, ExtractionProgress, KeyCoverage, SnapshotSource, SNAPSHOT_ENV,
    USER_SNAPSHOT_FILE_NAME, WITHOUT_A_SNAPSHOT,
};
#[cfg(feature = "crypto")]
pub use keys::{
    EncryptDeriveProgress, Gta5Keys, Gta5NgEncryptState, NgEncryptLut, NgEncryptLutRound,
    NgEncryptTable, NgEncryptTableRound, AWC_KEY_WORDS, GTA5_HASH_LUT_LEN, NG_DECRYPT_TABLE_BYTES,
    NG_KEY_LEN, PC_AES_KEY_LEN,
};
#[cfg(feature = "crypto")]
pub use magic::{decode_magic_blob, decode_magic_file, MAGIC_INFLATED_SIZE};
#[cfg(feature = "crypto")]
pub use provenance::{ExeVerdict, KeyOrigin, KeyProvenance, SnapshotAgreement};
pub use provider::{
    default_provider, CryptoTarget, NoOpCryptoProvider, RpfCryptoProvider, TocDecryptOutcome,
};
#[cfg(feature = "crypto")]
pub use rdr2::{Rdr2CryptoProvider, Rdr2Keys, RDR2_SNAPSHOT_ENV, RDR2_SNAPSHOT_FILE_NAME};
#[cfg(feature = "crypto")]
pub use retail::Gta5RetailCryptoProvider;

/// A range walked on every core with `rayon`, and in order without it.
#[cfg(all(feature = "crypto", feature = "rayon"))]
pub(crate) fn spread<I: rayon::iter::IntoParallelIterator>(items: I) -> I::Iter {
    items.into_par_iter()
}

/// A range walked on every core with `rayon`, and in order without it.
#[cfg(all(feature = "crypto", not(feature = "rayon")))]
pub(crate) fn spread<I: IntoIterator>(items: I) -> I::IntoIter {
    items.into_iter()
}
