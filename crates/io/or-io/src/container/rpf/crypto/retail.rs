use std::sync::Arc;

use crate::container::rpf::header::RpfEncryption;
use crate::error::{IoError, Result};

use super::aes;
use super::keys::Gta5Keys;
use super::ng;
use super::provider::{CryptoTarget, RpfCryptoProvider, TocDecryptOutcome};

#[derive(Clone)]
pub struct Gta5RetailCryptoProvider {
    keys: Arc<Gta5Keys>,
}

impl Gta5RetailCryptoProvider {
    pub fn new(keys: Arc<Gta5Keys>) -> Self {
        Self { keys }
    }

    pub fn keys(&self) -> &Gta5Keys {
        &self.keys
    }
}

impl RpfCryptoProvider for Gta5RetailCryptoProvider {
    fn supports(&self, encryption: RpfEncryption) -> bool {
        matches!(
            encryption,
            RpfEncryption::None
                | RpfEncryption::Open
                | RpfEncryption::Aes
                | RpfEncryption::Ng
                | RpfEncryption::Other(_)
        )
    }

    fn decrypt(
        &self,
        encryption: RpfEncryption,
        target: CryptoTarget<'_>,
        buffer: &mut [u8],
    ) -> Result<()> {
        match encryption {
            RpfEncryption::None | RpfEncryption::Open => Ok(()),
            RpfEncryption::Aes => {
                aes::decrypt_in_place(&self.keys.aes_key, buffer, 1);
                Ok(())
            }
            RpfEncryption::Ng | RpfEncryption::Other(_) => {
                if !self.keys.has_ng() {
                    return Err(IoError::RpfCryptoFailed {
                        mode: "NG",
                        message: format!(
                            "this archive is NG-encrypted and no NG keys are loaded. {}",
                            super::extractor::WITHOUT_A_SNAPSHOT
                        ),
                    });
                }
                let key =
                    pick_ng_key(&self.keys, &target).ok_or_else(|| IoError::RpfCryptoFailed {
                        mode: "NG",
                        message: "could not derive NG key (no LUT or no name context)".into(),
                    })?;
                ng::decrypt_in_place(buffer, key, &self.keys.ng_decrypt_tables);
                Ok(())
            }
            // Another title's cipher, whose keys are not GTA V's.
            RpfEncryption::Tagged(key_id) => Err(IoError::RpfEncryptionUnsupported { key_id }),
        }
    }

    fn decrypt_toc(
        &self,
        encryption: RpfEncryption,
        target: CryptoTarget<'_>,
        buffer: &mut [u8],
        validate_first_block: &dyn Fn(&[u8; 16]) -> bool,
    ) -> Result<TocDecryptOutcome> {
        match encryption {
            // AES uses one global key, so a renamed archive needs no recovery.
            RpfEncryption::None | RpfEncryption::Open | RpfEncryption::Aes => {
                self.decrypt(encryption, target, buffer)?;
                Ok(TocDecryptOutcome::Nominal)
            }
            RpfEncryption::Ng | RpfEncryption::Other(_) => {
                if buffer.len() < 16 {
                    // Not even one NG block, so nothing to probe.
                    self.decrypt(encryption, target, buffer)?;
                    return Ok(TocDecryptOutcome::Nominal);
                }
                if !self.keys.has_ng() {
                    return Err(IoError::RpfCryptoFailed {
                        mode: "NG",
                        message: format!(
                            "this archive is NG-encrypted and no NG keys are loaded. {}",
                            super::extractor::WITHOUT_A_SNAPSHOT
                        ),
                    });
                }

                // The probe is the first 16 bytes: one NG block is one
                // RpfEntry, the root directory.
                let mut probe = [0u8; 16];
                probe.copy_from_slice(&buffer[..16]);

                if let Some(key) = pick_ng_key(&self.keys, &target) {
                    let mut probe_nominal = probe;
                    ng::decrypt_block(&mut probe_nominal, key, &self.keys.ng_decrypt_tables);
                    if validate_first_block(&probe_nominal) {
                        ng::decrypt_in_place(buffer, key, &self.keys.ng_decrypt_tables);
                        return Ok(TocDecryptOutcome::Nominal);
                    }
                }

                // The name did not pick a working key (a renamed archive,
                // say): try every NG key index against the probe.
                for idx in 0..self.keys.ng_key_count() {
                    let Some(key) = self.keys.ng_key_at(idx) else {
                        continue;
                    };
                    let mut probe_recover = probe;
                    ng::decrypt_block(&mut probe_recover, key, &self.keys.ng_decrypt_tables);
                    if validate_first_block(&probe_recover) {
                        ng::decrypt_in_place(buffer, key, &self.keys.ng_decrypt_tables);
                        return Ok(TocDecryptOutcome::RecoveredNg { ng_key_index: idx });
                    }
                }

                Err(IoError::RpfCryptoFailed {
                    mode: "NG",
                    message: "no NG key produced a valid root-directory entry; the archive \
                              is either corrupted, uses a non-retail key, or relies on a \
                              filename we don't have keys for"
                        .into(),
                })
            }
            RpfEncryption::Tagged(key_id) => Err(IoError::RpfEncryptionUnsupported { key_id }),
        }
    }

