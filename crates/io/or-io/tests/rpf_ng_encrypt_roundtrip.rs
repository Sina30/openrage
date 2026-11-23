// End-to-end roundtrip tests for the NG encrypt path.

use std::sync::Arc;

use or_io::container::rpf::crypto::ng::{
    decrypt_block, decrypt_in_place, encrypt_block, encrypt_in_place,
};
use or_io::container::rpf::crypto::ng_encrypt_state::derive_encrypt_state;
use or_io::container::{
    Gta5Keys, Gta5RetailCryptoProvider, RpfArchive, RpfBuilder, RpfCryptoProvider, RpfEncryption,
};

const NG_KEY_LEN: usize = 0x110;

fn synthetic_decrypt_tables() -> Vec<[[u32; 256]; 16]> {
    let mut tables = Vec::with_capacity(17);
    let mut state: u64 = 0xC0DE_C0DE_C0DE_C0DEu64;
    let mut next = || -> u32 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let mixed = state.wrapping_mul(0x2545_F491_4F6C_DD1Du64);
        (mixed >> 32) as u32
    };
    for _round in 0..17u32 {
        let mut group = [[0u32; 256]; 16];
        for lane in group.iter_mut() {
            for slot in lane.iter_mut() {
                *slot = next();
            }
        }
        tables.push(group);
    }
    tables
}

fn synthetic_ng_keys() -> Vec<[u8; NG_KEY_LEN]> {
    let mut keys = Vec::with_capacity(101);
    let mut state: u64 = 0xCAFE_F00D_CAFE_F00Du64;
    for k in 0..101u8 {
        let mut key = [0u8; NG_KEY_LEN];
        for (i, byte) in key.iter_mut().enumerate() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = ((state >> 8) as u8) ^ k ^ (i as u8);
        }
        keys.push(key);
    }
    keys
}

fn synthetic_keys() -> Gta5Keys {
    let mut k = Gta5Keys::empty();
    k.aes_key = [0xA5u8; 32];
    for (i, b) in k.lut.iter_mut().enumerate() {
        *b = i as u8;
    }
    k.ng_keys = synthetic_ng_keys();
    k.ng_decrypt_tables = synthetic_decrypt_tables();
    k
}

#[test]
#[ignore]
fn ng_encrypt_decrypt_block_roundtrip() {
    let keys = synthetic_keys();
    let state =
        derive_encrypt_state(&keys.ng_decrypt_tables, &|_| {}).expect("derive encrypt state");
    let key = keys.ng_keys[42];

    for seed in 0u64..32 {
        let mut block = [0u8; 16];
        for (i, b) in block.iter_mut().enumerate() {
            *b = ((seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (i % 56)) & 0xFF) as u8;
        }
        let original = block;
        encrypt_block(&mut block, &key, &state).expect("encrypt");
        assert_ne!(block, original, "encrypt must change the block");
        decrypt_block(&mut block, &key, &keys.ng_decrypt_tables);
        assert_eq!(
            block, original,
            "decrypt(encrypt(block)) must equal block for seed={}",
            seed
        );
    }
}

#[test]
#[ignore]
fn ng_encrypt_decrypt_buffer_roundtrip() {
    let keys = synthetic_keys();
    let state =
        derive_encrypt_state(&keys.ng_decrypt_tables, &|_| {}).expect("derive encrypt state");
    let key = keys.ng_keys[7];

    let original: Vec<u8> = (0..91u8).collect();
    let mut buf = original.clone();
    encrypt_in_place(&mut buf, &key, &state).expect("encrypt_in_place");
    assert_ne!(
        buf[..80],
        original[..80],
        "encrypt must change the leading complete blocks"
    );
    assert_eq!(
        buf[80..],
        original[80..],
        "trailing partial block must be unchanged"
    );

    decrypt_in_place(&mut buf, &key, &keys.ng_decrypt_tables);
    assert_eq!(
        buf, original,
        "decrypt_in_place(encrypt_in_place(buf)) must equal buf"
    );
}

#[test]
#[ignore]
fn ng_archive_roundtrips_through_retail_provider() {
    let keys = Arc::new(synthetic_keys());
    let provider: Arc<dyn RpfCryptoProvider> =
        Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys)));

    let bytes = RpfBuilder::default()
        .encrypted(RpfEncryption::Ng, Arc::clone(&provider))
        .insert_with(
            "data/action/branches.meta",
            b"<?xml version=\"1.0\"?>".repeat(64).to_vec(),
            true,
        )
        .unwrap()
        .insert_with(
            "scripts/test.ysc",
            {
                let mut v = b"RSC7".to_vec();
                v.extend_from_slice(&[0u8; 12]);
                v.extend_from_slice(&b"opaque-script-payload".repeat(8));
                v
            },
            false,
        )
        .unwrap()
        .to_bytes()
        .expect("save ng");

    // Header magic intact, encryption tag is the NG key id.
    assert_eq!(
        u32::from_le_bytes(bytes[..4].try_into().unwrap()),
        or_io::container::rpf::RPF7_MAGIC
    );
    assert_eq!(
        u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        or_io::container::rpf::RPF_NG_MAGIC
    );

    let archive = RpfArchive::from_bytes_with(Some("synthetic.rpf"), bytes, provider)
        .expect("read NG archive");
    assert_eq!(archive.file_count(), 2);

    let extracted = archive.read("data/action/branches.meta").expect("extract");
    assert_eq!(
        extracted,
        b"<?xml version=\"1.0\"?>".repeat(64),
        "NG archive round-trip body mismatch"
    );
}
