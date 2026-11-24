//! Bodies cut short, changed byte by byte and changed at random, for the tests that a reader refuses bad bytes and never panics.
//!
//! ⚠ The test profile builds with Cranelift, which cannot unwind: a reader that panics aborts the run, at its own line.

/// The values each byte is set to in turn: the ends of a byte and of a signed byte, and one.
const VALUES: [u8; 5] = [0x00, 0x01, 0x7F, 0x80, 0xFF];
/// The bodies changed at random after the byte-by-byte pass, shared among a model's blocks.
const ROUNDS: usize = 2_000;

/// Hand `read` each prefix of `whole`, then `whole` with each byte set to each of [`VALUES`], then
/// with a few bytes at a time set at random.
pub(crate) fn every_change(whole: &[u8], read: impl FnMut(&[u8])) {
    changes(whole, ROUNDS, read);
}

fn changes(whole: &[u8], rounds: usize, mut read: impl FnMut(&[u8])) {
    for len in 0..whole.len() {
        read(&whole[..len]);
    }
    let mut bytes = whole.to_vec();
    for at in 0..whole.len() {
        for v in VALUES {
            bytes[at] = v;
            read(&bytes);
        }
        bytes[at] = whole[at];
    }
    if whole.is_empty() {
        return;
    }
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..rounds {
        bytes.copy_from_slice(whole);
        for _ in 0..1 + next() % 4 {
            let at = (next() % whole.len() as u64) as usize;
            bytes[at] = next() as u8;
        }
        read(&bytes);
    }
}

/// [`every_change`] over each data block of `whole`, one block at a time.
#[cfg(feature = "meta")]
pub(crate) fn every_block_change(
    whole: &crate::meta::binary::rbm::schema::RbmModel,
    mut read: impl FnMut(&crate::meta::binary::rbm::schema::RbmModel),
) {
    let rounds = ROUNDS / whole.data_blocks.len().max(1);
    for b in 0..whole.data_blocks.len() {
        let mut model = whole.clone();
        changes(&whole.data_blocks[b].data, rounds, |bytes| {
            model.data_blocks[b].data.clear();
            model.data_blocks[b].data.extend_from_slice(bytes);
            read(&model);
        });
    }
}
