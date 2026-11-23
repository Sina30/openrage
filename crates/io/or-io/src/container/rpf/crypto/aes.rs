use ::aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use ::aes::Aes256;

pub fn decrypt_in_place(key: &[u8; 32], buffer: &mut [u8], rounds: usize) {
    let cipher = Aes256::new(key.into());
    let aligned = buffer.len() & !0xF;
    for _ in 0..rounds {
        for block in buffer[..aligned].as_chunks_mut::<16>().0.iter_mut() {
            let block_arr: &mut [u8; 16] = block;
            cipher.decrypt_block(block_arr.into());
        }
    }
}

pub fn encrypt_in_place(key: &[u8; 32], buffer: &mut [u8], rounds: usize) {
    let cipher = Aes256::new(key.into());
    let aligned = buffer.len() & !0xF;
    for _ in 0..rounds {
        for block in buffer[..aligned].as_chunks_mut::<16>().0.iter_mut() {
            let block_arr: &mut [u8; 16] = block;
            cipher.encrypt_block(block_arr.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_arbitrary_buffer() {
        let key = [0x42u8; 32];
        let original: Vec<u8> = (0..67u8).collect();
        let mut buf = original.clone();
        encrypt_in_place(&key, &mut buf, 1);
        assert_eq!(&buf[64..], &original[64..]);
        assert_ne!(&buf[..64], &original[..64]);
        decrypt_in_place(&key, &mut buf, 1);
        assert_eq!(buf, original);
    }

    #[test]
    fn known_vector_zero_block() {
        let key = [0u8; 32];
        let mut buf = [0u8; 16];
        encrypt_in_place(&key, &mut buf, 1);
        let expected: [u8; 16] = [
            0xDC, 0x95, 0xC0, 0x78, 0xA2, 0x40, 0x89, 0x89, 0xAD, 0x48, 0xA2, 0x14, 0x92, 0x84,
            0x20, 0x87,
        ];
        assert_eq!(buf, expected);
    }

    #[test]
    fn empty_buffer_is_noop() {
        let key = [0u8; 32];
        let mut buf: [u8; 0] = [];
        encrypt_in_place(&key, &mut buf, 1);
        decrypt_in_place(&key, &mut buf, 1);
    }
}
