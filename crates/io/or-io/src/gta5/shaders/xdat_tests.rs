use super::*;

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn get32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

const D: usize = BASE + DESCRIPTOR as usize;

/// An `FSSM` package whose one used slot is `slot` and whose code, at `code_offset` from the
/// base, is `words`, then `s_endpgm`, then `s_code_end` to a whole 64-byte line; and its range.
fn package(slot: usize, code_offset: usize, words: &[u32]) -> (Vec<u8>, Range<usize>) {
    let mut code = words.to_vec();
    code.push(S_ENDPGM);
    code.push(S_CODE_END);
    while !code.len().is_multiple_of(16) {
        code.push(S_CODE_END);
    }
    let start = BASE + code_offset;
    let mut m = vec![0u8; start + 4 * code.len() + 16];
    let len = m.len();
    m[..4].copy_from_slice(b"FSSM");
    put32(&mut m, 4, FSSM_VERSION);
    put32(&mut m, 8, (len - 16) as u32);
    put32(&mut m, 0x10, 6);
    for s in 0..SLOTS {
        put64(
            &mut m,
            SLOT_TABLE + 8 * s,
            if s == slot { DESCRIPTOR } else { EMPTY },
        );
    }
    put32(&mut m, PACKAGE_WORD, 0x3_0002);
    put64(&mut m, D, code_offset as u64);
    put64(&mut m, D + 8, 4 * code.len() as u64);
    put32(&mut m, D + 0x14, 0x5A17);
    put32(&mut m, D + 0x18, 0xC);
    put64(&mut m, D + 0x28, (code_offset + 4 * code.len()) as u64);
    put64(&mut m, D + 0x30, 0x77_0000);
    let stage_word = SLOT_STAGE_WORDS.iter().find(|(s, _)| *s == slot).unwrap().1;
    put32(&mut m, D + 0x40, stage_word);
    for (i, w) in code.iter().enumerate() {
        put32(&mut m, start + 4 * i, *w);
    }
    for at in (len - 16..len).step_by(4) {
        put32(&mut m, at, 0xDEAD_BEEF);
    }
    (m, start..start + 4 * code.len())
}

/// A program of DXIL `kind` whose blocks use `slots`, each with code that differs.
fn program(kind: u32, slots: &[usize]) -> XdatProgram {
    let stages = (slots.iter().enumerate())
        .map(|(i, &slot)| {
            let words: Vec<u32> = (0..3 + 17 * i as u32)
                .map(|w| 0x7E00_0000 | (w << 8) | slot as u32)
                .collect();
            let (package, code) = package(slot, 0x300 + 0x40 * i, &words);
            let stage_word = SLOT_STAGE_WORDS.iter().find(|(s, _)| *s == slot).unwrap().1;
            XdatStage {
                slot,
                stage_word,
                package,
                code,
            }
        })
        .collect();
    XdatProgram {
        program_version: (kind << 16) | 0x66,
        digest: std::array::from_fn(|i| 0xA0 + i as u8),
        stages,
        stub_words: 0x1A5,
    }
}

fn rule(part: &[u8]) -> &'static str {
    read(part).unwrap_err().rule
}

/// Where entry `index`'s data starts in a written part.
fn entry_at(part: &[u8], index: usize) -> usize {
    get32(part, PROGRAM_HEADER + XDAT_HEAD + ENTRY * index + 4) as usize
}

#[test]
fn a_part_reads_its_stages_and_saving_it_again_is_a_fixed_point() {
    let cases: [(u32, &[usize]); 6] = [
        (0, &[5]),
        (1, &[4]),
        (1, &[2, 4]),
        (2, &[3]),
        (5, &[11]),
        (13, &[8]),
    ];
    for (kind, slots) in cases {
        let held = program(kind, slots);
        let part = held.write().unwrap();
        let len = part.len();
        assert_eq!(get32(&part, 4) as usize * 4, len);
        assert_eq!(&part[PROGRAM_HEADER..PROGRAM_HEADER + 4], b"XDAT");
        assert_eq!(get32(&part, PROGRAM_HEADER + 4) as usize, slots.len() + 2);
        assert!(part.ends_with(NOBC));
        let block = entry_at(&part, 1);
        assert_eq!(
            part[block + BLOCK_HEAD..block + BLOCK_HEAD + 2],
            [0x78u8, 0x9C]
        );

        let read = read(&part).unwrap();
        assert_eq!(read, held);
        assert_eq!(read.write().unwrap(), part);
        for stage in &read.stages {
            let code = &stage.package[stage.code.clone()];
            assert!(code.ends_with(&S_CODE_END.to_le_bytes()));
            assert_eq!(code.len() % CODE_ALIGN, 0);
        }
    }
}