    fn decrypt_names(
        &self,
        encryption: RpfEncryption,
        target: CryptoTarget<'_>,
        buffer: &mut [u8],
        toc_outcome: TocDecryptOutcome,
    ) -> Result<()> {
        match (encryption, toc_outcome) {
            (RpfEncryption::Ng, TocDecryptOutcome::RecoveredNg { ng_key_index })
            | (RpfEncryption::Other(_), TocDecryptOutcome::RecoveredNg { ng_key_index }) => {
                let key =
                    self.keys
                        .ng_key_at(ng_key_index)
                        .ok_or_else(|| IoError::RpfCryptoFailed {
                            mode: "NG",
                            message: format!(
                                "recovered NG key index {ng_key_index} is out of range"
                            ),
                        })?;
                ng::decrypt_in_place(buffer, key, &self.keys.ng_decrypt_tables);
                Ok(())
            }
            _ => self.decrypt(encryption, target, buffer),
        }
    }

    fn encrypt(
        &self,
        encryption: RpfEncryption,
        target: CryptoTarget<'_>,
        buffer: &mut [u8],
    ) -> Result<()> {
        match encryption {
            RpfEncryption::None | RpfEncryption::Open => Ok(()),
            RpfEncryption::Aes => {
                aes::encrypt_in_place(&self.keys.aes_key, buffer, 1);
                Ok(())
            }
            RpfEncryption::Ng | RpfEncryption::Other(_) => {
                if !self.keys.has_ng() {
                    return Err(IoError::RpfCryptoFailed {
                        mode: "NG",
                        message: format!(
                            "writing an NG-encrypted archive needs NG keys and none are \
                             loaded. {}",
                            super::extractor::WITHOUT_A_SNAPSHOT
                        ),
                    });
                }
                let state = self.keys.ensure_encrypt_state(&|_| {})?;
                let key =
                    pick_ng_key(&self.keys, &target).ok_or_else(|| IoError::RpfCryptoFailed {
                        mode: "NG",
                        message: "could not derive NG key (no LUT or no name context)".into(),
                    })?;
                ng::encrypt_in_place(buffer, key, &state)
            }
            RpfEncryption::Tagged(key_id) => Err(IoError::RpfEncryptionUnsupported { key_id }),
        }
    }
}

