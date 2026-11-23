use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::*;

const START: u64 = 0x2000;
const LEN: usize = 0x2200;
/// Body 0 at a page, body 1 behind a zero slot with a second piece, body 2 unpaged, then names.
const BODY0: (usize, usize) = (0x2000, 0x300);
const BODY1: (usize, usize) = (0x3010, 900);
const SECOND: usize = 0x33A5;
const HEAD: usize = 0x33B0;
const BODY2: (usize, usize) = (0x3800, 600);
const NAMES: (usize, usize) = (0x4000, 0x140);

/// Bytes of no structure that hold no zero byte and no zlib head.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            match (x >> 24) as u8 {
                0 => 1,
                0x78 => 0x79,
                b => b,
            }
        })
        .collect()
}

/// A whole zlib stream with no run of 16 zero bytes.
fn stream() -> Vec<u8> {
    let data: Vec<u8> = (0..2000u32).map(|i| (i * 31 % 253 + 1) as u8).collect();
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&data).expect("a write to memory");
    let s = z.finish().expect("a stream");
    assert!(!s.windows(16).any(|w| w.iter().all(|b| *b == 0)));
    assert!(s.len() < 0x180, "{}", s.len());
    s
}

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    let at = at - START as usize;
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

/// The span, with `second` as body 1's stream after 11 bytes of noise.
fn span(second: &[u8]) -> Vec<u8> {
    let mut v = vec![0u8; LEN];
    for (seed, (at, len)) in [(1, BODY0), (2, BODY1), (3, BODY2), (4, NAMES)] {
        put(&mut v, at, &noise(seed, len));
    }
    put(&mut v, SECOND, &noise(5, HEAD - SECOND));
    put(&mut v, HEAD, second);
    v
}

/// Body 1's stream cut short, so it stops before its end.
fn cut() -> Vec<u8> {
    let s = stream();
    s[..s.len() / 2].to_vec()
}

fn read(v: &[u8]) -> Result<OpaqueSpan, NotOpaque> {
    opaque_bodies(v, START, Some(NAMES.1 as u64))
}

fn rule(v: &[u8]) -> &'static str {
    read(v).expect_err("refused").rule
}

#[test]
fn a_span_splits_into_bodies_and_pieces_placed_against_the_page() {
    let second = cut();
    // A trailing zero byte of the stream belongs to the zero run after it.
    let kept = second.len() - second.iter().rev().take_while(|b| **b == 0).count();
    let bytes = span(&second);
    let read = read(&bytes).expect("opaque bodies");
    let first = BODY1.0 - START as usize;
    let piece = |offset: usize, len: usize, zlib_head: Option<usize>| Piece {
        offset: offset as u64,
        len: len as u64,
        zlib_head: zlib_head.map(|h| h as u64),
    };
    let expected = [
        OpaqueBody {
            offset: BODY0.0 as u64,
            placement: Placement::Page,
            pieces: vec![piece(BODY0.0, BODY0.1, None)],
            coder: None,
        },
        OpaqueBody {
            offset: BODY1.0 as u64,
            placement: Placement::AfterZeroSlot,
            pieces: vec![
                piece(BODY1.0, BODY1.1, None),
                piece(SECOND, HEAD + kept - SECOND, Some(HEAD)),
            ],
            coder: Some(CoderEntry {
                tag: u16::from_le_bytes([bytes[first], bytes[first + 1]]),
            }),
        },
        OpaqueBody {
            offset: BODY2.0 as u64,
            placement: Placement::Unpaged,
            pieces: vec![piece(BODY2.0, BODY2.1, None)],
            coder: None,
        },
    ];
    assert_eq!(read.bodies, expected);
    assert_eq!(read.names, Some(NAMES.0 as u64));
    let other = opaque_bodies(&span(&second), START, Some(NAMES.1 as u64 + 16)).expect("bodies");
    assert_eq!(other.bodies.len(), 4);
    assert_eq!(other.names, None);
}

#[test]
fn only_a_body_behind_a_zero_slot_is_a_coder_entry_named_by_its_tag() {
    let mut v = span(&cut());
    // The same two bytes open all three bodies: where a body is placed names it, not its bytes.
    for at in [BODY0.0, BODY1.0, BODY2.0] {
        put(&mut v, at, &[0x34, 0x12]);
    }
    let read = read(&v).expect("opaque bodies");
    let coder: Vec<Option<CoderEntry>> = read.bodies.iter().map(|b| b.coder).collect();
    assert_eq!(coder, [None, Some(CoderEntry { tag: 0x1234 }), None]);
    assert_eq!(read.bodies[1].pieces.len(), 2);
}

#[test]
fn each_broken_rule_refuses_the_body_by_its_name() {
    let mut v = span(&cut());
    put(&mut v, BODY0.0, &[0x78, 0xDA]);
    assert_eq!(rule(&v), "O1");

    let mut v = span(&cut());
    put(&mut v, BODY1.0, &[0x41; BODY1.1]);
    assert_eq!(rule(&v), "O2");

    let mut v = span(&cut());
    for at in [0x10, 0x60, 0x68] {
        put(&mut v, BODY2.0 + at, &0x5000_1000u64.to_le_bytes());
    }
    assert_eq!(rule(&v), "O3");

    let mut v = span(&cut());
    put(&mut v, HEAD, &[0x79]);
    assert_eq!(rule(&v), "O4");

    // A byte after the stream keeps a zero last byte of its checksum in the piece.
    assert_eq!(rule(&span(&[stream(), vec![0x5A]].concat())), "O5");

    let mut v = span(&cut());
    put(&mut v, BODY0.0 + 0x100, &stream());
    assert_eq!(rule(&v), "O6");
}

#[test]
fn a_stream_cut_short_past_the_inflate_budget_counts_as_compressed_data() {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
    let block = vec![0x41u8; 1 << 20];
    // Cut at half, the stream holds twice the budget: the read stops at the budget.
    for _ in 0..4 * (MOST_INFLATED >> 20) {
        z.write_all(&block).expect("a write to memory");
    }
    let s = z.finish().expect("a stream");
    let cut = inflate(&s[..s.len() / 2]);
    assert!(
        cut.inflated > MOST_INFLATED && cut.inflated < 2 * MOST_INFLATED,
        "{}",
        cut.inflated
    );
    assert!(cut.ends);
}