#[test]
fn a_part_cut_short_is_refused_and_never_panics() {
    let part = program(1, &[2, 4]).write().unwrap();
    for cut in 0..part.len() {
        assert!(read(&part[..cut]).is_err(), "cut at {cut}");
    }
}

/// A rule and an edit that breaks it.
type Break = (&'static str, fn(&mut Vec<u8>));

#[test]
fn each_part_level_break_is_refused_by_its_rule() {
    let good = program(0, &[5]).write().unwrap();
    let edits: [Break; 13] = [
        ("P1", |p| {
            let words = get32(p, 4) + 1;
            put32(p, 4, words);
        }),
        ("P1", |p| put32(p, 12, 0x107)),
        ("P2", |p| {
            let last = p.len() - 1;
            p[last] = b'D';
        }),
        ("X1", |p| p[PROGRAM_HEADER + 3] = b'X'),
        ("X1", |p| put32(p, PROGRAM_HEADER + 4, 5)),
        ("X1", |p| put32(p, PROGRAM_HEADER + 8, 0x30)),
        ("X2", |p| put32(p, PROGRAM_HEADER + XDAT_HEAD + 4, 0x4C)),
        ("X3", |p| put32(p, PROGRAM_HEADER + XDAT_HEAD, 0x13)),
        ("B1", |p| {
            let block = entry_at(p, 1);
            put32(p, block + 4, 4);
        }),
        ("B2", |p| {
            let block = entry_at(p, 1);
            let size = u64::from(get32(p, block + 8)) + 1;
            put64(p, block + 8, size);
        }),
        ("B3", |p| {
            let block = entry_at(p, 1);
            let raw = u64::from(get32(p, block + 16)) + 1;
            put64(p, block + 16, raw);
        }),
        ("B3", |p| {
            let block = entry_at(p, 1);
            put64(p, block + 16, u64::MAX);
        }),
        ("S1", |p| {
            let stub = p.len() - STUB_SIZE;
            let version = get32(p, stub) ^ 0x1_0000;
            put32(p, stub, version);
        }),
    ];
    for (want, edit) in edits {
        let mut part = good.clone();
        edit(&mut part);
        assert_eq!(rule(&part), want);
    }
    // A stream whose bytes are altered fails its own check.
    let mut part = good;
    let block = entry_at(&part, 1);
    part[block + BLOCK_HEAD + 6] ^= 0x55;
    assert_eq!(rule(&part), "B3");
    // A stage the program's kind does not run, or the two vertex blocks in the other order.
    assert_eq!(rule(&program(1, &[5]).write().unwrap()), "F9");
    assert_eq!(rule(&program(1, &[4, 2]).write().unwrap()), "F9");
    assert_eq!(rule(&program(9, &[5]).write().unwrap()), "F9");
}

#[test]
fn each_package_break_is_refused_by_its_rule() {
    let edits: [Break; 15] = [
        ("F1", |m| put32(m, 4, 0x102)),
        ("F1", |m| m.truncate(MIN_PACKAGE - 1)),
        ("F2", |m| {
            let size = get32(m, 8) + 1;
            put32(m, 8, size);
        }),
        ("F3", |m| put32(m, 0x10, 7)),
        ("F4", |m| put64(m, SLOT_TABLE, DESCRIPTOR)),
        ("F4", |m| put64(m, SLOT_TABLE + 8 * 5, DESCRIPTOR + 8)),
        ("F4", |m| put64(m, SLOT_TABLE + 8 * 5, EMPTY)),
        ("F10", |m| put32(m, PACKAGE_WORD, 0)),
        ("F6", |m| put64(m, D + 8, 1 << 40)),
        ("F6", |m| put64(m, D + 8, 0)),
        ("F7", |m| {
            let size = u64::from(get32(m, D + 8));
            put64(m, D + 8, size - 4);
            put64(m, D + 0x28, EMPTY);
        }),
        ("F8", |m| put64(m, D + 0x28, 0x1234)),
        ("F11", |m| put32(m, D + 0x10, 1)),
        ("F11", |m| put32(m, D + 0x18, 8)),
        ("F13", |m| put32(m, D + 0x40, 0x2_0004)),
    ];
    for (want, edit) in edits {
        let mut held = program(0, &[5]);
        edit(&mut held.stages[0].package);
        assert_eq!(rule(&held.write().unwrap()), want);
    }
    // Code with no `s_endpgm` in it.
    let mut held = program(0, &[5]);
    let code = held.stages[0].code.clone();
    let words = held.stages[0].package[code].to_vec();
    let at = (words.as_chunks::<4>().0.iter()).position(|w| *w == S_ENDPGM.to_le_bytes());
    let at = held.stages[0].code.start + 4 * at.unwrap();
    put32(&mut held.stages[0].package, at, S_CODE_END);
    assert_eq!(rule(&held.write().unwrap()), "F7");
}
