//! A first entry of raw DEFLATE streams, one per entry, and the streams a block is cut into.

use flate2::write::DeflateEncoder;

use super::*;

fn raw(bytes: &[u8]) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(bytes).expect("deflated");
    enc.finish().expect("finished")
}

/// An RBF document of `len` bytes: `RBF0`, then bytes a scan reads as no other head.
fn rbf(len: usize) -> Vec<u8> {
    let mut body = noise(len, 17);
    body[..4].copy_from_slice(b"RBF0");
    body
}

/// A top archive, and a nested one of `entries` entries whose first entry is `documents` as raw
/// DEFLATE streams, each padded to a slot; its names after them and the next nested archive on
/// the page after those.
fn raw_entries(entries: u32, documents: &[Vec<u8>]) -> Layout {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(entries, TABLE, 2);
    let nested = l.finds[1].offset;
    for document in documents {
        let stored = raw(document);
        let content = classify(document, document.len() as u64);
        let find = Rpf9ScanFind {
            offset: l.at(),
            kind: Rpf9BodyKind::RawDeflate { content },
            span: (stored.len() as u64).next_multiple_of(RPF9_SLOT),
            stored_len: Some(stored.len() as u64),
            inflated_len: Some(document.len() as u64),
            container: None,
        };
        l.claim(find, &stored);
    }
    l.names(NAMES as usize, nested);
    l.page();
    l.archive(2, TABLE, 3);
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    l
}

#[test]
fn raw_deflate_streams_as_many_as_the_entries_are_listed_one_each_and_read_back() {
    let documents = [rbf(0x900), pso(0x640)];
    let l = raw_entries(2, &documents);
    assert_eq!(scan_all(&l.bytes).0, l.finds);
    assert_eq!(scan_in_slices(&l.bytes, 0x800).0, l.finds);
    let is_raw = |f: &&Rpf9ScanFind| matches!(f.kind, Rpf9BodyKind::RawDeflate { .. });
    for (find, document) in l.finds.iter().filter(is_raw).zip(&documents) {
        let read = read_rpf9_body(Cursor::new(&l.bytes), find.offset, u64::MAX);
        let (again, bytes) = read.expect("read");
        assert_eq!(&again, find);
        assert_eq!(&bytes, document);
        let extracted = extract_rpf9_body(Cursor::new(&l.bytes), find.offset, u64::MAX);
        assert_eq!(&extracted.expect("extracted").1, document);
        let limited = read_rpf9_body(Cursor::new(&l.bytes), find.offset, 0x100);
        assert!(matches!(limited, Err(Rpf9ScanError::InflateLimit { .. })));
    }
}

#[test]
fn raw_deflate_streams_fewer_than_the_entries_stay_one_first_entry() {
    let l = raw_entries(3, &[rbf(0x900), pso(0x640)]);
    let finds = scan_all(&l.bytes).0;
    let first = finds.iter().find(|f| f.offset == l.finds[2].offset);
    assert!(matches!(
        first.map(|f| f.kind),
        Some(Rpf9BodyKind::FirstEntry { .. })
    ));
    let listed = |f: &Rpf9ScanFind| matches!(f.kind, Rpf9BodyKind::RawDeflate { .. });
    assert!(!finds.iter().any(listed));
}

#[test]
fn a_raw_deflate_stream_of_no_metadata_is_no_entry() {
    let l = raw_entries(1, &[xml(0x900)]);
    let finds = scan_all(&l.bytes).0;
    let listed = |f: &Rpf9ScanFind| matches!(f.kind, Rpf9BodyKind::RawDeflate { .. });
    assert!(!finds.iter().any(listed));
    let at = l.finds[2].offset;
    assert!(read_rpf9_body(Cursor::new(&l.bytes), at, u64::MAX).is_err());
}

#[test]
fn a_block_s_streams_are_what_each_inflates_to_in_order() {
    let (head, tail) = (noise(0x2_0000, 23), noise(0x1234, 29));
    let mut bytes = vec![0u8; 0x40];
    let offset = bytes.len() as u64;
    bytes.extend(deflate(&head));
    bytes.resize(bytes.len().next_multiple_of(16), 0);
    bytes.extend(deflate(&tail));
    let block = Rpf9Block {
        offset,
        stored_len: bytes.len() as u64 - offset,
        inflated_len: (head.len() + tail.len()) as u64,
        zlib: true,
        physical: false,
    };
    let streams = rpf9_block_streams(Cursor::new(&bytes), &block).expect("walked");
    assert_eq!(streams, [head.len() as u64, tail.len() as u64]);
    let stored = Rpf9Block {
        zlib: false,
        ..block
    };
    let one = rpf9_block_streams(Cursor::new(&bytes), &stored).expect("stored raw");
    assert_eq!(one, [block.inflated_len]);
    let mut broken = bytes.clone();
    broken[offset as usize + 2..offset as usize + 0x40].fill(0xFF);
    let refused = rpf9_block_streams(Cursor::new(&broken), &block);
    assert!(matches!(
        refused,
        Err(Rpf9ScanError::ResourceRefused { at, .. }) if at == offset
    ));
}
