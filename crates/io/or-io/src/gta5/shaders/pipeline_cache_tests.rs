use super::*;

fn le(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A blob of `len` bytes with `words` at their offsets.
fn blob(len: usize, words: &[(usize, u32)]) -> Vec<u8> {
    let mut b = vec![0x5Au8; len];
    for (at, w) in words {
        b[*at..at + 4].copy_from_slice(&w.to_le_bytes());
    }
    b
}

/// One blob per metadata role, each holding to its size rule: a graphics layout, state and slot,
/// then a compute layout, state and slot.
fn metadata() -> Vec<Vec<u8>> {
    let mut state = blob(0x78, &[(4, 0x1000), (0x4C, 1), (0x50, 9)]);
    state[0x54..0x5D].copy_from_slice(b"POSITION\0");
    vec![
        blob(48, &[(0x28, 0x0004_0010)]),
        state,
        blob(80, &[(0, 2), (4, 80 - 12), (0x0C, 0x0001_0440)]),
        blob(28, &[]),
        blob(20, &[]),
        blob(88, &[(0, 0x403), (4, 88 - 20), (0x14, 0x0001_0440)]),
    ]
}

/// A body of one graphics pipeline (layout 0, state 1, slot 0 with code 0 and metadata 2) and one
/// compute pipeline (layout 3, state 4, code 1 and metadata 5).
fn body(meta: &[Vec<u8>], compute_layout: u32) -> Vec<u8> {
    let code = [vec![0x11u8; 64], vec![0x22u8; 32]];
    let mut b = b"PS\x00\x09".to_vec();
    b.resize(HEADER, 0);
    b[0x04..0x09].copy_from_slice(b"ident");
    b[0x24..0x29].copy_from_slice(b"1.2.3");
    b[0x48..0x50].copy_from_slice(&0xABCDu64.to_le_bytes());
    b[0x50..0x60].copy_from_slice(&le(&[1, 1, meta.len() as u32, 2]));
    b[0x60..0x68].copy_from_slice(&le(&[0x0102, 0x0304]));
    let mut g = 1u64.to_le_bytes().to_vec();
    g.extend(le(&[0, 1]));
    g.resize(0x38, 0x33);
    g.extend(le(&[0, 2]));
    g.extend(le(&[NONE; 12]));
    b.extend(g);
    b.extend(2u64.to_le_bytes());
    b.extend(le(&[compute_layout, 4, 1, 5]));
    let mut offset = 0;
    for m in meta {
        b.extend(le(&[offset, m.len() as u32]));
        offset += m.len() as u32;
    }
    b.extend(le(&[0, 64, 256, 32]));
    meta.iter().for_each(|m| b.extend_from_slice(m));
    b.extend_from_slice(&code[0]);
    b.resize(b.len() + 256 - 64, 0);
    b.extend_from_slice(&code[1]);
    b.resize(b.len() + 24, 0);
    b
}

#[test]
fn a_cache_states_its_header_its_records_and_its_blobs_by_role() {
    let cache = PipelineCache::open(&body(&metadata(), 3)).expect("read");
    assert_eq!(
        (cache.version, cache.ident.as_str(), cache.build.as_str()),
        (9, "ident", "1.2.3")
    );
    assert_eq!((cache.key, cache.stamp), (0xABCD, 0x0102_0000_0304));
    assert_eq!((cache.graphics.len(), cache.compute.len()), (1, 1));
    assert_eq!(
        (cache.metadata_blobs, cache.code_blobs, cache.tail_len),
        (6, 2, 24)
    );
    assert_eq!(cache.graphics[0].pattern(), "x------");
    assert_eq!(
        cache.graphics[0].slots[0],
        Some(Slot {
            code: 0,
            metadata: Some(2)
        })
    );
    assert_eq!(
        cache.compute[0].slot,
        Some(Slot {
            code: 1,
            metadata: Some(5)
        })
    );
    assert_eq!(cache.sections[1], ("compute", HEADER + GRAPHICS));
    assert_eq!(cache.roles.values().sum::<usize>(), 6);
    assert_eq!(cache.roles.get("graphics slot 0"), Some(&1));
}

#[test]
fn a_blob_that_breaks_its_role_s_size_rule_is_refused() {
    let mut meta = metadata();
    meta[1].extend_from_slice(&[0; 4]);
    assert!(matches!(
        PipelineCache::open(&body(&meta, 3)),
        Err(PipelineCacheError::Invalid { .. })
    ));
}

#[test]
fn a_blob_in_two_roles_or_in_none_is_refused() {
    assert!(matches!(
        PipelineCache::open(&body(&metadata(), 0)),
        Err(PipelineCacheError::Invalid { .. })
    ));
    let mut meta = metadata();
    meta.push(blob(28, &[]));
    assert!(matches!(
        PipelineCache::open(&body(&meta, 3)),
        Err(PipelineCacheError::Invalid { .. })
    ));
}

#[test]
fn counts_past_the_body_and_a_cut_body_are_refused_without_a_panic() {
    let whole = body(&metadata(), 3);
    let mut huge = whole.clone();
    huge[0x50..0x54].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(PipelineCache::open(&huge).is_err());
    for len in 0..whole.len() - 24 {
        assert!(PipelineCache::open(&whole[..len]).is_err(), "cut at {len}");
    }
}

#[test]
fn code_metadata_and_saving_are_refused_by_name() {
    let cache = PipelineCache::open(&body(&metadata(), 3)).expect("read");
    assert_eq!(cache.code(0), Err(PipelineCacheError::NativeCode));
    assert_eq!(
        cache.metadata(0),
        Err(PipelineCacheError::MetadataNotDecoded)
    );
    assert_eq!(cache.save(), Err(PipelineCacheError::ReadOnly));
}