fn pick_ng_key<'a>(
    keys: &'a Gta5Keys,
    target: &CryptoTarget<'_>,
) -> Option<&'a [u8; super::keys::NG_KEY_LEN]> {
    match target {
        CryptoTarget::Toc {
            archive_name,
            archive_size,
        } => keys.ng_key_for(archive_name.unwrap_or(""), *archive_size),
        CryptoTarget::Body {
            entry_name,
            keyed_size,
            ..
        } => keys.ng_key_for(entry_name, *keyed_size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::rpf::header::RpfEncryption;

    fn empty_keys_arc() -> Arc<Gta5Keys> {
        let mut keys = Gta5Keys::empty();
        for (i, b) in keys.lut.iter_mut().enumerate() {
            *b = i as u8;
        }
        // 101 zero NG keys and 17 zero decrypt-table rounds, so `has_ng` holds.
        keys.ng_keys = vec![[0u8; super::super::keys::NG_KEY_LEN]; 101];
        keys.ng_decrypt_tables = vec![[[0u32; 256]; 16]; 17];
        Arc::new(keys)
    }

    #[test]
    fn retail_supports_all_modes_including_none() {
        let p = Gta5RetailCryptoProvider::new(empty_keys_arc());
        assert!(p.supports(RpfEncryption::None));
        assert!(p.supports(RpfEncryption::Open));
        assert!(p.supports(RpfEncryption::Aes));
        assert!(p.supports(RpfEncryption::Other(0xFEED_BEEF)));
    }

    #[test]
    fn another_titles_cipher_is_refused_and_never_taken_for_ng() {
        let p = Gta5RetailCryptoProvider::new(empty_keys_arc());
        let tagged = RpfEncryption::Tagged(0x0A);
        assert!(!p.supports(tagged));
        let target = CryptoTarget::Toc {
            archive_name: Some("common_0.rpf"),
            archive_size: 4096,
        };
        let mut buf = vec![7u8; 32];
        for outcome in [
            p.decrypt(tagged, target.clone(), &mut buf),
            p.encrypt(tagged, target.clone(), &mut buf),
            p.decrypt_toc(tagged, target, &mut buf, &|_| true)
                .map(|_| ()),
        ] {
            assert!(
                matches!(
                    outcome,
                    Err(IoError::RpfEncryptionUnsupported { key_id: 0x0A })
                ),
                "{outcome:?}"
            );
        }
        assert_eq!(
            buf,
            vec![7u8; 32],
            "a refused cipher leaves the bytes alone"
        );
    }

    #[test]
    fn aes_roundtrips_through_retail_provider() {
        let mut keys = Gta5Keys::empty();
        keys.aes_key = [0x42u8; 32];
        keys.ng_keys = vec![[0u8; super::super::keys::NG_KEY_LEN]; 101];
        keys.ng_decrypt_tables = vec![[[0u32; 256]; 16]; 17];
        for (i, b) in keys.lut.iter_mut().enumerate() {
            *b = i as u8;
        }
        let p = Gta5RetailCryptoProvider::new(Arc::new(keys));
        let original: Vec<u8> = (0..96u8).collect();
        let mut buf = original.clone();
        p.encrypt(
            RpfEncryption::Aes,
            CryptoTarget::Toc {
                archive_name: Some("dlc.rpf"),
                archive_size: 16384,
            },
            &mut buf,
        )
        .unwrap();
        assert_ne!(buf, original);
        p.decrypt(
            RpfEncryption::Aes,
            CryptoTarget::Toc {
                archive_name: Some("dlc.rpf"),
                archive_size: 16384,
            },
            &mut buf,
        )
        .unwrap();
        assert_eq!(buf, original);
    }

    #[test]
    fn ng_encrypt_without_ng_keys_returns_clear_error() {
        let mut keys = Gta5Keys::empty();
        keys.aes_key = [0x42u8; 32];
        for (i, b) in keys.lut.iter_mut().enumerate() {
            *b = i as u8;
        }
        let p = Gta5RetailCryptoProvider::new(Arc::new(keys));
        let mut buf = vec![0u8; 16];
        let err = p
            .encrypt(
                RpfEncryption::Other(0x1234_5678),
                CryptoTarget::Toc {
                    archive_name: Some("test.rpf"),
                    archive_size: 4096,
                },
                &mut buf,
            )
            .unwrap_err();
        assert!(matches!(err, IoError::RpfCryptoFailed { mode: "NG", .. }));
    }
}
